//! The viewer of the Feeds tab (SOC-044, SOC-076): a post's pictures shown big over the tab,
//! fitted to the window, with zoom, stepping through the pictures and videos of the post and the
//! description of each. The pictures come through the `ImageCache` (SOC-013) and the videos
//! through `Clips`, so the window never waits for the network; a video plays in the app's media
//! player (MEDIA-028) and stops when the viewer closes or steps away from it.

use super::clips::{Clip, Clips, Source};
use super::feed_ui::open_web;
use super::images::ImageCache;
use super::{MediaKind, Post};
use crate::media::look::Pic;
use crate::media::player::Start;
use crate::media::widget::{self, Card, Keyboard};
use crate::media::MediaPlayer;
use crate::theme;
use eframe::egui::{self, Color32, Key, Modifiers, Rect, RichText, Sense, Vec2};
use std::path::Path;

/// How much one zoom key press or button click zooms.
const ZOOM_STEP: f32 = 1.25;
const MIN_SCALE: f32 = 0.02;
const MAX_SCALE: f32 = 32.0;
/// The height of the bar at the top.
const BAR: f32 = 40.0;
/// The most height the description may take before it scrolls.
const ALT_MAX: f32 = 110.0;
/// The height of the line that says why a video cannot play.
const NOTE: f32 = 28.0;

/// Stands for the note that owns the video of the viewer (`MediaPlayer::stop_for_note`).
const OWNER: &str = "shale-feed-viewer";

/// What the viewer needs from the app besides the post.
pub struct Env<'a> {
    pub images: &'a ImageCache,
    pub clips: &'a Clips,
    pub media: &'a MediaPlayer,
}

/// The video or GIF of an item.
#[derive(Clone, Debug, PartialEq)]
pub struct Video {
    /// the file to play; None for a Bluesky video that names no file
    pub source: Option<Source>,
    /// Mastodon's "gifv": plays muted and again and again
    pub gif: bool,
    /// the preview picture ("" when none)
    pub preview: String,
    /// a name for the file with an extension, shown on the player
    pub name: String,
}

/// One picture or video of the viewer.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// the full size picture; of a video: the address "Open in browser" opens
    pub url: String,
    /// the description written by the author ("" when none)
    pub alt: String,
    pub video: Option<Video>,
}

/// The viewer open in one tab.
pub struct Viewer {
    /// the tab it covers
    pub tab: u64,
    /// the pictures and videos of the post, in the order of the post
    pub items: Vec<Item>,
    pub index: usize,
    media: MediaPlayer,
    /// ffmpeg was found when the viewer opened: a video can play
    can_play: bool,
    /// the item whose video was started
    started: Option<usize>,
    /// None: fitted to the window; else the scale (1.0 = one picture pixel per point)
    pub zoom: Option<f32>,
    /// where the picture was dragged to, from the middle of the window
    pub pan: Vec2,
}

impl std::fmt::Debug for Viewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Viewer").field("tab", &self.tab).field("items", &self.items).field("index", &self.index).finish()
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        self.media.stop_for_note(Path::new(OWNER));
    }
}

impl Viewer {
    /// The viewer for the picture or video `media_index` of the post (an index into `p.media`);
    /// None when that is neither.
    pub fn open(tab: u64, p: &Post, media_index: usize, media: &MediaPlayer) -> Option<Viewer> {
        let shown: Vec<(usize, &super::Media)> = p.media.iter().enumerate().filter(|(_, m)| matches!(m.kind, MediaKind::Image | MediaKind::Video | MediaKind::Gifv)).collect();
        let index = shown.iter().position(|(i, _)| *i == media_index)?;
        let items = shown.iter().map(|(_, m)| item(p, m)).collect();
        Some(Viewer { tab, items, index, media: media.clone(), can_play: media.ffmpeg().is_some(), started: None, zoom: None, pan: Vec2::ZERO })
    }

    fn go(&mut self, to: usize) {
        self.index = to.min(self.items.len() - 1);
        self.zoom = None;
        self.pan = Vec2::ZERO;
        // what played stops: a video is not heard from another page
        self.media.stop_for_note(Path::new(OWNER));
        self.started = None;
    }
}

