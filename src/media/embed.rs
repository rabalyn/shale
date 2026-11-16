//! What the editor lays over a shown picture or video (EDIT-024, MEDIA-012): draggable edges and
//! corners that change its size, a round "100%" button (natural size) and a round fullscreen
//! button, drawn like the play button of the player (its opacity setting, opaque on hover), and the
//! fullscreen view itself. The size is kept in the link as `![[pic.png|300]]`; this module only
//! says what the user did, the editor writes the link.

use super::player::button_alpha;
use super::MediaPlayer;
use crate::theme;
use eframe::egui::{self, Color32, CursorIcon, Id, Order, Painter, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, Vec2};
use std::path::{Path, PathBuf};

/// Narrowest a picture can be dragged to.
pub const MIN_WIDTH: f32 = 60.0;
const EDGE: f32 = 6.0;
const CORNER: f32 = 14.0;
const BUTTON_R: f32 = 17.0;

/// What the user did with a shown picture or video in one frame.
#[derive(Default, Debug, PartialEq)]
pub struct Did {
    /// A drag was let go: the width to keep in the link.
    pub resized: Option<f32>,
    /// The "100%" button was clicked.
    pub natural: bool,
    /// The fullscreen button was clicked.
    pub fullscreen: bool,
}

/// A shown embed: where it is, which link it is, and what its size may be.
pub struct Shown {
    pub rect: Rect,
    /// byte where its link starts in the note: tells which embed a drag belongs to
    pub at: usize,
    /// height / width
    pub aspect: f32,
    pub min_w: f32,
    pub max_w: f32,
    /// the part of it the round buttons sit in (a video card's picture, not its controls)
    pub buttons: Rect,
}

/// What the fullscreen view shows.
pub enum Full {
    Image(TextureHandle),
    Video(PathBuf),
}

struct Drag {
    /// where the link starts in the note: tells which embed is being dragged
    at: usize,
    /// the width when it was grabbed
    from: f32,
    width: f32,
}

/// The state of the editor's embeds: a drag in progress, the fullscreen view.
#[derive(Default)]
pub struct Embeds {
    drag: Option<Drag>,
    pub full: Option<Full>,
}

impl Embeds {
    /// The width the embed that starts at byte `at` is being dragged to, if it is.
    pub fn dragging(&self, at: usize) -> Option<f32> {
        self.drag.as_ref().filter(|d| d.at == at).map(|d| d.width)
    }

    /// The handles and buttons of an embed, shown while the mouse is over it or it is being
    /// dragged. `id` is the embed's own; `opacity` is the round buttons' (MEDIA-004).
    pub fn controls(&mut self, ui: &Ui, id: Id, opacity: f32, e: &Shown) -> Did {
        let Shown { rect, at, aspect, .. } = *e;
        let mut did = Did::default();
        if self.drag.as_ref().is_some_and(|d| d.at == at) && !ui.input(|i| i.pointer.primary_down()) {
            // let go: the size to keep, if it was changed at all
            if let Some(d) = self.drag.take().filter(|d| (d.width - d.from).abs() >= 1.0) {
                did.resized = Some(d.width);
            }
            return did;
        }
        let mine = self.drag.as_ref().is_some_and(|d| d.at == at);
        let hovered = ui.rect_contains_pointer(rect.expand(EDGE)) && self.drag.is_none();
        if !hovered && !mine {
            return did;
        }
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Outside);
        // eight handles: (x side, y side) each -1, 0 or 1; the corners win over the edges
        let zones: [(i8, i8); 8] = [(-1, -1), (1, -1), (-1, 1), (1, 1), (0, -1), (0, 1), (-1, 0), (1, 0)];
        for (n, (sx, sy)) in zones.into_iter().enumerate() {
            let zone = handle_rect(rect, sx, sy);
            let r = ui.interact(zone, id.with(("handle", n)), Sense::click_and_drag());
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(match (sx, sy) {
                    (0, _) => CursorIcon::ResizeVertical,
                    (_, 0) => CursorIcon::ResizeHorizontal,
                    (x, y) if x == y => CursorIcon::ResizeNwSe,
                    _ => CursorIcon::ResizeNeSw,
                });
            }
            // the grip starts with the press, so the drag goes on outside the picture too
            if r.is_pointer_button_down_on() && self.drag.is_none() {
                self.drag = Some(Drag { at, from: rect.width(), width: rect.width() });
            }
            if r.dragged() {
                if let Some(d) = self.drag.as_mut().filter(|d| d.at == at) {
                    let delta = r.drag_delta();
                    let (dx, dy) = (delta.x * sx as f32, delta.y * sy as f32 / aspect.max(0.05));
                    // a corner follows the bigger of the two moves
                    let by = if dx.abs() >= dy.abs() { dx } else { dy };
                    d.width = (d.width + by).clamp(e.min_w, e.max_w.max(e.min_w));
                }
            }
            if r.hovered() || r.dragged() {
                painter.rect_filled(zone.intersect(rect.expand(EDGE)), 2.0, theme::ACCENT.gamma_multiply(0.5));
            }
        }
        // the round buttons, in the corner of the picture
        let fs = Pos2::new(e.buttons.right() - 10.0 - BUTTON_R, e.buttons.top() + 10.0 + BUTTON_R);
        let hundred = Pos2::new(fs.x - 2.0 * BUTTON_R - 8.0, fs.y);
        if round_button(ui, &painter, hundred, id.with("natural"), opacity, "100 %", |p, c, col| {
            p.text(c, egui::Align2::CENTER_CENTER, "100%", egui::FontId::proportional(10.5), col);
        }) {
            did.natural = true;
        }
        if round_button(ui, &painter, fs, id.with("fullscreen"), opacity, "Fullscreen", paint_expand) {
            did.fullscreen = true;
        }
        did
    }

    /// The fullscreen view over the whole window: the picture, or the video's card, fitted in;
    /// Esc, the cross or a click beside it closes it.
    pub fn show_full(&mut self, id: Id, ctx: &egui::Context, media: &MediaPlayer, owner: &Path) {
        let Some(full) = &self.full else { return };
        let screen = ctx.content_rect();
        let mut close = false;
        egui::Area::new(id.with("fullscreen")).order(Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let backdrop = ui.allocate_rect(screen, Sense::click());
            let painter = ui.painter().clone();
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(240));
            let room = screen.shrink(24.0);
            match full {
                Full::Image(tex) => {
                    let size = tex.size_vec2();
                    let k = (room.width() / size.x).min(room.height() / size.y);
                    let r = Rect::from_center_size(screen.center(), size * k);
                    painter.image(tex.id(), r, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                }
                Full::Video(abs) => {
                    let size = super::widget::video_card_fit(room.size());
                    super::widget::paint(ui, Rect::from_center_size(screen.center(), size), media, abs, owner, id.with(("fullscreen-card", abs)));
                }
            }
            if round_button(ui, &painter, Pos2::new(screen.right() - 30.0, screen.top() + 30.0), id.with("fullscreen-close"), media.opacity(), "Close", paint_cross) || backdrop.clicked() {
                close = true;
            }
        });
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        if close {
            self.full = None;
        }
    }
}

