//! Template sets in the vault (TEX-005): where a set lives and what is in it, the manifests of its
//! document kinds, the person's settings of a set, the logo slot (TEX-007), and compiling a
//! document against a set. A set is a folder `templates/tex/<set>/` of the vault, so the exact
//! class files an export was written for are in the vault's backups and in its git sync.
//!
//! ```text
//! templates/tex/<set>/set.toml          what the set is and where it came from (`SetMeta`)
//! templates/tex/<set>/settings.toml     the person's choices (`SetSettings`); kept by a re-import
//! templates/tex/<set>/tex/              .cls .sty .def .cfg …: what goes on TEXINPUTS
//! templates/tex/<set>/kinds/<kind>.toml one document kind of the set (`KindManifest`)
//! templates/tex/<set>/logo/             the logo the person supplies; kept by a re-import
//! templates/tex/<set>/README.md         a note for people browsing the vault
//! ```

use super::compile::{compile, CompileError, CompileOutput, Options, WorkDir};
use super::engine::Engine;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where the sets are, relative to the vault.
pub const SETS_DIR: &str = "templates/tex";

/// The most bytes a logo file may have.
pub const MAX_LOGO: u64 = 5 << 20;

/// The name of the department logo's file without its extension (TEX-035): the slot every set has,
/// `logo/department_logo.pdf` or `.png`, and what `{department_logo}` in a kind's lines stands for.
pub const DEPARTMENT_LOGO: &str = "department_logo";

/// `<vault>/templates/tex`.
pub fn sets_root(vault: &Path) -> PathBuf {
    vault.join(SETS_DIR)
}

/// A file or repository name without `.tar.gz`, `.zip` or `.git`.
pub fn strip_archive_ext(name: &str) -> &str {
    name.strip_suffix(".tar.gz").or_else(|| name.strip_suffix(".zip")).or_else(|| name.strip_suffix(".git")).unwrap_or(name)
}

/// A folder name made from a repository or file name (`Tuda_Latex Templates.zip` → `tuda_latex-templates`).
pub fn id_from(name: &str) -> String {
    let stem = strip_archive_ext(name);
    let id: String = stem.to_lowercase().chars().map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() || "._-".contains(c) { c } else { '-' }).collect();
    let id = id.trim_matches(['.', '-']).chars().take(40).collect::<String>();
    if id.is_empty() {
        "templates".into()
    } else {
        id
    }
}

// ---------------------------------------------------------------- set.toml

/// Where a set came from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Source {
    /// A GitHub repository at a commit, tag or branch; `sha256` is the checksum of the archive
    /// that was checked (the built-in preset is pinned, other imports have none).
    Github {
        repo: String,
        reference: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sha256: Option<String>,
    },
    /// The newest version on CTAN (not pinned: whatever CTAN has the day it is imported).
    Ctan { url: String },
    /// A zip, a .tar.gz or a folder of this computer.
    Local { path: String },
}

impl Source {
    /// "GitHub tudace/tuda_latex_templates at e933be9, checked sha256" for the list of sets.
    pub fn describe(&self) -> String {
        match self {
            Source::Github { repo, reference, sha256 } => {
                let short: String = if reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit()) { reference.chars().take(7).collect() } else { reference.clone() };
                format!("GitHub {repo} at {short}{}", if sha256.is_some() { ", checksum checked" } else { ", no checksum" })
            }
            Source::Ctan { .. } => "CTAN, the newest version (no checksum)".into(),
            Source::Local { path } => format!("from {path}"),
        }
    }
}

/// `set.toml`: what a set is.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct SetMeta {
    /// The name shown in lists.
    pub name: String,
    /// The version of the templates, when they say it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// The day of the import, `2026-11-17`.
    pub imported_at: String,
    pub source: Source,
    /// The name of the logo file without its extension (`tuda_logo`): the set needs a logo the
    /// person supplies, in `logo/` (TEX-007). None: the set needs none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo_file: Option<String>,
}

