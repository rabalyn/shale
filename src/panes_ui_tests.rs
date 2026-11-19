//! Panes side by side in the whole window (headless): splitting, focusing, the borders, closing
//! and keeping the panes between starts (NAV-032..NAV-035, NAV-026).

use crate::app::{Options, ShaleApp, View};
use crate::keys::{Action as KeyAction, Binding};
use crate::ui_test_support::Headless;
use eframe::egui::{CursorIcon, Event, Key, Modifiers, PointerButton, Pos2};
use std::path::{Path, PathBuf};

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };

/// A point with no widget: the top left corner of the window lies in the inner margin of the ribbon
/// panel (6 points left, 8 points top, layout.rs `ribbon`), where no icon button is drawn. (5, 5) is
/// the ribbon's "Toggle left sidebar" button, which gives the keyboard back to the editor (EDIT-042).
fn empty_space() -> Pos2 {
    Pos2::new(2.0, 2.0)
}

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    /// A window on the sample vault with the notes Welcome and Ideas open as tabs of one pane.
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-panes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        // these tests drive the Linux/Windows defaults; macOS has other keys for six actions (KEY-019)
        app.keymap = crate::keys::Keymap::from_overrides_for(&app.cfg.keybindings, false);
        app.open(Path::new("Ideas.md"), true);
        let mut w = W { app, hl: Headless::new(), dir };
        w.frames(4);
        w.app.tabbar.active = 0;
        w.frames(2);
        w
    }

    fn frames(&mut self, n: usize) {
        self.hl.frames(&mut self.app, n);
    }

    fn click(&mut self, p: Pos2, b: PointerButton) {
        self.hl.click(&mut self.app, p, b);
    }

    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
        self.frames(2);
    }

    fn split(&mut self, rel: &str) {
        self.app.open_in_split_right(View::Note(PathBuf::from(rel)));
        self.frames(3);
    }

    /// The note names of the tabs of pane `i`.
    fn names(&self, i: usize) -> Vec<String> {
        self.app.pane_bar(i).tabs.iter().map(|t| t.view.path().map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).unwrap_or_else(|| "(empty)".into())).collect()
    }

    fn tab_center(&self, pane: usize, tab: usize) -> Pos2 {
        self.app.pane_bar(pane).tab_rects[tab].0.center()
    }

    fn pane_widths(&self) -> Vec<f32> {
        self.app.panes.rects.iter().map(|r| r.width()).collect()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// spec: NAV-032
// spec: NAV-033
#[test]
fn splitting_opens_the_view_in_a_new_pane_right_of_the_focused_one_and_focuses_it() {
    let mut w = W::new("split");
    assert_eq!(w.app.panes.len(), 1);
    assert!(w.app.tabbar.tab_marks.focus_line.is_none(), "one pane needs no marker");
    w.split("Daily log.md");
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (2, 1));
    assert_eq!(w.names(0), strs(&["Welcome", "Ideas"]), "the first pane keeps its tabs");
    assert_eq!(w.names(1), strs(&["Daily log"]));
    assert_eq!(w.app.current_tab().unwrap().view, View::Note("Daily log.md".into()), "the accessor is the focused pane's open tab");
    // side by side with equal width and a border between
    let (a, b) = (w.app.panes.rects[0], w.app.panes.rects[1]);
    assert!(a.max.x < b.min.x && (a.width() - b.width()).abs() < 1.0 && a.min.y == b.min.y, "{a:?} {b:?}");
    assert_eq!(w.app.panes.borders.len(), 1);
    // the focused pane has the marker, the other has not
    assert!(w.app.tabbar.tab_marks.focus_line.is_some());
    assert!(w.app.pane_bar(0).tab_marks.focus_line.is_none());
    // tab ids stay unique across the panes
    let mut ids: Vec<u64> = w.app.all_tabs().map(|t| t.id).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n);
    // a split to the right of the first pane lands between the two
    w.app.focus_pane(0);
    w.split("Ideas.md");
    assert_eq!((w.names(0), w.names(1), w.names(2)), (strs(&["Welcome", "Ideas"]), strs(&["Ideas"]), strs(&["Daily log"])));
    assert_eq!(w.app.panes.focused, 1);
}

