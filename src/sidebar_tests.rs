//! The menubars inside the whole window (LAY-001…): the window runs headless
//! (`ShaleApp::run_frame`) and the mouse is real pointer events at the places the window reports
//! in `ShaleApp::side`.

use crate::app::{Options, RightPanel, ShaleApp, View};
use crate::config::Config;
use crate::sidebar::{Item, Layout};
use crate::sidebar_ui::Cue;
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, Rect, Vec2};
use std::path::PathBuf;

pub(crate) struct W {
    pub app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    pub fn new(tag: &str) -> W {
        W::with_files(tag, &[])
    }

    /// A window on a vault with these notes more.
    pub fn with_files(tag: &str, files: &[(String, String)]) -> W {
        let dir = std::env::temp_dir().join(format!("shale-sideui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Welcome.md"), "# Welcome\n\n## First\n\n[[Other]]\n\n## Second\n").unwrap();
        std::fs::write(dir.join("Other.md"), "# Other\n\nSee [[Welcome]].\n").unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.frames(4);
        w
    }

    /// A window started again on these settings: what the settings file would hold after the
    /// first window saved it.
    pub fn restarted(&self, tag: &str) -> W {
        let json = serde_json::to_string(&self.app.cfg).unwrap();
        let (cfg, _) = crate::vault_state::parse::<Config>(json.as_bytes()).unwrap();
        let mut w = W::new(tag);
        w.app.cfg = cfg;
        w.app.side = crate::sidebar_ui::SideState::new(Layout::from_config(&w.app.cfg));
        // a new window has no memory of the panels of the first one (LAY-018)
        w.hl = Headless::new();
        w.frames(4);
        w
    }

    pub fn frame(&mut self, events: Vec<Event>) {
        self.hl.frame(&mut self.app, events);
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    /// Press at `from`, move to `to` and stay there with the button down.
    pub fn drag_to(&mut self, from: Pos2, to: Pos2) {
        self.hl.press_and_move(&mut self.app, from, &[from + Vec2::new(0.0, 12.0), to]);
    }

    pub fn release(&mut self, at: Pos2) {
        self.hl.release(&mut self.app, at);
    }

    /// A whole drag and drop.
    pub fn drag_and_drop(&mut self, from: Pos2, to: Pos2) {
        self.drag_to(from, to);
        self.release(to);
    }

    /// The mouse wheel turned by `dy` points over `at`.
    pub fn wheel(&mut self, at: Pos2, dy: f32) {
        let wheel = Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, dy), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel };
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![wheel]);
        self.frames(2);
    }

    /// Where the right sidebar draws the first text that contains `text`.
    pub fn right_text_at(&self, text: &str) -> Option<Pos2> {
        let x = self.app.side.right_rect?.min.x;
        self.hl.text.iter().find(|(t, p)| p.x >= x && t.contains(text)).map(|(_, p)| *p)
    }

    pub fn press_esc(&mut self) {
        let key = |pressed| Event::Key { key: egui::Key::Escape, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.frame(vec![key(true)]);
        self.frame(vec![key(false)]);
        self.frames(2);
    }

    pub fn click_at(&mut self, p: Pos2) {
        self.hl.click(&mut self.app, p, PointerButton::Primary);
    }

    pub fn ribbon_icon(&self, item: Item) -> Rect {
        self.app.side.ribbon_icons.iter().find(|(i, _)| *i == item).unwrap_or_else(|| panic!("no ribbon icon {item:?}")).1
    }

    /// The icon of a panel in the menubar of a view.
    pub fn bar_icon(&self, view: usize, item: Item) -> Rect {
        self.app.side.bars.iter().find(|b| b.view == view).and_then(|b| b.icons.iter().find(|(i, _)| *i == item)).unwrap_or_else(|| panic!("no icon {item:?} in view {view}")).1
    }

    pub fn bar(&self, view: usize) -> Rect {
        self.app.side.bars.iter().find(|b| b.view == view).unwrap_or_else(|| panic!("no view {view}")).rect
    }

    /// The content area of a view (or of the panel a ribbon shortcut shows).
    pub fn body(&self, view: usize) -> Rect {
        self.app.side.bodies.iter().find(|(at, _)| *at == view + 1).unwrap_or_else(|| panic!("no body {view}")).1
    }

    /// What the right sidebar says, one string per text.
    pub fn right_text(&self) -> Vec<String> {
        let x = self.app.side.right_rect.unwrap().min.x;
        self.hl.text.iter().filter(|(_, p)| p.x >= x).map(|(t, _)| t.clone()).collect()
    }

    pub fn right_has(&self, text: &str) -> bool {
        self.right_text().iter().any(|t| t.contains(text))
    }

    /// The panels of each view, top to bottom.
    pub fn views(&self) -> Vec<Vec<Item>> {
        self.app.side.layout.views.iter().map(|v| v.icons.clone()).collect()
    }

    pub fn ribbon(&self) -> Vec<Item> {
        self.app.side.ribbon_icons.iter().map(|(i, _)| *i).collect()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        let _ = std::fs::remove_dir_all(&self.app.mail.root);
        let _ = std::fs::remove_dir_all(&self.app.calendar.local_dir);
    }
}

// spec: LAY-001, LAY-002
#[test]
fn lay_001_a_ribbon_icon_is_dragged_to_a_new_place_and_the_order_survives_a_restart() {
    let mut w = W::new("ribbon-order");
    assert_eq!(w.ribbon(), Layout::default().ribbon, "the default order is drawn");
    // Mail is dragged up between Files and Search; the marker shows the place while the button is down
    let mail = w.ribbon_icon(Item::Mail).center();
    let between = (w.ribbon_icon(Item::Files).max.y + w.ribbon_icon(Item::Search).min.y) / 2.0;
    w.drag_to(mail, Pos2::new(mail.x, between));
    assert_eq!(w.app.side.drag, Some(Item::Mail));
    match w.app.side.cue {
        crate::sidebar_ui::Cue::Ribbon { at, line } => assert_eq!((at, (line[0].y - between).abs() < 3.0), (1, true), "the marker is between Files and Search"),
        c => panic!("no marker: {c:?}"),
    }
    assert_eq!(w.app.side.layout.ribbon, Layout::default().ribbon, "nothing moves before the button is let go");
    w.release(Pos2::new(mail.x, between));
    let mut want = Layout::default().ribbon;
    want.remove(6);
    want.insert(1, Item::Mail);
    assert_eq!(w.ribbon(), want, "the drawn ribbon has the new order");
    assert_eq!(w.app.side.drag, None);
    // a click still works on an icon that was moved, and a plain click is not a drag
    w.click_at(w.ribbon_icon(Item::Tags).center());
    assert!(w.app.side.left == crate::app::LeftPanel::Tags);
    assert_eq!(w.app.side.drag, None);
    // a restart: the settings hold the order and the new window draws it
    let w2 = w.restarted("ribbon-order-2");
    assert_eq!(w2.ribbon(), want, "the order comes back");
    // dragging to the bottom of the ribbon
    let mut w = w2;
    let first = w.ribbon_icon(Item::Files).center();
    let last = w.ribbon_icon(Item::NewNote);
    w.drag_and_drop(first, Pos2::new(first.x, last.max.y + 6.0));
    assert_eq!(w.ribbon().last(), Some(&Item::Files));
    assert_eq!(w.app.cfg.sidebars.ribbon.last().map(String::as_str), Some("files"), "and the settings follow");
}

// spec: LAY-001, LAY-010
#[test]
fn lay_001_dropping_a_ribbon_icon_outside_the_ribbon_or_pressing_esc_changes_nothing() {
    let mut w = W::new("ribbon-cancel");
    let mail = w.ribbon_icon(Item::Mail).center();
    w.drag_and_drop(mail, Pos2::new(700.0, 400.0));
    assert_eq!(w.ribbon(), Layout::default().ribbon, "dropped on the note: nothing happens");
    let between = (w.ribbon_icon(Item::Files).max.y + w.ribbon_icon(Item::Search).min.y) / 2.0;
    w.drag_to(mail, Pos2::new(mail.x, between));
    w.press_esc();
    assert_eq!(w.app.side.drag, None, "Esc ends the drag");
    w.release(Pos2::new(mail.x, between));
    assert_eq!(w.ribbon(), Layout::default().ribbon, "and the button let go afterwards drops nothing");
}

fn panel(p: RightPanel) -> Item {
    Item::Panel(p)
}

// spec: LAY-003, LAY-004, LAY-009
#[test]
fn lay_009_dropping_an_icon_on_the_content_makes_a_new_view_and_two_panels_show_at_once() {
    let mut w = W::new("split");
    w.app.open(std::path::Path::new("Welcome.md"), false);
    w.frames(4);
    assert_eq!(w.views().len(), 1, "one view by default, with all five panels");
    assert!(w.right_has("Linked mentions") && !w.right_has("Second"), "it shows the backlinks, not the outline: {:?}", w.right_text());
    // drag the Outline icon over the content of the view: the zone shows, nothing is created yet
    let from = w.bar_icon(0, panel(RightPanel::Outline)).center();
    let into = w.body(0).center();
    w.drag_to(from, into);
    match w.app.side.cue {
        Cue::Zone { at, rect } => assert_eq!((at, rect), (1, w.body(0)), "the zone is the content of the view"),
        c => panic!("no drop zone: {c:?}"),
    }
    assert!(w.right_has("Drop to open this icon in a new view"), "the zone says what dropping does: {:?}", w.right_text());
    assert_eq!(w.views().len(), 1);
    // let go: a second view with its own menubar holds the icon, and both panels are drawn
    w.release(into);
    assert_eq!(
        w.views(),
        vec![vec![panel(RightPanel::Backlinks), panel(RightPanel::Outgoing), panel(RightPanel::LocalGraph), panel(RightPanel::Calendar), panel(RightPanel::History), panel(RightPanel::Thread)], vec![panel(RightPanel::Outline)]]
    );
    assert_eq!(w.app.side.bars.len(), 2, "each view has its menubar");
    assert!(w.bar(1).min.y > w.body(0).min.y && w.body(1).min.y > w.bar(1).max.y - 1.0, "the views are stacked");
    assert!(w.right_has("Linked mentions") && w.right_has("Second") && w.right_has("First"), "backlinks and outline at once: {:?}", w.right_text());
    assert_eq!(w.app.side.cue, Cue::None);
    // each view shows what its own icon says
    w.click_at(w.bar_icon(0, panel(RightPanel::Outgoing)).center());
    assert!(w.right_has("Not created yet") && w.right_has("Second"), "the other view keeps its panel: {:?}", w.right_text());
    assert!(!w.right_has("Linked mentions"));
    // and it is all in the settings
    assert_eq!(w.app.cfg.sidebars.right.len(), 2);
    assert_eq!(w.app.cfg.sidebars.right[1].icons, ["panel:outline"]);
    assert_eq!(w.app.cfg.sidebars.right[0].active, "panel:outgoing");
}

// spec: LAY-010
#[test]
fn lay_010_esc_or_dropping_elsewhere_cancels_a_drop_on_the_content() {
    let mut w = W::new("zone-cancel");
    let before = w.app.side.layout.clone();
    let from = w.bar_icon(0, panel(RightPanel::Outline)).center();
    let into = w.body(0).center();
    // Esc while the zone shows
    w.drag_to(from, into);
    assert!(matches!(w.app.side.cue, Cue::Zone { .. }));
    w.press_esc();
    assert_eq!((w.app.side.drag, w.app.side.cue), (None, Cue::None), "the zone is gone");
    assert!(!w.right_has("Drop to open"));
    w.release(into);
    assert_eq!(w.app.side.layout, before, "the button let go after Esc drops nothing");
    // dropping on the note, the tab bar and the status bar
    for elsewhere in [Pos2::new(700.0, 400.0), Pos2::new(700.0, 18.0), Pos2::new(700.0, 890.0)] {
        w.drag_to(from, elsewhere);
        assert_eq!(w.app.side.cue, Cue::None, "no zone at {elsewhere:?}");
        w.release(elsewhere);
        assert_eq!(w.app.side.layout, before, "dropped at {elsewhere:?}");
    }
    // moving the pointer out of the zone again takes the zone away
    w.drag_to(from, into);
    assert!(matches!(w.app.side.cue, Cue::Zone { .. }));
    w.frame(vec![Event::PointerMoved(Pos2::new(700.0, 400.0))]);
    assert_eq!(w.app.side.cue, Cue::None);
    w.release(Pos2::new(700.0, 400.0));
    assert_eq!(w.app.side.layout, before);
}

