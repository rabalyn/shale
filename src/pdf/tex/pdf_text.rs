//! The text of a PDF made by LuaLaTeX, for the tests of the writer: every glyph a page draws, by
//! the `ToUnicode` map of its font, with hayro's interpreter. Not a layout engine: the text of a
//! page comes in the order it was drawn, so tests look for words with the spaces taken out.

use hayro::hayro_interpret::font::GlyphRun;
use hayro::hayro_interpret::hayro_cmap::BfString;
use hayro::hayro_interpret::kurbo::{Affine, BezPath, Rect};
use hayro::hayro_interpret::{interpret_page, BlendMode, ClipPath, Context, Device, DrawMode, DrawProps, Image, ImageDrawProps, InterpreterCache, InterpreterSettings, SoftMask};

#[derive(Default)]
struct Collect(String);

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
    fn draw_image(&mut self, _: Image<'a, '_>, _: ImageDrawProps<'a>) {}
    fn pop_clip(&mut self) {}
    fn pop_transparency_group(&mut self) {}
}

/// The text of every page of a PDF.
pub(super) fn page_texts(pdf: Vec<u8>) -> Vec<String> {
    let doc = hayro::hayro_syntax::Pdf::new(pdf).unwrap_or_else(|e| panic!("not a readable PDF: {e:?}"));
    let cache = InterpreterCache::new();
    doc.pages()
        .iter()
        .map(|page| {
            let (w, h) = page.render_dimensions();
            let mut ctx = Context::new(Affine::IDENTITY, Rect::new(0.0, 0.0, w as f64, h as f64), &cache, page.xref(), InterpreterSettings::default());
            let mut device = Collect::default();
            interpret_page(page, &mut ctx, &mut device);
            device.0
        })
        .collect()
}

/// The text of the PDF with every space and line break taken out, all pages in a row.
pub(super) fn squashed(pdf: Vec<u8>) -> String {
    page_texts(pdf).concat().chars().filter(|c| !c.is_whitespace()).collect()
}

/// How many bookmarks a PDF has: LuaLaTeX keeps its outline items in compressed object streams,
/// so every stream is inflated and its `/Title` entries are counted (the document's own title is
/// in the uncompressed information dictionary and is not counted).
pub(super) fn outline_entries(pdf: &[u8]) -> usize {
    let find = |from: usize, needle: &[u8]| pdf[from..].windows(needle.len()).position(|w| w == needle).map(|p| from + p);
    let (mut count, mut at) = (0, 0);
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
        // only the end of line before `endstream` goes: the compressed data may end in a byte that
        // looks like white space (a tab)
        let raw = &pdf[start + skipped..end];
        let data = raw.strip_suffix(b"\r\n").or_else(|| raw.strip_suffix(b"\n")).or_else(|| raw.strip_suffix(b"\r")).unwrap_or(raw);
        count += crate::pdf::write::inflate(data).windows(6).filter(|w| *w == b"/Title").count();
        at = end + 9;
    }
    count
}