// ---------------------------------------------------------------- settings.toml

/// The language of a set's documents.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    De,
}

impl Language {
    /// The language code of the PDF metadata.
    pub fn code(self) -> &'static str {
        match self {
            Language::En => "en",
            Language::De => "de",
        }
    }

    /// The name the LaTeX class and babel know it by.
    pub fn babel(self) -> &'static str {
        match self {
            Language::En => "english",
            Language::De => "ngerman",
        }
    }
}

/// `settings.toml`: the person's choices for a set. A re-import keeps this file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct SetSettings {
    /// The accent colour of the templates (`{accent}` in a kind's options), a name made of letters,
    /// digits, `-` and `_`: for tuda-ci `9c` (the colours are `0a` to `9d`).
    pub accent: String,
    /// The language of the documents (`{language}` in a kind's options and preamble).
    pub language: Language,
    /// The department that fills a kind's `department` field when the note and the dialog give none.
    pub department: String,
    /// The logo file in `logo/` (`tuda_logo.pdf`), set by "Choose logo…".
    pub logo: Option<String>,
    /// The department logo file in `logo/` (`department_logo.png`), set by "Choose department
    /// logo…" (TEX-035).
    pub department_logo: Option<String>,
    /// The name that fills a kind's `author` field when the note and the dialog give none (the
    /// guided setup asks for it, TEX-017).
    pub author: String,
}

impl SetSettings {
    /// An accent that cannot add an option or end a command.
    pub fn check(&self) -> Result<(), String> {
        if self.accent.len() > 20 || !self.accent.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) {
            return Err(format!("the accent colour {:?} may have only letters, digits, - and _", self.accent));
        }
        for (what, file) in [("logo", &self.logo), ("department_logo", &self.department_logo)] {
            if let Some(l) = file.iter().find(|l| !plain_file_name(l)) {
                return Err(format!("the {what} {l:?} must be a plain file name"));
            }
        }
        Ok(())
    }
}

/// A file name with no folders and nothing hidden.
fn plain_file_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\0', ':']) && name != ".."
}

/// `settings.toml` of a set folder; a missing file is the defaults.
pub fn load_settings(set_dir: &Path) -> Result<SetSettings, String> {
    let path = set_dir.join("settings.toml");
    let s: SetSettings = match crate::fsutil::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).map_err(|e| format!("settings.toml: {}", e.message()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => SetSettings::default(),
        Err(e) => return Err(e.to_string()),
    };
    s.check().map_err(|e| format!("settings.toml: {e}"))?;
    Ok(s)
}

/// Write `settings.toml` (atomically).
pub fn save_settings(set_dir: &Path, s: &SetSettings) -> Result<(), String> {
    let text = toml::to_string_pretty(s).map_err(|e| e.to_string())?;
    let text = format!("# The choices for this template set. Shale keeps this file when the set is updated.\n# accent: the accent colour of the templates (tuda-ci: 0a to 9d); language: en or de;\n# department and author: used when a note does not name one; logo and department_logo: set by\n# \"Choose logo…\" and \"Choose department logo…\".\n{text}");
    crate::fsutil::write_atomic(&set_dir.join("settings.toml"), text.as_bytes()).map_err(|e| format!("settings.toml could not be written ({e})"))
}

// ---------------------------------------------------------------- kinds

/// How the writer (TEX-008 to TEX-014) turns a note into the document of a kind.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Mapping {
    /// H1 is `\title`, H2 `\section`, H3 `\subsection` (TEX-010).
    Article,
    /// H1 is `\title`, H2 `\chapter`, H3 `\section` (TEX-011).
    Book,
    /// H1 is the title slide, every H2 a frame (TEX-012).
    Beamer,
    /// The recipient, subject and date come from the properties (TEX-013).
    Letter,
    /// Posters, leaflets, announcements (TEX-014).
    Poster,
}