// spec: LAY-005
#[test]
fn lay_005_icons_are_dragged_between_menubars_right_to_right_left_to_right_and_right_to_left() {
    let mut w = W::new("between");
    let view = |icons: &[&str], active: &str| crate::config::RightViewCfg { icons: icons.iter().map(|s| s.to_string()).collect(), active: active.into(), weight: 0 };
    let sidebars = crate::config::SidebarCfg { ribbon: vec![], right: vec![view(&["panel:backlinks", "panel:outline"], "panel:backlinks"), view(&["panel:outgoing", "panel:graph", "panel:calendar"], "panel:outgoing")], hidden: vec![] };
    w.app.side.layout = Layout::from_config(&Config { sidebars, ..Config::default() });
    w.frames(3);
    assert_eq!(w.views().len(), 2);
    // right to right: Outline into the menubar of the second view, behind the first icon
    let from = w.bar_icon(0, panel(RightPanel::Outline)).center();
    let first = w.bar_icon(1, panel(RightPanel::Outgoing));
    let to = Pos2::new(first.max.x + 2.0, first.center().y);
    w.drag_to(from, to);
    assert!(matches!(w.app.side.cue, Cue::Bar { view: 1, at: 1, .. }), "{:?}", w.app.side.cue);
    w.release(to);
    assert_eq!(w.views()[0], [panel(RightPanel::Backlinks)]);
    assert_eq!(w.views()[1], [panel(RightPanel::Outgoing), panel(RightPanel::Outline), panel(RightPanel::LocalGraph), panel(RightPanel::Calendar), panel(RightPanel::History), panel(RightPanel::Thread)]);
    assert!(w.right_has("Second") || w.right_has("No headings"), "the moved icon is the active one of its view and its panel shows");
    // right to left: Calendar into the ribbon, between Files and Search
    let cal = w.bar_icon(1, panel(RightPanel::Calendar)).center();
    let between = (w.ribbon_icon(Item::Files).max.y + w.ribbon_icon(Item::Search).min.y) / 2.0;
    w.drag_and_drop(cal, Pos2::new(w.ribbon_icon(Item::Files).center().x, between));
    assert_eq!(&w.app.side.layout.ribbon[..3], [Item::Files, panel(RightPanel::Calendar), Item::Search]);
    assert!(!w.views()[1].contains(&panel(RightPanel::Calendar)));
    assert_eq!(w.ribbon()[1], panel(RightPanel::Calendar), "the ribbon draws it");
    // left to right: the shortcut goes back to the menubar of the first view
    let sc = w.ribbon_icon(panel(RightPanel::Calendar)).center();
    let b0 = w.bar(0);
    w.drag_and_drop(sc, Pos2::new(b0.max.x - 4.0, b0.center().y));
    assert_eq!(w.views()[0], [panel(RightPanel::Backlinks), panel(RightPanel::Calendar)]);
    assert!(!w.ribbon().contains(&panel(RightPanel::Calendar)));
    assert_eq!(w.app.cfg.sidebars.right[0].icons, ["panel:backlinks", "panel:calendar"], "stored");
    assert!(!w.app.cfg.sidebars.ribbon.contains(&"panel:calendar".to_string()));
}

// spec: LAY-006
#[test]
fn lay_006_a_ribbon_icon_that_cannot_live_on_the_right_is_refused_with_a_no_drop_cue() {
    let mut w = W::new("refused");
    let before = w.app.side.layout.clone();
    let mail = w.ribbon_icon(Item::Graph).center();
    // over a menubar of the right sidebar
    let bar = w.bar(0).center();
    w.drag_to(mail, bar);
    assert_eq!(w.app.side.cue, Cue::Refused);
    assert_eq!(w.hl.cursor, egui::CursorIcon::NoDrop, "the pointer says no");
    w.release(bar);
    assert_eq!(w.app.side.layout, before, "nothing changed");
    // over the content area, where a panel icon would make a new view
    let body = w.body(0).center();
    w.drag_to(mail, body);
    assert_eq!(w.app.side.cue, Cue::Refused);
    assert!(!w.right_has("Drop to open"), "no zone for what cannot live there");
    w.release(body);
    assert_eq!(w.app.side.layout, before);
    // a panel icon over the same place is accepted: the pointer is the grabbing hand
    let from = w.bar_icon(0, panel(RightPanel::Outline)).center();
    w.drag_to(from, body);
    assert_eq!(w.hl.cursor, egui::CursorIcon::Grabbing);
    w.release(body);
}

// spec: LAY-007
#[test]
fn lay_007_taking_the_last_icon_out_of_a_view_removes_the_view_and_an_empty_sidebar_takes_a_drop() {
    let mut w = W::new("last-icon");
    // split Outline off, then drag it back to the ribbon: the second view goes away
    w.drag_and_drop(w.bar_icon(0, panel(RightPanel::Outline)).center(), w.body(0).center());
    assert_eq!(w.views().len(), 2);
    let end = w.ribbon_icon(Item::NewNote);
    let below = Pos2::new(end.center().x, end.max.y + 4.0);
    w.drag_and_drop(w.bar_icon(1, panel(RightPanel::Outline)).center(), below);
    assert_eq!(w.views().len(), 1, "the view without icons is gone");
    assert_eq!(w.app.side.bars.len(), 1);
    assert_eq!(w.ribbon().last(), Some(&panel(RightPanel::Outline)));
    // every panel goes to the ribbon: the sidebar has no view and says so
    for p in [RightPanel::Backlinks, RightPanel::Outgoing, RightPanel::LocalGraph, RightPanel::Calendar, RightPanel::History, RightPanel::Thread] {
        w.drag_and_drop(w.bar_icon(0, panel(p)).center(), below);
    }
    assert!(w.views().is_empty());
    assert!(w.right_has("Drag a panel icon here"), "{:?}", w.right_text());
    // a drop on the empty sidebar makes the first view; a ribbon icon is refused there
    let area = w.app.side.right_rect.unwrap().center();
    w.drag_and_drop(w.ribbon_icon(Item::Graph).center(), area);
    assert!(w.views().is_empty());
    w.drag_and_drop(w.ribbon_icon(panel(RightPanel::Calendar)).center(), area);
    assert_eq!(w.views(), vec![vec![panel(RightPanel::Calendar)]]);
}

// spec: LAY-008
#[test]
fn lay_008_the_split_layout_is_stored_and_comes_back_and_a_broken_one_gives_the_default() {
    let mut w = W::new("restore");
    w.drag_and_drop(w.bar_icon(0, panel(RightPanel::Outline)).center(), w.body(0).center());
    let first = w.ribbon_icon(Item::Files).center();
    let last = w.ribbon_icon(Item::NewNote);
    w.drag_and_drop(first, Pos2::new(first.x, last.max.y + 4.0));
    let (views, ribbon) = (w.views(), w.ribbon());
    assert_eq!(views.len(), 2);
    let w2 = w.restarted("restore-2");
    assert_eq!((w2.views(), w2.ribbon()), (views, ribbon), "both menubars come back as they were");
    assert_eq!(w2.app.side.bars.len(), 2);
    // a settings file with a layout that cannot be read: the rest loads, the layout is the default
    for broken in [r#"{"sidebars": {"right": "nonsense"}, "toolbar_size": 17}"#, r#"{"sidebars": 5}"#, r#"{"sidebars": {"ribbon": ["x"], "right": [{"icons": ["mail", "nope"], "active": 3}]}}"#] {
        let (cfg, _) = crate::vault_state::parse::<Config>(broken.as_bytes()).unwrap();
        assert_eq!(Layout::from_config(&cfg), Layout::default(), "{broken}");
    }
}

// spec: LAY-011, LAY-004
#[test]
fn lay_011_a_panel_shortcut_in_the_ribbon_shows_its_panel_below_the_views_and_closes_again() {
    let mut w = W::new("shortcut");
    w.app.open(std::path::Path::new("Welcome.md"), false);
    // Outline goes to the ribbon, and the sidebar is hidden
    let files = w.ribbon_icon(Item::Files);
    w.drag_and_drop(w.bar_icon(0, panel(RightPanel::Outline)).center(), Pos2::new(files.center().x, files.max.y + 3.0));
    assert!(!w.right_has("Second"));
    w.app.cfg.show_right = false;
    w.frames(3);
    assert!(w.app.side.bars.is_empty(), "no sidebar, no menubars");
    // the shortcut opens the sidebar with a view of its own at the bottom
    w.click_at(w.ribbon_icon(panel(RightPanel::Outline)).center());
    assert!(w.app.cfg.show_right);
    assert_eq!(w.app.side.peek, Some(RightPanel::Outline));
    assert!(w.right_has("Second") && w.right_has("Linked mentions"), "{:?}", w.right_text());
    assert_eq!(w.views().len(), 1, "it is not part of the stored layout");
    // the same click closes it
    w.click_at(w.ribbon_icon(panel(RightPanel::Outline)).center());
    assert_eq!(w.app.side.peek, None);
    assert!(!w.right_has("Second"));
    // and so does the icon in its own menubar
    w.click_at(w.ribbon_icon(panel(RightPanel::Outline)).center());
    let own_bar = w.body(1).min + Vec2::new(14.0, -16.0);
    w.click_at(own_bar);
    assert_eq!(w.app.side.peek, None, "the icon of the shortcut's own menubar closes it");
    // moving the shortcut's icon into a view takes the temporary view away
    w.click_at(w.ribbon_icon(panel(RightPanel::Outline)).center());
    assert_eq!(w.app.side.peek, Some(RightPanel::Outline));
    w.drag_and_drop(w.ribbon_icon(panel(RightPanel::Outline)).center(), Pos2::new(w.bar(0).max.x - 4.0, w.bar(0).center().y));
    assert_eq!(w.app.side.peek, None);
    assert!(w.views()[0].contains(&panel(RightPanel::Outline)));
}

impl W {
    fn right_click_at(&mut self, p: Pos2) {
        self.hl.click(&mut self.app, p, PointerButton::Secondary);
    }

    /// Two views: Backlinks and Outline above, the other panels below.
    fn two_views(&mut self) {
        let view = |icons: &[&str], active: &str| crate::config::RightViewCfg { icons: icons.iter().map(|s| s.to_string()).collect(), active: active.into(), weight: 0 };
        let sidebars = crate::config::SidebarCfg { ribbon: vec![], right: vec![view(&["panel:backlinks", "panel:outline"], "panel:backlinks"), view(&["panel:outgoing", "panel:graph", "panel:calendar"], "panel:outgoing")], hidden: vec![] };
        self.app.side.layout = Layout::from_config(&Config { sidebars, ..Config::default() });
        self.frames(3);
    }
}

// spec: LAY-012
#[test]
fn lay_012_dragging_a_split_border_resizes_two_views_and_the_heights_come_back() {
    let mut w = W::new("border");
    w.two_views();
    assert_eq!(w.app.side.borders.len(), 1);
    let (h0, h1) = (w.body(0).height(), w.body(1).height());
    assert!((h0 - h1).abs() < 4.0, "the views start equal: {h0} and {h1}");
    // the pointer says the border can be dragged
    let grip = w.app.side.borders[0].1.center();
    w.frame(vec![Event::PointerMoved(grip)]);
    assert_eq!(w.hl.cursor, egui::CursorIcon::ResizeVertical);
    // drag it down: the upper view grows by what the lower one loses
    w.drag_and_drop(grip, grip + Vec2::new(0.0, 100.0));
    let (n0, n1) = (w.body(0).height(), w.body(1).height());
    assert!((n0 - (h0 + 100.0)).abs() < 8.0 && (n1 - (h1 - 100.0)).abs() < 8.0, "{h0} {h1} -> {n0} {n1}");
    assert!(w.app.side.layout.views[0].weight > w.app.side.layout.views[1].weight);
    assert_eq!(w.app.cfg.sidebars.right[0].weight, w.app.side.layout.views[0].weight, "stored");
    // a restart draws the same heights
    let w2 = w.restarted("border-2");
    assert_eq!(w2.app.side.layout.views.iter().map(|v| v.weight).collect::<Vec<_>>(), w.app.side.layout.views.iter().map(|v| v.weight).collect::<Vec<_>>());
    assert!((w2.body(0).height() - n0).abs() < 2.0);
    // far past the end of the sidebar: the lower view keeps its minimum
    let grip = w.app.side.borders[0].1.center();
    w.drag_and_drop(grip, grip + Vec2::new(0.0, 2000.0));
    assert!(w.body(1).height() > 20.0, "{}", w.body(1).height());
    assert!(w.bar(1).height() + w.body(1).height() >= crate::sidebar::MIN_VIEW_H - 8.0);
}

