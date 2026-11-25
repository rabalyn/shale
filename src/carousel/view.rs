//! A carousel in the reading view (EDIT-143): one image at a time, the buttons "Previous image" and
//! "Next image", a dot per image, the caption and "2 / 6" below; the arrow keys go through the
//! images while the pointer is over it, and they go round at the ends. Which image is shown is
//! state of the screen (by carousel), not of the note.

use super::{Carousel, Slide};
use crate::theme;
use eframe::egui::{self, Id, Key, Modifiers, Pos2, Role, Sense, Shape, Stroke, Ui, Vec2, WidgetInfo};

/// The accessibility label of the button to the image before.
pub const PREVIOUS: &str = "Previous image";
/// The accessibility label of the button to the image after.
pub const NEXT: &str = "Next image";
const BUTTON: f32 = 24.0;
const DOT: f32 = 9.0;

/// The label of the position: "2 / 6" for the second of six images.
pub fn position(shown: usize, of: usize) -> String {
    format!("{} / {of}", shown + 1)
}

/// The image after (or, with `back`, before) `shown` of `len`: from the last one it goes to the first.
pub fn step(shown: usize, len: usize, back: bool) -> usize {
    if back {
        (shown + len - 1) % len
    } else {
        (shown + 1) % len
    }
}

/// Draw the carousel `c`; `id` tells it from the other carousels (its image is kept by it), and
/// `draw` draws one image the way the reading view draws an embed on a line of its own.
pub fn show(ui: &mut Ui, id: Id, c: &Carousel, mut draw: impl FnMut(&mut Ui, &Slide)) {
    let len = c.slides.len();
    let mut at = ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(len - 1);
    let frame = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 4, top: 2, bottom: 2 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        if let Some(t) = &c.title {
            ui.label(egui::RichText::new(t).strong().color(theme::palette().text_strong));
            ui.add_space(2.0);
        }
        draw(ui, &c.slides[at]);
        if let Some(caption) = &c.slides[at].caption {
            ui.label(egui::RichText::new(caption).color(theme::palette().muted));
        }
        ui.horizontal(|ui| {
            if nav(ui, id.with("previous"), PREVIOUS, true) {
                at = step(at, len, true);
            }
            if nav(ui, id.with("next"), NEXT, false) {
                at = step(at, len, false);
            }
            ui.label(egui::RichText::new(position(at, len)).color(theme::palette().muted));
            for n in 0..len {
                if dot(ui, id.with(("dot", n)), n, n == at) {
                    at = n;
                }
            }
        });
    });
    let r = frame.response.rect;
    ui.painter().line_segment([r.left_top() + Vec2::new(2.0, 0.0), r.left_bottom() + Vec2::new(2.0, 0.0)], Stroke::new(3.0, theme::palette().accent.gamma_multiply(0.7)));
    if ui.rect_contains_pointer(r) {
        let (left, right) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowLeft), i.consume_key(Modifiers::NONE, Key::ArrowRight)));
        if left != right {
            at = step(at, len, left);
        }
    }
    if ui.data(|d| d.get_temp::<usize>(id)) != Some(at) {
        ui.data_mut(|d| d.insert_temp(id, at));
        ui.ctx().request_repaint();
    }
    ui.add_space(6.0);
}

/// A round-cornered button with a triangle; true when clicked.
fn nav(ui: &mut Ui, id: Id, label: &str, back: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(BUTTON), Sense::hover());
    let r = ui.interact(rect, id, Sense::click());
    let r = crate::clickable::pointer(r);
    r.widget_info(|| WidgetInfo::labeled(Role::Button, true, label));
    let p = theme::palette();
    ui.painter().rect_filled(rect, 4.0, if r.hovered() { p.bg_hover } else { p.widget_bg });
    let (c, h) = (rect.center(), BUTTON * 0.22);
    let s = if back { -1.0 } else { 1.0 };
    let tri = vec![Pos2::new(c.x + s * h, c.y), Pos2::new(c.x - s * h * 0.8, c.y - h * 1.1), Pos2::new(c.x - s * h * 0.8, c.y + h * 1.1)];
    ui.painter().add(Shape::convex_polygon(tri, p.text, Stroke::NONE));
    r.on_hover_text(label).clicked()
}

/// The dot of image `n`: filled for the one shown; true when clicked.
fn dot(ui: &mut Ui, id: Id, n: usize, shown: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(DOT + 5.0, BUTTON), Sense::hover());
    let r = crate::clickable::pointer(ui.interact(rect, id, Sense::click()));
    r.widget_info(|| WidgetInfo::labeled(Role::Button, true, format!("Image {}", n + 1)));
    let p = theme::palette();
    let colour = if shown {
        p.accent
    } else if r.hovered() {
        p.muted
    } else {
        p.faint
    };
    ui.painter().circle_filled(rect.center(), DOT / 2.0, colour);
    r.clicked()
}