/// One value of a document kind, taken from a front matter key of the note.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Field {
    /// The key in the note's properties (`title`, `author`, `department` …).
    pub key: String,
    /// What the export dialog calls it.
    #[serde(default)]
    pub label: String,
    /// The LaTeX command that gets the value, without the backslash: `\title{value}`.
    #[serde(default)]
    pub command: Option<String>,
    /// A KOMA-Script variable that gets the value: `\setkomavar{name}{value}` (letters).
    #[serde(default)]
    pub komavar: Option<String>,
    /// The export cannot run without a value.
    #[serde(default)]
    pub required: bool,
    /// What is used when the note has no value, as LaTeX code (`\today`).
    #[serde(default)]
    pub default: String,
}

/// `kinds/<kind>.toml`: one document kind of a set. The name of the file (`pub`) is the kind's id.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct KindManifest {
    /// The name shown in the export dialog.
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// How a note becomes a document of this kind.
    pub mapping: Mapping,
    /// The document class (a file `<class>.cls` in `tex/` or in TeX Live).
    pub class: String,
    /// Lines of LaTeX before `\documentclass` (for tuda-ci: the PDF/A set-up of the LaTeX kernel,
    /// which the classes of some kinds need to run without the old `pdfx`). `{lang}` is the
    /// language code of the settings (`en`, `de`), and `{language}` and `{accent}` as below.
    #[serde(default)]
    pub before_class: Vec<String>,
    /// The class options. `{accent}` and `{language}` are replaced from the settings of the set;
    /// an option whose value is empty is left out.
    #[serde(default)]
    pub options: Vec<String>,
    /// Lines of LaTeX after `\documentclass` (the same replacements).
    #[serde(default)]
    pub preamble: Vec<String>,
    /// The kind needs the set's logo (TEX-007): without one the placeholder is used and the
    /// export says it is not conformant.
    #[serde(default)]
    pub logo: bool,
    /// Lines of LaTeX after the preamble that put the department logo (TEX-035) where the class
    /// shows it: written only when the set's department logo slot is filled and the export asks for
    /// it. `{department_logo}` is the file's name without extension (`logo/` is on `TEXINPUTS`).
    /// Empty: the kind does not show a department logo.
    #[serde(default)]
    pub department_logo: Vec<String>,
    /// Poster kinds: the text of the note is set in this many columns (0 and 1: one).
    #[serde(default)]
    pub columns: u8,
    /// The values of the kind and where they go.
    #[serde(default)]
    pub fields: Vec<Field>,
}

/// The text of a manifest line with `{accent}`, `{language}` and `{lang}` filled in; none when
/// the line uses a value that is empty.
fn fill(line: &str, s: &SetSettings) -> Option<String> {
    if line.contains("{accent}") && s.accent.is_empty() {
        return None;
    }
    Some(line.replace("{accent}", &s.accent).replace("{language}", s.language.babel()).replace("{lang}", s.language.code()))
}

/// A class name that cannot end a command: letters, digits, `-`, `@` and `*`.
fn plain_name(n: &str) -> bool {
    !n.is_empty() && n.len() <= 40 && n.chars().all(|c| c.is_ascii_alphanumeric() || "-@*".contains(c))
}

impl KindManifest {
    /// Parse a manifest and check what the text it will be written into depends on.
    pub fn parse(text: &str) -> Result<KindManifest, String> {
        let k: KindManifest = toml::from_str(text).map_err(|e| e.message().to_string())?;
        if !plain_name(&k.class) {
            return Err(format!("class {:?} is not a class name", k.class));
        }
        for f in &k.fields {
            if f.key.is_empty() {
                return Err("a field has no key".into());
            }
            for n in [&f.command, &f.komavar].into_iter().flatten() {
                if n.is_empty() || !n.chars().all(|c| c.is_ascii_alphabetic()) {
                    return Err(format!("field {}: {n:?} is not the name of a command", f.key));
                }
            }
        }
        Ok(k)
    }