// spec: NAV-033
#[test]
fn the_split_key_opens_the_current_tab_in_a_new_pane_and_the_tab_menu_has_the_entry() {
    let mut w = W::new("splitkey");
    w.key(Key::Backslash, CTRL);
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (2, 1));
    assert_eq!(w.names(1), strs(&["Welcome"]), "the open tab of the pane is shown in the new one too");
    // the tab menu of the first pane
    let p = w.tab_center(0, 1);
    w.click(p, PointerButton::Secondary);
    assert_eq!(w.app.panes.focused, 0, "the right click focuses the pane");
    let entry = w.app.tabbar.tab_marks.menu_split.expect("the tab menu has Open in split right");
    w.click(entry.center(), PointerButton::Primary);
    assert_eq!(w.app.panes.len(), 3);
    assert_eq!(w.names(1), strs(&["Ideas"]), "it opens right of the pane of the tab");
    assert_eq!(w.app.panes.focused, 1);
    // a pane that was empty before does not stay empty when the file is gone
    w.app.open_in_split_right(View::Note("Gone.md".into()));
    assert_eq!(w.app.panes.len(), 3, "a file that is not there leaves no empty pane");
}

// spec: NAV-032
// spec: KEY-002
#[test]
fn a_press_in_a_pane_focuses_it_and_global_actions_work_on_the_focused_pane() {
    let mut w = W::new("focus");
    w.split("Daily log.md");
    // click the second tab of the first pane
    let p = w.tab_center(0, 1);
    w.click(p, PointerButton::Primary);
    assert_eq!(w.app.panes.focused, 0);
    assert!(w.app.tabbar.tab_marks.focus_line.is_some(), "the marker moved");
    assert!(w.app.pane_bar(1).tab_marks.focus_line.is_none());
    assert_eq!(w.app.tabbar.active, 1);
    // a note opened now lands in the first pane, as a new tab there
    w.app.open(Path::new("Projects/Garden plan.md"), true);
    assert_eq!((w.names(0), w.names(1)), (strs(&["Welcome", "Ideas", "Garden plan"]), strs(&["Daily log"])));
    // a click into the content of the second pane focuses it; opening a link there replaces its tab
    let content = w.app.panes.rects[1].center() + eframe::egui::vec2(0.0, 150.0);
    w.click(content, PointerButton::Primary);
    assert_eq!(w.app.panes.focused, 1);
    w.app.open(Path::new("Welcome.md"), false);
    assert_eq!((w.names(0), w.names(1)), (strs(&["Welcome", "Ideas", "Garden plan"]), strs(&["Welcome"])));
    // previous / next tab and back act on the focused pane only
    w.app.open(Path::new("Ideas.md"), false);
    w.app.go_back();
    assert_eq!(w.names(1), strs(&["Welcome"]), "back in the second pane");
    assert_eq!(w.names(0), strs(&["Welcome", "Ideas", "Garden plan"]));
    w.app.step_tab(1);
    assert_eq!(w.names(1), strs(&["Welcome"]), "one tab: nowhere to go");
    w.app.focus_pane(0);
    w.app.step_tab(1);
    assert_eq!(w.app.tabbar.active, 0, "wraps around in the first pane");
}

// spec: KEY-002
// spec: NAV-032
#[test]
fn the_keys_move_the_focus_between_the_panes_and_close_one() {
    let mut w = W::new("keys");
    w.split("Daily log.md");
    w.split("Ideas.md");
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (3, 2));
    let (prev, next) = (Modifiers::SHIFT, Modifiers::NONE);
    w.key(Key::F6, prev);
    assert_eq!(w.app.panes.focused, 1);
    w.key(Key::F6, prev);
    w.key(Key::F6, prev);
    assert_eq!(w.app.panes.focused, 2, "wraps around to the last pane");
    w.key(Key::F6, next);
    assert_eq!(w.app.panes.focused, 0);
    // the keys are definable like the others
    assert_eq!(w.app.keymap.bindings(KeyAction::NextPane), vec![Binding::parse("F6").unwrap()]);
    w.app.keymap.set(KeyAction::NextPane, Binding::parse("Ctrl+Alt+J").unwrap()).unwrap();
    w.key(Key::F6, next);
    assert_eq!(w.app.panes.focused, 0, "the old key is gone");
    w.key(Key::J, CTRL_ALT);
    assert_eq!(w.app.panes.focused, 1, "the new one works");
    // Ctrl+Shift+W closes the focused pane with its tabs; the focus goes to the left neighbour
    w.app.focus_pane(1);
    w.key(Key::W, CTRL_SHIFT);
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (2, 0));
    assert_eq!((w.names(0), w.names(1)), (strs(&["Welcome", "Ideas"]), strs(&["Ideas"])));
    // the last pane stays
    w.app.close_pane(0);
    w.app.close_pane(0);
    assert_eq!(w.app.panes.len(), 1);
    assert_eq!(w.names(0), strs(&["Ideas"]));
}

