//! The player card (MEDIA-001, MEDIA-003…006): artwork with a round play/pause button over it, the
//! file name, a progress bar with elapsed and total time, a mute toggle and a volume slider. The
//! card paints itself into a rectangle it is given, so the reading view (which lays it out), the
//! editor and the preview (which paint it over the link text) all use the same code. It holds no
//! state of its own: everything comes from and goes to the `MediaPlayer`, and the artwork is a
//! texture, so a video can use the same card with its picture.

use super::player::{button_alpha, clock, MediaPlayer, View};
use crate::theme;
use eframe::egui::{self, Color32, CornerRadius, Id, Key, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};
use std::path::Path;

pub const CARD_HEIGHT: f32 = 112.0;
const CARD_MAX_WIDTH: f32 = 520.0;
const ART: f32 = 96.0;
const PAD: f32 = 8.0;
/// Radius of the round play/pause button.
pub const BUTTON_RADIUS: f32 = 22.0;

/// The size of a card in a place `avail_w` wide.
pub fn card_size(avail_w: f32) -> Vec2 {
    Vec2::new(avail_w.clamp(260.0, CARD_MAX_WIDTH), CARD_HEIGHT)
}

/// Lay out a card in the flow of `ui` and draw it.
pub fn show(ui: &mut Ui, media: &MediaPlayer, abs: &Path, owner: &Path) -> Response {
    let id = ui.auto_id_with(("media-card", abs));
    let (rect, resp) = ui.allocate_exact_size(card_size(ui.available_width()), Sense::hover());
    paint(ui, rect, media, abs, owner, id);
    resp
}