    /// The options of `\documentclass[…]` with the settings filled in.
    pub fn class_options(&self, s: &SetSettings) -> Vec<String> {
        self.options.iter().filter_map(|o| fill(o, s)).collect()
    }

    /// The lines before `\documentclass` with the settings filled in.
    pub fn before_class_lines(&self, s: &SetSettings) -> Vec<String> {
        self.before_class.iter().filter_map(|l| fill(l, s)).collect()
    }

    /// The preamble lines with the settings filled in.
    pub fn preamble_lines(&self, s: &SetSettings) -> Vec<String> {
        self.preamble.iter().filter_map(|l| fill(l, s)).collect()
    }

    /// The lines that put the department logo in (TEX-035), `{department_logo}` filled in.
    pub fn department_logo_lines(&self, s: &SetSettings) -> Vec<String> {
        self.department_logo.iter().filter_map(|l| fill(l, s)).map(|l| l.replace("{department_logo}", DEPARTMENT_LOGO)).collect()
    }

    /// Whether a document of this kind shows the set's department logo: the kind has a place for
    /// it, the slot is filled and the export asks for it (TEX-035).
    pub fn shows_department_logo(&self, set: &SetInfo, wanted: bool) -> bool {
        wanted && set.department_logo.is_some() && !self.department_logo.is_empty()
    }
}

/// The lines before the class, `\documentclass[options]{class}` and the preamble of a kind, and
/// the lines of the department logo when `department_logo` (TEX-035).
pub(super) fn class_and_preamble(k: &KindManifest, s: &SetSettings, department_logo: bool) -> String {
    let opts = k.class_options(s).join(",");
    let mut t: String = k.before_class_lines(s).iter().map(|l| format!("{l}\n")).collect();
    t.push_str(&if opts.is_empty() { format!("\\documentclass{{{}}}\n", k.class) } else { format!("\\documentclass[{opts}]{{{}}}\n", k.class) });
    let logo = if department_logo { k.department_logo_lines(s) } else { Vec::new() };
    for l in k.preamble_lines(s).into_iter().chain(logo) {
        t.push_str(&l);
        t.push('\n');
    }
    t
}

/// Text for TeX: every character that TeX would read as a command, a group, a comment or a shortcut
/// is written as itself, so that no text of a note can end the document or run anything
/// (TEX-008). The characters `\ { } $ & # ^ _ ~ %` get their commands, `"` (a shortcut of babel's German) and `< > |`
/// (ligatures and old encodings) too; control characters and the byte order mark are left out.
/// Unicode stays Unicode: the engine is LuaLaTeX.
pub fn tex_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_emoji = false;
    for c in text.chars() {
        // a run of emoji is set in the emoji font that travels with the document (TEX-041)
        if is_emoji(c) || (in_emoji && c == '\u{200d}') {
            if !in_emoji {
                out.push_str("\\shaleemoji{");
                in_emoji = true;
            }
            if c != '\u{200d}' {
                out.push(c);
            }
            continue;
        }
        // the variation selectors only choose between a text and a picture look of the character before
        if matches!(c, '\u{fe0e}' | '\u{fe0f}') {
            continue;
        }
        if in_emoji {
            out.push('}');
            in_emoji = false;
        }
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '{' | '}' | '$' | '&' | '#' | '_' | '%' => {
                out.push('\\');
                out.push(c);
            }
            '^' => out.push_str("\\textasciicircum{}"),
            '~' => out.push_str("\\textasciitilde{}"),
            '"' => out.push_str("\\textquotedbl{}"),
            '<' => out.push_str("\\textless{}"),
            '>' => out.push_str("\\textgreater{}"),
            '|' => out.push_str("\\textbar{}"),
            '\t' | '\n' | '\r' | '\u{2028}' | '\u{2029}' => out.push(' '),
            c if c.is_control() || c == '\u{feff}' => {}
            c => out.push(c),
        }
    }
    if in_emoji {
        out.push('}');
    }
    out
}

