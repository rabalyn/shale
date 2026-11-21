//! The history panel and the comparison view in the whole window (headless), against a real
//! history: both placements, hiding, the list, the comparison and its switch, the actions (HIST-*).

use super::history_tests::{setup, write, T};
use super::*;
use crate::app::Modal;
use crate::history::place::Place;
use crate::history::state::{Data, Mode};
use crate::ui_test_support::Headless;
use eframe::egui::{Event, PointerButton, Pos2, Rect, Vec2};
use std::time::Instant;

const V1: &str = "# Trip\n\nWe go on Friday.\n\n- tent\n- boat\n\nPack warm.\n\nBye.\n";
const V2: &str = "# Trip\n\nWe go on Saturday.\n\n- tent\n- boat\n- food\n\nPack warm.\n\nBye.\n";
const V3: &str = "# Trip plan\n\nWe go on Saturday.\n\n- tent\n- boat\n- food\n\nPack warm.\n\nBye now.\n";

struct W {
    t: T,
    hl: Headless,
    /// every text painted in any frame since the last `forget`
    seen: Vec<String>,
    /// what the frames copied to the clipboard
    copied: Vec<String>,
}

impl W {
    /// A window of this size on a vault with the note Trip in three versions, open in one pane.
    fn new(tag: &str, size: Vec2) -> W {
        let mut t = setup(tag, false);
        write(&t.vault, "Trip.md", V1);
        t.app.vault.as_mut().unwrap().rescan();
        t.start();
        t.app.open(Path::new("Trip.md"), false);
        t.type_into("Trip.md", V2);
        t.settle();
        t.at(2_000);
        t.type_into("Trip.md", V3);
        t.settle();
        let mut hl = Headless::new();
        hl.set_size(size);
        let mut w = W { t, hl, seen: vec![], copied: vec![] };
        w.settle();
        w
    }

    fn frame(&mut self, events: Vec<Event>) {
        let out = self.hl.frame(&mut self.t.app, events);
        self.seen.extend(self.hl.text.iter().map(|(s, _)| s.clone()));
        for c in &out.platform_output.commands {
            if let egui::OutputCommand::CopyText(s) = c {
                self.copied.push(s.clone());
            }
        }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    /// Frames until no job is left, then a few more.
    fn settle(&mut self) {
        let start = Instant::now();
        self.frames(2);
        while !self.t.app.history_quiet() {
            assert!(start.elapsed().as_secs() < 30, "the history did not settle");
            std::thread::sleep(Duration::from_millis(5));
            self.frame(vec![]);
        }
        self.frames(3);
    }

    fn click(&mut self, p: Pos2) {
        let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![b(true)]);
        self.frame(vec![b(false)]);
        self.settle();
    }

    fn panel_rect(&self, key: &str) -> Rect {
        self.t.app.history.panel.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", self.t.app.history.panel.rects)).1
    }

    fn changes_rect(&self, key: &str) -> Rect {
        self.t.app.history.changes.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", self.t.app.history.changes.rects)).1
    }

    fn has_text(&self, part: &str) -> bool {
        self.hl.text.iter().any(|(s, _)| s.contains(part))
    }

    fn place(&self) -> Option<Place> {
        self.t.app.history.panel.place
    }
}

const WIDE: Vec2 = Vec2::new(1900.0, 900.0);
const NARROW: Vec2 = Vec2::new(1100.0, 900.0);

// spec: HIST-001, HIST-004, HIST-005, HIST-006
#[test]
fn hist_001_one_note_in_a_wide_window_has_the_history_in_the_margin() {
    let w = W::new("margin", WIDE);
    assert_eq!(w.place(), Some(Place::Margin));
    let (list, right) = (w.panel_rect("row-0"), w.t.app.side.right_rect.unwrap());
    assert!(list.max.x <= right.min.x, "the column is left of the right sidebar: {list:?} {right:?}");
    assert!(list.min.x >= right.min.x - 240.0, "and is 236 points wide at the most: {list:?} {right:?}");
    // the list: a day heading, the time and the source in words, the size of the change
    assert!(w.has_text("History") && w.has_text("Today"), "{:?}", w.hl.text);
    assert!(w.has_text("just now") || w.has_text(":"), "a time");
    assert!(w.has_text("edited here") && w.has_text("first version kept"), "the sources in words: {:?}", w.hl.text);
    assert!(w.has_text("+2 \u{2212}2") || w.has_text("+2"), "the size of the newest change as text: {:?}", w.hl.text);
    assert!(w.t.app.history.panel.versions.len() == 3);
}