/// Draw the card for `abs` into `rect` and handle what it is given: `id` must be unique among the
/// cards of a frame; `owner` is the note that shows it (closing its tab stops the sound).
pub fn paint(ui: &Ui, rect: Rect, media: &MediaPlayer, abs: &Path, owner: &Path, id: Id) {
    let ctx = ui.ctx().clone();
    let v = media.view(&ctx, abs);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect(rect, CornerRadius::same(8), theme::BG_SECONDARY, Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
    let card = ui.interact(rect, id, Sense::click());

    // --- artwork and the round button
    let art = Rect::from_min_size(rect.min + Vec2::splat(PAD), Vec2::splat(ART));
    paint_art(&painter, art, &v);
    let button = ui.interact(Rect::from_center_size(art.center(), Vec2::splat(BUTTON_RADIUS * 2.0)), id.with("play"), Sense::click());
    let mut alpha = button_alpha(v.opacity, button.hovered());
    if v.unplayable {
        alpha /= 2;
    }
    painter.circle_filled(art.center(), BUTTON_RADIUS, Color32::from_black_alpha(alpha));
    paint_glyph(&painter, art.center(), v.playing, Color32::from_white_alpha(alpha));
    let label = if v.playing { "Pause" } else { "Play" };
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !v.unplayable, label));

    // --- the right side: name, message, progress, volume
    let x0 = art.right() + 12.0;
    let x1 = rect.right() - 12.0;
    let width = (x1 - x0).max(40.0);
    let mut name = egui::text::LayoutJob::simple(v.file_name.clone(), egui::FontId::proportional(14.0), theme::TEXT_STRONG, width);
    name.wrap.max_rows = 1;
    name.wrap.break_anywhere = true;
    painter.galley(Pos2::new(x0, rect.top() + 10.0), painter.layout_job(name), theme::TEXT_STRONG);
    if let Some(m) = &v.message {
        let colour = if v.unplayable { theme::LINK_MISSING } else { theme::MUTED };
        let mut job = egui::text::LayoutJob::simple(m.clone(), egui::FontId::proportional(12.0), colour, width);
        job.wrap.max_rows = 1;
        painter.galley(Pos2::new(x0, rect.top() + 29.0), painter.layout_job(job), colour);
    }

    // progress bar
    let bar_hit = Rect::from_min_size(Pos2::new(x0, rect.top() + 48.0), Vec2::new(width, 16.0));
    let bar = ui.interact(bar_hit, id.with("seek"), Sense::click_and_drag());
    bar.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, v.duration.is_some(), "Position"));
    let preview_id = id.with("seek-preview");
    let frac_at = |p: Pos2| ((p.x - bar_hit.left()) / bar_hit.width()).clamp(0.0, 1.0) as f64;
    let mut preview: Option<f64> = ctx.data(|d| d.get_temp(preview_id));
    if let Some(d) = v.duration.filter(|_| !v.unplayable) {
        if bar.dragged() {
            if let Some(p) = bar.interact_pointer_pos() {
                preview = Some(frac_at(p));
                ctx.data_mut(|data| data.insert_temp(preview_id, frac_at(p)));
            }
        }
        if bar.drag_stopped() {
            // the seek happens once, when the mouse is let go (MEDIA-005)
            if let Some(f) = preview.take() {
                media.seek_to(&ctx, abs, owner, f * d);
            }
            ctx.data_mut(|data| data.remove_temp::<f64>(preview_id));
        } else if bar.clicked() {
            if let Some(p) = bar.interact_pointer_pos() {
                media.seek_to(&ctx, abs, owner, frac_at(p) * d);
            }
        }
    }
    let shown = preview.zip(v.duration).map(|(f, d)| f * d).unwrap_or(v.position);
    let frac = v.duration.map(|d| (shown / d).clamp(0.0, 1.0) as f32).unwrap_or(0.0);
    let track = Rect::from_center_size(bar_hit.center(), Vec2::new(width, 4.0));
    painter.rect_filled(track, CornerRadius::same(2), theme::FAINT.gamma_multiply(0.6));
    let mut filled = track;
    filled.set_right(track.left() + track.width() * frac);
    painter.rect_filled(filled, CornerRadius::same(2), theme::ACCENT);
    if v.duration.is_some() && (bar.hovered() || preview.is_some()) {
        painter.circle_filled(Pos2::new(filled.right(), track.center().y), 5.0, theme::TEXT_STRONG);
    }
    let times = egui::FontId::proportional(11.0);
    painter.text(Pos2::new(x0, rect.top() + 66.0), egui::Align2::LEFT_TOP, clock(shown), times.clone(), theme::MUTED);
    let total = v.duration.map(clock).unwrap_or_else(|| "–:––".into());
    painter.text(Pos2::new(x1, rect.top() + 66.0), egui::Align2::RIGHT_TOP, total, times.clone(), theme::MUTED);

    // volume: mute toggle and slider
    let mute_rect = Rect::from_min_size(Pos2::new(x0, rect.top() + 82.0), Vec2::splat(20.0));
    let mute = ui.interact(mute_rect, id.with("mute"), Sense::click());
    let mute_label = if v.muted { "Unmute" } else { "Mute" };
    mute.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, mute_label));
    if mute.clicked() {
        media.set_muted(!v.muted);
    }
    let icon_colour = if mute.hovered() { theme::TEXT_STRONG } else { theme::MUTED };
    paint_speaker(&painter, mute_rect.shrink(1.0), v.muted || v.volume <= 0.0, icon_colour);
    let vol_hit = Rect::from_min_size(Pos2::new(mute_rect.right() + 6.0, rect.top() + 82.0), Vec2::new((width * 0.4).clamp(60.0, 120.0), 20.0));
    let vol = ui.interact(vol_hit, id.with("volume"), Sense::click_and_drag());
    vol.widget_info(|| egui::WidgetInfo::slider(true, v.volume as f64, "Volume"));
    if vol.dragged() || vol.clicked() {
        if let Some(p) = vol.interact_pointer_pos() {
            media.set_volume(((p.x - vol_hit.left()) / vol_hit.width()).clamp(0.0, 1.0));
        }
    }
    let vtrack = Rect::from_center_size(vol_hit.center(), Vec2::new(vol_hit.width(), 4.0));
    painter.rect_filled(vtrack, CornerRadius::same(2), theme::FAINT.gamma_multiply(0.6));
    let mut vfill = vtrack;
    vfill.set_right(vtrack.left() + vtrack.width() * if v.muted { 0.0 } else { v.volume });
    painter.rect_filled(vfill, CornerRadius::same(2), theme::MUTED);
    painter.text(Pos2::new(vol_hit.right() + 8.0, vol_hit.center().y), egui::Align2::LEFT_CENTER, format!("{} %", (if v.muted { 0.0 } else { v.volume } * 100.0).round() as u32), times, theme::MUTED);

    // --- toggling: the button, or Space while the card has the keyboard (MEDIA-004)
    // a click in the card gives it the keyboard
    if card.clicked() || button.clicked() || bar.clicked() || vol.clicked() {
        card.request_focus();
    }
    let focused = card.has_focus() || bar.has_focus() || vol.has_focus();
    let space = focused && ui.input(|i| i.key_pressed(Key::Space));
    if space {
        ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Space));
    }
    if (button.clicked() || space) && !v.unplayable {
        media.toggle(&ctx, abs, owner);
    }
    if button.hovered() {
        ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
    }
}