/// The name of the emoji font in the work folder (TEX-041).
pub const EMOJI_FONT_FILE: &str = "shale-emoji.ttf";

/// The emoji font of the document: the one the built-in export draws with (Noto Emoji, monochrome).
pub fn emoji_font() -> &'static [u8] {
    epaint_default_fonts::NOTO_EMOJI_REGULAR
}

/// A character of the emoji and pictograph blocks (a calendar, a check mark, an hourglass, a
/// cross mark, a star) that the text fonts of the templates have no glyph for.
fn is_emoji(c: char) -> bool {
    matches!(c as u32, 0x1F000..=0x1FAFF | 0x2600..=0x27BF | 0x231A..=0x231B | 0x23E9..=0x23FA | 0x2B50 | 0x2B55 | 0x2B05..=0x2B07)
}

/// The value a field gets in a sample document: its default, the department or author of the
/// settings, or a short text that says what it is.
fn sample_value(f: &Field, s: &SetSettings) -> String {
    if f.key == "department" && !s.department.is_empty() {
        return tex_escape(&s.department);
    }
    if f.key == "author" && !s.author.is_empty() {
        return tex_escape(&s.author);
    }
    if !f.default.is_empty() {
        return f.default.clone();
    }
    match f.key.as_str() {
        "title" => "Shale test page".into(),
        "subtitle" => "A test of the template set".into(),
        "author" | "sender" => "Shale".into(),
        "recipient" => "Sample Recipient\\\\Sample Street 1\\\\12345 Sample City".into(),
        "sender_address" => "Sample Street 2\\\\12345 Sample City".into(),
        "subject" => "Shale test page".into(),
        "date" => "2026-01-01".into(),
        _ => format!("Sample {}", f.key),
    }
}

/// A small document of a kind for the test page of a set: every field with a sample value, a few
/// paragraphs, in the shape of the kind's mapping, with the department logo when `department_logo`
/// and the kind has a place for it (TEX-035). The writer makes the real documents.
pub fn sample_document(k: &KindManifest, s: &SetSettings, department_logo: bool) -> String {
    let mut t = class_and_preamble(k, s, department_logo);
    for f in &k.fields {
        // no \date: the class puts in today's
        if let Some(c) = f.command.as_ref().filter(|_| f.key != "date") {
            t.push_str(&format!("\\{c}{{{}}}\n", sample_value(f, s)));
        }
    }
    let para = "A few words so that the page is not empty: the quick brown fox jumps over the lazy dog.";
    t.push_str("\\begin{document}\n");
    match k.mapping {
        Mapping::Article => t.push_str(&format!("\\maketitle\n\\section{{First section}}\n{para}\n\\subsection{{A part}}\n{para}\n")),
        Mapping::Book => t.push_str(&format!("\\maketitle\n\\chapter{{First chapter}}\n{para}\n\\section{{A section}}\n{para}\n")),
        Mapping::Beamer => t.push_str(&format!("\\maketitle\n\\begin{{frame}}{{First frame}}\n{para}\n\\end{{frame}}\n")),
        Mapping::Letter => {
            let get = |key: &str| k.fields.iter().find(|f| f.key == key).map(|f| sample_value(f, s)).unwrap_or_default();
            t.push_str(&format!("\\begin{{letter}}{{{}}}\n", get("recipient")));
            for f in &k.fields {
                if let Some(v) = &f.komavar {
                    t.push_str(&format!("\\setkomavar{{{v}}}{{{}}}\n", sample_value(f, s)));
                }
            }
            t.push_str(&format!("\\opening{{{}}}\n{para}\n\\closing{{{}}}\n\\end{{letter}}\n", get("opening"), get("closing")));
        }
        Mapping::Poster if k.class.contains("sciposter") => {
            t.push_str(&format!("\\maketitle\n\\begin{{tcbposter}}[poster={{columns=2,rows=2,spacing=1cm}}]\n\\begin{{posterboxenv}}[title=First box]{{name=a,column=1,row=1}}\n{para}\n\\end{{posterboxenv}}\n\\end{{tcbposter}}\n"));
        }
        Mapping::Poster => t.push_str(&format!("\\maketitle\n{para}\n")),
    }
    t.push_str("\\end{document}\n");
    t
}