// spec: HIST-001, HIST-002
#[test]
fn hist_002_a_narrow_window_has_no_column_and_the_panel_lives_in_the_right_sidebar() {
    let mut w = W::new("sidebar", NARROW);
    assert_eq!(w.place(), Some(Place::Sidebar));
    assert!(!w.t.app.history.panel.rects.iter().any(|r| r.0 == "hide"), "no column, no hide button");
    // the History icon of the sidebar (a panel like the outline) shows the list
    let icon = w.hl.find_label(&mut w.t.app, "File history of the note");
    w.click(icon.center());
    let (row, right) = (w.panel_rect("row-0"), w.t.app.side.right_rect.unwrap());
    assert!(right.contains_rect(row), "the list is in the right sidebar: {row:?} {right:?}");
    assert!(w.has_text("Today"));
}

// spec: HIST-001
#[test]
fn hist_001_two_notes_side_by_side_have_the_panel_in_the_sidebar_even_in_a_wide_window() {
    let mut w = W::new("two-panes", WIDE);
    assert_eq!(w.place(), Some(Place::Margin));
    w.t.app.open_in_split_right(crate::app::View::Note(PathBuf::from("Note.md")));
    w.frames(4);
    assert_eq!(w.t.app.panes.len(), 2);
    assert_eq!(w.place(), Some(Place::Sidebar), "several notes: the column goes");
    w.t.app.history_show(Path::new("Note.md"));
    w.settle();
    assert!(w.has_text("Today"), "the panel is shown in the sidebar after the menu entry");
    let right = w.t.app.side.right_rect.unwrap();
    assert!(right.contains_rect(w.panel_rect("deleted")), "the list of the focused note is in the sidebar");
}

// spec: HIST-003
#[test]
fn hist_003_the_margin_column_is_hidden_with_its_button_and_the_choice_is_remembered() {
    let mut w = W::new("hide", WIDE);
    assert_eq!(w.place(), Some(Place::Margin));
    let hide = w.panel_rect("hide");
    w.click(hide.center());
    assert!(!w.t.app.cfg.history.show_panel, "the choice is in the settings");
    w.frames(3);
    assert_eq!(w.place(), Some(Place::Sidebar));
    let text = crate::config::text::to_toml(&w.t.app.cfg);
    let (back, _) = crate::config::Config::parse_text(&text).unwrap();
    assert!(!back.history.show_panel, "and comes back at the next start");
    // the menu entry shows it again
    w.t.app.actions.push(crate::app::Action::History(PathBuf::from("Trip.md")));
    w.frames(4);
    assert!(w.t.app.cfg.history.show_panel);
    assert_eq!(w.place(), Some(Place::Margin));
    // the sidebar panel says the history is beside the note, and offers to take it
    w.t.app.show_history_in_sidebar();
    w.frames(3);
    assert!(w.has_text("shown beside it"), "{:?}", w.hl.text);
    let show = w.panel_rect("show-here");
    w.click(show.center());
    assert!(!w.t.app.cfg.history.show_panel);
    assert_eq!(w.place(), Some(Place::Sidebar));
    assert!(w.has_text("Today"), "the list is in the sidebar now");
}

// spec: HIST-007
#[test]
fn hist_007_the_list_is_read_in_the_background_and_again_after_a_new_version() {
    let mut w = W::new("loading", WIDE);
    // a new window on the same note: nothing is shown before the job is done
    w.t.app.history.panel = Default::default();
    w.frames(2);
    assert!(w.t.app.history.panel.loading && w.t.app.history.panel.versions.is_empty(), "asked in the frame, answered later");
    w.settle();
    assert_eq!(w.t.app.history.panel.versions.len(), 3);
    // a save that makes a version refreshes the list by itself
    w.t.at(9_000);
    w.t.type_into("Trip.md", "# Trip plan\n\nNothing.\n");
    w.settle();
    assert_eq!(w.t.app.history.panel.versions.len(), 4);
    assert!(w.has_text("+0") || w.has_text("\u{2212}"), "{:?}", w.hl.text);
}

