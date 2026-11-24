//! What Settings says about the bibliography (EDIT-123): the state of the files of the vault's
//! default and of the open note, and which keys the note cites that the bibliography lacks. The
//! keys come from the scanner of EDIT-122; the marks in the editor are those of EDIT-126
//! (`editor/cite.rs`), and this is the same question asked of the whole text, for the status.

use super::reader::Entry;
use super::Effect;
use crate::markdown::citations::{scan, Target, Token};

/// The citation keys a note writes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cited {
    /// each key once, in the order of the text
    pub keys: Vec<String>,
    /// the keys among them that the bibliography in effect lacks (empty while none is in effect)
    pub unknown: Vec<String>,
}

/// The keys that the text of a note cites (bracketed groups, and bare keys the bibliography holds).
pub fn cited(text: &str, effect: &Effect) -> Cited {
    let has = |k: &str| effect.has_key(k);
    let mut out = Cited::default();
    for token in scan(text, effect.in_effect().then_some(&has as &dyn Fn(&str) -> bool)) {
        let Token::Group(group) = token else { continue };
        for item in group.items {
            let Target::Citation(key) = item.target else { continue };
            if out.keys.contains(&key) {
                continue;
            }
            if item.known == Some(false) {
                out.unknown.push(key.clone());
            }
            out.keys.push(key);
        }
    }
    out
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The files of an effect and what they hold, as a sentence: "refs.bib: 231 entries, 2 skipped as bad".
fn files_line(effect: &Effect) -> String {
    let files = effect.files.iter().map(|(rel, _)| rel.display().to_string()).collect::<Vec<_>>().join(", ");
    let skipped = effect.skipped();
    let uncitable: Vec<&Entry> = effect.files.iter().flat_map(|(_, f)| f.entries.iter()).filter(|e| e.problem.is_some()).collect();
    let mut line = format!("{files}: {}", plural(effect.entries().count(), "entry", "entries"));
    if skipped > 0 {
        line.push_str(&format!(", {skipped} skipped as bad"));
    }
    if let Some((first, problem)) = uncitable.first().and_then(|e| e.problem.map(|p| (e, p))) {
        line.push_str(&format!(", {} that cannot be cited (“{}”: {})", plural(uncitable.len(), "key", "keys"), first.key, problem.says()));
    }
    line
}

/// The lines Settings shows under the vault's default: the problems of its file, or what it holds.
pub fn default_lines(effect: &Effect) -> Vec<String> {
    let mut lines = effect.messages.clone();
    if effect.in_effect() {
        lines.push(files_line(effect));
    } else if effect.loading {
        lines.push("Reading the bibliography…".into());
    }
    lines
}

/// The lines for the open note: its bibliography (its own property or the default), and its citations.
pub fn note_lines(name: &str, text: &str, effect: &Effect) -> Vec<String> {
    let mut lines = effect.messages.clone();
    let cited = cited(text, effect);
    if effect.in_effect() {
        let mut line = format!("{name} uses {} and cites {}", effect.files.iter().map(|(rel, _)| rel.display().to_string()).collect::<Vec<_>>().join(", "), plural(cited.keys.len(), "key", "keys"));
        if !cited.unknown.is_empty() {
            let shown = cited.unknown.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
            line.push_str(&format!("; not in the bibliography: {shown}{}", if cited.unknown.len() > 5 { ", …" } else { "" }));
        }
        lines.push(line);
    } else if effect.loading {
        lines.push("Reading the bibliography…".into());
    } else if !cited.keys.is_empty() {
        lines.push(format!("{name} cites {} and has no bibliography in effect.", plural(cited.keys.len(), "key", "keys")));
    }
    lines
}