// spec: LAY-013, LAY-014
#[test]
fn lay_013_the_icon_menu_removes_restores_and_resets() {
    let mut w = W::new("menu");
    w.two_views();
    // remove Mail from the ribbon
    w.right_click_at(w.ribbon_icon(Item::Mail).center());
    let remove = w.app.side.menu.remove.expect("the right click opens the menu");
    w.click_at(remove.center());
    assert!(!w.ribbon().contains(&Item::Mail));
    assert_eq!(w.app.side.layout.hidden, [Item::Mail]);
    assert_eq!(w.app.cfg.sidebars.hidden, ["mail"], "stored");
    assert_eq!(w.restarted("menu-2").app.side.layout.hidden, [Item::Mail], "it stays hidden after a restart");
    // remove Outline from its view
    w.right_click_at(w.bar_icon(0, panel(RightPanel::Outline)).center());
    w.click_at(w.app.side.menu.remove.unwrap().center());
    assert_eq!(w.views()[0], [panel(RightPanel::Backlinks)]);
    // the menu of another icon lists both hidden ones; restoring Mail puts it at the end of the ribbon
    w.right_click_at(w.ribbon_icon(Item::Files).center());
    assert_eq!(w.app.side.menu.restore.iter().map(|(i, _)| *i).collect::<Vec<_>>(), [Item::Mail, panel(RightPanel::Outline)]);
    w.click_at(w.app.side.menu.restore[0].1.center());
    assert_eq!(w.ribbon().last(), Some(&Item::Mail));
    // the panel comes back at the end of the last view
    w.right_click_at(w.ribbon_icon(Item::Files).center());
    w.click_at(w.app.side.menu.restore[0].1.center());
    assert_eq!(w.views()[1].last(), Some(&panel(RightPanel::Outline)));
    assert!(w.app.side.layout.hidden.is_empty());
    // the last icon of the ribbon cannot be removed
    w.app.side.layout.ribbon = vec![Item::Files];
    w.frames(3);
    w.right_click_at(w.ribbon_icon(Item::Files).center());
    w.click_at(w.app.side.menu.remove.unwrap().center());
    assert_eq!(w.app.side.layout.ribbon, [Item::Files]);
    // reset layout (LAY-014): the default again, hidden icons back too
    w.app.side.layout.hide(panel(RightPanel::Calendar), crate::sidebar::Side::Right);
    w.frames(3);
    w.right_click_at(w.ribbon_icon(Item::Files).center());
    w.click_at(w.app.side.menu.reset.unwrap().center());
    assert_eq!(w.app.side.layout, Layout::default());
    assert_eq!(w.app.cfg.sidebars, crate::config::SidebarCfg { ribbon: w.app.cfg.sidebars.ribbon.clone(), right: w.app.cfg.sidebars.right.clone(), hidden: vec![] });
    assert_eq!(Layout::from_config(&w.app.cfg), Layout::default(), "kept");
}

// spec: LAY-016
#[test]
fn lay_016_the_right_sidebar_keeps_the_width_it_is_dragged_to() {
    let mut w = W::new("width");
    let r = w.app.side.right_rect.unwrap();
    let edge = Pos2::new(r.min.x - 6.0, r.center().y);
    w.drag_and_drop(edge, edge - Vec2::new(80.0, 0.0));
    w.frames(3);
    let n = w.app.side.right_rect.unwrap();
    assert!((n.width() - (r.width() + 80.0)).abs() < 12.0, "{} -> {}", r.width(), n.width());
}

// spec: LAY-017
#[test]
fn lay_016_a_middle_click_on_mail_feeds_or_calendar_opens_a_new_tab() {
    let mut w = W::new("middle");
    for (item, view) in [(Item::Mail, View::Mail), (Item::Social, View::Social(None)), (Item::Calendar, View::Calendar)] {
        let before = w.app.tabbar.tabs.len();
        let first = w.app.tabbar.active;
        let p = w.ribbon_icon(item).center();
        w.hl.click(&mut w.app, p, PointerButton::Middle);
        w.frames(2);
        assert_eq!(w.app.tabbar.tabs.len(), before + 1, "{item:?}: a new tab");
        assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, view);
        assert_ne!(w.app.tabbar.active, first);
    }
    // a middle click on an icon that opens no view does nothing
    let n = w.app.tabbar.tabs.len();
    let p = w.ribbon_icon(Item::Files).center();
    w.hl.click(&mut w.app, p, PointerButton::Middle);
    assert_eq!(w.app.tabbar.tabs.len(), n);
}

// spec: VAULT-025, VAULT-042
#[test]
fn the_title_line_and_the_tab_hide_the_date_prefix_and_renaming_from_the_title_line_keeps_it() {
    let mut w = W::new("title-line");
    std::fs::write(w.dir.join("261117_Plan.md"), "Text of the plan with [[Other]].\n").unwrap();
    std::fs::write(w.dir.join("Other.md"), "Back to [[261117_Plan]].\n").unwrap();
    w.app.vault.as_mut().unwrap().rescan();
    w.app.open(std::path::Path::new("261117_Plan.md"), false);
    w.frames(4);
    let lowest = |w: &W, text: &str| w.hl.text.iter().filter(|(t, _)| t == text).map(|(_, p)| *p).max_by(|a, b| a.y.total_cmp(&b.y));
    let title = lowest(&w, "Plan").expect("the title line shows the name without the prefix");
    assert!(title.y > 40.0, "below the tab bar: {title:?}");
    assert!(w.hl.text.iter().filter(|(t, p)| t == "Plan" && p.y < 40.0).count() == 1, "the tab shows it too: {:?}", w.hl.text);
    // the path in the toolbar is a path: it keeps the full name, like the file list
    let full: Vec<f32> = w.hl.text.iter().filter(|(t, p)| t == "261117_Plan" && p.x > 300.0).map(|(_, p)| p.y).collect();
    assert_eq!(full.len(), 1, "only the path of the toolbar: {:?}", w.hl.text);
    assert!(w.hl.text.iter().any(|(t, p)| t == "261117_Plan" && p.x < 300.0), "the file list keeps the full name: {:?}", w.hl.text);
    // rename from the title line: the prefix stays, the links follow
    w.click_at(title + Vec2::new(10.0, 8.0));
    let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
    w.frame(vec![key(egui::Key::A, Modifiers::COMMAND)]);
    w.frame(vec![Event::Text("Roadmap".into())]);
    w.frame(vec![key(egui::Key::Enter, Modifiers::NONE)]);
    w.frames(4);
    assert!(w.dir.join("261117_Roadmap.md").exists() && !w.dir.join("261117_Plan.md").exists(), "renamed with the prefix");
    assert_eq!(w.app.current_path(), Some(PathBuf::from("261117_Roadmap.md")));
    assert_eq!(std::fs::read_to_string(w.dir.join("Other.md")).unwrap(), "Back to [[261117_Roadmap]].\n", "links are updated as for any rename");
    assert!(lowest(&w, "Roadmap").is_some_and(|p| p.y > 40.0), "the title line shows the new name: {:?}", w.hl.text);
    // a note without a prefix is renamed as before
    w.app.open(std::path::Path::new("Other.md"), false);
    w.frames(4);
    let title = lowest(&w, "Other").unwrap();
    w.click_at(title + Vec2::new(10.0, 8.0));
    w.frame(vec![key(egui::Key::A, Modifiers::COMMAND)]);
    w.frame(vec![Event::Text("Elsewhere".into())]);
    w.frame(vec![key(egui::Key::Enter, Modifiers::NONE)]);
    w.frames(4);
    assert!(w.dir.join("Elsewhere.md").exists());
}

// spec: LINK-007, LINK-014, LINK-015, CFG-042
#[test]
fn the_outline_shows_the_numbers_and_a_click_scrolls_the_reading_view_to_the_heading_and_keeps_the_mode() {
    let mut w = W::new("outline-numbers");
    let long: String = "# Long\n\n## First\n\n### Sub\n\n".to_string() + &"filler line\n\n".repeat(60) + "## Down here\n\n" + &"after\n\n".repeat(40);
    std::fs::write(w.dir.join("Long.md"), &long).unwrap();
    w.app.vault.as_mut().unwrap().rescan();
    w.app.open(std::path::Path::new("Long.md"), false);
    w.frames(4);
    w.click_at(w.bar_icon(0, panel(RightPanel::Outline)).center());
    w.frames(3);
    let outline = w.right_text();
    for want in ["1", "1.1", "1.1.1", "1.2", "Long", "First", "Sub", "Down here"] {
        assert!(outline.iter().any(|t| t == want), "{want} is in the outline: {outline:?}");
    }
    assert!(!outline.iter().any(|t| t == "0"), "no heading is numbered 0");
    // the setting, and the front matter over it, change the open view at once
    w.app.cfg.numbered_headings = false;
    w.frames(3);
    assert!(!w.right_text().iter().any(|t| t == "1.1"), "numbering off: {:?}", w.right_text());
    let doc = w.app.notes.get_mut(std::path::Path::new("Long.md")).unwrap();
    doc.text = format!("---\nnumbered-headings: true\n---\n{}", doc.text);
    w.frames(3);
    assert!(w.right_text().iter().any(|t| t == "1.1"), "the note's front matter turns it on: {:?}", w.right_text());
    w.app.cfg.numbered_headings = true;
    // reading view: the click scrolls, the tab stays in the reading view
    w.app.tabbar.tabs[w.app.tabbar.active].reading = true;
    w.frames(4);
    let y = |w: &W| w.hl.text.iter().find(|(t, p)| t == "Down here" && p.x < w.app.side.right_rect.unwrap().min.x).map(|(_, p)| p.y);
    assert!(y(&w).unwrap() > 1000.0, "far below the window: {:?}", y(&w));
    let at = w.hl.text.iter().find(|(t, p)| t == "Down here" && p.x >= w.app.side.right_rect.unwrap().min.x).unwrap().1;
    w.click_at(at + Vec2::new(4.0, 4.0));
    w.frames(30);
    assert!(w.app.tabbar.tabs[w.app.tabbar.active].reading, "the tab keeps the reading view");
    let top = y(&w).unwrap();
    assert!(top < 300.0, "the heading is at the top of the view: {top}");
    // editor: the cursor goes to the heading line
    w.app.tabbar.tabs[w.app.tabbar.active].reading = false;
    w.frames(3);
    let at = w.hl.text.iter().find(|(t, p)| t == "Down here" && p.x >= w.app.side.right_rect.unwrap().min.x).unwrap().1;
    w.click_at(at + Vec2::new(4.0, 4.0));
    w.frames(3);
    let tab = &w.app.tabbar.tabs[w.app.tabbar.active];
    let doc = &w.app.notes[std::path::Path::new("Long.md")];
    let at = tab.editor.selection_bytes(w.hl.ctx(), &doc.text).start;
    let line = doc.text[..at].matches('\n').count();
    assert_eq!(doc.text.lines().nth(line), Some("## Down here"), "the cursor is on the heading line");
    assert!(!tab.reading);
}

// spec: LAY-018, LAY-014
#[test]
fn lay_018_the_sidebar_widths_are_kept_after_a_restart_and_reset_with_the_layout() {
    // x of the first tab: it moves with the width of the left sidebar
    let tab_x = |w: &W| w.hl.text.iter().filter(|(t, p)| t == "Welcome" && p.y < 40.0).map(|(_, p)| p.x).fold(f32::MAX, f32::min);
    let mut w = W::new("widths");
    assert_eq!((w.app.cfg.left_sidebar_width, w.app.cfg.right_sidebar_width), (None, None), "nothing is stored before a border is dragged");
    let (tab0, right0) = (tab_x(&w), w.app.side.right_rect.unwrap());
    // the left border: the ribbon is 44 wide, the sidebar 260
    let left_edge = Pos2::new(w.app.side.ribbon_rect.unwrap().max.x + 260.0 - 2.0, right0.center().y);
    w.drag_to(left_edge, left_edge + Vec2::new(60.0, 0.0));
    assert_eq!(w.app.cfg.left_sidebar_width, None, "not written while the border is dragged");
    w.release(left_edge + Vec2::new(60.0, 0.0));
    w.frames(3);
    let stored = w.app.cfg.left_sidebar_width.expect("written when the drag ends");
    assert!((stored as f32 - 320.0).abs() < 12.0, "{stored}");
    assert!((tab_x(&w) - (tab0 + 60.0)).abs() < 12.0, "{} -> {}", tab0, tab_x(&w));
    let edge = Pos2::new(right0.min.x - 6.0, right0.center().y);
    w.drag_and_drop(edge, edge - Vec2::new(80.0, 0.0));
    w.frames(3);
    let right = w.app.cfg.right_sidebar_width.expect("written when the drag ends");
    assert!((right as f32 - (right0.width() + 80.0 + 20.0)).abs() < 14.0, "{right} for {}", right0.width());
    assert_eq!(w.app.cfg.left_sidebar_width, Some(stored), "the left width did not change");
    // a new window on this config draws the sidebars at these widths
    let mut n = w.restarted("widths-2");
    assert!((tab_x(&n) - tab_x(&w)).abs() < 1.5, "{} and {}", tab_x(&n), tab_x(&w));
    assert!((n.app.side.right_rect.unwrap().width() - w.app.side.right_rect.unwrap().width()).abs() < 1.5);
    assert_eq!((n.app.cfg.left_sidebar_width, n.app.cfg.right_sidebar_width), (Some(stored), Some(right)));
    // a hidden sidebar keeps its width
    n.app.cfg.show_left = false;
    n.frames(3);
    n.app.cfg.show_left = true;
    n.frames(3);
    assert!((tab_x(&n) - tab_x(&w)).abs() < 1.5, "the left sidebar came back as wide as before");
    // a stored width out of range is clamped
    let mut f = W::new("widths-3");
    f.app.cfg.left_sidebar_width = Some(5000);
    f.app.cfg.right_sidebar_width = Some(1);
    let f = f.restarted("widths-4");
    let narrow = f.app.side.right_rect.unwrap().width();
    assert!(narrow > 150.0 && narrow < right0.width() - 60.0, "clamped up to 200 points outside, not 1: {narrow}");
    // Reset layout (LAY-014) puts both back
    w.right_click_at(w.ribbon_icon(Item::Files).center());
    w.click_at(w.app.side.menu.reset.unwrap().center());
    w.frames(3);
    assert_eq!((w.app.cfg.left_sidebar_width, w.app.cfg.right_sidebar_width), (None, None));
    assert!((tab_x(&w) - tab0).abs() < 1.5, "{} and {}", tab_x(&w), tab0);
    assert!((w.app.side.right_rect.unwrap().width() - right0.width()).abs() < 1.5);
}

