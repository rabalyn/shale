//! Export notes and drawings to PDF.
//!
//! - Notes: the markdown is laid out on pages (paper size, orientation, margins and text size
//!   are options). Text uses embedded fonts, so it stays selectable and searchable; web links
//!   are clickable; pictures, drawings and PDF pages that are embedded in the note are included.
//! - Drawings: vector output (shapes, hatching, arrows and the hand-drawn text as outlines), on a
//!   page that fits the drawing or on a paper size, with a white, dark or no background.

use crate::drawing::{self, Element, Scene, View};
use crate::markdown;
use crate::pdf::write::{Bookmark, Dest, Doc, FontKind, Image, Page, Rgba, TextStyle};
use crate::theme;
use crate::vault::{FileKind, Vault};
use eframe::egui::{self, Color32, Pos2, Shape, Vec2};
use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

pub(super) const MM: f32 = 72.0 / 25.4;

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
    /// notes: a *Contents* page with the `#`–`###` headings and the pages they start on (PDF-008)
    pub toc: bool,
    /// notes: the setting for numbered headings (CFG-042); a note's front matter overrides it
    pub numbered_headings: bool,
    /// notes: the width in points of the editor's text column, where the note was shown
    /// (`NoteEditor::column`); embeds are as big in proportion to the text as there (PDF-005).
    /// None: the column of `embed_size::DEFAULT_COLUMN`.
    pub column: Option<f32>,
    /// a book (BOOK-005): every part and chapter starts on a new page
    pub new_page_each: bool,
    /// drawings
    pub background: Background,
    /// drawings: scale to fill the page (otherwise 100%, shrunk only if it doesn't fit)
    pub fill_page: bool,
}

