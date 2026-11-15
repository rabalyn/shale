//! The menubars inside the whole window (LAY-001…): the window runs headless
//! (`ShaleApp::run_frame`) and the mouse is real pointer events at the places the window reports
//! in `ShaleApp::side`.

use crate::app::{Options, RightPanel, ShaleApp};
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
        let dir = std::env::temp_dir().join(format!("shale-sideui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Welcome.md"), "# Welcome\n\n## First\n\n[[Other]]\n\n## Second\n").unwrap();
        std::fs::write(dir.join("Other.md"), "# Other\n\nSee [[Welcome]].\n").unwrap();
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
    assert!(w.app.left == crate::app::LeftPanel::Tags);
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

// spec: LAY-001
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
    assert_eq!(w.views(), vec![vec![panel(RightPanel::Backlinks), panel(RightPanel::Outgoing), panel(RightPanel::LocalGraph), panel(RightPanel::Calendar)], vec![panel(RightPanel::Outline)]]);
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
    assert_eq!(w.views()[1], [panel(RightPanel::Outgoing), panel(RightPanel::Outline), panel(RightPanel::LocalGraph), panel(RightPanel::Calendar)]);
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
    let mail = w.ribbon_icon(Item::Mail).center();
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
    for p in [RightPanel::Backlinks, RightPanel::Outgoing, RightPanel::LocalGraph, RightPanel::Calendar] {
        w.drag_and_drop(w.bar_icon(0, panel(p)).center(), below);
    }
    assert!(w.views().is_empty());
    assert!(w.right_has("Drag a panel icon here"), "{:?}", w.right_text());
    // a drop on the empty sidebar makes the first view; a ribbon icon is refused there
    let area = w.app.side.right_rect.unwrap().center();
    w.drag_and_drop(w.ribbon_icon(Item::Mail).center(), area);
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
    for broken in [r#"{"sidebars": {"right": "nonsense"}, "font_size": 17.0}"#, r#"{"sidebars": 5}"#, r#"{"sidebars": {"ribbon": ["x"], "right": [{"icons": ["mail", "nope"], "active": 3}]}}"#] {
        let (cfg, _) = crate::vault_state::parse::<Config>(broken.as_bytes()).unwrap();
        assert_eq!(Layout::from_config(&cfg), Layout::default(), "{broken}");
    }
}

// spec: LAY-011
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
    w.app.side.layout.hide(panel(RightPanel::Calendar));
    w.frames(3);
    w.right_click_at(w.ribbon_icon(Item::Files).center());
    w.click_at(w.app.side.menu.reset.unwrap().center());
    assert_eq!(w.app.side.layout, Layout::default());
    assert_eq!(w.app.cfg.sidebars, crate::config::SidebarCfg { ribbon: w.app.cfg.sidebars.ribbon.clone(), right: w.app.cfg.sidebars.right.clone(), hidden: vec![] });
    assert_eq!(Layout::from_config(&w.app.cfg), Layout::default(), "kept");
}
