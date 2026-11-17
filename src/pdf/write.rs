//! A small PDF writer: pages with vector paths, text in embedded TrueType fonts (WinAnsi),
//! glyph outlines for other fonts/characters, images, transparency and link annotations.
//! Coordinates given to this module are in points with the origin at the TOP-left of the page
//! (y grows downwards, like on screen); they are flipped when written.

use skrifa::charmap::Charmap;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::{GlyphMetrics, Metrics};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::{FontRef, GlyphId, MetadataProvider, OutlineGlyphCollection};
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::sync::OnceLock;

/// Fonts available for text.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, PartialOrd, Ord)]
pub enum FontKind {
    /// Ubuntu Light — embedded, selectable text.
    Sans,
    /// Hack — embedded, selectable text (code).
    Mono,
    /// Virgil (hand-drawn) — drawn as outlines.
    Hand,
}

/// A bundled font, parsed once: the charmap, the horizontal advances and the outlines in font
/// units (unscaled), and the font-wide metrics.
struct LoadedFont {
    data: &'static [u8],
    charmap: Charmap<'static>,
    advances: GlyphMetrics<'static>,
    outlines: OutlineGlyphCollection<'static>,
    metrics: Metrics,
}

impl LoadedFont {
    fn parse(data: &'static [u8]) -> Self {
        let font = FontRef::new(data).expect("bundled font");
        LoadedFont { data, charmap: font.charmap(), advances: font.glyph_metrics(Size::unscaled(), LocationRef::default()), outlines: font.outline_glyphs(), metrics: font.metrics(Size::unscaled(), LocationRef::default()) }
    }

    fn glyph_index(&self, c: char) -> Option<GlyphId> {
        self.charmap.map(c)
    }

    fn units_per_em(&self) -> f32 {
        self.metrics.units_per_em as f32
    }
}

fn font(kind: FontKind) -> &'static LoadedFont {
    static SANS: OnceLock<LoadedFont> = OnceLock::new();
    static MONO: OnceLock<LoadedFont> = OnceLock::new();
    static HAND: OnceLock<LoadedFont> = OnceLock::new();
    let (cell, data): (&OnceLock<LoadedFont>, &'static [u8]) = match kind {
        FontKind::Sans => (&SANS, epaint_default_fonts::UBUNTU_LIGHT),
        FontKind::Mono => (&MONO, epaint_default_fonts::HACK_REGULAR),
        FontKind::Hand => (&HAND, include_bytes!("../../assets/fonts/Virgil.otf")),
    };
    cell.get_or_init(|| LoadedFont::parse(data))
}

/// The emoji and symbols font, a last resort for characters the text fonts lack (arrows come from
/// Hack, a calendar from here).
fn symbols() -> &'static LoadedFont {
    static SYMBOLS: OnceLock<LoadedFont> = OnceLock::new();
    SYMBOLS.get_or_init(|| LoadedFont::parse(epaint_default_fonts::NOTO_EMOJI_REGULAR))
}

/// A character the fonts have no glyph for, as one they have (a heavy cross as a multiplication sign, a check mark as the heavy one).
fn substitute(c: char) -> char {
    match c {
        '✕' | '✖' => '×',
        '✓' => '✔',
        _ => c,
    }
}

/// The font that actually has a glyph for `c`: the text's own, else Sans, Hack, then the symbols.
fn face_for(kind: FontKind, c: char) -> (&'static LoadedFont, GlyphId) {
    let c = substitute(c);
    for f in [font(kind), font(FontKind::Sans), font(FontKind::Mono), symbols()] {
        if let Some(g) = f.glyph_index(c) {
            return (f, g);
        }
    }
    (font(kind), GlyphId::NOTDEF)
}

/// Whether some bundled font has a glyph for `c` (the PDF draws it, not an empty box).
#[cfg(test)]
pub fn has_glyph(c: char) -> bool {
    c.is_whitespace() || face_for(FontKind::Sans, c).1 != GlyphId::NOTDEF
}

/// Width of one character in points.
pub fn char_width(kind: FontKind, size: f32, c: char) -> f32 {
    let (f, g) = face_for(kind, c);
    f.advances.advance_width(g).unwrap_or(0.0) * size / f.units_per_em()
}

/// Width of a string in points.
pub fn text_width(kind: FontKind, size: f32, s: &str) -> f32 {
    s.chars().map(|c| char_width(kind, size, c)).sum()
}

/// Ascender of a font as a fraction of the size.
pub fn ascent(kind: FontKind) -> f32 {
    let f = font(kind);
    f.metrics.ascent / f.units_per_em()
}