/// The part of the picture a handle grabs: the corners are squares, the edges strips between them.
fn handle_rect(r: Rect, sx: i8, sy: i8) -> Rect {
    let x = |s: i8| match s {
        -1 => r.left(),
        1 => r.right(),
        _ => r.center().x,
    };
    let y = |s: i8| match s {
        -1 => r.top(),
        1 => r.bottom(),
        _ => r.center().y,
    };
    match (sx, sy) {
        (0, _) => Rect::from_x_y_ranges(r.left() + CORNER..=r.right() - CORNER, y(sy) - EDGE..=y(sy) + EDGE),
        (_, 0) => Rect::from_x_y_ranges(x(sx) - EDGE..=x(sx) + EDGE, r.top() + CORNER..=r.bottom() - CORNER),
        _ => Rect::from_center_size(Pos2::new(x(sx), y(sy)), Vec2::splat(CORNER)),
    }
}

/// A round button like the play button (it takes drags too, so the text box under it does not get the press): black disc and white mark, as opaque as the setting says
/// and fully opaque under the mouse. True when it was clicked.
fn round_button(ui: &Ui, painter: &Painter, at: Pos2, id: Id, opacity: f32, label: &str, mark: impl FnOnce(&Painter, Pos2, Color32)) -> bool {
    let r = ui.interact(Rect::from_center_size(at, Vec2::splat(BUTTON_R * 2.0)), id, Sense::click_and_drag());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let alpha = button_alpha(opacity, r.hovered());
    painter.circle_filled(at, BUTTON_R, Color32::from_black_alpha(alpha));
    mark(painter, at, Color32::from_white_alpha(alpha.max(120)));
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    r.clicked()
}

/// Four corner brackets pointing outwards.
fn paint_expand(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.6, col);
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
        let corner = c + Vec2::new(sx * 6.5, sy * 6.5);
        p.line_segment([corner, corner - Vec2::new(sx * 4.5, 0.0)], s);
        p.line_segment([corner, corner - Vec2::new(0.0, sy * 4.5)], s);
    }
}

fn paint_cross(p: &Painter, c: Pos2, col: Color32) {
    let s = Stroke::new(1.8, col);
    p.line_segment([c + Vec2::new(-5.5, -5.5), c + Vec2::new(5.5, 5.5)], s);
    p.line_segment([c + Vec2::new(-5.5, 5.5), c + Vec2::new(5.5, -5.5)], s);
}