fn item(p: &Post, m: &super::Media) -> Item {
    let alt = m.alt.trim().to_string();
    if m.kind == MediaKind::Image {
        return Item { url: m.url.clone(), alt, video: None };
    }
    let source = Source::of(m, &p.author.id);
    // a Bluesky video has no address of its own to open: its post has
    let url = if matches!(source, Some(Source::Url(_))) { m.url.clone() } else { p.url.clone() };
    Item { url, alt, video: Some(Video { source, gif: m.kind == MediaKind::Gifv, preview: m.preview_url.clone(), name: m.file_name_hint.clone() }) }
}

/// What the keys asked for this frame.
struct Keys {
    close: bool,
    step: i32,
    zoom: f32,
    toggle: bool,
    /// Space: play or pause the video
    space: bool,
}

fn read_keys(ctx: &egui::Context) -> Keys {
    let mut k = Keys { close: false, step: 0, zoom: 1.0, toggle: false, space: false };
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
        k.space = i.consume_key(Modifiers::NONE, Key::Space);
    });
    k
}

fn text(s: impl Into<String>) -> RichText {
    RichText::new(s).color(Color32::WHITE)
}

/// What the middle of the window shows.
enum Body {
    /// a picture, or the preview of a video that cannot play (with the reason)
    Picture {
        url: String,
        note: Option<&'static str>,
    },
    Loading,
    Failed(String),
    /// a video that is here
    Player(std::path::PathBuf),
}

fn body_of(v: &Viewer, env: &Env) -> Body {
    let it = &v.items[v.index];
    let Some(video) = &it.video else { return Body::Picture { url: it.url.clone(), note: None } };
    if !v.can_play {
        return Body::Picture { url: video.preview.clone(), note: Some("ffmpeg is not installed, so this video cannot play here. Open it in the browser.") };
    }
    match video.source.as_ref().map(|s| env.clips.get(s)) {
        None => Body::Failed("This video has no file that Shale can play.".into()),
        Some(Clip::Loading) => Body::Loading,
        Some(Clip::Failed(e)) => Body::Failed(format!("Could not load this video: {e}.")),
        Some(Clip::Ready(path)) => Body::Player(path),
    }
}