// spec: HIST-007
#[test]
fn hist_007_a_note_without_versions_and_a_window_without_history_say_so_in_words() {
    let mut w = W::new("states", NARROW);
    w.t.app.cfg.history.enabled = false;
    w.t.app.show_history_in_sidebar();
    w.frames(3);
    assert!(w.has_text("The history is switched off"), "{:?}", w.hl.text);
    let b = w.panel_rect("settings");
    w.click(b.center());
    assert!(matches!(w.t.app.modal, Some(Modal::Settings { tab: crate::app::SettingsTab::Sync, .. })), "the button opens Settings → Sync");
    w.t.app.modal = None;
    w.t.app.cfg.history.enabled = true;
    // a note that was never saved has no versions yet
    write(&w.t.vault, "Fresh.md", "new\n");
    w.t.app.vault.as_mut().unwrap().rescan();
    w.t.app.open(Path::new("Fresh.md"), false);
    w.settle();
    assert!(w.has_text("No versions yet"), "{:?}", w.hl.text);
}

/// The window at the comparison of the oldest version with the note as it is now.
fn compare(tag: &str) -> W {
    let mut w = W::new(tag, WIDE);
    let oldest = w.t.app.history.panel.versions.len() - 1;
    let row = w.panel_rect(&format!("row-{oldest}"));
    w.click(row.center());
    w.settle();
    w
}

// spec: HIST-008, HIST-009, HIST-010, HIST-011
#[test]
fn hist_008_a_click_on_a_row_opens_the_changes_since_that_version_as_track_changes() {
    let w = compare("changes");
    assert!(matches!(w.t.app.modal, Some(Modal::Changes)));
    assert!(w.has_text("Changes since"), "{:?}", w.hl.text);
    assert_eq!(w.t.app.history.changes.mode, Mode::Changes);
    let Some(Data::Text(t)) = &w.t.app.history.changes.data else { panic!("text data") };
    assert!(t.old == V1 && t.new == V3, "the oldest version against the note now");
    // the rendered view: the words, the gutter marks, no Markdown marks
    assert!(w.has_text("Saturday") && w.has_text("Friday") && w.has_text("food"), "{:?}", w.hl.text);
    assert!(t.rows.iter().all(|r| r.spans.iter().all(|s| !s.text.contains('#'))), "headings have no hash marks");
    assert!(w.has_text("+") && w.has_text("~"), "the gutter marks of an added and a changed block");
    assert!(w.has_text("Today") || w.has_text(":"));
}

// spec: HIST-017
#[test]
fn hist_017_nothing_in_the_panel_or_the_changes_talks_like_a_git_tool() {
    let mut w = compare("words");
    w.frames(2);
    let mut texts = w.seen.clone();
    let source = w.changes_rect("mode-source");
    w.click(source.center());
    texts.extend(w.seen.clone());
    assert!(!texts.is_empty());
    for t in texts {
        let l = t.to_lowercase();
        for bad in ["commit", "head", "diff", "revision", "snapshot"] {
            // "ahead", "header" and the like are other words
            let found = l.split(|c: char| !c.is_alphanumeric()).any(|word| word == bad);
            assert!(!found, "{bad:?} in {t:?}");
        }
    }
}

// spec: HIST-012, HIST-013
#[test]
fn hist_012_the_switch_shows_the_source_side_by_side_and_copy_as_patch_copies_unified_diff_text() {
    let mut w = compare("source");
    assert!(!w.t.app.history.changes.rects.iter().any(|r| r.0 == "patch"), "no patch button in the rendered view");
    let source = w.changes_rect("mode-source");
    w.click(source.center());
    assert_eq!(w.t.app.history.changes.mode, Mode::Source);
    assert!(w.has_text("Bye now."), "the Markdown source is shown: {:?}", w.hl.text);
    assert!(w.has_text("# Trip plan") && w.has_text("# Trip"), "with its marks, on both sides");
    assert!(w.has_text("\u{2192}"), "and the two versions named by their short hashes");
    let patch = w.changes_rect("patch");
    w.click(patch.center());
    let copied = w.copied.last().cloned().expect("the patch is copied");
    assert!(copied.starts_with("--- Trip.md (") && copied.contains("+++ Trip.md (now)\n@@ -1,"), "{copied}");
    assert!(copied.contains("-We go on Friday.\n+We go on Saturday.\n") || copied.contains("-We go on Friday."), "{copied}");
    // and back
    let changes = w.changes_rect("mode-changes");
    w.click(changes.center());
    assert_eq!(w.t.app.history.changes.mode, Mode::Changes);
}

