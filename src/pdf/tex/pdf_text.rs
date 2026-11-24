//! The text of a PDF made by LuaLaTeX, for the tests of the writer (TEX-008): every glyph a page draws, by
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

/// The direct `/Length` of the stream dictionary that ends right before the `stream` keyword at
/// `at`, `None` for an indirect length (`5 0 R`), a missing one or a dictionary that is not there.
fn direct_length(pdf: &[u8], at: usize) -> Option<usize> {
    let head = pdf[..at].trim_ascii_end();
    let dict = &head[head.windows(3).rposition(|w| w == b"obj")? + 3..];
    if !dict.ends_with(b">>") {
        return None;
    }
    let after = &dict[dict.windows(8).position(|w| w == b"/Length ")? + 8..];
    let after = after.trim_ascii_start();
    let digits = after.iter().take_while(|b| b.is_ascii_digit()).count();
    let mut rest = after[digits..].trim_ascii_start().split(|b| b.is_ascii_whitespace());
    // `5 0 R` is a reference, the number in it is not the length
    let indirect = rest.next().is_some_and(|g| !g.is_empty() && g.iter().all(u8::is_ascii_digit)) && rest.next() == Some(b"R");
    if indirect {
        return None;
    }
    std::str::from_utf8(&after[..digits]).ok()?.parse().ok()
}

/// The streams of a PDF, inflated: LuaLaTeX keeps the pages' contents, its outline items and its
/// annotations in compressed streams. A stream ends where its dictionary's direct `/Length` says,
/// since the data may end in a `\r` that looks like the end of line before `endstream`; without a
/// direct length (a reference, or one that does not fit the file) the end of line before
/// `endstream` is dropped.
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
        let data_start = start + skipped;
        if let Some(len) = direct_length(pdf, p).filter(|len| data_start + len <= pdf.len()) {
            out.push(crate::pdf::write::inflate(&pdf[data_start..data_start + len]));
            at = data_start + len;
            continue;
        }
        let Some(end) = find(data_start, b"endstream") else { break };
        // only the end of line before `endstream` goes: the compressed data may end in a byte that
        // looks like white space (a tab)
        let raw = &pdf[data_start..end];
        let data = raw.strip_suffix(b"\r\n").or_else(|| raw.strip_suffix(b"\n")).or_else(|| raw.strip_suffix(b"\r")).unwrap_or(raw);
        out.push(crate::pdf::write::inflate(data));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A short text whose deflated form ends in `\r`, found by trying numbers.
    fn text_deflating_to_cr() -> (String, Vec<u8>) {
        (0..100_000)
            .map(|n| format!("{n} 0.5 0.5 0.5 rg"))
            .map(|t| {
                let z = miniz_oxide::deflate::compress_to_vec_zlib(t.as_bytes(), 6);
                (t, z)
            })
            .find(|(_, z)| z.last() == Some(&b'\r'))
            .expect("some text deflates to data ending in CR")
    }

    fn pdf_with(length: &str, data: &[u8]) -> Vec<u8> {
        let mut pdf = format!("%PDF-1.5\n4 0 obj\n<< /Length {length} /Filter /FlateDecode >>\nstream\n").into_bytes();
        pdf.extend(data);
        pdf.extend(b"\nendstream\nendobj\n");
        pdf
    }

    // spec: TEX-030
    #[test]
    fn a_stream_whose_data_ends_in_cr_is_cut_by_its_length() {
        let (text, z) = text_deflating_to_cr();
        assert_eq!(z.last(), Some(&b'\r'));
        let pdf = pdf_with(&z.len().to_string(), &z);
        assert_eq!(streams(&pdf), vec![text.into_bytes()]);
    }

    // spec: TEX-030
    #[test]
    fn a_stream_with_an_indirect_length_ends_at_the_end_of_line() {
        let z = miniz_oxide::deflate::compress_to_vec_zlib(b"1 0 0 rg", 6);
        let pdf = pdf_with("5 0 R", &z);
        assert_eq!(direct_length(&pdf, pdf.windows(7).position(|w| w == b"\nstream").unwrap() + 1), None);
        assert_eq!(streams(&pdf), vec![b"1 0 0 rg".to_vec()]);
    }
}