/// A note with many links, linked from many notes, with many headings: every panel overflows.
fn long_hub(tag: &str) -> W {
    let mut files: Vec<(String, String)> = (0..40).map(|i| (format!("Src{i}.md"), format!("# Src{i}\n\nSee [[Hub]].\n"))).collect();
    let body: String = (0..40).map(|i| format!("## Head{i}\n\n[[Target{i}]]\n\n")).collect();
    files.push(("Hub.md".into(), format!("# Hub\n\n{body}")));
    let mut w = W::with_files(tag, &files);
    w.app.open(std::path::Path::new("Hub.md"), false);
    w.frames(6);
    w
}

/// The wheel over the panel of a view scrolls it: `probe` is a text at the top of the panel.
fn assert_wheel_scrolls(w: &mut W, body: Rect, probe: &str, what: &str) {
    assert!(!w.right_text().iter().any(|t| t.contains("use of")), "{what}: egui reports an id used twice: {:?}", w.right_text());
    let before = w.right_text_at(probe).unwrap_or_else(|| panic!("{what}: no {probe:?} in {:?}", w.right_text()));
    w.wheel(body.center(), -120.0);
    let after = w.right_text_at(probe).unwrap_or_else(|| panic!("{what}: no {probe:?} after the wheel"));
    assert!(after.y < before.y - 20.0, "{what}: the wheel over the panel scrolls it ({before:?} -> {after:?})");
}

// spec: LAY-003
#[test]
fn lay_003_the_wheel_scrolls_each_of_the_stacked_views() {
    let mut w = long_hub("wheel-stacked");
    let panels = [(RightPanel::Outline, "Head2"), (RightPanel::Backlinks, "Src11"), (RightPanel::Outgoing, "Target3")];
    w.app.side.layout.views = panels.iter().map(|(p, _)| crate::sidebar::View::new(vec![panel(*p)], panel(*p))).collect();
    w.frames(4);
    for (i, (p, probe)) in panels.iter().enumerate() {
        let body = w.body(i);
        assert_wheel_scrolls(&mut w, body, probe, &format!("{p:?} in view {i}"));
    }
}

// spec: LAY-003
#[test]
fn lay_003_the_wheel_scrolls_a_single_view_and_the_panel_of_a_shortcut() {
    let mut w = long_hub("wheel-single");
    assert_eq!(w.views().len(), 1);
    let body = w.body(0);
    assert_wheel_scrolls(&mut w, body, "Src11", "the only view");
    // another panel in the same view starts at its top, not at the offset of the one before
    w.click_at(w.bar_icon(0, panel(RightPanel::Outgoing)).center());
    w.frames(3);
    let top = w.right_text_at("Links").expect("the header of the Outgoing panel is at the top");
    assert!(top.y < w.body(0).min.y + 40.0, "the new panel is not scrolled: {top:?}");
    // the panel of a ribbon shortcut below the view
    w.app.side.peek = Some(RightPanel::Outline);
    w.frames(4);
    let peek = w.body(1);
    assert_wheel_scrolls(&mut w, peek, "Head2", "the shortcut's panel");
}

impl W {
    /// Answer the question "Move or copy?": the buttons are where the window drew them.
    fn answer(&mut self, button: fn(&crate::sidebar_ui::AskButtons) -> Option<Rect>) {
        let at = button(&self.app.side.ask_buttons).expect("the question is open").center();
        self.click_at(at);
        self.frames(2);
    }

    /// Drag a view to the menubar of the first right view (behind its icons).
    fn drag_to_first_bar(&mut self, from: Pos2) {
        let b = self.bar(0);
        self.drag_and_drop(from, Pos2::new(b.max.x - 4.0, b.center().y));
    }

    /// The left of the window: what the left sidebar draws.
    fn left_has(&self, text: &str) -> bool {
        let x = self.app.side.right_rect.map_or(f32::MAX, |r| r.min.x);
        self.hl.text.iter().any(|(t, p)| p.x < x && p.x > 50.0 && t.contains(text))
    }
}

fn movable_places(w: &W, item: Item) -> (bool, bool) {
    (w.app.side.layout.ribbon.contains(&item), w.views().iter().any(|v| v.contains(&item)))
}

// spec: LAY-020, LAY-021, LAY-026
#[test]
fn lay_021_dragging_files_to_the_right_sidebar_asks_and_copy_keeps_both_and_it_survives_a_restart() {
    let mut w = W::new("copy-files");
    let before = w.app.side.layout.clone();
    w.drag_to_first_bar(w.ribbon_icon(Item::Files).center());
    // nothing has changed yet: the question is open, with its three answers
    assert_eq!(w.app.side.layout, before, "the question comes first");
    assert!(w.app.side.ask.is_some() && w.hl.text.iter().any(|(t, _)| t == "Move or copy?"), "{:?}", w.hl.text);
    assert!(w.app.side.ask_buttons.moved.is_some() && w.app.side.ask_buttons.copied.is_some() && w.app.side.ask_buttons.cancel.is_some());
    w.answer(|b| b.copied);
    assert_eq!(w.app.side.ask, None);
    assert_eq!(movable_places(&w, Item::Files), (true, true), "a copy: the ribbon keeps the icon");
    assert_eq!(w.views()[0].last(), Some(&Item::Files), "behind the other icons, where it was dropped");
    assert_eq!(w.app.side.layout.views[0].active, Item::Files);
    assert!(w.right_has("Welcome") && w.left_has("Welcome"), "the file list is drawn on both sides: {:?}", w.hl.text);
    assert_eq!(w.app.cfg.sidebars.right[0].icons.last().map(String::as_str), Some("files"), "stored");
    assert!(w.app.cfg.sidebars.ribbon.contains(&"files".to_string()));
    // after a restart both are there again
    let w2 = w.restarted("copy-files-2");
    assert_eq!(movable_places(&w2, Item::Files), (true, true));
    assert!(w2.right_has("Welcome"), "and the right one draws the list");
}

// spec: LAY-021, LAY-026
#[test]
fn lay_021_moving_search_to_a_new_view_takes_it_from_the_ribbon_and_cancel_or_esc_changes_nothing() {
    let mut w = W::new("move-search");
    let before = w.app.side.layout.clone();
    // Cancel
    w.drag_and_drop(w.ribbon_icon(Item::Search).center(), w.body(0).center());
    assert!(w.app.side.ask.is_some());
    w.answer(|b| b.cancel);
    assert_eq!(w.app.side.layout, before, "Cancel: nothing changed");
    // Esc
    w.drag_and_drop(w.ribbon_icon(Item::Search).center(), w.body(0).center());
    w.press_esc();
    assert_eq!((w.app.side.ask, w.app.side.layout.clone()), (None, before.clone()), "Esc: nothing changed");
    // Move
    w.drag_and_drop(w.ribbon_icon(Item::Search).center(), w.body(0).center());
    w.answer(|b| b.moved);
    assert_eq!(movable_places(&w, Item::Search), (false, true), "a move: the ribbon has lost the icon");
    assert_eq!(w.views().len(), 2, "a new view below the first");
    assert_eq!(w.views()[1], [Item::Search]);
    assert!(w.right_has("Search notes, mail and posts"), "the search box is drawn in the right sidebar: {:?}", w.right_text());
    assert!(!w.ribbon().contains(&Item::Search));
    // the order of the stack is kept after a restart, and so is the move
    let w2 = w.restarted("move-search-2");
    assert_eq!(movable_places(&w2, Item::Search), (false, true));
    assert_eq!(w2.views()[1], [Item::Search]);
}

// spec: LAY-022
#[test]
fn lay_022_dragging_a_view_back_to_the_ribbon_asks_unless_it_is_there_and_a_moved_one_comes_back() {
    let mut w = W::new("back");
    // Tags goes to the right (move) ...
    w.drag_to_first_bar(w.ribbon_icon(Item::Tags).center());
    w.answer(|b| b.moved);
    assert_eq!(movable_places(&w, Item::Tags), (false, true));
    // ... and back between Files and Search: the question again, and a move puts it at the place of the drop
    let between = (w.ribbon_icon(Item::Files).max.y + w.ribbon_icon(Item::Search).min.y) / 2.0;
    let x = w.ribbon_icon(Item::Files).center().x;
    w.drag_and_drop(w.bar_icon(0, Item::Tags).center(), Pos2::new(x, between));
    assert!(w.app.side.ask.is_some());
    w.answer(|b| b.moved);
    assert_eq!(movable_places(&w, Item::Tags), (true, false));
    assert_eq!(&w.app.side.layout.ribbon[..3], [Item::Files, Item::Tags, Item::Search]);
    // a copy in a view dragged to the ribbon, where the view is already: no question, the copy is taken out of the view
    w.drag_to_first_bar(w.ribbon_icon(Item::Tags).center());
    w.answer(|b| b.copied);
    assert_eq!(movable_places(&w, Item::Tags), (true, true));
    w.drag_and_drop(w.bar_icon(0, Item::Tags).center(), Pos2::new(x, between));
    assert_eq!((w.app.side.ask, movable_places(&w, Item::Tags)), (None, (true, false)), "no question: it is in the ribbon already");
    // reordering inside the right menubar asks nothing either
    w.drag_to_first_bar(w.ribbon_icon(Item::Search).center());
    w.answer(|b| b.copied);
    let first = w.bar_icon(0, panel(RightPanel::Backlinks));
    w.drag_and_drop(w.bar_icon(0, Item::Search).center(), Pos2::new(first.center().x - 6.0, first.center().y));
    assert_eq!((w.app.side.ask, w.views()[0][0]), (None, Item::Search), "moved to the front of the bar");
}

// spec: LAY-006, LAY-020
#[test]
fn lay_020_the_main_views_without_a_pane_and_commands_are_still_refused_on_the_right_and_a_movable_view_is_accepted() {
    let mut w = W::new("movable-cue");
    for item in [Item::Graph, Item::Tasks, Item::Import] {
        w.drag_to(w.ribbon_icon(item).center(), w.bar(0).center());
        assert_eq!(w.app.side.cue, Cue::Refused, "{item:?}");
        w.release(w.bar(0).center());
        assert_eq!(w.app.side.ask, None);
    }
    w.drag_to(w.ribbon_icon(Item::Files).center(), w.body(0).center());
    assert!(matches!(w.app.side.cue, Cue::Zone { .. }), "{:?}", w.app.side.cue);
    assert_eq!(w.hl.cursor, egui::CursorIcon::Grabbing);
    w.release(w.body(0).center());
    w.press_esc();
}