// spec: NAV-035
#[test]
fn closing_the_last_tab_of_a_pane_removes_the_pane_and_the_last_pane_stays() {
    let mut w = W::new("close");
    w.split("Daily log.md");
    w.split("Ideas.md");
    // the second pane (focus there) loses its only tab with Ctrl+W
    w.app.focus_pane(1);
    w.key(Key::W, CTRL);
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (2, 0), "the focus moves to the left neighbour");
    assert_eq!((w.names(0), w.names(1)), (strs(&["Welcome", "Ideas"]), strs(&["Ideas"])));
    // the x of the tab in the right pane; the focus goes to the left one, which is the last neighbour
    let x = w.app.pane_bar(1).tab_rects[0].1.center();
    w.click(x, PointerButton::Primary);
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (1, 0));
    // the weights of a closed pane go to its neighbour
    assert_eq!(w.app.panes.weights.len(), 1);
    // the first pane: closing its tabs leaves one pane with the empty view
    w.key(Key::W, CTRL);
    w.key(Key::W, CTRL);
    assert_eq!(w.app.panes.len(), 1);
    assert!(w.app.tabbar.tabs.is_empty());
    // closing the first of two panes focuses the one that was right of it
    w.app.show_view(View::Tasks, false);
    w.split("Welcome.md");
    w.app.focus_pane(0);
    w.key(Key::W, CTRL);
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (1, 0));
    assert_eq!(w.names(0), strs(&["Welcome"]));
}

// spec: NAV-034
#[test]
fn the_border_between_panes_drags_and_stops_at_the_minimum_width() {
    let mut w = W::new("border");
    w.split("Daily log.md");
    let before = w.pane_widths();
    let border = w.app.panes.borders[0];
    let grip = border.center();
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(grip)]);
    w.frames(1);
    assert_eq!(w.hl.cursor, CursorIcon::ResizeHorizontal, "the border shows the resize cursor");
    let to = grip + eframe::egui::vec2(150.0, 0.0);
    w.hl.press_and_move(&mut w.app, grip, &Headless::steps(grip, to, 6));
    w.hl.release(&mut w.app, to);
    let after = w.pane_widths();
    assert!((after[0] - before[0] - 150.0).abs() < 2.0 && (before[1] - after[1] - 150.0).abs() < 2.0, "{before:?} -> {after:?}");
    assert_eq!(w.app.panes.focused, 1, "dragging a border does not change the focus");
    // far to the left: the first pane keeps the minimum
    let grip = w.app.panes.borders[0].center();
    let to = Pos2::new(grip.x - 2000.0, grip.y);
    w.hl.press_and_move(&mut w.app, grip, &Headless::steps(grip, Pos2::new(grip.x - 700.0, grip.y), 8));
    w.hl.release(&mut w.app, to);
    let min = crate::app::panes::MIN_PANE_W;
    assert!((w.pane_widths()[0] - min).abs() < 1.0, "{:?}", w.pane_widths());
}

// spec: NAV-034
#[test]
fn a_narrow_window_shares_the_width_equally_when_the_minimum_does_not_fit() {
    let mut w = W::new("narrow");
    for n in ["Daily log.md", "Ideas.md", "Welcome.md", "Daily log.md", "Ideas.md", "Welcome.md"] {
        w.split(n);
    }
    assert_eq!(w.app.panes.len(), 7);
    let widths = w.pane_widths();
    assert!(widths.iter().all(|x| (x - widths[0]).abs() < 1.0 && *x > 50.0), "{widths:?}");
}

