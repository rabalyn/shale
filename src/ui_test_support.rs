//! What the headless whole-window tests share (`tab_order_tests`, `sidebar_tests`): a clock and an
//! egui context that run `ShaleApp::run_frame` frame by frame, and real pointer events for
//! clicks and drags.

use crate::app::ShaleApp;
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

pub(crate) struct Headless {
    ctx: egui::Context,
    t: f64,
    /// the text painted in the last frame, and where
    pub text: Vec<(String, Pos2)>,
    /// the mouse cursor the last frame asked for
    pub cursor: egui::CursorIcon,
}

fn collect_text(shape: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect_text(s, out)),
        _ => {}
    }
}

impl Headless {
    pub fn new() -> Headless {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        Headless { ctx, t: 0.0, text: vec![], cursor: egui::CursorIcon::Default }
    }

    /// The egui context the frames run in (to set a drag-and-drop payload the way a view does).
    /// Only the drop tests use it, and they run on Linux only (#123).
    #[cfg(target_os = "linux")]
    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Run one frame of a 1500 x 900 window with these events.
    pub fn frame(&mut self, app: &mut ShaleApp, events: Vec<Event>) -> egui::FullOutput {
        self.frame_with(app, events, |_, _| {})
    }

    /// Like `frame`, with a last look at the input of the frame (files dropped through the
    /// window system, a scripted step) before it runs.
    pub fn frame_with(&mut self, app: &mut ShaleApp, events: Vec<Event>, adjust: impl FnOnce(&mut ShaleApp, &mut RawInput)) -> egui::FullOutput {
        self.t += 0.05;
        let mut raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 900.0))), time: Some(self.t), events, ..Default::default() };
        adjust(app, &mut raw);
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
        self.cursor = out.platform_output.cursor_icon;
        self.text.clear();
        out.shapes.iter().for_each(|c| collect_text(&c.shape, &mut self.text));
        out
    }

    pub fn frames(&mut self, app: &mut ShaleApp, n: usize) {
        for _ in 0..n {
            self.frame(app, vec![]);
        }
    }

    pub fn button(p: Pos2, b: PointerButton, pressed: bool) -> Event {
        Event::PointerButton { pos: p, button: b, pressed, modifiers: Modifiers::NONE }
    }

    /// Move to `p`, press and release `b` there, and let the window settle.
    pub fn click(&mut self, app: &mut ShaleApp, p: Pos2, b: PointerButton) {
        self.frame(app, vec![Event::PointerMoved(p)]);
        self.frame(app, vec![Self::button(p, b, true)]);
        self.frame(app, vec![Self::button(p, b, false)]);
        self.frames(app, 3);
    }

    /// Press the primary button at `from`, move through `path` and stay at its end with the
    /// button down.
    pub fn press_and_move(&mut self, app: &mut ShaleApp, from: Pos2, path: &[Pos2]) {
        self.frame(app, vec![Event::PointerMoved(from)]);
        self.frame(app, vec![Self::button(from, PointerButton::Primary, true)]);
        for p in path {
            self.frame(app, vec![Event::PointerMoved(*p)]);
        }
        self.frames(app, 2);
    }

    /// `n` points on the way from `from` to `to`, the last one `to`.
    pub fn steps(from: Pos2, to: Pos2, n: u32) -> Vec<Pos2> {
        (1..=n).map(|k| from + (to - from) * (k as f32 / n as f32)).collect()
    }

    /// Let go of the primary button at `at`.
    pub fn release(&mut self, app: &mut ShaleApp, at: Pos2) {
        self.frame(app, vec![Self::button(at, PointerButton::Primary, false)]);
        self.frames(app, 3);
    }
}