// spec: LAY-024
#[test]
fn lay_024_the_icon_menu_moves_and_copies_a_view_to_the_other_side_without_the_mouse_drag() {
    let mut w = W::new("menu-move");
    // ribbon -> right: Copy; the entry goes when the view is on the right already
    w.right_click_at(w.ribbon_icon(Item::Tags).center());
    let (_, copy) = w.app.side.menu.to_other.expect("Files, Search and Tags have the entries");
    w.click_at(copy.center());
    assert_eq!(movable_places(&w, Item::Tags), (true, true));
    assert_eq!(w.views()[0].last(), Some(&Item::Tags), "at the end of the last view");
    w.right_click_at(w.ribbon_icon(Item::Tags).center());
    assert!(w.app.side.menu.to_other.is_none(), "it is there already: nothing to move or copy");
    w.press_esc();
    // right -> left: the entries of a right menubar are not offered while the ribbon has the view
    w.right_click_at(w.bar_icon(0, Item::Tags).center());
    assert!(w.app.side.menu.to_other.is_none());
    // Remove from this view takes the copy only, and the view is not hidden
    w.click_at(w.app.side.menu.remove.unwrap().center());
    assert_eq!((movable_places(&w, Item::Tags), w.app.side.layout.hidden.is_empty()), ((true, false), true));
    // Move to the right sidebar (a new view when there is none), then back with Move to the left sidebar
    w.app.side.layout.views.clear();
    w.frames(3);
    w.right_click_at(w.ribbon_icon(Item::Search).center());
    let (mv, _) = w.app.side.menu.to_other.unwrap();
    w.click_at(mv.center());
    assert_eq!((movable_places(&w, Item::Search), w.views()), ((false, true), vec![vec![Item::Search]]));
    w.right_click_at(w.bar_icon(0, Item::Search).center());
    let (mv, _) = w.app.side.menu.to_other.expect("the ribbon has lost it: Move and Copy to the left");
    w.click_at(mv.center());
    assert_eq!((movable_places(&w, Item::Search), w.views().len()), ((true, false), 0));
    assert_eq!(w.app.side.layout.ribbon.last(), Some(&Item::Search), "at the end of the ribbon");
    // the main views that have no pane yet have no such entries
    w.right_click_at(w.ribbon_icon(Item::Graph).center());
    assert!(w.app.side.menu.to_other.is_none());
}

// spec: LAY-025
#[test]
fn lay_025_the_left_sidebar_shows_the_next_view_when_its_own_has_moved_and_says_so_when_none_is_left() {
    let mut w = W::new("left-fallback");
    assert!(w.app.side.left == crate::app::LeftPanel::Files && w.left_has("Welcome"));
    for item in [Item::Files, Item::Search] {
        w.right_click_at(w.ribbon_icon(item).center());
        w.click_at(w.app.side.menu.to_other.unwrap().0.center());
    }
    assert_eq!(w.app.left_view(), Some(crate::app::LeftPanel::Tags), "Files and Search are on the right: Tags is the first one left");
    w.right_click_at(w.ribbon_icon(Item::Tags).center());
    w.click_at(w.app.side.menu.to_other.unwrap().0.center());
    assert_eq!(w.app.left_view(), None);
    assert!(w.left_has("No view is here"), "{:?}", w.hl.text);
    // a shortcut for Search shows it where it is: in the right sidebar
    w.app.cfg.show_right = false;
    w.app.show_left_view(crate::app::LeftPanel::Search);
    w.frames(2);
    assert!(w.app.cfg.show_right && w.app.side.layout.views.iter().any(|v| v.active == Item::Search));
    // the left one is back with the first icon that comes back to the ribbon
    w.right_click_at(w.bar_icon(0, Item::Files).center());
    w.click_at(w.app.side.menu.to_other.unwrap().0.center());
    assert_eq!(w.app.left_view(), Some(crate::app::LeftPanel::Files));
    assert!(w.left_has("Welcome"));
}

// spec: LAY-024
#[test]
fn lay_024_two_copies_of_a_view_share_one_state() {
    let mut w = W::new("shared-state");
    w.right_click_at(w.ribbon_icon(Item::Search).center());
    w.click_at(w.app.side.menu.to_other.unwrap().1.center());
    w.app.side.left = crate::app::LeftPanel::Search;
    w.app.search.query = "Welcome".into();
    // (the list comes from the search worker: wait for its answer, not for a number of frames)
    w.hl.settle(&mut w.app);
    w.frames(2);
    assert!(w.left_has("Welcome") && w.right_has("Welcome"), "the same search is shown on both sides: {:?}", w.hl.text);
    assert!(w.app.side.layout.on_left(Item::Search) && w.app.side.layout.on_right(Item::Search));
}

impl W {
    /// An account with the demo store (Inbox with 8 mails, 2 of them unread, Sent, Trash, Projects/Hall B);
    /// returns when the index is open.
    fn with_mail(tag: &str) -> W {
        let mut w = W::new(tag);
        crate::mail::demo::seed(&w.app.mail.root).unwrap();
        w.app.cfg.mail_accounts = vec![crate::config::MailAccount { id: crate::mail::demo::ACCOUNT.into(), name: "Demo mail".into(), host: "imap.example.com".into(), user: "ada@example.com".into(), ..Default::default() }];
        // no sync, no watch, no timer that marks a message read and no server: only what the test does changes the index
        w.app.cfg.mail_sync_minutes = 0;
        w.app.cfg.mail_mark_read = false;
        w.app.mail.watch.enabled = false;
        w.app.mail.factory = std::sync::Arc::new(|_, _| Err("no server in this test".to_string()));
        w.hl.settle(&mut w.app);
        w.frames(3);
        w
    }

    /// Mail copied to the first right view (the way the icon menu does it).
    fn copy_mail_right(&mut self) {
        self.right_click_at(self.ribbon_icon(Item::Mail).center());
        self.click_at(self.app.side.menu.to_other.expect("Mail has the entries").1.center());
        self.frames(3);
    }

    /// Where the pane of the right sidebar drew a control (see `MailPane::rects`).
    fn pane(&self, name: &str) -> Rect {
        let rects = &self.app.side.panes.mail.rects;
        rects.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("the Mail pane drew no {name}: {:?}", rects.iter().map(|(n, _)| n).collect::<Vec<_>>())).1
    }

    fn mail_folder(&self, name: &str) -> i64 {
        self.app.mail.folders.iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no folder {name}")).id
    }

    fn pane_unread(&self) -> usize {
        self.app.side.panes.mail.rects.iter().filter(|(n, _)| n.starts_with("mail:unread:")).count()
    }
}

// spec: LAY-028
#[test]
fn lay_028_dragging_mail_to_the_right_sidebar_asks_and_both_answers_work_and_the_ribbon_icon_still_opens_the_tab() {
    let mut w = W::with_mail("mail-drag");
    let before = w.app.side.layout.clone();
    // the drop is no longer refused: the cue is the bar's and the question opens
    let at = Pos2::new(w.bar(0).max.x - 4.0, w.bar(0).center().y);
    w.drag_to(w.ribbon_icon(Item::Mail).center(), at);
    assert!(matches!(w.app.side.cue, Cue::Bar { .. }), "{:?}", w.app.side.cue);
    assert_eq!(w.hl.cursor, egui::CursorIcon::Grabbing);
    w.release(at);
    assert!(w.app.side.ask.is_some());
    assert_eq!(w.app.side.layout, before, "nothing changed before the answer");
    w.answer(|b| b.copied);
    assert_eq!(movable_places(&w, Item::Mail), (true, true), "a copy: the ribbon keeps it");
    assert_eq!(w.app.side.layout.views[0].active, Item::Mail);
    assert_eq!(w.app.cfg.sidebars.right[0].icons.last().map(String::as_str), Some("mail"), "stored under its own id");
    // the ribbon icon still opens the Mail tab; the pane stays
    w.click_at(w.ribbon_icon(Item::Mail).center());
    w.frames(2);
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Mail);
    assert!(w.right_has("INBOX"), "{:?}", w.right_text());
    // a drag of the copy to the ribbon, which has the view: the copy leaves the right side, nothing is asked
    w.drag_and_drop(w.bar_icon(0, Item::Mail).center(), w.ribbon_icon(Item::Files).center());
    assert_eq!(movable_places(&w, Item::Mail), (true, false));
    // Move to the right: the ribbon loses it; the left sidebar never shows it as a pane
    w.drag_to_first_bar(w.ribbon_icon(Item::Mail).center());
    w.answer(|b| b.moved);
    assert_eq!(movable_places(&w, Item::Mail), (false, true));
    assert!(w.app.left_view().is_some(), "the left sidebar shows Files, Search or Tags");
    assert!(w.right_has("INBOX"));
    // it survives a restart
    let w2 = w.restarted("mail-drag-2");
    assert_eq!(movable_places(&w2, Item::Mail), (false, true));
}

// spec: LAY-028, LAY-006
#[test]
fn lay_028_the_icon_menu_moves_mail_and_back_and_graph_and_tasks_are_still_refused() {
    let mut w = W::with_mail("mail-menu");
    w.right_click_at(w.ribbon_icon(Item::Mail).center());
    let (mv, _) = w.app.side.menu.to_other.expect("Mail has Move and Copy");
    w.click_at(mv.center());
    assert_eq!((movable_places(&w, Item::Mail), w.views()[0].last()), ((false, true), Some(&Item::Mail)));
    w.right_click_at(w.bar_icon(0, Item::Mail).center());
    let (mv, _) = w.app.side.menu.to_other.expect("and back");
    w.click_at(mv.center());
    assert_eq!(movable_places(&w, Item::Mail), (true, false));
    assert_eq!(w.app.side.layout.ribbon.last(), Some(&Item::Mail));
    // the main views without a pane are refused (LAY-006)
    for item in [Item::Graph, Item::Tasks] {
        w.right_click_at(w.ribbon_icon(item).center());
        assert!(w.app.side.menu.to_other.is_none(), "{item:?}");
        w.press_esc();
        w.drag_to(w.ribbon_icon(item).center(), w.bar(0).center());
        assert_eq!(w.app.side.cue, Cue::Refused, "{item:?}");
        w.release(w.bar(0).center());
        assert_eq!(movable_places(&w, item), (true, false));
    }
}

// spec: LAY-030
#[test]
fn lay_030_the_pane_lists_the_folders_with_unread_counts_and_the_messages_with_their_unread_state() {
    let mut w = W::with_mail("mail-pane");
    w.copy_mail_right();
    // the first folder of the first account, the Inbox, is listed
    let text = w.right_text();
    assert!(text.iter().any(|t| t.starts_with("INBOX") && t.contains("(2)")), "{text:?}");
    for name in ["Sent", "Trash", "Hall B"] {
        assert!(w.right_has(name), "{name}: {text:?}");
    }
    assert!(w.right_has("8 messages"), "{text:?}");
    assert!(w.right_has("Lecture hall B"), "{text:?}");
    assert_eq!(w.pane_unread(), 2, "two of the eight are unread");
    // a folder click lists that folder: Sent has one mail, read
    w.click_at(w.pane(&format!("mailpane:folder:{}", w.mail_folder("Sent"))).center());
    w.frames(2);
    assert!(w.right_has("1 message") && !w.right_has("1 messages"), "{:?}", w.right_text());
    assert_eq!(w.pane_unread(), 0);
    assert_eq!(w.app.side.panes.mail.folder, Some(w.mail_folder("Sent")));
}

// spec: LAY-030
#[test]
fn lay_030_a_click_on_a_message_opens_it_in_the_mail_tab_and_opens_the_tab_when_there_is_none() {
    let mut w = W::with_mail("mail-click");
    w.copy_mail_right();
    let mail_tabs = |w: &W| w.app.tabbar.tabs.iter().filter(|t| t.view == View::Mail).count();
    assert_eq!(mail_tabs(&w), 0, "no Mail tab yet");
    w.click_at(w.pane("mailpane:row:1").center());
    w.frames(3);
    assert_eq!(mail_tabs(&w), 1, "the tab was opened");
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Mail);
    let clicked = w.app.side.panes.mail.selected.expect("the pane marks the message");
    assert_eq!(w.app.mail.open.as_ref().map(|o| o.msg.id), Some(clicked), "the tab shows that message");
    assert_eq!(w.app.mail.folder, w.app.side.panes.mail.folder);
    // another message: the same tab
    w.click_at(w.pane("mailpane:row:3").center());
    w.frames(3);
    assert_eq!(mail_tabs(&w), 1, "no second tab");
    assert_ne!(w.app.mail.open.as_ref().map(|o| o.msg.id), Some(clicked));
    assert_eq!(w.app.mail.open.as_ref().map(|o| o.msg.id), w.app.side.panes.mail.selected);
}

