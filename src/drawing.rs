//! Drawings stored in the open Excalidraw scene format (`.excalidraw` JSON files),
//! with a small hand-drawn style canvas editor.

use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, Vec2};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[path = "drawing_connect.rs"]
mod connect;
use connect::*;
#[path = "drawing_links.rs"]
mod links;
pub use links::{link_lines, rewrite_link_texts, DrawingAction};

/// Excalidraw font id of a text element (1 hand-drawn, 2 normal, 3 code).
pub fn font_family_of_pub(e: &Element) -> u64 {
    font_family_of(e)
}

/// Is this text a label inside a shape?
pub fn is_label_pub(e: &Element) -> bool {
    container_of(e).is_some()
}

/// Text alignment: 0 left, 1 centre, 2 right.
pub fn text_align_pub(e: &Element) -> u8 {
    match text_align_of(e) {
        egui::Align::Center => 1,
        egui::Align::Max => 2,
        _ => 0,
    }
}

pub const SAMPLE_SCENE: &str = r##"{
  "type": "excalidraw",
  "version": 2,
  "source": "shale",
  "elements": [
    {"id":"s1","type":"rectangle","x":40,"y":40,"width":220,"height":110,"angle":0,"strokeColor":"#1971c2","backgroundColor":"#a5d8ff","fillStyle":"hachure","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"seed":11,"version":1,"versionNonce":1,"isDeleted":false,"groupIds":[],"frameId":null,"roundness":{"type":3},"boundElements":[{"id":"s2","type":"text"},{"id":"s3","type":"arrow"}],"updated":1,"link":null,"locked":false},
    {"id":"s2","type":"text","x":78,"y":80,"width":150,"height":30,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":1,"strokeStyle":"solid","roughness":1,"opacity":100,"seed":12,"version":1,"versionNonce":1,"isDeleted":false,"groupIds":[],"frameId":null,"roundness":null,"boundElements":null,"updated":1,"link":null,"locked":false,"text":"Idea","fontSize":24,"fontFamily":1,"textAlign":"center","verticalAlign":"middle","containerId":"s1","originalText":"Idea","lineHeight":1.25,"autoResize":true},
    {"id":"s3","type":"arrow","x":270,"y":95,"width":140,"height":0,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"seed":13,"version":1,"versionNonce":1,"isDeleted":false,"groupIds":[],"frameId":null,"roundness":{"type":2},"boundElements":null,"updated":1,"link":null,"locked":false,"points":[[0,0],[140,0]],"lastCommittedPoint":null,"startBinding":{"elementId":"s1","focus":0,"gap":0,"fixedPoint":[1,0.5]},"endBinding":{"elementId":"s4","focus":0,"gap":0,"fixedPoint":[0,0.5]},"startArrowhead":null,"endArrowhead":"triangle"},
    {"id":"s4","type":"ellipse","x":420,"y":40,"width":170,"height":110,"angle":0,"strokeColor":"#2f9e44","backgroundColor":"#b2f2bb","fillStyle":"cross-hatch","strokeWidth":2,"strokeStyle":"solid","roughness":1,"opacity":100,"seed":14,"version":1,"versionNonce":1,"isDeleted":false,"groupIds":[],"frameId":null,"roundness":{"type":2},"boundElements":[{"id":"s5","type":"text"},{"id":"s3","type":"arrow"}],"updated":1,"link":null,"locked":false},
    {"id":"s5","type":"text","x":462,"y":80,"width":90,"height":30,"angle":0,"strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":1,"strokeStyle":"solid","roughness":1,"opacity":100,"seed":15,"version":1,"versionNonce":1,"isDeleted":false,"groupIds":[],"frameId":null,"roundness":null,"boundElements":null,"updated":1,"link":null,"locked":false,"text":"Note","fontSize":24,"fontFamily":1,"textAlign":"center","verticalAlign":"middle","containerId":"s4","originalText":"Note","lineHeight":1.25,"autoResize":true}
  ],
  "appState": {"viewBackgroundColor": "#ffffff", "gridSize": null},
  "files": {}
}"##;

fn d_type() -> String { "excalidraw".into() }
fn d_two() -> u32 { 2 }
fn d_black() -> String { "#1e1e1e".into() }
fn d_transparent() -> String { "transparent".into() }
fn d_hachure() -> String { "hachure".into() }
fn d_solid() -> String { "solid".into() }
fn d_one() -> f64 { 1.0 }
fn d_hundred() -> f64 { 100.0 }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Scene {
    #[serde(rename = "type", default = "d_type")]
    pub kind: String,
    #[serde(default = "d_two")]
    pub version: u32,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub elements: Vec<Element>,
    #[serde(rename = "appState", default)]
    pub app_state: Map<String, Value>,
    #[serde(default)]
    pub files: Map<String, Value>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
    /// Pictures of an Obsidian plugin drawing: (fileId, link target) from its `## Embedded Files`
    /// section. Not part of the scene JSON.
    #[serde(skip)]
    pub embedded: Vec<(String, String)>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Element {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub width: f64,
    #[serde(default)]
    pub height: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default = "d_black")]
    pub stroke_color: String,
    #[serde(default = "d_transparent")]
    pub background_color: String,
    #[serde(default = "d_hachure")]
    pub fill_style: String,
    #[serde(default = "d_one")]
    pub stroke_width: f64,
    #[serde(default = "d_solid")]
    pub stroke_style: String,
    #[serde(default = "d_one")]
    pub roughness: f64,
    #[serde(default = "d_hundred")]
    pub opacity: f64,
    #[serde(default)]
    pub seed: u64,
    #[serde(default)]
    pub version: u64,
    #[serde(default)]
    pub version_nonce: u64,
    #[serde(default)]
    pub is_deleted: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub points: Vec<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    /// Every other field (bindings, groups, arrowheads, ...) is kept as-is.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Scene {
    fn default() -> Self {
        let mut app_state = Map::new();
        app_state.insert("viewBackgroundColor".into(), json!("#ffffff"));
        app_state.insert("gridSize".into(), Value::Null);
        Scene { kind: d_type(), version: 2, source: "shale".into(), elements: vec![], app_state, files: Map::new(), extra: Map::new(), embedded: vec![] }
    }
}

impl Scene {
    pub fn parse(s: &str) -> Result<Scene, String> {
        if s.trim().is_empty() {
            return Ok(Scene::default());
        }
        serde_json::from_str(s).map_err(|e| e.to_string())
    }
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }
    /// The scene of a drawing file's text: `.excalidraw` JSON, a `.excalidraw.md` file of the
    /// Obsidian Excalidraw plugin, or an Obsidian `.canvas`.
    pub fn from_file_text(path: &Path, text: &str) -> Result<Scene, String> {
        if crate::excalidraw_md::is_plugin_file(path) {
            crate::excalidraw_md::read(text)
        } else if crate::canvas::is_canvas_path(path) {
            crate::canvas::to_scene(text)
        } else {
            Scene::parse(text)
        }
    }
    /// Read a drawing file (any of the two formats).
    pub fn read_file(abs: &Path) -> Result<Scene, String> {
        let text = std::fs::read_to_string(abs).map_err(|e| e.to_string())?;
        Scene::from_file_text(abs, &text)
    }
    pub fn live(&self) -> impl Iterator<Item = &Element> {
        self.elements.iter().filter(|e| !e.is_deleted)
    }
    /// World bounds of all visible elements.
    pub fn bounds(&self) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        for e in self.live() {
            let b = e.bounds();
            r = Some(match r {
                Some(x) => x.union(b),
                None => b,
            });
        }
        r
    }
}

impl Element {
    pub fn is_linear(&self) -> bool {
        matches!(self.kind.as_str(), "line" | "arrow" | "freedraw")
    }
    pub fn bounds(&self) -> Rect {
        if self.is_linear() && !self.points.is_empty() {
            let mut r = Rect::NOTHING;
            for p in &self.points {
                r.extend_with(Pos2::new((self.x + p[0]) as f32, (self.y + p[1]) as f32));
            }
            return r;
        }
        Rect::from_min_size(Pos2::new(self.x as f32, self.y as f32), Vec2::new(self.width as f32, self.height as f32))
    }
    fn arrowhead(&self, key: &str) -> Option<String> {
        self.extra.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
    }
    fn has_roundness(&self) -> bool {
        self.extra.get("roundness").map(|v| !v.is_null()).unwrap_or(false)
    }
    fn touch(&mut self, rng: &mut Rng) {
        self.version += 1;
        self.version_nonce = rng.next() % 2_000_000_000;
        self.extra.insert("updated".into(), json!(now_ms()));
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Tiny deterministic random generator (xorshift).
#[derive(Clone)]
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1)
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// -1.0 ..= 1.0
    pub fn f(&mut self) -> f32 {
        (self.next() % 20001) as f32 / 10000.0 - 1.0
    }
}

pub fn new_id(rng: &mut Rng) -> String {
    const A: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_-";
    (0..20).map(|_| A[(rng.next() % A.len() as u64) as usize] as char).collect()
}

// ---------------------------------------------------------------------------------------------
// Colours

pub fn parse_color(s: &str) -> Option<Color32> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("transparent") || s.is_empty() {
        return None;
    }
    if let Some(h) = s.strip_prefix('#') {
        let v = |i: usize, n: usize| h.get(i..i + n).and_then(|x| u8::from_str_radix(x, 16).ok());
        return match h.len() {
            3 => Some(Color32::from_rgb(v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17)),
            6 => Some(Color32::from_rgb(v(0, 2)?, v(2, 2)?, v(4, 2)?)),
            8 => Some(Color32::from_rgba_unmultiplied(v(0, 2)?, v(2, 2)?, v(4, 2)?, v(6, 2)?)),
            _ => None,
        };
    }
    Some(match s.to_ascii_lowercase().as_str() {
        "black" => Color32::BLACK,
        "white" => Color32::WHITE,
        "red" => Color32::RED,
        "green" => Color32::GREEN,
        "blue" => Color32::BLUE,
        "yellow" => Color32::YELLOW,
        _ => Color32::GRAY,
    })
}

