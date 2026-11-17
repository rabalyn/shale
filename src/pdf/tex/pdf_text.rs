//! The text of a PDF made by LuaLaTeX, for the tests of the writer: every glyph a page draws, by
//! the `ToUnicode` map of its font, with hayro's interpreter. Not a layout engine: the text of a
//! page comes in the order it was drawn, so tests look for words with the spaces taken out.

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::kurbo::{Affine, BezPath, Rect};
use hayro::hayro_interpret::{interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask};

/// The text of a page, and how many pictures it draws.
#[derive(Default)]
struct Collect(String, usize);

impl<'a> Device<'a> for Collect {
    fn draw_path(&mut self, _: &BezPath, _: DrawProps<'a>, _: &DrawMode) {}
    fn push_clip_path(&mut self, _: &ClipPath) {}
    fn push_transparency_group(&mut self, _: f32, _: Option<SoftMask<'a>>, _: BlendMode) {}
    fn draw_glyph_run(&mut self, run: &GlyphRun<'_, 'a>, _: DrawProps<'a>, _: &DrawMode) {
        for g in run.glyphs() {
            match g.as_unicode() {
                Some(BfString::Char(c)) => self.0.push(c),
                Some(BfString::String(s)) => self.0.push_str(&s),
                None => self.0.push('\u{fffd}'),
            }
        }
        self.0.push(' ');
    }
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {
        self.1 += 1;
    }
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// The text of every page of a PDF.
pub(super) fn page_texts(pdf: Vec<u8>) -> Vec<String> {
    pages(pdf).into_iter().map(|c| c.0).collect()
}

/// How many pictures the pages of a PDF draw, all pages together.
pub(super) fn pictures_drawn(pdf: Vec<u8>) -> usize {
    pages(pdf).iter().map(|c| c.1).sum()
}

fn pages(pdf: Vec<u8>) -> Vec<Collect> {
    let doc = hayro::hayro_syntax::Pdf::new(pdf).unwrap_or_else(|e| panic!("not a readable PDF: {e:?}"));
    let cache = InterpreterCache::new();
    doc.pages()
        .iter()
        .map(|page| {
            let (w, h) = page.render_dimensions();
            let mut ctx = Context::new(Affine::IDENTITY, Rect::new(0.0, 0.0, w as f64, h as f64), &cache, page.xref(), InterpreterSettings::default());
            let mut device = Collect::default();
            interpret_page(page, &mut ctx, &mut device);
            device
        })
        .collect()
}

/// The text of the PDF with every space and line break taken out, all pages in a row.
pub(super) fn squashed(pdf: Vec<u8>) -> String {
    page_texts(pdf).concat().chars().filter(|c| !c.is_whitespace()).collect()
}

/// The streams of a PDF, inflated: LuaLaTeX keeps the pages' contents, its outline items and its
/// annotations in compressed streams.
fn streams(pdf: &[u8]) -> Vec<Vec<u8>> {
    let find = |from: usize, needle: &[u8]| pdf[from..].windows(needle.len()).position(|w| w == needle).map(|p| from + p);
    let (mut out, mut at) = (Vec::new(), 0);
    while let Some(p) = find(at, b"stream") {
        // `endstream` has the word in it too: only a `stream` that ends its line starts a stream
        let start = p + 6;
        let (Some(rest), true) = (pdf.get(start..), p == 0 || pdf[p - 1] != b'd') else {
            at = start;
            continue;
        };
        let body = rest.strip_prefix(b"\r\n").or_else(|| rest.strip_prefix(b"\n")).unwrap_or(rest);
        let skipped = rest.len() - body.len();
        let Some(end) = find(start + skipped, b"endstream") else { break };
        out.push(crate::pdf::write::inflate(pdf[start + skipped..end].trim_ascii_end()));
        at = end + 9;
    }
    out
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

/// How many bookmarks a PDF has: its outline items are in compressed object streams, so every
/// stream is inflated and its `/Title` entries are counted (the document's own title is in the
/// uncompressed information dictionary and is not counted).
pub(super) fn outline_entries(pdf: &[u8]) -> usize {
    streams(pdf).iter().map(|s| count(s, b"/Title")).sum()
}

/// How many link annotations of the PDF go to a named destination inside it whose name starts
/// with `prefix` (`/A << /S /GoTo /D (shale-h12) >>`), in the file or in its compressed streams.
pub(super) fn links_inside(pdf: &[u8], prefix: &str) -> usize {
    let needle = format!("/S /GoTo /D ({prefix}");
    count(pdf, needle.as_bytes()) + streams(pdf).iter().map(|s| count(s, needle.as_bytes())).sum::<usize>()
}

/// Whether a page fills something with this colour: `r g b rg` in its contents, or `c m y k k`
/// for a class that sets xcolor to CMYK (the leaflet), converted as xcolor does; to 0.002.
pub(super) fn fills_with(pdf: &[u8], rgb: [u8; 3]) -> bool {
    let want = rgb.map(|c| c as f32 / 255.0);
    let k = want.iter().map(|v| 1.0 - v).fold(1.0, f32::min);
    let cmyk = [1.0 - want[0] - k, 1.0 - want[1] - k, 1.0 - want[2] - k, k];
    let near = |words: &[&str], values: &[f32]| words.iter().zip(values).all(|(w, v)| w.parse::<f32>().is_ok_and(|x| (x - v).abs() < 0.002));
    streams(pdf).iter().any(|s| {
        let text = String::from_utf8_lossy(s);
        let words: Vec<&str> = text.split_whitespace().collect();
        words.windows(4).any(|w| w[3] == "rg" && near(&w[..3], &want)) || words.windows(5).any(|w| w[4] == "k" && near(&w[..4], &cmyk))
    })
}
