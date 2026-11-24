//! The font resolver of the exports (PDF-034, PDF-035, PDF-037, PDF-038, ADR-0040): which font
//! each of the three roles (text, headings, code) of an export gets, from which entry of the
//! precedence and from which source, with the warnings for what could not be used. The built-in
//! PDF (phase 3) and the LaTeX export (phase 4) both call [`resolve`].

use super::licence::Licence;
use super::scan::{faces_of_file, is_font_file, name_key, Catalogue, Face, Place};
use crate::pdf::write::FontKind;
use std::path::{Component, Path, PathBuf};

/// What a font is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// `print-font`: the text, and everything the other two do not take.
    Text,
    /// `print-font-sans`: headings, title, contents, page numbers; the sans of a TeX template.
    Sans,
    /// `print-font-mono`: code.
    Mono,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Text, Role::Sans, Role::Mono];

    /// The property of a note.
    pub fn property(self) -> &'static str {
        match self {
            Role::Text => "print-font",
            Role::Sans => "print-font-sans",
            Role::Mono => "print-font-mono",
        }
    }

    /// The key of a template set's manifest.
    fn manifest_key(self) -> &'static str {
        match self {
            Role::Text => "font",
            Role::Sans => "font_sans",
            Role::Mono => "font_mono",
        }
    }

    /// The field of the vault's settings file.
    fn setting(self) -> &'static str {
        match self {
            Role::Text => "print_font",
            Role::Sans => "print_font_sans",
            Role::Mono => "print_font_mono",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Role::Text => "text",
            Role::Sans => "headings",
            Role::Mono => "code",
        }
    }
}

/// The three values of one entry of the precedence: what a note, a set or the vault says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Values {
    pub text: Option<String>,
    pub sans: Option<String>,
    pub mono: Option<String>,
}

impl Values {
    /// An empty value is a missing one.
    fn clean(v: Option<&str>) -> Option<String> {
        v.map(str::trim).filter(|v| !v.is_empty()).map(String::from)
    }

    /// The `print-font*` properties of a note's front matter.
    pub fn of_note(text: &str) -> Values {
        let props = crate::pdf::export::print_properties(text);
        let get = |r: Role| Values::clean(props.get(r.property()).map(String::as_str));
        Values { text: get(Role::Text), sans: get(Role::Sans), mono: get(Role::Mono) }
    }

    /// The vault's settings (CFG-080).
    pub fn of_settings(s: &crate::vault_state::VaultSettings) -> Values {
        Values { text: Values::clean(Some(&s.print_font)), sans: Values::clean(Some(&s.print_font_sans)), mono: Values::clean(Some(&s.print_font_mono)) }
    }

    pub fn get(&self, role: Role) -> Option<&str> {
        match role {
            Role::Text => self.text.as_deref(),
            Role::Sans => self.sans.as_deref(),
            Role::Mono => self.mono.as_deref(),
        }
    }

    fn is_empty(&self) -> bool {
        Role::ALL.iter().all(|r| self.get(*r).is_none())
    }
}

/// Which export asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// The built-in PDF: no template set, no TeX Live.
    Pdf,
    /// The LaTeX export: the template set's values and `fonts/` count, and a name found nowhere
    /// goes to TeX Live.
    Tex,
}

/// What an export asks the resolver.
pub struct Request<'a> {
    pub engine: Engine,
    pub vault_root: &'a Path,
    /// The note's values, or for a book (`book`) those of its top folder note.
    pub note: Values,
    pub book: bool,
    /// A book's chapter notes (name, values): a chapter's own font is ignored with a warning.
    pub chapters: Vec<(String, Values)>,
    /// The template set (TeX only): its folder name and its manifest's values.
    pub set: Option<(String, Values)>,
    pub vault: Values,
}

/// Which entry of the precedence gave the font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    Note,
    BookTop,
    Set,
    Vault,
    /// None did: the built-in default.
    Default,
}

impl Entry {
    /// Where the value came from, in words.
    pub fn label(self) -> &'static str {
        match self {
            Entry::Note => "the note",
            Entry::BookTop => "the book's top note",
            Entry::Set => "the template set",
            Entry::Vault => "the vault's settings",
            Entry::Default => "the default",
        }
    }
}

/// One real file of a face.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFile {
    pub path: PathBuf,
    /// The face's index in a collection.
    pub index: u32,
}