// ---------------------------------------------------------------- scanning

/// Whether a set has the logo it asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LogoStatus {
    /// The set needs no logo.
    NotNeeded,
    /// The set needs one and the slot is empty: exports use a placeholder.
    Missing,
    /// The file in `logo/`.
    Present(String),
}

impl LogoStatus {
    /// For the list of sets.
    pub fn text(&self) -> String {
        match self {
            LogoStatus::NotNeeded => "no logo needed".into(),
            LogoStatus::Missing => "logo missing: exports are not CI-conformant".into(),
            LogoStatus::Present(f) => format!("logo: {f}"),
        }
    }
}

/// A set that was read.
#[derive(Clone, Debug)]
pub struct SetInfo {
    /// The folder name below `templates/tex`.
    pub id: String,
    /// The folder, absolute.
    pub dir: PathBuf,
    pub meta: SetMeta,
    pub settings: SetSettings,
    /// The kinds with their ids, by id; the ones that could not be read are in `problems`.
    pub kinds: Vec<(String, KindManifest)>,
    pub problems: Vec<String>,
    pub logo: LogoStatus,
    /// The department logo file in `logo/` (TEX-035); none: the slot is empty.
    pub department_logo: Option<String>,
}

impl SetInfo {
    /// "department logo: department_logo.png" for the list of sets; none when the slot is empty
    /// (which is no warning).
    pub fn department_logo_text(&self) -> Option<String> {
        self.department_logo.as_ref().map(|f| format!("department logo: {f}"))
    }
}

/// A folder of `templates/tex`: the set, or why it could not be read.
#[derive(Clone, Debug)]
pub struct SetEntry {
    pub id: String,
    pub info: Result<SetInfo, String>,
}

/// The logo file of a set: the one the settings name (`named`), else `<stem>.pdf` or `<stem>.png`.
fn find_logo(dir: &Path, stem: &str, named: &Option<String>) -> Option<String> {
    let named = named.iter().cloned();
    let guessed = ["pdf", "png"].map(|e| format!("{stem}.{e}"));
    named.chain(guessed).find(|f| plain_file_name(f) && std::fs::symlink_metadata(dir.join("logo").join(f)).is_ok_and(|m| m.is_file()))
}

/// Read one set folder.
pub fn read_set(id: &str, dir: &Path) -> Result<SetInfo, String> {
    let meta_text = crate::fsutil::read_to_string(dir.join("set.toml")).map_err(|e| format!("set.toml cannot be read ({e})"))?;
    let meta: SetMeta = toml::from_str(&meta_text).map_err(|e| format!("set.toml: {}", e.message()))?;
    let settings = load_settings(dir)?;
    let (mut kinds, mut problems) = (Vec::new(), Vec::new());
    if let Ok(rd) = std::fs::read_dir(dir.join("kinds")) {
        let mut files: Vec<PathBuf> = rd.filter_map(|e| Some(e.ok()?.path())).filter(|p| p.extension().is_some_and(|e| e == "toml")).collect();
        files.sort();
        for f in files {
            let kind_id = f.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            match crate::fsutil::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| KindManifest::parse(&t)) {
                Ok(k) => kinds.push((kind_id, k)),
                Err(e) => problems.push(format!("kinds/{kind_id}.toml: {e}")),
            }
        }
    }
    let logo = match &meta.logo_file {
        None => LogoStatus::NotNeeded,
        Some(stem) => find_logo(dir, stem, &settings.logo).map_or(LogoStatus::Missing, LogoStatus::Present),
    };
    let department_logo = find_logo(dir, DEPARTMENT_LOGO, &settings.department_logo);
    Ok(SetInfo { id: id.to_string(), dir: dir.to_path_buf(), meta, settings, kinds, problems, logo, department_logo })
}