/// Show the viewer over `rect` (the tab). `keys`: plain keys may be used (no dialog is open).
/// Returns true when it is to be closed.
pub fn show(ctx: &egui::Context, rect: Rect, v: &mut Viewer, env: &Env, rects: &mut Vec<(String, Rect)>, keys: bool) -> bool {
    let k = if keys { read_keys(ctx) } else { Keys { close: false, step: 0, zoom: 1.0, toggle: false, space: false } };
    let mut close = k.close;
    let mut step = k.step;
    let mut zoom_by = k.zoom;
    let mut toggle = k.toggle;
    let last = v.items.len() - 1;
    let alt = v.items[v.index].alt.clone();
    let alt_h = if alt.is_empty() { 0.0 } else { ALT_MAX };
    let body = body_of(v, env);
    let note_h = if matches!(body, Body::Picture { note: Some(_), .. }) { NOTE } else { 0.0 };
    let pic = Rect::from_min_max(rect.min + Vec2::new(0.0, BAR), rect.max - Vec2::new(0.0, alt_h + note_h)).shrink(8.0);
    // the neighbours' pictures are fetched ahead, so stepping does not wait (videos are not: they are big)
    for n in [v.index.wrapping_sub(1), v.index + 1] {
        if let Some(i) = v.items.get(n) {
            env.images.get(i.video.as_ref().map(|x| x.preview.as_str()).unwrap_or(&i.url));
        }
    }
    let is_picture = matches!(body, Body::Picture { note: None, .. });
    // a video starts when it is here (the person opened it), once for each visit
    if let (Body::Player(path), Some(video)) = (&body, &v.items[v.index].video) {
        if v.started != Some(v.index) {
            v.started = Some(v.index);
            env.media.play_outside(ctx, path, Path::new(OWNER), if video.gif { Start::Gif } else { Start::Video });
        }
        if k.space {
            env.media.toggle(ctx, path, Path::new(OWNER));
        }
    }
    egui::Area::new(egui::Id::unique(("social-viewer", v.tab))).order(egui::Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        // the backdrop takes the clicks and the wheel, so nothing reaches the list below
        ui.allocate_exact_size(rect.size(), Sense::click_and_drag());
        ui.painter().rect_filled(rect, 0.0, Color32::from_black_alpha(238));
        // ---- the bar
        let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), BAR)).shrink2(Vec2::new(10.0, 6.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            let r = ui.add_enabled(v.index > 0, egui::Button::new("◀")).on_hover_text("Previous (Left)");
            rects.push(("viewer:prev".into(), r.rect));
            step -= r.clicked() as i32;
            let r = ui.add_enabled(v.index < last, egui::Button::new("▶")).on_hover_text("Next (Right)");
            rects.push(("viewer:next".into(), r.rect));
            step += r.clicked() as i32;
            let r = ui.label(text(format!("{} / {}", v.index + 1, v.items.len())));
            rects.push(("viewer:count".into(), r.rect));
            ui.add_enabled_ui(is_picture, |ui| {
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
            });
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
        // ---- the picture, or the video
        match body {
            Body::Picture { url, note } => {
                let state = match env.images.get(&url) {
                    Some(path) => crate::media::picture(ctx, &path),
                    None if url.is_empty() || env.images.failed(&url) => Pic::Broken,
                    None => Pic::Loading,
                };
                match state {
                    Pic::Ready(t) => {
                        let size = t.size_vec2();
                        let fit = (pic.width() / size.x).min(pic.height() / size.y).clamp(MIN_SCALE, MAX_SCALE);
                        let area = ui.interact(pic, ui.scope_id().with("viewer-picture"), Sense::drag());
                        let wheel = if area.hovered() { ctx.input(|i| i.smooth_scroll_delta().y) } else { 0.0 };
                        if wheel != 0.0 && note.is_none() {
                            zoom_by *= (wheel * 0.004).exp();
                        }
                        if toggle {
                            v.zoom = if v.zoom.is_some() { None } else { Some(1.0) };
                            v.pan = Vec2::ZERO;
                        }
                        if zoom_by != 1.0 && note.is_none() {
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
                    Pic::Broken if note.is_some() => {}
                    Pic::Broken => message(ui, pic, "Could not load this picture.", "viewer:error", rects),
                    Pic::Loading => spinner(ui, ctx, pic, rects),
                }
                if let Some(n) = note {
                    let at = Rect::from_min_max(egui::pos2(rect.min.x + 16.0, pic.max.y), egui::pos2(rect.max.x - 16.0, pic.max.y + NOTE));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                        let r = ui.add(egui::Label::new(RichText::new(n).color(theme::palette().link_missing)).wrap());
                        rects.push(("viewer:no-ffmpeg".into(), r.rect));
                    });
                }
            }
            Body::Loading => spinner(ui, ctx, pic, rects),
            Body::Failed(e) => message(ui, pic, &e, "viewer:error", rects),
            Body::Player(path) => {
                let size = widget::video_card_fit(pic.size()).min(pic.size().max(Vec2::splat(1.0)));
                let card = Rect::from_center_size(pic.center(), size);
                let title = v.items[v.index].video.as_ref().map(|x| x.name.as_str()).filter(|n| !n.is_empty());
                widget::paint_titled(ui, card, env.media, &path, Card { owner: Path::new(OWNER), id: ui.scope_id().with(("viewer-video", v.index)), keyboard: Keyboard::Takes, title });
                rects.push(("viewer:video".into(), card));
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

/// A line of words in the middle of `pic`.
fn message(ui: &mut egui::Ui, pic: Rect, words: &str, key: &str, rects: &mut Vec<(String, Rect)>) {
    let at = Rect::from_center_size(pic.center(), Vec2::new(pic.width().min(420.0), 60.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::top_down(egui::Align::Center)), |ui| {
        let r = ui.add(egui::Label::new(RichText::new(words).color(theme::palette().link_missing)).wrap());
        rects.push((key.into(), r.rect));
    });
}

fn spinner(ui: &mut egui::Ui, ctx: &egui::Context, pic: Rect, rects: &mut Vec<(String, Rect)>) {
    let r = Rect::from_center_size(pic.center(), Vec2::splat(28.0));
    egui::Spinner::new().size(28.0).color(Color32::WHITE).paint_at(ui, r);
    rects.push(("viewer:spinner".into(), r));
    ctx.request_repaint_after(std::time::Duration::from_millis(100));
}