/// The four faces of a family: regular, bold, italic, bold italic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceKind {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FaceKind {
    pub const ALL: [FaceKind; 4] = [FaceKind::Regular, FaceKind::Bold, FaceKind::Italic, FaceKind::BoldItalic];

    fn name(self) -> &'static str {
        match self {
            FaceKind::Regular => "regular",
            FaceKind::Bold => "bold",
            FaceKind::Italic => "italic",
            FaceKind::BoldItalic => "bold italic",
        }
    }
}

/// A family that was found: where, and a file for each face it has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    pub place: Place,
    /// Per [`FaceKind::ALL`]: the file of the face, or none where the face is missing and the
    /// writer has to imitate it (the regular face always has a file).
    pub faces: [Option<FontFile>; 4],
    /// A face of the family must not be subsetted: it is embedded whole (`fsType` bit 8).
    pub whole: bool,
}

impl Family {
    pub fn face(&self, kind: FaceKind) -> Option<&FontFile> {
        self.faces[FaceKind::ALL.iter().position(|k| *k == kind).unwrap_or(0)].as_ref()
    }
}

/// What a role got.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Chosen {
    /// Nothing was asked: the built-in default (the named font of the PDF, or the template's).
    Default(&'static str),
    /// One of the words `sans`, `serif`, `mono`: the font bundled with the PDF. The LaTeX export
    /// selects nothing for it.
    Bundled(FontKind),
    /// A font file of the vault, a set or the system.
    Family(Family),
    /// A name that is none of the vault's or the system's, in the LaTeX export: TeX Live looks.
    TexLive(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pick {
    pub role: Role,
    pub entry: Entry,
    pub chosen: Chosen,
}

impl Pick {
    /// The choice in words, for the export dialog and for the warnings: "Garamond (system, bold
    /// imitated)".
    pub fn describe(&self) -> String {
        match &self.chosen {
            Chosen::Default(name) => (*name).to_string(),
            Chosen::Bundled(kind) => match kind {
                FontKind::Serif => "Liberation Serif".into(),
                FontKind::Mono => "Hack".into(),
                _ => "Ubuntu Light".into(),
            },
            Chosen::TexLive(name) => format!("{name} (TeX Live, checked when exporting)"),
            Chosen::Family(f) => {
                let from = match &f.place {
                    Place::Vault => "vault".to_string(),
                    Place::Set(s) => format!("template set {s}"),
                    Place::System => "system".to_string(),
                };
                let file = |x: &FontFile| match x.index {
                    0 => x.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    i => format!("{} #{i}", x.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                };
                let mut notes = vec![from];
                if let Some(regular) = f.face(FaceKind::Regular).filter(|_| f.place != Place::System) {
                    notes.push(file(regular));
                }
                let imitated: Vec<&str> = FaceKind::ALL.iter().skip(1).filter(|k| f.face(**k).is_none()).map(|k| k.name()).collect();
                if !imitated.is_empty() && imitated.len() < 3 {
                    notes.push(format!("{} imitated", imitated.join(" and ")));
                } else if !imitated.is_empty() {
                    notes.push("bold and italic imitated".into());
                }
                if f.whole {
                    notes.push("embedded whole".into());
                }
                format!("{} ({})", f.name, notes.join(", "))
            }
        }
    }
}

/// The answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub text: Pick,
    pub sans: Pick,
    pub mono: Pick,
    /// What could not be used and what is used instead (PDF-037).
    pub warnings: Vec<String>,
    /// The system scan is not in and a name was not found elsewhere: ask again when it is.
    pub pending: bool,
}

impl Resolved {
    pub fn pick(&self, role: Role) -> &Pick {
        match role {
            Role::Text => &self.text,
            Role::Sans => &self.sans,
            Role::Mono => &self.mono,
        }
    }
}

/// Why a value could not be used.
struct Unusable(String);

fn default_name(engine: Engine, role: Role) -> &'static str {
    match (engine, role) {
        (Engine::Tex, _) => "the template's font",
        (Engine::Pdf, Role::Text) => "Ubuntu Light",
        (Engine::Pdf, Role::Sans) => "the text font",
        (Engine::Pdf, Role::Mono) => "Hack",
    }
}

/// Decide the fonts of an export (PDF-034): each role on its own, the first entry that gives a
/// usable value wins, and a value that cannot be used falls to the next entry with a warning.
pub fn resolve(req: &Request, cat: &Catalogue) -> Resolved {
    let mut warnings = Vec::new();
    let mut pending = false;
    if req.book {
        for (name, _) in req.chapters.iter().filter(|(_, v)| !v.is_empty()) {
            warnings.push(format!("The chapter “{name}” sets its own font; it is ignored, a book has one font (the top folder note's)."));
        }
    }
    let [text, sans, mono] = Role::ALL.map(|role| {
        let mut entries: Vec<(Entry, &Values)> = vec![(if req.book { Entry::BookTop } else { Entry::Note }, &req.note)];
        if let (Engine::Tex, Some((_, set))) = (req.engine, &req.set) {
            entries.push((Entry::Set, set));
        }
        entries.push((Entry::Vault, &req.vault));
        let mut failed: Vec<String> = Vec::new();
        let mut pick = Pick { role, entry: Entry::Default, chosen: Chosen::Default(default_name(req.engine, role)) };
        for (entry, values) in entries {
            let Some(raw) = values.get(role) else { continue };
            match use_value(req, cat, role, raw) {
                Ok(Some(chosen)) => {
                    pick = Pick { role, entry, chosen };
                    break;
                }
                // not found, and the system scan is not in: no warning yet
                Ok(None) => pending = true,
                Err(Unusable(why)) => {
                    let key = match entry {
                        Entry::Set => format!("the template set's `{}`", role.manifest_key()),
                        Entry::Vault => format!("the vault's `{}`", role.setting()),
                        _ => format!("`{}`", role.property()),
                    };
                    failed.push(format!("{key}: {why}"));
                }
            }
        }
        let used = pick.describe();
        warnings.extend(failed.into_iter().map(|f| format!("{f}; using {used}.")));
        pick
    });
    Resolved { text, sans, mono, warnings, pending }
}

/// The value as a font: `Ok(Some)` a font, `Err` it cannot be used, `Ok(None)` it may be found
/// when the system scan is in.
fn use_value(req: &Request, cat: &Catalogue, role: Role, raw: &str) -> Result<Option<Chosen>, Unusable> {
    if raw.chars().count() > 200 {
        return Err(Unusable("the value is longer than 200 characters; it is ignored".into()));
    }
    if raw.chars().any(char::is_control) {
        return Err(Unusable("the value has a control character; it is ignored".into()));
    }
    if role == Role::Text {
        let kind = match raw.to_lowercase().as_str() {
            "sans" => Some(FontKind::Sans),
            "serif" => Some(FontKind::Serif),
            "mono" => Some(FontKind::Mono),
            _ => None,
        };
        if let Some(kind) = kind {
            return Ok(Some(Chosen::Bundled(kind)));
        }
    }
    let link = raw.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")).map(|r| r.split('|').next().unwrap_or(r).trim());
    let target = link.unwrap_or(raw);
    if link.is_some() || target.contains('/') || target.contains('\\') || is_font_file(Path::new(target)) {
        return font_file(req, cat, raw, target).map(|f| Some(Chosen::Family(f)));
    }
    family(req, cat, raw)
}

/// A vault path to one font file: it gives that face only.
fn font_file(req: &Request, cat: &Catalogue, raw: &str, target: &str) -> Result<Family, Unusable> {
    let rel = Path::new(target.trim_start_matches("./"));
    if rel.is_absolute() || rel.components().any(|c| matches!(c, Component::ParentDir | Component::Prefix(_))) {
        return Err(Unusable(format!("the path “{raw}” is outside the vault; it is ignored")));
    }
    if !is_font_file(rel) {
        return Err(Unusable(format!("“{raw}” is not a font file (.ttf, .otf, .ttc)")));
    }
    // a bare file name is found as a link is: among the fonts of the vault
    let by_name = (rel.components().count() == 1).then(|| cat.vault.iter().find(|f| f.place == Place::Vault && f.path.file_name().is_some_and(|n| n.eq_ignore_ascii_case(rel.as_os_str()))).map(|f| f.path.clone())).flatten();
    let path = by_name.unwrap_or_else(|| req.vault_root.join(rel));
    let inside = match (path.canonicalize(), req.vault_root.canonicalize()) {
        (Ok(p), Ok(root)) => p.starts_with(root),
        _ => false,
    };
    if !path.is_file() {
        return Err(Unusable(format!("the font file “{raw}” was not found")));
    }
    if !inside {
        return Err(Unusable(format!("the path “{raw}” is outside the vault; it is ignored")));
    }
    let faces = faces_of_file(&path, &Place::Vault);
    let Some(face) = faces.first() else { return Err(Unusable(format!("“{raw}” cannot be read as a font"))) };
    refused(face, raw)?;
    Ok(Family { name: face.families.first().cloned().unwrap_or_default(), place: Place::Vault, faces: [Some(FontFile { path: face.path.clone(), index: face.index }), None, None, None], whole: face.licence == Licence::Whole })
}

fn refused(face: &Face, name: &str) -> Result<(), Unusable> {
    match face.licence {
        Licence::Refused(why) => Err(Unusable(format!("the font “{name}” cannot be used: {why}"))),
        _ => Ok(()),
    }
}

/// A family by name: the vault's `fonts/`, then (TeX) the set's, then the system, then (TeX) TeX Live.
fn family(req: &Request, cat: &Catalogue, name: &str) -> Result<Option<Chosen>, Unusable> {
    let key = name_key(name);
    let of = |faces: &[Face], place: &dyn Fn(&Place) -> bool| -> Vec<Face> { faces.iter().filter(|f| place(&f.place) && f.families.iter().any(|n| name_key(n) == key)).cloned().collect() };
    let set_name = req.set.as_ref().filter(|_| req.engine == Engine::Tex).map(|(n, _)| n.as_str());
    let mut found = of(&cat.vault, &|p| *p == Place::Vault);
    if found.is_empty() {
        found = of(&cat.vault, &|p| matches!(p, Place::Set(s) if Some(s.as_str()) == set_name));
    }
    if found.is_empty() {
        found = of(&cat.system, &|_| true);
    }
    if found.is_empty() {
        return match req.engine {
            Engine::Tex => Ok(Some(Chosen::TexLive(name.to_string()))),
            Engine::Pdf if !cat.system_ready => Ok(None),
            Engine::Pdf => Err(Unusable(format!("the font “{name}” was not found"))),
        };
    }
    let picked = pick_faces(&found.iter().collect::<Vec<_>>());
    let chosen: Vec<&Face> = picked.iter().flatten().copied().collect();
    for f in &chosen {
        refused(f, name)?;
    }
    let file = |f: &Face| FontFile { path: f.path.clone(), index: f.index };
    Ok(Some(Chosen::Family(Family {
        name: found[0].families.first().cloned().unwrap_or_else(|| name.to_string()),
        place: found[0].place.clone(),
        faces: picked.map(|f| f.map(file)),
        whole: chosen.iter().any(|f| f.licence == Licence::Whole),
    })))
}

/// The regular, bold, italic and bold italic face of one family (PDF-035): regular is upright at
/// the weight nearest 400, bold the nearest to 700 among 600 to 900, italic the nearest to 400
/// among the slanted faces lighter than 600, bold italic the nearest to 700 among the slanted
/// ones from 600; a face that the family lacks is none. Condensed and expanded faces are used
/// only when the family has no normal width.
fn pick_faces<'a>(faces: &[&'a Face]) -> [Option<&'a Face>; 4] {
    let normal: Vec<&Face> = faces.iter().copied().filter(|f| f.normal_width).collect();
    let faces = if normal.is_empty() { faces.to_vec() } else { normal };
    let nearest = |pool: &mut dyn Iterator<Item = &'a Face>, to: u16| pool.min_by_key(|f| (f.weight.abs_diff(to), f.weight));
    let regular = nearest(&mut faces.iter().copied().filter(|f| !f.italic), 400).or_else(|| nearest(&mut faces.iter().copied(), 400));
    let same = |f: &&Face| regular.is_none_or(|r| !std::ptr::eq(*f, r));
    let bold = nearest(&mut faces.iter().copied().filter(|f| !f.italic && (600..=900).contains(&f.weight)).filter(same), 700);
    let italic = nearest(&mut faces.iter().copied().filter(|f| f.italic && f.weight < 600).filter(same), 400);
    let bold_italic = nearest(&mut faces.iter().copied().filter(|f| f.italic && (600..=900).contains(&f.weight)).filter(same), 700);
    [regular, bold, italic, bold_italic]
}
