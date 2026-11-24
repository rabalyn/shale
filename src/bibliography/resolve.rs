//! Which files a note's bibliography is (EDIT-123, ADR-0039 §5): the property `bibliography:`,
//! found as the `logo:` of a book is (BOOK-019: a vault path, a file name or a wiki link), and a
//! list for several; a note without the property uses the default of the vault's settings.

use crate::markdown;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

/// The files that are asked for, and what is wrong with the names that name no file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Wanted {
    pub files: Vec<PathBuf>,
    pub problems: Vec<String>,
}

/// The vault target a value names: `refs.bib`, `lit/refs.bib`, `[[refs.bib]]` or `![[refs.bib|alias]]`.
/// A wiki link that is not quoted in the front matter reads as a list inside a list, so one pair of
/// brackets is taken off as well.
pub fn target_of(value: &str) -> String {
    let v = value.trim().trim_start_matches('!');
    let inner = v.strip_prefix("[[").and_then(|r| r.strip_suffix("]]")).or_else(|| v.strip_prefix('[').and_then(|r| r.strip_suffix(']'))).unwrap_or(v);
    inner.split('|').next().unwrap_or(inner).trim().to_string()
}

/// The names that the property `bibliography:` of a note lists; empty when it has none.
pub fn property(text: &str) -> Vec<String> {
    let values = markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "bibliography").map(|(_, v)| v).unwrap_or_default();
    values.iter().map(|v| target_of(v)).filter(|t| !t.is_empty()).collect()
}

/// The files of the bibliography of the note `note` (its path and text) or, without a note, of the
/// vault's default. In a book the caller passes the top folder note (ADR-0039 §5: one bibliography
/// per book).
pub fn wanted(vault: &Vault, note: Option<(&Path, &str)>, default: &str) -> Wanted {
    let mut names = note.map(|(_, text)| property(text)).unwrap_or_default();
    if names.is_empty() {
        names.extend(Some(target_of(default)).filter(|t| !t.is_empty()));
    }
    let mut out = Wanted::default();
    for name in names {
        match vault.resolve(&name, note.map(|(rel, _)| rel)) {
            Some(rel) if !out.files.contains(&rel) => out.files.push(rel),
            Some(_) => {}
            None => out.problems.push(format!("The bibliography “{name}” is not a file of the vault.")),
        }
    }
    out
}
