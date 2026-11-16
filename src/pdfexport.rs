//! Export notes and drawings to PDF.
//!
//! - Notes: the markdown is laid out on pages (paper size, orientation, margins and text size
//!   are options). Text uses embedded fonts, so it stays selectable and searchable; web links
//!   are clickable; pictures, drawings and PDF pages that are embedded in the note are included.
//! - Drawings: vector output (shapes, hatching, arrows and the hand-drawn text as outlines), on a
//!   page that fits the drawing or on a paper size, with a white, dark or no background.

use crate::drawing::{self, Element, Scene, View};
use crate::markdown;
use crate::pdfwrite::{Doc, FontKind, Image, Page, Rgba, TextStyle};
use crate::theme;
use crate::vault::{FileKind, Vault};
use eframe::egui::{self, Color32, Pos2, Shape, Vec2};
use std::collections::HashMap;
use std::path::Path;

const MM: f32 = 72.0 / 25.4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Paper {
    A3,
    A4,
    A5,
    Letter,
    Legal,
    /// drawings only: the page is exactly as big as the drawing
    FitDrawing,
}

impl Paper {
    pub const NOTE: [Paper; 5] = [Paper::A4, Paper::A5, Paper::A3, Paper::Letter, Paper::Legal];
    pub const DRAWING: [Paper; 6] = [Paper::FitDrawing, Paper::A4, Paper::A5, Paper::A3, Paper::Letter, Paper::Legal];
    pub fn label(self) -> &'static str {
        match self {
            Paper::A3 => "A3 (297 × 420 mm)",
            Paper::A4 => "A4 (210 × 297 mm)",
            Paper::A5 => "A5 (148 × 210 mm)",
            Paper::Letter => "Letter (8.5 × 11 in)",
            Paper::Legal => "Legal (8.5 × 14 in)",
            Paper::FitDrawing => "Fit to the drawing",
        }
    }
    /// Portrait size in points.
    pub fn size(self) -> Option<[f32; 2]> {
        Some(match self {
            Paper::A3 => [842.0, 1191.0],
            Paper::A4 => [595.0, 842.0],
            Paper::A5 => [420.0, 595.0],
            Paper::Letter => [612.0, 792.0],
            Paper::Legal => [612.0, 1008.0],
            Paper::FitDrawing => return None,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Background {
    White,
    Dark,
    None,
}

#[derive(Clone, Debug)]
pub struct ExportOptions {
    pub paper: Paper,
    pub landscape: bool,
    pub margin_mm: f32,
    /// notes: body text size in points
    pub font_pt: f32,
    /// notes: print the note's name as a title
    pub title: bool,
    pub page_numbers: bool,
    /// notes: include embedded pictures, drawings and PDF pages
    pub embeds: bool,
    /// drawings
    pub background: Background,
    /// drawings: scale to fill the page (otherwise 100%, shrunk only if it doesn't fit)
    pub fill_page: bool,
}

impl ExportOptions {
    pub fn for_note() -> Self {
        ExportOptions { paper: Paper::A4, landscape: false, margin_mm: 20.0, font_pt: 11.0, title: true, page_numbers: true, embeds: true, background: Background::White, fill_page: true }
    }
    pub fn for_drawing(scene: &Scene) -> Self {
        let wide = scene.bounds().map(|b| b.width() > b.height()).unwrap_or(true);
        ExportOptions { paper: Paper::FitDrawing, landscape: wide, margin_mm: 10.0, font_pt: 11.0, title: false, page_numbers: false, embeds: true, background: Background::White, fill_page: true }
    }
    fn page_size(&self) -> [f32; 2] {
        let [w, h] = self.paper.size().unwrap_or([595.0, 842.0]);
        if self.landscape {
            [h, w]
        } else {
            [w, h]
        }
    }
}

// ---------------------------------------------------------------- colours for print

fn rgba(c: Color32) -> Rgba {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    Rgba(r, g, b, a)
}

/// Screen (dark theme) colours → colours that print well on white paper.
fn print_color(c: Color32) -> Rgba {
    let map = [
        (theme::LINK, Rgba(26, 95, 180, 255)),
        (theme::LINK_MISSING, Rgba(192, 28, 40, 255)),
        (theme::TAG, Rgba(105, 65, 198, 255)),
        (theme::CODE, Rgba(150, 70, 0, 255)),
        (theme::DONE, Rgba(140, 140, 140, 255)),
        (theme::QUOTE, Rgba(85, 85, 85, 255)),
        (theme::MUTED, Rgba(100, 100, 100, 255)),
        (theme::ACCENT, Rgba(105, 65, 198, 255)),
    ];
    for (k, v) in map {
        if c == k {
            return v;
        }
    }
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    if (r as u32 + g as u32 + b as u32) / 3 > 150 {
        Rgba(30, 30, 30, 255) // light text on screen → dark on paper
    } else {
        rgba(c)
    }
}

fn print_bg(c: Color32) -> Option<Rgba> {
    if c == Color32::TRANSPARENT {
        None
    } else if c == theme::CODE_BG {
        Some(Rgba(238, 238, 240, 255))
    } else if c == theme::TAG_BG {
        Some(Rgba(236, 232, 251, 255))
    } else {
        Some(Rgba(255, 241, 168, 255))
    }
}

fn font_kind(f: &egui::FontFamily) -> FontKind {
    match f {
        egui::FontFamily::Monospace => FontKind::Mono,
        egui::FontFamily::Name(n) if n.as_ref() == theme::HAND_FONT => FontKind::Hand,
        _ => FontKind::Sans,
    }
}

// ---------------------------------------------------------------- note layout

#[derive(Clone)]
struct Piece {
    text: String,
    kind: FontKind,
    size: f32,
    color: Rgba,
    bg: Option<Rgba>,
    italic: bool,
    bold: bool,
    underline: bool,
    strike: bool,
    link: Option<String>,
}

struct Writer<'a> {
    doc: Doc,
    opts: &'a ExportOptions,
    w: f32,
    h: f32,
    m: f32,
    page: usize,
    y: f32,
}

impl<'a> Writer<'a> {
    fn new(title: &str, opts: &'a ExportOptions) -> Self {
        let [w, h] = opts.page_size();
        let mut doc = Doc::new(title);
        let page = doc.add_page(w, h);
        let m = opts.margin_mm * MM;
        Writer { doc, opts, w, h, m, page, y: m }
    }
    fn cw(&self) -> f32 {
        self.w - 2.0 * self.m
    }
    fn p(&mut self) -> &mut Page {
        self.doc.page(self.page)
    }
    fn new_page(&mut self) {
        self.page = self.doc.add_page(self.w, self.h);
        self.y = self.m;
    }
    /// Make room for `height`; start a new page if it doesn't fit (unless the page is empty).
    fn need(&mut self, height: f32) {
        if self.y + height > self.h - self.m && self.y > self.m + 0.5 {
            self.new_page();
        }
    }

    /// Lay out pieces from `x0` with width `avail`; returns nothing, moves `y`.
    fn paragraph(&mut self, pieces: &[Piece], x0: f32, avail: f32) {
        // split into words (keeping the trailing spaces with each word)
        let mut words: Vec<Piece> = Vec::new();
        for p in pieces {
            let mut cur = String::new();
            for c in p.text.chars() {
                cur.push(c);
                if c == ' ' {
                    words.push(Piece { text: std::mem::take(&mut cur), ..p.clone() });
                }
            }
            if !cur.is_empty() {
                words.push(Piece { text: cur, ..p.clone() });
            }
        }
        let width = |p: &Piece| crate::pdfwrite::text_width(p.kind, p.size, &p.text);
        let mut lines: Vec<Vec<Piece>> = vec![vec![]];
        let mut line_w = 0.0;
        for wd in words {
            let ww = width(&wd);
            if line_w + ww > avail && !lines.last().unwrap().is_empty() {
                lines.push(vec![]);
                line_w = 0.0;
                if wd.text.trim().is_empty() {
                    continue;
                }
            }
            if ww > avail {
                // a very long word: break it by characters
                let mut part = String::new();
                for c in wd.text.chars() {
                    let cw = crate::pdfwrite::char_width(wd.kind, wd.size, c);
                    if line_w + cw > avail && !(lines.last().unwrap().is_empty() && part.is_empty()) {
                        if !part.is_empty() {
                            lines.last_mut().unwrap().push(Piece { text: std::mem::take(&mut part), ..wd.clone() });
                        }
                        lines.push(vec![]);
                        line_w = 0.0;
                    }
                    part.push(c);
                    line_w += cw;
                }
                if !part.is_empty() {
                    lines.last_mut().unwrap().push(Piece { text: part, ..wd.clone() });
                }
                continue;
            }
            lines.last_mut().unwrap().push(wd);
            line_w += ww;
        }
        for line in lines {
            let size = line.iter().map(|p| p.size).fold(self.opts.font_pt, f32::max);
            let lh = size * 1.45;
            self.need(lh);
            let base = self.y + size * 1.08;
            let mut x = x0;
            for p in &line {
                let pw = width(p);
                if let Some(bg) = p.bg {
                    let pad = p.size * 0.15;
                    self.p().rect(x - pad * 0.5, base - p.size * 0.95, pw + pad, p.size * 1.25, Some(bg), None);
                }
                self.p().text(TextStyle { kind: p.kind, size: p.size, color: p.color, italic: p.italic, bold: p.bold }, x, base, &p.text);
                let tw = if p.text.ends_with(' ') && p.link.is_some() || p.text.ends_with(' ') && p.underline && line.last().map(|l| !std::ptr::eq(l, p)).unwrap_or(false) {
                    pw
                } else {
                    crate::pdfwrite::text_width(p.kind, p.size, p.text.trim_end())
                };
                if p.underline {
                    self.p().line([x, base + p.size * 0.15], [x + tw, base + p.size * 0.15], p.color, p.size * 0.06);
                }
                if p.strike {
                    self.p().line([x, base - p.size * 0.3], [x + tw, base - p.size * 0.3], p.color, p.size * 0.06);
                }
                if let Some(uri) = &p.link {
                    self.p().link(x, base - p.size, tw, p.size * 1.3, uri);
                }
                x += pw;
            }
            self.y += lh;
        }
    }

    /// Markdown inline text → styled pieces (using the reading view's parser).
    fn pieces(&self, vault: &Vault, rel: &Path, text: &str, size: f32, color: Color32, strike: bool) -> Vec<Piece> {
        let links: Vec<(markdown::LinkSpan, bool)> = markdown::find_links(text)
            .into_iter()
            .map(|l| {
                let ok = vault.exists(&l.target, Some(rel));
                (l, ok)
            })
            .collect();
        let (job, spans) = crate::reader::inline_job(text, &links, size, color, strike);
        let mut out = Vec::new();
        for s in &job.sections {
            let r = s.byte_range.start.0..s.byte_range.end.0;
            let f = &s.format;
            let link = spans.iter().find(|(sr, l, _)| sr.start <= r.start && r.end <= sr.end && crate::lists::is_web_url(&l.target)).map(|(_, l, _)| l.target.clone());
            out.push(Piece {
                text: job.text[r].to_string(),
                kind: font_kind(&f.font_id.family),
                size: f.font_id.size,
                color: print_color(f.color),
                bg: print_bg(f.background),
                italic: f.italics,
                bold: f.extra_letter_spacing > 0.0,
                underline: f.underline.width > 0.0,
                strike: f.strikethrough.width > 0.0,
                link,
            });
        }
        out
    }

    fn image(&mut self, img: image::RgbaImage, max_h: f32) {
        let (w, h) = (img.width() as f32, img.height() as f32);
        let k = (self.cw() / (w * 0.75)).min(max_h / (h * 0.75)).min(1.0) * 0.75;
        let (dw, dh) = (w * k, h * k);
        let idx = self.doc.add_image(pdf_image(&img));
        self.need(dh + 6.0);
        let (x, y) = (self.m, self.y);
        self.p().image(idx, x, y, dw, dh);
        self.y += dh + 8.0;
    }
}

/// Export a note (its current text) to PDF bytes.
pub fn export_note(vault: &Vault, rel: &Path, text: &str, opts: &ExportOptions) -> Result<Vec<u8>, String> {
    let name = vault.file(rel).map(|f| f.display_name()).unwrap_or_else(|| markdown::strip_md_ext(&rel.to_string_lossy()).to_string());
    let mut w = Writer::new(&name, opts);
    let base = opts.font_pt;
    let ink = Color32::from_rgb(30, 30, 30);
    if opts.title {
        let pieces = vec![Piece { text: name.clone(), kind: FontKind::Sans, size: base * 2.0, color: Rgba(0, 0, 0, 255), bg: None, italic: false, bold: true, underline: false, strike: false, link: None }];
        let x = w.m;
        let cw = w.cw();
        w.paragraph(&pieces, x, cw);
        w.y += base * 0.6;
    }
    let fenced = markdown::fenced_code_lines(text);
    let lines = markdown::lines_with_offsets(text);
    let mut i = 0;
    // skip front matter
    if lines.first().map(|(_, l)| l.trim() == "---").unwrap_or(false) {
        if let Some(end) = lines.iter().skip(1).position(|(_, l)| l.trim() == "---") {
            i = end + 2;
        }
    }
    while i < lines.len() {
        let (off, line) = lines[i];
        let trimmed = line.trim();
        let x0 = w.m;
        let cw = w.cw();
        if fenced.get(i).copied().unwrap_or(false) {
            // code block: collect, then draw in a grey box
            let mut code = Vec::new();
            i += 1;
            while i < lines.len() && !(lines[i].1.trim_start().starts_with("```") || lines[i].1.trim_start().starts_with("~~~")) {
                code.push(lines[i].1.replace('\t', "    "));
                i += 1;
            }
            i += 1;
            let size = base * 0.9;
            let lh = size * 1.4;
            for l in code {
                // wrap long code lines by characters
                let mut rest: String = l;
                loop {
                    let mut cut = rest.len();
                    let mut acc = 0.0;
                    for (bi, c) in rest.char_indices() {
                        acc += crate::pdfwrite::char_width(FontKind::Mono, size, c);
                        if acc > cw - 12.0 {
                            cut = bi;
                            break;
                        }
                    }
                    let (now, later) = rest.split_at(cut.max(rest.chars().next().map(|c| c.len_utf8()).unwrap_or(0).min(rest.len())));
                    w.need(lh);
                    let y = w.y;
                    w.p().rect(x0, y, cw, lh, Some(Rgba(238, 238, 240, 255)), None);
                    w.p().text(TextStyle::plain(FontKind::Mono, size, Rgba(150, 70, 0, 255)), x0 + 6.0, y + size * 1.05, now);
                    w.y += lh;
                    if later.is_empty() {
                        break;
                    }
                    rest = later.to_string();
                }
            }
            w.y += base * 0.5;
            continue;
        }
        // an embed on its own line
        let line_links = markdown::find_links(line);
        if line_links.len() == 1 && line_links[0].embed && trimmed.len() == line_links[0].range.len() {
            let target = vault.resolve(&line_links[0].target, Some(rel));
            if let (Some(t), true) = (target, opts.embeds) {
                let abs = vault.abs(&t);
                let kind = vault.file(&t).map(|f| f.kind).unwrap_or(FileKind::Other);
                let max_h = (w.h - 2.0 * w.m) * 0.8;
                if kind == FileKind::Drawing {
                    if let Ok(scene) = Scene::read_file(&abs) {
                        if let Some(b) = scene.bounds() {
                            let b = b.expand(20.0);
                            let k = (cw / b.width()).min(max_h / b.height()).min(0.75);
                            let dh = b.height() * k;
                            w.need(dh + 6.0);
                            let (x, y) = (x0 + (cw - b.width() * k) / 2.0, w.y);
                            let pics = add_pictures(&mut w.doc, &crate::drawing_pictures::images(&scene, vault, &t));
                            draw_scene(w.p(), &scene, [x, y, b.width() * k, dh], false, None, &pics);
                            w.y += dh + 8.0;
                        }
                    }
                    i += 1;
                    continue;
                } else if crate::media::is_image(&t) {
                    if let Ok(img) = image::open(&abs) {
                        w.image(img.to_rgba8(), max_h);
                        i += 1;
                        continue;
                    }
                } else if crate::pdfview::is_pdf(&t) {
                    if let Ok(r) = crate::fsutil::read(&abs).map_err(|e| e.to_string()).and_then(|d| crate::pdfview::render_pages(d, 40, 2.0)) {
                        for img in r.images {
                            let [iw, ih] = img.size;
                            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                            if let Some(rgba) = image::RgbaImage::from_raw(iw as u32, ih as u32, bytes) {
                                // PDF pages were rendered at 2× — show them at their real size
                                let scaled = image::imageops::resize(&rgba, (iw / 2).max(1) as u32 * 4 / 3, (ih / 2).max(1) as u32 * 4 / 3, image::imageops::FilterType::Triangle);
                                w.image(scaled, max_h);
                            }
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }
        if trimmed.is_empty() {
            w.y += base * 0.6;
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            w.need(base);
            let y = w.y + base * 0.5;
            w.p().line([x0, y], [x0 + cw, y], Rgba(180, 180, 180, 255), 0.8);
            w.y += base;
        } else if let Some(level) = markdown::heading_level(line) {
            w.y += base * 0.5;
            let scale = crate::editor::heading_scale(level as u8);
            let mut ps = w.pieces(vault, rel, line[level..].trim_start(), base * scale, theme::HEADING, false);
            for p in &mut ps {
                p.bold = true;
                p.color = Rgba(0, 0, 0, 255);
            }
            w.need(base * scale * 2.0);
            w.paragraph(&ps, x0, cw);
            w.y += base * 0.2;
        } else if let Some((_, done, rest, indent)) = markdown::parse_task_line(line) {
            let ind = indent as f32 * 4.0;
            let size = base;
            w.need(size * 1.45);
            let y = w.y;
            let bx = x0 + ind;
            let box_s = size * 0.8;
            let by = y + size * 0.35;
            w.p().rect(bx, by, box_s, box_s, None, Some((Rgba(90, 90, 90, 255), 0.8)));
            if done {
                w.p().path(&[[bx + box_s * 0.2, by + box_s * 0.55], [bx + box_s * 0.42, by + box_s * 0.78], [bx + box_s * 0.82, by + box_s * 0.22]], false, None, Some((Rgba(105, 65, 198, 255), 1.2)));
            }
            let ps = w.pieces(vault, rel, rest, size, ink, done);
            w.paragraph(&ps, bx + box_s + size * 0.5, cw - ind - box_s - size * 0.5);
        } else if let Some((marker, rest_off)) = list_marker(line) {
            let ind = (line.len() - line.trim_start().len()) as f32 * 4.0;
            w.need(base * 1.45);
            let y = w.y;
            w.p().text(TextStyle::plain(FontKind::Sans, base, Rgba(105, 65, 198, 255)), x0 + ind + 2.0, y + base * 1.08, &marker);
            let ps = w.pieces(vault, rel, &line[rest_off..], base, ink, false);
            let indent = ind + base * 1.6;
            w.paragraph(&ps, x0 + indent, cw - indent);
        } else if trimmed.starts_with('>') {
            let content = line[line.find('>').unwrap() + 1..].trim_start();
            let start_y = w.y;
            let start_page = w.page;
            let ps = w.pieces(vault, rel, content, base, theme::QUOTE, false);
            w.paragraph(&ps, x0 + 12.0, cw - 12.0);
            let end_y = w.y;
            if w.page == start_page {
                w.p().line([x0 + 2.0, start_y + 2.0], [x0 + 2.0, end_y - 2.0], Rgba(105, 65, 198, 180), 2.5);
            }
        } else {
            let ps = w.pieces(vault, rel, line, base, ink, false);
            w.paragraph(&ps, x0, cw);
        }
        let _ = off;
        i += 1;
    }
    finish(w.doc, opts)
}

fn list_marker(line: &str) -> Option<(String, usize)> {
    let t = line.trim_start();
    let ind = line.len() - t.len();
    if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        return Some(("•".into(), ind + 2));
    }
    let digits = t.bytes().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
        return Some((format!("{}.", &t[..digits]), ind + digits + 2));
    }
    None
}

fn finish(mut doc: Doc, opts: &ExportOptions) -> Result<Vec<u8>, String> {
    if opts.page_numbers {
        let n = doc.pages.len();
        for i in 0..n {
            let (pw, ph) = (doc.pages[i].width, doc.pages[i].height);
            let label = format!("{} / {n}", i + 1);
            let tw = crate::pdfwrite::text_width(FontKind::Sans, 9.0, &label);
            doc.page(i).text(TextStyle::plain(FontKind::Sans, 9.0, Rgba(120, 120, 120, 255)), (pw - tw) / 2.0, ph - opts.margin_mm * MM * 0.5, &label);
        }
    }
    Ok(doc.finish())
}

// ---------------------------------------------------------------- drawings

/// A picture in the PDF writer's form (alpha only when it is used).
fn pdf_image(img: &image::RgbaImage) -> Image {
    let has_alpha = img.pixels().any(|p| p[3] < 255);
    let rgb: Vec<u8> = img.pixels().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let alpha = has_alpha.then(|| img.pixels().map(|p| p[3]).collect());
    Image { width: img.width(), height: img.height(), rgb, alpha }
}

/// Add a drawing's pictures to the document: fileId → image index.
fn add_pictures(doc: &mut Doc, pictures: &HashMap<String, image::RgbaImage>) -> HashMap<String, usize> {
    pictures.iter().map(|(id, img)| (id.clone(), doc.add_image(pdf_image(img)))).collect()
}

/// Draw a scene into `area` = [x, y, width, height] (points, from the page's top-left).
/// `pictures`: the scene's pictures already added to the document (fileId → image index).
pub fn draw_scene(page: &mut Page, scene: &Scene, area: [f32; 4], dark: bool, bg: Option<Rgba>, pictures: &HashMap<String, usize>) {
    let Some(b) = scene.bounds() else { return };
    let b = b.expand(20.0);
    let zoom = (area[2] / b.width()).min(area[3] / b.height());
    let origin = Pos2::new(area[0] + (area[2] - b.width() * zoom) / 2.0, area[1] + (area[3] - b.height() * zoom) / 2.0);
    let view = View { origin, offset: -b.min.to_vec2(), zoom };
    if let Some(bg) = bg {
        page.rect(area[0], area[1], area[2], area[3], Some(bg), None);
    }
    let ctx = egui::Context::default();
    let painter = egui::Painter::new(ctx, egui::LayerId::background(), egui::Rect::EVERYTHING);
    let no_textures = crate::drawing_pictures::Pictures::new();
    for e in scene.live() {
        let picture = if e.kind == "image" { e.extra.get("fileId").and_then(|v| v.as_str()).and_then(|id| pictures.get(id)) } else { None };
        if let Some(&idx) = picture {
            let b = e.bounds();
            let (p, q) = (view.to_screen(b.min), view.to_screen(b.max));
            page.image(idx, p.x, p.y, q.x - p.x, q.y - p.y);
        } else if e.kind == "text" {
            draw_text_element(page, e, &view, dark);
        } else {
            let mut shapes = Vec::new();
            drawing::paint_element(&painter, e, &view, dark, &no_textures, &mut shapes);
            for s in &shapes {
                emit_shape(page, s);
            }
        }
    }
}

fn emit_shape(page: &mut Page, s: &Shape) {
    let col = |c: Color32| (c.a() > 0).then(|| rgba(c));
    match s {
        Shape::Vec(v) => v.iter().for_each(|s| emit_shape(page, s)),
        Shape::LineSegment { points, stroke } => {
            if let Some(c) = col(stroke.color) {
                page.line([points[0].x, points[0].y], [points[1].x, points[1].y], c, stroke.width);
            }
        }
        Shape::Path(p) => {
            let pts: Vec<[f32; 2]> = p.points.iter().map(|q| [q.x, q.y]).collect();
            let stroke = match &p.stroke.color {
                egui::epaint::ColorMode::Solid(c) if p.stroke.width > 0.0 => col(*c).map(|c| (c, p.stroke.width)),
                _ => None,
            };
            page.path(&pts, p.closed, col(p.fill), stroke);
        }
        Shape::Circle(c) => {
            page.circle(c.center.x, c.center.y, c.radius, col(c.fill), (c.stroke.width > 0.0).then(|| col(c.stroke.color).map(|k| (k, c.stroke.width))).flatten());
        }
        Shape::Rect(r) => {
            page.rect(r.rect.min.x, r.rect.min.y, r.rect.width(), r.rect.height(), col(r.fill), (r.stroke.width > 0.0).then(|| col(r.stroke.color).map(|k| (k, r.stroke.width))).flatten());
        }
        _ => {}
    }
}

fn draw_text_element(page: &mut Page, e: &Element, view: &View, dark: bool) {
    let text = e.text.clone().unwrap_or_default();
    if text.trim().is_empty() {
        return;
    }
    let kind = match crate::drawing::font_family_of_pub(e) {
        2 | 6 => FontKind::Sans,
        3 | 8 => FontKind::Mono,
        _ => FontKind::Hand,
    };
    let size = e.font_size.unwrap_or(20.0) as f32 * view.zoom;
    let mut color = drawing::parse_color(&e.stroke_color).unwrap_or(Color32::BLACK);
    if dark {
        color = drawing::dark_variant(color);
    }
    let color = Rgba(color.r(), color.g(), color.b(), (e.opacity / 100.0 * 255.0) as u8);
    let left = view.to_screen(Pos2::new(e.x as f32, e.y as f32));
    let box_w = e.width as f32 * view.zoom;
    let wrap = crate::drawing::is_label_pub(e).then_some(box_w + 2.0 * view.zoom);
    // break into lines (and wrap labels like on screen)
    let mut lines: Vec<String> = Vec::new();
    for para in text.split('\n') {
        match wrap {
            Some(max) => {
                let mut cur = String::new();
                for word in para.split_inclusive(' ') {
                    if !cur.is_empty() && crate::pdfwrite::text_width(kind, size, &(cur.clone() + word.trim_end())) > max {
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
        let bg = if dark { drawing::dark_variant(bg) } else { bg };
        page.rect(left.x - 4.0 * view.zoom, left.y - 4.0 * view.zoom, box_w + 8.0 * view.zoom, lh * lines.len() as f32 + 8.0 * view.zoom, Some(rgba(bg)), None);
    }
    let align = crate::drawing::text_align_pub(e);
    for (i, l) in lines.iter().enumerate() {
        let l = l.trim_end();
        let tw = crate::pdfwrite::text_width(kind, size, l);
        let x = match align {
            1 => left.x + (box_w - tw) / 2.0,
            2 => left.x + box_w - tw,
            _ => left.x,
        };
        let base = left.y + i as f32 * lh + size * (0.25 * 0.5) + size * crate::pdfwrite::ascent(kind).min(1.0);
        page.text(TextStyle::plain(kind, size, color), x, base, l);
    }
}

/// Export a drawing to PDF bytes. `pictures`: its pictures by fileId (`drawing_pictures::images`).
pub fn export_drawing(scene: &Scene, title: &str, opts: &ExportOptions, pictures: &HashMap<String, image::RgbaImage>) -> Result<Vec<u8>, String> {
    let b = scene.bounds().ok_or("The drawing is empty")?.expand(20.0);
    let m = opts.margin_mm * MM;
    let natural = Vec2::new(b.width(), b.height()) * 0.75; // 100% = screen pixels at 96 dpi
    let [pw, ph] = match opts.paper {
        Paper::FitDrawing => [natural.x + 2.0 * m, natural.y + 2.0 * m],
        _ => opts.page_size(),
    };
    let (aw, ah) = (pw - 2.0 * m, ph - 2.0 * m);
    let k = if opts.paper == Paper::FitDrawing {
        1.0
    } else if opts.fill_page {
        (aw / natural.x).min(ah / natural.y)
    } else {
        (aw / natural.x).min(ah / natural.y).min(1.0)
    };
    let (dw, dh) = (natural.x * k, natural.y * k);
    let mut doc = Doc::new(title);
    let p = doc.add_page(pw, ph);
    let (dark, page_bg) = match opts.background {
        Background::White => (false, Some(Rgba(255, 255, 255, 255))),
        Background::Dark => (true, Some(Rgba(18, 18, 18, 255))),
        Background::None => (false, None),
    };
    if let Some(bg) = page_bg {
        doc.page(p).rect(0.0, 0.0, pw, ph, Some(bg), None);
    }
    let pics = add_pictures(&mut doc, pictures);
    draw_scene(doc.page(p), scene, [m + (aw - dw) / 2.0, m + (ah - dh) / 2.0, dw, dh], dark, None, &pics);
    finish(doc, &ExportOptions { page_numbers: false, ..opts.clone() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdfwrite::inflate;

    fn render(bytes: &[u8]) -> Vec<(u32, u32, Vec<u8>)> {
        let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec()).expect("valid PDF");
        let cache = hayro::RenderCache::new();
        pdf.pages()
            .iter()
            .map(|pg| {
                let pm = hayro::render(pg, &cache, &Default::default(), &Default::default(), &hayro::PixmapSettings { x_scale: 0.5, y_scale: 0.5, bg_color: hayro::vello_cpu::color::palette::css::WHITE });
                (pm.width() as u32, pm.height() as u32, pm.data_as_u8_slice().to_vec())
            })
            .collect()
    }

    fn contents(bytes: &[u8]) -> String {
        // inflate every FlateDecode stream and join them (enough to look for text)
        let mut out = String::new();
        let mut i = 0;
        while let Some(p) = find(bytes, b"stream\n", i) {
            let start = p + 7;
            let Some(end) = find(bytes, b"\nendstream", start) else { break };
            out.push_str(&String::from_utf8_lossy(&inflate(&bytes[start..end])));
            i = end + b"\nendstream".len();
        }
        out
    }

    fn find(h: &[u8], n: &[u8], from: usize) -> Option<usize> {
        h[from..].windows(n.len()).position(|w| w == n).map(|p| p + from)
    }

    fn hex(s: &str) -> String {
        s.chars().filter_map(crate::pdfwrite::win_ansi).map(|b| format!("{b:02x}")).collect()
    }

    fn vault() -> (Vault, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-export-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        std::fs::write(d.join("doc.pdf"), crate::pdfview::sample_pdf()).unwrap();
        (Vault::open(&d).unwrap(), d)
    }

    fn rand_suffix() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos() as u64
    }

    // spec: PDF-001
    #[test]
    fn note_export_has_title_text_links_and_pages() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        let text = v.notes[rel].text.clone() + "\n![[doc.pdf]]\n";
        let bytes = export_note(&v, rel, &text, &ExportOptions::for_note()).unwrap();
        let pages = render(&bytes);
        assert!(pages.len() >= 2, "long note with an embedded PDF spans pages, got {}", pages.len());
        assert_eq!((pages[0].0, pages[0].1), (297, 421), "A4 portrait at half scale");
        let c = contents(&bytes);
        assert!(c.contains(&hex("Welcome")), "title text is real, selectable text");
        assert!(c.contains(&hex("blue")), "link text is there");
        assert!(String::from_utf8_lossy(&bytes).contains("/URI (https://example.com)"), "web links are clickable");
        assert!(c.contains("/Im"), "the embedded picture / PDF pages are included");
        assert!(c.contains(&hex("1 / ")), "page numbers");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-001
    #[test]
    fn note_export_options_change_the_pages() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let text = v.notes[rel].text.clone();
        let opts = ExportOptions { paper: Paper::A5, landscape: true, title: false, page_numbers: false, ..ExportOptions::for_note() };
        let bytes = export_note(&v, rel, &text, &opts).unwrap();
        let pages = render(&bytes);
        assert_eq!((pages[0].0, pages[0].1), (297, 210), "A5 landscape");
        let c = contents(&bytes);
        assert!(!c.contains(&hex("1 / 1")), "no page numbers");
        // bigger text → more pages
        let long: String = (0..60).map(|i| format!("Line number {i} with some words to wrap around the page.\n")).collect();
        let small = export_note(&v, rel, &long, &ExportOptions { font_pt: 9.0, ..ExportOptions::for_note() }).unwrap();
        let big = export_note(&v, rel, &long, &ExportOptions { font_pt: 16.0, ..ExportOptions::for_note() }).unwrap();
        assert!(render(&big).len() > render(&small).len());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DRAW-018
    #[test]
    fn drawing_pictures_are_in_the_pdf() {
        let scene = Scene::parse(r#"{"type":"excalidraw","version":2,"elements":[{"id":"a","type":"image","x":0,"y":0,"width":200,"height":100,"fileId":"f"}]}"#).unwrap();
        let red = image::RgbaImage::from_pixel(4, 2, image::Rgba([230, 20, 20, 255]));
        let reds = |bytes: &[u8]| {
            let (w, h, px) = render(bytes).remove(0);
            (0..(w * h) as usize).filter(|i| px[i * 4] > 200 && px[i * 4 + 1] < 60).count() as f32 / (w * h) as f32
        };
        let with = export_drawing(&scene, "P", &ExportOptions::for_drawing(&scene), &HashMap::from([("f".to_string(), red)])).unwrap();
        // 200×100 of a 240×140 drawing, inside the page margins
        assert!(reds(&with) > 0.25, "the picture is drawn: {} of the page is red", reds(&with));
        let without = export_drawing(&scene, "P", &ExportOptions::for_drawing(&scene), &HashMap::new()).unwrap();
        assert_eq!(reds(&without), 0.0, "a missing picture is only a dashed box");
    }

    // spec: PDF-001
    #[test]
    fn drawing_export_fit_and_paper() {
        let scene = Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap();
        let fit = export_drawing(&scene, "Sketch", &ExportOptions::for_drawing(&scene), &HashMap::new()).unwrap();
        let p = render(&fit);
        assert_eq!(p.len(), 1);
        let (w, h, px) = &p[0];
        assert!(*w > *h, "page fits the wide drawing");
        // the blue rectangle's outline is drawn somewhere on the left half
        let blue = (0..(*w * *h) as usize).any(|i| {
            let x = (i as u32) % *w;
            px[i * 4 + 2] > 150 && px[i * 4] < 80 && x < *w / 2
        });
        assert!(blue, "rectangle stroke rendered");
        let a4 = export_drawing(&scene, "Sketch", &ExportOptions { paper: Paper::A4, landscape: true, ..ExportOptions::for_drawing(&scene) }, &HashMap::new()).unwrap();
        let p = render(&a4);
        assert_eq!((p[0].0, p[0].1), (421, 297), "A4 landscape");
        let dark = export_drawing(&scene, "Sketch", &ExportOptions { background: Background::Dark, ..ExportOptions::for_drawing(&scene) }, &HashMap::new()).unwrap();
        let p = render(&dark);
        assert!(p[0].2[0] < 40, "dark background");
        assert!(String::from_utf8_lossy(&dark).contains("/MediaBox"));
        assert!(export_drawing(&Scene::default(), "x", &ExportOptions::for_drawing(&Scene::default()), &HashMap::new()).is_err());
    }
}

#[cfg(test)]
mod preview {
    /// `cargo test write_previews -- --ignored`: writes exported pages as PNGs to /tmp for a look.
    #[test]
    #[ignore]
    fn write_previews() {
        use super::*;
        let d = std::env::temp_dir().join("shale-export-preview");
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d.join("vault")).unwrap();
        let v = Vault::open(&d.join("vault")).unwrap();
        let rel = Path::new("Welcome.md");
        let note = export_note(&v, rel, &v.notes[rel].text.clone(), &ExportOptions::for_note()).unwrap();
        let scene = Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap();
        let drawing = export_drawing(&scene, "Sketch", &ExportOptions::for_drawing(&scene), &HashMap::new()).unwrap();
        for (name, bytes) in [("note", note), ("drawing", drawing)] {
            std::fs::write(d.join(format!("{name}.pdf")), &bytes).unwrap();
            let r = crate::pdfview::render_pages(bytes, 3, 1.2).unwrap();
            for (i, img) in r.images.iter().enumerate() {
                let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, raw).unwrap().save(d.join(format!("{name}-{i}.png"))).unwrap();
            }
        }
    }
}