/// WinAnsiEncoding byte for a character, if it has one.
pub fn win_ansi(c: char) -> Option<u8> {
    let u = c as u32;
    if (0x20..0x7f).contains(&u) || (0xa0..=0xff).contains(&u) {
        return Some(u as u8);
    }
    const HIGH: [(char, u8); 27] = [
        ('€', 0x80),
        ('‚', 0x82),
        ('ƒ', 0x83),
        ('„', 0x84),
        ('…', 0x85),
        ('†', 0x86),
        ('‡', 0x87),
        ('ˆ', 0x88),
        ('‰', 0x89),
        ('Š', 0x8a),
        ('‹', 0x8b),
        ('Œ', 0x8c),
        ('Ž', 0x8e),
        ('‘', 0x91),
        ('’', 0x92),
        ('“', 0x93),
        ('”', 0x94),
        ('•', 0x95),
        ('–', 0x96),
        ('—', 0x97),
        ('˜', 0x98),
        ('™', 0x99),
        ('š', 0x9a),
        ('›', 0x9b),
        ('œ', 0x9c),
        ('ž', 0x9e),
        ('Ÿ', 0x9f),
    ];
    HIGH.iter().find(|(ch, _)| *ch == c).map(|(_, b)| *b)
}

fn char_of_win_ansi(b: u8) -> Option<char> {
    match b {
        0x20..=0x7e | 0xa0..=0xff => Some(b as char),
        _ => ['€', '\0', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\0', 'Ž', '\0', '\0', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\0', 'ž', 'Ÿ']
            .get((b as usize).wrapping_sub(0x80))
            .copied()
            .filter(|c| *c != '\0'),
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Rgba {
    #[cfg(test)]
    pub const BLACK: Rgba = Rgba(0, 0, 0, 255);
    fn rgb(self) -> String {
        format!("{:.3} {:.3} {:.3}", self.0 as f32 / 255.0, self.1 as f32 / 255.0, self.2 as f32 / 255.0)
    }
}

pub struct Image {
    pub width: u32,
    pub height: u32,
    /// RGB bytes
    pub rgb: Vec<u8>,
    /// optional alpha channel
    pub alpha: Option<Vec<u8>>,
}

/// How a piece of text is drawn: the font, the size, the colour, and whether it is slanted or bold.
#[derive(Clone, Copy)]
pub struct TextStyle {
    pub kind: FontKind,
    pub size: f32,
    pub color: Rgba,
    pub italic: bool,
    pub bold: bool,
}

impl TextStyle {
    /// Upright, regular weight.
    pub fn plain(kind: FontKind, size: f32, color: Rgba) -> TextStyle {
        TextStyle { kind, size, color, italic: false, bold: false }
    }
}

pub struct Page {
    pub width: f32,
    pub height: f32,
    ops: String,
    images: BTreeSet<usize>,
    alphas: BTreeSet<u8>,
    fonts: BTreeSet<FontKind>,
    links: Vec<([f32; 4], String)>,
    /// clickable rectangles that go to a place in the document (PDF-008)
    jumps: Vec<([f32; 4], Dest)>,
}

/// A place in the document: a page (by index) and the distance from its top in points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Dest {
    pub page: usize,
    pub y: f32,
}

/// An entry of the bookmarks (the document outline of PDF readers): nested by `level`, a deeper
/// level belongs to the nearest entry before it with a shallower one (PDF-008).
#[derive(Clone, Debug)]
pub struct Bookmark {
    pub level: usize,
    pub title: String,
    pub to: Dest,
}

impl Page {
    fn y(&self, y: f32) -> f32 {
        self.height - y
    }

    fn set_alpha(&mut self, a: u8) {
        self.alphas.insert(a);
        let _ = write!(self.ops, "/GA{a} gs ");
    }

    fn fill_color(&mut self, c: Rgba) {
        self.set_alpha(c.3);
        let _ = write!(self.ops, "{} rg ", c.rgb());
    }

    fn stroke_color(&mut self, c: Rgba, width: f32) {
        self.set_alpha(c.3);
        let _ = write!(self.ops, "{} RG {:.3} w 1 J 1 j ", c.rgb(), width.max(0.1));
    }

    /// A polyline / polygon. `fill` and/or `stroke`.
    pub fn path(&mut self, pts: &[[f32; 2]], closed: bool, fill: Option<Rgba>, stroke: Option<(Rgba, f32)>) {
        if pts.len() < 2 || (fill.is_none() && stroke.is_none()) {
            return;
        }
        self.ops.push_str("q ");
        if let Some(c) = fill {
            self.fill_color(c);
        }
        if let Some((c, w)) = stroke {
            self.stroke_color(c, w);
        }
        for (i, p) in pts.iter().enumerate() {
            let _ = write!(self.ops, "{:.2} {:.2} {} ", p[0], self.y(p[1]), if i == 0 { "m" } else { "l" });
        }
        if closed {
            self.ops.push_str("h ");
        }
        let op = match (fill.is_some(), stroke.is_some()) {
            (true, true) => "B",
            (true, false) => "f",
            _ => "S",
        };
        let _ = writeln!(self.ops, "{op} Q");
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, fill: Option<Rgba>, stroke: Option<(Rgba, f32)>) {
        self.path(&[[x, y], [x + w, y], [x + w, y + h], [x, y + h]], true, fill, stroke);
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, fill: Option<Rgba>, stroke: Option<(Rgba, f32)>) {
        let pts: Vec<[f32; 2]> = (0..24)
            .map(|i| {
                let a = i as f32 / 24.0 * std::f32::consts::TAU;
                [cx + r * a.cos(), cy + r * a.sin()]
            })
            .collect();
        self.path(&pts, true, fill, stroke);
    }

    pub fn line(&mut self, a: [f32; 2], b: [f32; 2], color: Rgba, width: f32) {
        self.path(&[a, b], false, None, Some((color, width)));
    }

    /// Text on a baseline at (x, y). Returns the width. Embedded fonts give selectable text;
    /// the hand-drawn font and characters outside WinAnsi are drawn as glyph outlines.
    pub fn text(&mut self, style: TextStyle, x: f32, y: f32, s: &str) -> f32 {
        let TextStyle { kind, size, color, italic, bold } = style;
        let mut cx = x;
        let mut run = String::new();
        let mut run_x = x;
        let skew = if italic { 0.2 } else { 0.0 };
        let flush = |page: &mut Page, run: &mut String, run_x: f32| {
            if run.is_empty() {
                return;
            }
            page.fonts.insert(kind);
            page.fill_color(color);
            if bold {
                page.stroke_color(color, size * 0.03);
            }
            let hex: String = run.chars().filter_map(win_ansi).map(|b| format!("{b:02x}")).collect();
            let _ = writeln!(page.ops, "q BT /{} {:.2} Tf {} Tr 1 0 {skew} 1 {:.2} {:.2} Tm <{hex}> Tj ET Q", font_res(kind), size, if bold { 2 } else { 0 }, run_x, page.y(y));
            run.clear();
        };
        for c in s.chars() {
            let w = char_width(kind, size, c);
            let embedded = kind != FontKind::Hand && win_ansi(c).is_some() && font(kind).glyph_index(c).is_some();
            if embedded {
                if run.is_empty() {
                    run_x = cx;
                }
                run.push(c);
            } else {
                flush(self, &mut run, run_x);
                if !c.is_whitespace() {
                    self.glyph_outline(style, cx, y, c, skew);
                }
            }
            cx += w;
        }
        flush(self, &mut run, run_x);
        cx - x
    }

    fn glyph_outline(&mut self, style: TextStyle, x: f32, y: f32, c: char, skew: f32) {
        let TextStyle { kind, size, color, .. } = style;
        let (f, g) = face_for(kind, c);
        let scale = size / f.units_per_em();
        let mut b = PathOps { out: String::new(), x, y: self.height - y, scale, skew };
        let drawn = f.outlines.get(g).is_some_and(|o| o.draw(DrawSettings::unhinted(Size::unscaled(), LocationRef::default()), &mut b).is_ok());
        if drawn {
            self.ops.push_str("q ");
            self.fill_color(color);
            self.ops.push_str(&b.out);
            self.ops.push_str("f Q\n");
        }
    }

    /// A typeset formula (PDF-012) with the left end of its baseline at (x, baseline): the outlines
    /// of its glyphs from the math font, and its rules.
    pub fn math(&mut self, layout: &crate::math::layout::Layout, x: f32, baseline: f32, color: Rgba) {
        use crate::math::layout::Item;
        let font = crate::math::font::font();
        for item in &layout.items {
            match item {
                Item::Glyph { gid, x: gx, y: gy, size } => {
                    let mut pen = PathOps { out: String::new(), x: x + gx, y: self.height - baseline + gy, scale: size / font.upem(), skew: 0.0 };
                    font.draw(*gid, &mut pen);
                    self.ops.push_str("q ");
                    self.fill_color(color);
                    self.ops.push_str(&pen.out);
                    self.ops.push_str("f Q\n");
                }
                Item::Rule { x: rx, y: ry, w, h } => self.rect(x + rx, baseline - ry - h, *w, *h, Some(color), None),
            }
        }
    }

    /// Place an image (from `Doc::add_image`) into the rect.
    pub fn image(&mut self, index: usize, x: f32, y: f32, w: f32, h: f32) {
        self.images.insert(index);
        let _ = writeln!(self.ops, "q {:.2} 0 0 {:.2} {:.2} {:.2} cm /Im{index} Do Q", w, h, x, self.y(y + h));
    }

    /// A clickable link to a web address over the rect.
    pub fn link(&mut self, x: f32, y: f32, w: f32, h: f32, uri: &str) {
        let r = [x, self.y(y + h), x + w, self.y(y)];
        self.links.push((r, uri.to_string()));
    }

    /// A clickable rect that goes to a place in the document (a page and the distance from its top).
    pub fn jump(&mut self, x: f32, y: f32, w: f32, h: f32, to: Dest) {
        let r = [x, self.y(y + h), x + w, self.y(y)];
        self.jumps.push((r, to));
    }
}

struct PathOps {
    out: String,
    x: f32,
    y: f32,
    scale: f32,
    skew: f32,
}

impl PathOps {
    fn p(&self, gx: f32, gy: f32) -> (f32, f32) {
        let px = self.x + gx * self.scale + gy * self.scale * self.skew;
        (px, self.y + gy * self.scale)
    }
}

impl OutlinePen for PathOps {
    fn move_to(&mut self, x: f32, y: f32) {
        let (a, b) = self.p(x, y);
        let _ = write!(self.out, "{a:.2} {b:.2} m ");
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (a, b) = self.p(x, y);
        let _ = write!(self.out, "{a:.2} {b:.2} l ");
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // PDF has only cubic curves: draw the quadratic as a cubic through the same control point
        let (c, d) = self.p(x1, y1);
        let (e, f) = self.p(x, y);
        let _ = write!(self.out, "{c:.2} {d:.2} {c:.2} {d:.2} {e:.2} {f:.2} c ");
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b) = self.p(x1, y1);
        let (c, d) = self.p(x2, y2);
        let (e, f) = self.p(x, y);
        let _ = write!(self.out, "{a:.2} {b:.2} {c:.2} {d:.2} {e:.2} {f:.2} c ");
    }
    fn close(&mut self) {
        self.out.push_str("h ");
    }
}