// spec: LAY-032
#[test]
fn lay_032_the_pane_says_why_it_is_empty() {
    // no account
    let mut w = W::new("mail-none");
    w.copy_mail_right();
    assert!(w.right_has("No mail account yet"), "{:?}", w.right_text());
    assert!(w.app.side.panes.mail.rects.is_empty());
    // the index is opening, or could not be opened
    w.app.cfg.mail_accounts = vec![crate::config::MailAccount { id: "a".into(), name: "A".into(), ..Default::default() }];
    w.app.mail.store = crate::mail::ui::StoreState::Opening;
    w.frames(2);
    assert!(w.right_has("Opening the mail index"), "{:?}", w.right_text());
    w.app.mail.store = crate::mail::ui::StoreState::Failed("disk full".into());
    w.frames(2);
    assert!(w.right_has("could not be opened: disk full"), "{:?}", w.right_text());
    // an empty folder, and an account that has no folder yet
    let mut w = W::with_mail("mail-empty");
    w.copy_mail_right();
    w.click_at(w.pane(&format!("mailpane:folder:{}", w.mail_folder("Trash"))).center());
    w.frames(2);
    assert!(w.right_has("No messages here."), "{:?}", w.right_text());
    w.app.cfg.mail_accounts.push(crate::config::MailAccount { id: "empty".into(), name: "Empty account".into(), ..Default::default() });
    w.frames(2);
    w.click_at(w.pane("mailpane:account:empty").center());
    w.frames(2);
    assert!(w.right_has("No folders yet"), "{:?}", w.right_text());
    assert_eq!(w.app.side.panes.mail.folder, None);
    // back to the first account: its first folder is listed again
    w.click_at(w.pane(&format!("mailpane:account:{}", crate::mail::demo::ACCOUNT)).center());
    w.frames(2);
    assert_eq!(w.app.side.panes.mail.folder, Some(w.mail_folder("INBOX")));
}

// spec: LAY-031
#[test]
fn lay_031_the_pane_and_the_tab_choose_their_folders_separately() {
    let mut w = W::with_mail("mail-independent");
    w.copy_mail_right();
    let (inbox, sent, trash) = (w.mail_folder("INBOX"), w.mail_folder("Sent"), w.mail_folder("Trash"));
    assert_eq!((w.app.side.panes.mail.folder, w.app.mail.folder), (Some(inbox), Some(inbox)));
    // the pane goes to Sent: the tab's folder stays
    w.click_at(w.pane(&format!("mailpane:folder:{sent}")).center());
    w.frames(2);
    assert_eq!((w.app.side.panes.mail.folder, w.app.mail.folder), (Some(sent), Some(inbox)));
    // the tab goes to Trash: the pane stays at Sent
    let cfg = w.app.cfg.clone();
    w.app.mail.select_folder(&cfg, trash);
    w.frames(3);
    assert_eq!((w.app.side.panes.mail.folder, w.app.mail.folder), (Some(sent), Some(trash)));
    assert!(w.right_has("Re: Schedule for the autumn workshop"), "{:?}", w.right_text());
    // a message selected in the tab is not the pane's selection
    w.app.mail.cursor = Some(0);
    w.frames(2);
    assert_eq!(w.app.side.panes.mail.selected, None);
}

// spec: LAY-031
#[test]
fn lay_031_the_pane_and_the_tab_scroll_separately_and_share_one_index() {
    let mut w = W::with_mail("mail-scroll");
    w.copy_mail_right();
    // a low window: eight rows do not fit in the pane
    w.hl.set_size(Vec2::new(1500.0, 420.0));
    w.click_at(w.ribbon_icon(Item::Mail).center());
    w.frames(4);
    assert_eq!(w.app.side.panes.mail.scroll, 0.0);
    let list = w.pane("mailpane:row:0");
    w.wheel(list.center(), -120.0);
    assert!(w.app.side.panes.mail.scroll > 0.0, "the pane's list scrolled: {}", w.app.side.panes.mail.scroll);
    assert_eq!(w.app.mail.visible.start, 0, "the tab's list did not");
    // what the tab changes in the index shows in the pane: it reads the newest unread mail
    w.wheel(list.center(), 500.0);
    assert_eq!(w.pane_unread(), 2);
    w.app.mail.cursor = Some(0);
    let m = w.app.mail.cursor_message().expect("the tab has a first message");
    let cfg = w.app.cfg.clone();
    w.app.mail.toggle_read_on(&cfg, vec![m]);
    w.frames(3);
    assert_eq!(w.pane_unread(), 1, "the pane read the index again");
}

impl W {
    /// A local calendar with the sample events (a normal day today, all-day events, a busy day).
    fn with_calendar(tag: &str) -> W {
        let mut w = W::new(tag);
        let src = w.app.calendar.new_local_source("Test", &[]);
        let id = src.id.clone();
        w.app.cfg.calendars = vec![src];
        w.app.calendar.set_sources(w.app.cfg.calendars.clone());
        w.app.calendar.fill_sample(&id).unwrap();
        w.hl.settle(&mut w.app);
        w.frames(3);
        w
    }

    /// An item copied to the first right view (the way the icon menu does it).
    fn copy_right(&mut self, item: Item) {
        self.right_click_at(self.ribbon_icon(item).center());
        self.click_at(self.app.side.menu.to_other.expect("the item has the entries").1.center());
        self.frames(3);
    }

    /// Where the Calendar pane of the right sidebar drew a control (see `CalendarPane::rects`).
    fn cal_pane(&self, name: &str) -> Rect {
        let rects = &self.app.side.panes.calendar.rects;
        rects.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("the Calendar pane drew no {name}: {:?}", rects.iter().map(|(n, _)| n).collect::<Vec<_>>())).1
    }

    fn cal_pane_has(&self, name: &str) -> bool {
        self.app.side.panes.calendar.rects.iter().any(|(n, _)| n == name)
    }

    fn cal_pane_count(&self, prefix: &str) -> usize {
        self.app.side.panes.calendar.rects.iter().filter(|(n, _)| n.starts_with(prefix)).count()
    }

    fn calendar_tabs(&self) -> usize {
        self.app.tabbar.tabs.iter().filter(|t| t.view == View::Calendar).count()
    }
}

/// "November 2026", the title of the month `day` is in.
fn month_title(day: i64) -> String {
    let (y, m, _) = crate::calendar::time::civil_from_days(day);
    format!("{} {y}", crate::calendar::time::MONTHS[m as usize - 1])
}

// spec: LAY-029
#[test]
fn lay_029_dragging_calendar_to_the_right_sidebar_asks_and_both_answers_work_and_the_ribbon_icon_still_opens_the_tab() {
    let mut w = W::with_calendar("cal-drag");
    let before = w.app.side.layout.clone();
    let at = Pos2::new(w.bar(0).max.x - 4.0, w.bar(0).center().y);
    w.drag_to(w.ribbon_icon(Item::Calendar).center(), at);
    assert!(matches!(w.app.side.cue, Cue::Bar { .. }), "{:?}", w.app.side.cue);
    w.release(at);
    assert!(w.app.side.ask.is_some());
    assert_eq!(w.app.side.layout, before, "nothing changed before the answer");
    w.answer(|b| b.copied);
    assert_eq!(movable_places(&w, Item::Calendar), (true, true), "a copy: the ribbon keeps it");
    assert_eq!(w.app.side.layout.views[0].active, Item::Calendar);
    assert_eq!(w.app.cfg.sidebars.right[0].icons.last().map(String::as_str), Some("calendar"), "stored under its own id");
    // the ribbon icon still opens the Calendar tab; the pane stays
    w.click_at(w.ribbon_icon(Item::Calendar).center());
    w.frames(2);
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Calendar);
    assert!(w.right_has(&month_title(crate::calendar::time::today())), "{:?}", w.right_text());
    // a drag of the copy to the ribbon, which has the view: the copy leaves the right side, nothing is asked
    w.drag_and_drop(w.bar_icon(0, Item::Calendar).center(), w.ribbon_icon(Item::Files).center());
    assert_eq!(movable_places(&w, Item::Calendar), (true, false));
    // Move to the right: the ribbon loses it; the left sidebar never shows it as a pane
    w.drag_to_first_bar(w.ribbon_icon(Item::Calendar).center());
    w.answer(|b| b.moved);
    assert_eq!(movable_places(&w, Item::Calendar), (false, true));
    assert!(w.app.left_view().is_some(), "the left sidebar shows Files, Search or Tags");
    assert!(w.right_has(&month_title(crate::calendar::time::today())));
    // it survives a restart
    let w2 = w.restarted("cal-drag-2");
    assert_eq!(movable_places(&w2, Item::Calendar), (false, true));
}

// spec: LAY-029
#[test]
fn lay_029_the_icon_menu_moves_calendar_and_back_and_the_calendar_panel_is_another_icon() {
    let mut w = W::with_calendar("cal-menu");
    w.right_click_at(w.ribbon_icon(Item::Calendar).center());
    let (mv, _) = w.app.side.menu.to_other.expect("Calendar has Move and Copy");
    w.click_at(mv.center());
    assert_eq!((movable_places(&w, Item::Calendar), w.views()[0].last()), ((false, true), Some(&Item::Calendar)));
    assert!(w.views().iter().any(|v| v.contains(&panel(RightPanel::Calendar))), "the Calendar panel is still there");
    w.right_click_at(w.bar_icon(0, Item::Calendar).center());
    let (mv, _) = w.app.side.menu.to_other.expect("and back");
    w.click_at(mv.center());
    assert_eq!(movable_places(&w, Item::Calendar), (true, false));
    assert_eq!(w.app.side.layout.ribbon.last(), Some(&Item::Calendar));
}

// spec: LAY-033
#[test]
fn lay_033_the_pane_shows_the_month_with_today_and_busy_days_and_the_agenda_of_the_next_days() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-pane");
    w.copy_right(Item::Calendar);
    let today = t::today();
    let text = w.right_text();
    assert!(text.iter().any(|x| x == &month_title(today)), "{text:?}");
    assert!(w.cal_pane_has(&format!("calpane:today:{today}")), "today is marked");
    assert_eq!(w.cal_pane_count("calpane:today:"), 1);
    assert!(w.cal_pane_has(&format!("calpane:busy:{today}")), "a day with events has its dot");
    // the agenda starts today: the events with their times, all-day events with "all day"
    assert!(w.right_has("Today · "), "{text:?}");
    for e in ["Design review", "Lunch with Sam", "Focus time"] {
        assert!(w.cal_pane_has(&format!("calpane:event:{e}:{today}")), "{e}");
    }
    for time in ["10:00", "12:30", "14:00"] {
        assert!(w.right_has(time), "{time}: {text:?}");
    }
    let (review, lunch) = (w.cal_pane(&format!("calpane:event:Design review:{today}")), w.cal_pane(&format!("calpane:event:Lunch with Sam:{today}")));
    assert!(review.min.y < lunch.min.y, "by time");
    assert!(w.right_has("Tomorrow · ") && w.cal_pane_has(&format!("calpane:event:Report deadline:{}", today + 1)));
    assert!(w.right_has("all day"));
    // the agenda reaches 14 days from the chosen day, the chosen day included: the conference on day 7 is in, the holiday on day 14 is not
    assert_eq!(crate::calendar::sidebar_pane::AGENDA_DAYS, 14);
    assert!(w.cal_pane_has(&format!("calpane:event:Conference:{}", today + 7)));
    assert!(!w.right_has("Public holiday"), "{text:?}");
}

// spec: LAY-033
#[test]
fn lay_033_all_day_events_come_before_the_timed_ones_of_their_day() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-allday");
    w.copy_right(Item::Calendar);
    // the three days of the conference: on a weekday the team standup (09:00) is there too
    let weekday = (7..=9).map(|n| t::today() + n).find(|d| t::weekday(*d) < 5).expect("three days have a weekday");
    w.click_at(w.cal_pane(&format!("calpane:day:{weekday}")).center());
    w.frames(2);
    let (conf, standup) = (w.cal_pane(&format!("calpane:event:Conference:{weekday}")), w.cal_pane(&format!("calpane:event:Team standup:{weekday}")));
    assert!(conf.min.y < standup.min.y, "all day first, though the standup is at 09:00");
}

// spec: LAY-033
#[test]
fn lay_033_the_month_is_navigated_with_the_arrows_and_today_brings_it_back() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-nav");
    w.copy_right(Item::Calendar);
    let today = t::today();
    let this_month = w.app.side.panes.calendar.shown_month();
    assert_eq!(this_month, today - t::civil_from_days(today).2 as i64 + 1);
    w.click_at(w.cal_pane("calpane:next").center());
    w.frames(2);
    let next = w.app.side.panes.calendar.shown_month();
    assert!(next > this_month && w.right_has(&month_title(next)), "{:?}", w.right_text());
    assert_eq!(w.app.side.panes.calendar.chosen_day(), today, "the agenda stays where it was");
    // two months back from this one (a month on from the arrow above): no event is there
    for _ in 0..3 {
        w.click_at(w.cal_pane("calpane:prev").center());
        w.frames(2);
    }
    let (y, m, _) = t::civil_from_days(this_month);
    let (y, m) = t::add_months(y, m, -2);
    assert_eq!(w.app.side.panes.calendar.shown_month(), t::days_from_civil(y, m, 1), "two months back");
    let dots: Vec<_> = w.app.side.panes.calendar.rects.iter().filter(|(n, _)| n.starts_with("calpane:busy:")).map(|(n, _)| n.clone()).collect();
    assert_eq!(dots, Vec::<String>::new(), "no dots two months ago");
    assert!(w.right_has(&month_title(w.app.side.panes.calendar.shown_month())));
    // Today: the month and the day of today
    w.click_at(w.cal_pane("calpane:today").center());
    w.frames(2);
    assert_eq!((w.app.side.panes.calendar.shown_month(), w.app.side.panes.calendar.selected, w.app.side.panes.calendar.month), (this_month, None, None));
    assert!(w.cal_pane_has(&format!("calpane:busy:{today}")));
}

