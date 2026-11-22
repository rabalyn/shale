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

/// A video card is wider and has its picture (16:9) above the controls (MEDIA-009).
pub const VIDEO_MAX_WIDTH: f32 = 640.0;
pub const VIDEO_MIN_WIDTH: f32 = 260.0;
/// Below a video's picture: name, progress bar and volume.
const VIDEO_CONTROLS: f32 = 108.0;

/// The size of the card of `abs` in a place `avail_w` wide; `width` is the width the link asks
/// for (`![[clip.mp4|480]]`, EDIT-052), which a video takes up to the place's width. A sound's
/// card keeps its own size.
pub fn card_size(abs: &Path, avail_w: f32, width: Option<f32>) -> Vec2 {
    if super::is_video(abs) {
        let w = match width {
            Some(w) => w.min(avail_w).max(VIDEO_MIN_WIDTH),
            None => avail_w.clamp(VIDEO_MIN_WIDTH, VIDEO_MAX_WIDTH),
        };
        Vec2::new(w, PAD + video_art_height(w) + VIDEO_CONTROLS)
    } else {
        Vec2::new(avail_w.clamp(260.0, CARD_MAX_WIDTH), CARD_HEIGHT)
    }
}

/// Where the parts of a card of width `card_w` are, from its top-left corner: the artwork (the
/// picture of a video above the controls, the cover of a sound beside them), the left edge of the
/// name, bar and volume, and the top of the controls. The PDF export draws the card from it too.
pub struct Geometry {
    pub art: Rect,
    pub text_x: f32,
    pub top: f32,
}

pub fn geometry(abs: &Path, card_w: f32) -> Geometry {
    if super::is_video(abs) {
        let art = Rect::from_min_size(Pos2::new(PAD, PAD), Vec2::new(card_w - 2.0 * PAD, video_art_height(card_w)));
        Geometry { art, text_x: 12.0, top: art.bottom() - 2.0 }
    } else {
        let art = Rect::from_min_size(Pos2::new(PAD, PAD), Vec2::splat(ART));
        Geometry { art, text_x: art.right() + 12.0, top: 0.0 }
    }
}

/// The video card that fills `room` as far as its shape lets it (the fullscreen view).
pub fn video_card_fit(room: Vec2) -> Vec2 {
    let w = room.x.min((room.y - PAD - VIDEO_CONTROLS) * 16.0 / 9.0 + 2.0 * PAD).max(VIDEO_MIN_WIDTH);
    Vec2::new(w, PAD + video_art_height(w) + VIDEO_CONTROLS)
}

/// The card width at which the video's picture is shown pixel for pixel ("100%").
pub fn video_natural_width(picture_w: u32) -> f32 {
    picture_w as f32 + 2.0 * PAD
}

/// The rectangle of a video card's picture, where its corner buttons go.
pub fn video_art_rect(card: Rect) -> Rect {
    Rect::from_min_size(card.min + Vec2::splat(PAD), Vec2::new(card.width() - 2.0 * PAD, video_art_height(card.width())))
}

fn video_art_height(card_w: f32) -> f32 {
    (card_w - 2.0 * PAD) * 9.0 / 16.0
}

/// Lay out a card in the flow of `ui` and draw it.
pub fn show(ui: &mut Ui, media: &MediaPlayer, abs: &Path, owner: &Path, width: Option<f32>) -> Response {
    let id = ui.auto_id_with(("media-card", abs));
    let (rect, resp) = ui.allocate_exact_size(card_size(abs, ui.available_width(), width), Sense::hover());
    paint(ui, rect, media, abs, owner, id, Keyboard::Takes);
    resp
}

/// Whether a click in a card gives the card the keyboard (MEDIA-004).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    /// The reading view and the fullscreen view: a click gives the card the keyboard, so Space toggles it.
    Takes,
    /// The editor: the keyboard stays with the text, so Space types a space (rabalyn/shale#441).
    Leaves,
}