// spec: NAV-036
#[test]
fn renaming_and_trashing_a_file_follows_the_tabs_of_all_panes() {
    let mut w = W::new("rename");
    w.split("Ideas.md");
    w.app.rename(Path::new("Ideas.md"), "Thoughts").unwrap();
    w.frames(2);
    assert_eq!((w.names(0), w.names(1)), (strs(&["Welcome", "Thoughts"]), strs(&["Thoughts"])));
    w.app.trash(Path::new("Thoughts.md"));
    w.frames(2);
    assert_eq!((w.names(0), w.names(1)), (strs(&["Welcome", "(empty)"]), strs(&["(empty)"])));
}

fn reopen(w: &W) -> ShaleApp {
    let mut again = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
    again.open_vault(&w.dir.clone());
    again
}

// spec: NAV-026
#[test]
fn panes_with_their_tabs_open_tab_focus_and_widths_come_back() {
    let mut w = W::new("persist");
    w.split("Daily log.md");
    w.app.open(Path::new("Projects/Garden plan.md"), true);
    w.app.step_tab(-1);
    w.frames(2);
    let weights = vec![0.3, 0.7];
    w.app.panes.weights = weights.clone();
    w.app.focus_pane(0);
    w.app.tabbar.active = 1;
    w.frames(2);
    w.app.save_config();
    w.app.vdata.save().unwrap();
    let state = w.app.vdata.state.clone();
    assert_eq!(state.panes.len(), 2);
    assert_eq!(state.tabs, state.panes[0].tabs, "the first pane is also the flat list of before");
    let again = reopen(&w);
    assert_eq!(again.panes.len(), 2);
    let names = |a: &ShaleApp, i: usize| a.pane_bar(i).tabs.iter().map(|t| t.view.clone()).collect::<Vec<_>>();
    assert_eq!(names(&again, 0), vec![View::Note("Welcome.md".into()), View::Note("Ideas.md".into())]);
    assert_eq!(names(&again, 1), vec![View::Note("Daily log.md".into()), View::Note("Projects/Garden plan.md".into())]);
    assert_eq!((again.pane_bar(0).active, again.pane_bar(1).active), (1, 0), "the open tab of each pane");
    assert_eq!(again.panes.focused, 0);
    let sum: f32 = again.panes.weights.iter().sum();
    assert!((again.panes.weights[0] / sum - 0.3).abs() < 0.001, "{:?}", again.panes.weights);
    let mut ids: Vec<u64> = again.all_tabs().map(|t| t.id).collect();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4, "unique tab ids after restoring");
}

impl W {
    /// Give the notes `rels` a text of 150 lines, saved to their files (a state that is read again from the vault).
    fn long_notes(&mut self, rels: &[&str]) {
        for rel in rels {
            let doc = self.app.notes.get_mut(Path::new(rel)).unwrap();
            doc.text = (0..150).map(|i| format!("line number {i}\n")).collect();
            doc.dirty = true;
        }
        self.app.flush_all();
        self.frames(3);
    }

    /// Select chars `from..to` in the open tab of pane `pane` (the pane gets the focus, the editor the keyboard).
    fn select_in(&mut self, pane: usize, from: usize, to: usize) {
        self.app.focus_pane(pane);
        self.app.tabbar.tabs[self.app.tabbar.active].editor.request_focus = true;
        self.frames(3);
        let id = self.app.tabbar.tabs[self.app.tabbar.active].editor.id;
        let mut st = eframe::egui::text_edit::TextEditState::load(self.hl.ctx(), id).unwrap_or_default();
        st.cursor.set_char_range(Some(eframe::egui::text::CCursorRange::two(eframe::egui::text::CCursor::new(from), eframe::egui::text::CCursor::new(to))));
        st.store(self.hl.ctx(), id);
        self.frames(3);
    }

    /// Scroll the note of pane `pane` down by `dy` points with the mouse wheel.
    fn scroll_pane(&mut self, pane: usize, dy: f32) {
        let at = self.app.panes.rects[pane].center();
        self.hl.frame(&mut self.app, vec![Event::PointerMoved(at)]);
        self.hl.frame(
            &mut self.app,
            vec![Event::MouseWheel { unit: eframe::egui::MouseWheelUnit::Point, delta: eframe::egui::vec2(0.0, -dy), modifiers: Modifiers::NONE, phase: eframe::egui::TouchPhase::Move, source: eframe::egui::MouseWheelSource::Wheel }],
        );
        self.frames(3);
    }
}

