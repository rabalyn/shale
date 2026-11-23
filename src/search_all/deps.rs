//! Which notes are blocked and which block (QUERY-002 `blocked` and `blocking`, BOARD-023). A
//! note names its blockers in its front matter, `blocked-by: [[A]], [[B]]` (a comma-separated
//! list of links, or a list of `- [[A]]` lines); the links are read by name, as the vault does
//! (the folder, `#heading` and `|alias` of a link are left out). The rules of what is blocked
//! are here and nowhere else, so that the board (#603) extends [`is_done`] and not the search.

use super::Snapshot;
use crate::markdown::front_matter_values;
use std::collections::HashMap;

/// The key of a note's name for the comparison of names: no folder, `#heading`, `|alias` or
/// `.md`, lower case.
pub fn link_key(name: &str) -> String {
    let name = name.split('|').next().unwrap_or_default();
    let name = name.split('#').next().unwrap_or_default();
    let name = name.rsplit('/').next().unwrap_or_default().trim().to_lowercase();
    name.strip_suffix(".md").map_or(name.clone(), str::to_string)
}

/// The keys of the notes `blocked-by:` of the front matter of `text` names.
pub fn blocked_by(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (key, values) in front_matter_values(text) {
        if key != "blocked-by" {
            continue;
        }
        for value in values {
            // (a value `[[A]], [[B]]` reads as a list of brackets; the brackets are what is left to strip)
            out.extend(value.replace(['[', ']'], "").split(',').map(link_key).filter(|k| !k.is_empty()));
        }
    }
    out
}

/// Is the note done, so that it blocks nothing? Today: it has `status: done`.
pub fn is_done(text: &str) -> bool {
    front_matter_values(text).iter().any(|(key, values)| key == "status" && values.iter().any(|v| v.trim().eq_ignore_ascii_case("done")))
}

/// What the notes of a snapshot block, by the index of the note in the snapshot.
pub struct Deps {
    blocked: Vec<bool>,
    blocking: Vec<bool>,
}

impl Deps {
    /// A note is blocked while a blocker it names exists and is not done. A note is blocking
    /// while it is not done and a note that is not done names it.
    pub fn of(snapshot: &Snapshot) -> Deps {
        let notes = &snapshot.notes;
        let mut names: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, n) in notes.iter().enumerate().filter(|(_, n)| n.note.is_some()) {
            names.entry(link_key(&n.file.display_name())).or_default().push(i);
        }
        let done: Vec<bool> = notes.iter().map(|n| n.note.as_ref().is_some_and(|n| is_done(&n.text))).collect();
        let (mut blocked, mut blocking) = (vec![false; notes.len()], vec![false; notes.len()]);
        for (i, n) in notes.iter().enumerate() {
            let Some(n) = &n.note else { continue };
            for blocker in blocked_by(&n.text).iter().filter_map(|k| names.get(k)).flatten().copied().filter(|j| !done[*j]) {
                blocked[i] = true;
                blocking[blocker] |= !done[i];
            }
        }
        Deps { blocked, blocking }
    }

    pub fn is_blocked(&self, note: usize) -> bool {
        self.blocked[note]
    }

    pub fn is_blocking(&self, note: usize) -> bool {
        self.blocking[note]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: QUERY-002
    #[test]
    fn names_are_compared_without_folder_heading_alias_and_case() {
        assert_eq!(link_key("Folder/My Note.md#Head|alias"), "my note");
        assert_eq!(link_key("  Plain "), "plain");
        assert_eq!(link_key(""), "");
    }

    // spec: QUERY-002
    #[test]
    fn blocked_by_is_a_list_of_links_in_any_front_matter_shape() {
        let note = |fm: &str| format!("---\n{fm}\n---\nbody\n");
        assert_eq!(blocked_by(&note("blocked-by: [[A]], [[B]]")), ["a", "b"]);
        assert_eq!(blocked_by(&note("blocked-by: [[A]]")), ["a"]);
        assert_eq!(blocked_by(&note("blocked-by: [[A|x]],[[Dir/B#h]]")), ["a", "b"]);
        assert_eq!(blocked_by(&note("blocked-by:\n  - [[A]]\n  - \"[[B]]\"")), ["a", "b"]);
        assert_eq!(blocked_by(&note("blocked-by: [\"[[A]]\", \"[[B]]\"]")), ["a", "b"]);
        assert!(blocked_by(&note("blocked-by:")).is_empty());
        assert!(blocked_by(&note("status: doing")).is_empty());
        assert!(blocked_by("blocked-by: [[A]]\nno front matter").is_empty());
    }

    // spec: QUERY-002
    #[test]
    fn a_note_is_done_with_status_done() {
        assert!(is_done("---\nstatus: Done\n---\n"));
        assert!(!is_done("---\nstatus: doing\n---\n"));
        assert!(!is_done("status: done\n"), "outside the front matter it is text");
        assert!(!is_done("# Note\n"));
    }
}