/// Draw the card for `abs` into `rect` and handle what it is given: `id` must be unique among the
/// cards of a frame; `owner` is the note that shows it (closing its tab stops the sound).
pub fn paint(ui: &Ui, rect: Rect, media: &MediaPlayer, abs: &Path, owner: &Path, id: Id, keyboard: Keyboard) {
    let ctx = ui.ctx().clone();
    let v = media.view(&ctx, abs);
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect(rect, CornerRadius::same(8), theme::palette().bg_secondary, Stroke::new(1.0, theme::palette().border), egui::StrokeKind::Inside);
    let card = ui.interact(rect, id, Sense::click_and_drag());

    // --- artwork and the round button
    let geo = geometry(abs, rect.width());
    let art = geo.art.translate(rect.min.to_vec2());
    // the controls: beside the artwork of a sound, below the picture of a video
    let top = rect.top() + geo.top;
    paint_art(&painter, art, &v);
    if v.external {
        // no ffmpeg: the whole tile opens the video in the system's player (MEDIA-010)
        let tile = ui.interact(art, id.with("tile"), Sense::click_and_drag());
        if tile.clicked() {
            media.toggle(&ctx, abs, owner);
        }
    }
    let button = ui.interact(Rect::from_center_size(art.center(), Vec2::splat(BUTTON_RADIUS * 2.0)), id.with("play"), Sense::click_and_drag());
    let mut alpha = button_alpha(v.opacity, button.hovered());
    if v.unplayable {
        alpha /= 2;
    }
    painter.circle_filled(art.center(), BUTTON_RADIUS, Color32::from_black_alpha(alpha));
    paint_glyph(&painter, art.center(), v.playing, Color32::from_white_alpha(alpha));
    let label = if v.playing { "Pause" } else { "Play" };
    button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !v.unplayable, label));

    // --- the right side: name, message, progress, volume
    let x0 = rect.left() + geo.text_x;
    let x1 = rect.right() - 12.0;
    let width = (x1 - x0).max(40.0);
    let mut name = egui::text::LayoutJob::simple(v.file_name.clone(), egui::FontId::proportional(14.0), theme::palette().text_strong, width);
    name.wrap.max_rows = 1;
    name.wrap.break_anywhere = true;
    painter.galley(Pos2::new(x0, top + 10.0), painter.layout_job(name), theme::palette().text_strong);
    let mut download_link = None;
    if let Some(m) = &v.message {
        let failed = v.download.as_ref().is_some_and(|d| d.error.is_some());
        let colour = if v.unplayable || failed { theme::palette().link_missing } else { theme::palette().muted };
        let mut job = egui::text::LayoutJob::simple(m.clone(), egui::FontId::proportional(12.0), colour, width);
        job.wrap.max_rows = 1;
        let galley = painter.layout_job(job);
        let end = x0 + galley.size().x;
        painter.galley(Pos2::new(x0, top + 29.0), galley, colour);
        if v.download.as_ref().is_some_and(|d| !d.running) {
            download_link = Some(paint_download_link(ui, &painter, Pos2::new(end + 6.0, top + 29.0), x1, id));
        } else if v.busy && v.download.is_some() {
            egui::Spinner::new().size(12.0).paint_at(ui, Rect::from_min_size(Pos2::new(end + 6.0, top + 30.0), Vec2::splat(12.0)));
        }
    }
    if download_link.is_some_and(|l| l.clicked()) {
        media.download(abs);
        ctx.request_repaint();
    }

    // progress bar
    let bar_hit = Rect::from_min_size(Pos2::new(x0, top + 48.0), Vec2::new(width, 16.0));
    let bar = ui.interact(bar_hit, id.with("seek"), Sense::click_and_drag());
    // a click before the length is known waits for it (MEDIA-005, #258)
    let seekable = !v.unplayable && !v.external && v.download.is_none();
    bar.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Slider, seekable, "Position"));
    let preview_id = id.with("seek-preview");
    let frac_at = |p: Pos2| ((p.x - bar_hit.left()) / bar_hit.width()).clamp(0.0, 1.0) as f64;
    let mut preview: Option<f64> = ctx.data(|d| d.get_temp(preview_id));
    if seekable {
        let seek = |f: f64| match v.duration {
            Some(d) => media.seek_to(&ctx, abs, owner, f * d),
            None => media.seek_to_share(&ctx, abs, owner, f),
        };
        if bar.dragged() {
            if let Some(p) = bar.interact_pointer_pos() {
                preview = Some(frac_at(p));
                ctx.data_mut(|data| data.insert_temp(preview_id, frac_at(p)));
            }
        }
        if bar.drag_stopped() {
            // the seek happens once, when the mouse is let go (MEDIA-005)
            if let Some(f) = preview.take() {
                seek(f);
            }
            ctx.data_mut(|data| data.remove_temp::<f64>(preview_id));
        } else if bar.clicked() {
            if let Some(p) = bar.interact_pointer_pos() {
                seek(frac_at(p));
            }
        }
    }
    let share = preview.or(v.waiting_share);
    let shown = share.zip(v.duration).map(|(f, d)| f * d).unwrap_or(v.position);
    let frac = match (share, v.duration) {
        (Some(f), _) => f as f32,
        (None, Some(d)) => (shown / d).clamp(0.0, 1.0) as f32,
        (None, None) => 0.0,
    };
    let track = Rect::from_center_size(bar_hit.center(), Vec2::new(width, 4.0));
    painter.rect_filled(track, CornerRadius::same(2), theme::palette().faint.gamma_multiply(0.6));
    let mut filled = track;
    filled.set_right(track.left() + track.width() * frac);
    painter.rect_filled(filled, CornerRadius::same(2), theme::palette().accent);
    if seekable && (bar.hovered() || share.is_some()) {
        painter.circle_filled(Pos2::new(filled.right(), track.center().y), 5.0, theme::palette().text_strong);
    }
    let times = egui::FontId::proportional(11.0);
    painter.text(Pos2::new(x0, top + 66.0), egui::Align2::LEFT_TOP, clock(shown), times.clone(), theme::palette().muted);
    let total = v.duration.map(clock).unwrap_or_else(|| "–:––".into());
    painter.text(Pos2::new(x1, top + 66.0), egui::Align2::RIGHT_TOP, total, times.clone(), theme::palette().muted);

    // volume: mute toggle and slider
    let mute_rect = Rect::from_min_size(Pos2::new(x0, top + 82.0), Vec2::splat(20.0));
    let mute = ui.interact(mute_rect, id.with("mute"), Sense::click_and_drag());
    let mute_label = if v.muted { "Unmute" } else { "Mute" };
    mute.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, mute_label));
    if mute.clicked() {
        media.set_muted(!v.muted);
    }
    let icon_colour = if mute.hovered() { theme::palette().text_strong } else { theme::palette().muted };
    paint_speaker(&painter, mute_rect.shrink(1.0), v.muted || v.volume <= 0.0, icon_colour);
    let vol_hit = Rect::from_min_size(Pos2::new(mute_rect.right() + 6.0, top + 82.0), Vec2::new((width * 0.4).clamp(60.0, 120.0), 20.0));
    let vol = ui.interact(vol_hit, id.with("volume"), Sense::click_and_drag());
    vol.widget_info(|| egui::WidgetInfo::slider(true, v.volume as f64, "Volume"));
    if vol.dragged() || vol.clicked() {
        if let Some(p) = vol.interact_pointer_pos() {
            media.set_volume(((p.x - vol_hit.left()) / vol_hit.width()).clamp(0.0, 1.0));
        }
    }
    let vtrack = Rect::from_center_size(vol_hit.center(), Vec2::new(vol_hit.width(), 4.0));
    painter.rect_filled(vtrack, CornerRadius::same(2), theme::palette().faint.gamma_multiply(0.6));
    let mut vfill = vtrack;
    vfill.set_right(vtrack.left() + vtrack.width() * if v.muted { 0.0 } else { v.volume });
    painter.rect_filled(vfill, CornerRadius::same(2), theme::palette().muted);
    painter.text(Pos2::new(vol_hit.right() + 8.0, vol_hit.center().y), egui::Align2::LEFT_CENTER, format!("{} %", (if v.muted { 0.0 } else { v.volume } * 100.0).round() as u32), times, theme::palette().muted);

    // --- toggling: the button, or Space while the card has the keyboard (MEDIA-004)
    // a click in the card gives it the keyboard, except in the editor, where it stays with the text
    if keyboard == Keyboard::Takes && [&card, &button, &bar, &mute, &vol].iter().any(|r| r.clicked() || r.drag_stopped()) {
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

/// "— Download" after the "Not downloaded (12 MB)" text (SYNC-020); the link's response.
fn paint_download_link(ui: &Ui, painter: &Painter, at: Pos2, right: f32, id: Id) -> Response {
    let galley = painter.layout_no_wrap("— Download".into(), egui::FontId::proportional(12.0), theme::palette().accent);
    let rect = Rect::from_min_size(at, galley.size()).intersect(Rect::everything_left_of(right.max(at.x + 1.0)));
    let link = ui.interact(rect, id.with("download"), Sense::click_and_drag());
    link.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Download"));
    painter.galley(at, galley, theme::palette().accent);
    if link.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    link
}

/// A picture that is still an LFS pointer: "Not downloaded (12 MB) — Download", or the spinner
/// while it comes (SYNC-020). None when the file is real, so the caller shows it as it does.
pub fn show_not_downloaded(ui: &mut Ui, media: &MediaPlayer, abs: &Path) -> Option<Response> {
    let n = media.notice(abs)?;
    let name = abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let failed = n.error.is_some();
    let mut clicked = false;
    let r = ui
        .horizontal(|ui| {
            ui.label(egui::RichText::new(format!("{name}: {}", n.line())).color(if failed { theme::palette().link_missing } else { theme::palette().muted }));
            if n.running {
                ui.spinner();
            } else {
                clicked = ui.link("Download").clicked();
            }
        })
        .response;
    if clicked {
        media.download(abs);
    }
    if n.running {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    Some(r)
}

/// What stands in place of an outside file that cannot be shown (MEDIA-016): where it should be, or
/// that its library is not set up here. Cut to the width it has; the whole text is in the tooltip.
pub fn show_notice(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Label::new(egui::RichText::new(text).color(theme::palette().link_missing)).truncate()).on_hover_text(text)
}

fn paint_art(painter: &Painter, art: Rect, v: &View) {
    painter.rect_filled(art, CornerRadius::same(6), if v.cover.is_some() { Color32::from_rgb(20, 20, 20) } else { theme::palette().bg_hover });
    match &v.cover {
        Some(tex) => {
            let [w, h] = tex.size_vec2().into();
            let k = (art.width() / w).min(art.height() / h);
            let r = Rect::from_center_size(art.center(), Vec2::new(w * k, h * k));
            painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            // no cover: a neutral tile with the file name
            let mut job = egui::text::LayoutJob::simple(v.file_name.clone(), egui::FontId::proportional(11.0), theme::palette().faint, ART - 12.0);
            job.wrap.max_rows = 4;
            job.wrap.break_anywhere = true;
            job.halign = egui::Align::Center;
            let galley = painter.layout_job(job);
            let pos = Pos2::new(art.center().x - galley.size().x / 2.0, art.bottom() - galley.size().y - 8.0);
            painter.galley(pos, galley, theme::palette().faint);
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

#[cfg(test)]
mod tests {
    use super::*;

    // spec: MEDIA-012
    #[test]
    fn a_videos_card_takes_the_width_of_its_link_and_fits_the_fullscreen_view() {
        let clip = Path::new("clip.mp4");
        assert_eq!(card_size(clip, 900.0, None).x, 640.0);
        assert_eq!(card_size(clip, 900.0, Some(800.0)).x, 800.0, "a link can ask for more than the usual width");
        assert_eq!(card_size(clip, 500.0, Some(800.0)).x, 500.0, "but never more than the place has");
        assert_eq!(card_size(clip, 900.0, Some(10.0)).x, 260.0);
        let s = card_size(clip, 900.0, Some(480.0));
        assert_eq!(s.y, PAD + video_art_height(480.0) + VIDEO_CONTROLS);
        assert_eq!(card_size(Path::new("a.mp3"), 900.0, Some(100.0)), Vec2::new(520.0, CARD_HEIGHT), "a sound keeps its card");
        // wide room: the height decides; tall room: the width
        let wide = video_card_fit(Vec2::new(1800.0, 700.0));
        assert!((wide.y - 700.0).abs() < 1.0, "{wide:?}");
        let tall = video_card_fit(Vec2::new(500.0, 1000.0));
        assert_eq!(tall.x, 500.0);
        assert_eq!(video_natural_width(1280), 1296.0);
    }
}
