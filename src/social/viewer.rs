//! The picture viewer of the Feeds tab (SOC-044): a post's pictures shown big over the tab,
//! fitted to the window, with zoom, stepping through the pictures of the post and the
//! description of the picture. The files come through the `ImageCache` (SOC-013), so the
//! window never waits for the network.

use super::feed_ui::open_web;
use super::images::ImageCache;
use super::{MediaKind, Post};
use crate::media::look::Pic;
use crate::theme;
use eframe::egui::{self, Color32, Key, Modifiers, Rect, RichText, Sense, Vec2};

/// How much one zoom key press or button click zooms.
const ZOOM_STEP: f32 = 1.25;
const MIN_SCALE: f32 = 0.02;
const MAX_SCALE: f32 = 32.0;
/// The height of the bar at the top.
const BAR: f32 = 40.0;
/// The most height the description may take before it scrolls.
const ALT_MAX: f32 = 110.0;

/// One picture of the viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// the full size picture
    pub url: String,
    /// the description written by the author ("" when none)
    pub alt: String,
}

/// The picture viewer open in one tab.
#[derive(Debug)]
pub struct Viewer {
    /// the tab it covers
    pub tab: u64,
    /// the pictures of the post (not its videos)
    pub items: Vec<Item>,
    pub index: usize,
    /// None: fitted to the window; else the scale (1.0 = one picture pixel per point)
    pub zoom: Option<f32>,
    /// where the picture was dragged to, from the middle of the window
    pub pan: Vec2,
}

impl Viewer {
    /// The viewer for the picture `media_index` of the post (an index into `p.media`); None when
    /// that is not a picture.
    pub fn open(tab: u64, p: &Post, media_index: usize) -> Option<Viewer> {
        let pictures: Vec<(usize, &super::Media)> = p.media.iter().enumerate().filter(|(_, m)| m.kind == MediaKind::Image).collect();
        let index = pictures.iter().position(|(i, _)| *i == media_index)?;
        let items = pictures.iter().map(|(_, m)| Item { url: m.url.clone(), alt: m.alt.trim().to_string() }).collect();
        Some(Viewer { tab, items, index, zoom: None, pan: Vec2::ZERO })
    }

    fn go(&mut self, to: usize) {
        self.index = to.min(self.items.len() - 1);
        self.zoom = None;
        self.pan = Vec2::ZERO;
    }
}

/// What the keys asked for this frame.
struct Keys {
    close: bool,
    step: i32,
    zoom: f32,
    toggle: bool,
}

fn read_keys(ctx: &egui::Context) -> Keys {
    let mut k = Keys { close: false, step: 0, zoom: 1.0, toggle: false };
    ctx.input_mut(|i| {
        k.close = i.consume_key(Modifiers::NONE, Key::Escape);
        if i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
            k.step -= 1;
        }
        if i.consume_key(Modifiers::NONE, Key::ArrowRight) {
            k.step += 1;
        }
        if i.consume_key(Modifiers::NONE, Key::Plus) || i.consume_key(Modifiers::NONE, Key::Equals) || i.consume_key(Modifiers::SHIFT, Key::Equals) {
            k.zoom *= ZOOM_STEP;
        }
        if i.consume_key(Modifiers::NONE, Key::Minus) {
            k.zoom /= ZOOM_STEP;
        }
        k.toggle = i.consume_key(Modifiers::NONE, Key::Num0);
    });
    k
}

fn text(s: impl Into<String>) -> RichText {
    RichText::new(s).color(Color32::WHITE)
}