/// The selection of a restored tab as (from, to), whatever end is the moving one.
fn ordered((a, b): (usize, usize)) -> (usize, usize) {
    (a.min(b), a.max(b))
}

// spec: NAV-038, NAV-039, NAV-041
#[test]
fn the_selection_and_scroll_of_notes_in_both_panes_and_the_reading_view_come_back() {
    let mut w = W::new("place");
    w.split("Daily log.md");
    w.long_notes(&["Welcome.md", "Daily log.md", "Ideas.md"]);
    // pane 0: Welcome with a selection and scrolled, Ideas (not the open tab) with a cursor; pane 1: Daily log
    w.select_in(0, 10, 25);
    w.scroll_pane(0, 300.0);
    w.app.tabbar.active = 1;
    w.select_in(0, 5, 5);
    w.app.tabbar.active = 0;
    w.frames(2);
    w.select_in(1, 40, 40);
    w.scroll_pane(1, 500.0);
    let (s0, s1) = (w.app.pane_bar(0).tabs[0].place.scroll.unwrap(), w.app.pane_bar(1).tabs[0].place.scroll.unwrap());
    assert!(s0 > 100.0 && s1 > s0, "both panes scrolled: {s0} {s1}");
    w.app.toggle_reading_view();
    w.frames(2);
    w.app.save_config();
    w.app.vdata.save().unwrap();
    let saved = &w.app.vdata.state.panes;
    assert_eq!(saved[0].tabs[0].cursor.map(|[a, b]| ordered((a, b))), Some((10, 25)));
    assert_eq!(saved[0].tabs[1].cursor.map(|[a, b]| ordered((a, b))), Some((5, 5)), "a tab that is not the open one");
    assert_eq!(saved[1].tabs[0].cursor.map(|[a, b]| ordered((a, b))), Some((40, 40)));
    assert!(saved[1].tabs[0].reading && !saved[0].tabs[0].reading);

    let mut again = reopen(&w);
    let mut hl = Headless::new();
    hl.frames(&mut again, 4);
    assert!(again.pane_bar(1).tabs[0].reading && !again.pane_bar(0).tabs[0].reading, "the reading view comes back for that tab only");
    assert_eq!(again.pane_bar(0).tabs[0].place.cursor.map(ordered), Some((10, 25)));
    assert_eq!(again.pane_bar(1).tabs[0].place.cursor.map(ordered), Some((40, 40)), "also in the second pane, whose tab is in the reading view");
    assert_eq!(again.pane_bar(0).tabs[1].place.cursor.map(ordered), Some((5, 5)), "a tab that was never shown keeps its position");
    let now = again.pane_bar(0).tabs[0].place.scroll.unwrap();
    assert!((now - s0).abs() <= 1.0, "scrolled where it was: {now} instead of {s0}");
    let editor = &again.pane_bar(0).tabs[0].editor;
    assert_eq!(editor.selection_chars(hl.ctx()).map(ordered), Some((10, 25)), "the note opens with its selection");
    // the tab that was not shown gets its position when it is first shown
    again.focus_pane(0);
    again.tabbar.active = 1;
    hl.frames(&mut again, 3);
    assert_eq!(again.pane_bar(0).tabs[1].editor.selection_chars(hl.ctx()).map(ordered), Some((5, 5)));
    // and what the tab bar of the second pane shows is still the reading view: its position waits for the edit view
    assert!(again.pane_bar(1).tabs[0].place.restore_cursor.is_some());
    again.focus_pane(1);
    again.toggle_reading_view();
    hl.frames(&mut again, 3);
    assert_eq!(again.pane_bar(1).tabs[0].editor.selection_chars(hl.ctx()).map(ordered), Some((40, 40)));
}

// spec: NAV-038
#[test]
fn a_selection_behind_the_end_of_a_note_that_shrank_on_disk_is_kept_inside_it() {
    let mut w = W::new("shrunk");
    w.long_notes(&["Welcome.md"]);
    w.select_in(0, 200, 300);
    w.app.save_config();
    w.app.vdata.save().unwrap();
    crate::fsutil::write_atomic(&w.dir.join("Welcome.md"), "short \u{e4}\u{f6}\u{fc}\n".as_bytes()).unwrap();
    let mut again = reopen(&w);
    let mut hl = Headless::new();
    hl.frames(&mut again, 4);
    let n = "short \u{e4}\u{f6}\u{fc}\n".chars().count();
    assert_eq!(again.pane_bar(0).tabs[0].editor.selection_chars(hl.ctx()).map(ordered), Some((n, n)), "clamped to the end of the text, on a character");
}