/// The sets of a vault, by folder name. A folder with a hidden name (a half-made import) is left out.
pub fn scan(vault: &Path) -> Vec<SetEntry> {
    let mut found: Vec<SetEntry> = std::fs::read_dir(sets_root(vault))
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| {
            let id = e.file_name().to_string_lossy().into_owned();
            (!id.starts_with('.')).then(|| SetEntry { info: read_set(&id, &e.path()), id })
        })
        .collect();
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found
}

// ---------------------------------------------------------------- the logo slot (TEX-007)

/// A grey bar as a one-page PDF, 60 by 20 mm: the logo of a set whose slot is empty, so a class
/// that needs `tuda_logo.pdf` still compiles. It is made for one compile and never kept.
pub fn placeholder_logo_pdf() -> Vec<u8> {
    let content = "0.75 g 0 0 170 57 re f\n";
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 170 57] /Contents 4 0 R /Resources << >> >>".to_string(),
        format!("<< /Length {} >>\nstream\n{content}endstream", content.len()),
    ];
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1));
    for o in offsets {
        pdf.push_str(&format!("{o:010} 00000 n \n"));
    }
    pdf.push_str(&format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1));
    pdf.into_bytes()
}

/// The extension a logo file may have, from its first bytes: a PDF or a PNG.
fn logo_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF-") {
        Some("pdf")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("png")
    } else {
        None
    }
}

/// Copy a PDF or PNG into `logo/` of the set as `<stem>.pdf` or `.png` (as the classes look for
/// it, at most [`MAX_LOGO`] bytes, recognised by its first bytes). A logo that was there, of either
/// kind, is moved to the vault's trash, not overwritten. Returns the file name in `logo/`.
fn put_logo(vault: &Path, set: &SetInfo, stem: &str, from: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(from).map_err(|e| format!("{} cannot be read ({e})", from.display()))?;
    if !meta.is_file() || meta.len() > MAX_LOGO {
        return Err(format!("{} is not a file of at most {} MB", from.display(), MAX_LOGO >> 20));
    }
    let bytes = crate::fsutil::read(from).map_err(|e| e.to_string())?;
    let ext = logo_extension(&bytes).ok_or("the logo must be a PDF or a PNG file")?;
    let name = format!("{stem}.{ext}");
    let dir = set.dir.join("logo");
    std::fs::create_dir_all(&dir).map_err(|e| format!("the logo folder could not be made ({e})"))?;
    // whatever file the classes could pick instead is moved away first, so exactly one logo is there
    for old in ["pdf", "png"].map(|e| format!("{stem}.{e}")) {
        if std::fs::symlink_metadata(dir.join(&old)).is_ok() && !(old == name && crate::fsutil::read(dir.join(&old)).is_ok_and(|b| b == bytes)) {
            trash_logo(vault, set, &old)?;
        }
    }
    crate::fsutil::write_atomic(&dir.join(&name), &bytes).map_err(|e| format!("the logo could not be written ({e})"))?;
    Ok(name)
}

/// Move a file of the set's `logo/` to the vault's trash.
fn trash_logo(vault: &Path, set: &SetInfo, file: &str) -> Result<(), String> {
    crate::vault::move_to_trash(vault, &Path::new(SETS_DIR).join(&set.id).join("logo").join(file)).map(|_| ()).map_err(|e| format!("the logo that is there could not be moved to the trash ({e})"))
}

/// "Choose logo…": copy a PDF or PNG into `logo/` of the set (named `<logo_file>.pdf` or `.png`, as
/// the classes look for it) and record it in `settings.toml`. A logo that was there is moved to
/// the vault's trash, not overwritten. Returns the file name in `logo/`.
pub fn choose_logo(vault: &Path, set: &SetInfo, from: &Path) -> Result<String, String> {
    let stem = set.meta.logo_file.as_deref().ok_or("this set has no logo slot")?;
    let name = put_logo(vault, set, stem, from)?;
    save_settings(&set.dir, &SetSettings { logo: Some(name.clone()), ..set.settings.clone() })?;
    Ok(name)
}