/// Dark canvas: invert lightness but keep the hue (like a dark mode filter).
pub fn dark_variant(c: Color32) -> Color32 {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let (r, g, b) = (r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let target_l = 1.0 - l * 0.93;
    let shift = target_l - l;
    let f = |x: f32| ((x + shift).clamp(0.0, 1.0) * 255.0) as u8;
    Color32::from_rgba_unmultiplied(f(r), f(g), f(b), a)
}

pub const STROKE_COLORS: [&str; 6] = ["#1e1e1e", "#e03131", "#2f9e44", "#1971c2", "#f08c00", "#9c36b5"];
pub const BG_COLORS: [&str; 6] = ["transparent", "#ffc9c9", "#b2f2bb", "#a5d8ff", "#ffec99", "#eebefa"];
const ACCENT: Color32 = Color32::from_rgb(105, 101, 219);

// ---------------------------------------------------------------------------------------------
// Rendering

/// World → screen transform.
#[derive(Clone, Copy)]
pub struct View {
    pub origin: Pos2,
    pub offset: Vec2,
    pub zoom: f32,
}
impl View {
    pub fn to_screen(&self, p: Pos2) -> Pos2 {
        self.origin + (p.to_vec2() + self.offset) * self.zoom
    }
    pub fn to_world(&self, p: Pos2) -> Pos2 {
        ((p - self.origin) / self.zoom - self.offset).to_pos2()
    }
}

fn rotate_about(p: Pos2, c: Pos2, angle: f32) -> Pos2 {
    if angle == 0.0 {
        return p;
    }
    let (s, co) = angle.sin_cos();
    let d = p - c;
    c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
}

/// Outline of a closed shape in world coordinates.
fn outline(e: &Element) -> Vec<Pos2> {
    let x = e.x as f32;
    let y = e.y as f32;
    let w = e.width as f32;
    let h = e.height as f32;
    let c = Pos2::new(x + w / 2.0, y + h / 2.0);
    let pts: Vec<Pos2> = match e.kind.as_str() {
        "rectangle" => {
            if e.has_roundness() {
                let r = (w.abs().min(h.abs()) * 0.25).min(32.0);
                let mut v = Vec::new();
                let corners = [(x + w - r, y + r, -90.0f32), (x + w - r, y + h - r, 0.0), (x + r, y + h - r, 90.0), (x + r, y + r, 180.0)];
                for (cx, cy, start) in corners {
                    for i in 0..=6 {
                        let a = (start + i as f32 * 15.0).to_radians();
                        v.push(Pos2::new(cx + r * a.cos(), cy + r * a.sin()));
                    }
                }
                v
            } else {
                vec![Pos2::new(x, y), Pos2::new(x + w, y), Pos2::new(x + w, y + h), Pos2::new(x, y + h)]
            }
        }
        "diamond" => vec![Pos2::new(c.x, y), Pos2::new(x + w, c.y), Pos2::new(c.x, y + h), Pos2::new(x, c.y)],
        "ellipse" => (0..48)
            .map(|i| {
                let a = i as f32 / 48.0 * std::f32::consts::TAU;
                Pos2::new(c.x + w / 2.0 * a.cos(), c.y + h / 2.0 * a.sin())
            })
            .collect(),
        _ => vec![],
    };
    pts.into_iter().map(|p| rotate_about(p, c, e.angle as f32)).collect()
}

fn linear_points(e: &Element) -> Vec<Pos2> {
    let c = e.bounds().center();
    e.points.iter().map(|p| rotate_about(Pos2::new((e.x + p[0]) as f32, (e.y + p[1]) as f32), c, e.angle as f32)).collect()
}

/// Hand-drawn wobble: subdivide and jitter points (deterministic per seed).
fn roughen(pts: &[Pos2], closed: bool, amount: f32, rng: &mut Rng) -> Vec<Pos2> {
    if amount <= 0.0 || pts.len() < 2 {
        let mut v = pts.to_vec();
        if closed && !v.is_empty() {
            v.push(v[0]);
        }
        return v;
    }
    let mut out = Vec::new();
    let n = pts.len();
    let segs = if closed { n } else { n - 1 };
    for i in 0..segs {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let len = (b - a).length();
        let steps = ((len / 40.0).ceil() as usize).clamp(1, 12);
        let normal = if len > 0.0 { Vec2::new(-(b - a).y, (b - a).x) / len } else { Vec2::ZERO };
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let jitter = if s == 0 { 0.6 } else { 1.0 };
            let p = a + (b - a) * t + normal * rng.f() * amount * jitter + Vec2::new(rng.f(), rng.f()) * amount * 0.3;
            out.push(p);
        }
    }
    let last = if closed { pts[0] } else { pts[n - 1] };
    out.push(last + Vec2::new(rng.f(), rng.f()) * amount * 0.5);
    out
}

/// Parallel hatch lines inside a polygon (world coords), at `angle` radians, `gap` apart.
fn hatch_lines(poly: &[Pos2], gap: f32, angle: f32) -> Vec<[Pos2; 2]> {
    if poly.len() < 3 || gap <= 0.5 {
        return vec![];
    }
    let (s, c) = angle.sin_cos();
    let rot: Vec<Pos2> = poly.iter().map(|p| Pos2::new(p.x * c + p.y * s, -p.x * s + p.y * c)).collect();
    let min_y = rot.iter().map(|p| p.y).fold(f32::MAX, f32::min);
    let max_y = rot.iter().map(|p| p.y).fold(f32::MIN, f32::max);
    let mut out = Vec::new();
    let mut y = min_y + gap / 2.0;
    let mut guard = 0;
    while y < max_y && guard < 2000 {
        guard += 1;
        let mut xs = Vec::new();
        for i in 0..rot.len() {
            let a = rot[i];
            let b = rot[(i + 1) % rot.len()];
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                let t = (y - a.y) / (b.y - a.y);
                xs.push(a.x + (b.x - a.x) * t);
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks(2) {
            if pair.len() == 2 {
                let un = |x: f32| Pos2::new(x * c - y * s, x * s + y * c);
                out.push([un(pair[0]), un(pair[1])]);
            }
        }
        y += gap;
    }
    out
}

fn with_opacity(c: Color32, opacity: f64) -> Color32 {
    c.gamma_multiply((opacity / 100.0).clamp(0.0, 1.0) as f32)
}

fn stroke_shapes(points: Vec<Pos2>, stroke: Stroke, style: &str, out: &mut Vec<Shape>) {
    match style {
        "dashed" => out.extend(Shape::dashed_line(&points, stroke, stroke.width * 4.0 + 4.0, stroke.width * 3.0 + 3.0)),
        "dotted" => out.extend(Shape::dotted_line(&points, stroke.color, stroke.width * 2.5 + 3.0, stroke.width * 0.6 + 0.5)),
        "sparsedots" => out.extend(Shape::dotted_line(&points, stroke.color, stroke.width * 5.0 + 8.0, stroke.width * 0.7 + 0.6)),
        "longdash" => out.extend(Shape::dashed_line(&points, stroke, stroke.width * 8.0 + 14.0, stroke.width * 3.0 + 6.0)),
        "dashdot" => out.extend(Shape::dashed_line_with_offset(&points, stroke, &[stroke.width * 5.0 + 10.0, stroke.width * 0.6 + 0.8], &[stroke.width * 2.0 + 4.0, stroke.width * 2.0 + 4.0], 0.0)),
        _ => out.push(Shape::line(points, stroke)),
    }
}

/// The dotted grid behind a drawing.
fn paint_grid(painter: &egui::Painter, rect: Rect, view: &View, dark: bool) {
    let grid = 20.0 * view.zoom;
    if grid > 7.0 {
        let dot = if dark { Color32::from_gray(42) } else { Color32::from_gray(222) };
        let start = view.to_screen(Pos2::ZERO);
        let ox = (start.x - rect.min.x).rem_euclid(grid);
        let oy = (start.y - rect.min.y).rem_euclid(grid);
        let mut y = rect.min.y + oy;
        while y < rect.max.y {
            let mut x = rect.min.x + ox;
            while x < rect.max.x {
                painter.circle_filled(Pos2::new(x, y), 0.9, dot);
                x += grid;
            }
            y += grid;
        }
    }
}

/// Paint all elements. `dark` turns colours into their dark-canvas variants.
pub fn paint_scene(painter: &egui::Painter, scene: &Scene, view: &View, dark: bool, skip_id: Option<&str>, pics: &crate::drawing_pictures::Pictures) {
    let mut shapes = Vec::new();
    for e in scene.live() {
        if Some(e.id.as_str()) == skip_id {
            continue;
        }
        paint_element(painter, e, view, dark, pics, &mut shapes);
    }
    painter.extend(shapes);
}

pub(crate) fn paint_element(painter: &egui::Painter, e: &Element, view: &View, dark: bool, pics: &crate::drawing_pictures::Pictures, out: &mut Vec<Shape>) {
    let col = |s: &str| parse_color(s).map(|c| if dark { dark_variant(c) } else { c }).map(|c| with_opacity(c, e.opacity));
    let stroke_col = col(&e.stroke_color).unwrap_or(Color32::TRANSPARENT);
    let width = (e.stroke_width as f32 * view.zoom).max(0.6);
    let stroke = Stroke::new(width, stroke_col);
    let mut rng = Rng::new(e.seed.wrapping_add(7));
    let rough = e.roughness as f32 * 1.4;
    let to_s = |pts: Vec<Pos2>| pts.into_iter().map(|p| view.to_screen(p)).collect::<Vec<_>>();
    match e.kind.as_str() {
        "rectangle" | "diamond" | "ellipse" => {
            let poly = outline(e);
            if let Some(bg) = col(&e.background_color) {
                match e.fill_style.as_str() {
                    "solid" => out.push(Shape::convex_polygon(to_s(poly.clone()), bg, Stroke::NONE)),
                    style => {
                        let gap = (e.stroke_width as f32 * 4.0).max(6.0) + 2.0;
                        let hs = Stroke::new((e.stroke_width as f32 * 0.6 * view.zoom).max(0.6), bg);
                        let mut lines = hatch_lines(&poly, gap, -0.715);
                        if style == "cross-hatch" {
                            lines.extend(hatch_lines(&poly, gap, 0.855));
                        }
                        for l in lines {
                            let pts = roughen(&l, false, rough * 0.4, &mut rng);
                            out.push(Shape::line(to_s(pts), hs));
                        }
                    }
                }
            }
            let passes = if rough > 0.0 { 2 } else { 1 };
            for _ in 0..passes {
                let pts = roughen(&poly, true, rough, &mut rng);
                stroke_shapes(to_s(pts), stroke, &e.stroke_style, out);
            }
        }
        "line" | "arrow" => {
            let pts = linear_points(e);
            if pts.len() < 2 {
                return;
            }
            let closed_fill = e.kind == "line" && pts.len() > 3 && (pts[0] - pts[pts.len() - 1]).length() < 1.0;
            if closed_fill {
                if let Some(bg) = col(&e.background_color) {
                    out.push(Shape::convex_polygon(to_s(pts.clone()), bg, Stroke::NONE));
                }
            }
            let passes = if rough > 0.0 { 2 } else { 1 };
            for _ in 0..passes {
                let r = roughen(&pts, false, rough * 0.8, &mut rng);
                stroke_shapes(to_s(r), stroke, &e.stroke_style, out);
            }
            let head = |tip: Pos2, from: Pos2, out: &mut Vec<Shape>| {
                let d = tip - from;
                let len = d.length();
                if len < 0.1 {
                    return;
                }
                let size = (len * 0.4).min(24.0 + e.stroke_width as f32 * 2.0);
                let dir = d / len;
                for a in [0.45f32, -0.45] {
                    let (s, c) = a.sin_cos();
                    let back = Vec2::new(-dir.x * c + dir.y * s, -dir.x * s - dir.y * c) * size;
                    out.push(Shape::line_segment([view.to_screen(tip), view.to_screen(tip + back)], stroke));
                }
            };
            let _ = &head;
            let solid = Stroke::new(width, stroke_col);
            if let Some(k) = e.arrowhead("endArrowhead") {
                paint_head(&k, pts[pts.len() - 1], pts[pts.len() - 2], e.stroke_width as f32, solid, view, out);
            }
            if let Some(k) = e.arrowhead("startArrowhead") {
                paint_head(&k, pts[0], pts[1], e.stroke_width as f32, solid, view, out);
            }
        }
        "freedraw" => {
            let pts = linear_points(e);
            if pts.len() == 1 {
                out.push(Shape::circle_filled(view.to_screen(pts[0]), width * 1.2, stroke_col));
            } else if pts.len() > 1 {
                out.push(Shape::line(to_s(pts), Stroke::new(width * 1.8, stroke_col)));
            }
        }
        "text" => {
            let label = container_of(e).is_some();
            let wrap = if label { Some((e.width as f32 + 2.0) * view.zoom) } else { None };
            let job = text_job(e, view.zoom, stroke_col, wrap);
            let galley = painter.fonts_mut(|f| f.layout_job(job));
            let left = view.to_screen(Pos2::new(e.x as f32, e.y as f32));
            let w = e.width as f32 * view.zoom;
            let anchor = match text_align_of(e) {
                egui::Align::Center => left + Vec2::new(w / 2.0, 0.0),
                egui::Align::Max => left + Vec2::new(w, 0.0),
                _ => left,
            };
            if let Some(bg) = col(&e.background_color) {
                let r = galley.rect.translate(anchor.to_vec2()).expand(4.0 * view.zoom);
                out.push(Shape::rect_filled(r, CornerRadius::same(4), bg));
            }
            out.push(Shape::galley(anchor, galley, stroke_col));
        }
        _ => {
            let b = e.bounds();
            let r = Rect::from_min_max(view.to_screen(b.min), view.to_screen(b.max));
            let picture = if e.kind == "image" { e.extra.get("fileId").and_then(|v| v.as_str()).and_then(|id| pics.get(id)) } else { None };
            if let Some(tex) = picture {
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                out.push(Shape::image(*tex, r, uv, with_opacity(Color32::WHITE, e.opacity)));
                return;
            }
            // Unknown element types (frames, pictures that can't be shown, ...): a dashed box.
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            out.extend(Shape::dashed_line(&pts, Stroke::new(1.0, Color32::GRAY), 6.0, 4.0));
        }
    }
}

/// Paint a whole scene fitted into `rect` (used for embeds in notes).
pub fn paint_thumbnail(painter: &egui::Painter, scene: &Scene, rect: Rect, dark: bool, pics: &crate::drawing_pictures::Pictures) {
    let Some(b) = scene.bounds() else {
        painter.text(rect.center(), Align2::CENTER_CENTER, "Empty drawing", FontId::proportional(13.0), Color32::GRAY);
        return;
    };
    let b = b.expand(20.0);
    let zoom = (rect.width() / b.width()).min(rect.height() / b.height()).min(1.5);
    let content = b.size() * zoom;
    let origin = rect.center() - content / 2.0;
    let view = View { origin, offset: -b.min.to_vec2(), zoom };
    paint_scene(&painter.with_clip_rect(rect), scene, &view, dark, None, pics);
}

// ---------------------------------------------------------------------------------------------
// Hit testing

fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
    (a + ab * t - p).length()
}