// spec: NAV-041
#[test]
fn the_state_is_written_every_few_seconds_only_when_it_changed_and_not_at_every_frame() {
    let mut w = W::new("tick");
    w.long_notes(&["Welcome.md"]);
    let path = crate::vault_state::VaultData::state_path(&w.dir);
    // (only the state is written here, never the global config: that is what `save_config` does too)
    w.app.persist_config = true;
    w.app.vdata.save().unwrap();
    w.app.state_tick.0 = std::time::Instant::now();
    w.select_in(0, 12, 12);
    let on_disk = || crate::fsutil::read_to_string(&path).unwrap();
    assert!(!on_disk().contains("\"cursor\": [\n"), "frames alone write nothing: {}", on_disk());
    w.app.state_tick.0 = std::time::Instant::now() - std::time::Duration::from_secs(6);
    w.frames(1);
    assert!(on_disk().contains("12"), "a cursor that moved is written after the pause: {}", on_disk());
    // nothing changed: the file is not touched again, however many frames run
    let stamp = std::fs::metadata(&path).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(30));
    w.app.state_tick.0 = std::time::Instant::now() - std::time::Duration::from_secs(6);
    w.frames(1);
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), stamp);
}

// spec: NAV-026
#[test]
fn a_state_file_with_a_flat_tab_list_loads_as_one_pane_and_gone_panes_are_skipped() {
    let mut w = W::new("old");
    w.app.vdata.state.panes.clear();
    w.app.vdata.state.tabs = vec![crate::vault_state::SavedTab { file: Some("Ideas.md".into()), ..Default::default() }, crate::vault_state::SavedTab { file: Some("Welcome.md".into()), ..Default::default() }];
    w.app.vdata.save().unwrap();
    let again = reopen(&w);
    assert_eq!(again.panes.len(), 1);
    assert_eq!(again.tabbar.tabs.iter().map(|t| t.view.clone()).collect::<Vec<_>>(), vec![View::Note("Ideas.md".into()), View::Note("Welcome.md".into())]);
    assert_eq!(again.tabbar.active, 1, "the last tab is the open one, as before panes");
    // a second pane whose files are all gone does not come back; a bad weight falls back to equal
    let mut w = W::new("gone");
    let tab = |f: &str| crate::vault_state::SavedTab { file: Some(f.into()), ..Default::default() };
    w.app.vdata.state.panes = vec![
        crate::vault_state::SavedPane { tabs: vec![tab("Ideas.md")], ..Default::default() },
        crate::vault_state::SavedPane { tabs: vec![tab("Missing.md")], ..Default::default() },
        crate::vault_state::SavedPane { tabs: vec![tab("Welcome.md")], weight: -3.0, ..Default::default() },
    ];
    w.app.vdata.state.focused_pane = 5;
    w.app.vdata.save().unwrap();
    let again = reopen(&w);
    assert_eq!(again.panes.len(), 2);
    assert_eq!(again.panes.weights.len(), 2);
    assert_eq!(again.panes.focused, 1, "a focus out of range is the last pane");
}

impl W {
    /// The pane whose editor has the keyboard, or None (the sidebar or nowhere).
    fn keyboard_pane(&self) -> Option<usize> {
        let id = |i: usize| self.app.pane_bar(i).tabs.get(self.app.pane_bar(i).active).map(|t| t.editor.id);
        (0..self.app.panes.len()).find(|&i| id(i).is_some_and(|id| self.hl.ctx().memory(|m| m.has_focus(id))))
    }

    fn in_sidebar(&self) -> bool {
        self.app.tree.sel.is_some()
    }
}

