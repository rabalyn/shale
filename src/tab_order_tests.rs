//! Reordering and pinning tabs in the whole window (headless): real pointer events at the
//! places the tab bar reports in `ShaleApp::tab_rects` and `ShaleApp::tab_marks`.

use crate::app::{Options, ShaleApp, View};
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::{Path, PathBuf};

struct W {
    app: ShaleApp,
    ctx: egui::Context,
    t: f64,
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
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut w = W { app, ctx, t: 0.0, dir };
        w.frames(4);
        w.app.active = 0;
        w.frames(2);
        w
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 900.0))), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn button(p: Pos2, b: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos: p, button: b, pressed, modifiers: Modifiers::NONE }
    }

    fn click_with(&mut self, p: Pos2, b: PointerButton) {
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![Self::button(p, b, true)]);
        self.frame(vec![Self::button(p, b, false)]);
        self.frames(3);
    }

    fn tab(&self, i: usize) -> Rect {
        self.app.tab_rects[i].0
    }

    /// Press on tab `i`, move to `to` in steps, let `while_dragging` look at the window, release.
    fn drag(&mut self, i: usize, to: Pos2, while_dragging: impl FnOnce(&mut W)) {
        let from = self.tab(i).center();
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![Self::button(from, PointerButton::Primary, true)]);
        for k in 1..=6 {
            self.frame(vec![Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
        }
        self.frames(2);
        while_dragging(self);
        self.frame(vec![Self::button(to, PointerButton::Primary, false)]);
        self.frames(3);
    }

    fn order(&self) -> Vec<String> {
        self.app.tabs.iter().map(|t| t.view.path().map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).unwrap_or_default()).collect()
    }

    fn active_name(&self) -> String {
        self.order()[self.app.active].clone()
    }

    /// Pin or unpin tab `i` through its right-click menu.
    fn pin_via_menu(&mut self, i: usize) {
        let p = self.tab(i).center();
        self.click_with(p, PointerButton::Secondary);
        let entry = self.app.tab_marks.menu_pin.expect("the right click opens the tab menu");
        self.click_with(entry.center(), PointerButton::Primary);
        assert!(self.app.tab_marks.menu_pin.is_none(), "the menu closes after the choice");
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
    assert!(w.app.tab_marks.drop_marker.is_none(), "no marker without a drag");
    // Welcome to the right half of the third tab: it lands after "Daily log"
    let to = Pos2::new(w.tab(2).center().x + 30.0, w.tab(2).center().y);
    let right_of_third = w.tab(3).min.x;
    w.drag(0, to, |w| {
        let m = w.app.tab_marks.drop_marker.expect("a marker while dragging");
        assert!((m.center().x - right_of_third).abs() < 4.0, "the marker is where the tab would land: {m:?} vs {right_of_third}");
        assert_eq!(w.order()[0], "Welcome", "nothing moved yet");
    });
    assert_eq!(w.order(), names(&["Ideas", "Daily log", "Welcome", "Garden plan"]));
    assert_eq!(w.active_name(), "Welcome", "the dragged tab is the open one");
    assert!(w.app.tab_marks.drop_marker.is_none(), "the marker is gone after the drop");
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
    let x = w.app.tab_rects[0].1.center();
    w.click_with(x, PointerButton::Primary);
    assert_eq!(w.order(), names(&["Daily log", "Garden plan"]));
}

// spec: NAV-016
#[test]
fn pinning_moves_a_tab_to_the_left_with_a_pin_and_unpinning_puts_it_back() {
    let mut w = W::new("pin");
    assert!(w.app.tab_marks.pinned.is_empty());
    w.pin_via_menu(2);
    assert_eq!(w.order(), names(&["Daily log", "Welcome", "Ideas", "Garden plan"]));
    assert_eq!(w.app.tab_marks.pinned, vec![0], "the pinned tab shows a pin");
    // a second pinned tab goes behind the first, the others keep their order
    w.pin_via_menu(3);
    assert_eq!(w.order(), names(&["Daily log", "Garden plan", "Welcome", "Ideas"]));
    assert_eq!(w.app.tab_marks.pinned, vec![0, 1]);
    assert_eq!(w.app.vdata.state.tabs.iter().filter(|t| t.pinned).map(|t| t.file.clone().unwrap()).collect::<Vec<_>>(), vec!["Daily log.md", "Projects/Garden plan.md"]);
    // unpinning: the tab goes right of the pinned ones
    w.pin_via_menu(0);
    assert_eq!(w.order(), names(&["Garden plan", "Daily log", "Welcome", "Ideas"]));
    assert_eq!(w.app.tab_marks.pinned, vec![0]);
    // a pinned tab can still be closed without a question
    let x = w.app.tab_rects[0].1.center();
    w.click_with(x, PointerButton::Primary);
    assert_eq!(w.order(), names(&["Daily log", "Welcome", "Ideas"]));
    assert!(w.app.tab_marks.pinned.is_empty());
}

// spec: NAV-017
#[test]
fn a_drag_cannot_cross_the_border_between_pinned_and_unpinned_tabs() {
    let mut w = W::new("border");
    w.pin_via_menu(0);
    w.pin_via_menu(1);
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Daily log", "Garden plan"]));
    assert_eq!(w.app.tab_marks.pinned, vec![0, 1]);
    // an unpinned tab dragged to the far left stops right of the pinned ones; the marker too
    let border = w.tab(2).min.x;
    let to = Pos2::new(w.tab(0).min.x + 2.0, w.tab(0).center().y);
    w.drag(3, to, |w| {
        let m = w.app.tab_marks.drop_marker.expect("a marker");
        assert!((m.center().x - border).abs() < 4.0, "the marker stays inside the unpinned group: {m:?} vs {border}");
    });
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Garden plan", "Daily log"]));
    // a pinned tab dragged to the far right stays among the pinned ones
    let to = Pos2::new(w.tab(3).max.x - 2.0, w.tab(3).center().y);
    w.drag(0, to, |_| {});
    assert_eq!(w.order(), names(&["Ideas", "Welcome", "Garden plan", "Daily log"]));
    assert_eq!(w.app.tab_marks.pinned, vec![0, 1]);
    // pinned tabs still reorder among themselves
    let to = Pos2::new(w.tab(0).min.x + 2.0, w.tab(0).center().y);
    w.drag(1, to, |_| {});
    assert_eq!(w.order(), names(&["Welcome", "Ideas", "Garden plan", "Daily log"]));
}

// spec: NAV-016
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
    let got: Vec<(View, bool)> = again.tabs.iter().map(|t| (t.view.clone(), t.pinned)).collect();
    let want: Vec<(View, bool)> = ["Projects/Garden plan", "Ideas", "Welcome", "Daily log"].iter().enumerate().map(|(i, n)| (View::Note(PathBuf::from(format!("{n}.md"))), i == 0)).collect();
    assert_eq!(got, want);
}