impl ExportOptions {
    pub fn for_note() -> Self {
        ExportOptions {
            paper: Paper::A4,
            landscape: false,
            margin_mm: 20.0,
            font_pt: 11.0,
            title: true,
            page_numbers: true,
            embeds: true,
            plain_tables: false,
            toc: true,
            numbered_headings: true,
            column: None,
            new_page_each: true,
            background: Background::White,
            fill_page: true,
        }
    }
    pub fn for_drawing(scene: &Scene) -> Self {
        let wide = scene.bounds().map(|b| b.width() > b.height()).unwrap_or(true);
        ExportOptions {
            paper: Paper::FitDrawing,
            landscape: wide,
            margin_mm: 10.0,
            font_pt: 11.0,
            title: false,
            page_numbers: false,
            embeds: true,
            plain_tables: false,
            toc: false,
            numbered_headings: false,
            column: None,
            new_page_each: false,
            background: Background::White,
            fill_page: true,
        }
    }
    /// The editor's text column the sizes of the embeds are worked out for.
    pub(crate) fn column(&self) -> f32 {
        self.column.unwrap_or(crate::embed_size::DEFAULT_COLUMN)
    }
    pub(super) fn page_size(&self) -> [f32; 2] {
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
pub(crate) fn print_color(c: Color32) -> Rgba {
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

/// A colour with transparency laid over white paper.
pub(crate) fn over_white([r, g, b, a]: [u8; 4]) -> Rgba {
    let mix = |c: u8| ((c as u32 * a as u32 + 255 * (255 - a as u32)) / 255) as u8;
    Rgba(mix(r), mix(g), mix(b), 255)
}

pub(crate) fn print_bg(c: Color32) -> Option<Rgba> {
    if c == Color32::TRANSPARENT {
        None
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

/// Where a clickable piece of text goes.
#[derive(Clone)]
enum Link {
    /// a web address (PDF-001)
    Web(String),
    /// a note of the book and maybe a heading of it (BOOK-009)
    Book(PathBuf, Option<String>),
}

#[derive(Clone)]
pub(super) struct Piece {
    pub(super) text: String,
    kind: FontKind,
    size: f32,
    color: Rgba,
    bg: Option<Rgba>,
    italic: bool,
    bold: bool,
    underline: bool,
    strike: bool,
    link: Option<Link>,
    /// 1 raised and 2 lowered, the text being smaller (EDIT-051, EDIT-050); 0 on the line
    script: i8,
    /// the label of the footnote whose definition this is a link to (EDIT-050)
    jump: Option<String>,
    /// a typeset formula that takes the place of the text (PDF-012)
    math: Option<std::sync::Arc<crate::math::Formula>>,
}

impl Piece {
    /// Plain sans text of a colour, bold or not.
    pub(super) fn plain(text: &str, size: f32, color: Rgba, bold: bool) -> Piece {
        Piece { text: text.to_string(), kind: FontKind::Sans, size, color, bg: None, italic: false, bold, underline: false, strike: false, link: None, script: 0, jump: None, math: None }
    }
}

fn piece_width(p: &Piece) -> f32 {
    match &p.math {
        Some(f) => f.layout.width,
        None => crate::pdf::write::text_width(p.kind, p.size, &p.text),
    }
}

/// The width of a piece without the blanks at its end (a formula has none).
fn trimmed_width(p: &Piece) -> f32 {
    match &p.math {
        Some(f) => f.layout.width,
        None => crate::pdf::write::text_width(p.kind, p.size, p.text.trim_end()),
    }
}

/// Break pieces into lines of at most `avail` points (words stay whole unless longer than a line).
pub(super) fn wrap_pieces(pieces: &[Piece], avail: f32) -> Vec<Vec<Piece>> {
    // split into words (keeping the trailing spaces with each word)
    let mut words: Vec<Piece> = Vec::new();
    for p in pieces {
        if p.math.is_some() {
            // a formula is one word
            words.push(p.clone());
            continue;
        }
        let mut cur = String::new();
        for c in p.text.chars() {
            if c == '\n' {
                // a line break (`<br>` in a table cell, EDIT-045): a word of its own
                if !cur.is_empty() {
                    words.push(Piece { text: std::mem::take(&mut cur), ..p.clone() });
                }
                words.push(Piece { text: "\n".into(), ..p.clone() });
                continue;
            }
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
        if wd.text == "\n" {
            lines.push(vec![]);
            line_w = 0.0;
            continue;
        }
        let ww = width(&wd);
        if line_w + ww > avail && !lines.last().unwrap().is_empty() {
            lines.push(vec![]);
            line_w = 0.0;
            if wd.text.trim().is_empty() {
                continue;
            }
        }
        if ww > avail && wd.math.is_none() {
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

/// The height of a line of the *Contents* list, in text sizes.
const CONTENTS_LINE: f32 = 1.7;

/// A line of the *Contents* list: the heading's level, its text with its number, the place it is at.
pub(super) struct Entry {
    pub(super) level: usize,
    pub(super) label: String,
    pub(super) dest: Option<Dest>,
    /// a part of a book is set in bold
    pub(super) bold: bool,
}

/// `text` shortened with an ellipsis to fit `room` points.
pub(super) fn fit_text(text: &str, size: f32, room: f32) -> String {
    let mut out = text.to_string();
    if crate::pdf::write::text_width(FontKind::Sans, size, &out) <= room {
        return out;
    }
    while !out.is_empty() && crate::pdf::write::text_width(FontKind::Sans, size, &format!("{out}…")) > room {
        out.pop();
    }
    format!("{}…", out.trim_end())
}

/// Space between a table cell's border and its text.
const CELL_PAD_X: f32 = 5.0;
const CELL_PAD_Y: f32 = 3.0;

/// A link from a place on a page to a note of the book, found out when the book is done (BOOK-009).
pub(super) struct BookLink {
    pub(super) page: usize,
    /// x, y, width, height from the page's top left
    pub(super) rect: [f32; 4],
    pub(super) rel: PathBuf,
    pub(super) heading: Option<String>,
}

/// What the running header of a page says (BOOK-008).
#[derive(Clone, Default)]
pub(super) struct PageHead {
    /// the title of the chapter the page belongs to; none: no header
    pub(super) title: Option<String>,
    /// the first page of a part or chapter has none
    pub(super) first: bool,
}

pub(super) struct Writer<'a> {
    pub(super) doc: Doc,
    pub(super) opts: &'a ExportOptions,
    pub(super) w: f32,
    pub(super) h: f32,
    pub(super) m: f32,
    pub(super) page: usize,
    pub(super) y: f32,
    /// a book: the notes in it; a link to one of them is a link inside the PDF (BOOK-009)
    pub(super) members: Option<HashSet<PathBuf>>,
    pub(super) book_links: Vec<BookLink>,
    /// a book: the header of every page (BOOK-008), and the one the next new page gets
    pub(super) heads: Vec<PageHead>,
    pub(super) running: Option<String>,
    /// the links to footnote definitions drawn so far: page, the box and the label (PDF-011)
    pub(super) jumps: Vec<(usize, [f32; 4], String)>,
}

impl<'a> Writer<'a> {
    pub(super) fn new(title: &str, opts: &'a ExportOptions) -> Self {
        let [w, h] = opts.page_size();
        let mut doc = Doc::new(title);
        let page = doc.add_page(w, h);
        let m = opts.margin_mm * MM;
        Writer { doc, opts, w, h, m, page, y: m, members: None, book_links: vec![], heads: vec![PageHead::default()], running: None, jumps: Vec::new() }
    }
    pub(super) fn cw(&self) -> f32 {
        self.w - 2.0 * self.m
    }
    pub(super) fn p(&mut self) -> &mut Page {
        self.doc.page(self.page)
    }
    pub(super) fn new_page(&mut self) {
        self.page = self.doc.add_page(self.w, self.h);
        self.y = self.m;
        self.heads.push(PageHead { title: self.running.clone(), first: false });
    }
    /// Make room for `height`; start a new page if it doesn't fit (unless the page is empty).
    pub(super) fn need(&mut self, height: f32) {
        if self.y + height > self.h - self.m && self.y > self.m + 0.5 {
            self.new_page();
        }
    }

    /// Room for the *Contents* list (PDF-008): its title is printed now (under the note's title,
    /// when there is one), and each of the `count` entries gets a line (page and distance from
    /// the page's top), going on at new pages as needed. The note's text starts on a new page after
    /// them. The entries are drawn later by [`Writer::draw_contents`], when the pages of the
    /// headings are known.
    pub(super) fn reserve_contents(&mut self, count: usize) -> Vec<(usize, f32)> {
        self.reserve_contents_gapped(&vec![0.0; count])
    }

    /// [`Writer::reserve_contents`] with some more room above each line (in points).
    pub(super) fn reserve_contents_gapped(&mut self, gaps: &[f32]) -> Vec<(usize, f32)> {
        let base = self.opts.font_pt;
        if self.y > self.m + 0.5 {
            self.y += base * 0.8;
        }
        let head =
            vec![Piece { text: "Contents".into(), kind: FontKind::Sans, size: base * 1.5, color: Rgba(0, 0, 0, 255), bg: None, italic: false, bold: true, underline: false, strike: false, link: None, script: 0, jump: None, math: None }];
        let (x, cw) = (self.m, self.cw());
        self.paragraph(&head, x, cw);
        self.y += base * 0.6;
        let mut slots = Vec::with_capacity(gaps.len());
        for gap in gaps {
            if self.y + gap + base * CONTENTS_LINE > self.h - self.m {
                self.new_page();
            } else {
                self.y += gap;
            }
            slots.push((self.page, self.y));
            self.y += base * CONTENTS_LINE;
        }
        self.new_page();
        slots
    }

    /// The entries of the *Contents* list in the room [`Writer::reserve_contents`] made: each
    /// indented by its level, with its number, a dotted leader and the page it starts on,
    /// right-aligned; the whole line is a link to that page.
    pub(super) fn draw_contents(&mut self, entries: &[Entry], slots: &[(usize, f32)]) {
        let size = self.opts.font_pt;
        let ink = Rgba(30, 30, 30, 255);
        let grey = Rgba(120, 120, 120, 255);
        let (left, right) = (self.m, self.m + self.cw());
        let shallowest = entries.iter().map(|e| e.level).min().unwrap_or(1);
        for (e, (page, top)) in entries.iter().zip(slots) {
            let x = left + (e.level - shallowest) as f32 * size * 1.4;
            let number = e.dest.map(|d| (d.page + 1).to_string()).unwrap_or_default();
            let nw = crate::pdf::write::text_width(FontKind::Sans, size, &number);
            let style = |c| TextStyle { bold: e.bold, ..TextStyle::plain(FontKind::Sans, size, c) };
            let room = right - x - nw - size * 1.5;
            let label = fit_text(&e.label, size, room);
            let lw = crate::pdf::write::text_width(FontKind::Sans, size, &label) + if e.bold { size * 0.3 } else { 0.0 };
            let base = top + size * 1.08;
            let unit = crate::pdf::write::text_width(FontKind::Sans, size, ". ");
            let dots = (((right - nw - size * 0.5) - (x + lw + size * 0.5)) / unit).floor().max(0.0) as usize;
            let p = self.doc.page(*page);
            p.text(style(ink), x, base, &label);
            if dots > 0 {
                p.text(style(grey), right - nw - size * 0.5 - dots as f32 * unit, base, &". ".repeat(dots));
            }
            if let Some(d) = e.dest {
                p.text(style(ink), right - nw, base, &number);
                p.jump(left, *top, right - left, size * CONTENTS_LINE, d);
            }
        }
    }

    /// Lay out pieces from `x0` with width `avail`; moves `y`.
    pub(super) fn paragraph(&mut self, pieces: &[Piece], x0: f32, avail: f32) {
        for line in wrap_pieces(pieces, avail) {
            let lh = self.line_height(&line);
            self.need(lh);
            let base = self.y + self.line_ascent(&line);
            self.draw_line(&line, x0, base);
            self.y += lh;
        }
    }

    /// A table as a grid (EDIT-024): column widths from the table's stored sizes in proportion
    /// (EDIT-031) or else from the same rule as on screen, cells
    /// wrapped inside their column, the header filled and bold. A table that does not fit the
    /// rest of the page goes on at the next page between two rows, with the header row again.
    fn table(&mut self, vault: &Vault, rel: &Path, t: &crate::table::Table, sizes: &crate::table::Sizes, ink: Color32) {
        let base = self.opts.font_pt;
        let x0 = self.m;
        let ncols = t.header.len();
        let rows: Vec<Vec<Vec<Piece>>> = std::iter::once(&t.header)
            .chain(&t.rows)
            .enumerate()
            .map(|(r, row)| {
                row.iter()
                    .map(|cell| {
                        let mut ps = self.cell_pieces(vault, rel, cell, base, ink);
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
                natural[c] = natural[c].max(wrap_pieces(ps, f32::INFINITY).iter().map(|l| l.iter().map(piece_width).sum::<f32>()).fold(0.0, f32::max));
                for p in ps {
                    if p.math.is_some() {
                        least[c] = least[c].max(piece_width(p));
                    }
                    for word in p.text.split_whitespace() {
                        least[c] = least[c].max(crate::pdf::write::text_width(p.kind, p.size, word));
                    }
                }
            }
        }
        let text_w = match crate::table::stored_widths(&sizes.cols, ncols, self.cw()) {
            Some(w) => w.iter().map(|w| (w - 2.0 * CELL_PAD_X).max(1.0)).collect(),
            None => crate::table::column_widths(&natural, &least, self.cw() - 2.0 * CELL_PAD_X * ncols as f32),
        };
        let lines: Vec<Vec<Vec<Vec<Piece>>>> = rows.iter().map(|row| row.iter().enumerate().map(|(c, ps)| wrap_pieces(ps, text_w[c].max(1.0))).collect()).collect();
        let rows_fit = sizes.rows.len() == lines.len() && sizes.rows.iter().all(|h| h.is_finite());
        let heights: Vec<f32> = lines
            .iter()
            .enumerate()
            .map(|(r, row)| {
                let h = row.iter().map(|ls| ls.iter().map(|l| self.line_height(l)).sum::<f32>()).fold(base * 1.45, f32::max) + 2.0 * CELL_PAD_Y;
                if rows_fit {
                    h.max(sizes.rows[r])
                } else {
                    h
                }
            })
            .collect();
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
                let used = line.iter().map(piece_width).sum::<f32>() - line.last().map_or(0.0, |p| piece_width(p) - trimmed_width(p));
                let room = widths[c] - 2.0 * CELL_PAD_X - used;
                let dx = match aligns[c] {
                    crate::table::Col::Left => 0.0,
                    crate::table::Col::Center => room / 2.0,
                    crate::table::Col::Right => room,
                };
                self.draw_line(line, x + CELL_PAD_X + dx.max(0.0), top + self.line_ascent(line).max(size * 1.08));
                top += self.line_height(line);
            }
            x += widths[c];
        }
        self.y += height;
    }

    /// The text of a note laid out from here on (PDF-001…PDF-008): the paragraphs, lists, tables,
    /// code, embeds and headings. `numbers` are the numbers printed before headings, by line;
    /// `levels` (a book chapter, BOOK-006) say what level a heading is printed at, by line, else
    /// it is as big as the `#`s say. Returns where each heading is, by line.
    pub(super) fn note_body(&mut self, vault: &Vault, rel: &Path, text: &str, numbers: &HashMap<usize, String>, levels: Option<&HashMap<usize, usize>>) -> HashMap<usize, Dest> {
        let opts = self.opts;
        let base = opts.font_pt;
        let ink = Color32::from_rgb(30, 30, 30);
        let mut places: HashMap<usize, Dest> = HashMap::new();
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
        let formulas = markdown::find_math(text);
        // the footnotes (PDF-011): a reference with a definition is a mark that links to it
        let notes = markdown::find_footnotes(text);
        let _defined = markdown::DefinedFootnotes::set(notes.defined());
        let mut footnote_places: HashMap<String, Dest> = HashMap::new();
        let first_jump = self.jumps.len();
        // a line that goes on the paragraph above it (PDF-009): text, no blank, heading, rule, list or
        // task marker, quote, table, fence or embed of its own
        let continues = |j: usize| -> bool {
            let Some((_, l)) = lines.get(j) else { return false };
            let t = l.trim();
            if t.is_empty() || fenced.get(j).copied().unwrap_or(false) || tables.iter().any(|b| b.lines.contains(&j)) {
                return false;
            }
            // a footnote definition, its continuation and a formula on lines of its own are blocks (PDF-011, PDF-012)
            if notes.def_at(j).is_some() || notes.is_continuation(j) || formulas.iter().any(|m| m.block && m.lines.contains(&j)) {
                return false;
            }
            if matches!(t, "---" | "***" | "___") || markdown::heading_level(l).is_some() || markdown::parse_task_line(l).is_some() || list_marker(l).is_some() || t.starts_with('>') {
                return false;
            }
            let links = markdown::find_links(l);
            !(links.len() == 1 && links[0].embed && t.len() == links[0].range.len())
        };
        // a line that ends in two spaces or a backslash is a hard break: the next line starts a new one
        let hard = |j: usize| lines[j].1.ends_with("  ") || lines[j].1.ends_with('\\');
        // the text of line `i` with the lines that go on it joined by a space, and how many they are;
        // `indented`: only lines that start with a space (the continuation of a list item)
        let gather = |i: usize, first: &str, indented: bool| -> (String, usize) {
            let (mut text, mut n) = (first.to_string(), 0);
            while !hard(i + n) && continues(i + n + 1) && (!indented || lines[i + n + 1].1.starts_with(char::is_whitespace)) {
                n += 1;
                text = format!("{} {}", text.trim_end().trim_end_matches('\\'), lines[i + n].1.trim());
            }
            (text, n)
        };
        let mut extra;
        while i < lines.len() {
            extra = 0;
            if let Some(b) = tables.iter().find(|b| b.lines.start == i) {
                self.table(vault, rel, &b.table, &b.meta.sizes, ink);
                i = b.lines.end;
                continue;
            }
            let (off, line) = lines[i];
            let trimmed = line.trim();
            let x0 = self.m;
            let cw = self.cw();
            // a formula on lines of its own: centred, or printed as code when it cannot be typeset (PDF-012)
            if let Some(m) = formulas.iter().find(|m| m.block && m.lines.start == i) {
                self.display_math(&text[m.tex.clone()], base);
                i = m.lines.end;
                continue;
            }
            if fenced.get(i).copied().unwrap_or(false) {
                // code block: collect, then draw on the code background (PDF-007)
                let word = crate::syntax::fence_language(line);
                let mut code = Vec::new();
                i += 1;
                while i < lines.len() && !(lines[i].1.trim_start().starts_with("```") || lines[i].1.trim_start().starts_with("~~~")) {
                    code.push(lines[i].1);
                    i += 1;
                }
                i += 1;
                self.code_block(&word, &code, base * 0.9);
                self.y += base * 0.5;
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
                                let size = self.page_size(crate::embed_size::drawing(Vec2::new(b.width(), b.height()), ref_w));
                                self.need(size.y + 6.0 + self.caption_room(Some(&caption)));
                                let (x, y) = (x0, self.y);
                                let pics = add_pictures(&mut self.doc, &crate::drawing::pictures::images(&scene, vault, &t));
                                draw_scene(self.p(), &scene, [x, y, size.x, size.y], false, None, &pics);
                                self.y += size.y;
                                self.picture_end(Some(&caption));
                            }
                        }
                        i += 1;
                        continue;
                    } else if crate::media::is_image(&t) {
                        if let Ok(img) = image::open(&abs) {
                            let img = img.to_rgba8();
                            let size = self.page_size(crate::embed_size::picture(natural_size(img.width(), img.height()), asked, ref_w));
                            self.image(&img, size, Some(&caption));
                            i += 1;
                            continue;
                        }
                    } else if crate::media::is_media(&t) && abs.is_file() {
                        self.player_card(&abs, asked, Some(&caption));
                        i += 1;
                        continue;
                    } else if crate::pdf::view::is_pdf(&t) {
                        if let Ok(r) = crate::fsutil::read(&abs).map_err(|e| e.to_string()).and_then(|d| crate::pdf::view::render_pages(d, 40, 2.0)) {
                            let last = r.images.len().saturating_sub(1);
                            for (n, img) in r.images.iter().enumerate() {
                                if let Some(rgba) = rgba_of(img) {
                                    // the editor shows its pages rendered at 1.5; these are at 2.0
                                    let size = self.page_size(crate::embed_size::pdf_page(Vec2::new(img.size[0] as f32, img.size[1] as f32) * 0.75, ref_w));
                                    self.image(&rgba, size, (n == last).then_some(caption.as_str()));
                                }
                            }
                            i += 1;
                            continue;
                        }
                    }
                }
            }
            if trimmed.is_empty() {
                self.y += base * 0.6;
            } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
                self.need(base);
                let y = self.y + base * 0.5;
                self.p().line([x0, y], [x0 + cw, y], Rgba(180, 180, 180, 255), 0.8);
                self.y += base;
            } else if let Some(def) = notes.def_at(i) {
                // a footnote definition where it stands: its label raised, its text a little smaller
                let links = Writer::links_of(vault, rel, line);
                let ps = self.pieces_from(vault, rel, crate::reader::footnote_def_painted(line, &def.label, &def.marker, &links, base, ink), base);
                self.need(base * 1.45);
                footnote_places.entry(def.label.clone()).or_insert(Dest { page: self.page, y: self.y });
                self.paragraph(&ps, x0, cw);
            } else if notes.is_continuation(i) {
                let links = Writer::links_of(vault, rel, line);
                let ps = self.pieces_from(vault, rel, crate::reader::footnote_text_painted(line, &links, base, ink), base);
                self.paragraph(&ps, x0 + base * 1.2, cw - base * 1.2);
            } else if let Some(level) = markdown::heading_level(line) {
                self.y += base * 0.5;
                let scale = match levels.and_then(|l| l.get(&i)) {
                    Some(printed) => book_scale(*printed),
                    None => crate::editor::heading_scale(level as u8),
                };
                let mut ps = self.pieces(vault, rel, line[level..].trim_start(), base * scale, theme::HEADING, false);
                for p in &mut ps {
                    p.bold = true;
                    p.color = Rgba(0, 0, 0, 255);
                }
                if let Some(n) = numbers.get(&i) {
                    ps.insert(
                        0,
                        Piece {
                            text: format!("{n} "),
                            kind: FontKind::Sans,
                            size: base * scale,
                            color: Rgba(130, 130, 130, 255),
                            bg: None,
                            italic: false,
                            bold: false,
                            underline: false,
                            strike: false,
                            link: None,
                            script: 0,
                            jump: None,
                            math: None,
                        },
                    );
                }
                self.need(base * scale * 2.0);
                places.insert(i, Dest { page: self.page, y: self.y });
                self.paragraph(&ps, x0, cw);
                self.y += base * 0.2;
            } else if let Some((_, done, rest, indent)) = markdown::parse_task_line(line) {
                let ind = indent as f32 * 4.0;
                let size = base;
                self.need(size * 1.45);
                let y = self.y;
                let bx = x0 + ind;
                let box_s = size * 0.8;
                let by = y + size * 0.35;
                self.p().rect(bx, by, box_s, box_s, None, Some((Rgba(90, 90, 90, 255), 0.8)));
                if done {
                    self.p().path(&[[bx + box_s * 0.2, by + box_s * 0.55], [bx + box_s * 0.42, by + box_s * 0.78], [bx + box_s * 0.82, by + box_s * 0.22]], false, None, Some((Rgba(105, 65, 198, 255), 1.2)));
                }
                let (rest, n) = gather(i, rest, true);
                extra = n;
                let ps = self.pieces(vault, rel, &rest, size, ink, done);
                self.paragraph(&ps, bx + box_s + size * 0.5, cw - ind - box_s - size * 0.5);
            } else if let Some((marker, rest_off)) = list_marker(line) {
                let ind = (line.len() - line.trim_start().len()) as f32 * 4.0;
                self.need(base * 1.45);
                let y = self.y;
                self.p().text(TextStyle::plain(FontKind::Sans, base, Rgba(105, 65, 198, 255)), x0 + ind + 2.0, y + base * 1.08, &marker);
                let (rest, n) = gather(i, &line[rest_off..], true);
                extra = n;
                let ps = self.pieces(vault, rel, &rest, base, ink, false);
                let indent = ind + base * 1.6;
                self.paragraph(&ps, x0 + indent, cw - indent);
            } else if trimmed.starts_with('>') {
                let mut content = line[line.find('>').unwrap() + 1..].trim_start().to_string();
                // the lines of a quote that go on, with or without their own `>`
                while !hard(i + extra) {
                    let next = lines.get(i + extra + 1).map(|(_, l)| l.trim_start());
                    let more = match next {
                        Some(n) if n.starts_with('>') => Some(n[1..].trim()).filter(|c| !c.is_empty()),
                        Some(_) if continues(i + extra + 1) => Some(lines[i + extra + 1].1.trim()),
                        _ => None,
                    };
                    let Some(more) = more else { break };
                    extra += 1;
                    content = format!("{} {more}", content.trim_end().trim_end_matches('\\'));
                }
                let start_y = self.y;
                let start_page = self.page;
                let ps = self.pieces(vault, rel, &content, base, theme::QUOTE, false);
                self.paragraph(&ps, x0 + 12.0, cw - 12.0);
                let end_y = self.y;
                if self.page == start_page {
                    self.p().line([x0 + 2.0, start_y + 2.0], [x0 + 2.0, end_y - 2.0], Rgba(105, 65, 198, 180), 2.5);
                }
            } else {
                let (text, n) = gather(i, line, false);
                extra = n;
                let ps = self.pieces(vault, rel, &text, base, ink, false);
                self.paragraph(&ps, x0, cw);
            }
            let _ = off;
            i += 1 + extra;
        }
        // the references of footnotes link to their definitions in this note
        for (page, [x, y, width, height], label) in self.jumps.split_off(first_jump) {
            if let Some(to) = footnote_places.get(&label) {
                self.doc.page(page).jump(x, y, width, height, *to);
            }
        }
        places
    }

    pub(super) fn line_size(&self, line: &[Piece]) -> f32 {
        line.iter().map(|p| p.size).fold(self.opts.font_pt, f32::max)
    }

    /// How far the baseline of a line is below its top: the text's, or what a formula needs above
    /// its baseline (PDF-012).
    fn line_ascent(&self, line: &[Piece]) -> f32 {
        line.iter().filter_map(|p| p.math.as_ref()).map(|f| f.layout.ascent + 1.0).fold(self.line_size(line) * 1.08, f32::max)
    }

    pub(super) fn line_height(&self, line: &[Piece]) -> f32 {
        let below = line.iter().filter_map(|p| p.math.as_ref()).map(|f| f.layout.descent + 1.0).fold(self.line_size(line) * 0.37, f32::max);
        self.line_ascent(line) + below
    }

    /// One wrapped line with its baseline at `base`, starting at `x`.
    pub(super) fn draw_line(&mut self, line: &[Piece], x: f32, base: f32) {
        let mut x = x;
        for p in line {
            let pw = piece_width(p);
            if let Some(f) = &p.math {
                self.p().math(&f.layout, x, base, p.color);
                x += pw;
                continue;
            }
            // sub and superscript: the smaller text is moved off the baseline
            let unit = p.size / crate::editor::SCRIPT_SCALE;
            let base = match p.script {
                1 => base - unit * 0.33,
                2 => base + unit * 0.12,
                _ => base,
            };
            if let Some(bg) = p.bg {
                let pad = p.size * 0.15;
                self.p().rect(x - pad * 0.5, base - p.size * 0.95, pw + pad, p.size * 1.25, Some(bg), None);
            }
            self.p().text(TextStyle { kind: p.kind, size: p.size, color: p.color, italic: p.italic, bold: p.bold }, x, base, &p.text);
            let tw = if p.text.ends_with(' ') && p.link.is_some() || p.text.ends_with(' ') && p.underline && line.last().map(|l| !std::ptr::eq(l, p)).unwrap_or(false) { pw } else { trimmed_width(p) };
            if p.underline {
                self.p().line([x, base + p.size * 0.15], [x + tw, base + p.size * 0.15], p.color, p.size * 0.06);
            }
            if p.strike {
                self.p().line([x, base - p.size * 0.3], [x + tw, base - p.size * 0.3], p.color, p.size * 0.06);
            }
            match &p.link {
                Some(Link::Web(uri)) => self.p().link(x, base - p.size, tw, p.size * 1.3, uri),
                Some(Link::Book(rel, heading)) => {
                    let page = self.page;
                    self.book_links.push(BookLink { page, rect: [x, base - p.size, tw, p.size * 1.3], rel: rel.clone(), heading: heading.clone() });
                }
                None => {}
            }
            if let Some(label) = &p.jump {
                let page = self.page;
                self.jumps.push((page, [x, base - p.size, tw, p.size * 1.3], label.clone()));
            }
            x += pw;
        }
    }

    /// The links of a text: its `[[links]]` and whether their notes exist.
    fn links_of(vault: &Vault, rel: &Path, text: &str) -> Vec<(markdown::LinkSpan, bool)> {
        markdown::find_links(text)
            .into_iter()
            .map(|l| {
                let ok = vault.exists(&l.target, Some(rel));
                (l, ok)
            })
            .collect()
    }

    /// Markdown inline text → styled pieces (using the reading view's parser).
    fn pieces(&self, vault: &Vault, rel: &Path, text: &str, size: f32, color: Color32, strike: bool) -> Vec<Piece> {
        let links = Self::links_of(vault, rel, text);
        self.pieces_from(vault, rel, crate::reader::inline_painted(text, &links, size, color, strike), size)
    }

    /// The text of a table cell: `<br>` is a line break (EDIT-045).
    fn cell_pieces(&self, vault: &Vault, rel: &Path, text: &str, size: f32, color: Color32) -> Vec<Piece> {
        let links = Self::links_of(vault, rel, text);
        self.pieces_from(vault, rel, crate::reader::cell_painted(text, &links, size, color), size)
    }

    /// The pieces of a laid-out line of the reading view; `size` is the size of its text.
    fn pieces_from(&self, vault: &Vault, rel: &Path, painted: crate::reader::Painted, size: f32) -> Vec<Piece> {
        let (job, spans, paints, maths) = painted;
        let mut out = Vec::new();
        for s in &job.sections {
            let r = s.byte_range.start.0..s.byte_range.end.0;
            let f = &s.format;
            // a formula that was typeset: drawn in place of its TeX (PDF-012)
            if let Some((m, done)) = maths.iter().find_map(|m| m.formula.as_ref().ok().filter(|_| m.range.start <= r.start && r.end <= m.range.end).map(|d| (m, d))) {
                // the hidden TeX is two sections: the formula is drawn in place of the first
                if r.start != m.range.start {
                    continue;
                }
                out.push(Piece {
                    text: String::new(),
                    kind: FontKind::Sans,
                    size: m.size,
                    color: print_color(m.color),
                    bg: None,
                    italic: false,
                    bold: false,
                    underline: false,
                    strike: false,
                    link: None,
                    script: 0,
                    jump: None,
                    math: Some(done.clone()),
                });
                continue;
            }
            // colours the note asks for are printed as written (EDIT-028), not mapped for paper
            let paint = paints.iter().find(|(pr, ..)| pr.start <= r.start && r.end <= pr.end);
            let link = spans.iter().find(|(sr, l, _)| sr.start <= r.start && r.end <= sr.end && (crate::lists::is_web_url(&l.target) || self.members.is_some())).and_then(|(_, l, _)| {
                if crate::lists::is_web_url(&l.target) {
                    return Some(Link::Web(l.target.clone()));
                }
                // a link to a note of the book is a link inside the PDF (BOOK-009)
                let target = vault.resolve(&l.target, Some(rel))?;
                self.members.as_ref()?.contains(&target).then(|| Link::Book(target, l.heading.clone()))
            });
            // inline code keeps its screen colours on the code background (PDF-007)
            let on_code = f.background == theme::CODE_BG;
            // a bold or italic face of a chosen font has no imitation that says so (CFG-039)
            let (face_bold, face_italic) = crate::fonts::bold_italic_of(&f.font_id.family);
            out.push(Piece {
                text: job.text[r.clone()].to_string(),
                kind: font_kind(&f.font_id.family),
                size: f.font_id.size,
                color: if paint.is_some() || on_code { rgba(f.color) } else { print_color(f.color) },
                bg: match paint.and_then(|p| p.1) {
                    Some([r, g, b, a]) => Some(over_white([r, g, b, a])),
                    None if on_code => Some(rgba(theme::CODE_BG)),
                    None => print_bg(f.background),
                },
                italic: f.italics || face_italic,
                bold: f.extra_letter_spacing > 0.0 || face_bold,
                underline: f.underline.width > 0.0,
                strike: f.strikethrough.width > 0.0,
                link,
                script: match (f.valign, f.font_id.size < size * 0.8) {
                    (egui::Align::TOP, true) => 1,
                    (_, true) => 2,
                    _ => 0,
                },
                jump: spans.iter().find(|(sr, ..)| sr.start <= r.start && r.end <= sr.end).and_then(|(_, l, _)| match crate::reader::footnote_of(l) {
                    Some((crate::reader::Foot::Reference, label)) => Some(label.to_string()),
                    _ => None,
                }),
                math: None,
            });
        }
        out
    }

    /// A formula alone on its lines (PDF-012): centred in display style, or its TeX as a code block
    /// when it cannot be typeset (EDIT-049).
    fn display_math(&mut self, tex: &str, base: f32) {
        match crate::math::typeset(tex, base * crate::math::DISPLAY_SCALE, true) {
            Ok(f) => {
                let l = &f.layout;
                self.need(l.height() + base * 0.8);
                let x = self.m + ((self.cw() - l.width) / 2.0).max(0.0);
                let y = self.y + base * 0.4 + l.ascent;
                let ink = print_color(theme::TEXT);
                self.p().math(l, x, y, ink);
                self.y += l.height() + base * 0.8;
            }
            Err(_) => {
                let lines: Vec<&str> = tex.trim_matches('\n').lines().collect();
                self.code_block("", &lines, base * 0.9);
                self.y += base * 0.5;
            }
        }
    }

    /// A fenced code block as the reading view shows it (PDF-007): the lines on the code
    /// background, each piece in the colour of its kind (`syntax::highlight`; plain without a
    /// known language), long lines wrapped by characters.
    fn code_block(&mut self, word: &str, lines: &[&str], size: f32) {
        let (x0, cw) = (self.m, self.cw());
        let lh = size * 1.4;
        let plain = rgba(theme::CODE);
        let text = lines.join("\n");
        let spans = crate::syntax::highlight(word, &text);
        let mut span = spans.iter().peekable();
        let mut at = 0;
        for line in lines {
            // every character with its colour; a tab is four spaces
            let mut chars: Vec<(char, Rgba)> = Vec::new();
            for (bi, c) in line.char_indices() {
                let pos = at + bi;
                while span.next_if(|(r, _)| r.end <= pos).is_some() {}
                let colour = span.peek().filter(|(r, _)| r.start <= pos).map_or(plain, |(_, k)| rgba(theme::syntax_colour(*k)));
                chars.extend(std::iter::repeat_n((if c == '\t' { ' ' } else { c }, colour), if c == '\t' { 4 } else { 1 }));
            }
            at += line.len() + 1;
            let mut rest = &chars[..];
            loop {
                let mut cut = rest.len();
                let mut acc = 0.0;
                for (n, (c, _)) in rest.iter().enumerate() {
                    acc += crate::pdf::write::char_width(FontKind::Mono, size, *c);
                    if acc > cw - 12.0 {
                        cut = n.max(1);
                        break;
                    }
                }
                let (now, later) = rest.split_at(cut);
                self.need(lh);
                let y = self.y;
                self.p().rect(x0, y, cw, lh, Some(rgba(theme::CODE_BG)), None);
                let mut x = x0 + 6.0;
                let mut n = 0;
                while n < now.len() {
                    let colour = now[n].1;
                    let run: String = now[n..].iter().take_while(|(_, k)| *k == colour).map(|(c, _)| *c).collect();
                    n += run.chars().count();
                    x += self.p().text(TextStyle::plain(FontKind::Mono, size, colour), x, y + size * 1.05, &run);
                }
                self.y += lh;
                if later.is_empty() {
                    break;
                }
                rest = later;
            }
        }
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
    // the title line's text: no `.md`, no date prefix (VAULT-025)
    let name = vault.file(rel).map(|f| f.title()).unwrap_or_else(|| crate::vault::title_of(&rel.to_string_lossy()).to_string());
    let mut w = Writer::new(&name, opts);
    let base = opts.font_pt;
    let heads = markdown::heading_numbers(text);
    // a first heading `# Title` that says the same as the title makes the title a second print of it (PDF-008)
    let said_by_heading = heads.first().is_some_and(|(_, level, title, _)| *level == 1 && markdown::heading_plain(title).to_lowercase() == markdown::heading_plain(&name).to_lowercase());
    if opts.title && !said_by_heading {
        let pieces = vec![Piece { text: name.clone(), kind: FontKind::Sans, size: base * 2.0, color: Rgba(0, 0, 0, 255), bg: None, italic: false, bold: true, underline: false, strike: false, link: None, script: 0, jump: None, math: None }];
        let x = w.m;
        let cw = w.cw();
        w.paragraph(&pieces, x, cw);
        w.y += base * 0.6;
    }
    // the headings with their numbers (LINK-014); where they are is found while the text is laid out
    let numbers = crate::reader::Extras::numbers_of(text, opts.numbered_headings);
    let contents: Vec<&(usize, usize, String, Option<String>)> = heads.iter().filter(|h| opts.toc && (1..=3).contains(&h.1)).collect();
    let slots = if contents.is_empty() { vec![] } else { w.reserve_contents(contents.len()) };
    let places = w.note_body(vault, rel, text, &numbers, None);
    let label = |line: usize, title: &str| match numbers.get(&line) {
        Some(n) => format!("{n} {}", markdown::heading_plain(title)),
        None => markdown::heading_plain(title),
    };
    let entries: Vec<Entry> = contents.iter().map(|(line, level, title, _)| Entry { level: *level, label: label(*line, title), dest: places.get(line).copied(), bold: false }).collect();
    w.draw_contents(&entries, &slots);
    // the bookmarks of PDF readers: every # to ###### heading, nested by level
    for (line, level, title, _) in &heads {
        if let Some(to) = places.get(line) {
            w.doc.bookmarks.push(Bookmark { level: *level, title: label(*line, title), to: *to });
        }
    }
    finish(w.doc, opts)
}

pub(crate) fn list_marker(line: &str) -> Option<(String, usize)> {
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

fn finish(doc: Doc, opts: &ExportOptions) -> Result<Vec<u8>, String> {
    finish_from(doc, opts, 0)
}

/// The document's bytes, the pages from `first` on with their numbers (a book has none on its title page).
pub(super) fn finish_from(mut doc: Doc, opts: &ExportOptions, first: usize) -> Result<Vec<u8>, String> {
    if opts.page_numbers {
        let n = doc.pages.len();
        for i in first..n {
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

    pub(crate) fn hex(s: &str) -> String {
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

    /// The content stream of each page (the streams that draw text).
    pub(crate) fn page_streams(bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(p) = find(bytes, b"stream\n", i) {
            let start = p + 7;
            let Some(end) = find(bytes, b"\nendstream", start) else { break };
            let s = String::from_utf8_lossy(&inflate(&bytes[start..end])).into_owned();
            if s.contains(" Tf") {
                out.push(s);
            }
            i = end + b"\nendstream".len();
        }
        out
    }

    /// The object numbers of the pages, in order, and of the targets of the links (in the file's order).
    pub(crate) fn page_ids_and_link_targets(bytes: &[u8]) -> (Vec<usize>, Vec<usize>) {
        let text = String::from_utf8_lossy(bytes);
        let mut pages = Vec::new();
        let mut links = Vec::new();
        for obj in text.split("\nendobj") {
            let Some((head, body)) = obj.split_once(" 0 obj\n") else { continue };
            let id: usize = head.trim().rsplit('\n').next().unwrap_or("").trim().parse().unwrap_or(0);
            if body.starts_with("<< /Type /Page /Parent") {
                pages.push(id);
            }
            if body.contains("/Subtype /Link") {
                if let Some((_, d)) = body.split_once("/Dest [") {
                    links.push(d.split_whitespace().next().unwrap().parse().unwrap());
                }
            }
        }
        (pages, links)
    }

    /// `text` with enough filler lines after each heading that the headings are on different pages.
    fn long_note(headings: &[&str]) -> String {
        headings.iter().map(|h| format!("{h}\n\n") + &"filler line of the section\n\n".repeat(25)).collect()
    }

    /// The distance from the page's bottom of the first word that starts with `word` in the page text.
    fn line_of(bytes: &[u8], word: &str) -> String {
        let c = contents(bytes);
        let at = c.find(&format!("<{}", hex(word.trim_end()))).unwrap_or_else(|| panic!("no {word:?} in the text: {}", c.lines().filter(|l| l.contains(" Tm")).collect::<Vec<_>>().join("|")));
        let head = &c[..at];
        let tm = head.rfind(" Tm").unwrap();
        head[..tm].split_whitespace().last().unwrap().to_string()
    }

    // spec: PDF-009
    #[test]
    fn lines_of_one_paragraph_are_joined_and_flow_to_the_page_width() {
        let (v, _) = vault();
        let export = |t: &str| export_note(&v, Path::new("Welcome.md"), t, &ExportOptions { title: false, toc: false, ..ExportOptions::for_note() }).unwrap();
        let same = |t: &str, a: &str, b: &str| {
            let bytes = export(t);
            line_of(&bytes, a) == line_of(&bytes, b)
        };
        // a paragraph wrapped in the source is one paragraph, reflowed
        assert!(same("alpha beta\ngamma delta epsilon\n", "beta ", "gamma "), "the source line break is a space");
        let long = format!("{}\n{}\n", "word ".repeat(60).trim(), "tail end of the paragraph");
        let bytes = export(&long);
        assert_ne!(line_of(&bytes, "tail "), line_of(&bytes, "word "), "a long paragraph still wraps at the page's width");
        // a blank line, a hard break (two spaces, a backslash), a heading, a list, a table and an embed stay apart
        assert!(!same("alpha beta\n\ngamma delta\n", "beta ", "gamma "), "a blank line");
        assert!(!same("alpha beta  \ngamma delta epsilon\n", "beta ", "gamma "), "two spaces at the end");
        assert!(!same("alpha beta\\\ngamma delta epsilon\n", "beta", "gamma "), "a backslash at the end");
        assert!(!same("alpha beta\n## Head\ngamma delta epsilon\n", "beta", "gamma "), "a heading");
        assert!(!same("alpha beta\n- gamma delta epsilon\n", "beta", "gamma "), "a list item");
        assert!(!same("alpha beta\n---\ngamma delta epsilon\n", "beta", "gamma "), "a rule");
        assert!(!same("alpha beta\n![[Sketch.excalidraw]]\ngamma delta epsilon\n", "beta", "gamma "), "an embed on its own line");
        assert!(!same("alpha beta\n```\ngamma\n```\n", "beta", "gamma"), "a fence");
    }

    // spec: PDF-009
    #[test]
    fn a_continuation_line_joins_its_list_item_task_or_quote() {
        let (v, _) = vault();
        let bytes = |t: &str| export_note(&v, Path::new("Welcome.md"), t, &ExportOptions { title: false, toc: false, ..ExportOptions::for_note() }).unwrap();
        let b = bytes("- first item\n  goes on here\n- second one\n");
        assert_eq!(line_of(&b, "item "), line_of(&b, "goes "), "an indented line joins its item");
        assert_ne!(line_of(&b, "goes "), line_of(&b, "second "), "the next item is its own");
        let b = bytes("- first item\nnot indented\n");
        assert_ne!(line_of(&b, "item "), line_of(&b, "not "), "a line that is not indented is a paragraph");
        let b = bytes("- [ ] open task\n  with more text\n");
        assert_eq!(line_of(&b, "task "), line_of(&b, "with "), "and a task");
        let b = bytes("> quoted words\n> go on here\n\nafter\n");
        assert_eq!(line_of(&b, "words "), line_of(&b, "go "), "the lines of a quote");
        assert_ne!(line_of(&b, "go "), line_of(&b, "after"), "a quote ends at the blank line");
        let b = bytes("1. numbered item\n   continues\n2. next\n");
        assert_eq!(line_of(&b, "item "), line_of(&b, "continues"));
    }

    // spec: PDF-008, LINK-014, VAULT-025
    #[test]
    fn the_pdf_starts_with_a_contents_page_of_numbered_headings_with_their_pages_and_links() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        let text = long_note(&["# Title of the note", "## Alpha", "### Beta", "## Gamma", "#### Deep", "##### Five"]);
        let bytes = export_note(&v, rel, &text, &ExportOptions::for_note()).unwrap();
        let pages = page_streams(&bytes);
        assert_eq!(render(&bytes).len(), pages.len(), "every page renders");
        // page 1: the title, then the contents with the numbers and the pages the headings start on
        let first = &pages[0];
        assert!(first.contains(&hex("Welcome")) && first.contains(&hex("Contents")), "the name as the title, then the contents");
        for label in ["1 Title of the note", "1.1 Alpha", "1.1.1 Beta", "1.2 Gamma"] {
            assert!(first.contains(&hex(label)), "{label} is in the contents");
        }
        assert!(!first.contains(&hex("Deep")) && !first.contains(&hex("Five")), "#### and ##### are not in the contents");
        assert!(!first.contains(&hex("filler")), "the text starts on a new page");
        assert!(pages[1].contains(&hex("Title")), "the note's text follows on page 2");
        let body = |word: &str| (1..pages.len()).find(|i| pages[*i].contains(&hex(word))).unwrap_or_else(|| panic!("{word} is in the text of no page")) + 1;
        let (alpha, gamma) = (body("Alpha"), body("Gamma"));
        assert!(gamma > alpha && alpha >= 2, "the headings are on pages {alpha} and {gamma}");
        for (label, page) in [("1.1 Alpha", alpha), ("1.2 Gamma", gamma)] {
            assert!(first.contains(&hex(&page.to_string())), "the contents say {label} is on page {page}");
        }
        // each entry is a link to the page of its heading, and so is each bookmark
        let (ids, links) = page_ids_and_link_targets(&bytes);
        assert_eq!(links.len(), 4, "a link per entry");
        assert_eq!(links[1], ids[alpha - 1]);
        assert_eq!(links[3], ids[gamma - 1]);
        assert!(links.iter().all(|l| ids.contains(l)));
        let raw = String::from_utf8_lossy(&bytes);
        assert!(raw.contains("/Type /Outlines") && raw.contains("/Count 6"), "bookmarks for # to ######: the six headings");
        assert!(raw.contains("/Type /Catalog") && raw.contains("/Outlines "), "the catalog points to them");
        let utf16 = |t: &str| t.encode_utf16().map(|u| format!("{u:04X}")).collect::<String>();
        for label in ["1 Title of the note", "1.1 Alpha", "1.1.1 Beta", "1.2 Gamma", "1.2.1 Deep", "1.2.1.1 Five"] {
            assert!(raw.contains(&format!("<FEFF{}>", utf16(label))), "bookmark {label}");
        }
        // nested by level: Beta is below Alpha, Gamma is not
        let obj_of = |label: &str| raw.split("\nendobj").find(|o| o.contains(&format!("/Title <FEFF{}>", utf16(label)))).unwrap().to_string();
        let id_of = |o: &str| o.split_whitespace().next().unwrap().to_string();
        let (alpha_obj, beta_obj, gamma_obj) = (obj_of("1.1 Alpha"), obj_of("1.1.1 Beta"), obj_of("1.2 Gamma"));
        assert!(beta_obj.contains(&format!("/Parent {} 0 R", id_of(&alpha_obj))), "Beta is below Alpha");
        assert!(!gamma_obj.contains(&format!("/Parent {} 0 R", id_of(&alpha_obj))), "Gamma is not");
        assert!(alpha_obj.contains("/First") && alpha_obj.contains("/Next"), "Alpha has Beta below and Gamma after");
        // the headings in the text carry their numbers too
        let (page, number) = (&pages[alpha - 1], hex("1.1 "));
        assert!(page.contains(&format!("<{number}> Tj")), "the number is printed with the heading");
        assert_eq!(y_of(page, "1.1 "), y_of(page, "Alpha"), "on its line");
        assert!(x_of(page, "1.1 ") < x_of(page, "Alpha"), "before its text");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-008, VAULT-025
    #[test]
    fn the_title_is_printed_once_when_the_first_heading_says_the_same_and_has_no_date_prefix() {
        let (v, d) = vault();
        std::fs::write(d.join("261117_Plan.md"), "").unwrap();
        let v = {
            drop(v);
            Vault::open(&d).unwrap()
        };
        let rel = Path::new("261117_Plan.md");
        let count = |text: &str, opts: &ExportOptions, word: &str| contents(&export_note(&v, rel, text, opts).unwrap()).matches(&format!("<{}> Tj", hex(word))).count();
        let plain = ExportOptions { toc: false, ..ExportOptions::for_note() };
        // the same text as the title (ignoring case and markup): once, from the heading
        assert_eq!(count("# **plan**\ntext\n", &plain, "plan"), 1, "the heading is the title");
        assert_eq!(count("# Plan\ntext\n", &plain, "Plan"), 1);
        // another first heading, a `##`, or a later `# Plan`: the title is printed as well
        assert_eq!(count("# Roadmap\n# Plan\n", &plain, "Plan"), 2, "only the first heading counts");
        assert_eq!(count("## Plan\ntext\n", &plain, "Plan"), 2);
        assert_eq!(count("text\n", &plain, "Plan"), 1, "no heading: the title alone, without the date prefix");
        assert_eq!(count("text\n", &plain, "261117_Plan"), 0);
        // the box off: no title, whatever the headings
        assert_eq!(count("## Plan\n", &ExportOptions { title: false, ..plain.clone() }, "Plan"), 1);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-008, CFG-042
    #[test]
    fn without_the_box_or_without_headings_there_is_no_contents_page_and_numbering_follows_the_note() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        let text = long_note(&["## Alpha", "### Beta"]);
        let off = export_note(&v, rel, &text, &ExportOptions { toc: false, ..ExportOptions::for_note() }).unwrap();
        let (pages, raw) = (page_streams(&off), String::from_utf8_lossy(&off).into_owned());
        assert!(!pages.iter().any(|p| p.contains(&hex("Contents"))), "no contents page with the box off");
        assert!(pages[0].contains(&hex("Welcome")) && pages[0].contains(&hex("Alpha")), "the text starts at once");
        assert!(page_ids_and_link_targets(&off).1.is_empty(), "no links to pages");
        assert!(raw.contains("/Type /Outlines"), "the bookmarks are there anyway");
        let (_, c) = export_text(&v, "Just text, no headings.\n");
        assert!(!c.contains(&hex("Contents")), "no headings, no contents");
        assert!(!String::from_utf8_lossy(&export_note(&v, rel, "text only\n", &ExportOptions::for_note()).unwrap()).contains("/Outlines"), "no bookmarks without headings");
        assert!(String::from_utf8_lossy(&export_note(&v, rel, "# Only a title\ntext\n", &ExportOptions::for_note()).unwrap()).contains("/Outlines"), "a # heading has a bookmark too");
        // numbering: off by the setting, on or off by the note's front matter
        let plain = |t: &str, numbered: bool| contents(&export_note(&v, rel, t, &ExportOptions { numbered_headings: numbered, ..ExportOptions::for_note() }).unwrap());
        assert!(plain("## Alpha\n", true).contains(&hex("1 Alpha")));
        let c = plain("## Alpha\n", false);
        assert!(c.contains(&hex("Alpha")) && !c.contains(&hex("1 Alpha")), "numbering off: no numbers in the contents or the text");
        assert!(plain("---\nnumbered-headings: true\n---\n## Alpha\n", false).contains(&hex("1 Alpha")));
        assert!(!plain("---\nnumbered-headings: false\n---\n## Alpha\n", true).contains(&hex("1 Alpha")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-008
    #[test]
    fn a_long_contents_list_takes_as_many_pages_as_it_needs_and_non_ascii_titles_work() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        let heads: Vec<String> = (1..=90).map(|n| format!("## Überblick {n}")).collect();
        let text = long_note(&heads.iter().map(String::as_str).collect::<Vec<_>>());
        let bytes = export_note(&v, rel, &text, &ExportOptions::for_note()).unwrap();
        let pages = page_streams(&bytes);
        let contents_pages = pages.iter().take_while(|p| !p.contains(&hex("filler"))).count();
        assert!(contents_pages >= 2, "90 entries do not fit one page: {contents_pages}");
        assert!(pages[contents_pages - 1].contains(&hex("90 Überblick 90")), "the last entry is on the last contents page");
        let (ids, links) = page_ids_and_link_targets(&bytes);
        assert_eq!(links.len(), 90);
        assert_eq!(links[0], ids[contents_pages], "the first entry goes to the first page of the text");
        assert_eq!(render(&bytes).len(), pages.len());
        let _ = std::fs::remove_dir_all(&d);
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

    /// The y (PDF coordinates, up) of the text drawn as `word`.
    fn y_of(content: &str, word: &str) -> f32 {
        let at = content.find(&format!("<{}> Tj", hex(word))).unwrap_or_else(|| panic!("{word} is not drawn"));
        let toks: Vec<&str> = content[..at].split_whitespace().collect();
        toks[toks.len() - 2].parse().unwrap()
    }

    // spec: PDF-011, EDIT-051
    #[test]
    fn underline_sub_and_superscript_are_drawn_in_the_pdf_and_their_tags_are_not_printed() {
        let (v, d) = vault();
        let page = |text: &str| {
            let bytes = export_note(&v, Path::new("Welcome.md"), text, &ExportOptions::for_note()).unwrap();
            page_streams(&bytes).remove(0)
        };
        let c = page("x<sup>7</sup> and a<sub>3</sub>b and <u>word</u> end\n");
        assert!(!c.contains(&hex("<u>")) && !c.contains(&hex("sup>")) && !c.contains(&hex("</")), "the tags are not printed");
        let (x, a, seven, three) = (y_of(&c, "x"), y_of(&c, "a"), y_of(&c, "7"), y_of(&c, "3"));
        assert!(seven > x + 2.0, "the superscript is raised: {seven} over {x}");
        assert!(three < a - 0.5, "the subscript is lowered: {three} under {a}");
        let small = format!("{:.2} Tf", 11.0 * crate::editor::SCRIPT_SCALE);
        assert!(c.contains(&small), "both are smaller: {small}");
        // the underline is a line under the word
        let plain = page("x and word end\n");
        let lines = |s: &str| s.matches(" l S Q").count();
        assert_eq!(lines(&c), lines(&plain) + 1, "one line under <u>word</u>");
        // an unclosed tag stays text
        assert!(page("a <u>b\n").contains(&hex("<u>")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-011, EDIT-050
    #[test]
    fn a_footnote_reference_in_the_pdf_is_a_raised_link_to_its_definition_which_is_printed_where_it_stands() {
        let (v, d) = vault();
        let text = "Claim[^1] and[^2] here\n\n".to_string() + &"filler line\n\n".repeat(60) + "[^1]: The body[^1]\n    and more\n\nlast\n";
        let bytes = export_note(&v, Path::new("Welcome.md"), &text, &ExportOptions::for_note()).unwrap();
        let pages = page_streams(&bytes);
        assert!(pages.len() >= 2, "the definition is on a later page");
        let all = pages.join("\n");
        assert!(!all.contains(&hex("[^1")) && !all.contains(&hex("]:")), "no raw footnote markup");
        assert!(all.contains(&hex("[^2]")), "a reference without a definition is printed as written");
        let (body_page, _) = pages.iter().enumerate().find(|(_, p)| p.contains(&hex("body"))).unwrap();
        assert!(body_page > 0 && all.contains(&hex("more")), "the definition and its continuation line are where they stand: page {body_page}");
        // the mark is smaller and raised, the text of the definition a little smaller
        assert!(pages[0].contains(&format!("{:.2} Tf", 11.0 * crate::editor::SCRIPT_SCALE)), "the mark of the reference is small");
        assert!(pages[body_page].contains(&format!("{:.2} Tf", 11.0 * crate::editor::FOOTNOTE_TEXT)), "the text of the definition is a little smaller");
        assert!(y_of(&pages[0], "1") > y_of(&pages[0], "Claim"), "the mark is raised");
        // the references (one in the text, one in the definition) link to the page of the definition
        let (ids, links) = page_ids_and_link_targets(&bytes);
        assert_eq!(links.len(), 2, "a link per reference to a defined footnote");
        assert!(links.iter().all(|l| *l == ids[body_page]), "they go to the page of the definition: {links:?} / {ids:?}");
        assert_eq!(render(&bytes).len(), pages.len());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-009, PDF-011, EDIT-050
    #[test]
    fn a_footnote_definition_is_not_joined_into_the_paragraph_before_or_after_it() {
        let (v, d) = vault();
        let export = |t: &str| export_note(&v, Path::new("Welcome.md"), t, &ExportOptions { title: false, toc: false, ..ExportOptions::for_note() }).unwrap();
        let bytes = export("alpha beta[^1]\n[^1]: note text\n    goes on here\ngamma delta epsilon\n");
        let c = page_streams(&bytes).remove(0);
        let (beta, note, goes, gamma) = (y_of(&c, "beta"), y_of(&c, "note "), y_of(&c, "goes "), y_of(&c, "gamma "));
        assert!(beta > note, "the definition is under the paragraph, not on its line: {beta} {note}");
        assert!(note > goes, "its continuation line is a line of its own: {note} {goes}");
        assert!(goes > gamma, "the paragraph after it is not joined to it: {goes} {gamma}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-009, PDF-012
    #[test]
    fn a_display_formula_stays_a_block_between_reflowed_lines() {
        let (v, d) = vault();
        let export = |t: &str| export_note(&v, Path::new("Welcome.md"), t, &ExportOptions { title: false, toc: false, ..ExportOptions::for_note() }).unwrap();
        let centred = |t: &str| {
            let bytes = export(t);
            let c = page_streams(&bytes).remove(0);
            assert!(c.contains(&hex("before")) && c.contains(&hex("after")), "the text is printed");
            assert!(!c.contains(&hex("$$")) && !c.contains(&hex("frac")), "no TeX is printed as text");
            assert!(y_of(&c, "before") > y_of(&c, "after"), "the formula does not join the lines around it");
            let page = render(&bytes).remove(0);
            let width = page.0 as f32 * 2.0;
            let page_h = page.1 as f32 * 2.0;
            // the ink between the baselines of the two text lines
            let (l, r) = ink_columns(&page, page_h - y_of(&c, "before") + 5.0..page_h - y_of(&c, "after") - 12.0).expect("ink of the formula");
            ((l + r) / 2.0 - width / 2.0).abs() < 6.0
        };
        assert!(centred("before\n$$\n\\frac{a}{b}\n$$\nafter\n"), "$$ lines");
        assert!(centred("before\n$$\\frac{a}{b}$$\nafter\n"), "a line holding only $$…$$");
        assert!(centred("before\n```math\n\\frac{a}{b}\n```\nafter\n"), "a math fence");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-011, EDIT-045
    #[test]
    fn br_in_a_table_cell_is_a_new_line_in_the_pdf() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let text = "| A | B |\n|---|---|\n| first<br>second<br/>third | one |\n";
        let bytes = export_note(&v, rel, text, &ExportOptions::for_note()).unwrap();
        let c = page_streams(&bytes).remove(0);
        assert!(!c.contains(&hex("<br")), "the tag is not printed");
        let (f, s, t, one) = (y_of(&c, "first"), y_of(&c, "second"), y_of(&c, "third"), y_of(&c, "one"));
        assert!(f > s && s > t, "one line under the other: {f} {s} {t}");
        assert_eq!(f, one, "the other cell starts at the top");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-024, PDF-001
    #[test]
    fn a_table_is_a_grid_in_the_pdf_and_plain_text_lines_when_asked() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let text = "Intro\n\n<!-- shale-table edit=raw -->\n| Where | What |\n|---|--:|\n| `a.json` | **yes** [[Ideas]] |\n\nAfter\n";
        let grid = export_note(&v, rel, text, &ExportOptions::for_note()).unwrap();
        let c = contents(&grid);
        assert!(!c.contains(&hex("shale-table")) && !c.contains(&hex("<!--")), "the table's comment line is not printed (EDIT-030)");
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

    // spec: EDIT-031, PDF-001
    #[test]
    fn stored_column_widths_and_row_heights_shape_the_pdf_table() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let opts = ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() };
        let x_second = |text: &str| {
            let c = contents(&export_note(&v, rel, text, &opts).unwrap());
            x_of(&c, "Bbb")
        };
        let body = "| Aaa | Bbb |\n|---|---|\n| a | b |\n";
        let auto = x_second(body);
        let m = 20.0 * MM;
        let text_w = 595.0 - 2.0 * m;
        // a table that fits keeps its stored widths: a narrow table stays narrow, as in the editor
        let narrow = x_second(&format!("<!-- shale-table cols=100,300 -->\n{body}"));
        assert!((narrow - (m + 100.0 + CELL_PAD_X)).abs() < 1.0, "{narrow}");
        // one that is too wide is scaled down together to the text width
        let wide = x_second(&format!("<!-- shale-table cols=3000,1000 -->\n{body}"));
        assert!((wide - (m + (text_w - 1.0) * 0.75 + CELL_PAD_X)).abs() < 1.0, "{wide}");
        // a tiny stored width is raised to MIN_COL
        let clamped = x_second(&format!("<!-- shale-table cols=1,100 -->\n{body}"));
        assert!((clamped - (m + crate::table::MIN_COL + CELL_PAD_X)).abs() < 1.0, "{clamped}");
        // sizes that do not match the columns (or are no numbers) are ignored
        for bad in ["cols=100", "cols=100,200,300", "cols=0,100", "cols=x,y"] {
            let got = x_second(&format!("<!-- shale-table {bad} -->\n{body}"));
            assert!((got - auto).abs() < 0.01, "{bad}: {got} {auto}");
        }
        // a stored row height is a least height: the table is that much taller
        let y_last = |text: &str| y_of(&contents(&export_note(&v, rel, text, &opts).unwrap()), "a");
        let plain = y_last(body);
        let tall = y_last(&format!("<!-- shale-table rows=60,80 -->\n{body}"));
        assert!(tall < plain - 30.0, "the header row is at least 60 high: {plain} {tall}");
        let short = y_last(&format!("<!-- shale-table rows=1,1 -->\n{body}"));
        assert!((short - plain).abs() < 0.01, "a row is never shorter than its text");
        let mismatch = y_last(&format!("<!-- shale-table rows=60 -->\n{body}"));
        assert!((mismatch - plain).abs() < 0.01, "row heights that do not match are ignored");
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

    // spec: PDF-006, EDIT-029
    #[test]
    fn colours_are_printed_as_a_filled_rectangle_and_a_text_colour_without_their_tags() {
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let text =
            "Plain <mark style=\"background: #39FF14;\">neon</mark> and <span style=\"color: #FF0000;\">redtext</span> and ==soft== and <mark style=\"background: #000080;\">navy</mark> end <mark style=\"background: #FFFF00;\">unclosed";
        let bytes = export_note(&v, rel, text, &ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() }).unwrap();
        let c = contents(&bytes);
        for w in ["neon", "redtext", "soft", "navy"] {
            assert!(c.contains(&hex(w)), "{w} is real text");
        }
        assert!(!c.contains(&hex("</mark>")) && !c.contains(&hex("</span>")) && !c.contains(&hex("#39FF14")) && !c.contains(&hex("#FF0000")), "the tags are not printed");
        assert!(c.contains(&hex("unclosed")), "text after an unclosed tag stays");
        // the neon fill, as a rectangle before the text: 0.224 1.000 0.078 rg ... f
        assert!(c.contains("0.224 1.000 0.078 rg"), "the highlight colour fills a rectangle: {c}");
        assert!(c.contains("1.000 0.000 0.000 rg"), "the text is red");
        assert!(c.contains("1.000 0.945 0.659 rg"), "==soft== is filled too");
        // black text on neon (dark text on a bright background), white on navy
        assert!(c.contains("0.000 0.000 0.502 rg"), "navy fill");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The `rg` operator of a screen colour, as the writer prints it.
    fn rg(c: Color32) -> String {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        format!("{:.3} {:.3} {:.3} rg", r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
    }

    /// The fill colour (as an `rg` operator) last set before the run drawn as `word`.
    fn colour_of(content: &str, word: &str) -> String {
        let at = content.find(&format!("<{}> Tj", hex(word))).unwrap_or_else(|| panic!("{word} is not drawn"));
        let head = &content[..at];
        let end = head.rfind(" rg").expect("a fill colour before the text");
        let rgb: Vec<&str> = head[..end].rsplit(' ').take(3).collect();
        format!("{} {} {} rg", rgb[2], rgb[1], rgb[0])
    }

    // spec: PDF-007, EDIT-032
    #[test]
    fn code_blocks_have_the_code_background_and_the_syntax_colours_of_the_reading_view() {
        use crate::syntax::Kind;
        let (v, d) = vault();
        let rel = Path::new("Ideas.md");
        let o = ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() };
        let js = export_note(&v, rel, "```js\nconst x = \"hi\"; // note\n```\n", &o).unwrap();
        let c = contents(&js);
        assert!(c.contains(&rg(theme::CODE_BG)), "the lines are filled with the code background");
        assert_eq!(colour_of(&c, "const"), rg(theme::syntax_colour(Kind::Keyword)));
        assert_eq!(colour_of(&c, "\"hi\""), rg(theme::syntax_colour(Kind::String)));
        assert_eq!(colour_of(&c, "// note"), rg(theme::syntax_colour(Kind::Comment)));
        let colours: std::collections::HashSet<String> = ["const", "\"hi\"", "// note"].iter().map(|w| colour_of(&c, w)).collect();
        assert!(colours.len() >= 3, "several distinct colours for the code text");
        // no language, an unknown one: plain code colour, still on the code background
        for fence in ["```", "```klingon"] {
            let plain = export_note(&v, rel, &format!("{fence}\nconst x = \"hi\";\n```\n"), &o).unwrap();
            let c = contents(&plain);
            assert!(c.contains(&rg(theme::CODE_BG)), "{fence}: code background");
            assert_eq!(colour_of(&c, "const x = \"hi\";"), rg(theme::CODE), "{fence}: one plain run in the code colour");
        }
        // a long line wraps and keeps its colours; a tab is four spaces
        let long = format!("```js\n\tconst s = \"{}\";\n```\n", "x".repeat(300));
        let c = contents(&export_note(&v, rel, &long, &o).unwrap());
        assert!(c.contains(&format!("<{}> Tj", hex("    "))), "the tab is four spaces");
        assert!(c.matches(&rg(theme::syntax_colour(Kind::String))).count() >= 2, "the string goes on in its colour on the next row");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-007, EDIT-032
    #[test]
    fn inline_code_is_on_the_code_background_with_the_colours_of_the_inline_rule() {
        use crate::syntax::Kind;
        let (v, d) = vault();
        let o = ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() };
        let c = contents(&export_note(&v, Path::new("Ideas.md"), "Call `log(\"hi\", 42)` now and `plain` too.\n", &o).unwrap());
        assert!(c.contains(&rg(theme::CODE_BG)), "a code background behind the run");
        assert_eq!(colour_of(&c, "log"), rg(theme::syntax_colour(Kind::Function)));
        assert_eq!(colour_of(&c, "\"hi\""), rg(theme::syntax_colour(Kind::String)));
        assert_eq!(colour_of(&c, "42"), rg(theme::syntax_colour(Kind::Number)));
        assert_eq!(colour_of(&c, "plain"), rg(theme::CODE), "a span without such parts is code-coloured");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: CFG-039
    #[test]
    fn bold_and_italic_in_a_font_s_own_faces_are_still_printed_bold_and_italic() {
        use crate::fonts::{AreaFont, Faces};
        let (v, d) = vault();
        let o = ExportOptions { title: false, page_numbers: false, ..ExportOptions::for_note() };
        let font = AreaFont { family: egui::FontFamily::Name("system:X".into()), size: 15.0, faces: Faces { bold: true, italic: true, bold_italic: true } };
        // even inside an area whose font has the faces, as when the export starts from the editor
        let bytes = crate::fonts::with_family(&font, || export_note(&v, Path::new("Ideas.md"), "Some **strong** and *slanted* words.\n", &o).unwrap());
        let c = contents(&bytes);
        let op = |w: &str| {
            let at = c.find(&format!("<{}> Tj", hex(w))).unwrap_or_else(|| panic!("{w} is not drawn"));
            c[c[..at].rfind("BT").unwrap()..at].to_string()
        };
        assert!(op("strong").contains(" 2 Tr"), "bold: {}", op("strong"));
        assert!(op("slanted").contains("1 0 0.2 1"), "italic: {}", op("slanted"));
        assert!(op("words.").contains(" 0 Tr 1 0 0 1"), "plain: {}", op("words."));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The ink of a rendered page (at the scale of `render`: half a pixel per point) inside a
    /// rectangle given in PDF points from the top left: how many pixels are not white.
    fn ink(page: &(u32, u32, Vec<u8>), x: std::ops::Range<f32>, y: std::ops::Range<f32>) -> usize {
        let (w, h, data) = page;
        let mut n = 0;
        for py in (y.start * 0.5).max(0.0) as u32..((y.end * 0.5) as u32).min(*h) {
            for px in (x.start * 0.5).max(0.0) as u32..((x.end * 0.5) as u32).min(*w) {
                let i = ((py * w + px) * 4) as usize;
                if data[i] < 235 {
                    n += 1;
                }
            }
        }
        n
    }

    /// The columns (in PDF points) between which a page has ink in the rows `y`.
    fn ink_columns(page: &(u32, u32, Vec<u8>), y: std::ops::Range<f32>) -> Option<(f32, f32)> {
        let (w, _, _) = page;
        let cols: Vec<u32> = (0..*w).filter(|c| ink(page, (*c as f32 * 2.0)..(*c as f32 * 2.0 + 2.0), y.clone()) > 0).collect();
        Some((*cols.first()? as f32 * 2.0, *cols.last()? as f32 * 2.0 + 2.0))
    }

    // spec: PDF-012, EDIT-047
    #[test]
    fn math_in_the_pdf_is_vector_glyphs_and_rules_in_the_line_and_its_tex_is_not_printed() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        let plain = export_note(&v, rel, "Energy and here.\n", &ExportOptions::for_note()).unwrap();
        let text = "Energy $E = mc^2$ and here.\n\n$$\\frac{a}{b}$$\n";
        let bytes = export_note(&v, rel, text, &ExportOptions::for_note()).unwrap();
        let c = page_streams(&bytes).remove(0);
        assert!(!c.contains(&hex("$E")) && !c.contains(&hex("mc^2")) && !c.contains(&hex("frac")), "no TeX is printed as text");
        assert!(c.contains(&hex("Energy ")) && c.contains(&hex("and ")) && c.contains(&hex("here.")), "the text around is text");
        let outlines = |s: &str| s.matches(" f Q").count();
        let base = outlines(&page_streams(&plain).remove(0));
        // E = m c 2 are glyph outlines, and the fraction bar is a filled rectangle
        assert!(outlines(&c) >= base + 7, "glyph outlines: {} against {base}", outlines(&c));
        assert!(c.contains(" c "), "curves of glyph outlines");
        // the formula is on the line of its text: ink between the two pieces of text, on the same baseline
        let (y_text, x_end, x_next) = (y_of(&c, "Energy "), x_of(&c, "Energy ") + crate::pdf::write::text_width(FontKind::Sans, 11.0, "Energy "), x_of(&c, "and "));
        let page = render(&bytes).remove(0);
        let page_h = page.1 as f32 * 2.0;
        let top = page_h - y_text - 14.0;
        assert!(x_next - x_end > 30.0, "room for the formula: {x_end} {x_next}");
        assert!(ink(&page, x_end + 1.0..x_next - 1.0, top..top + 22.0) > 15, "the formula is drawn in the gap");
        assert_eq!(render(&plain).remove(0).1, page.1, "same paper");
        // the display formula is centred on the page and under the text
        let width = page.0 as f32 * 2.0;
        let (l, r) = ink_columns(&page, top + 40.0..page_h - 100.0).expect("ink of the display formula");
        assert!(((l + r) / 2.0 - width / 2.0).abs() < 6.0, "centred: {l} {r} of {width}");
        assert!(r - l > 6.0, "{l} {r}");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-012, EDIT-049
    #[test]
    fn a_formula_that_cannot_be_typeset_is_printed_as_code_in_the_pdf() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        let bytes = export_note(&v, rel, "Try $\\unknown{x}$ here.\n\n$$\n\\frac{a}\n$$\n", &ExportOptions::for_note()).unwrap();
        let c = page_streams(&bytes).remove(0);
        assert!(c.contains(&hex("$\\unknown{x}$")), "the TeX is printed");
        let at = c.find(&format!("<{}> Tj", hex("$\\unknown{x}$"))).unwrap();
        assert!(c[..at].rfind("/F2").is_some_and(|f| c[f..at].contains(" Tf")), "in the code font");
        assert!(c.contains(&hex("\\frac{a}")), "a display formula as a code block");
        assert_eq!(render(&bytes).len(), page_streams(&bytes).len());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PDF-012
    #[test]
    fn math_wraps_with_its_line_makes_it_taller_and_works_in_table_cells() {
        let (v, d) = vault();
        let rel = Path::new("Welcome.md");
        // a tall formula makes its line taller; the next line is further down
        let tall = export_note(&v, rel, "first $\\frac{\\frac{a}{b}}{\\frac{c}{d}}$ line\n\nsecond line\n", &ExportOptions::for_note()).unwrap();
        let plain = export_note(&v, rel, "first x line\n\nsecond line\n", &ExportOptions::for_note()).unwrap();
        let gap = |b: &[u8]| {
            let c = page_streams(b).remove(0);
            y_of(&c, "first ") - y_of(&c, "second ")
        };
        assert!(gap(&tall) > gap(&plain) + 5.0, "{} {}", gap(&tall), gap(&plain));
        // a long paragraph with a formula in it wraps and stays within the margins
        let words = "word ".repeat(60);
        let bytes = export_note(&v, rel, &format!("{words}$a+b+c+d+e+f+g+h$ {words}\n"), &ExportOptions::for_note()).unwrap();
        let page = render(&bytes).remove(0);
        let width = page.0 as f32 * 2.0;
        let margin = 20.0 * 72.0 / 25.4;
        let (l, r) = ink_columns(&page, 0.0..page.1 as f32 * 2.0).unwrap();
        assert!(l >= margin - 3.0 && r <= width - margin + 3.0, "{l} {r} within {margin}..{}", width - margin);
        // a table cell
        let bytes = export_note(&v, rel, "| A | B |\n|---|:-:|\n| one | $x^2 + \\sqrt{y}$ |\n", &ExportOptions::for_note()).unwrap();
        let c = page_streams(&bytes).remove(0);
        assert!(!c.contains(&hex("$x")) && !c.contains(&hex("sqrt")), "no TeX in the cell");
        assert!(c.matches(" f Q").count() >= 6, "the glyphs of the cell");
        let page = render(&bytes).remove(0);
        let page_h = page.1 as f32 * 2.0;
        let y = page_h - y_of(&c, "one");
        let (l, r) = ink_columns(&page, y - 14.0..y + 8.0).unwrap();
        assert!(ink(&page, (l + (r - l) / 2.0 + 2.0)..r - 2.0, y - 12.0..y + 6.0) > 15, "the formula is drawn in the second cell: {l} {r}");
        let _ = std::fs::remove_dir_all(&d);
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

/// How big a heading of a book is set, by the level it is printed at (BOOK-006), in text sizes.
pub(super) fn book_scale(level: usize) -> f32 {
    match level {
        0 | 1 => 2.1,
        2 => 1.7,
        3 => 1.4,
        4 => 1.2,
        5 => 1.1,
        _ => 1.0,
    }
}