fn paint_art(painter: &Painter, art: Rect, v: &View) {
    painter.rect_filled(art, CornerRadius::same(6), if v.cover.is_some() { Color32::from_rgb(20, 20, 20) } else { theme::BG_HOVER });
    match &v.cover {
        Some(tex) => {
            let [w, h] = tex.size_vec2().into();
            let k = (ART / w).min(ART / h);
            let r = Rect::from_center_size(art.center(), Vec2::new(w * k, h * k));
            painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            // no cover: a neutral tile with the file name
            let mut job = egui::text::LayoutJob::simple(v.file_name.clone(), egui::FontId::proportional(11.0), theme::FAINT, ART - 12.0);
            job.wrap.max_rows = 4;
            job.wrap.break_anywhere = true;
            job.halign = egui::Align::Center;
            let galley = painter.layout_job(job);
            let pos = Pos2::new(art.center().x - galley.size().x / 2.0, art.bottom() - galley.size().y - 8.0);
            painter.galley(pos, galley, theme::FAINT);
        }
    }
}

/// The triangle or the two bars in the middle of the round button.
fn paint_glyph(painter: &Painter, c: Pos2, playing: bool, colour: Color32) {
    if playing {
        for dx in [-5.5, 1.5] {
            painter.rect_filled(Rect::from_min_size(c + Vec2::new(dx, -7.5), Vec2::new(4.0, 15.0)), CornerRadius::same(1), colour);
        }
    } else {
        let tri = vec![c + Vec2::new(-5.0, -9.0), c + Vec2::new(-5.0, 9.0), c + Vec2::new(9.0, 0.0)];
        painter.add(Shape::convex_polygon(tri, colour, Stroke::NONE));
    }
}

fn paint_speaker(painter: &Painter, r: Rect, muted: bool, colour: Color32) {
    let at = |x: f32, y: f32| r.min + Vec2::new(r.width() * x, r.height() * y);
    painter.add(Shape::convex_polygon(vec![at(0.05, 0.38), at(0.3, 0.38), at(0.3, 0.62), at(0.05, 0.62)], colour, Stroke::NONE));
    painter.add(Shape::convex_polygon(vec![at(0.3, 0.38), at(0.52, 0.18), at(0.52, 0.82), at(0.3, 0.62)], colour, Stroke::NONE));
    let stroke = Stroke::new(1.3, colour);
    if muted {
        painter.line_segment([at(0.65, 0.35), at(0.95, 0.65)], stroke);
        painter.line_segment([at(0.65, 0.65), at(0.95, 0.35)], stroke);
    } else {
        painter.add(Shape::line(vec![at(0.64, 0.36), at(0.7, 0.5), at(0.64, 0.64)], stroke));
        painter.add(Shape::line(vec![at(0.75, 0.24), at(0.86, 0.5), at(0.75, 0.76)], stroke));
    }
}