fn point_in_poly(p: Pos2, poly: &[Pos2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn hit(e: &Element, p: Pos2, tol: f32) -> bool {
    match e.kind.as_str() {
        "rectangle" | "diamond" | "ellipse" => {
            let poly = outline(e);
            point_in_poly(p, &poly) || (0..poly.len()).any(|i| dist_to_segment(p, poly[i], poly[(i + 1) % poly.len()]) < tol)
        }
        "line" | "arrow" | "freedraw" => {
            let pts = linear_points(e);
            if pts.len() == 1 {
                return (pts[0] - p).length() < tol;
            }
            pts.windows(2).any(|w| dist_to_segment(p, w[0], w[1]) < tol + e.stroke_width as f32)
        }
        _ => e.bounds().expand(tol * 0.5).contains(p),
    }
}

// ---------------------------------------------------------------------------------------------
// Editor

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    Hand,
    Rectangle,
    Diamond,
    Ellipse,
    Arrow,
    Line,
    Draw,
    Text,
    Eraser,
}

impl Tool {
    pub const ALL: [Tool; 10] = [Tool::Hand, Tool::Select, Tool::Rectangle, Tool::Diamond, Tool::Ellipse, Tool::Arrow, Tool::Line, Tool::Draw, Tool::Text, Tool::Eraser];
    fn name(self) -> &'static str {
        match self {
            Tool::Hand => "Hand (move the drawing) — 1 or H, or hold Space",
            Tool::Select => "Selection — 2 or V",
            Tool::Rectangle => "Rectangle — 3 or R",
            Tool::Diamond => "Diamond — 4 or D",
            Tool::Ellipse => "Ellipse — 5 or O",
            Tool::Arrow => "Arrow — 6 or A (start or end on a shape to connect it)",
            Tool::Line => "Line — 7 or L (start or end on a shape to connect it)",
            Tool::Draw => "Draw — 8 or P",
            Tool::Text => "Text — 9 or T",
            Tool::Eraser => "Eraser — 0 or E",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Tool::Hand => "1",
            Tool::Select => "2",
            Tool::Rectangle => "3",
            Tool::Diamond => "4",
            Tool::Ellipse => "5",
            Tool::Arrow => "6",
            Tool::Line => "7",
            Tool::Draw => "8",
            Tool::Text => "9",
            Tool::Eraser => "0",
        }
    }
    fn element_kind(self) -> &'static str {
        match self {
            Tool::Rectangle => "rectangle",
            Tool::Diamond => "diamond",
            Tool::Ellipse => "ellipse",
            Tool::Arrow => "arrow",
            Tool::Line => "line",
            _ => "freedraw",
        }
    }
}

#[derive(Clone, PartialEq)]
pub struct Style {
    pub stroke: String,
    pub background: String,
    pub fill: String,
    pub width: f64,
    pub stroke_style: String,
    pub roughness: f64,
    pub opacity: f64,
    pub font_size: f64,
    /// Excalidraw font id: 1 hand-drawn, 2 normal, 3 code
    pub font_family: u64,
    pub text_color: String,
    pub label_bg: String,
    pub start_head: Option<String>,
    pub end_head: Option<String>,
    pub round: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            stroke: "#1e1e1e".into(),
            background: "transparent".into(),
            fill: "hachure".into(),
            width: 2.0,
            stroke_style: "solid".into(),
            roughness: 1.0,
            opacity: 100.0,
            font_size: 20.0,
            font_family: 1,
            text_color: "#1e1e1e".into(),
            label_bg: "transparent".into(),
            start_head: None,
            end_head: Some("arrow".into()),
            round: true,
        }
    }
}

enum Drag {
    None,
    Create { id: String, start: Pos2, bind_start: Option<(String, [f32; 2])>, bind_end: Option<(String, [f32; 2])> },
    /// dragging one end of the selected line/arrow
    Endpoint { id: String, start: bool, target: Option<(String, [f32; 2])> },
    Move { last: Pos2 },
    Resize { id: String, anchor: Pos2, orig: Box<Element> },
    Marquee { start: Pos2 },
    Pan,
    Erase,
}

pub struct DrawingDoc {
    pub rel: PathBuf,
    pub scene: Scene,
    pub error: Option<String>,
    pub dirty: bool,
    pub last_edit: Instant,
    undo: Vec<Vec<Element>>,
    redo: Vec<Vec<Element>>,
    pub offset: Vec2,
    pub zoom: f32,
    pub tool: Tool,
    pub tool_locked: bool,
    pub selection: Vec<String>,
    pub style: Style,
    drag: Drag,
    editing_text: Option<String>,
    text_buf: String,
    marquee: Option<Rect>,
    rng: Rng,
    centered: bool,
    /// result of the last arrange / spread
    layout_msg: Option<String>,
    remeasure_free_text: bool,
    /// shape anchor a connection end would snap to right now (for highlighting)
    snap_hint: Option<(String, Pos2)>,
    /// where the style panel was drawn last frame (clicks there don't reach the canvas)
    props_rect: Option<Rect>,
    /// screen rect of every button / control drawn this frame, by name (used by the UI tests)
    pub ui_rects: Vec<(String, Rect)>,
    /// for an Obsidian `.excalidraw.md` drawing: the file as read (its layout is kept on save)
    plugin_file: Option<String>,
    /// the first frame (which tidies bindings without counting as an edit) is done
    settled: bool,
    /// shown but never edited or written (an Obsidian canvas); the reason is shown on screen
    pub read_only: Option<String>,
    /// `[[`, `[[p ` and `[!` helpers while a text is edited
    assist: crate::link_assist::LinkAssist,
}

impl DrawingDoc {
    /// `text` is the file's content (`.excalidraw` JSON or an Obsidian `.excalidraw.md` file).
    pub fn new(rel: PathBuf, text: &str) -> DrawingDoc {
        let (scene, error) = match Scene::from_file_text(&rel, text) {
            Ok(s) => (s, None),
            Err(e) => (Scene::default(), Some(format!("Could not read this drawing: {e}"))),
        };
        let plugin_file = crate::excalidraw_md::is_plugin_file(&rel).then(|| text.to_string());
        let read_only = crate::canvas::is_canvas_path(&rel).then(|| "Obsidian canvas · read-only".to_string());
        DrawingDoc {
            read_only,
            assist: crate::link_assist::LinkAssist::new(egui::Id::unique("draw-text")),
            plugin_file,
            settled: false,
            rel,
            scene,
            error,
            dirty: false,
            last_edit: Instant::now(),
            undo: vec![],
            redo: vec![],
            offset: Vec2::ZERO,
            zoom: 1.0,
            tool: Tool::Select,
            tool_locked: false,
            selection: vec![],
            style: Style::default(),
            drag: Drag::None,
            editing_text: None,
            text_buf: String::new(),
            marquee: None,
            rng: Rng::new(now_ms()),
            centered: false,
            layout_msg: None,
            remeasure_free_text: false,
            snap_hint: None,
            props_rect: None,
            ui_rects: Vec::new(),
        }
    }

