//! A drawing as an SVG picture (SITE-037): the scene of an `.excalidraw`, `.excalidraw.md` or
//! `.canvas` file (a canvas is turned into a scene by `drawing::canvas`) is drawn with the same
//! code as the drawing view and the PDF export (`drawing::paint_element`, which makes egui shapes),
//! and the shapes are written as SVG elements. Text is written as SVG text with the font families
//! of the viewer, so nothing is loaded from another site. The links of a drawing are links of the
//! picture only where `href` finds a page for them (SITE-007): the text of a link to a note that is
//! not published stays text, and no address of it is written.

use crate::drawing::{self, Element, Scene, View};
use crate::drawing::{element_link, ElementLink};
use crate::markdown;
use crate::pdf::write::{ascent, text_width, FontKind};
use eframe::egui::{self, epaint::ColorMode, Color32, Pos2, Shape};
use std::collections::HashMap;

/// A picture of the drawing is at most this many pixels on a side (a bigger one is scaled down).
const PICTURE_MAX: u32 = 1600;

/// The size of the picture of a scene in pixels: the drawing and a margin of 20 around it.
pub fn size(scene: &Scene) -> Option<(u32, u32)> {
    let b = scene.bounds()?.expand(20.0);
    Some((b.width().ceil().max(1.0) as u32, b.height().ceil().max(1.0) as u32))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn num(v: f32) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// `fill="…"` style attributes of a colour: `none` when it is transparent.
fn paint(attr: &str, c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 0 {
        return format!(" {attr}=\"none\"");
    }
    let opacity = if a < 255 { format!(" {attr}-opacity=\"{}\"", num(a as f32 / 255.0)) } else { String::new() };
    format!(" {attr}=\"rgb({r},{g},{b})\"{opacity}")
}

fn stroke_attrs(color: Color32, width: f32) -> String {
    format!("{} stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"", paint("stroke", color), num(width))
}

fn points(pts: &[Pos2]) -> String {
    pts.iter().map(|p| format!("{},{}", num(p.x), num(p.y))).collect::<Vec<_>>().join(" ")
}

/// One egui shape as SVG; the kinds `paint_element` makes without a picture or a text.
fn shape(s: &Shape, out: &mut String) {
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| shape(s, out)),
        Shape::LineSegment { points, stroke } => {
            if stroke.color.a() > 0 {
                out.push_str(&format!("<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}/>\n", num(points[0].x), num(points[0].y), num(points[1].x), num(points[1].y), stroke_attrs(stroke.color, stroke.width)));
            }
        }
        Shape::Path(p) => {
            let stroke = match &p.stroke.color {
                ColorMode::Solid(c) if p.stroke.width > 0.0 && c.a() > 0 => stroke_attrs(*c, p.stroke.width),
                _ => " stroke=\"none\"".to_string(),
            };
            let tag = if p.closed { "polygon" } else { "polyline" };
            out.push_str(&format!("<{tag} points=\"{}\"{}{stroke}/>\n", points(&p.points), paint("fill", p.fill)));
        }
        Shape::Circle(c) => {
            let stroke = if c.stroke.width > 0.0 && c.stroke.color.a() > 0 { stroke_attrs(c.stroke.color, c.stroke.width) } else { " stroke=\"none\"".to_string() };
            out.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}{stroke}/>\n", num(c.center.x), num(c.center.y), num(c.radius), paint("fill", c.fill)));
        }
        Shape::Rect(r) => {
            let stroke = if r.stroke.width > 0.0 && r.stroke.color.a() > 0 { stroke_attrs(r.stroke.color, r.stroke.width) } else { " stroke=\"none\"".to_string() };
            out.push_str(&format!("<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{stroke}/>\n", num(r.rect.min.x), num(r.rect.min.y), num(r.rect.width()), num(r.rect.height()), paint("fill", r.fill)));
        }
        _ => {}
    }
}

/// The bytes of `data` as Base64 (the standard alphabet, padded).
pub(super) fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// A picture of the drawing as an `href` for `<image>`: a PNG in the page itself.
fn picture_href(img: &image::RgbaImage) -> Option<String> {
    let img = if img.width() > PICTURE_MAX || img.height() > PICTURE_MAX { image::DynamicImage::ImageRgba8(img.clone()).thumbnail(PICTURE_MAX, PICTURE_MAX).to_rgba8() } else { img.clone() };
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(format!("data:image/png;base64,{}", base64(png.get_ref())))
}

fn font(e: &Element) -> (FontKind, &'static str) {
    match drawing::font_family_of_pub(e) {
        2 | 6 => (FontKind::Sans, "system-ui, -apple-system, 'Segoe UI', sans-serif"),
        3 | 8 => (FontKind::Mono, "ui-monospace, 'Cascadia Code', Menlo, Consolas, monospace"),
        _ => (FontKind::Hand, "Virgil, 'Segoe Print', 'Comic Sans MS', 'Bradley Hand', cursive"),
    }
}

/// The text of an element as it is shown: a link in it is its shown text (SITE-007), whatever it
/// points to.
fn shown_text(text: &str) -> String {
    let mut out = text.to_string();
    for l in markdown::find_links(text).into_iter().rev() {
        out.replace_range(l.range.clone(), &l.display());
    }
    out
}

