//! Dragging in the file list (IMP-017) in the headless whole window: a note or folder is picked
//! up with the pointer, carried over rows, and let go; the tests look at the hint under the
//! pointer and at the files on disk afterwards.

use super::drops_tests::W;
use crate::ui_test_support::Headless;
use eframe::egui::{Pos2, Vec2};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FILES: &[(&str, &str)] = &[("Sub/Deep/Leaf.md", "# Leaf\n"), ("Sub/Welcome.md", "# The other Welcome\n"), ("Top.md", "# Top\n")];

impl W {
    fn root(&self) -> PathBuf {
        self.app.vault.as_ref().unwrap().root.clone()
    }

    /// The place of a row: a folder, or a note by its vault path.
    fn row(&self, rel: &str) -> Pos2 {
        let p = Path::new(rel);
        if p.extension().is_some() {
            self.zone(&p.parent().unwrap().to_string_lossy(), Some(rel), true).center()
        } else {
            self.zone(rel, None, true).center()
        }
    }

    /// Pick up the row at `from` and carry it over `to` (the button stays down).
    fn carry(&mut self, from: Pos2, to: Pos2) {
        let mut path = vec![from + Vec2::new(0.0, 12.0)];
        path.extend(Headless::steps(from, to, 4));
        self.hl.press_and_move(&mut self.app, from, &path);
    }

    /// A place of the editor: inside the window, on no row of the file list.
    fn editor(&self) -> Pos2 {
        let open = self.app.current_path().expect("a note is open");
        self.zone(&open.parent().unwrap().to_string_lossy(), Some(&open.to_string_lossy()), false).center()
    }

    fn exist(&self, rels: &[&str]) -> Vec<bool> {
        rels.iter().map(|r| self.root().join(r).exists()).collect()
    }
}

/// A vault with a folder inside a folder, open down to the leaf.
fn window(tag: &str) -> W {
    let mut w = W::with_files(tag, FILES);
    w.app.tree.expanded.insert(PathBuf::from("Sub"));
    w.app.tree.expanded.insert(PathBuf::from("Sub/Deep"));
    w.frames(4);
    w
}

