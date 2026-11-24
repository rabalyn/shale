//! Reordering and pinning tabs in the whole window (headless): real pointer events at the
//! places the tab bar reports in `ShaleApp::tab_rects` and `ShaleApp::tab_marks`.

use crate::app::{Options, ShaleApp, View};
use crate::ui_test_support::Headless;
use eframe::egui::{PointerButton, Pos2, Rect};
use std::path::{Path, PathBuf};

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

const NOTES: [&str; 4] = ["Welcome", "Ideas", "Daily log", "Projects/Garden plan"];

impl W {
    /// A window on the sample vault with four note tabs, in the order of `NOTES`.
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-taborder-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        for n in &NOTES[1..] {
            app.open(Path::new(&format!("{n}.md")), true);
        }
        let mut w = W { app, hl: Headless::new(), dir };
        w.frames(4);
        w.app.tabbar.active = 0;
        w.frames(2);
        w
    }

    fn frames(&mut self, n: usize) {
        self.hl.frames(&mut self.app, n);
    }

    fn click_with(&mut self, p: Pos2, b: PointerButton) {
        self.hl.click(&mut self.app, p, b);
    }

    fn tab(&self, i: usize) -> Rect {
        self.app.tabbar.tab_rects[i].0
    }

    /// Press on tab `i`, move to `to` in steps, let `while_dragging` look at the window, release.
    fn drag(&mut self, i: usize, to: Pos2, while_dragging: impl FnOnce(&mut W)) {
        let from = self.tab(i).center();
        self.hl.press_and_move(&mut self.app, from, &Headless::steps(from, to, 6));
        while_dragging(self);
        self.hl.release(&mut self.app, to);
    }

    fn order(&self) -> Vec<String> {
        self.app.tabbar.tabs.iter().map(|t| t.view.path().map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).unwrap_or_default()).collect()
    }

    fn active_name(&self) -> String {
        self.order()[self.app.tabbar.active].clone()
    }

    /// Pin or unpin tab `i` through its right-click menu.
    fn pin_via_menu(&mut self, i: usize) {
        let p = self.tab(i).center();
        self.click_with(p, PointerButton::Secondary);
        let entry = self.app.tabbar.tab_marks.menu_pin.expect("the right click opens the tab menu");
        self.click_with(entry.center(), PointerButton::Primary);
        assert!(self.app.tabbar.tab_marks.menu_pin.is_none(), "the menu closes after the choice");
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn names(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// spec: NAV-014
#[test]
fn dragging_a_tab_past_others_moves_it_and_shows_the_marker() {
    let mut w = W::new("drag");
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Daily log", "Garden plan"]));
    assert!(w.app.tabbar.tab_marks.drop_marker.is_none(), "no marker without a drag");
    // Welcome to the right half of the third tab: it lands after "Daily log"
    let to = Pos2::new(w.tab(2).center().x + 30.0, w.tab(2).center().y);
    let right_of_third = w.tab(3).min.x;
    w.drag(0, to, |w| {
        let m = w.app.tabbar.tab_marks.drop_marker.expect("a marker while dragging");
        assert!((m.center().x - right_of_third).abs() < 4.0, "the marker is where the tab would land: {m:?} vs {right_of_third}");
        assert_eq!(w.order()[0], "Welcome", "nothing moved yet");
    });
    assert_eq!(w.order(), names(&["Ideas", "Daily log", "Welcome", "Garden plan"]));
    assert_eq!(w.active_name(), "Welcome", "the dragged tab is the open one");
    assert!(w.app.tabbar.tab_marks.drop_marker.is_none(), "the marker is gone after the drop");
    assert_eq!(w.app.vdata.state.tabs.iter().map(|t| t.file.clone().unwrap()).collect::<Vec<_>>(), vec!["Ideas.md", "Daily log.md", "Welcome.md", "Projects/Garden plan.md"], "the order is kept for the next start");
    // and back to the left end
    let to = Pos2::new(w.tab(0).min.x + 5.0, w.tab(0).center().y);
    w.drag(2, to, |_| {});
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Daily log", "Garden plan"]));
}

// spec: NAV-015
#[test]
fn a_click_without_movement_selects_and_middle_click_closes() {
    let mut w = W::new("click");
    let p = w.tab(2).center();
    w.click_with(p, PointerButton::Primary);
    assert_eq!(w.active_name(), "Daily log");
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Daily log", "Garden plan"]), "a click does not reorder");
    let p = w.tab(1).center();
    w.click_with(p, PointerButton::Middle);
    assert_eq!(w.order(), names(&["Welcome", "Daily log", "Garden plan"]));
    // the ✕ of a tab
    let x = w.app.tabbar.tab_rects[0].1.center();
    w.click_with(x, PointerButton::Primary);
    assert_eq!(w.order(), names(&["Daily log", "Garden plan"]));
}

// spec: NAV-016
#[test]
fn pinning_moves_a_tab_to_the_left_with_a_pin_and_unpinning_puts_it_back() {
    let mut w = W::new("pin");
    assert!(w.app.tabbar.tab_marks.pinned.is_empty());
    w.pin_via_menu(2);
    assert_eq!(w.order(), names(&["Daily log", "Welcome", "Ideas", "Garden plan"]));
    assert_eq!(w.app.tabbar.tab_marks.pinned, vec![0], "the pinned tab shows a pin");
    // a second pinned tab goes behind the first, the others keep their order
    w.pin_via_menu(3);
    assert_eq!(w.order(), names(&["Daily log", "Garden plan", "Welcome", "Ideas"]));
    assert_eq!(w.app.tabbar.tab_marks.pinned, vec![0, 1]);
    assert_eq!(w.app.vdata.state.tabs.iter().filter(|t| t.pinned).map(|t| t.file.clone().unwrap()).collect::<Vec<_>>(), vec!["Daily log.md", "Projects/Garden plan.md"]);
    // unpinning: the tab goes right of the pinned ones
    w.pin_via_menu(0);
    assert_eq!(w.order(), names(&["Garden plan", "Daily log", "Welcome", "Ideas"]));
    assert_eq!(w.app.tabbar.tab_marks.pinned, vec![0]);
    // a pinned tab can still be closed without a question
    let x = w.app.tabbar.tab_rects[0].1.center();
    w.click_with(x, PointerButton::Primary);
    assert_eq!(w.order(), names(&["Daily log", "Welcome", "Ideas"]));
    assert!(w.app.tabbar.tab_marks.pinned.is_empty());
}

// spec: NAV-016
#[test]
fn a_pinned_tab_is_closed_by_a_middle_click_and_by_ctrl_w_without_a_question() {
    use eframe::egui::{Event, Key, Modifiers};
    let mut w = W::new("pinned-close");
    w.pin_via_menu(0);
    let p = w.tab(0).center();
    w.click_with(p, PointerButton::Middle);
    assert_eq!(w.order(), names(&["Ideas", "Daily log", "Garden plan"]));
    assert!(w.app.modal.is_none(), "no question");
    w.pin_via_menu(0);
    w.app.tabbar.active = 0;
    w.frames(2);
    for pressed in [true, false] {
        w.hl.frame(&mut w.app, vec![Event::Key { key: Key::W, physical_key: None, pressed, repeat: false, modifiers: Modifiers::COMMAND }]);
    }
    w.frames(2);
    assert_eq!(w.order(), names(&["Daily log", "Garden plan"]));
    assert!(w.app.modal.is_none(), "no question");
}

// spec: NAV-017
#[test]
fn a_drag_cannot_cross_the_border_between_pinned_and_unpinned_tabs() {
    let mut w = W::new("border");
    w.pin_via_menu(0);
    w.pin_via_menu(1);
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Daily log", "Garden plan"]));
    assert_eq!(w.app.tabbar.tab_marks.pinned, vec![0, 1]);
    // an unpinned tab dragged to the far left stops right of the pinned ones; the marker too
    let border = w.tab(2).min.x;
    let to = Pos2::new(w.tab(0).min.x + 2.0, w.tab(0).center().y);
    w.drag(3, to, |w| {
        let m = w.app.tabbar.tab_marks.drop_marker.expect("a marker");
        assert!((m.center().x - border).abs() < 4.0, "the marker stays inside the unpinned group: {m:?} vs {border}");
    });
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Garden plan", "Daily log"]));
    // a pinned tab dragged to the far right stays among the pinned ones
    let to = Pos2::new(w.tab(3).max.x - 2.0, w.tab(3).center().y);
    w.drag(0, to, |_| {});
    assert_eq!(w.order(), names(&["Ideas", "Welcome", "Garden plan", "Daily log"]));
    assert_eq!(w.app.tabbar.tab_marks.pinned, vec![0, 1]);
    // pinned tabs still reorder among themselves
    let to = Pos2::new(w.tab(0).min.x + 2.0, w.tab(0).center().y);
    w.drag(1, to, |_| {});
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Garden plan", "Daily log"]));
}

// spec: NAV-016, NAV-026
#[test]
fn pinned_tabs_are_restored_pinned_and_first() {
    let mut w = W::new("restore");
    w.pin_via_menu(3);
    let to = Pos2::new(w.tab(2).center().x + 30.0, w.tab(2).center().y);
    w.drag(1, to, |_| {});
    assert_eq!(w.order(), names(&["Garden plan", "Ideas", "Welcome", "Daily log"]));
    w.app.vdata.save().unwrap();
    let mut again = ShaleApp::build(Options { screenshot: None, vault: Some(w.dir.clone()), view: None, keys: None }, false);
    again.open_vault(&w.dir.clone());
    let got: Vec<(View, bool)> = again.tabbar.tabs.iter().map(|t| (t.view.clone(), t.pinned)).collect();
    let want: Vec<(View, bool)> = ["Projects/Garden plan", "Ideas", "Welcome", "Daily log"].iter().enumerate().map(|(i, n)| (View::Note(PathBuf::from(format!("{n}.md"))), i == 0)).collect();
    assert_eq!(got, want);
}

impl W {
    /// A window whose tab 0 (Welcome) is pinned and active, with notes `T1.md` … `T4.md` in the vault.
    fn pinned_welcome(tag: &str) -> W {
        let mut w = W::new(tag);
        for i in 1..=4 {
            std::fs::write(w.dir.join(format!("T{i}.md")), format!("# T{i}\nneedle\n")).unwrap();
        }
        w.app.vault.as_mut().unwrap().rescan();
        w.pin_via_menu(0);
        w.app.tabbar.active = 0;
        w.frames(2);
        w
    }

    /// The pinned Welcome tab kept its note and `target` opened in a new tab, which is the open one.
    fn assert_opened_beside_the_pin(&self, target: &str, tabs_before: usize) {
        assert_eq!(self.app.tabbar.tabs[0].view, View::Note(PathBuf::from("Welcome.md")), "the pinned tab keeps its note");
        assert!(self.app.tabbar.tabs[0].pinned);
        assert_eq!(self.app.tabbar.tabs.len(), tabs_before + 1, "a new tab holds the target");
        assert_eq!(self.app.current_path(), Some(PathBuf::from(target)));
        assert!(!self.app.tabbar.tabs[self.app.tabbar.active].pinned);
    }
}

// spec: NAV-062
#[test]
fn a_pinned_tab_is_never_replaced_by_opening_a_note_whichever_way_it_is_opened() {
    let mut w = W::pinned_welcome("pinned-open");
    let n = w.app.tabbar.tabs.len();
    // the file tree, the quick switcher, a graph click, the reader, an import summary: `Action::Open`
    w.app.actions.push(crate::app::Action::Open { rel: PathBuf::from("T1.md"), new_tab: false });
    w.app.process_actions();
    w.assert_opened_beside_the_pin("T1.md", n);
    // a followed link
    w.app.tabbar.active = 0;
    w.app.open_link("T2", None, Some(PathBuf::from("Welcome.md")), false);
    w.assert_opened_beside_the_pin("T2.md", n + 1);
    // a line of the search panel
    w.app.tabbar.active = 0;
    w.app.actions.push(crate::app::Action::GotoMatch { rel: PathBuf::from("T3.md"), line: 1, col: 0 });
    w.app.process_actions();
    w.assert_opened_beside_the_pin("T3.md", n + 2);
    // an import (a saved mail, post or event) opens its note the same way
    w.app.tabbar.active = 0;
    w.app.open(Path::new("T4.md"), false);
    w.assert_opened_beside_the_pin("T4.md", n + 3);
    // a view (a mail hit, a post hit, the calendar) too
    w.app.tabbar.active = 0;
    w.app.show_view(View::Calendar, false);
    assert_eq!(w.app.tabbar.tabs[0].view, View::Note(PathBuf::from("Welcome.md")));
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Calendar);
    // a note that is open already is just made the open one
    w.app.tabbar.active = 0;
    w.app.open(Path::new("Ideas.md"), false);
    assert_eq!(w.app.tabbar.tabs.len(), n + 5);
    assert_eq!(w.app.current_path(), Some(PathBuf::from("Ideas.md")));
}

// spec: NAV-062
#[test]
fn an_unpinned_tab_still_opens_a_note_in_place() {
    let mut w = W::pinned_welcome("unpinned-open");
    w.app.tabbar.active = 1;
    let n = w.app.tabbar.tabs.len();
    w.app.open(Path::new("T1.md"), false);
    assert_eq!(w.app.tabbar.tabs.len(), n, "no new tab");
    assert_eq!(w.app.tabbar.tabs[1].view, View::Note(PathBuf::from("T1.md")));
    assert_eq!(w.app.tabbar.tabs[0].view, View::Note(PathBuf::from("Welcome.md")));
    // and an unpinned tab keeps its history
    w.app.go_back();
    assert_eq!(w.app.current_path(), Some(PathBuf::from("Ideas.md")));
}

// spec: NAV-062
#[test]
fn back_and_forward_of_a_pinned_tab_open_the_other_note_beside_it() {
    let mut w = W::new("pinned-history");
    std::fs::write(w.dir.join("T1.md"), "# T1\n").unwrap();
    w.app.vault.as_mut().unwrap().rescan();
    w.app.open(Path::new("Projects/Garden plan.md"), true);
    w.app.open(Path::new("T1.md"), false);
    let i = w.app.tabbar.active;
    assert_eq!(w.app.tabbar.tabs[i].back.len(), 1, "the tab came from the Garden plan");
    w.app.set_tab_pinned(i, true);
    let i = w.app.tabbar.active;
    let n = w.app.tabbar.tabs.len();
    w.app.go_back();
    assert_eq!(w.app.tabbar.tabs.len(), n + 1);
    assert_eq!(w.app.current_path(), Some(PathBuf::from("Projects/Garden plan.md")));
    assert_eq!(w.app.tabbar.tabs[i].view, View::Note(PathBuf::from("T1.md")), "the pinned tab stays where it is");
    assert_eq!(w.app.tabbar.tabs[i].back.len(), 1, "and keeps its history");
    // forward of a pinned tab with a forward step does the same
    w.app.tabbar.tabs[i].fwd.push(View::Note(PathBuf::from("T9.md")));
    w.app.tabbar.active = i;
    w.app.go_forward();
    assert_eq!(w.app.tabbar.tabs.len(), n + 2);
    assert_eq!(w.app.tabbar.tabs[i].view, View::Note(PathBuf::from("T1.md")));
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Note(PathBuf::from("T9.md")));
}
