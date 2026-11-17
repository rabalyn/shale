//! Panes side by side in the whole window (headless): splitting, focusing, the borders, closing
//! and keeping the panes between starts (NAV-032..NAV-035, NAV-026).

use crate::app::{Options, ShaleApp, View};
use crate::keys::{Action as KeyAction, Binding};
use crate::ui_test_support::Headless;
use eframe::egui::{CursorIcon, Event, Key, Modifiers, PointerButton, Pos2};
use std::path::{Path, PathBuf};

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

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
    let ca = CTRL_ALT;
    w.key(Key::ArrowLeft, ca);
    assert_eq!(w.app.panes.focused, 1);
    w.key(Key::ArrowLeft, ca);
    w.key(Key::ArrowLeft, ca);
    assert_eq!(w.app.panes.focused, 2, "wraps around to the last pane");
    w.key(Key::ArrowRight, ca);
    assert_eq!(w.app.panes.focused, 0);
    // the keys are definable like the others
    assert_eq!(w.app.keymap.bindings(KeyAction::NextPane), vec![Binding::parse("Ctrl+Alt+Right").unwrap()]);
    w.app.keymap.set(KeyAction::NextPane, Binding::parse("Ctrl+Alt+J").unwrap()).unwrap();
    w.key(Key::ArrowRight, ca);
    assert_eq!(w.app.panes.focused, 0, "the old key is gone");
    w.key(Key::J, ca);
    assert_eq!(w.app.panes.focused, 1, "the new one works");
    // Ctrl+Alt+W closes the focused pane with its tabs; the focus goes to the left neighbour
    w.app.focus_pane(1);
    w.key(Key::W, ca);
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