// spec: IMP-017
#[test]
fn imp_017_a_folder_can_not_go_into_itself_or_into_its_own_child() {
    let mut w = window("tree-self");
    let sub = w.row("Sub");
    let deep = w.row("Sub/Deep");
    // onto its own child
    w.carry(sub, deep);
    assert!(w.painted("“Sub”: a folder can't go into itself"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, deep);
    w.frames(3);
    assert_eq!(w.exist(&["Sub/Deep/Leaf.md", "Sub/Inner.md", "Sub/Welcome.md"]), [true, true, true]);
    assert!(w.app.toasts.is_empty(), "{:?}", w.app.toasts);
    // onto itself
    let sub = w.row("Sub");
    w.carry(sub, sub + Vec2::new(30.0, 0.0));
    assert!(w.painted("“Sub”: a folder can't go into itself"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, sub);
    w.frames(3);
    assert_eq!(w.exist(&["Sub/Inner.md", "Sub/Deep"]), [true, true]);
    assert!(w.app.toasts.is_empty(), "{:?}", w.app.toasts);
}

// spec: IMP-017
#[test]
fn imp_017_dropping_where_it_already_is_changes_nothing() {
    let mut w = window("tree-here");
    // a note onto the folder it is in
    let inner = w.row("Sub/Inner.md");
    let sub = w.row("Sub");
    w.carry(inner, sub);
    assert!(w.painted("“Inner”: already here"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, sub);
    w.frames(3);
    // and onto a note next to it
    let sibling = w.row("Sub/Welcome.md");
    w.carry(inner, sibling);
    assert!(w.painted("“Inner”: already here"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, sibling);
    w.frames(3);
    assert_eq!(w.exist(&["Sub/Inner.md"]), [true]);
    assert!(w.app.toasts.is_empty(), "{:?}", w.app.toasts);
    // a note of the top onto the empty space of the list: also where it is
    let top = w.row("Top.md");
    let empty = Pos2::new(top.x, top.y + 400.0);
    w.carry(top, empty);
    assert!(w.painted("“Top”: already here"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, empty);
    w.frames(3);
    assert!(w.root().join("Top.md").is_file() && w.app.toasts.is_empty());
}

// spec: IMP-017
#[test]
fn imp_017_a_name_that_is_taken_is_refused_and_both_files_stay() {
    let mut w = window("tree-clash");
    let welcome = w.row("Welcome.md");
    let sub = w.row("Sub");
    w.carry(welcome, sub);
    assert!(w.painted("Move “Welcome” into “Sub”"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, sub);
    w.frames(3);
    let said = w.app.toasts.iter().any(|(t, _)| t.contains("Could not move") && t.contains("already has a “Welcome.md”"));
    assert!(said, "{:?}", w.app.toasts);
    assert_eq!(std::fs::read_to_string(w.root().join("Welcome.md")).unwrap(), "# Welcome\n\nHello\n");
    assert_eq!(std::fs::read_to_string(w.root().join("Sub/Welcome.md")).unwrap(), "# The other Welcome\n");
}

// spec: IMP-017
#[test]
fn imp_017_a_note_dropped_on_a_folder_a_note_or_the_empty_list_moves_there() {
    let mut w = window("tree-move");
    // on a note in another folder: next to it
    let top = w.row("Top.md");
    let leaf = w.row("Sub/Deep/Leaf.md");
    w.carry(top, leaf);
    assert!(w.painted("Move “Top” into “Sub/Deep”"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, leaf);
    let root = w.root();
    w.until("Top moved", |_| root.join("Sub/Deep/Top.md").is_file());
    assert!(!root.join("Top.md").exists());
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("Moved “Top” to “Sub/Deep”")), "{:?}", w.app.toasts);
    // a folder with its content onto the top of the list: the whole folder goes
    w.frames(4);
    let deep = w.row("Sub/Deep");
    let welcome = w.row("Welcome.md");
    w.carry(deep, welcome);
    assert!(w.painted("Move “Deep” into the top of the vault"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, welcome);
    w.until("Deep moved", |_| root.join("Deep/Leaf.md").is_file());
    assert_eq!(w.exist(&["Sub/Deep", "Deep/Top.md"]), [false, true]);
    // onto the empty space of the list: the top of the vault
    w.frames(4);
    let inner = w.row("Sub/Inner.md");
    let empty = Pos2::new(inner.x, inner.y + 400.0);
    w.carry(inner, empty);
    assert!(w.painted("Move “Inner” into the top of the vault"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, empty);
    w.until("Inner moved", |_| root.join("Inner.md").is_file());
    assert!(!root.join("Sub/Inner.md").exists());
}

// spec: IMP-017
#[test]
fn imp_017_off_the_list_nothing_moves_and_a_closed_folder_opens_under_the_pointer() {
    let mut w = window("tree-hover");
    w.app.tree.expanded.clear();
    w.frames(3);
    let top = w.row("Top.md");
    // let go over the editor: not a place for it
    let editor = w.editor();
    w.carry(top, editor);
    assert!(w.painted("Move “Top”: drop it on a folder"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, editor);
    w.frames(3);
    assert!(w.root().join("Top.md").is_file() && !w.app.toasts.iter().any(|(t, _)| t.contains("Moved")));
    // resting on a closed folder opens it (once the pointer has stayed there long enough)
    let sub = w.row("Sub");
    w.carry(top, sub);
    assert!(!w.app.tree.expanded.contains(Path::new("Sub")));
    assert!(matches!(&w.app.drag.tree_hover, Some((f, _)) if f == Path::new("Sub")));
    w.app.drag.tree_hover = Some((PathBuf::from("Sub"), Instant::now() - Duration::from_secs(2)));
    w.frames(2);
    assert!(w.app.tree.expanded.contains(Path::new("Sub")));
    assert!(w.hl.text.iter().any(|(t, _)| t == "Inner"), "its notes are listed now");
    // moving off it forgets the rest
    w.hl.release(&mut w.app, editor);
    w.frames(2);
    assert!(w.app.drag.tree_hover.is_none());
}