/// "Choose department logo…" (TEX-035): the same for the department logo slot every set has,
/// `logo/department_logo.pdf` or `.png`, recorded as `department_logo`.
pub fn choose_department_logo(vault: &Path, set: &SetInfo, from: &Path) -> Result<String, String> {
    let name = put_logo(vault, set, DEPARTMENT_LOGO, from)?;
    save_settings(&set.dir, &SetSettings { department_logo: Some(name.clone()), ..set.settings.clone() })?;
    Ok(name)
}

/// "Remove department logo" (TEX-035): the file goes to the vault's trash and the setting is cleared.
pub fn remove_department_logo(vault: &Path, set: &SetInfo) -> Result<(), String> {
    for old in ["pdf", "png"].map(|e| format!("{DEPARTMENT_LOGO}.{e}")) {
        if std::fs::symlink_metadata(set.dir.join("logo").join(&old)).is_ok() {
            trash_logo(vault, set, &old)?;
        }
    }
    save_settings(&set.dir, &SetSettings { department_logo: None, ..set.settings.clone() })
}

// ---------------------------------------------------------------- compiling against a set

/// What a compile against a set made.
#[derive(Debug)]
pub struct SetOutput {
    pub output: CompileOutput,
    /// The logo slot was empty and the placeholder was used: the PDF is not CI-conformant.
    pub placeholder_logo: bool,
}

/// Compile `main_tex` (with its text and the files it needs) against a set: `TEXINPUTS` is the
/// set's `tex/`, its `logo/` and TeX's own. Without a logo in the slot a placeholder is put into
/// the compile's work folder (never into the vault) so the class does not fail on the missing file.
/// `opts` carry the timeout and the cancel flag (TEX-004); its `tex_inputs` are replaced by the set's.
pub fn compile_with_set(engine: &Engine, set: &SetInfo, main_tex: &str, source: &str, files: &[(&str, &[u8])], opts: Options) -> Result<SetOutput, CompileError> {
    let fail = |m: String| CompileError { headline: m, problems: vec![], log_tail: String::new() };
    let _stage = tracing::info_span!("flow.tex_export.stage", peer = "disk").entered();
    let work = WorkDir::new().map_err(|e| fail(format!("no temporary folder ({e})")))?;
    work.write(main_tex, source.as_bytes()).map_err(|e| fail(format!("{main_tex} could not be written ({e})")))?;
    for (name, bytes) in files {
        work.write(name, bytes).map_err(|e| fail(format!("{name} could not be written ({e})")))?;
    }
    let placeholder_logo = match (&set.logo, &set.meta.logo_file) {
        (LogoStatus::Missing, Some(stem)) => {
            work.write(&format!("{stem}.pdf"), &placeholder_logo_pdf()).map_err(|e| fail(format!("the placeholder logo could not be written ({e})")))?;
            true
        }
        _ => false,
    };
    let mut tex_inputs = vec![set.dir.join("tex")];
    if set.dir.join("logo").is_dir() {
        tex_inputs.push(set.dir.join("logo"));
    }
    drop(_stage);
    let output = tracing::info_span!("flow.tex_export.compile", peer = "tex").in_scope(|| compile(engine, work, main_tex, &Options { tex_inputs, ..opts }))?;
    Ok(SetOutput { output, placeholder_logo })
}

/// The kind the test page compiles: `pub`, else the first article-like one.
pub fn test_kind(set: &SetInfo) -> Option<&(String, KindManifest)> {
    set.kinds.iter().find(|(id, _)| id == "pub").or_else(|| set.kinds.iter().find(|(_, k)| k.mapping == Mapping::Article))
}