// spec: HIST-014
#[test]
fn hist_014_next_and_previous_change_move_through_the_changes() {
    let mut w = compare("steps");
    let n = w.t.app.history.changes.starts().len();
    assert!(n >= 3, "{n} changes");
    assert!(w.has_text(&format!("1 of {n}")), "{:?}", w.hl.text);
    let next = w.changes_rect("next");
    w.click(next.center());
    assert_eq!(w.t.app.history.changes.current, 1);
    assert!(w.has_text(&format!("2 of {n}")));
    let prev = w.changes_rect("prev");
    w.click(prev.center());
    assert_eq!(w.t.app.history.changes.current, 0);
    // at the end the next button does nothing
    for _ in 0..n + 2 {
        let next = w.changes_rect("next");
        w.click(next.center());
    }
    assert_eq!(w.t.app.history.changes.current, n - 1);
}

// spec: HIST-014
#[test]
fn hist_014_the_slider_moves_the_older_end_and_the_to_list_the_newer_one() {
    let mut w = compare("slider");
    let versions = w.t.app.history.panel.versions.clone();
    assert_eq!(w.t.app.history.changes.from, versions[2].commit);
    // click at the right end of the slider: the newest version is the older end
    let s = w.changes_rect("slider");
    w.click(Pos2::new(s.max.x - 2.0, s.center().y));
    assert_eq!(w.t.app.history.changes.from, versions[0].commit, "the right end is the newest version");
    let Some(Data::Text(t)) = &w.t.app.history.changes.data else { panic!("text data") };
    assert_eq!((t.old.as_str(), t.new.as_str()), (V3, V3), "read again for the new end");
    assert!(w.has_text("No changes"), "{:?}", w.hl.text);
    // and to the middle
    let s = w.changes_rect("slider");
    w.click(Pos2::new(s.min.x + 2.0, s.center().y));
    assert_eq!(w.t.app.history.changes.from, versions[2].commit);
    // the "to" list: a version later than the older end
    w.t.app.history.changes.to = Some(versions[1].commit.clone());
    w.t.app.history_compare();
    w.settle();
    let Some(Data::Text(t)) = &w.t.app.history.changes.data else { panic!("text data") };
    assert_eq!((t.old.as_str(), t.new.as_str()), (V1, V2));
    assert!(w.has_text("Changes from"), "{:?}", w.hl.text);
}

// spec: HIST-014
#[test]
fn hist_014_restore_this_version_from_the_changes_puts_it_back_and_one_undo_brings_the_text_back() {
    let mut w = compare("restore");
    let restore = w.changes_rect("restore");
    w.click(restore.center());
    assert!(w.t.app.modal.is_none(), "the view closes");
    assert_eq!(w.t.app.notes[Path::new("Trip.md")].text, V1);
    assert_eq!(crate::fsutil::read_to_string(w.t.vault.join("Trip.md")).unwrap(), V1);
    let mut text = w.t.app.notes[Path::new("Trip.md")].text.clone();
    assert!(w.t.app.undo.of(Path::new("Trip.md")).undo(&mut text).is_some());
    assert_eq!(text, V3, "one undo step");
}

// spec: HIST-014
#[test]
fn hist_014_copy_text_and_read_only_use_the_actions_of_the_app() {
    let mut w = compare("copy");
    w.t.app.ctx = Some(w.hl.ctx().clone());
    let copy = w.changes_rect("copy");
    w.click(copy.center());
    assert!(w.copied.iter().any(|c| c == V1), "the text of the older version is copied: {:?}", w.copied);
    let ro = w.changes_rect("read-only");
    w.click(ro.center());
    let (title, text) = w.t.app.history.changes.alone.clone().expect("the version alone");
    assert_eq!(text, V1);
    assert!(title.starts_with("Trip.md as of "), "{title}");
    assert!(w.has_text("Back to the changes"));
    let back = w.changes_rect("back");
    w.click(back.center());
    assert!(w.t.app.history.changes.alone.is_none());
}

// spec: HIST-015
#[test]
fn hist_015_a_binary_version_is_changed_with_two_placeholders() {
    let mut t = setup("binary", false);
    write(&t.vault, "Bin.md", "a\0b\n");
    t.app.vault.as_mut().unwrap().rescan();
    t.start();
    t.app.open(Path::new("Bin.md"), false);
    t.at(2_000);
    t.type_into("Bin.md", "a\0b\0c\n");
    t.settle();
    let mut hl = Headless::new();
    hl.set_size(WIDE);
    let mut w = W { t, hl, seen: vec![], copied: vec![] };
    w.settle();
    assert!(w.has_text("changed"), "no counts for a binary version: {:?}", w.hl.text);
    let newest = w.panel_rect("row-0");
    w.click(newest.center());
    w.settle();
    assert!(matches!(w.t.app.history.changes.data, Some(Data::Binary { .. })));
    assert!(w.has_text("not text") && w.has_text("Before") && w.has_text("After"), "{:?}", w.hl.text);
}

