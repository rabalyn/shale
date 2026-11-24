//! What a card of a board knows of its note (BOARD-010, BOARD-016): the title, the chips and the
//! values the sort keys compare. A drawing is a card too, with a title and no other fields
//! (BOARD-011).

use crate::calendar::time::{self, parse_ymd};
use crate::markdown::front_matter_values;
use crate::note_props::{self, Priority};
use crate::search_all::matcher::due_day;
use crate::vault::{FileEntry, FileKind, Vault};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub rel: PathBuf,
    /// the title line of the note (VAULT-025)
    pub title: String,
    pub drawing: bool,
    /// the due day: `due:` of the front matter, else the earliest open task date of the note (BOARD-016)
    pub due: Option<i64>,
    pub status: Option<String>,
    pub priority: Option<Priority>,
    pub tags: Vec<String>,
    /// UTC seconds, with the fallbacks of BOARD-016
    pub created: Option<i64>,
    pub modified: Option<i64>,
}

fn secs(t: Option<SystemTime>) -> Option<i64> {
    t?.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// The `status:` of a note's front matter: its first value that is not blank.
fn status_of(text: &str) -> Option<String> {
    front_matter_values(text).into_iter().find(|(k, _)| k == "status").and_then(|(_, v)| v.into_iter().map(|x| x.trim().to_string()).find(|x| !x.is_empty()))
}

/// The earliest date of an open task of the note (BOARD-016, TASK-002).
fn task_due(tasks: &[crate::markdown::Task]) -> Option<i64> {
    tasks.iter().filter(|t| !t.done).filter_map(|t| t.due.as_deref().and_then(parse_ymd)).min()
}

impl Card {
    /// The card of the file `rel`; none when it is not in the vault or is no note or drawing.
    pub fn of(vault: &Vault, rel: &Path) -> Option<Card> {
        let file: &FileEntry = vault.file(rel)?;
        match file.kind {
            FileKind::Other => None,
            FileKind::Drawing => {
                Some(Card { rel: rel.to_path_buf(), title: file.title(), drawing: true, due: None, status: None, priority: None, tags: Vec::new(), created: secs(file.created.or(file.modified)), modified: secs(file.modified) })
            }
            FileKind::Note => {
                let note = vault.notes.get(rel)?;
                let text = note.text.as_str();
                let file_time = secs(file.created.or(file.modified));
                Some(Card {
                    rel: rel.to_path_buf(),
                    title: file.title(),
                    drawing: false,
                    due: due_day(text).or_else(|| task_due(&note.tasks)),
                    status: status_of(text),
                    priority: note_props::priority(text),
                    tags: note.tags.clone(),
                    created: note_props::created(rel, text, file_time),
                    modified: note_props::modified(text, secs(file.modified)),
                })
            }
        }
    }

    /// Is the due day over today?
    pub fn overdue(&self) -> bool {
        self.due.is_some_and(|d| d < time::today())
    }
}

/// A day as the chip shows it, `2026-12-01`.
pub fn day_text(day: i64) -> String {
    let (y, m, d) = time::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-card-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for (name, text) in files {
            std::fs::write(d.join(name), text).unwrap();
        }
        (Vault::open(&d).unwrap(), d)
    }

    // spec: BOARD-010
    #[test]
    fn board_010_a_card_has_the_chips_of_its_note() {
        let (v, d) = vault("chips", &[("240101_Task.md", "---\nstatus: doing\npriority: high\ndue: 2026-12-01\n---\nBody #todo\n")]);
        let c = Card::of(&v, Path::new("240101_Task.md")).unwrap();
        assert_eq!((c.title.as_str(), c.status.as_deref(), c.priority, c.due, c.drawing), ("Task", Some("doing"), Some(Priority::High), parse_ymd("2026-12-01"), false));
        assert!(c.tags.iter().any(|t| t == "todo"), "{:?}", c.tags);
        assert_eq!(day_text(c.due.unwrap()), "2026-12-01");
        assert_eq!(c.overdue(), c.due.unwrap() < time::today());
        let _ = std::fs::remove_dir_all(d);
    }

    // spec: BOARD-016
    #[test]
    fn board_016_due_is_the_property_else_the_earliest_open_task() {
        let (v, d) = vault(
            "due",
            &[("A.md", "---\ndue: 2026-12-05\n---\n- [ ] later 📅 2026-12-01\n"), ("B.md", "- [x] done 📅 2026-01-01\n- [ ] second 📅 2026-12-09\n- [ ] first 📅 2026-12-02\n- [ ] none\n"), ("C.md", "- [x] done 📅 2026-01-01\nplain\n")],
        );
        let due = |n: &str| Card::of(&v, Path::new(n)).unwrap().due.map(day_text);
        assert_eq!(due("A.md").as_deref(), Some("2026-12-05"));
        assert_eq!(due("B.md").as_deref(), Some("2026-12-02"), "open tasks only, the earliest");
        assert_eq!(due("C.md"), None);
        let _ = std::fs::remove_dir_all(d);
    }

    // spec: BOARD-016
    #[test]
    fn board_016_created_and_modified_fall_back_to_the_name_prefix_and_the_file() {
        let (v, d) = vault("times", &[("240115_Dated.md", "x\n"), ("Plain.md", "---\ncreated: 2025-03-04T10:00:00+00:00\nmodified: 2025-03-05T10:00:00+00:00\n---\n"), ("Bare.md", "x\n")]);
        let c = |n: &str| Card::of(&v, Path::new(n)).unwrap();
        assert_eq!(c("Plain.md").created, note_props::parse_timestamp("2025-03-04T10:00:00+00:00"));
        assert_eq!(c("Plain.md").modified, note_props::parse_timestamp("2025-03-05T10:00:00+00:00"));
        assert_eq!(c("240115_Dated.md").created, note_props::created(Path::new("240115_Dated.md"), "", None), "the name prefix, not the file time");
        assert!(c("Bare.md").created.is_some() && c("Bare.md").modified.is_some(), "the file time");
        let _ = std::fs::remove_dir_all(d);
    }

    // spec: BOARD-011
    #[test]
    fn board_011_a_drawing_is_a_card_with_a_title_and_no_fields() {
        let (v, d) = vault("drawing", &[("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE), ("Pic.png", "x")]);
        let c = Card::of(&v, Path::new("Sketch.excalidraw")).unwrap();
        assert!(c.drawing && c.title == "Sketch" && c.due.is_none() && c.status.is_none() && c.priority.is_none() && c.tags.is_empty());
        assert!(Card::of(&v, Path::new("Pic.png")).is_none(), "a picture is no card");
        assert!(Card::of(&v, Path::new("Nothing.md")).is_none());
        let _ = std::fs::remove_dir_all(d);
    }
}