fn font_res(k: FontKind) -> &'static str {
    match k {
        FontKind::Sans => "F1",
        FontKind::Mono => "F2",
        FontKind::Hand => "F3",
    }
}

#[derive(Default)]
pub struct Doc {
    pub pages: Vec<Page>,
    images: Vec<Image>,
    pub title: String,
    /// the bookmarks, in document order (PDF-008)
    pub bookmarks: Vec<Bookmark>,
}

fn deflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec_zlib(data, 6)
}

fn esc(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x80 => out.push(c),
            _ => out.push('?'),
        }
    }
    out
}

/// A PDF text string with any characters: UTF-16BE in hex.
fn text_string(s: &str) -> String {
    let mut out = String::from("<FEFF");
    for u in s.encode_utf16() {
        let _ = write!(out, "{u:04X}");
    }
    out.push('>');
    out
}

impl Doc {
    pub fn new(title: &str) -> Doc {
        Doc { title: title.to_string(), ..Default::default() }
    }

    pub fn add_page(&mut self, width: f32, height: f32) -> usize {
        self.pages.push(Page { width, height, ops: String::new(), images: BTreeSet::new(), alphas: BTreeSet::new(), fonts: BTreeSet::new(), links: vec![], jumps: vec![] });
        self.pages.len() - 1
    }