// spec: LAY-033
#[test]
fn lay_033_a_click_on_a_day_moves_the_agenda_to_that_day() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-day");
    w.copy_right(Item::Calendar);
    // the busy day two days from now (its month may be the next one)
    let busy = t::today() + 2;
    while w.app.side.panes.calendar.shown_month() != busy - t::civil_from_days(busy).2 as i64 + 1 {
        w.click_at(w.cal_pane("calpane:next").center());
        w.frames(2);
    }
    assert!(w.cal_pane_has(&format!("calpane:event:Design review:{}", t::today())), "the agenda starts today");
    w.click_at(w.cal_pane(&format!("calpane:day:{busy}")).center());
    w.frames(2);
    assert_eq!(w.app.side.panes.calendar.selected, Some(busy));
    assert!(w.cal_pane_has(&format!("calpane:event:Retro:{busy}")), "{:?}", w.right_text());
    assert!(!w.cal_pane_has(&format!("calpane:event:Design review:{}", t::today())), "today is before the chosen day");
    assert!(w.right_has(&t::long_date(busy)));
    // the agenda is 14 days from there: the holiday (today + 14) is in now
    assert!(w.cal_pane_has(&format!("calpane:event:Public holiday:{}", t::today() + 14)));
}

// spec: LAY-033
#[test]
fn lay_033_a_click_on_an_event_opens_it_in_the_calendar_tab_and_opens_the_tab_when_there_is_none() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-click");
    w.copy_right(Item::Calendar);
    assert_eq!(w.calendar_tabs(), 0, "no Calendar tab yet");
    let today = t::today();
    w.click_at(w.cal_pane(&format!("calpane:event:Design review:{today}")).center());
    w.frames(3);
    assert_eq!(w.calendar_tabs(), 1, "the tab was opened");
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Calendar);
    assert_eq!((w.app.cal.main.mode, w.app.cal.main.selected), (crate::calendar::ui::Mode::Month, today));
    let shown = w.app.cal.main.popup.as_ref().and_then(|p| w.app.calendar.event(&p.occ)).map(|e| e.summary.clone());
    assert_eq!(shown.as_deref(), Some("Design review"), "the tab shows the event's details");
    // another event: the same tab
    w.click_at(w.cal_pane(&format!("calpane:event:Lunch with Sam:{today}")).center());
    w.frames(3);
    assert_eq!(w.calendar_tabs(), 1, "no second tab");
    let shown = w.app.cal.main.popup.as_ref().and_then(|p| w.app.calendar.event(&p.occ)).map(|e| e.summary.clone());
    assert_eq!(shown.as_deref(), Some("Lunch with Sam"));
}

// spec: LAY-031
#[test]
fn lay_031_the_pane_and_the_tab_show_their_own_month_and_day() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-independent");
    w.copy_right(Item::Calendar);
    let today = t::today();
    w.click_at(w.ribbon_icon(Item::Calendar).center());
    w.frames(3);
    // the pane goes to the next month, the tab stays
    let (tab_anchor, tab_selected) = (w.app.cal.main.anchor, w.app.cal.main.selected);
    w.click_at(w.cal_pane("calpane:next").center());
    w.frames(2);
    assert!(w.app.side.panes.calendar.shown_month() > today);
    assert_eq!((w.app.cal.main.anchor, w.app.cal.main.selected), (tab_anchor, tab_selected));
    // the tab goes to another day and month, the pane stays
    let pane_month = w.app.side.panes.calendar.shown_month();
    w.app.cal.main.anchor = today + 90;
    w.app.cal.main.selected = today + 90;
    w.frames(3);
    assert_eq!((w.app.side.panes.calendar.shown_month(), w.app.side.panes.calendar.selected), (pane_month, None));
    assert!(w.right_has(&month_title(pane_month)));
    // a day chosen in the pane is not the tab's selection
    let d = pane_month + 3;
    w.click_at(w.cal_pane(&format!("calpane:day:{d}")).center());
    w.frames(2);
    assert_eq!((w.app.side.panes.calendar.selected, w.app.cal.main.selected), (Some(d), today + 90));
    // the Calendar panel of the right sidebar is a third state
    assert_eq!(w.app.cal.side.selected, today);
}

// spec: LAY-031
#[test]
fn lay_031_the_calendar_pane_scrolls_on_its_own_and_shares_the_events_of_the_tab() {
    use crate::calendar::time as t;
    let mut w = W::with_calendar("cal-scroll");
    w.copy_right(Item::Calendar);
    w.hl.set_size(Vec2::new(1500.0, 420.0));
    w.click_at(w.ribbon_icon(Item::Calendar).center());
    w.frames(4);
    assert_eq!(w.app.side.panes.calendar.scroll, 0.0);
    let at = w.cal_pane(&format!("calpane:day:{}", t::today())).center();
    w.wheel(at, -300.0);
    assert!(w.app.side.panes.calendar.scroll > 0.0, "the pane scrolled: {}", w.app.side.panes.calendar.scroll);
    // an event the tab deletes or adds shows in the pane: the same store, one cache
    w.wheel(at, 1000.0);
    assert!(w.cal_pane_has(&format!("calpane:event:Gym:{}", t::today())));
    let id = w.app.calendar.sources[0].id.clone();
    let gym = w.app.calendar.events[&id].iter().find(|e| e.summary == "Gym").cloned().unwrap();
    crate::calendar::store::tests::wait(&w.app.calendar.delete_event(&id, gym, None).unwrap()).unwrap();
    w.app.calendar.after_write(std::slice::from_ref(&id));
    w.frames(3);
    assert!(!w.cal_pane_has(&format!("calpane:event:Gym:{}", t::today())), "the pane read the store again");
}

// spec: LAY-036
#[test]
fn lay_036_the_pane_says_why_it_is_empty() {
    // no calendar
    let mut w = W::new("cal-none");
    w.copy_right(Item::Calendar);
    assert!(w.right_has("No calendar yet"), "{:?}", w.right_text());
    assert!(w.app.side.panes.calendar.rects.is_empty());
    // a calendar without events: the month is there, the agenda says so
    let src = w.app.calendar.new_local_source("Empty", &[]);
    w.app.cfg.calendars = vec![src];
    w.app.calendar.set_sources(w.app.cfg.calendars.clone());
    w.frames(3);
    assert!(w.right_has(&format!("Nothing in the next {} days.", crate::calendar::sidebar_pane::AGENDA_DAYS)), "{:?}", w.right_text());
    assert!(w.cal_pane_has("calpane:next") && w.cal_pane_count("calpane:event:") == 0);
}

// spec: LAY-036
#[test]
fn lay_036_the_pane_says_so_while_the_calendars_load() {
    if std::process::Command::new("curl").arg("--version").output().is_err() {
        return;
    }
    // a server that answers when the test lets it
    let (open, gate) = std::sync::mpsc::channel::<()>();
    let gate = std::sync::Mutex::new(gate);
    let (base, _h) = crate::calendar::dav::tests::mock_http(10, move |_, _, _, _| {
        let _ = gate.lock().unwrap().recv();
        (207, r#"<d:multistatus xmlns:d="DAV:"/>"#.to_string())
    });
    let mut w = W::new("cal-loading");
    w.copy_right(Item::Calendar);
    let src = crate::calendar::store::CalendarSource {
        id: "dav".into(),
        name: "Server".into(),
        color: "#1971c2".into(),
        enabled: true,
        sync_minutes: 0,
        kind: crate::calendar::store::SourceKind::Caldav { url: format!("{base}/a/"), username: "ada".into(), password: "x".into() },
    };
    w.app.cfg.calendars = vec![src];
    w.app.calendar.set_sources(w.app.cfg.calendars.clone());
    w.frames(3);
    assert!(w.app.calendar.is_loading());
    assert!(w.right_has("Loading the calendars"), "{:?}", w.right_text());
    assert!(w.cal_pane_has("calpane:next"), "the month is there meanwhile");
    // the server answers: nothing in the range
    drop(open);
    w.hl.settle(&mut w.app);
    w.frames(3);
    assert!(w.right_has("Nothing in the next"), "{:?}", w.right_text());
}

impl W {
    /// The demo account with two feeds of 40 posts each ("home": h0…h39, "list:1": l0…l39) and a
    /// pretend server that answers a request for a page; no tab shows Feeds yet.
    fn with_feeds(tag: &str) -> (W, std::sync::Arc<crate::social::ui_tests::Script>) {
        use crate::social::state::Timeline;
        let mut w = W::new(tag);
        let entry = crate::social::demo::account();
        w.app.cfg.social_accounts = vec![entry.clone()];
        w.app.social.load_demo(&entry);
        for (f, p) in [("home", "h"), ("list:1", "l")] {
            w.app.social.timelines.insert((entry.account.id.clone(), f.into()), Timeline { posts: crate::social::ui_tests::feed_posts(p, 40), loaded: true, ..Default::default() });
        }
        let script = std::sync::Arc::new(crate::social::ui_tests::Script::default());
        w.app.social.source = Some(script.clone());
        w.frames(3);
        (w, script)
    }

    /// Where the Feeds pane of the right sidebar drew a control (see `SocialPane::rects`).
    fn feed_pane(&self, name: &str) -> Rect {
        let rects = &self.app.side.panes.social.rects;
        rects.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("the Feeds pane drew no {name}: {:?}", rects.iter().map(|(n, _)| n).collect::<Vec<_>>())).1
    }

    fn feed_pane_count(&self, prefix: &str) -> usize {
        self.app.side.panes.social.rects.iter().filter(|(n, _)| n.starts_with(prefix)).count()
    }

    fn feeds_tabs(&self) -> usize {
        self.app.tabbar.tabs.iter().filter(|t| matches!(t.view, View::Social(_))).count()
    }

    /// The feed and conversation the active tab shows.
    fn tab_feed(&self) -> crate::social::FeedRef {
        match &self.app.tabbar.tabs[self.app.tabbar.active].view {
            View::Social(Some(f)) => f.clone(),
            v => panic!("the active tab is no Feeds tab: {v:?}"),
        }
    }
}

fn demo_account_id() -> String {
    crate::social::demo::account().account.id
}

// spec: LAY-035
#[test]
fn lay_035_dragging_feeds_to_the_right_sidebar_asks_and_both_answers_work_and_the_ribbon_icon_still_opens_the_tab() {
    let (mut w, _) = W::with_feeds("feeds-drag");
    let before = w.app.side.layout.clone();
    let at = Pos2::new(w.bar(0).max.x - 4.0, w.bar(0).center().y);
    w.drag_to(w.ribbon_icon(Item::Social).center(), at);
    assert!(matches!(w.app.side.cue, Cue::Bar { .. }), "{:?}", w.app.side.cue);
    w.release(at);
    assert!(w.app.side.ask.is_some());
    assert_eq!(w.app.side.layout, before, "nothing changed before the answer");
    w.answer(|b| b.copied);
    assert_eq!(movable_places(&w, Item::Social), (true, true), "a copy: the ribbon keeps it");
    assert_eq!(w.app.side.layout.views[0].active, Item::Social);
    assert_eq!(w.app.cfg.sidebars.right[0].icons.last().map(String::as_str), Some("social"), "stored under its own id");
    assert!(w.right_has("text of h0"), "{:?}", w.right_text());
    // the ribbon icon still opens the Feeds tab; the pane stays
    w.click_at(w.ribbon_icon(Item::Social).center());
    w.frames(2);
    assert!(matches!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Social(_)));
    assert!(w.right_has("text of h0"));
    // a drag of the copy to the ribbon, which has the view: the copy leaves the right side, nothing is asked
    w.drag_and_drop(w.bar_icon(0, Item::Social).center(), w.ribbon_icon(Item::Files).center());
    assert_eq!(movable_places(&w, Item::Social), (true, false));
    // Move to the right: the ribbon loses it; the left sidebar never shows it as a pane
    w.drag_to_first_bar(w.ribbon_icon(Item::Social).center());
    w.answer(|b| b.moved);
    assert_eq!(movable_places(&w, Item::Social), (false, true));
    assert!(w.app.left_view().is_some(), "the left sidebar shows Files, Search or Tags");
    assert!(w.right_has("text of h0"));
    // it survives a restart
    let w2 = w.restarted("feeds-drag-2");
    assert_eq!(movable_places(&w2, Item::Social), (false, true));
}