// spec: NAV-011
#[test]
fn ctrl_h_walks_left_through_the_panes_into_the_sidebar() {
    for panes in [2usize, 3] {
        let mut w = W::new(&format!("ctrlh{panes}"));
        w.split("Daily log.md");
        if panes == 3 {
            w.split("Ideas.md");
        }
        w.app.focus_editor();
        w.frames(2);
        assert_eq!(w.keyboard_pane(), Some(panes - 1), "the last pane has the keyboard");
        for want in (0..panes - 1).rev() {
            w.key(Key::H, CTRL);
            assert_eq!((w.app.panes.focused, w.keyboard_pane(), w.in_sidebar()), (want, Some(want), false));
        }
        w.key(Key::H, CTRL);
        assert!(w.in_sidebar() && w.keyboard_pane().is_none(), "from the first pane into the left sidebar");
        assert_eq!(w.app.panes.focused, 0);
        w.key(Key::H, CTRL);
        assert!(w.in_sidebar(), "in the sidebar Ctrl+H stays");
        assert_eq!(w.app.panes.focused, 0);
    }
}

// spec: NAV-011
#[test]
fn ctrl_l_walks_right_from_the_sidebar_through_the_panes_and_stops_at_the_last() {
    for panes in [2usize, 3] {
        let mut w = W::new(&format!("ctrll{panes}"));
        w.split("Daily log.md");
        if panes == 3 {
            w.split("Ideas.md");
        }
        // the focused pane is the last one; the sidebar leads back into the first
        w.app.focus_sidebar(w.hl.ctx());
        w.frames(2);
        assert!(w.in_sidebar());
        for want in 0..panes {
            w.key(Key::L, CTRL);
            assert_eq!((w.app.panes.focused, w.keyboard_pane(), w.in_sidebar()), (want, Some(want), false));
        }
        w.key(Key::L, CTRL);
        assert_eq!((w.app.panes.focused, w.keyboard_pane()), (panes - 1, Some(panes - 1)), "no wrap at the right end");
    }
}

// spec: NAV-011
#[test]
fn ctrl_h_and_ctrl_l_with_one_pane_go_between_the_sidebar_and_the_editor() {
    let mut w = W::new("onepane");
    w.app.focus_editor();
    w.frames(2);
    assert_eq!(w.keyboard_pane(), Some(0));
    w.key(Key::H, CTRL);
    assert!(w.in_sidebar() && w.keyboard_pane().is_none());
    w.key(Key::L, CTRL);
    assert!(!w.in_sidebar());
    assert_eq!((w.app.panes.len(), w.keyboard_pane()), (1, Some(0)));
    w.key(Key::L, CTRL);
    assert_eq!(w.keyboard_pane(), Some(0), "Ctrl+L in the only pane stays");
}

// spec: NAV-011
#[test]
fn ctrl_l_from_the_search_box_goes_into_the_first_pane() {
    let mut w = W::new("searchbox");
    w.split("Daily log.md");
    w.app.side.left = crate::app::LeftPanel::Search;
    w.key(Key::H, CTRL);
    w.key(Key::H, CTRL);
    assert_eq!(w.app.panes.focused, 0);
    w.key(Key::H, CTRL);
    w.frames(2);
    assert!(w.keyboard_pane().is_none(), "the search box has the keyboard");
    w.key(Key::L, CTRL);
    assert_eq!((w.app.panes.focused, w.keyboard_pane()), (0, Some(0)));
}

// spec: NAV-011
#[test]
fn vim_normal_mode_works_in_the_pane_ctrl_h_and_ctrl_l_reach() {
    let mut w = W::new("vim");
    w.split("Daily log.md");
    w.app.cfg.vim_mode = true;
    w.frames(3);
    for (key, to) in [(Key::H, 0usize), (Key::L, 1)] {
        w.key(key, CTRL);
        w.frames(2);
        assert_eq!(w.keyboard_pane(), Some(to));
        assert!(w.app.tabbar.tabs[w.app.tabbar.active].editor.vim_normal_keys(w.hl.ctx()), "normal mode keys are read in pane {to}");
        // `i` enters insert mode of that pane's editor
        w.hl.frame(&mut w.app, vec![Event::Key { key: Key::I, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }, Event::Text("i".into())]);
        w.frames(2);
        assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].editor.vim.mode, crate::vim::Mode::Insert, "i worked in pane {to}");
        // back to normal mode for the next round
        w.key(Key::Escape, Modifiers::NONE);
    }
}

impl W {
    /// Type `text` into whatever has the keyboard.
    fn type_text(&mut self, text: &str) {
        self.hl.frame(&mut self.app, vec![Event::Text(text.into())]);
        self.frames(2);
    }

    fn note_text(&self, rel: &str) -> String {
        self.app.notes.get(Path::new(rel)).unwrap().text.clone()
    }