    pub fn page(&mut self, i: usize) -> &mut Page {
        &mut self.pages[i]
    }

    pub fn add_image(&mut self, img: Image) -> usize {
        self.images.push(img);
        self.images.len() - 1
    }

    /// The bookmarks as PDF objects (PDF-008): the root of the outline, with entries nested by
    /// level, each with its parent, its neighbours, its first and last child and the number of
    /// entries below it. Returns the root's object number, None without bookmarks.
    fn outline_objects(&self, objs: &mut Vec<Vec<u8>>, dest: &dyn Fn(&Dest) -> String) -> Option<usize> {
        let marks = &self.bookmarks;
        if marks.is_empty() {
            return None;
        }
        objs.push(Vec::new());
        let root = objs.len();
        let ids: Vec<usize> = marks
            .iter()
            .map(|_| {
                objs.push(Vec::new());
                objs.len()
            })
            .collect();
        let mut parent: Vec<Option<usize>> = Vec::new();
        let mut open: Vec<usize> = Vec::new();
        for (i, m) in marks.iter().enumerate() {
            while open.last().is_some_and(|&j| marks[j].level >= m.level) {
                open.pop();
            }
            parent.push(open.last().copied());
            open.push(i);
        }
        let kids = |p: Option<usize>| -> Vec<usize> { (0..marks.len()).filter(|&i| parent[i] == p).collect() };
        // the entries below an entry: all that come after it until one is as shallow as it
        let below = |i: usize| marks[i + 1..].iter().take_while(|m| m.level > marks[i].level).count();
        let top = kids(None);
        objs[root - 1] = format!("<< /Type /Outlines /First {} 0 R /Last {} 0 R /Count {} >>", ids[top[0]], ids[*top.last().unwrap_or(&0)], marks.len()).into_bytes();
        for (i, m) in marks.iter().enumerate() {
            let siblings = kids(parent[i]);
            let at = siblings.iter().position(|&j| j == i).unwrap_or(0);
            let mut o = format!("<< /Title {} /Parent {} 0 R /Dest {}", text_string(&m.title), parent[i].map_or(root, |p| ids[p]), dest(&m.to));
            if at > 0 {
                let _ = write!(o, " /Prev {} 0 R", ids[siblings[at - 1]]);
            }
            if let Some(next) = siblings.get(at + 1) {
                let _ = write!(o, " /Next {} 0 R", ids[*next]);
            }
            let mine = kids(Some(i));
            if let (Some(first), Some(last)) = (mine.first(), mine.last()) {
                let _ = write!(o, " /First {} 0 R /Last {} 0 R /Count {}", ids[*first], ids[*last], below(i));
            }
            o.push_str(" >>");
            objs[ids[i] - 1] = o.into_bytes();
        }
        Some(root)
    }

