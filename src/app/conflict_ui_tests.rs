//! Conflict copies in the whole window (NET-015, headless): the badge in the file tree, the
//! banner above the copy, the comparison, and both ways to keep one.

use super::{Modal, Options, ShaleApp, View};
use crate::ui_test_support::Headless;
use eframe::egui::PointerButton;
use std::path::{Path, PathBuf};

const COPY: &str = "Plan (conflict 2026-11-21 1424).md";
const LATER: &str = "Plan (conflict 2026-11-22 0900).md";
const ORPHAN: &str = "Lost (conflict 2026-11-21 1424).md";
const ORIGINAL_TEXT: &str = "# Plan\n\nWe go on Friday.\n";
const COPY_TEXT: &str = "# Plan\n\nWe go on Saturday.\n";

struct W {
    app: ShaleApp,
    hl: Headless,
    root: PathBuf,
}

impl W {
    fn new(tag: &str) -> W {
        let root = std::env::temp_dir().join(format!("shale-conflict-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        for (n, t) in [("Plan.md", ORIGINAL_TEXT), (COPY, COPY_TEXT), (ORPHAN, "orphan\n"), ("Other.md", "# Other\n")] {
            std::fs::write(root.join(n), t).unwrap();
        }
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(root.clone()), view: None, keys: None }, false);
        let mut w = W { app, hl: Headless::new(), root };
        w.hl.settle(&mut w.app);
        w
    }

    fn has_text(&self, part: &str) -> bool {
        self.hl.text.iter().any(|(s, _)| s.contains(part))
    }

    fn press(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    fn open(&mut self, name: &str) {
        self.app.open(Path::new(name), false);
        self.hl.frames(&mut self.app, 3);
    }

    fn view(&self) -> Option<PathBuf> {
        match &self.app.tabbar.tabs[self.app.tabbar.active].view {
            View::Note(p) => Some(p.clone()),
            _ => None,
        }
    }

    fn edit(&mut self, name: &str, text: &str) {
        let doc = self.app.notes.get_mut(Path::new(name)).unwrap();
        doc.text = text.into();
        doc.dirty = true;
    }

    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.root.join(name)).unwrap()
    }

    /// Whether the trash holds a file of this name.
    fn in_trash(&self, name: &str) -> bool {
        let mut stack = vec![self.root.join(".trash")];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                if e.path().is_dir() {
                    stack.push(e.path());
                } else if e.file_name().to_string_lossy() == name {
                    return true;
                }
            }
        }
        false
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

// spec: NET-015
#[test]
fn net_015_the_file_tree_marks_a_conflict_copy_and_the_note_has_a_banner() {
    let mut w = W::new("banner");
    assert!(w.hl.text.iter().filter(|(s, _)| s == "conflict").count() == 2, "a badge on the copy and on the orphan: {:?}", w.hl.text);
    w.open(COPY);
    assert!(w.has_text("Another device changed this note at the same time"), "{:?}", w.hl.text);
    assert!(w.hl.try_label(&mut w.app, "Compare").is_some() && w.hl.try_label(&mut w.app, "Keep this one").is_some() && w.hl.try_label(&mut w.app, "Keep the other").is_some());
    // an ordinary note has none
    w.open("Plan.md");
    assert!(!w.has_text("Another device changed") && w.hl.try_label(&mut w.app, "Compare").is_none());
}

// spec: NET-015
#[test]
fn net_015_a_copy_without_an_original_has_a_notice_and_no_buttons() {
    let mut w = W::new("orphan");
    w.open(ORPHAN);
    assert!(w.has_text("the note it belongs to is not in this folder"), "{:?}", w.hl.text);
    assert!(w.hl.try_label(&mut w.app, "Keep this one").is_none() && w.hl.try_label(&mut w.app, "Compare").is_none());
}

// spec: NET-015
#[test]
fn net_015_compare_shows_the_two_texts_in_both_modes() {
    let mut w = W::new("compare");
    w.open(COPY);
    w.press("Compare");
    assert!(matches!(w.app.modal, Some(Modal::Conflict(_))));
    w.hl.frames(&mut w.app, 2);
    assert!(w.has_text("Saturday") && w.has_text("Friday"), "the changes view shows what was added and removed: {:?}", w.hl.text);
    assert!(w.has_text("Left: \u{201c}Plan.md\u{201d}"));
    w.press("Source, side by side");
    w.hl.frames(&mut w.app, 2);
    assert!(w.has_text("Saturday") && w.has_text("Friday"), "{:?}", w.hl.text);
    // nothing changed on disk by looking
    assert_eq!((w.read("Plan.md").as_str(), w.read(COPY).as_str()), (ORIGINAL_TEXT, COPY_TEXT));
}

// spec: NET-015
#[test]
fn net_015_keep_this_one_puts_the_copys_text_in_the_original_and_trashes_the_copy() {
    let mut w = W::new("keep-this");
    w.open("Plan.md");
    // an unsaved edit in the original is saved first, then replaced
    w.edit("Plan.md", "# Plan\n\nunsaved edit\n");
    w.open(COPY);
    w.press("Compare");
    w.press("Keep this copy");
    assert_eq!(w.read("Plan.md"), COPY_TEXT);
    assert!(!w.root.join(COPY).exists() && w.in_trash(COPY), "the copy is in the trash, not deleted");
    assert_eq!(w.view().as_deref(), Some(Path::new("Plan.md")));
    let doc = &w.app.notes[Path::new("Plan.md")];
    assert!(doc.text == COPY_TEXT && !doc.dirty, "the open buffer has the text and is saved");
    assert!(w.app.modal.is_none());
    assert!(!w.app.vault.as_ref().unwrap().files.iter().any(|f| f.rel == Path::new(COPY)));
}

// spec: NET-015
#[test]
fn net_015_keep_this_one_takes_the_edited_text_of_the_open_copy_and_writes_an_unopened_original() {
    let mut w = W::new("keep-this-edited");
    w.open(COPY);
    w.edit(COPY, "# Plan\n\nedited in the copy\n");
    w.press("Keep this one");
    assert_eq!(w.read("Plan.md"), "# Plan\n\nedited in the copy\n");
    assert!(!w.root.join(COPY).exists() && w.in_trash(COPY));
    assert_eq!(w.view().as_deref(), Some(Path::new("Plan.md")));
    assert!(w.has_text("edited in the copy"), "the original is shown with the text: {:?}", w.hl.text);
}

// spec: NET-015
#[test]
fn net_015_keep_the_other_leaves_the_original_and_trashes_the_copy() {
    let mut w = W::new("keep-other");
    w.open("Plan.md");
    w.open(COPY);
    w.edit(COPY, "# Plan\n\nedit that is dropped\n");
    w.press("Keep the other");
    assert_eq!(w.read("Plan.md"), ORIGINAL_TEXT);
    assert!(!w.root.join(COPY).exists() && w.in_trash(COPY));
    assert!(!w.app.notes.contains_key(Path::new(COPY)));
    assert_ne!(w.view().as_deref(), Some(Path::new(COPY)));
    // the dialog's own button does the same
    std::fs::write(w.root.join(LATER), "# Plan\n\nlater\n").unwrap();
    w.app.vault.as_mut().unwrap().rescan();
    w.open(LATER);
    w.press("Compare");
    w.press("Keep the original");
    assert!(w.app.modal.is_none() && !w.root.join(LATER).exists());
    assert_eq!(w.read("Plan.md"), ORIGINAL_TEXT);
}