    /// Take the keyboard away from the widget that has it.
    fn drop_keyboard(&mut self) {
        let ctx = self.hl.ctx().clone();
        if let Some(f) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(f));
        }
    }
}

// spec: NAV-037
#[test]
fn nav_037_closing_the_last_tab_of_a_pane_keeps_the_keyboard_in_the_neighbour() {
    let mut w = W::new("kb-lasttab");
    w.split("Daily log.md");
    w.app.focus_editor();
    w.frames(2);
    assert_eq!(w.keyboard_pane(), Some(1));
    w.key(Key::W, CTRL);
    w.frames(2);
    assert_eq!((w.app.panes.len(), w.app.panes.focused), (1, 0));
    assert_eq!(w.keyboard_pane(), Some(0), "the neighbour's editor has the keyboard");
    w.type_text("QQ");
    assert!(w.note_text("Welcome.md").contains("QQ"), "typing lands in the left note");
}

// spec: NAV-037
#[test]
fn nav_037_close_pane_keeps_the_keyboard_in_the_neighbour() {
    let mut w = W::new("kb-closepane");
    w.split("Daily log.md");
    w.app.focus_editor();
    w.frames(2);
    w.key(Key::W, CTRL_SHIFT);
    w.frames(2);
    assert_eq!((w.app.panes.len(), w.keyboard_pane()), (1, Some(0)));
    w.type_text("QQ");
    assert!(w.note_text("Welcome.md").contains("QQ"));
}

// spec: NAV-037
#[test]
fn nav_037_closing_a_tab_that_is_not_the_last_gives_the_keyboard_to_the_next_tab() {
    let mut w = W::new("kb-nexttab");
    w.app.focus_editor();
    w.frames(2);
    assert_eq!(w.names(0), strs(&["Welcome", "Ideas"]));
    w.key(Key::W, CTRL);
    w.frames(2);
    assert_eq!(w.names(0), strs(&["Ideas"]));
    assert_eq!(w.keyboard_pane(), Some(0));
    w.type_text("QQ");
    assert!(w.note_text("Ideas.md").contains("QQ"));
}

// spec: NAV-037
#[test]
fn nav_037_the_guard_gives_the_keyboard_back_to_the_editor_but_not_to_the_sidebar_or_over_a_dialog() {
    let mut w = W::new("kb-guard");
    w.split("Daily log.md");
    w.app.focus_editor();
    w.frames(2);
    assert_eq!(w.keyboard_pane(), Some(1));
    // the keyboard vanishes: the editor of the focused pane takes it
    w.drop_keyboard();
    w.frames(3);
    assert_eq!(w.keyboard_pane(), Some(1), "the guard refocuses the editor");
    // a press with the pointer on empty space is the user's choice and is left alone
    w.drop_keyboard();
    w.hl.frame_with(&mut w.app, vec![Event::PointerButton { pos: empty_space(), button: PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }], |_, _| {});
    w.hl.frame(&mut w.app, vec![Event::PointerButton { pos: empty_space(), button: PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
    // sidebar browsing keeps the keyboard
    w.app.focus_sidebar(w.hl.ctx());
    w.frames(3);
    assert!(w.in_sidebar() && w.keyboard_pane().is_none(), "the sidebar is browsed, not the editor");
    // a dialog has the keyboard
    w.app.focus_editor();
    w.frames(2);
    w.app.modal = Some(crate::app::Modal::QuickSwitch { query: String::new(), sel: 0 });
    w.frames(2);
    w.drop_keyboard();
    w.frames(3);
    assert!(w.keyboard_pane().is_none(), "the guard leaves the keyboard to the dialog");
}

// spec: NAV-037
#[test]
fn nav_037_a_click_that_takes_the_keyboard_away_is_not_undone_by_the_guard() {
    let mut w = W::new("kb-click");
    w.app.focus_editor();
    w.frames(2);
    assert_eq!(w.keyboard_pane(), Some(0));
    let at = empty_space();
    let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(at)]);
    w.hl.frame(&mut w.app, vec![button(true)]);
    // egui drops the focus while the press frame runs
    w.drop_keyboard();
    w.hl.frame(&mut w.app, vec![button(false)]);
    w.frames(4);
    assert_eq!(w.keyboard_pane(), None, "the click's choice stays");
}