/// Show the viewer over `rect` (the tab). `keys`: plain keys may be used (no dialog is open).
/// Returns true when it is to be closed.
pub fn show(ctx: &egui::Context, rect: Rect, v: &mut Viewer, images: &ImageCache, rects: &mut Vec<(String, Rect)>, keys: bool) -> bool {
    let k = if keys { read_keys(ctx) } else { Keys { close: false, step: 0, zoom: 1.0, toggle: false } };
    let mut close = k.close;
    let mut step = k.step;
    let mut zoom_by = k.zoom;
    let mut toggle = k.toggle;
    let last = v.items.len() - 1;
    let alt = v.items[v.index].alt.clone();
    let alt_h = if alt.is_empty() { 0.0 } else { ALT_MAX };
    let pic = Rect::from_min_max(rect.min + Vec2::new(0.0, BAR), rect.max - Vec2::new(0.0, alt_h)).shrink(8.0);
    // the neighbours are fetched ahead, so stepping does not wait
    for n in [v.index.wrapping_sub(1), v.index + 1] {
        if let Some(i) = v.items.get(n) {
            images.get(&i.url);
        }
    }
    egui::Area::new(egui::Id::unique(("social-viewer", v.tab))).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        // the backdrop takes the clicks and the wheel, so nothing reaches the list below
        ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(238));
        // ---- the bar
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BAR)).shrink2(Vec2::new(10.0, 6.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let r = ui.add_enabled(v.index > 0, egui::Button::new("◀")).on_hover_text("Previous picture (Left)");
            rects.push(("viewer:prev".into(), r.rect));
            step -= r.clicked() as i32;
            let r = ui.add_enabled(v.index < last, egui::Button::new("▶")).on_hover_text("Next picture (Right)");
            rects.push(("viewer:next".into(), r.rect));
            step += r.clicked() as i32;
            let r = ui.label(text(format!("{} / {}", v.index + 1, v.items.len())));
            rects.push(("viewer:count".into(), r.rect));
            let r = ui.button("−").on_hover_text("Zoom out (−, or the mouse wheel)");
            rects.push(("viewer:zoom-out".into(), r.rect));
            if r.clicked() {
                zoom_by /= ZOOM_STEP;
            }
            let r = ui.button("+").on_hover_text("Zoom in (+, or the mouse wheel)");
            rects.push(("viewer:zoom-in".into(), r.rect));
            if r.clicked() {
                zoom_by *= ZOOM_STEP;
            }
            let r = ui.button(if v.zoom.is_some() { "Fit" } else { "100%" }).on_hover_text("Fitted to the window, or the picture's own size (0)");
            rects.push(("viewer:fit".into(), r.rect));
            toggle |= r.clicked();
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let r = ui.button("Close").on_hover_text("Close the viewer (Esc)");
                rects.push(("viewer:close".into(), r.rect));
                close |= r.clicked();
                let r = ui.button("Open in browser");
                rects.push(("viewer:browser".into(), r.rect));
                if r.clicked() {
                    open_web(&v.items[v.index].url);
                }
            });
        });
        // ---- the picture
        let url = v.items[v.index].url.clone();
        let state = match images.get(&url) {
            Some(path) => crate::media::picture(ctx, &path),
            None if images.failed(&url) => Pic::Broken,
            None => Pic::Loading,
        };
        match state {
            Pic::Ready(t) => {
                let size = t.size_vec2();
                let fit = (pic.width() / size.x).min(pic.height() / size.y).clamp(MIN_SCALE, MAX_SCALE);
                let area = ui.interact(pic, ui.scope_id().with("viewer-picture"), Sense::drag());
                let wheel = if area.hovered() { ctx.input(|i| i.smooth_scroll_delta().y) } else { 0.0 };
                if wheel != 0.0 {
                    zoom_by *= (wheel * 0.004).exp();
                }
                if toggle {
                    v.zoom = if v.zoom.is_some() { None } else { Some(1.0) };
                    v.pan = Vec2::ZERO;
                }
                if zoom_by != 1.0 {
                    v.zoom = Some((v.zoom.unwrap_or(fit) * zoom_by).clamp(MIN_SCALE, MAX_SCALE));
                }
                v.pan = if v.zoom.is_some() { v.pan + area.drag_delta() } else { Vec2::ZERO };
                let scale = v.zoom.unwrap_or(fit);
                let drawn = Rect::from_center_size(pic.center() + v.pan, size * scale);
                ui.painter_at(pic).image(t.id(), drawn, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                rects.push(("viewer:image".into(), drawn.intersect(pic)));
                if v.zoom.is_some() {
                    ui.ctx().set_cursor_icon(if area.dragged() { egui::CursorIcon::Grabbing } else { egui::CursorIcon::Grab });
                }
            }
            Pic::Broken => {
                let at = Rect::from_center_size(pic.center(), Vec2::new(pic.width().min(420.0), 60.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
                    let r = ui.add(egui::Label::new(RichText::new("Could not load this picture.").color(theme::palette().link_missing)).wrap());
                    rects.push(("viewer:error".into(), r.rect));
                });
            }
            Pic::Loading => {
                let r = Rect::from_center_size(pic.center(), Vec2::splat(28.0));
                egui::Spinner::new().size(28.0).color(Color32::WHITE).paint_at(ui, r);
                rects.push(("viewer:spinner".into(), r));
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        }
        // ---- the description
        if !alt.is_empty() {
            let at = Rect::from_min_max(egui::pos2(rect.min.x + 16.0, rect.max.y - alt_h), rect.max - Vec2::new(16.0, 6.0));
            ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                egui::ScrollArea::vertical().id_salt("viewer-alt").auto_shrink([false, true]).show(ui, |ui| {
                    let r = ui.add(egui::Label::new(text(&alt).size(crate::fonts::px(14.0))).wrap());
                    rects.push(("viewer:alt".into(), r.rect));
                });
            });
        }
    });
    if step != 0 {
        v.go((v.index as i64 + step as i64).clamp(0, last as i64) as usize);
    }
    close
}
