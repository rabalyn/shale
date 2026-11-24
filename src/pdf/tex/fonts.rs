//! The fonts of the LaTeX export (TEX-054 to TEX-057, TEX-059, BOOK-022, ADR-0040 §7): the three
//! roles of the shared resolver (`fonts::resolve`) turned into `\setmainfont`, `\setsansfont` and
//! `\setmonofont`, the font files of the vault and of a set copied into the work folder, and what
//! the compile needs to take an override out again when TeX cannot find its font.

use super::sets::{SetInfo, EMOJI_FONT_FILE};
use crate::fonts::resolve::{self, Chosen, Engine, FaceKind, Family, Role, Values};
use crate::fonts::scan::{Catalogue, Place};
use crate::vault::Vault;
use std::path::{Path, PathBuf};

/// What the resolver needs besides the note and the set: the fonts the window knows and the
/// vault's own settings (CFG-080).
#[derive(Clone, Debug, Default)]
pub struct FontInput {
    pub catalogue: Catalogue,
    pub vault: Values,
}

/// One `\setmainfont`, `\setsansfont` or `\setmonofont` line of a document, with what the compile
/// needs to take it out again (TEX-057).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FontOverride {
    /// The font as the person named it, for the warning.
    pub label: String,
    /// The names this line gives fontspec: the font and its faces. A "cannot be found" for one of
    /// them belongs to this line.
    pub names: Vec<String>,
    /// The line, without its line end.
    pub line: String,
}

/// What the fonts of a document come to: the lines, the font files they need next to `main.tex`,
/// and the warnings of the resolver and of the copy.
#[derive(Debug, Default)]
pub(super) struct Overrides {
    pub fonts: Vec<FontOverride>,
    pub files: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
}

/// The fonts of one export: the note (for a book its top folder note) and, for a book, the notes
/// of the chapters, whose own font is ignored with a warning (BOOK-022).
pub(super) struct Source<'a> {
    pub vault: &'a Vault,
    pub set: &'a SetInfo,
    pub note: Values,
    pub book: bool,
    pub chapters: Vec<(String, Values)>,
}

/// The command of a role.
fn command(role: Role) -> &'static str {
    match role {
        Role::Text => "\\setmainfont",
        Role::Sans => "\\setsansfont",
        Role::Mono => "\\setmonofont",
    }
}

/// Decide the fonts of a document and write their lines (TEX-054, TEX-055). Without an input
/// (a test page of a set) nothing is overridden.
pub(super) fn overrides(input: Option<&FontInput>, src: Source) -> Overrides {
    let Some(input) = input else { return Overrides::default() };
    let request = resolve::Request { engine: Engine::Tex, vault_root: &src.vault.root, note: src.note, book: src.book, chapters: src.chapters, set: Some((src.set.id.clone(), src.set.settings.fonts())), vault: input.vault.clone() };
    let resolved = resolve::resolve(&request, &input.catalogue);
    let mut out = Overrides { warnings: resolved.warnings.clone(), ..Overrides::default() };
    let mut copies = Copies::default();
    for role in Role::ALL {
        let pick = resolved.pick(role);
        let made = match &pick.chosen {
            // nothing was asked, or one of the words of the built-in PDF, which select nothing in TeX (TEX-054)
            Chosen::Default(_) | Chosen::Bundled(_) => continue,
            Chosen::TexLive(name) => by_name(role, name),
            Chosen::Family(f) if f.place == Place::System => by_name(role, &f.name),
            Chosen::Family(f) => from_files(role, f, &mut copies),
        };
        match made {
            Ok(line) => out.fonts.push(line),
            Err(why) => out.warnings.push(format!("`{}` ({}): {why}; the template's font is used.", role.property(), pick.entry.label())),
        }
    }
    out.files = copies.files;
    out
}

/// The characters that TeX reads as markup: a font name with one of them is not passed on, so no
/// name can end the argument or run anything (TEX-008).
const MARKUP: [char; 10] = ['\\', '{', '}', '%', '#', '$', '^', '~', '&', '"'];

/// A family that fontspec looks up by name: a system font or a TeX Live font.
fn by_name(role: Role, name: &str) -> Result<FontOverride, String> {
    if let Some(bad) = name.chars().find(|c| MARKUP.contains(c)) {
        return Err(format!("the font name “{name}” has the character {bad:?}, which cannot be passed to TeX"));
    }
    Ok(FontOverride { label: name.to_string(), names: vec![name.to_string()], line: format!("{}{{{name}}}", command(role)) })
}

