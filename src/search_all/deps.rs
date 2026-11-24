//! Which notes are blocked and which block (QUERY-002 `blocked` and `blocking`, BOARD-023). A
//! note names its blockers in its front matter, `blocked-by: [[A]], [[B]]` (a comma-separated
//! list of links, or a list of `- [[A]]` lines); the links are read by name, as the vault does
//! (the folder, `#heading` and `|alias` of a link are left out). The rules of what is blocked
//! are here and nowhere else: the board (BOARD-024) builds [`Deps`] over its notes with the
//! lanes that hold finished cards, and does not repeat them.

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
    // (most notes name no blockers: their front matter is read only when the word is there)
    if !text.contains("blocked-by") {
        return out;
    }
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

/// Is the note done, so that it blocks nothing? It has `status: done`, or it is in a lane of a
/// board that holds finished cards (`in_done_lane`, BOARD-024). Outside a board there are no
/// lanes: the search passes `false`.
pub fn is_done(text: &str, in_done_lane: bool) -> bool {
    in_done_lane || front_matter_values(text).iter().any(|(key, values)| key == "status" && values.iter().any(|v| v.trim().eq_ignore_ascii_case("done")))
}

/// What the notes block, by the index of the note in the list the [`Deps`] were made from.
pub struct Deps {
    /// the notes each note names as blockers that exist, each once
    blockers: Vec<Vec<usize>>,
    done: Vec<bool>,
    blocked: Vec<bool>,
    blocking: Vec<bool>,
}

impl Deps {
    /// The notes of a snapshot; no lane of a board makes a note done here.
    pub fn of(snapshot: &Snapshot) -> Deps {
        let items: Vec<(String, Option<&str>)> = snapshot.notes.iter().map(|n| (n.file.display_name(), n.note.as_ref().map(|n| n.text.as_str()))).collect();
        Deps::build(&items, &vec![false; items.len()])
    }

    /// The rules of QUERY-002 and BOARD-023 over `notes`: the name and the text of each (a file
    /// that is no note has no text and is not named by a link) and for each whether it is in a
    /// lane that holds finished cards. A note is blocked while a blocker it names exists and is
    /// not done. A note is blocking while it is not done and a note that is not done names it.
    pub fn build(notes: &[(String, Option<&str>)], in_done_lane: &[bool]) -> Deps {
        let mut names: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, (name, _)) in notes.iter().enumerate().filter(|(_, (_, text))| text.is_some()) {
            names.entry(link_key(name)).or_default().push(i);
        }
        let done: Vec<bool> = notes.iter().zip(in_done_lane).map(|((_, text), lane)| text.is_some_and(|t| is_done(t, *lane))).collect();
        let blockers: Vec<Vec<usize>> = notes
            .iter()
            .map(|(_, text)| {
                let mut found: Vec<usize> = text.map(blocked_by).unwrap_or_default().iter().filter_map(|k| names.get(k)).flatten().copied().collect();
                found.sort_unstable();
                found.dedup();
                found
            })
            .collect();
        let (mut blocked, mut blocking) = (vec![false; notes.len()], vec![false; notes.len()]);
        for (i, list) in blockers.iter().enumerate() {
            for blocker in list.iter().copied().filter(|j| !done[*j]) {
                blocked[i] = true;
                blocking[blocker] |= !done[i];
            }
        }
        Deps { blockers, done, blocked, blocking }
    }

    pub fn is_blocked(&self, note: usize) -> bool {
        self.blocked[note]
    }

    pub fn is_blocking(&self, note: usize) -> bool {
        self.blocking[note]
    }

    /// The blockers of `note` that exist, done or not.
    pub fn blockers(&self, note: usize) -> &[usize] {
        &self.blockers[note]
    }

    /// The blockers of `note` that are not done: what it waits for.
    pub fn waiting(&self, note: usize) -> impl Iterator<Item = usize> + '_ {
        self.blockers[note].iter().copied().filter(|j| !self.done[*j])
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
        assert!(is_done("---\nstatus: Done\n---\n", false));
        assert!(!is_done("---\nstatus: doing\n---\n", false));
        assert!(!is_done("status: done\n", false), "outside the front matter it is text");
        assert!(!is_done("# Note\n", false));
    }

    // spec: BOARD-024
    #[test]
    fn board_024_a_note_in_a_done_lane_is_done_whatever_its_status() {
        assert!(is_done("---\nstatus: doing\n---\n", true));
        assert!(is_done("# Note\n", true));
    }

    // spec: BOARD-023, BOARD-024
    #[test]
    fn board_024_a_blocker_in_a_done_lane_blocks_nothing() {
        let a = "---\nstatus: doing\n---\n";
        let b = "---\nblocked-by: [[A]]\n---\n";
        let notes = [("A".to_string(), Some(a)), ("B".to_string(), Some(b)), ("pic.png".to_string(), None)];
        let open = Deps::build(&notes, &[false; 3]);
        assert!(open.is_blocked(1) && open.is_blocking(0) && !open.is_blocked(0));
        assert_eq!((open.blockers(1), open.waiting(1).collect::<Vec<_>>()), (&[0][..], vec![0]));
        let lane = Deps::build(&notes, &[true, false, false]);
        assert!(!lane.is_blocked(1) && !lane.is_blocking(0), "A is in a done lane");
        assert_eq!((lane.blockers(1), lane.waiting(1).count()), (&[0][..], 0), "the arrow stays, the waiting is over");
    }
}
