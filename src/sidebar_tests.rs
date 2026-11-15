//! The menubars inside the whole window (LAY-001…): the window runs headless
//! (`ShaleApp::run_frame`) and the mouse is real pointer events at the places the window reports
//! in `ShaleApp::side`.

use crate::app::{Options, ShaleApp};
use crate::config::Config;
use crate::sidebar::{Item, Layout};
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::PathBuf;

pub(crate) struct W {
    pub app: ShaleApp,
    ctx: egui::Context,
    t: f64,
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
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let mut w = W { app, ctx, t: 0.0, dir };
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
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 900.0))), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
    }

    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(vec![]);
        }
    }

    fn button(p: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
    }

    /// Press at `from`, move to `to` in two steps, and stay there with the button down.
    pub fn drag_to(&mut self, from: Pos2, to: Pos2) {
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![Self::button(from, true)]);
        self.frame(vec![Event::PointerMoved(from + Vec2::new(0.0, 12.0))]);
        self.frame(vec![Event::PointerMoved(to)]);
        self.frames(2);
    }

    pub fn release(&mut self, at: Pos2) {
        self.frame(vec![Self::button(at, false)]);
        self.frames(3);
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
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![Self::button(p, true)]);
        self.frame(vec![Self::button(p, false)]);
        self.frames(3);
    }

    pub fn ribbon_icon(&self, item: Item) -> Rect {
        self.app.side.ribbon_icons.iter().find(|(i, _)| *i == item).unwrap_or_else(|| panic!("no ribbon icon {item:?}")).1
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
    assert_eq!(w.ribbon(), Item::ALL.to_vec(), "the default order is drawn");
    // Mail is dragged up between Files and Search; the marker shows the place while the button is down
    let mail = w.ribbon_icon(Item::Mail).center();
    let between = (w.ribbon_icon(Item::Files).max.y + w.ribbon_icon(Item::Search).min.y) / 2.0;
    w.drag_to(mail, Pos2::new(mail.x, between));
    assert_eq!(w.app.side.drag, Some(Item::Mail));
    match w.app.side.cue {
        crate::sidebar_ui::Cue::Ribbon { at, line } => assert_eq!((at, (line[0].y - between).abs() < 3.0), (1, true), "the marker is between Files and Search"),
        c => panic!("no marker: {c:?}"),
    }
    assert_eq!(w.app.side.layout.ribbon, Item::ALL.to_vec(), "nothing moves before the button is let go");
    w.release(Pos2::new(mail.x, between));
    let mut want = Item::ALL.to_vec();
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
    assert_eq!(w.ribbon(), Item::ALL.to_vec(), "dropped on the note: nothing happens");
    let between = (w.ribbon_icon(Item::Files).max.y + w.ribbon_icon(Item::Search).min.y) / 2.0;
    w.drag_to(mail, Pos2::new(mail.x, between));
    w.press_esc();
    assert_eq!(w.app.side.drag, None, "Esc ends the drag");
    w.release(Pos2::new(mail.x, between));
    assert_eq!(w.ribbon(), Item::ALL.to_vec(), "and the button let go afterwards drops nothing");
}