    fn snapshot(&mut self) {
        self.undo.push(self.scene.elements.clone());
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// A file that could not be read: shown with the reason, never written.
    pub fn unreadable(rel: PathBuf, why: &str) -> DrawingDoc {
        let mut d = DrawingDoc::new(rel, "");
        d.error = Some(format!("Could not read this drawing: {why}"));
        d
    }

    /// What to write to the file: JSON, or the Obsidian plugin's markdown layout.
    pub fn file_text(&self) -> Result<String, String> {
        if self.error.is_some() {
            return Err("the drawing could not be read, so it is not written".into());
        }
        if self.read_only.is_some() {
            return Err("an Obsidian canvas is shown read-only and never written".into());
        }
        match &self.plugin_file {
            Some(original) => crate::excalidraw_md::write(original, &self.scene),
            None => Ok(self.scene.to_json()),
        }
    }

    fn changed(&mut self) {
        self.dirty = true;
        self.last_edit = Instant::now();
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.scene.elements, prev));
            self.selection.clear();
            self.changed();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.scene.elements, next));
            self.selection.clear();
            self.changed();
        }
    }

    fn touch_ids(&mut self, ids: &[String]) {
        let mut rng = self.rng.clone();
        for e in self.scene.elements.iter_mut().filter(|e| ids.contains(&e.id)) {
            e.touch(&mut rng);
        }
        self.rng = rng;
    }

    pub fn new_element(&mut self, kind: &str, p: Pos2) -> Element {
        let mut extra = Map::new();
        extra.insert("groupIds".into(), json!([]));
        extra.insert("frameId".into(), Value::Null);
        extra.insert(
            "roundness".into(),
            match kind {
                "rectangle" | "diamond" if self.style.round => json!({"type": 3}),
                "rectangle" | "diamond" => Value::Null,
                "arrow" | "line" | "ellipse" => json!({"type": 2}),
                _ => Value::Null,
            },
        );
        extra.insert("boundElements".into(), Value::Null);
        extra.insert("updated".into(), json!(now_ms()));
        extra.insert("link".into(), Value::Null);
        extra.insert("locked".into(), json!(false));
        let s = self.style.clone();
        let mut e = Element {
            id: new_id(&mut self.rng),
            kind: kind.into(),
            x: p.x as f64,
            y: p.y as f64,
            width: 0.0,
            height: 0.0,
            angle: 0.0,
            stroke_color: s.stroke.clone(),
            background_color: if matches!(kind, "rectangle" | "diamond" | "ellipse") { s.background.clone() } else { "transparent".into() },
            fill_style: s.fill.clone(),
            stroke_width: s.width,
            stroke_style: s.stroke_style.clone(),
            roughness: s.roughness,
            opacity: s.opacity,
            seed: self.rng.next() % 2_000_000_000,
            version: 1,
            version_nonce: self.rng.next() % 2_000_000_000,
            is_deleted: false,
            points: vec![],
            text: None,
            font_size: None,
            extra,
        };
        match kind {
            "line" | "arrow" => {
                e.points = vec![[0.0, 0.0], [0.0, 0.0]];
                e.extra.insert("lastCommittedPoint".into(), Value::Null);
                e.extra.insert("startBinding".into(), Value::Null);
                e.extra.insert("endBinding".into(), Value::Null);
                e.extra.insert("startArrowhead".into(), s.start_head.clone().filter(|_| kind == "arrow").map(Value::from).unwrap_or(Value::Null));
                e.extra.insert("endArrowhead".into(), if kind == "arrow" { s.end_head.clone().map(Value::from).unwrap_or(Value::Null) } else { Value::Null });
            }
            "freedraw" => {
                e.points = vec![[0.0, 0.0]];
                e.extra.insert("pressures".into(), json!([]));
                e.extra.insert("simulatePressure".into(), json!(true));
                e.extra.insert("lastCommittedPoint".into(), Value::Null);
            }
            "text" => {
                e.text = Some(String::new());
                e.font_size = Some(s.font_size);
                e.stroke_color = s.text_color.clone();
                e.extra.insert("fontFamily".into(), json!(s.font_family));
                e.extra.insert("textAlign".into(), json!("left"));
                e.extra.insert("verticalAlign".into(), json!("top"));
                e.extra.insert("containerId".into(), Value::Null);
                e.extra.insert("originalText".into(), json!(""));
                e.extra.insert("lineHeight".into(), json!(1.25));
                e.extra.insert("autoResize".into(), json!(true));
            }
            _ => {}
        }
        e
    }

    fn topmost_hit(&self, p: Pos2, tol: f32) -> Option<String> {
        let live: Vec<&Element> = self.scene.live().collect();
        live.into_iter().rev().filter(|e| !(e.kind == "text" && container_of(e).is_some())).find(|e| hit(e, p, tol)).map(|e| e.id.clone())
    }

    fn selection_bounds(&self) -> Option<Rect> {
        let mut r: Option<Rect> = None;
        for e in self.scene.live().filter(|e| self.selection.contains(&e.id)) {
            let b = e.bounds();
            r = Some(r.map(|x| x.union(b)).unwrap_or(b));
        }
        r
    }

    fn delete_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.snapshot();
        let mut sel = std::mem::take(&mut self.selection);
        let labels: Vec<String> = self.scene.live().filter(|e| container_of(e).map(|c| sel.contains(&c)).unwrap_or(false)).map(|e| e.id.clone()).collect();
        sel.extend(labels);
        for e in self.scene.elements.iter_mut().filter(|e| sel.contains(&e.id)) {
            e.is_deleted = true;
        }
        self.touch_ids(&sel);
        self.changed();
    }

    fn duplicate_selection(&mut self) {
        if self.selection.is_empty() {
            return;
        }
        self.snapshot();
        let copies: Vec<Element> = self.scene.live().filter(|e| self.selection.contains(&e.id)).cloned().collect();
        let mut new_sel = Vec::new();
        for mut c in copies {
            let old_id = c.id.clone();
            c.id = new_id(&mut self.rng);
            c.x += 20.0;
            c.y += 20.0;
            c.seed = self.rng.next() % 2_000_000_000;
            if c.is_linear() {
                c.extra.insert("startBinding".into(), Value::Null);
                c.extra.insert("endBinding".into(), Value::Null);
            }
            c.extra.insert("boundElements".into(), Value::Null);
            let label = self.label_of(&old_id).and_then(|l| self.scene.elements.iter().find(|e| e.id == l).cloned());
            new_sel.push(c.id.clone());
            let cid = c.id.clone();
            self.scene.elements.push(c);
            if let Some(mut l) = label {
                l.id = new_id(&mut self.rng);
                l.extra.insert("containerId".into(), json!(cid.clone()));
                let lid = l.id.clone();
                self.scene.elements.push(l);
                if let Some(sh) = self.scene.elements.iter_mut().find(|e| e.id == cid) {
                    sh.extra.insert("boundElements".into(), json!([{"id": lid, "type": "text"}]));
                }
            }
        }
        self.selection = new_sel;
        self.changed();
    }

    fn reorder_selection(&mut self, to_front: bool) {
        if self.selection.is_empty() {
            return;
        }
        self.snapshot();
        let mut sel_ids = self.selection.clone();
        let labels: Vec<String> = self.scene.elements.iter().filter(|e| container_of(e).map(|c| sel_ids.contains(&c)).unwrap_or(false)).map(|e| e.id.clone()).collect();
        sel_ids.extend(labels);
        let (sel, rest): (Vec<Element>, Vec<Element>) = self.scene.elements.drain(..).partition(|e| sel_ids.contains(&e.id));
        self.scene.elements = if to_front { rest.into_iter().chain(sel).collect() } else { sel.into_iter().chain(rest).collect() };
        self.changed();
    }

    fn commit_text(&mut self, ui: &Ui) {
        let Some(id) = self.editing_text.take() else { return };
        let text = std::mem::take(&mut self.text_buf);
        let mut measured = self.scene.elements.iter().find(|e| e.id == id).cloned();
        if let Some(m) = measured.as_mut() {
            m.text = Some(text.clone());
        }
        let galley = match &measured {
            Some(m) => ui.ctx().fonts_mut(|f| f.layout_job(text_job(m, 1.0, Color32::WHITE, None))),
            None => ui.painter().layout_no_wrap(text.clone(), FontId::proportional(20.0), Color32::WHITE),
        };
        let container = measured.as_ref().and_then(container_of);
        if text.trim().is_empty() {
            if let Some(c) = &container {
                if let Some(sh) = self.scene.elements.iter_mut().find(|e| e.id == *c) {
                    let list: Vec<Value> = sh.extra.get("boundElements").and_then(|v| v.as_array()).cloned().unwrap_or_default().into_iter().filter(|b| b.get("id").and_then(|v| v.as_str()) != Some(id.as_str())).collect();
                    sh.extra.insert("boundElements".into(), if list.is_empty() { Value::Null } else { Value::Array(list) });
                }
            }
        }
        if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
            if text.trim().is_empty() {
                e.is_deleted = true;
            } else {
                e.width = galley.size().x as f64;
                e.height = galley.size().y as f64;
                e.extra.insert("originalText".into(), json!(text.clone()));
                // the Obsidian plugin keeps the text with link markup here; keep it in step
                if e.extra.contains_key("rawText") {
                    e.extra.insert("rawText".into(), json!(text.clone()));
                }
                e.text = Some(text);
            }
        }
        self.touch_ids(&[id]);
        self.changed();
    }

    /// The first frame: show the whole drawing.
    fn fit_once(&mut self, rect: Rect) {
        if !self.centered {
            self.centered = true;
            if let Some(b) = self.scene.bounds() {
                let z = (rect.width() / (b.width() + 120.0)).min(rect.height() / (b.height() + 200.0)).clamp(0.1, 1.0);
                self.zoom = z;
                self.offset = rect.size() / 2.0 / z - b.center().to_vec2();
            } else {
                self.offset = Vec2::new(260.0, 120.0);
            }
        }
    }

    /// Wheel: scroll pans, ctrl+wheel / pinch zooms.
    fn wheel(&mut self, ui: &Ui, rect: Rect, resp: &egui::Response) {
        if resp.contains_pointer() {
            let (zoom_delta, scroll, hover) = ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta(), i.pointer.hover_pos()));
            if zoom_delta != 1.0 {
                if let Some(hp) = hover {
                    let v = View { origin: rect.min, offset: self.offset, zoom: self.zoom };
                    let before = v.to_world(hp);
                    self.zoom = (self.zoom * zoom_delta).clamp(0.1, 10.0);
                    let after = View { zoom: self.zoom, ..v }.to_world(hp);
                    self.offset += after - before;
                }
            } else if scroll != Vec2::ZERO {
                self.offset += scroll / self.zoom;
            }
        }
    }

    /// Draw the view into `ui`: the editor, or the read-only view. `dark` = dark canvas; `vault`
    /// is used for link colours and the `[[` / `[!` helpers.
    pub fn ui(&mut self, ui: &mut Ui, dark: bool, vault: &crate::vault::Vault) -> Option<DrawingAction> {
        if let Some(err) = &self.error {
            ui.colored_label(Color32::from_rgb(255, 120, 120), err);
            ui.label("The file was not changed. Fix or delete it to draw here.");
            return None;
        }
        if self.read_only.is_some() {
            return self.ui_read_only(ui, dark, vault);
        }
        let mut action = None;
        self.ui_rects.clear();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.ui_rects.push(("canvas".into(), rect));
        let bg = if dark { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE };
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, bg);
        self.fit_once(rect);

        // ---- keyboard
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && (resp.contains_pointer() || resp.has_focus()) {
            let mut undo = false;
            let mut redo = false;
            let mut dup = false;
            let mut all = false;
            let mut tool = None;
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::Z) || i.consume_key(egui::Modifiers::COMMAND, Key::Y) {
                    redo = true;
                } else if i.consume_key(egui::Modifiers::COMMAND, Key::Z) {
                    undo = true;
                } else if i.consume_key(egui::Modifiers::COMMAND, Key::D) {
                    dup = true;
                } else if i.consume_key(egui::Modifiers::COMMAND, Key::A) {
                    all = true;
                } else if !i.modifiers.command && !i.modifiers.alt {
                    let keys = [
                        (Key::Num1, Tool::Hand),
                        (Key::H, Tool::Hand),
                        (Key::Num2, Tool::Select),
                        (Key::V, Tool::Select),
                        (Key::Num3, Tool::Rectangle),
                        (Key::R, Tool::Rectangle),
                        (Key::Num4, Tool::Diamond),
                        (Key::D, Tool::Diamond),
                        (Key::Num5, Tool::Ellipse),
                        (Key::O, Tool::Ellipse),
                        (Key::Num6, Tool::Arrow),
                        (Key::A, Tool::Arrow),
                        (Key::Num7, Tool::Line),
                        (Key::L, Tool::Line),
                        (Key::Num8, Tool::Draw),
                        (Key::P, Tool::Draw),
                        (Key::Num9, Tool::Text),
                        (Key::T, Tool::Text),
                        (Key::Num0, Tool::Eraser),
                        (Key::E, Tool::Eraser),
                    ];
                    for (k, t) in keys {
                        if i.key_pressed(k) {
                            tool = Some(t);
                        }
                    }
                }
            });
            if undo {
                self.undo();
            }
            if redo {
                self.redo();
            }
            if dup {
                self.duplicate_selection();
            }
            if all {
                self.selection = self.scene.live().map(|e| e.id.clone()).collect();
            }
            if let Some(t) = tool {
                self.tool = t;
            }
            if ui.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) {
                self.delete_selection();
            }
            if self.selection.len() == 1 && ui.input(|i| i.key_pressed(Key::Enter)) {
                let id = self.selection[0].clone();
                if self.scene.live().any(|e| e.id == id && is_shape(e)) {
                    self.edit_label(&id);
                }
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                self.selection.clear();
                self.tool = Tool::Select;
            }
        }

        self.wheel(ui, rect, &resp);
        let view = View { origin: rect.min, offset: self.offset, zoom: self.zoom };
        paint_grid(&painter, rect, &view, dark);

        // keep labels centred and connections attached; the first pass after opening only tidies
        // the scene in memory, so looking at a drawing never rewrites its file
        self.sync_labels(ui.ctx());
        if self.update_bindings() && self.settled {
            self.changed();
        }
        self.settled = true;
        if self.remeasure_free_text {
            self.remeasure_free_text = false;
            let ctx = ui.ctx().clone();
            for e in self.scene.elements.iter_mut().filter(|e| !e.is_deleted && e.kind == "text" && container_of(e).is_none()) {
                let sz = ctx.fonts_mut(|f| f.layout_job(text_job(e, 1.0, Color32::WHITE, None))).size();
                e.width = sz.x as f64;
                e.height = sz.y as f64;
            }
        }
        let pics = crate::drawing_pictures::textures(ui.ctx(), &self.scene, vault, &self.rel);
        paint_scene(&painter, &self.scene, &view, dark, self.editing_text.as_deref(), &pics);
        let on_mark = self.paint_links(ui, &painter, &view, &resp, vault);

        // ---- pointer
        let space_down = !typing && ui.input(|i| i.key_down(Key::Space));
        let middle = ui.input(|i| i.pointer.middle_down());
        let tol = 6.0 / self.zoom;
        let shift = ui.input(|i| i.modifiers.shift);

        if resp.drag_started() {
            // start where the button went down, not where the drag was first noticed
            if let Some(sp) = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos()) {
                if self.editing_text.is_some() {
                    self.commit_text(ui);
                }
                let wp = view.to_world(sp);
                if space_down || middle || self.tool == Tool::Hand {
                    self.drag = Drag::Pan;
                } else {
                    self.begin_drag(sp, wp, &view, tol, shift);
                }
            }
        }
        if resp.dragged() {
            if let Some(sp) = ui.input(|i| i.pointer.interact_pos()) {
                let wp = view.to_world(sp);
                self.continue_drag(ui, wp, tol, shift);
            }
        }
        if resp.drag_stopped() {
            self.end_drag();
        }
        if resp.clicked() || resp.double_clicked() {
            if let Some(sp) = resp.interact_pointer_pos() {
                // a link mark, or Ctrl+click on an element with a link, follows the link
                let follow = if resp.clicked() && self.editing_text.is_none() { self.follow_click(ui, &view, sp, tol) } else { None };
                if follow.is_some() {
                    action = follow;
                } else {
                    let wp = view.to_world(sp);
                    self.click(ui, wp, tol, shift, resp.double_clicked());
                }
            }
        }

        // inline text editor
        if let Some(id) = self.editing_text.clone() {
            if let Some(e) = self.scene.elements.iter().find(|e| e.id == id).cloned() {
                let size = e.font_size.unwrap_or(20.0) as f32 * self.zoom;
                let fid = font_id_for(font_family_of(&e), size);
                let color = parse_color(&e.stroke_color).map(|c| if dark { dark_variant(c) } else { c }).unwrap_or(Color32::GRAY);
                let galley = ui.painter().layout_no_wrap(format!("{}WW", self.text_buf), fid.clone(), color);
                let container = container_of(&e).and_then(|c| self.scene.elements.iter().find(|x| x.id == c).map(|x| x.bounds()));
                let (r, align) = match container {
                    Some(cb) => {
                        let cr = Rect::from_min_max(view.to_screen(cb.min), view.to_screen(cb.max));
                        let h = (galley.size().y.max(size * 1.3) + 6.0).min(cr.height().max(size * 1.4));
                        (Rect::from_center_size(cr.center(), Vec2::new((cr.width() - 8.0).max(60.0), h)), egui::Align::Center)
                    }
                    None => {
                        let pos = view.to_screen(Pos2::new(e.x as f32, e.y as f32));
                        let w = galley.size().x.max(60.0) + 10.0;
                        let h = galley.size().y.max(size * 1.3) + 6.0;
                        (Rect::from_min_size(pos - Vec2::new(3.0, 3.0), Vec2::new(w, h)), egui::Align::Min)
                    }
                };
                let w = r.width();
                let mut child = ui.new_child(UiBuilder::new().max_rect(r));
                // `[[`, `[[p ` and `[!` work here as in notes; the popups take their keys first
                let te_id = egui::Id::unique(("draw-text", id.clone()));
                self.assist.id = te_id;
                let keys = self.assist.take_keys(&mut child, &mut self.text_buf, vault);
                if let Some(a) = keys.action {
                    action = Some(DrawingAction::Editor(a));
                }
                self.assist.arm(&self.text_buf, keys.handled);
                let out = egui::TextEdit::multiline(&mut self.text_buf)
                    .align(egui::Align2([align, egui::Align::TOP]))
                    .font(fid)
                    .text_color(color)
                    .frame(egui::Frame::NONE)
                    .desired_width(w)
                    .desired_rows(1)
                    // Esc and Tab stay with the text box (the popups and the commit handle them)
                    .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
                    .id(te_id)
                    .show(&mut child);
                let (_, a) = self.assist.after(&mut child, &mut self.text_buf, vault, &out, true, keys.handled);
                if let Some(a) = a {
                    action = Some(DrawingAction::Editor(a));
                }
                let out = out.response.response;
                painter.rect_stroke(r, CornerRadius::same(2), Stroke::new(1.0, ACCENT), StrokeKind::Outside);
                let popup = self.assist.is_open();
                if !out.has_focus() && (!out.lost_focus() || popup) {
                    out.request_focus();
                }
                let esc = ui.input(|i| i.key_pressed(Key::Escape));
                if (out.lost_focus() && !popup) || esc {
                    self.commit_text(ui);
                }
            }
        }

        // selection outline + handles
        if !self.selection.is_empty() && self.editing_text.is_none() {
            for e in self.scene.live().filter(|e| self.selection.contains(&e.id)) {
                let b = e.bounds().expand(4.0 / self.zoom);
                let r = Rect::from_min_max(view.to_screen(b.min), view.to_screen(b.max));
                painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, ACCENT), StrokeKind::Middle);
            }
            let single_line = if self.selection.len() == 1 { self.scene.live().find(|e| e.id == self.selection[0] && e.is_linear() && e.kind != "freedraw" && e.points.len() >= 2).cloned() } else { None };
            if let Some(l) = &single_line {
                for (i, p) in [l.points[0], *l.points.last().unwrap()].iter().enumerate() {
                    let sp = view.to_screen(Pos2::new((l.x + p[0]) as f32, (l.y + p[1]) as f32));
                    let bound = binding_of(l, if i == 0 { "startBinding" } else { "endBinding" }).is_some();
                    painter.circle_filled(sp, 5.5, if bound { ACCENT } else { bg });
                    painter.circle_stroke(sp, 5.5, Stroke::new(1.5, ACCENT));
                }
            } else if self.selection.len() == 1 {
                if let Some(b) = self.selection_bounds() {
                    let b = b.expand(4.0 / self.zoom);
                    for c in [b.left_top(), b.right_top(), b.left_bottom(), b.right_bottom()] {
                        let hr = Rect::from_center_size(view.to_screen(c), Vec2::splat(8.0));
                        painter.rect_filled(hr, CornerRadius::same(2), bg);
                        painter.rect_stroke(hr, CornerRadius::same(2), Stroke::new(1.0, ACCENT), StrokeKind::Middle);
                    }
                }
            }
        }
        // connection anchors: show them while drawing or re-attaching a line
        let connecting = matches!(self.drag, Drag::Create { .. } | Drag::Endpoint { .. }) || matches!(self.tool, Tool::Arrow | Tool::Line);
        if connecting {
            let hp = ui.input(|i| i.pointer.hover_pos()).map(|p| view.to_world(p));
            let target = self.snap_hint.clone().map(|sn| sn.0).or_else(|| hp.and_then(|p| self.find_snap(p, None)).map(|sn| sn.0));
            if let Some(tid) = target {
                if let Some(t) = self.scene.live().find(|e| e.id == tid) {
                    for ratio in crate::drawing_layout::anchor_ratios(&t.kind) {
                        let ap = view.to_screen(crate::drawing_layout::anchor_point(t.bounds(), ratio));
                        painter.circle_stroke(ap, 4.0, Stroke::new(1.2, ACCENT));
                    }
                }
            }
            if let Some((_, p)) = &self.snap_hint {
                painter.circle_filled(view.to_screen(*p), 6.0, ACCENT);
            }
        }
        if let Some(m) = self.marquee {
            let r = Rect::from_min_max(view.to_screen(m.min), view.to_screen(m.max));
            painter.rect_filled(r, CornerRadius::ZERO, ACCENT.gamma_multiply(0.1));
            painter.rect_stroke(r, CornerRadius::ZERO, Stroke::new(1.0, ACCENT), StrokeKind::Middle);
        }

        if resp.hovered() {
            let icon = if space_down || matches!(self.drag, Drag::Pan) {
                egui::CursorIcon::Grabbing
            } else if on_mark {
                egui::CursorIcon::PointingHand
            } else {
                match self.tool {
                    Tool::Hand => egui::CursorIcon::Grab,
                    Tool::Select => {
                        let hp = ui.input(|i| i.pointer.hover_pos());
                        if hp.map(|p| self.topmost_hit(view.to_world(p), tol).is_some()).unwrap_or(false) {
                            egui::CursorIcon::Move
                        } else {
                            egui::CursorIcon::Default
                        }
                    }
                    Tool::Text => egui::CursorIcon::Text,
                    _ => egui::CursorIcon::Crosshair,
                }
            };
            ui.ctx().set_cursor_icon(icon);
        }

        self.toolbar(ui, rect, dark);
        self.properties(ui, rect, dark);
        self.zoom_bar(ui, rect, dark);
        self.arrange_bar(ui, rect, dark);
        action
    }

    fn begin_drag(&mut self, sp: Pos2, wp: Pos2, view: &View, tol: f32, shift: bool) {
        match self.tool {
            Tool::Select => {
                // ends of a selected line / arrow: drag to move or re-attach them
                if self.selection.len() == 1 {
                    let id = self.selection[0].clone();
                    let line = self.scene.live().find(|e| e.id == id && e.is_linear() && e.kind != "freedraw" && e.points.len() >= 2).cloned();
                    if let Some(l) = line {
                        for (start, p) in [(true, l.points[0]), (false, *l.points.last().unwrap())] {
                            let ep = view.to_screen(Pos2::new((l.x + p[0]) as f32, (l.y + p[1]) as f32));
                            if (ep - sp).length() < 10.0 {
                                self.snapshot();
                                self.drag = Drag::Endpoint { id, start, target: None };
                                return;
                            }
                        }
                    }
                }
                let single_linear = self.selection.len() == 1 && self.scene.live().any(|e| e.id == self.selection[0] && e.is_linear() && e.kind != "freedraw");
                if self.selection.len() == 1 && !single_linear {
                    if let Some(b) = self.selection_bounds() {
                        let be = b.expand(4.0 / self.zoom);
                        let corners = [(be.left_top(), b.right_bottom()), (be.right_top(), b.left_bottom()), (be.left_bottom(), b.right_top()), (be.right_bottom(), b.left_top())];
                        for (corner, anchor) in corners {
                            if (view.to_screen(corner) - sp).length() < 9.0 {
                                let id = self.selection[0].clone();
                                if let Some(orig) = self.scene.elements.iter().find(|e| e.id == id).cloned() {
                                    self.snapshot();
                                    self.drag = Drag::Resize { id, anchor, orig: Box::new(orig) };
                                    return;
                                }
                            }
                        }
                    }
                }
                match self.topmost_hit(wp, tol) {
                    Some(id) => {
                        if shift && !self.selection.contains(&id) {
                            self.selection.push(id);
                        } else if !self.selection.contains(&id) {
                            self.selection = vec![id];
                        }
                        self.snapshot();
                        self.detach_unselected();
                        self.drag = Drag::Move { last: wp };
                    }
                    None => {
                        if !shift {
                            self.selection.clear();
                        }
                        self.drag = Drag::Marquee { start: wp };
                    }
                }
            }
            Tool::Eraser => {
                self.snapshot();
                self.drag = Drag::Erase;
                self.erase_at(wp, tol);
            }
            Tool::Text | Tool::Hand => {}
            t => {
                self.snapshot();
                // lines and arrows that start on a shape attach to its nearest anchor
                let snap = if matches!(t, Tool::Arrow | Tool::Line) { self.find_snap(wp, None) } else { None };
                let start = snap.as_ref().map(|sn| sn.2).unwrap_or(wp);
                let e = self.new_element(t.element_kind(), start);
                let id = e.id.clone();
                self.scene.elements.push(e);
                self.selection = vec![id.clone()];
                self.drag = Drag::Create { id, start, bind_start: snap.map(|sn| (sn.0, sn.1)), bind_end: None };
            }
        }
    }

    fn erase_at(&mut self, wp: Pos2, tol: f32) {
        if let Some(id) = self.topmost_hit(wp, tol * 1.5) {
            if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
                e.is_deleted = true;
            }
            self.touch_ids(std::slice::from_ref(&id));
            self.selection.retain(|s| *s != id);
            self.changed();
        }
    }

    fn continue_drag(&mut self, ui: &Ui, wp: Pos2, tol: f32, shift: bool) {
        let zoom = self.zoom;
        // where a connection end would snap to (not onto the shape the other end is on)
        let snap = match &self.drag {
            Drag::Create { id, bind_start, .. } if self.scene.elements.iter().any(|e| e.id == *id && matches!(e.kind.as_str(), "line" | "arrow")) => self.find_snap(wp, bind_start.as_ref().map(|b| b.0.as_str())),
            Drag::Endpoint { id, start, .. } => {
                let other = self.scene.elements.iter().find(|e| e.id == *id).and_then(|e| binding_of(e, if *start { "endBinding" } else { "startBinding" })).map(|b| b.0);
                self.find_snap(wp, other.as_deref())
            }
            _ => None,
        };
        self.snap_hint = snap.as_ref().map(|sn| (sn.0.clone(), sn.2));
        let lock45 = |from: Pos2, to: Pos2| -> Pos2 {
            let d = to - from;
            let step = std::f32::consts::FRAC_PI_4;
            let a = (d.angle() / step).round() * step;
            from + Vec2::angled(a) * d.length()
        };
        if let Drag::Endpoint { id, start, target } = &mut self.drag {
            let (id, start) = (id.clone(), *start);
            *target = snap.as_ref().map(|sn| (sn.0.clone(), sn.1));
            if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
                let mut abs: Vec<Pos2> = e.points.iter().map(|p| Pos2::new((e.x + p[0]) as f32, (e.y + p[1]) as f32)).collect();
                let n = abs.len();
                let (idx, other) = if start { (0, abs[1.min(n - 1)]) } else { (n - 1, abs[n.saturating_sub(2)]) };
                abs[idx] = match &snap {
                    Some(sn) => sn.2,
                    None if shift => lock45(other, wp),
                    None => wp,
                };
                set_abs_points(e, &abs);
            }
            self.changed();
            return;
        }
        if let Drag::Create { bind_end, .. } = &mut self.drag {
            *bind_end = snap.as_ref().map(|sn| (sn.0.clone(), sn.1));
        }
        match &mut self.drag {
            Drag::Pan => {
                self.offset += ui.input(|i| i.pointer.delta()) / zoom;
            }
            Drag::Create { id, start, .. } => {
                let (id, start) = (id.clone(), *start);
                if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
                    match e.kind.as_str() {
                        "line" | "arrow" => {
                            // snapped to a shape anchor, or locked to 45 degree steps with Shift
                            let end = match &snap {
                                Some(sn) => sn.2,
                                None if shift => lock45(start, wp),
                                None => wp,
                            };
                            let d = end - start;
                            e.points = vec![[0.0, 0.0], [d.x as f64, d.y as f64]];
                            e.width = d.x.abs() as f64;
                            e.height = d.y.abs() as f64;
                        }
                        "freedraw" => {
                            let d = wp - start;
                            let last = e.points.last().copied().unwrap_or([0.0, 0.0]);
                            if ((d.x as f64 - last[0]).powi(2) + (d.y as f64 - last[1]).powi(2)).sqrt() > 1.5 / zoom as f64 {
                                e.points.push([d.x as f64, d.y as f64]);
                            }
                            let b = e.bounds();
                            e.width = b.width() as f64;
                            e.height = b.height() as f64;
                        }
                        _ => {
                            let mut d = wp - start;
                            if shift {
                                let m = d.x.abs().max(d.y.abs());
                                d = Vec2::new(m * d.x.signum(), m * d.y.signum());
                            }
                            let r = Rect::from_two_pos(start, start + d);
                            e.x = r.min.x as f64;
                            e.y = r.min.y as f64;
                            e.width = r.width() as f64;
                            e.height = r.height() as f64;
                        }
                    }
                }
                self.changed();
            }
            Drag::Move { last } => {
                let d = wp - *last;
                *last = wp;
                if d != Vec2::ZERO {
                    let sel = self.selection.clone();
                    for e in self.scene.elements.iter_mut().filter(|e| sel.contains(&e.id)) {
                        e.x += d.x as f64;
                        e.y += d.y as f64;
                    }
                    self.changed();
                }
            }
            Drag::Resize { id, anchor, orig } => {
                let (id, anchor, orig) = (id.clone(), *anchor, orig.clone());
                let ob = orig.bounds();
                let nr = Rect::from_two_pos(anchor, wp);
                let sx = if ob.width() > 0.5 { nr.width() / ob.width() } else { 1.0 };
                let sy = if ob.height() > 0.5 { nr.height() / ob.height() } else { 1.0 };
                let flip_x = (wp.x < anchor.x) == ((anchor.x - ob.min.x).abs() < 0.01);
                let flip_y = (wp.y < anchor.y) == ((anchor.y - ob.min.y).abs() < 0.01);
                if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
                    if e.is_linear() {
                        e.points = orig
                            .points
                            .iter()
                            .map(|p| {
                                let ax = orig.x + p[0] - ob.min.x as f64;
                                let ay = orig.y + p[1] - ob.min.y as f64;
                                let mut nx = ax * sx as f64;
                                let mut ny = ay * sy as f64;
                                if flip_x {
                                    nx = nr.width() as f64 - nx;
                                }
                                if flip_y {
                                    ny = nr.height() as f64 - ny;
                                }
                                [nx, ny]
                            })
                            .collect();
                        e.x = nr.min.x as f64;
                        e.y = nr.min.y as f64;
                        e.width = nr.width() as f64;
                        e.height = nr.height() as f64;
                    } else if e.kind == "text" {
                        let s = sy.max(0.1) as f64;
                        e.font_size = Some((orig.font_size.unwrap_or(20.0) * s).clamp(4.0, 400.0));
                        e.x = nr.min.x as f64;
                        e.y = nr.min.y as f64;
                        e.width = orig.width * s;
                        e.height = orig.height * s;
                    } else {
                        e.x = nr.min.x as f64;
                        e.y = nr.min.y as f64;
                        e.width = nr.width() as f64;
                        e.height = nr.height() as f64;
                    }
                }
                self.changed();
            }
            Drag::Marquee { start } => {
                self.marquee = Some(Rect::from_two_pos(*start, wp));
            }
            Drag::Erase => {
                self.erase_at(wp, tol);
            }
            Drag::Endpoint { .. } | Drag::None => {}
        }
    }

    fn end_drag(&mut self) {
        self.snap_hint = None;
        match std::mem::replace(&mut self.drag, Drag::None) {
            Drag::Endpoint { id, start, target } => {
                self.set_binding(&id, start, target);
                self.update_bindings();
                self.touch_ids(std::slice::from_ref(&id));
                self.changed();
            }
            Drag::Create { id, bind_start, bind_end, .. } => {
                let is_line = self.scene.elements.iter().any(|e| e.id == id && matches!(e.kind.as_str(), "line" | "arrow"));
                if is_line {
                    let long_enough = self.scene.elements.iter().find(|e| e.id == id).map(|e| e.bounds().size().length() > 3.0 / self.zoom).unwrap_or(false);
                    if long_enough || bind_end.is_some() {
                        if bind_start.is_some() {
                            self.set_binding(&id, true, bind_start);
                        }
                        if bind_end.is_some() {
                            self.set_binding(&id, false, bind_end);
                        }
                        self.update_bindings();
                    }
                }
                let zoom = self.zoom;
                if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == id) {
                    let b = e.bounds();
                    if b.width() < 3.0 / zoom && b.height() < 3.0 / zoom && e.kind != "freedraw" {
                        if matches!(e.kind.as_str(), "line" | "arrow") {
                            e.points = vec![[0.0, 0.0], [100.0, 0.0]];
                            e.width = 100.0;
                        } else {
                            e.width = 100.0;
                            e.height = 70.0;
                        }
                    }
                }
                self.touch_ids(std::slice::from_ref(&id));
                self.changed();
                if !self.tool_locked && self.tool != Tool::Draw {
                    self.tool = Tool::Select;
                }
            }
            Drag::Marquee { .. } => {
                if let Some(m) = self.marquee.take() {
                    let ids: Vec<String> = self.scene.live().filter(|e| m.contains_rect(e.bounds())).map(|e| e.id.clone()).collect();
                    for id in ids {
                        if !self.selection.contains(&id) {
                            self.selection.push(id);
                        }
                    }
                }
            }
            Drag::Move { .. } | Drag::Resize { .. } => {
                let sel = self.selection.clone();
                self.update_bindings();
                self.tidy_connections(&sel);
                self.touch_ids(&sel);
                self.changed();
            }
            _ => {}
        }
        self.marquee = None;
    }

    fn click(&mut self, ui: &Ui, wp: Pos2, tol: f32, shift: bool, double: bool) {
        if self.editing_text.is_some() {
            self.commit_text(ui);
            return;
        }
        let hit_text = {
            let live: Vec<&Element> = self.scene.live().collect();
            live.iter().rev().find(|e| e.kind == "text" && hit(e, wp, tol)).map(|e| e.id.clone())
        };
        match self.tool {
            Tool::Text => self.start_text(wp, hit_text),
            Tool::Select if double => {
                let free_text = hit_text.filter(|id| self.scene.live().any(|e| e.id == *id && container_of(e).is_none()));
                let shape = self.topmost_hit(wp, tol).filter(|id| self.scene.live().any(|e| e.id == *id && is_shape(e)));
                match (free_text, shape) {
                    (Some(t), _) => self.start_text(wp, Some(t)),
                    (None, Some(sid)) => self.edit_label(&sid),
                    (None, None) => self.start_text(wp, None),
                }
            }
            Tool::Select => match self.topmost_hit(wp, tol) {
                Some(id) => {
                    if shift {
                        if let Some(i) = self.selection.iter().position(|s| *s == id) {
                            self.selection.remove(i);
                        } else {
                            self.selection.push(id);
                        }
                    } else {
                        self.selection = vec![id];
                    }
                }
                None => {
                    if !shift {
                        self.selection.clear();
                    }
                }
            },
            Tool::Eraser => {
                self.snapshot();
                self.erase_at(wp, tol);
            }
            Tool::Hand => {}
            t => {
                // a plain click with a shape tool drops a default-sized shape
                self.snapshot();
                let mut e = self.new_element(t.element_kind(), wp);
                if t == Tool::Draw {
                    e.points = vec![[0.0, 0.0]];
                } else if matches!(t, Tool::Arrow | Tool::Line) {
                    e.points = vec![[0.0, 0.0], [100.0, 0.0]];
                    e.width = 100.0;
                } else {
                    e.x -= 50.0;
                    e.y -= 35.0;
                    e.width = 100.0;
                    e.height = 70.0;
                }
                self.selection = vec![e.id.clone()];
                self.scene.elements.push(e);
                self.changed();
                if !self.tool_locked && t != Tool::Draw {
                    self.tool = Tool::Select;
                }
            }
        }
    }

    fn start_text(&mut self, wp: Pos2, existing: Option<String>) {
        self.snapshot();
        if let Some(id) = existing {
            self.text_buf = self.scene.elements.iter().find(|e| e.id == id).and_then(|e| e.text.clone()).unwrap_or_default();
            self.editing_text = Some(id.clone());
            self.selection = vec![id];
        } else {
            let fs = self.style.font_size;
            let mut e = self.new_element("text", wp - Vec2::new(0.0, fs as f32 * 0.6));
            e.width = 10.0;
            e.height = fs * 1.25;
            let id = e.id.clone();
            self.scene.elements.push(e);
            self.editing_text = Some(id.clone());
            self.text_buf.clear();
            self.selection = vec![id];
        }
        if !self.tool_locked {
            self.tool = Tool::Select;
        }
    }

    fn panel_frame(dark: bool) -> egui::Frame {
        let (fill, stroke) = if dark { (Color32::from_rgb(35, 35, 41), Color32::from_rgb(60, 60, 70)) } else { (Color32::WHITE, Color32::from_rgb(220, 220, 228)) };
        egui::Frame::new().fill(fill).stroke(Stroke::new(1.0, stroke)).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(6)).shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 8,
            spread: 0,
            color: Color32::from_black_alpha(if dark { 80 } else { 30 }),
        })
    }

    /// Block canvas input behind a floating panel.
    fn blocker(ui: &mut Ui, r: Rect, salt: &str) {
        ui.interact(r, ui.scope_id().with(salt), Sense::click_and_drag());
    }

    fn toolbar(&mut self, ui: &mut Ui, canvas: Rect, dark: bool) {
        let w = 10.0 * 38.0 + 64.0;
        let r = Rect::from_min_size(Pos2::new(canvas.center().x - w / 2.0, canvas.min.y + 10.0), Vec2::new(w, 50.0));
        Self::blocker(ui, r, "toolbar-block");
        let mut child = ui.new_child(UiBuilder::new().max_rect(r));
        let fg = if dark { Color32::from_gray(225) } else { Color32::from_gray(40) };
        let mut commit = false;
        Self::panel_frame(dark).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let (lr, lresp) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::click());
                if self.tool_locked {
                    ui.painter().rect_filled(lr, CornerRadius::same(6), ACCENT.gamma_multiply(0.3));
                }
                paint_lock_icon(ui.painter(), lr.shrink(10.0), if self.tool_locked { ACCENT } else { fg }, self.tool_locked);
                self.ui_rects.push(("lock".into(), lr));
                if lresp.on_hover_text("Keep the selected tool active after drawing").clicked() {
                    self.tool_locked = !self.tool_locked;
                }
                ui.separator();
                for t in Tool::ALL {
                    let (tr, tresp) = ui.allocate_exact_size(Vec2::new(36.0, 34.0), Sense::click());
                    self.ui_rects.push((format!("tool:{t:?}"), tr));
                    let active = self.tool == t;
                    if active {
                        ui.painter().rect_filled(tr, CornerRadius::same(6), if dark { Color32::from_rgb(64, 61, 120) } else { Color32::from_rgb(224, 223, 255) });
                    } else if tresp.hovered() {
                        ui.painter().rect_filled(tr, CornerRadius::same(6), fg.gamma_multiply(0.08));
                    }
                    let ic = if active { if dark { Color32::WHITE } else { ACCENT } } else { fg };
                    paint_tool_icon(ui.painter(), tr.shrink2(Vec2::new(11.0, 10.0)), t, ic);
                    let k = t.key();
                    if !k.is_empty() {
                        ui.painter().text(tr.right_bottom() - Vec2::new(3.0, 1.0), Align2::RIGHT_BOTTOM, k, FontId::proportional(9.0), fg.gamma_multiply(0.5));
                    }
                    if tresp.on_hover_text(t.name()).clicked() {
                        self.tool = t;
                        commit = true;
                    }
                }
            });
        });
        if commit && self.editing_text.is_some() {
            self.commit_text(ui);
        }
    }

    fn zoom_bar(&mut self, ui: &mut Ui, canvas: Rect, dark: bool) {
        let r = Rect::from_min_size(Pos2::new(canvas.min.x + 10.0, canvas.max.y - 54.0), Vec2::new(360.0, 46.0));
        Self::blocker(ui, Rect::from_min_size(r.min, Vec2::new(330.0, 40.0)), "zoom-block");
        let mut child = ui.new_child(UiBuilder::new().max_rect(r));
        child.horizontal(|ui| {
            Self::panel_frame(dark).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let b = ui.button(" − ");
                    self.ui_rects.push(("zoom-out".into(), b.rect));
                    if b.on_hover_text("Zoom out").clicked() {
                        self.zoom_about(canvas, 1.0 / 1.2);
                    }
                    let b = ui.button(format!("{:.0}%", self.zoom * 100.0));
                    self.ui_rects.push(("zoom-reset".into(), b.rect));
                    if b.on_hover_text("Reset zoom to 100%").clicked() {
                        self.zoom_about(canvas, 1.0 / self.zoom);
                    }
                    let b = ui.button(" + ");
                    self.ui_rects.push(("zoom-in".into(), b.rect));
                    if b.on_hover_text("Zoom in").clicked() {
                        self.zoom_about(canvas, 1.2);
                    }
                    let b = ui.button("Fit");
                    self.ui_rects.push(("fit".into(), b.rect));
                    if b.on_hover_text("Show the whole drawing").clicked() {
                        self.centered = false;
                    }
                });
            });
            if self.read_only.is_some() {
                return;
            }
            Self::panel_frame(dark).show(ui, |ui| {
                ui.horizontal(|ui| {
                    let b = ui.add_enabled(!self.undo.is_empty(), egui::Button::new("Undo"));
                    self.ui_rects.push(("undo".into(), b.rect));
                    if b.on_hover_text("Ctrl+Z").clicked() {
                        self.undo();
                    }
                    let b = ui.add_enabled(!self.redo.is_empty(), egui::Button::new("Redo"));
                    self.ui_rects.push(("redo".into(), b.rect));
                    if b.on_hover_text("Ctrl+Shift+Z").clicked() {
                        self.redo();
                    }
                });
            });
        });
    }

    fn zoom_about(&mut self, canvas: Rect, factor: f32) {
        let view = View { origin: canvas.min, offset: self.offset, zoom: self.zoom };
        let c = canvas.center();
        let before = view.to_world(c);
        self.zoom = (self.zoom * factor).clamp(0.1, 10.0);
        let after = View { zoom: self.zoom, ..view }.to_world(c);
        self.offset += after - before;
    }
}

