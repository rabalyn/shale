//! Anchors and the pulse (CFG-073): the drawing code of a page names the rect of a setting with
//! `anchor(ui, "<id>", |ui| …)` (or `mark` for a rect it has already), and the id is a row of
//! the registry (`registry.rs`). A search result that is picked starts a jump to an id (`start`):
//! the page scrolls the anchor into view and a box around it pulses for a moment, then fades.
//! The state is small and lives in the egui context, so an anchor deep in a page needs nothing
//! passed to it.

use super::registry;
use crate::theme;
use eframe::egui::{self, Align, Context, Id, Rect, Stroke, Ui};

/// How long the box stays, in seconds.
pub const DURATION: f64 = 1.8;
/// The time of one beat of the pulse.
const BEAT: f64 = 0.6;
/// How many frames the page is scrolled to the anchor: the first frame of a page may not have
/// laid out everything above the anchor yet.
const SCROLL_FRAMES: u8 = 2;

/// The jump in progress.
#[derive(Clone, Default)]
struct Jump {
    /// The anchor to pulse; empty: none.
    id: String,
    started: f64,
    scroll_frames: u8,
    /// The row's fallback has been taken (it has none of its own).
    retargeted: bool,
}

/// The anchors drawn in this frame, in order.
#[derive(Clone, Default)]
struct Seen(Vec<(String, Rect)>);

fn jump_id() -> Id {
    Id::unique("settings-anchor-jump")
}

fn seen_id() -> Id {
    Id::unique("settings-anchors-seen")
}

fn now(ctx: &Context) -> f64 {
    ctx.input(|i| i.time)
}

/// Whether the person asked for no animation (`animation_time` 0, which the screenshots set too):
/// the box then stays as it is and ends, with no pulse and no fade.
fn steady(ctx: &Context) -> bool {
    ctx.global_style().animation_time == 0.0
}

/// How strong the box is, 0 to 1, `elapsed` seconds after the jump: it beats and fades; steady
/// without animation until it ends.
pub fn intensity(elapsed: f64, steady: bool) -> f32 {
    if !(0.0..DURATION).contains(&elapsed) {
        return 0.0;
    }
    if steady {
        return 1.0;
    }
    let fade = (1.0 - elapsed / DURATION).sqrt();
    let beat = 0.5 + 0.5 * (std::f64::consts::TAU * elapsed / BEAT).cos();
    (fade * (0.35 + 0.65 * beat)) as f32
}

/// Start the jump to the anchor `id`: scroll to it and pulse a box around it.
pub fn start(ctx: &Context, id: &str) {
    let jump = Jump { id: id.to_string(), started: now(ctx), scroll_frames: SCROLL_FRAMES, retargeted: false };
    ctx.data_mut(|d| d.insert_temp(jump_id(), jump));
    ctx.request_repaint();
}

/// The anchor that pulses and how strong the box is now; `None` when there is none.
#[cfg(test)]
pub fn active(ctx: &Context) -> Option<(String, f32)> {
    let jump: Jump = ctx.data(|d| d.get_temp(jump_id()))?;
    let k = intensity(now(ctx) - jump.started, steady(ctx));
    (!jump.id.is_empty() && k > 0.0).then_some((jump.id, k))
}

/// The start of a frame of the dialog: no anchor is drawn yet.
pub fn begin(ctx: &Context) {
    ctx.data_mut(|d| d.insert_temp(seen_id(), Seen::default()));
}

