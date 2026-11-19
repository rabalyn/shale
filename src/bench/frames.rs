//! Window frames without a window (BUILD-029): a `ShaleApp` runs `run_frame` in an egui context
//! that is fed events by hand, and the shapes of the frame are tessellated as the real window
//! does before it paints. Only the GPU upload and the drawing are left out.

use crate::app::ShaleApp;
use eframe::egui::{self, Event, Pos2, RawInput, Rect, Vec2};

/// The size of the window of every UI scenario, in points.
pub const WINDOW: Vec2 = Vec2::new(1500.0, 900.0);

pub struct Frames {
    ctx: egui::Context,
    t: f64,
}

impl Frames {
    pub fn new() -> Frames {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        Frames { ctx, t: 0.0 }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// One frame with these events; returns the number of tessellated shapes (so the work is not optimised away).
    pub fn frame(&mut self, app: &mut ShaleApp, events: Vec<Event>) -> usize {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)), time: Some(self.t), events, ..Default::default() };
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
        self.ctx.tessellate(out.shapes, out.pixels_per_point).len()
    }

    /// `n` frames without events, to let the window settle.
    pub fn settle(&mut self, app: &mut ShaleApp, n: usize) {
        for _ in 0..n {
            self.frame(app, vec![]);
        }
    }
}