/// The files of a family of the vault or a set, copied into the work folder, as one line with `Path=./`
/// and the faces it has.
pub(super) fn from_files(role: Role, family: &Family, copies: &mut Copies) -> Result<FontOverride, String> {
    let mut named: Vec<(FaceKind, String)> = Vec::new();
    for kind in FaceKind::ALL {
        if let Some(file) = family.face(kind) {
            // fontspec takes a file, not a face of a collection
            if file.index != 0 {
                return Err(format!("the font “{}” is the face {} of a font collection, which TeX cannot select", family.name, file.index));
            }
            named.push((kind, copies.copy(&file.path)?));
        }
    }
    let Some((_, regular)) = named.first().filter(|(k, _)| *k == FaceKind::Regular) else { return Err(format!("the font “{}” has no regular face", family.name)) };
    let option = |kind: FaceKind| match kind {
        FaceKind::Regular => None,
        FaceKind::Bold => Some("BoldFont"),
        FaceKind::Italic => Some("ItalicFont"),
        FaceKind::BoldItalic => Some("BoldItalicFont"),
    };
    let mut options = vec!["Path=./".to_string()];
    options.extend(named.iter().filter_map(|(k, n)| option(*k).map(|o| format!("{o}={n}"))));
    let line = format!("{}{{{regular}}}[{}]", command(role), options.join(", "));
    Ok(FontOverride { label: family.name.clone(), names: named.iter().map(|(_, n)| n.clone()).collect(), line })
}

/// The font files copied into the work folder: each file once, under a name that TeX can take
/// and that no other file of the folder has.
pub(super) struct Copies {
    done: Vec<(PathBuf, String)>,
    files: Vec<(String, Vec<u8>)>,
    used: Vec<String>,
}

impl Default for Copies {
    fn default() -> Self {
        Copies { done: Vec::new(), files: Vec::new(), used: vec![EMOJI_FONT_FILE.to_string(), "main.tex".to_string()] }
    }
}

impl Copies {
    /// The name in the work folder of the file at `path`; the file is read and kept the first time.
    pub(super) fn copy(&mut self, path: &Path) -> Result<String, String> {
        if let Some((_, name)) = self.done.iter().find(|(p, _)| p == path) {
            return Ok(name.clone());
        }
        let shown = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let bytes = crate::fsutil::read(path).map_err(|e| format!("the font file “{shown}” could not be read ({e})"))?;
        let name = self.free_name(&shown);
        self.done.push((path.to_path_buf(), name.clone()));
        self.used.push(name.clone());
        self.files.push((name.clone(), bytes));
        Ok(name)
    }

    /// `Garamond Bold_1.otf` as `Garamond-Bold-1.otf`: letters, digits, `.` and `-`; a name that is taken gets a number.
    fn free_name(&self, file: &str) -> String {
        let clean: String = file.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '-' }).collect();
        let (stem, ext) = clean.rsplit_once('.').unwrap_or((&clean, "otf"));
        let stem = if stem.is_empty() { "font" } else { stem };
        let mut name = format!("{stem}.{ext}");
        let mut n = 1;
        while self.used.iter().any(|u| u.eq_ignore_ascii_case(&name)) {
            n += 1;
            name = format!("{stem}-{n}.{ext}");
        }
        name
    }
}

/// The longest line of a TeX log (`max_print_line`): a longer line is broken there, also inside a
/// font name.
pub(super) const LOG_LINE: usize = 79;

/// The font names of fontspec's "The font "X" cannot be found" in the log of a compile, each once.
/// fontspec says it on the lines after "Package fontspec Error:", and TeX breaks a long line where
/// the log has [`LOG_LINE`] characters, so the log is put together again first.
pub fn missing_fonts(log: &str) -> Vec<String> {
    let mut text = String::new();
    for line in log.lines() {
        text.push_str(line);
        if line.chars().count() != LOG_LINE {
            text.push('\n');
        }
    }
    let mut found: Vec<String> = Vec::new();
    let mut rest = text.as_str();
    while let Some((_, after)) = rest.split_once("The font \"") {
        rest = after;
        let Some((name, tail)) = after.split_once('"') else { break };
        if tail.trim_start().starts_with("cannot be found") && !name.contains('\n') && !found.iter().any(|f| f == name) {
            found.push(name.to_string());
        }
    }
    found
}

/// Take the lines of the overrides whose font TeX could not find out of the text of `main.tex`
/// (TEX-057). Returns the new text, the warnings for the fonts taken out, and the lines that
/// stay; none when the error names none of the overrides (the template's own font, say).
pub fn without_missing(source: &str, overrides: &[FontOverride], missing: &[String]) -> Option<(String, Vec<String>)> {
    let hit = |o: &FontOverride| o.names.iter().any(|n| missing.iter().any(|m| m.eq_ignore_ascii_case(n) || m.trim_start_matches("./").eq_ignore_ascii_case(n)));
    let gone: Vec<&FontOverride> = overrides.iter().filter(|o| hit(o)).collect();
    if gone.is_empty() {
        return None;
    }
    let mut text = source.to_string();
    for o in &gone {
        text = text.replacen(&format!("{}\n", o.line), "", 1);
    }
    let warnings = gone.iter().map(|o| format!("the font “{}” was not found by TeX; the template's font is used", o.label)).collect();
    Some((text, warnings))
}