    /// Serialize the whole document.
    pub fn finish(self) -> Vec<u8> {
        let mut objs: Vec<Vec<u8>> = Vec::new(); // object n is objs[n-1]
        let reserve = |objs: &mut Vec<Vec<u8>>| {
            objs.push(Vec::new());
            objs.len()
        };
        let catalog = reserve(&mut objs);
        let pages_id = reserve(&mut objs);
        let info = reserve(&mut objs);
        // fonts actually used
        let used: BTreeSet<FontKind> = self.pages.iter().flat_map(|p| p.fonts.iter().copied()).collect();
        let mut font_ids: Vec<(FontKind, usize)> = Vec::new();
        for kind in used {
            let lf = font(kind);
            let m = &lf.metrics;
            let upem = lf.units_per_em();
            let file_id = reserve(&mut objs);
            let compressed = deflate(lf.data);
            let mut fo = format!("<< /Length {} /Length1 {} /Filter /FlateDecode >>\nstream\n", compressed.len(), lf.data.len()).into_bytes();
            fo.extend(compressed);
            fo.extend(b"\nendstream");
            objs[file_id - 1] = fo;
            let desc_id = reserve(&mut objs);
            let bb = m.bounds.expect("bundled font has a head table");
            let name = match kind {
                FontKind::Sans => "UbuntuLight",
                FontKind::Mono => "Hack",
                FontKind::Hand => "Virgil",
            };
            let s = |v: f32| (v * 1000.0 / upem).round() as i32;
            objs[desc_id - 1] = format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags 32 /FontBBox [{} {} {} {}] /ItalicAngle 0 /Ascent {} /Descent {} /CapHeight {} /StemV 80 /FontFile2 {file_id} 0 R >>",
                s(bb.x_min),
                s(bb.y_min),
                s(bb.x_max),
                s(bb.y_max),
                s(m.ascent),
                s(m.descent),
                s(m.cap_height.unwrap_or(m.ascent))
            )
            .into_bytes();
            let widths: Vec<String> = (32u8..=255).map(|b| char_of_win_ansi(b).and_then(|c| lf.glyph_index(c)).and_then(|g| lf.advances.advance_width(g)).map(|a| (a * 1000.0 / upem).round() as i32).unwrap_or(0).to_string()).collect();
            let fid = reserve(&mut objs);
            objs[fid - 1] = format!("<< /Type /Font /Subtype /TrueType /BaseFont /{name} /FirstChar 32 /LastChar 255 /Widths [{}] /Encoding /WinAnsiEncoding /FontDescriptor {desc_id} 0 R >>", widths.join(" ")).into_bytes();
            font_ids.push((kind, fid));
        }
        // images
        let mut image_ids = Vec::new();
        for img in &self.images {
            let smask = img.alpha.as_ref().map(|a| {
                let id = reserve(&mut objs);
                let data = deflate(a);
                let mut o = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {} >>\nstream\n", img.width, img.height, data.len()).into_bytes();
                o.extend(data);
                o.extend(b"\nendstream");
                objs[id - 1] = o;
                id
            });
            let id = reserve(&mut objs);
            let data = deflate(&img.rgb);
            let sm = smask.map(|s| format!(" /SMask {s} 0 R")).unwrap_or_default();
            let mut o = format!("<< /Type /XObject /Subtype /Image /Width {} /Height {} /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /FlateDecode /Length {}{sm} >>\nstream\n", img.width, img.height, data.len()).into_bytes();
            o.extend(data);
            o.extend(b"\nendstream");
            objs[id - 1] = o;
            image_ids.push(id);
        }
        // pages (their ids first: links and bookmarks point to pages that come later)
        let page_ids: Vec<usize> = self.pages.iter().map(|_| reserve(&mut objs)).collect();
        let dest = |d: &Dest| {
            let (id, height) = page_ids.get(d.page).zip(self.pages.get(d.page)).map_or((page_ids[0], 0.0), |(id, p)| (*id, p.height));
            format!("[{id} 0 R /XYZ 0 {:.2} 0]", height - d.y)
        };
        let mut kids = Vec::new();
        for (pi, p) in self.pages.iter().enumerate() {
            let content_id = reserve(&mut objs);
            let data = deflate(p.ops.as_bytes());
            let mut o = format!("<< /Length {} /Filter /FlateDecode >>\nstream\n", data.len()).into_bytes();
            o.extend(data);
            o.extend(b"\nendstream");
            objs[content_id - 1] = o;
            let fonts: String = font_ids.iter().map(|(k, id)| format!("/{} {id} 0 R ", font_res(*k))).collect();
            let xobj: String = p.images.iter().map(|i| format!("/Im{i} {} 0 R ", image_ids[*i])).collect();
            let gs: String = p.alphas.iter().map(|a| format!("/GA{a} << /Type /ExtGState /ca {:.3} /CA {:.3} >> ", *a as f32 / 255.0, *a as f32 / 255.0)).collect();
            let mut annots = Vec::new();
            for (r, uri) in &p.links {
                let id = reserve(&mut objs);
                objs[id - 1] = format!("<< /Type /Annot /Subtype /Link /Rect [{:.2} {:.2} {:.2} {:.2}] /Border [0 0 0] /A << /S /URI /URI ({}) >> >>", r[0], r[1], r[2], r[3], esc(uri)).into_bytes();
                annots.push(format!("{id} 0 R"));
            }
            for (r, to) in &p.jumps {
                let id = reserve(&mut objs);
                objs[id - 1] = format!("<< /Type /Annot /Subtype /Link /Rect [{:.2} {:.2} {:.2} {:.2}] /Border [0 0 0] /Dest {} >>", r[0], r[1], r[2], r[3], dest(to)).into_bytes();
                annots.push(format!("{id} 0 R"));
            }
            let annots = if annots.is_empty() { String::new() } else { format!(" /Annots [{}]", annots.join(" ")) };
            let page_id = page_ids[pi];
            objs[page_id - 1] =
                format!("<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {:.2} {:.2}] /Contents {content_id} 0 R /Resources << /Font << {fonts}>> /XObject << {xobj}>> /ExtGState << {gs}>> >>{annots} >>", p.width, p.height)
                    .into_bytes();
            kids.push(format!("{page_id} 0 R"));
        }
        let outlines = self.outline_objects(&mut objs, &dest);
        let outlines = outlines.map(|id| format!(" /Outlines {id} 0 R")).unwrap_or_default();
        objs[catalog - 1] = format!("<< /Type /Catalog /Pages {pages_id} 0 R{outlines} >>").into_bytes();
        objs[pages_id - 1] = format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), kids.len()).into_bytes();
        objs[info - 1] = format!("<< /Title ({}) /Producer (Shale) >>", esc(&self.title)).into_bytes();
        let mut out = b"%PDF-1.5\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n", i + 1).into_bytes());
            out.extend(o);
            out.extend(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).into_bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").into_bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root {catalog} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).into_bytes());
        out
    }
}