// spec: LAY-035
#[test]
fn lay_035_the_icon_menu_moves_feeds_and_back() {
    let (mut w, _) = W::with_feeds("feeds-menu");
    w.right_click_at(w.ribbon_icon(Item::Social).center());
    let (mv, _) = w.app.side.menu.to_other.expect("Feeds has Move and Copy");
    w.click_at(mv.center());
    assert_eq!((movable_places(&w, Item::Social), w.views()[0].last()), ((false, true), Some(&Item::Social)));
    w.right_click_at(w.bar_icon(0, Item::Social).center());
    let (mv, _) = w.app.side.menu.to_other.expect("and back");
    w.click_at(mv.center());
    assert_eq!(movable_places(&w, Item::Social), (true, false));
    assert_eq!(w.app.side.layout.ribbon.last(), Some(&Item::Social));
}

// spec: LAY-034
#[test]
fn lay_034_the_pane_lists_the_home_timeline_as_rows_with_author_age_text_and_pictures() {
    use crate::social::{Media, MediaKind};
    let (mut w, _) = W::with_feeds("feeds-pane");
    let key = (demo_account_id(), "home".to_string());
    let picture = Media { kind: MediaKind::Image, url: String::new(), preview_url: String::new(), alt: String::new(), file_name_hint: String::new(), blob_cid: None };
    {
        let posts = &mut w.app.social.timelines.get_mut(&key).unwrap().posts;
        posts[0].media = vec![picture.clone(), picture];
        posts[1].content_warning = Some("spoilers".into());
        posts[1].text_markdown = "the secret text".into();
        posts[2].reposted_by = Some(crate::social::tests::post("x", "carol", 0, None).author);
        posts[3].text_markdown = "Hello [world](https://x.test/page)".into();
        posts[4].created_at = crate::calendar::time::now() - 3 * 3600;
    }
    w.copy_right(Item::Social);
    let text = w.right_text();
    assert!(w.right_has("Home"), "the feed picker names the feed shown: {text:?}");
    assert!(w.right_has("BOB"), "{text:?}");
    assert!(w.right_has("text of h0") && w.right_has("2 pictures"), "{text:?}");
    assert!(w.right_has("Content warning: spoilers") && !w.right_has("the secret text"), "{text:?}");
    assert!(w.right_has("CAROL"), "who boosted it: {text:?}");
    assert!(w.right_has("Hello world") && !w.right_has("https://x.test"), "{text:?}");
    assert!(w.right_has("3h"), "the age: {text:?}");
    // one account: no row of names; a long list is drawn in part only
    assert_eq!(w.feed_pane_count("feedpane:account:"), 0);
    let rows = w.feed_pane_count("feedpane:row:");
    assert!(rows > 3 && rows < 40, "{rows} rows");
    assert_eq!(w.app.side.panes.social.shown, Some(key));
    assert_eq!(w.feeds_tabs(), 0, "the pane needs no tab");
}

// spec: LAY-034
#[test]
fn lay_034_a_click_on_a_post_opens_its_conversation_in_the_feeds_tab_and_opens_the_tab_when_there_is_none() {
    let (mut w, _) = W::with_feeds("feeds-click");
    w.copy_right(Item::Social);
    assert_eq!(w.feeds_tabs(), 0, "no Feeds tab yet");
    let thread = |id: &str| crate::social::FeedRef { thread: Some(id.into()), ..crate::social::FeedRef::new(demo_account_id(), "home") };
    w.click_at(w.feed_pane("feedpane:row:1").center());
    w.frames(3);
    assert_eq!(w.feeds_tabs(), 1, "the tab was opened");
    assert_eq!(w.tab_feed(), thread("h1"));
    // another post: the same tab, one step further in its history
    w.click_at(w.feed_pane("feedpane:row:3").center());
    w.frames(3);
    assert_eq!(w.feeds_tabs(), 1, "no second tab");
    assert_eq!(w.tab_feed(), thread("h3"));
    assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].back.last(), Some(&View::Social(Some(thread("h1")))));
    // another tab in front: the Feeds tab is brought back, not a new one opened
    let feeds_tab = w.app.tabbar.active;
    w.click_at(w.app.tabbar.tab_rects[0].0.center());
    w.frames(2);
    assert_ne!(w.app.tabbar.active, feeds_tab, "a note is in front");
    w.click_at(w.feed_pane("feedpane:row:5").center());
    w.frames(3);
    assert_eq!((w.feeds_tabs(), w.app.tabbar.active), (1, feeds_tab));
    assert_eq!(w.tab_feed(), thread("h5"));
}

// spec: LAY-031
#[test]
fn lay_031_the_feeds_pane_and_the_tab_choose_their_account_and_feed_separately() {
    use crate::social::state::{FeedsState, Timeline};
    use crate::social::{Account, Feed, FeedRef, Network, SocialAccountEntry};
    let (mut w, _) = W::with_feeds("feeds-independent");
    let (a1, a2) = (demo_account_id(), "mastodon:bob@other.social".to_string());
    let other = SocialAccountEntry { account: Account::new(Network::Mastodon, "https://other.social", "bob@other.social", "Bob"), secret: Default::default() };
    assert_eq!(other.account.id, a2);
    w.app.cfg.social_accounts.push(other);
    w.app.social.feeds.insert(a2.clone(), FeedsState::Ready(vec![Feed { id: "home".into(), title: "Home".into() }]));
    let posts: Vec<_> = crate::social::ui_tests::feed_posts("o", 5).into_iter().map(|mut p| (p.text_markdown = format!("other {}", p.id), p).1).collect();
    w.app.social.timelines.insert((a2.clone(), "home".into()), Timeline { posts, loaded: true, ..Default::default() });
    w.copy_right(Item::Social);
    w.click_at(w.ribbon_icon(Item::Social).center());
    w.frames(3);
    assert_eq!(w.tab_feed(), FeedRef::new(&a1, "home"));
    assert_eq!(w.app.side.panes.social.shown, Some((a1.clone(), "home".into())));
    // the pane picks another feed of the account, the tab stays
    w.click_at(w.feed_pane("feedpane:feed").center());
    w.frames(2);
    w.click_at(w.feed_pane("feedpane:feed:list:1").center());
    w.frames(2);
    assert_eq!(w.app.side.panes.social.shown, Some((a1.clone(), "list:1".into())));
    assert!(w.right_has("text of l0") && !w.right_has("text of h0"), "{:?}", w.right_text());
    assert_eq!(w.tab_feed(), FeedRef::new(&a1, "home"));
    // the pane picks the other account: it starts at that account's first feed, the tab stays
    w.click_at(w.feed_pane(&format!("feedpane:account:{a2}")).center());
    w.frames(2);
    assert_eq!(w.app.side.panes.social.shown, Some((a2.clone(), "home".into())));
    assert!(w.right_has("other o0"), "{:?}", w.right_text());
    assert_eq!(w.tab_feed(), FeedRef::new(&a1, "home"));
    // the tab goes to a list, the pane stays
    w.app.open_feed(FeedRef::new(&a1, "list:1"), false);
    w.frames(3);
    assert_eq!(w.tab_feed(), FeedRef::new(&a1, "list:1"));
    assert_eq!(w.app.side.panes.social.shown, Some((a2, "home".into())));
}

// spec: LAY-031
#[test]
fn lay_031_the_feeds_pane_scrolls_on_its_own_and_shows_the_posts_of_the_tab() {
    let (mut w, _) = W::with_feeds("feeds-scroll");
    w.copy_right(Item::Social);
    w.hl.set_size(Vec2::new(1500.0, 420.0));
    w.click_at(w.ribbon_icon(Item::Social).center());
    w.frames(4);
    assert_eq!(w.app.side.panes.social.scroll, 0.0);
    let tab_top = w.app.social.post_rects[0].1.min.y;
    // the pane scrolls, the tab does not
    let at = w.feed_pane("feedpane:row:1").center();
    w.wheel(at, -300.0);
    assert!(w.app.side.panes.social.scroll > 100.0, "the pane scrolled: {}", w.app.side.panes.social.scroll);
    assert!((w.app.social.post_rects[0].1.min.y - tab_top).abs() < 1.0, "the tab stayed");
    // the tab scrolls, the pane does not
    let pane_scroll = w.app.side.panes.social.scroll;
    w.wheel(Pos2::new(500.0, 250.0), -400.0);
    assert!(w.app.social.post_rects[0].1.min.y < tab_top - 100.0, "the tab scrolled");
    assert_eq!(w.app.side.panes.social.scroll, pane_scroll);
    // a post that comes into the shared list shows in the pane
    let key = (demo_account_id(), "home".to_string());
    let mut fresh = crate::social::tests::post("fresh", "dave", crate::calendar::time::now(), None);
    fresh.text_markdown = "brand new post".into();
    w.app.social.timelines.get_mut(&key).unwrap().posts.insert(0, fresh);
    w.wheel(at, 5000.0);
    assert_eq!(w.app.side.panes.social.scroll, 0.0);
    assert!(w.right_has("brand new post"), "{:?}", w.right_text());
}

// spec: LAY-037
#[test]
fn lay_037_the_feeds_pane_says_why_it_is_empty() {
    use crate::social::state::{FeedsState, Timeline};
    // no account
    let mut w = W::new("feeds-none");
    w.copy_right(Item::Social);
    assert!(w.right_has("No feed account yet"), "{:?}", w.right_text());
    assert!(w.app.side.panes.social.rects.is_empty());
    // an account whose feeds are not known yet, or could not be loaded
    let (mut w, _) = W::with_feeds("feeds-empty");
    let id = demo_account_id();
    w.copy_right(Item::Social);
    w.app.social.feeds.insert(id.clone(), FeedsState::Loading);
    w.frames(2);
    assert!(w.right_has("Loading the feeds"), "{:?}", w.right_text());
    w.app.social.feeds.insert(id.clone(), FeedsState::Failed("no route".into()));
    w.frames(2);
    assert!(w.right_has("Could not load the feeds: no route"), "{:?}", w.right_text());
    w.app.social.feeds.insert(id.clone(), FeedsState::Ready(crate::social::demo::feeds()));
    // a feed that is on its way, one that failed, one that is empty, one that has older posts
    let key = (id.clone(), "home".to_string());
    for (timeline, says) in [
        (Timeline { loading: true, ..Default::default() }, "Loading the feed"),
        (Timeline { error: Some("boom".into()), ..Default::default() }, "Could not load posts: boom"),
        (Timeline { loaded: true, ..Default::default() }, "Nothing here yet."),
        (Timeline { loaded: true, posts: crate::social::ui_tests::feed_posts("h", 2), cursor: Some("next".into()), ..Default::default() }, "Older posts are in the Feeds tab."),
    ] {
        w.app.social.timelines.insert(key.clone(), timeline);
        w.frames(2);
        assert!(w.right_has(says), "{says}: {:?}", w.right_text());
    }
    // the notifications feed has its own words
    w.app.social.timelines.insert((id, crate::social::notifications::NOTIFICATIONS.into()), Timeline { loaded: true, ..Default::default() });
    w.app.side.panes.social.feed = Some(crate::social::notifications::NOTIFICATIONS.into());
    w.frames(2);
    assert!(w.right_has("No notifications yet."), "{:?}", w.right_text());
}

// spec: LAY-034, LAY-037
#[test]
fn lay_034_the_pane_asks_for_the_first_page_once_also_while_the_tab_opens_and_without_any_tab() {
    let (mut w, script) = W::with_feeds("feeds-first-page");
    let key = (demo_account_id(), "home".to_string());
    w.app.social.timelines.remove(&key);
    let (release, gate) = std::sync::mpsc::channel::<()>();
    *script.page_gate.lock().unwrap() = Some(gate);
    *script.first_page.lock().unwrap() = Some(crate::social::Page { posts: crate::social::ui_tests::feed_posts("n", 3), cursor: None, top: None });
    w.copy_right(Item::Social);
    // no tab shows Feeds: the pane starts the first page, which is on its way
    assert_eq!(w.feeds_tabs(), 0);
    assert!(w.app.social.timelines[&key].loading);
    assert!(w.right_has("Loading the feed"), "{:?}", w.right_text());
    // the tab opens meanwhile and does not ask again
    w.click_at(w.ribbon_icon(Item::Social).center());
    w.frames(3);
    assert_eq!(w.feeds_tabs(), 1);
    // the server answers: the pane and the tab show the posts, and the server was asked once
    drop(release);
    w.hl.settle(&mut w.app);
    w.frames(2);
    assert!(w.right_has("text of n0"), "{:?}", w.right_text());
    assert_eq!(*script.page_calls.lock().unwrap(), ["home"]);
}