/// A text element as SVG: its lines (a label wraps in its shape as on screen), each at its place.
fn text_element(e: &Element, view: &View, out: &mut String) {
    let text = shown_text(e.text.as_deref().unwrap_or(""));
    if text.trim().is_empty() {
        return;
    }
    let (kind, family) = font(e);
    let size = e.font_size.unwrap_or(20.0) as f32 * view.zoom;
    let color = drawing::parse_color(&e.stroke_color).unwrap_or(Color32::BLACK);
    let color = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (e.opacity.clamp(0.0, 100.0) / 100.0 * 255.0) as u8);
    let left = view.to_screen(Pos2::new(e.x as f32, e.y as f32));
    let box_w = e.width as f32 * view.zoom;
    let wrap = drawing::is_label_pub(e).then_some(box_w + 2.0 * view.zoom);
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        match wrap {
            Some(max) => {
                let mut cur = String::new();
                for word in para.split_inclusive(' ') {
                    if !cur.is_empty() && text_width(kind, size, &(cur.clone() + word.trim_end())) > max {
                        lines.push(std::mem::take(&mut cur));
                    }
                    cur.push_str(word);
                }
                lines.push(cur);
            }
            None => lines.push(para.to_string()),
        }
    }
    let lh = size * 1.25;
    if let Some(bg) = drawing::parse_color(&e.background_color) {
        out.push_str(&format!(
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{} stroke=\"none\"/>\n",
            num(left.x - 4.0 * view.zoom),
            num(left.y - 4.0 * view.zoom),
            num(box_w + 8.0 * view.zoom),
            num(lh * lines.len() as f32 + 8.0 * view.zoom),
            paint("fill", bg)
        ));
    }
    let (x, anchor) = match drawing::text_align_pub(e) {
        1 => (left.x + box_w / 2.0, "middle"),
        2 => (left.x + box_w, "end"),
        _ => (left.x, "start"),
    };
    for (i, l) in lines.iter().enumerate() {
        let l = l.trim_end();
        if l.is_empty() {
            continue;
        }
        let base = left.y + i as f32 * lh + size * (0.25 * 0.5) + size * ascent(kind).min(1.0);
        out.push_str(&format!("<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" text-anchor=\"{anchor}\" xml:space=\"preserve\"{}>{}</text>\n", num(x), num(base), xml(family), num(size), paint("fill", color), xml(l)));
    }
}

/// The link of an element for the picture: that of its shape for a label (the shape carries it).
fn link_of(scene: &Scene, e: &Element) -> Option<ElementLink> {
    match e.extra.get("containerId").and_then(|v| v.as_str()) {
        Some(id) => scene.live().find(|c| c.id == id).and_then(|c| element_link(scene, c)),
        None => element_link(scene, e),
    }
}

/// `scene` as an SVG picture, or none when it has nothing to draw. `pictures` are the scene's
/// pictures by file id; `href` gives the address of a link of the drawing that the site may
/// write (a published note, a web address), and none for any other.
pub fn scene_svg(scene: &Scene, pictures: &HashMap<String, image::RgbaImage>, href: &dyn Fn(&ElementLink) -> Option<String>) -> Option<String> {
    let b = scene.bounds()?.expand(20.0);
    let (w, h) = size(scene)?;
    let view = View { origin: Pos2::ZERO, offset: -b.min.to_vec2(), zoom: 1.0 };
    let background = scene.app_state.get("viewBackgroundColor").and_then(|v| v.as_str()).and_then(drawing::parse_color).filter(|c| c.a() > 0).unwrap_or(Color32::WHITE);
    let mut out = format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" width=\"{w}\" height=\"{h}\" role=\"img\">\n<rect width=\"{w}\" height=\"{h}\"{} stroke=\"none\"/>\n", paint("fill", background));
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::EVERYTHING);
    let no_textures = drawing::pictures::Pictures::new();
    for e in scene.live() {
        let mut part = String::new();
        let picture = if e.kind == "image" { e.extra.get("fileId").and_then(|v| v.as_str()).and_then(|id| pictures.get(id)).and_then(picture_href) } else { None };
        if let Some(data) = picture {
            let r = e.bounds();
            let (p, q) = (view.to_screen(r.min), view.to_screen(r.max));
            part.push_str(&format!("<image href=\"{data}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"none\" opacity=\"{}\"/>\n", num(p.x), num(p.y), num(q.x - p.x), num(q.y - p.y), num(e.opacity as f32 / 100.0)));
        } else if e.kind == "text" {
            text_element(e, &view, &mut part);
        } else {
            let mut shapes = Vec::new();
            drawing::paint_element(&painter, e, &view, false, &no_textures, &mut shapes);
            shapes.iter().for_each(|s| shape(s, &mut part));
        }
        match link_of(scene, e).and_then(|l| href(&l)).filter(|_| !part.is_empty()) {
            // `_top`: the picture is shown in an `<object>`, and a link leaves it
            Some(url) => out.push_str(&format!("<a href=\"{}\" target=\"_top\">\n{part}</a>\n", xml(&url))),
            None => out.push_str(&part),
        }
    }
    out.push_str("</svg>\n");
    Some(out)
}