/// Inflate a zlib stream (used by tests to look inside content streams).
#[cfg(test)]
pub fn inflate(data: &[u8]) -> Vec<u8> {
    miniz_oxide::inflate::decompress_to_vec_zlib(data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: PDF-001
    #[test]
    fn writes_a_pdf_that_renders() {
        let mut d = Doc::new("Test");
        let p = d.add_page(300.0, 200.0);
        d.page(p).rect(10.0, 10.0, 100.0, 50.0, Some(Rgba(200, 30, 30, 255)), None);
        let w = d.page(p).text(TextStyle::plain(FontKind::Sans, 18.0, Rgba::BLACK), 20.0, 120.0, "Hello Üml €");
        assert!(w > 50.0);
        d.page(p).text(TextStyle::plain(FontKind::Hand, 18.0, Rgba(0, 0, 200, 128)), 20.0, 160.0, "hand → drawn");
        d.page(p).link(20.0, 100.0, 80.0, 20.0, "https://example.com");
        let img = d.add_image(Image { width: 2, height: 2, rgb: vec![255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255], alpha: Some(vec![255, 128, 255, 0]) });
        d.page(p).image(img, 200.0, 20.0, 40.0, 40.0);
        let bytes = d.finish();
        assert!(bytes.starts_with(b"%PDF-1.5"));
        let pdf = hayro::hayro_syntax::Pdf::new(bytes).expect("our PDF parses");
        assert_eq!(pdf.pages().len(), 1);
        let pm = hayro::render(&pdf.pages()[0], &hayro::RenderCache::new(), &Default::default(), &Default::default(), &hayro::PixmapSettings { x_scale: 1.0, y_scale: 1.0, bg_color: hayro::vello_cpu::color::palette::css::WHITE });
        let px = pm.data_as_u8_slice();
        // the red rectangle is where we put it (x 50, y 30 from the top)
        let i = ((30 * pm.width() as usize) + 50) * 4;
        assert!(px[i] > 150 && px[i + 1] < 100, "red rectangle rendered: {:?}", &px[i..i + 4]);
    }

    // spec: PDF-008
    #[test]
    fn bookmarks_nest_by_level_and_links_to_pages_are_destinations_and_the_pdf_still_renders() {
        let mut d = Doc::new("Test");
        let (a, b) = (d.add_page(300.0, 200.0), d.add_page(300.0, 200.0));
        d.page(a).jump(10.0, 10.0, 100.0, 20.0, Dest { page: b, y: 40.0 });
        d.page(b).text(TextStyle::plain(FontKind::Sans, 12.0, Rgba::BLACK), 10.0, 50.0, "second");
        for (level, title, page) in [(2, "One Ü", a), (3, "Sub", b), (3, "Sub 2", b), (2, "Two", b)] {
            d.bookmarks.push(Bookmark { level, title: title.into(), to: Dest { page, y: 40.0 } });
        }
        let bytes = d.finish();
        let raw = String::from_utf8_lossy(&bytes).into_owned();
        let obj = |needle: &str| raw.split("\nendobj").find(|o| o.contains(needle)).unwrap_or_else(|| panic!("no object with {needle}")).to_string();
        let id = |o: &str| o.split_whitespace().next().unwrap().to_string();
        let (one, sub, sub2, two) = (obj(&text_string("One Ü")), obj(&format!("/Title {}", text_string("Sub"))), obj(&text_string("Sub 2")), obj(&text_string("Two")));
        assert!(raw.contains("/Type /Outlines") && raw.contains("/Count 4"), "the root counts all entries");
        assert!(one.contains("/Count 2") && one.contains(&format!("/First {} 0 R /Last {} 0 R", id(&sub), id(&sub2))), "{one}");
        assert!(sub.contains(&format!("/Parent {} 0 R", id(&one))) && sub.contains(&format!("/Next {} 0 R", id(&sub2))) && !sub.contains("/Prev"), "{sub}");
        assert!(sub2.contains(&format!("/Prev {} 0 R", id(&sub))) && !sub2.contains("/Next"), "{sub2}");
        assert!(two.contains(&format!("/Prev {} 0 R", id(&one))) && !two.contains("/First"), "{two}");
        assert!(one.contains("/XYZ 0 160.00 0]"), "a place 40 points from the top of a 200 point page: {one}");
        let link = obj("/Subtype /Link");
        assert!(link.contains("/Dest [") && link.contains("/XYZ 0 160.00 0]") && !link.contains("/URI"), "{link}");
        let pdf = hayro::hayro_syntax::Pdf::new(bytes).expect("a PDF with bookmarks parses");
        assert_eq!(pdf.pages().len(), 2);
        let mut plain = Doc::new("Plain");
        plain.add_page(100.0, 100.0);
        assert!(!String::from_utf8_lossy(&plain.finish()).contains("/Outlines"), "no bookmarks, no outline");
    }

    // spec: PDF-001
    #[test]
    fn win_ansi_mapping() {
        assert_eq!(win_ansi('A'), Some(0x41));
        assert_eq!(win_ansi('ä'), Some(0xe4));
        assert_eq!(win_ansi('€'), Some(0x80));
        assert_eq!(win_ansi('→'), None);
        assert_eq!(char_of_win_ansi(0x95), Some('•'));
    }

    // spec: PDF-001, PDF-010
    #[test]
    fn font_metrics_come_from_the_bundled_fonts_in_design_units() {
        // Ubuntu Light: 1000 units per em, "a" is 509 wide, ascender 932 (hhea)
        assert!((char_width(FontKind::Sans, 1000.0, 'a') - 509.0).abs() < 0.01);
        assert!((ascent(FontKind::Sans) - 0.932).abs() < 1e-4);
        // Hack: 2048 units per em, every glyph is 1233 wide
        assert!((char_width(FontKind::Mono, 2048.0, 'a') - 1233.0).abs() < 0.01);
        assert!((text_width(FontKind::Mono, 2048.0, "abc") - 3.0 * 1233.0).abs() < 0.01);
        // a character no font has is as wide as the .notdef glyph of the font
        let (f, g) = face_for(FontKind::Sans, '\u{10ffff}');
        assert!(std::ptr::eq(f, font(FontKind::Sans)) && g == GlyphId::NOTDEF);
        assert_eq!(char_width(FontKind::Sans, 10.0, '\u{10ffff}'), f.advances.advance_width(g).unwrap() * 10.0 / 1000.0);
        assert!(!has_glyph('\u{10ffff}'));
        // arrows come from Hack, a calendar from the symbols font, a heavy cross as a multiplication sign, a check mark as the heavy one (PDF-010)
        for c in ['→', '←', '↑', '↓', '↗', '⋯', '▸', '📅', '✕', '✓', '✔'] {
            assert!(has_glyph(c), "{c}");
        }
        assert_eq!(char_width(FontKind::Sans, 10.0, '✕'), char_width(FontKind::Sans, 10.0, '×'));
        assert_eq!(char_width(FontKind::Sans, 10.0, '✓'), char_width(FontKind::Sans, 10.0, '✔'));
        assert!(!std::ptr::eq(face_for(FontKind::Sans, '→').0, font(FontKind::Sans)));
    }

    // spec: PDF-001
    #[test]
    fn hand_written_text_is_drawn_as_filled_outlines_and_embedded_text_is_not() {
        let mut d = Doc::new("Test");
        let p = d.add_page(300.0, 200.0);
        d.page(p).text(TextStyle::plain(FontKind::Hand, 20.0, Rgba::BLACK), 10.0, 50.0, "Ag");
        let ops = d.page(p).ops.clone();
        assert!(ops.contains(" m ") && ops.contains(" c ") && ops.contains("h f Q"), "outline path ops: {ops}");
        assert!(!ops.contains(" Tj "), "no selectable text for the hand font");
        let mut d = Doc::new("Test");
        let p = d.add_page(300.0, 200.0);
        d.page(p).text(TextStyle::plain(FontKind::Sans, 20.0, Rgba::BLACK), 10.0, 50.0, "Ag");
        let ops = d.page(p).ops.clone();
        assert!(ops.contains(" Tj ") && !ops.contains(" m "), "embedded text: {ops}");
    }

    // spec: PDF-001
    #[test]
    fn the_font_descriptor_has_the_metrics_of_the_font_in_a_thousand_units() {
        let mut d = Doc::new("Test");
        let p = d.add_page(300.0, 200.0);
        d.page(p).text(TextStyle::plain(FontKind::Mono, 12.0, Rgba::BLACK), 10.0, 50.0, "a");
        let text = String::from_utf8_lossy(&d.finish()).into_owned();
        // Hack: bbox (-954 -605 1355 2027) / 2048, ascent 1901, descent -483, cap height 1493
        assert!(text.contains("/FontBBox [-466 -295 662 990] /ItalicAngle 0 /Ascent 928 /Descent -236 /CapHeight 729"), "descriptor");
        // "a" (0x61) is 1233 / 2048 em = 602
        assert!(text.contains(" 602 "), "widths");
    }
}