/// The end of a frame of the dialog: a jump to an anchor that the page did not draw (a font of one
/// area while one font is used for all) goes to the row's fallback; a jump that has run out ends.
pub fn end(ctx: &Context) {
    let Some(mut jump) = ctx.data(|d| d.get_temp::<Jump>(jump_id())) else { return };
    if jump.id.is_empty() {
        return;
    }
    if now(ctx) - jump.started >= DURATION {
        ctx.data_mut(|d| d.remove::<Jump>(jump_id()));
        return;
    }
    let seen: Seen = ctx.data(|d| d.get_temp(seen_id())).unwrap_or_default();
    if !jump.retargeted && !seen.0.iter().any(|(id, _)| *id == jump.id) {
        // (the page of the row's tab was drawn in this frame, or the jump would have no tab)
        if let Some(fallback) = registry::entry(&jump.id).and_then(|e| e.fallback) {
            jump.id = fallback.to_string();
            jump.retargeted = true;
            jump.scroll_frames = SCROLL_FRAMES;
            ctx.data_mut(|d| d.insert_temp(jump_id(), jump));
        }
    }
    ctx.request_repaint();
}

/// The anchors drawn since `begin`, for the tests.
#[cfg(test)]
pub fn seen(ctx: &Context) -> Vec<(String, Rect)> {
    ctx.data(|d| d.get_temp::<Seen>(seen_id())).unwrap_or_default().0
}

/// An anchor id chosen at run time, from a set of literals (`match area { … => anchor_id("fonts.ui") }`):
/// the registry test finds the literal by this call, as it finds those of `anchor` and `mark`.
pub const fn anchor_id(id: &'static str) -> &'static str {
    id
}

/// Draw `add` and make its rect the anchor `id`.
pub fn anchor<R>(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let out = ui.scope(add);
    mark(ui, id, out.response.rect);
    out.inner
}

/// Make `rect` the anchor `id`: it is recorded, and while a jump is to it, scrolled into view and
/// boxed. An empty rect is no anchor.
pub fn mark(ui: &Ui, id: &str, rect: Rect) {
    // nothing was drawn (a part of a page that has nothing to show now): there is nothing to box
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let ctx = ui.ctx();
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Seen>(seen_id()).0.push((id.to_string(), rect)));
    let Some(mut jump) = ctx.data(|d| d.get_temp::<Jump>(jump_id())).filter(|j| j.id == id) else { return };
    if jump.scroll_frames > 0 {
        ui.scroll_to_rect(rect.expand(8.0), Some(Align::Center));
        jump.scroll_frames -= 1;
        ctx.data_mut(|d| d.insert_temp(jump_id(), jump.clone()));
    }
    let k = intensity(now(ctx) - jump.started, steady(ctx));
    if k > 0.0 {
        let accent = theme::palette().accent;
        let around = rect.expand(3.0);
        // the box may reach a little beyond the page's own clip rectangle
        let painter = ui.painter().with_clip_rect(ui.clip_rect().expand(6.0));
        painter.rect_filled(around, 4.0, accent.gamma_multiply(0.16 * k));
        painter.rect_stroke(around, 4.0, Stroke::new(2.0, accent.gamma_multiply(k)), egui::StrokeKind::Outside);
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: CFG-073
    #[test]
    fn cfg_073_the_box_starts_strong_beats_and_has_faded_when_the_time_is_up() {
        assert_eq!(intensity(0.0, false), 1.0);
        let peak = |from: f64, to: f64| (0..100).map(|i| intensity(from + (to - from) * i as f64 / 100.0, false)).fold(0.0, f32::max);
        assert!(peak(0.0, 0.6) > peak(1.2, 1.8), "it fades");
        assert!(intensity(0.3, false) < intensity(0.0, false) * 0.6, "it beats: the middle of a beat is dim");
        assert_eq!(intensity(DURATION, false), 0.0);
        assert_eq!(intensity(DURATION + 5.0, false), 0.0);
        assert_eq!(intensity(-1.0, false), 0.0, "not before the start");
    }

    // spec: CFG-073
    #[test]
    fn cfg_073_without_animation_the_box_stands_still_until_the_time_is_up() {
        assert!([0.0, 0.3, 1.0, 1.7].iter().all(|t| intensity(*t, true) == 1.0));
        assert_eq!(intensity(DURATION, true), 0.0);
    }
}