// ---------------------------------------------------------------------------------------------
// Icons (painted, so no icon font is needed)

fn paint_lock_icon(p: &egui::Painter, r: Rect, c: Color32, locked: bool) {
    let s = Stroke::new(1.5, c);
    let body = Rect::from_min_max(Pos2::new(r.min.x, r.center().y - 1.0), Pos2::new(r.max.x, r.max.y));
    p.rect_stroke(body, CornerRadius::same(2), s, StrokeKind::Middle);
    let top = body.min.y;
    let l = body.min.x + 3.0;
    let rr = body.max.x - 3.0;
    let right_end = if locked { top } else { top - 3.0 };
    p.add(Shape::line(vec![Pos2::new(l, top), Pos2::new(l, r.min.y + 3.0), Pos2::new((l + rr) / 2.0, r.min.y), Pos2::new(rr, r.min.y + 3.0), Pos2::new(rr, right_end)], s));
}

pub fn paint_tool_icon(p: &egui::Painter, r: Rect, t: Tool, c: Color32) {
    let s = Stroke::new(1.6, c);
    let cx = r.center();
    let o = r.min;
    match t {
        Tool::Select => {
            let pts = vec![o + Vec2::new(2.0, 0.0), o + Vec2::new(2.0, 13.0), o + Vec2::new(5.5, 10.0), o + Vec2::new(8.0, 15.0), o + Vec2::new(10.0, 14.0), o + Vec2::new(7.5, 9.0), o + Vec2::new(12.0, 9.0)];
            p.add(Shape::closed_line(pts, s));
        }
        Tool::Hand => {
            for i in 0..4 {
                let x = r.min.x + 2.5 + i as f32 * 3.2;
                let top = r.min.y + if i == 0 || i == 3 { 3.0 } else { 0.5 };
                p.line_segment([Pos2::new(x, top), Pos2::new(x, cx.y + 1.0)], s);
            }
            p.rect_stroke(Rect::from_min_max(Pos2::new(r.min.x + 1.0, cx.y), Pos2::new(r.max.x - 1.0, r.max.y)), CornerRadius::same(4), s, StrokeKind::Middle);
        }
        Tool::Rectangle => {
            p.rect_stroke(r.shrink(0.5), CornerRadius::same(3), s, StrokeKind::Middle);
        }
        Tool::Diamond => {
            p.add(Shape::closed_line(vec![Pos2::new(cx.x, r.min.y - 1.0), Pos2::new(r.max.x + 1.0, cx.y), Pos2::new(cx.x, r.max.y + 1.0), Pos2::new(r.min.x - 1.0, cx.y)], s));
        }
        Tool::Ellipse => {
            p.circle_stroke(cx, r.width().min(r.height()) / 2.0 + 0.5, s);
        }
        Tool::Arrow => {
            let a = Pos2::new(r.min.x, cx.y);
            let b = Pos2::new(r.max.x, cx.y);
            p.line_segment([a, b], s);
            p.line_segment([b, b + Vec2::new(-5.0, -4.0)], s);
            p.line_segment([b, b + Vec2::new(-5.0, 4.0)], s);
        }
        Tool::Line => {
            p.line_segment([Pos2::new(r.min.x, cx.y), Pos2::new(r.max.x, cx.y)], s);
        }
        Tool::Draw => {
            let a = Pos2::new(r.min.x + 1.0, r.max.y - 1.0);
            let b = Pos2::new(r.max.x - 1.0, r.min.y + 1.0);
            p.add(Shape::closed_line(vec![a, b + Vec2::new(-2.0, -0.5), b, b + Vec2::new(0.5, 2.0), a + Vec2::new(3.0, -0.5)], s));
        }
        Tool::Text => {
            p.text(cx, Align2::CENTER_CENTER, "A", FontId::proportional(r.height() + 4.0), c);
        }
        Tool::Eraser => {
            let pts = vec![o + Vec2::new(0.0, 9.0), o + Vec2::new(8.0, 1.0), o + Vec2::new(14.0, 7.0), o + Vec2::new(7.0, 14.0), o + Vec2::new(4.0, 14.0)];
            p.add(Shape::closed_line(pts, s));
            p.line_segment([o + Vec2::new(4.0, 5.0), o + Vec2::new(10.0, 11.0)], s);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: DRAW-001, DATA-005
    #[test]
    fn sample_parses_and_round_trips() {
        let s = Scene::parse(SAMPLE_SCENE).unwrap();
        assert_eq!(s.elements.len(), 5);
        assert_eq!(s.elements[1].text.as_deref(), Some("Idea"));
        let back: Value = serde_json::from_str(&s.to_json()).unwrap();
        assert_eq!(back["type"], "excalidraw");
        assert_eq!(back["elements"][2]["endArrowhead"], "triangle");
        assert_eq!(back["elements"][0]["roundness"]["type"], 3);
        assert_eq!(back["elements"][1]["fontFamily"], 1);
        let with_unknown = SAMPLE_SCENE.replacen("\"source\": \"shale\"", "\"source\": \"x\", \"custom\": 5", 1);
        let s2 = Scene::parse(&with_unknown).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&s2.to_json()).unwrap()["custom"], 5);
    }

    // spec: DRAW-001, DATA-005
    #[test]
    fn new_elements_have_excalidraw_fields() {
        let mut d = DrawingDoc::new(PathBuf::from("x.excalidraw"), "");
        let e = d.new_element("arrow", Pos2::new(1.0, 2.0));
        let v = serde_json::to_value(&e).unwrap();
        for key in ["id", "type", "x", "y", "strokeColor", "backgroundColor", "fillStyle", "strokeWidth", "roughness", "opacity", "seed", "versionNonce", "isDeleted", "groupIds", "boundElements", "points", "endArrowhead"] {
            assert!(v.get(key).is_some(), "missing {key}");
        }
        let t = d.new_element("text", Pos2::ZERO);
        let tv = serde_json::to_value(&t).unwrap();
        assert!(tv.get("fontSize").is_some() && tv.get("text").is_some() && tv.get("originalText").is_some());
    }

    // spec: DATA-007
    #[test]
    fn empty_file_is_empty_scene() {
        let s = Scene::parse("").unwrap();
        assert!(s.elements.is_empty());
        assert!(s.bounds().is_none());
        assert!(Scene::parse("{not json").is_err());
    }

    // spec: DRAW-006
    #[test]
    fn undo_redo() {
        let mut d = DrawingDoc::new(PathBuf::from("x.excalidraw"), SAMPLE_SCENE);
        d.selection = vec!["s1".into()];
        d.delete_selection();
        // the shape's label goes with it
        assert_eq!(d.scene.live().count(), 3);
        d.undo();
        assert_eq!(d.scene.live().count(), 5);
        d.redo();
        assert_eq!(d.scene.live().count(), 3);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ff0000"), Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(parse_color("#f00"), Some(Color32::from_rgb(255, 0, 0)));
        assert_eq!(parse_color("transparent"), None);
        assert!(dark_variant(Color32::from_rgb(30, 30, 30)).r() > 200);
    }

    #[test]
    fn hit_testing() {
        let s = Scene::parse(SAMPLE_SCENE).unwrap();
        assert!(hit(&s.elements[0], Pos2::new(100.0, 100.0), 4.0));
        assert!(!hit(&s.elements[0], Pos2::new(1000.0, 100.0), 4.0));
        assert!(hit(&s.elements[2], Pos2::new(340.0, 96.0), 4.0));
        assert!(!hit(&s.elements[2], Pos2::new(340.0, 140.0), 4.0));
    }

    // spec: DRAW-002
    #[test]
    fn hatching_stays_inside() {
        let poly = vec![Pos2::new(0.0, 0.0), Pos2::new(100.0, 0.0), Pos2::new(100.0, 50.0), Pos2::new(0.0, 50.0)];
        let lines = hatch_lines(&poly, 8.0, -0.7);
        assert!(!lines.is_empty());
        for l in lines {
            for p in l {
                assert!(p.x > -0.5 && p.x < 100.5 && p.y > -0.5 && p.y < 50.5);
            }
        }
    }
}