// spec: HIST-015
#[test]
fn hist_015_a_drawing_is_changed_with_the_drawing_before_and_after() {
    let mut t = setup("drawing", false);
    let scene = |n: usize| {
        let el = |i: usize| format!(r##"{{"id":"e{i}","type":"rectangle","x":{x},"y":10,"width":80,"height":40,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":2,"roughness":0,"opacity":100,"version":1,"versionNonce":1,"isDeleted":false,"seed":{i}}}"##, x = i * 100);
        format!(r#"{{"type":"excalidraw","version":2,"source":"test","elements":[{}],"appState":{{}},"files":{{}}}}"#, (0..n).map(el).collect::<Vec<_>>().join(","))
    };
    write(&t.vault, "Sketch.excalidraw", &scene(1));
    t.app.vault.as_mut().unwrap().rescan();
    t.start();
    t.app.open(Path::new("Sketch.excalidraw"), false);
    t.at(2_000);
    write(&t.vault, "Sketch.excalidraw", &scene(3));
    t.app.history.note_saved(Path::new("Sketch.excalidraw"));
    t.settle();
    let versions = t.versions("Sketch.excalidraw");
    assert_eq!(versions.len(), 2, "{versions:?}");
    t.app.open_history_changes(Path::new("Sketch.excalidraw"), &versions[1]);
    t.settle();
    let mut hl = Headless::new();
    hl.set_size(WIDE);
    let mut w = W { t, hl, seen: vec![], copied: vec![] };
    w.settle();
    assert!(matches!(w.t.app.history.changes.data, Some(Data::Drawing { .. })), "a drawing is not diffed as text");
    assert!(w.has_text("The drawing changed."), "{:?}", w.hl.text);
    assert!(w.has_text("1 elements") && w.has_text("3 elements"), "{:?}", w.hl.text);
}

// spec: HIST-017
#[test]
fn hist_017_the_menu_says_history_of_this_note() {
    let mut w = W::new("menu", WIDE);
    w.frames(2);
    assert!(w.t.app.history.panel.place.is_some());
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/layout.rs")).unwrap();
    assert!(src.contains("\"History of this note\"") && !src.contains("Versions of this note"));
}

// spec: HIST-009, HIST-011
#[test]
fn hist_011_the_marks_are_drawn_in_the_colours_of_a_dark_and_a_light_theme_and_always_with_a_glyph() {
    let mut w = compare("themes");
    let themes = crate::themes::Themes::load(None);
    for name in ["Shale", "Catppuccin Latte"] {
        let p = themes.get(name).unwrap().palette;
        assert_eq!(p.dark, name == "Shale");
        crate::theme::switch_palette(w.hl.ctx(), p);
        w.frames(3);
        let fills: Vec<egui::Color32> = w.hl.rects.iter().map(|r| r.1).collect();
        assert!(fills.contains(&p.good_bg), "{name}: the mark of an added block");
        assert!(fills.contains(&p.warn_bg), "{name}: the mark of a changed block");
        assert!(w.has_text("+") && w.has_text("~"), "{name}: the glyphs are there too, so it is not colour alone");
    }
}

// spec: SYNC-045, SYNC-040
#[test]
fn sync_045_the_deleted_notes_dialog_lists_a_trashed_note_and_restores_it() {
    let mut w = W::new("deleted-dialog", WIDE);
    w.t.app.trash(Path::new("Trip.md"));
    w.settle();
    assert!(!w.t.vault.join("Trip.md").exists());
    // the link of the panel opens the dialog
    let link = w.panel_rect("deleted");
    w.click(link.center());
    assert!(matches!(w.t.app.modal, Some(Modal::History)));
    assert!(w.has_text("Trip.md") && w.has_text("deleted"), "{:?}", w.hl.text);
    let restore = w.t.app.history.deleted.rects.iter().find(|r| r.0 == "restore-deleted-0").expect("a Restore button").1;
    w.click(restore.center());
    assert_eq!(crate::fsutil::read_to_string(w.t.vault.join("Trip.md")).unwrap(), V3);
}
