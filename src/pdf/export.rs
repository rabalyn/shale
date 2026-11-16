//! Export notes and drawings to PDF.
//!
//! - Notes: the markdown is laid out on pages (paper size, orientation, margins and text size
//!   are options). Text uses embedded fonts, so it stays selectable and searchable; web links
//!   are clickable; pictures, drawings and PDF pages that are embedded in the note are included.
//! - Drawings: vector output (shapes, hatching, arrows and the hand-drawn text as outlines), on a
//!   page that fits the drawing or on a paper size, with a white, dark or no background.

use crate::drawing::{self, Element, Scene, View};
use crate::markdown;
use crate::pdf::write::{Doc, FontKind, Image, Page, Rgba, TextStyle};
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
    /// notes: print tables as plain text lines instead of a grid (EDIT-024)
    pub plain_tables: bool,
    /// notes: the width in points of the editor's text column, where the note was shown
    /// (`NoteEditor::column`); embeds are as big in proportion to the text as there (PDF-005).
    /// None: the column of `embed_size::DEFAULT_COLUMN`.
    pub column: Option<f32>,
    /// drawings
    pub background: Background,
    /// drawings: scale to fill the page (otherwise 100%, shrunk only if it doesn't fit)
    pub fill_page: bool,
}

impl ExportOptions {
    pub fn for_note() -> Self {
        ExportOptions { paper: Paper::A4, landscape: false, margin_mm: 20.0, font_pt: 11.0, title: true, page_numbers: true, embeds: true, plain_tables: false, column: None, background: Background::White, fill_page: true }
    }
    pub fn for_drawing(scene: &Scene) -> Self {
        let wide = scene.bounds().map(|b| b.width() > b.height()).unwrap_or(true);
        ExportOptions { paper: Paper::FitDrawing, landscape: wide, margin_mm: 10.0, font_pt: 11.0, title: false, page_numbers: false, embeds: true, plain_tables: false, column: None, background: Background::White, fill_page: true }
    }
    /// The editor's text column the sizes of the embeds are worked out for.
    fn column(&self) -> f32 {
        self.column.unwrap_or(crate::embed_size::DEFAULT_COLUMN)
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

fn piece_width(p: &Piece) -> f32 {
    crate::pdf::write::text_width(p.kind, p.size, &p.text)
}

/// Break pieces into lines of at most `avail` points (words stay whole unless longer than a line).
fn wrap_pieces(pieces: &[Piece], avail: f32) -> Vec<Vec<Piece>> {
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
    let width = piece_width;
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
                let cw = crate::pdf::write::char_width(wd.kind, wd.size, c);
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
    lines
}

/// Space between a table cell's border and its text.
const CELL_PAD_X: f32 = 5.0;
const CELL_PAD_Y: f32 = 3.0;

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

    /// Lay out pieces from `x0` with width `avail`; moves `y`.
    fn paragraph(&mut self, pieces: &[Piece], x0: f32, avail: f32) {
        for line in wrap_pieces(pieces, avail) {
            let lh = self.line_height(&line);
            self.need(lh);
            let base = self.y + self.line_size(&line) * 1.08;
            self.draw_line(&line, x0, base);
            self.y += lh;
        }
    }

    /// A table as a grid (EDIT-024): column widths from the same rule as on screen, cells
    /// wrapped inside their column, the header filled and bold. A table that does not fit the
    /// rest of the page goes on at the next page between two rows, with the header row again.
    fn table(&mut self, vault: &Vault, rel: &Path, t: &crate::table::Table, ink: Color32) {
        let base = self.opts.font_pt;
        let x0 = self.m;
        let ncols = t.header.len();
        let rows: Vec<Vec<Vec<Piece>>> = std::iter::once(&t.header)
            .chain(&t.rows)
            .enumerate()
            .map(|(r, row)| {
                row.iter()
                    .map(|cell| {
                        let mut ps = self.pieces(vault, rel, cell, base, ink, false);
                        ps.iter_mut().for_each(|p| p.bold |= r == 0);
                        ps
                    })
                    .collect()
            })
            .collect();
        let mut natural = vec![0.0f32; ncols];
        let mut least = vec![0.0f32; ncols];
        for row in &rows {
            for (c, ps) in row.iter().enumerate() {
                natural[c] = natural[c].max(ps.iter().map(piece_width).sum::<f32>());
                for p in ps {
                    for word in p.text.split_whitespace() {
                        least[c] = least[c].max(crate::pdf::write::text_width(p.kind, p.size, word));
                    }
                }
            }
        }
        let text_w = crate::table::column_widths(&natural, &least, self.cw() - 2.0 * CELL_PAD_X * ncols as f32);
        let lines: Vec<Vec<Vec<Vec<Piece>>>> = rows.iter().map(|row| row.iter().enumerate().map(|(c, ps)| wrap_pieces(ps, text_w[c].max(1.0))).collect()).collect();
        let heights: Vec<f32> = lines.iter().map(|row| row.iter().map(|ls| ls.iter().map(|l| self.line_height(l)).sum::<f32>()).fold(base * 1.45, f32::max) + 2.0 * CELL_PAD_Y).collect();
        let widths: Vec<f32> = text_w.iter().map(|w| w + 2.0 * CELL_PAD_X).collect();
        self.need(heights[0] + heights.get(1).copied().unwrap_or(0.0));
        for r in 0..lines.len() {
            if r > 0 && self.y + heights[r] > self.h - self.m && self.y > self.m + 0.5 {
                self.new_page();
                self.table_row(x0, &widths, &t.aligns, &lines[0], heights[0], true);
            }
            self.table_row(x0, &widths, &t.aligns, &lines[r], heights[r], r == 0);
        }
        self.y += base * 0.6;
    }

    fn table_row(&mut self, x0: f32, widths: &[f32], aligns: &[crate::table::Col], cells: &[Vec<Vec<Piece>>], height: f32, header: bool) {
        let y = self.y;
        let mut x = x0;
        for (c, cell) in cells.iter().enumerate() {
            let fill = header.then_some(Rgba(236, 234, 246, 255));
            self.p().rect(x, y, widths[c], height, fill, Some((Rgba(150, 150, 150, 255), 0.6)));
            let mut top = y + CELL_PAD_Y;
            for line in cell {
                let size = self.line_size(line);
                let used = line.iter().map(piece_width).sum::<f32>() - line.last().map_or(0.0, |p| piece_width(p) - crate::pdf::write::text_width(p.kind, p.size, p.text.trim_end()));
                let room = widths[c] - 2.0 * CELL_PAD_X - used;
                let dx = match aligns[c] {
                    crate::table::Col::Left => 0.0,
                    crate::table::Col::Center => room / 2.0,
                    crate::table::Col::Right => room,
                };
                self.draw_line(line, x + CELL_PAD_X + dx.max(0.0), top + size * 1.08);
                top += self.line_height(line);
            }
            x += widths[c];
        }
        self.y += height;
    }

    fn line_size(&self, line: &[Piece]) -> f32 {
        line.iter().map(|p| p.size).fold(self.opts.font_pt, f32::max)
    }

    fn line_height(&self, line: &[Piece]) -> f32 {
        self.line_size(line) * 1.45
    }

    /// One wrapped line with its baseline at `base`, starting at `x`.
    fn draw_line(&mut self, line: &[Piece], x: f32, base: f32) {
        let mut x = x;
        for p in line {
            let pw = piece_width(p);
            if let Some(bg) = p.bg {
                let pad = p.size * 0.15;
                self.p().rect(x - pad * 0.5, base - p.size * 0.95, pw + pad, p.size * 1.25, Some(bg), None);
            }
            self.p().text(TextStyle { kind: p.kind, size: p.size, color: p.color, italic: p.italic, bold: p.bold }, x, base, &p.text);
            let tw = if p.text.ends_with(' ') && p.link.is_some() || p.text.ends_with(' ') && p.underline && line.last().map(|l| !std::ptr::eq(l, p)).unwrap_or(false) {
                pw
            } else {
                crate::pdf::write::text_width(p.kind, p.size, p.text.trim_end())
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

    /// What an embed that is `editor_size` big in the editor is in the PDF (PDF-005).
    fn page_size(&self, editor_size: Vec2) -> Vec2 {
        crate::embed_size::to_page(editor_size, self.opts.column(), self.cw(), (self.h - 2.0 * self.m) * 0.8)
    }

    /// A picture `size` big (PDF points) at the left margin, with its caption below it (PDF-004).
    fn image(&mut self, img: &image::RgbaImage, size: Vec2, caption: Option<&str>) {
        let idx = self.doc.add_image(pdf_image(img));
        self.need(size.y + 6.0 + self.caption_room(caption));
        let (x, y) = (self.m, self.y);
        self.p().image(idx, x, y, size.x, size.y);
        self.y += size.y;
        self.picture_end(caption);
    }

    /// The card of a sound or video (MEDIA-001) as the editor shows it: the same parts at the same
    /// places (`media::widget::geometry`), as big in proportion to the text, with the cover or
    /// first frame as the artwork, the name, the empty progress bar and the length.
    fn player_card(&mut self, abs: &Path, asked: Option<f32>, caption: Option<&str>) {
        use crate::media::widget;
        let video = crate::media::is_video(abs);
        let look = if video { crate::media::video::look(abs, crate::media::ffmpeg::locate).look } else { crate::media::cover::look(abs) };
        let card = widget::card_size(abs, self.opts.column(), asked);
        let size = self.page_size(card);
        let k = size.x / card.x;
        let geo = widget::geometry(abs, card.x);
        self.need(size.y + 6.0 + self.caption_room(caption));
        let (ox, oy) = (self.m, self.y);
        let at = |r: egui::Rect| [ox + r.left() * k, oy + r.top() * k, r.width() * k, r.height() * k];
        let name = abs.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let ink = Rgba(30, 30, 30, 255);
        let grey = Rgba(105, 105, 110, 255);
        let page = self.page;
        let cover = look.cover.as_ref().and_then(rgba_of).map(|img| (self.doc.add_image(pdf_image(&img)), img.width() as f32, img.height() as f32));
        let p = self.doc.page(page);
        p.rect(ox, oy, size.x, size.y, Some(Rgba(246, 246, 248, 255)), Some((Rgba(200, 200, 205, 255), 0.8)));
        let [ax, ay, aw, ah] = at(geo.art);
        match cover {
            Some((idx, iw, ih)) => {
                p.rect(ax, ay, aw, ah, Some(Rgba(20, 20, 20, 255)), None);
                let f = (aw / iw).min(ah / ih);
                p.image(idx, ax + (aw - iw * f) / 2.0, ay + (ah - ih * f) / 2.0, iw * f, ih * f);
            }
            None => {
                p.rect(ax, ay, aw, ah, Some(Rgba(226, 226, 230, 255)), None);
                let size = 11.0 * k;
                let shown = crate::caption::fit(&name, aw - 12.0 * k, |t| crate::pdf::write::text_width(FontKind::Sans, size, t));
                p.text(TextStyle::plain(FontKind::Sans, size, grey), ax + 6.0 * k, ay + ah - 8.0 * k, &shown);
            }
        }
        // the round play button over the artwork
        let (cx, cy, r) = (ax + aw / 2.0, ay + ah / 2.0, widget::BUTTON_RADIUS * k);
        p.circle(cx, cy, r, Some(Rgba(50, 50, 55, 255)), None);
        p.path(&[[cx - 0.35 * r, cy - 0.5 * r], [cx - 0.35 * r, cy + 0.5 * r], [cx + 0.55 * r, cy]], true, Some(Rgba(255, 255, 255, 255)), None);
        // name, progress bar with its times, volume
        let x0 = ox + geo.text_x * k;
        let width = size.x - (geo.text_x + 12.0) * k;
        let top = oy + geo.top * k;
        let name_size = 14.0 * k;
        let shown = crate::caption::fit(&name, width, |t| crate::pdf::write::text_width(FontKind::Sans, name_size, t));
        p.text(TextStyle::plain(FontKind::Sans, name_size, ink), x0, top + 10.0 * k + name_size, &shown);
        p.rect(x0, top + 54.0 * k, width, 4.0 * k, Some(Rgba(205, 205, 210, 255)), None);
        let time = 11.0 * k;
        let total = look.duration.map(crate::media::player::clock).unwrap_or_else(|| "–:––".into());
        p.text(TextStyle::plain(FontKind::Sans, time, grey), x0, top + 66.0 * k + time, "0:00");
        let tw = crate::pdf::write::text_width(FontKind::Sans, time, &total);
        p.text(TextStyle::plain(FontKind::Sans, time, grey), x0 + width - tw, top + 66.0 * k + time, &total);
        p.rect(x0, top + 90.0 * k, (width / k * 0.4).clamp(60.0, 120.0) * k, 4.0 * k, Some(Rgba(205, 205, 210, 255)), None);
        self.y += size.y;
        self.picture_end(caption);
    }

    /// The room the caption takes under a picture.
    fn caption_room(&self, caption: Option<&str>) -> f32 {
        caption.map_or(0.0, |_| crate::caption::height(self.opts.font_pt))
    }

    /// The end of a picture (its bottom is at `y`): the caption below it, then some space.
    fn picture_end(&mut self, caption: Option<&str>) {
        if let Some(text) = caption {
            let size = crate::caption::size(self.opts.font_pt);
            let shown = crate::caption::fit(text, self.cw(), |s| crate::pdf::write::text_width(FontKind::Sans, size, s));
            let (x, y) = (self.m, self.y + crate::caption::GAP);
            self.p().text(TextStyle::plain(FontKind::Sans, size, Rgba(105, 105, 110, 255)), x, y + size * 1.08, &shown);
            self.y += self.caption_room(caption);
        }
        self.y += 8.0;
    }
}

/// A frame or cover (as the player shows it) as an image for the PDF.
fn rgba_of(img: &egui::ColorImage) -> Option<image::RgbaImage> {
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, bytes)
}

/// A picture's size as the editor takes it: pictures over 2048 points are scaled down to it.
fn natural_size(w: u32, h: u32) -> Vec2 {
    let k = (2048.0 / w.max(h) as f32).min(1.0);
    Vec2::new(w as f32 * k, h as f32 * k)
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
    let tables = if opts.plain_tables { vec![] } else { crate::table::find_tables(text) };
    while i < lines.len() {
        if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
            w.table(vault, rel, &b.table, ink);
            i = b.lines.end;
            continue;
        }
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
                        acc += crate::pdf::write::char_width(FontKind::Mono, size, c);
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
                // the link under the thumbnail (PDF-004); the size the editor gives it (PDF-005)
                let caption = crate::caption::text(&line_links[0]);
                let asked = line_links[0].embed_width();
                let ref_w = opts.column();
                if kind == FileKind::Drawing {
                    if let Ok(scene) = Scene::read_file(&abs) {
                        if let Some(b) = scene.bounds() {
                            let b = b.expand(20.0);
                            let size = w.page_size(crate::embed_size::drawing(Vec2::new(b.width(), b.height()), ref_w));
                            w.need(size.y + 6.0 + w.caption_room(Some(&caption)));
                            let (x, y) = (x0, w.y);
                            let pics = add_pictures(&mut w.doc, &crate::drawing::pictures::images(&scene, vault, &t));
                            draw_scene(w.p(), &scene, [x, y, size.x, size.y], false, None, &pics);
                            w.y += size.y;
                            w.picture_end(Some(&caption));
                        }
                    }
                    i += 1;
                    continue;
                } else if crate::media::is_image(&t) {
                    if let Ok(img) = image::open(&abs) {
                        let img = img.to_rgba8();
                        let size = w.page_size(crate::embed_size::picture(natural_size(img.width(), img.height()), asked, ref_w));
                        w.image(&img, size, Some(&caption));
                        i += 1;
                        continue;
                    }
                } else if crate::media::is_media(&t) && abs.is_file() {
                    w.player_card(&abs, asked, Some(&caption));
                    i += 1;
                    continue;
                } else if crate::pdf::view::is_pdf(&t) {
                    if let Ok(r) = crate::fsutil::read(&abs).map_err(|e| e.to_string()).and_then(|d| crate::pdf::view::render_pages(d, 40, 2.0)) {
                        let last = r.images.len().saturating_sub(1);
                        for (n, img) in r.images.iter().enumerate() {
                            if let Some(rgba) = rgba_of(img) {
                                // the editor shows its pages rendered at 1.5; these are at 2.0
                                let size = w.page_size(crate::embed_size::pdf_page(Vec2::new(img.size[0] as f32, img.size[1] as f32) * 0.75, ref_w));
                                w.image(&rgba, size, (n == last).then_some(caption.as_str()));
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
            let tw = crate::pdf::write::text_width(FontKind::Sans, 9.0, &label);
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
    let no_textures = crate::drawing::pictures::Pictures::new();
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
                    if !cur.is_empty() && crate::pdf::write::text_width(kind, size, &(cur.clone() + word.trim_end())) > max {
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
        let tw = crate::pdf::write::text_width(kind, size, l);
        let x = match align {
            1 => left.x + (box_w - tw) / 2.0,
            2 => left.x + box_w - tw,
            _ => left.x,
        };
        let base = left.y + i as f32 * lh + size * (0.25 * 0.5) + size * crate::pdf::write::ascent(kind).min(1.0);
        page.text(TextStyle::plain(kind, size, color), x, base, l);
    }
}

/// Export a drawing to PDF bytes. `pictures`: its pictures by fileId (`drawing::pictures::images`).
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
pub(crate) mod tests {
    use super::*;
    use crate::pdf::write::inflate;

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

    pub(crate) fn contents(bytes: &[u8]) -> String {
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
        s.chars().filter_map(crate::pdf::write::win_ansi).map(|b| format!("{b:02x}")).collect()
    }

    fn vault() -> (Vault, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-export-{}-{}", std::process::id(), rand_suffix()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        std::fs::write(d.join("doc.pdf"), crate::pdf::view::sample_pdf()).unwrap();
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

    /// A vault with a picture, a sound with a cover, a sound without one, a video and a drawing.
    fn media_vault() -> (Vault, std::path::PathBuf) {
        let (_, d) = vault();
        image::RgbaImage::from_pixel(40, 30, image::Rgba([230, 20, 20, 255])).save(d.join("pic.png")).unwrap();
        for f in ["tone-cover.mp3", "tone.wav", "clip.mp4"] {
            std::fs::copy(crate::media::testdata::fixture(f), d.join(f)).unwrap();
        }
        std::fs::write(d.join("Sketch.excalidraw"), crate::drawing::SAMPLE_SCENE).unwrap();
        (Vault::open(&d).unwrap(), d)
    }

    fn export_text(v: &Vault, text: &str) -> (Vec<u8>, String) {
        let bytes = export_note(v, Path::new("Welcome.md"), text, &ExportOptions::for_note()).unwrap();
        let c = contents(&bytes);
        (bytes, c)
    }

    // spec: PDF-004
    #[test]
    fn an_embedded_picture_drawing_and_pdf_are_thumbnails_with_the_link_as_caption() {
        let (v, d) = media_vault();
        for (link, caption) in [("![[pic.png]]", "pic.png"), ("![[Sketch.excalidraw]]", "Sketch.excalidraw"), ("![[doc.pdf]]", "doc.pdf"), ("![[pic.png|Sea at dusk|200]]", "Sea at dusk")] {
            let (bytes, c) = export_text(&v, &format!("![[pic.png]]\n\n{link}\n"));
            let _ = bytes;
            assert!(c.contains("/Im") || link.contains("Sketch"), "{link}: the thumbnail is in the page");
            assert!(c.contains(&hex(caption)), "{link}: the caption says {caption}");
            assert!(!c.contains(&hex("![[")), "{link}: the raw link text is not printed");
        }
        // the wanted name only: an alias replaces the target, a width is no caption
        let (_, c) = export_text(&v, "![[pic.png|Sea at dusk|200]]\n");
        assert!(!c.contains(&hex("pic.png")), "the alias is the caption, not the target");
        // the caption is below the thumbnail, in small grey text, on the same page as it
        let (bytes, _) = export_text(&v, "![[pic.png]]\n");
        let pages = render(&bytes);
        assert_eq!(pages.len(), 1);
        let (w, h, px) = &pages[0];
        let red_rows: Vec<usize> = (0..*h as usize).filter(|y| (0..*w as usize).any(|x| px[(y * *w as usize + x) * 4] > 200 && px[(y * *w as usize + x) * 4 + 1] < 60)).collect();
        let inked = |y: usize| (0..*w as usize).any(|x| px[(y * *w as usize + x) * 4..][..3].iter().any(|c| *c < 245));
        assert!(!red_rows.is_empty(), "the red picture is drawn");
        let picture_bottom = *red_rows.last().unwrap();
        assert!((picture_bottom + 1..picture_bottom + 14).any(inked), "caption text is right below the picture (it ends at row {picture_bottom})");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The size (width, height) of every image drawn on the pages of `bytes`, in points.
    pub(crate) fn image_sizes(bytes: &[u8]) -> Vec<(f32, f32)> {
        let c = contents(bytes);
        c.lines()
            .filter(|l| l.starts_with("q ") && l.contains(" cm /Im"))
            .map(|l| {
                let n: Vec<f32> = l.split_whitespace().skip(1).take(2).filter_map(|v| v.parse().ok()).collect();
                let h: f32 = l.split_whitespace().nth(4).unwrap().parse().unwrap();
                (n[0], h)
            })
            .collect()
    }

    /// The text width of the pages of a default note export.
    fn text_width() -> f32 {
        let o = ExportOptions::for_note();
        o.page_size()[0] - 2.0 * o.margin_mm * MM
    }

    fn sized(v: &Vault, link: &str) -> Vec<(f32, f32)> {
        image_sizes(&export_text(v, &format!("{link}\n")).0)
    }

    fn near(got: f32, want: f32, what: &str) {
        assert!((got - want).abs() <= want * 0.02 + 0.6, "{what}: {got} should be {want}");
    }

    // spec: PDF-005
    #[test]
    fn a_picture_has_the_share_of_the_text_width_it_has_in_the_editor() {
        let (v, d) = media_vault();
        let col = crate::embed_size::DEFAULT_COLUMN;
        let cw = text_width();
        image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 120, 10, 255])).save(d.join("wide.png")).unwrap();
        image::RgbaImage::from_pixel(1600, 800, image::Rgba([10, 120, 10, 255])).save(d.join("huge.png")).unwrap();
        image::RgbaImage::from_pixel(100, 1000, image::Rgba([10, 120, 10, 255])).save(d.join("tall.png")).unwrap();
        let mut v = v;
        v.rescan();
        // a width in the link: half the column is half the text, and the picture keeps its shape
        let s = sized(&v, "![[wide.png|360]]")[0];
        near(s.0, cw * 0.5, "half the column");
        near(s.1, s.0 / 2.0, "the shape");
        near(sized(&v, "![[wide.png|180]]")[0].0, cw * 0.25, "a quarter");
        // no width: its own size in the editor (400 points of 720)
        near(sized(&v, "![[wide.png]]")[0].0, cw * 400.0 / col, "natural size");
        // too big for the column: as wide as the text, as the editor fits it to the column
        let s = sized(&v, "![[huge.png]]")[0];
        near(s.0, cw, "fits the text");
        near(s.1, cw / 2.0, "fit shape");
        near(sized(&v, "![[wide.png|5000]]")[0].0, cw, "a width beyond the column is the column");
        // too tall: the editor stops at 420 points (of a 720 column)
        let s = sized(&v, "![[tall.png]]")[0];
        near(s.1, 420.0 * cw / col, "the height limit of the editor");
        near(s.0, s.1 / 10.0, "tall shape");
        // a narrower page keeps the share: landscape A5 with small margins
        let o = ExportOptions { paper: Paper::A5, margin_mm: 10.0, ..ExportOptions::for_note() };
        let bytes = export_note(&v, Path::new("Welcome.md"), "![[wide.png|360]]\n", &o).unwrap();
        near(image_sizes(&bytes)[0].0, (o.page_size()[0] - 2.0 * o.margin_mm * MM) * 0.5, "half of a smaller page");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-005
    #[test]
    fn the_editors_column_is_what_the_share_is_taken_of_and_720_is_the_fallback() {
        let (v, d) = media_vault();
        image::RgbaImage::from_pixel(400, 200, image::Rgba([10, 120, 10, 255])).save(d.join("wide.png")).unwrap();
        let mut v = v;
        v.rescan();
        let cw = text_width();
        let width_in = |column: Option<f32>| {
            let o = ExportOptions { column, ..ExportOptions::for_note() };
            image_sizes(&export_note(&v, Path::new("Welcome.md"), "![[wide.png|300]]\n", &o).unwrap())[0].0
        };
        // 300 points in a column of 600 are half the text, in a column of 1200 a quarter
        near(width_in(Some(600.0)), cw * 0.5, "half");
        near(width_in(Some(1200.0)), cw * 0.25, "a quarter");
        near(width_in(Some(300.0)), cw, "all of it");
        near(width_in(Some(100.0)), cw, "never wider than the text");
        // nothing recorded: the column of 720
        assert_eq!(crate::embed_size::DEFAULT_COLUMN, 720.0);
        near(width_in(None), cw * 300.0 / 720.0, "the fallback column");
        near(width_in(None), width_in(Some(720.0)), "the same as 720 recorded");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-005
    #[test]
    fn a_sound_and_a_video_are_drawn_as_big_as_their_players_in_proportion() {
        let (v, d) = media_vault();
        let k = text_width() / crate::embed_size::DEFAULT_COLUMN;
        // the cover (64x48) fills the 96-point artwork square of the player
        let s = sized(&v, "![[tone-cover.mp3]]");
        assert_eq!(s.len(), 1, "one cover");
        near(s[0].0, 96.0 * k, "the cover's width in the player");
        near(s[0].1, 72.0 * k, "and its height");
        // without a cover the card has no picture, the file's name stays as text
        let (b, c) = export_text(&v, "![[tone.wav]]\n");
        assert!(image_sizes(&b).is_empty() && c.contains(&hex("tone.wav")) && c.contains(&hex("0:00")), "a card with the name and the times");
        // the video: the card is 640 points wide in a roomy editor, the picture 16 points less
        if crate::media::ffmpeg::locate().is_some() {
            near(sized(&v, "![[clip.mp4]]")[0].0, 624.0 * k, "the first frame in a 640 card");
            near(sized(&v, "![[clip.mp4|320]]")[0].0, 304.0 * k, "a video card as wide as its link says");
            near(sized(&v, "![[clip.mp4|5000]]")[0].0, (crate::embed_size::DEFAULT_COLUMN - 16.0) * k, "up to the column");
        } else {
            println!("skipping the video: ffmpeg is not installed");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-004
    #[test]
    fn a_sound_with_a_cover_and_a_video_show_their_thumbnail_and_a_missing_file_keeps_its_link_text() {
        let (v, d) = media_vault();
        let (_, c) = export_text(&v, "![[tone-cover.mp3]]\n");
        assert!(c.contains("/Im") && c.contains(&hex("tone-cover.mp3")), "the cover is the thumbnail");
        // a sound without a cover is its player card, as in the editor (PDF-005)
        let (_, c) = export_text(&v, "![[tone.wav]]\n");
        assert!(!c.contains("/Im") && c.contains(&hex("tone.wav")), "a card without a picture");
        assert!(!c.contains(&hex("![[")), "not the raw link");
        // a picture that cannot be read and a link to a file that is not there stay text too
        std::fs::write(v.abs(Path::new("broken.png")), b"no picture").unwrap();
        let mut v = v;
        v.rescan();
        let (_, c) = export_text(&v, "![[broken.png]]\n\n![[nothing.png]]\n");
        assert!(!c.contains("/Im") && c.contains(&hex("broken.png")) && c.contains(&hex("nothing.png")));
        // with the picture embeds switched off nothing is drawn but the link text
        let off = ExportOptions { embeds: false, ..ExportOptions::for_note() };
        let c = contents(&export_note(&v, Path::new("Welcome.md"), "![[pic.png]]\n", &off).unwrap());
        assert!(!c.contains("/Im") && c.contains(&hex("pic.png")));
        // the first frame of a video, when ffmpeg is there
        if crate::media::ffmpeg::locate().is_some() {
            let (_, c) = export_text(&v, "![[clip.mp4]]\n");
            assert!(c.contains("/Im") && c.contains(&hex("clip.mp4")), "the first frame is the thumbnail");
        } else {
            println!("skipping the video: ffmpeg is not installed");
        }
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

    /// The x of the text drawn as `word` (one piece) in the page content.
    fn x_of(content: &str, word: &str) -> f32 {
        let at = content.find(&format!("<{}> Tj", hex(word))).unwrap_or_else(|| panic!("{word} is not drawn"));
        let head = &content[..at];
        let toks: Vec<&str> = head.split_whitespace().collect();
        toks[toks.len() - 3].parse().unwrap()
    }

    // spec: EDIT-024, PDF-001
    #[test]
    fn a_table_is_a_grid_in_the_pdf_and_plain_text_lines_when_asked() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let text = "Intro\n\n| Where | What |\n|---|--:|\n| `a.json` | **yes** [[Ideas]] |\n\nAfter\n";
        let grid = export_note(&v, rel, text, &ExportOptions::for_note()).unwrap();
        let c = contents(&grid);
        for w in ["Where", "What", "a.json", "yes", "Ideas"] {
            assert!(c.contains(&hex(w)), "cell text {w} is drawn");
        }
        assert!(!c.contains(&hex("|---|--:|")), "the delimiter row is not printed");
        assert!(c.contains(" h B") && c.contains(" h S"), "a filled header cell and bordered cells");
        let plain = export_note(&v, rel, text, &ExportOptions { plain_tables: true, ..ExportOptions::for_note() }).unwrap();
        let c = contents(&plain);
        assert!(c.contains(&hex("|---|--:|")), "plain text: the raw lines");
        assert!(!c.contains(" h B"), "and no grid");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-024, PDF-001
    #[test]
    fn table_columns_follow_the_alignment_and_long_cells_wrap_in_their_column() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let long = "words ".repeat(120);
        let text = format!("| LeftHeadLong | CtrHeadLong | RightHeadLong |\n|:--|:-:|--:|\n| l | c | r |\n| {long}| c | r |\n");
        let bytes = export_note(&v, rel, &text, &ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() }).unwrap();
        let c = contents(&bytes);
        let (hl, hc, hr) = (x_of(&c, "LeftHeadLong"), x_of(&c, "CtrHeadLong"), x_of(&c, "RightHeadLong"));
        let (l, ce, r) = (x_of(&c, "l"), x_of(&c, "c"), x_of(&c, "r"));
        assert!((l - hl).abs() < 0.5, "left aligned: {l} {hl}");
        assert!(ce > hc + 10.0, "centred: {ce} {hc}");
        assert!(r > hr + 30.0, "right aligned: {r} {hr}");
        assert!(hr < 595.0 - 20.0 * MM + 1.0, "inside the page");
        // the long cell wraps in its column: the table is a few lines tall but still one page wide
        assert!(c.matches(&format!("<{}> Tj", hex("words "))).count() >= 119, "all the words are drawn");
        assert_eq!(render(&bytes).len(), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-024, PDF-001
    #[test]
    fn a_long_table_breaks_between_rows_and_repeats_its_header() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let mut text = String::from("| Hdrcol | Other |\n|---|---|\n");
        for i in 0..150 {
            text.push_str(&format!("| row{i} | x |\n"));
        }
        let bytes = export_note(&v, rel, &text, &ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() }).unwrap();
        let pages = render(&bytes).len();
        assert!(pages >= 3, "150 rows need several pages, got {pages}");
        let c = contents(&bytes);
        assert_eq!(c.matches(&format!("<{}> Tj", hex("Hdrcol"))).count(), pages, "the header row is at the top of every page");
        for i in [0, 75, 149] {
            assert_eq!(c.matches(&format!("<{}> Tj", hex(&format!("row{i}")))).count(), 1, "row {i} is drawn once");
        }
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
            let r = crate::pdf::view::render_pages(bytes, 3, 1.2).unwrap();
            for (i, img) in r.images.iter().enumerate() {
                let raw: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                image::RgbaImage::from_raw(img.size[0] as u32, img.size[1] as u32, raw).unwrap().save(d.join(format!("{name}-{i}.png"))).unwrap();
            }
        }
    }
}
