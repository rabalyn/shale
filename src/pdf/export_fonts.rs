//! The fonts of the built-in PDF (PDF-034, PDF-036, PDF-037, BOOK-022, ADR-0040): the export asks
//! the resolver of `fonts::resolve` for the fonts of the text, the headings and the code, makes
//! the faces of the chosen families known to the writer, and collects what could not be used.

use crate::fonts::resolve::{self, Chosen, Engine, FaceKind, Request, Role, Values};
use crate::fonts::scan::Catalogue;
use crate::pdf::export::ExportOptions;
use crate::pdf::write::{register_family, FontKind};
use std::path::{Path, PathBuf};

/// What an export needs to look fonts up: what the font scan found and what the vault's settings
/// say (CFG-080). The window makes it when the export starts; an export without one sets the
/// text in the fonts of a note's words `sans`, `serif` and `mono` only.
#[derive(Clone, Debug)]
pub struct FontsAsk {
    pub catalogue: Catalogue,
    pub vault: Values,
    /// The system fonts were not in when the export started and it did not wait for them any longer.
    pub late: bool,
}

/// The PDF and what the export has to say about it (the warnings of PDF-037).
#[derive(Debug)]
pub struct Made {
    pub bytes: Vec<u8>,
    pub warnings: Vec<String>,
}

/// The options of an export with the fonts its note or book asks for (PDF-034). `note` is the
/// note's values, or a book's top folder note's; `chapters` are a book's chapter notes (BOOK-022).
/// Gives the warnings of the resolver and of fonts that cannot be read.
pub fn apply(opts: &ExportOptions, vault_root: &Path, note: Values, chapters: Option<Vec<(String, Values)>>) -> (ExportOptions, Vec<String>) {
    let Some(ask) = opts.fonts.clone() else { return (opts.clone(), Vec::new()) };
    let book = chapters.is_some();
    let request = Request { engine: Engine::Pdf, vault_root, note, book, chapters: chapters.unwrap_or_default(), set: None, vault: ask.vault.clone() };
    let resolved = resolve::resolve(&request, &ask.catalogue);
    let mut warnings = resolved.warnings.clone();
    if ask.late && resolved.pending {
        warnings.push("The installed fonts were not looked up in time: a font that only the system has was not used.".into());
    }
    let mut out = opts.clone();
    let mut kinds: Vec<Option<FontKind>> = Vec::new();
    for role in Role::ALL {
        kinds.push(match &resolved.pick(role).chosen {
            Chosen::Bundled(kind) => Some(*kind),
            Chosen::Family(f) => {
                let files: [Option<(PathBuf, u32)>; 4] = std::array::from_fn(|n| f.face(FaceKind::ALL[n]).map(|x| (x.path.clone(), x.index)));
                let kind = register_family(&f.name, &files, f.whole);
                if kind.is_none() {
                    warnings.push(format!("`{}`: the font “{}” cannot be read; using {}.", role.property(), f.name, default_of(role)));
                }
                kind
            }
            Chosen::Default(_) | Chosen::TexLive(_) => None,
        });
    }
    if let Some(text) = kinds[0] {
        out.font = text;
    }
    // the headings follow the text unless `print-font-sans` says otherwise (ADR-0040 §1)
    out.head_font = kinds[1].or(out.head_font);
    out.code_font = kinds[2].unwrap_or(FontKind::Mono);
    (out, warnings)
}

fn default_of(role: Role) -> &'static str {
    match role {
        Role::Text => "Ubuntu Light",
        Role::Sans => "the text font",
        Role::Mono => "Hack",
    }
}

/// The warnings for faces that a chosen family lacks and the document used, once per face
/// (PDF-036): "Garamond has no bold face: bold text is imitated."
pub fn imitation_warnings(faces: &[(String, bool, bool)]) -> Vec<String> {
    faces
        .iter()
        .map(|(family, bold, italic)| {
            let face = match (bold, italic) {
                (true, true) => "bold italic",
                (true, false) => "bold",
                _ => "italic",
            };
            format!("{family} has no {face} face: {face} text is imitated.")
        })
        .collect()
}
