//! The format of the editor's text block by block (EDIT-109): the text is cut into blocks of whole
//! lines at blank lines where nothing is carried over (no open code fence, no front matter, no
//! formula over several lines), each block is formatted on its own, and the result is kept under
//! the block's text and everything else the block's format depends on. An edit of one block formats
//! that block again and puts the others together from what was kept; the result is the same as
//! that of formatting the whole text at once (a cache with one block does that).

use super::*;
use crate::markdown::Footnotes;
use std::collections::{HashMap, HashSet};

/// A block is at least this many bytes (it ends at the first fit place after that): a note of
/// thousands of short paragraphs is a few hundred blocks, not thousands.
const MIN_BLOCK: usize = 2_000;

/// What the format needs of a link of the note (the link list of the layouter, in the block's bytes).
#[derive(Clone, PartialEq)]
struct LinkMark {
    range: Range<usize>,
    markdown_style: bool,
    embed: bool,
    /// the note it points to exists
    exists: bool,
}

/// A range of the layouter's `images` in the block's bytes (cut to the block): a picture, or the
/// rows of a table or fold. `valid` says the whole range, as given, lies in the text on char boundaries.
#[derive(Clone)]
struct ImageMark {
    range: Range<usize>,
    h: f32,
    valid: bool,
}

impl PartialEq for ImageMark {
    fn eq(&self, other: &Self) -> bool {
        self.range == other.range && self.h.to_bits() == other.h.to_bits() && self.valid == other.valid
    }
}

/// Everything the format of one block depends on besides the fonts.
#[derive(Clone, PartialEq)]
struct Input {
    text: String,
    links: Vec<LinkMark>,
    images: Vec<ImageMark>,
    /// the raw lines of the preview in the block's lines (`None`: no preview), cut to the block
    raw: Option<Range<usize>>,
    /// the block has `[^`: footnote marks depend on the labels that are defined
    footnotes: bool,
    /// the labels defined in the note, for a block with `[^`
    defined: u64,
}

impl Input {
    fn key(&self) -> u64 {
        let mut h = DefaultHasher::new();
        crate::fold_ui::hash_of(&self.text).hash(&mut h);
        for l in &self.links {
            (l.range.start, l.range.end, l.markdown_style, l.embed, l.exists).hash(&mut h);
        }
        for i in &self.images {
            (i.range.start, i.range.end, i.h.to_bits(), i.valid).hash(&mut h);
        }
        (self.raw.as_ref().map(|r| (r.start, r.end)), self.footnotes, self.defined).hash(&mut h);
        h.finish()
    }
}

/// The format of one block: its sections (in the block's bytes), the formats of its first and last
/// characters (to put sections of equal format together across the cut) and its formulas.
struct Out {
    sections: Vec<egui::text::LayoutSection>,
    first: Option<Fmt>,
    last: Option<Fmt>,
    marks: Vec<MathMark>,
}

struct Entry {
    input: Input,
    out: Arc<Out>,
}

/// Where a block starts: its byte and its line.
struct Cut {
    byte: usize,
    line: usize,
}

/// The formatted blocks of the last text (EDIT-109), found again by the block's text and what else
/// its format depends on.
pub(crate) struct FormatCache {
    fonts: Option<EditorFonts>,
    blocks: HashMap<u64, Entry>,
    min_block: usize,
    /// how many blocks were formatted (not found in the cache), for the tests
    #[cfg(test)]
    pub formatted: usize,
}

impl Default for FormatCache {
    fn default() -> Self {
        FormatCache {
            fonts: None,
            blocks: HashMap::new(),
            min_block: MIN_BLOCK,
            #[cfg(test)]
            formatted: 0,
        }
    }
}

impl FormatCache {
    /// A cache whose blocks are at least `min` bytes (tests: small blocks on small notes, or one block).
    #[cfg(test)]
    pub fn with_min_block(min: usize) -> Self {
        FormatCache { min_block: min, ..Default::default() }
    }

    /// [`highlight_with_math`], formatting only the blocks that are not in the cache.
    pub fn highlight(&mut self, text: &str, links: &[(LinkSpan, bool)], fonts: &EditorFonts, images: &[(Range<usize>, f32)], raw_lines: Option<Range<usize>>) -> (LayoutJob, Vec<MathMark>) {
        if self.fonts.as_ref() != Some(fonts) {
            self.blocks.clear();
            self.fonts = Some(fonts.clone());
        }
        let cuts = cuts(text, links, self.min_block);
        let end_of = |n: usize| cuts.get(n + 1).map_or(text.len(), |c| c.byte);
        // the labels the note defines: a block with a footnote mark is formatted for them
        let with_notes: Vec<bool> = (0..cuts.len()).map(|n| text[cuts[n].byte..end_of(n)].contains("[^")).collect();
        let mut defined = HashSet::new();
        if with_notes.iter().any(|w| *w) {
            defined = markdown::defined_footnotes_in_scope();
            for (n, _) in with_notes.iter().enumerate().filter(|(_, w)| **w) {
                defined.extend(markdown::find_footnotes(&text[cuts[n].byte..end_of(n)]).defined());
            }
        }
        let defined_hash = defined.iter().fold(0u64, |acc, label| {
            let mut h = DefaultHasher::new();
            label.hash(&mut h);
            acc.wrapping_add(h.finish())
        });
        let total_lines = text.matches('\n').count() + 1;
        // the ranges that reach into a block, in the order they were given
        let mut link_order: Vec<usize> = (0..links.len()).collect();
        link_order.sort_by_key(|&i| links[i].0.range.start);
        let mut image_order: Vec<usize> = (0..images.len()).collect();
        image_order.sort_by_key(|&i| images[i].0.start);
        let (mut next_link, mut next_image) = (0, 0);
        let mut active: Vec<usize> = Vec::new();
        let valid = |r: &Range<usize>| r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end);

        let mut old = std::mem::take(&mut self.blocks);
        let mut blocks: HashMap<u64, Entry> = HashMap::with_capacity(cuts.len());
        let mut parts: Vec<(usize, usize, Arc<Out>)> = Vec::with_capacity(cuts.len());
        for (n, cut) in cuts.iter().enumerate() {
            let (a, b) = (cut.byte, end_of(n));
            let lines = cuts.get(n + 1).map_or(total_lines, |c| c.line) - cut.line;
            let mut mine = Vec::new();
            while next_link < link_order.len() && links[link_order[next_link]].0.range.start < b {
                mine.push(link_order[next_link]);
                next_link += 1;
            }
            mine.sort_unstable();
            let links_here = mine
                .into_iter()
                .map(|i| {
                    let (l, exists) = &links[i];
                    LinkMark { range: l.range.start.saturating_sub(a)..l.range.end.saturating_sub(a), markdown_style: l.markdown_style, embed: l.embed, exists: *exists }
                })
                .collect();
            while next_image < image_order.len() && images[image_order[next_image]].0.start < b {
                active.push(image_order[next_image]);
                next_image += 1;
            }
            active.retain(|&i| images[i].0.end > a);
            let mut mine = active.clone();
            mine.sort_unstable();
            let images_here = mine
                .into_iter()
                .map(|i| {
                    let (r, h) = &images[i];
                    ImageMark { range: r.start.max(a) - a..r.end.min(b) - a, h: *h, valid: valid(r) }
                })
                .collect();
            let raw = raw_lines.as_ref().map(|r| {
                // cut to the block's lines; a range that touches none of them is the same range for every block
                let (s, e) = (r.start.saturating_sub(cut.line).min(lines), r.end.saturating_sub(cut.line).min(lines));
                if s == e && (s == 0 || s == lines) {
                    0..0
                } else {
                    s..e
                }
            });
            let input = Input { text: text[a..b].to_string(), links: links_here, images: images_here, raw, footnotes: with_notes[n], defined: if with_notes[n] { defined_hash } else { 0 } };
            let key = input.key();
            let out = if let Some(e) = blocks.get(&key).filter(|e| e.input == input) {
                e.out.clone()
            } else if let Some(e) = old.remove(&key).filter(|e| e.input == input) {
                let out = e.out.clone();
                blocks.insert(key, e);
                out
            } else {
                #[cfg(test)]
                {
                    self.formatted += 1;
                }
                let out = Arc::new(format_chunk(&input.text, &input, &defined, fonts));
                blocks.insert(key, Entry { input, out: out.clone() });
                out
            };
            parts.push((a, cut.line, out));
        }
        self.blocks = blocks;
        join(text, &parts, fonts)
    }
}

/// The blocks of `text` by where they start: after a blank line, at least `min` bytes after the
/// block before, and nowhere that something is carried over: in front matter, in a code fence, in a
/// formula of several lines, in a link, or before a line that would start front matter.
fn cuts(text: &str, links: &[(LinkSpan, bool)], min: usize) -> Vec<Cut> {
    let mut cuts = vec![Cut { byte: 0, line: 0 }];
    if text.len() < min.saturating_mul(2).max(1) {
        return cuts;
    }
    // what a cut may not fall into
    let mut inside: Vec<Range<usize>> = links.iter().map(|(l, _)| l.range.clone()).collect();
    if text.contains("$$") {
        inside.extend(markdown::find_math(text).into_iter().filter(|m| m.lines.len() > 1).map(|m| m.range));
    }
    // an alignment wrapper is kept whole in a block (PDF-028): its two lines are formatted together
    if text.contains("<div") {
        let lines = markdown::lines_with_offsets(text);
        inside.extend(markdown::align_blocks(text).into_iter().map(|w| lines[w.open].0..lines[w.close].0 + lines[w.close].1.len()));
    }
    inside.sort_by_key(|r| r.start);
    let (mut next, mut reach) = (0, 0);
    let (mut in_front, mut in_code, mut after_blank) = (false, false, false);
    let mut offset = 0;
    let mut last = 0;
    for (li, line) in text.split('\n').enumerate() {
        let plain = line.strip_suffix('\r').unwrap_or(line);
        while next < inside.len() && inside[next].start < offset {
            reach = reach.max(inside[next].end);
            next += 1;
        }
        if li > 0 && after_blank && !in_front && !in_code && offset < text.len() && offset - last >= min && plain.trim() != "---" && reach <= offset {
            cuts.push(Cut { byte: offset, line: li });
            last = offset;
        }
        let fence = plain.trim_start().starts_with("```") || plain.trim_start().starts_with("~~~");
        if li == 0 && plain.trim() == "---" {
            in_front = true;
        } else if in_front {
            if fence {
                // the fences of the front matter are counted by one rule and skipped by another: no cuts at all
                return vec![Cut { byte: 0, line: 0 }];
            }
            in_front = plain.trim() != "---";
        } else if fence {
            in_code = !in_code;
        }
        after_blank = plain.trim().is_empty();
        offset += line.len() + 1;
    }
    cuts
}

/// The job of the text from its blocks: their sections in the bytes of the text, sections of equal
/// format across a cut put together, as they are in a text formatted at once.
fn join(text: &str, parts: &[(usize, usize, Arc<Out>)], fonts: &EditorFonts) -> (LayoutJob, Vec<MathMark>) {
    let mut job = LayoutJob { text: text.to_string(), ..Default::default() };
    job.sections.reserve(parts.iter().map(|p| p.2.sections.len()).sum());
    let mut marks = Vec::new();
    let mut last: Option<Fmt> = None;
    for (at, line, out) in parts {
        let mut sections = out.sections.iter();
        // the section of the same format that goes on over the cut
        if let (Some(a), Some(b)) = (last, out.first) {
            if a == b && a.math == 0 && !a.math_rest {
                if let (Some(into), Some(first)) = (job.sections.last_mut(), sections.next()) {
                    into.byte_range.end = egui::text::ByteIndex(at + first.byte_range.end.0);
                }
            }
        }
        for s in sections {
            job.sections.push(egui::text::LayoutSection { leading_space: s.leading_space, byte_range: egui::text::ByteIndex(at + s.byte_range.start.0)..egui::text::ByteIndex(at + s.byte_range.end.0), format: s.format.clone() });
        }
        marks.extend(out.marks.iter().map(|m| MathMark { range: m.range.start + at..m.range.end + at, lines: m.lines.start + line..m.lines.end + line, ..m.clone() }));
        last = out.last;
    }
    if job.sections.is_empty() {
        job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len()), format: to_format(Fmt::default(), fonts) });
    }
    (job, marks)
}

/// The format of one block (EDIT-109): what [`highlight_with_math`] makes of a whole text, for the text of a block that starts where nothing is carried over.
fn format_chunk(text: &str, input: &Input, defined: &HashSet<String>, fonts: &EditorFonts) -> Out {
    let raw_lines = input.raw.clone();
    let mut fmts = vec![Fmt::default(); text.len()];
    let fenced = markdown::fenced_code_lines(text);
    for block in syntax::fenced_blocks(text) {
        let body = &text[block.body.clone()];
        for (r, kind) in syntax::highlight(&block.lang, body).iter() {
            set(&mut fmts, block.body.start + r.start..block.body.start + r.end, |f| f.syn_kind = Some(*kind));
        }
    }
    let mut code_ranges: Vec<Range<usize>> = Vec::new();
    let notes = if input.footnotes { markdown::find_footnotes(text) } else { Footnotes::default() };
    let mut in_front = false;
    let wrappers = markdown::align_blocks(text);
    for (li, (off, line)) in markdown::lines_with_offsets(text).into_iter().enumerate() {
        let lr = off..off + line.len();
        // YAML front matter at the top
        if li == 0 && line.trim() == "---" {
            in_front = true;
            set(&mut fmts, lr, |f| f.dim = true);
            continue;
        }
        if in_front {
            set(&mut fmts, lr.clone(), |f| {
                f.code = true;
                f.dim = true
            });
            if line.trim() == "---" {
                in_front = false;
            }
            continue;
        }
        if fenced.get(li).copied().unwrap_or(false) {
            set(&mut fmts, lr, |f| f.fence = true);
            continue;
        }
        if let Some(level) = markdown::heading_level(line) {
            set(&mut fmts, lr.clone(), |f| f.heading = level as u8);
            let gap = usize::from(line.as_bytes().get(level) == Some(&b' '));
            set(&mut fmts, off..off + level + gap, |f| {
                f.dim = true;
                f.syn = true
            });
        }
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        if trimmed.starts_with('>') {
            set(&mut fmts, lr.clone(), |f| f.quote = true);
            let gap = usize::from(trimmed.as_bytes().get(1) == Some(&b' '));
            set(&mut fmts, off + indent..off + indent + 1 + gap, |f| {
                f.dim = true;
                f.syn = true
            });
        }
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            set(&mut fmts, lr.clone(), |f| f.dim = true);
        }
        // a page-break marker is markup all through: the preview hides it for the dashed rule (EDIT-065)
        if markdown::page_break_line(line) {
            set(&mut fmts, lr.clone(), |f| {
                f.dim = true;
                f.syn = true
            });
        }
        // the lines of an alignment wrapper are markup (PDF-028): dimmed like a rule
        if markdown::is_align_edge(&wrappers, li) {
            set(&mut fmts, lr.clone(), |f| f.dim = true);
        }
        if let Some((check, done, _, _)) = markdown::parse_task_line(line) {
            set(&mut fmts, off + indent..off + check + 2, |f| f.marker = true);
            if done {
                set(&mut fmts, off + check + 2..lr.end, |f| f.strike = true);
            }
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            set(&mut fmts, off + indent..off + indent + 1, |f| f.marker = true);
        } else {
            let digits = trimmed.bytes().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && trimmed[digits..].starts_with(". ") {
                set(&mut fmts, off + indent..off + indent + digits + 1, |f| f.marker = true);
            }
        }
        // inline code
        let lb = line.as_bytes();
        let mut i = 0;
        let mut local_code = Vec::new();
        while i < lb.len() {
            if lb[i] == b'`' {
                if let Some(e) = line[i + 1..].find('`') {
                    let r = off + i..off + i + e + 2;
                    set(&mut fmts, r.clone(), |f| f.code = true);
                    set(&mut fmts, r.start..r.start + 1, |f| f.syn = true);
                    set(&mut fmts, r.end - 1..r.end, |f| f.syn = true);
                    let inner = off + i + 1;
                    for (sr, kind) in syntax::highlight_inline(&line[i + 1..i + 1 + e]) {
                        set(&mut fmts, inner + sr.start..inner + sr.end, |f| f.syn_kind = Some(kind));
                    }
                    local_code.push(r);
                    i += e + 2;
                    continue;
                }
            }
            i += 1;
        }
        inline_pairs(line, off, &mut fmts, &local_code);
        colour_pairs(line, off, &mut fmts);
        footnote_pairs(line, off, &mut fmts, defined, notes.is_continuation(li));
        code_ranges.extend(local_code);
    }
    for l in &input.links {
        set(&mut fmts, l.range.clone(), |f| {
            f.dim = true;
            f.syn = true;
            f.italic = false;
        });
        let kind = if l.exists { 1 } else { 2 };
        // colour the shown part (target, or alias) strongly
        let shown = if l.markdown_style {
            // [label](target): colour the label
            let start = l.range.start + if l.embed { 2 } else { 1 };
            let label_end = text[start..l.range.end].find("](").map(|p| start + p).unwrap_or(l.range.end);
            start..label_end
        } else {
            let inner_start = l.range.start + if l.embed { 3 } else { 2 };
            let inner_end = l.range.end - 2;
            let inner = &text[inner_start..inner_end];
            match inner.find('|') {
                Some(p) => inner_start + p + 1..inner_end,
                None => inner_start..inner_end,
            }
        };
        set(&mut fmts, l.range.clone(), |f| f.link = kind);
        set(&mut fmts, shown, |f| {
            f.dim = false;
            f.syn = false
        });
    }
    for w in crate::lists::find_web_links(text) {
        if code_ranges.iter().any(|c| c.contains(&w.range.start)) {
            continue;
        }
        set(&mut fmts, w.range.clone(), |f| {
            f.dim = true;
            f.syn = true;
            f.link = 3;
        });
        set(&mut fmts, w.label_range.clone(), |f| {
            f.dim = false;
            f.syn = false
        });
    }
    for (r, _) in markdown::find_tags(text) {
        if !code_ranges.iter().any(|c| c.contains(&r.start)) {
            set(&mut fmts, r, |f| f.tag = true);
        }
    }

    // the preview: hide the markup of the lines that are not raw; the flag goes either way
    for (li, (off, line)) in markdown::lines_with_offsets(text).into_iter().enumerate() {
        let raw = raw_lines.as_ref().is_none_or(|r| r.contains(&li));
        // the line break belongs to the line, so that it does not make the row taller
        let end = (off + line.len() + 1).min(text.len());
        set(&mut fmts, off..end, |f| {
            f.hidden = f.syn && !raw;
            f.rendered = !raw;
            f.syn = false
        });
    }
    // the code blocks the preview draws as boxes (EDIT-080): the lines have no background of their own;
    // the fences are hidden, the closing one with its row, unless the cursor is in the block
    if let Some(rl) = raw_lines.as_ref() {
        let lines = markdown::lines_with_offsets(text);
        let plain: Vec<&str> = lines.iter().map(|(_, l)| *l).collect();
        for fence in syntax::fences(&plain) {
            let raw = fence.open < rl.end && rl.start < fence.end;
            for (n, &(off, line)) in lines.iter().enumerate().take(fence.end).skip(fence.open) {
                let end = (off + line.len() + 1).min(text.len());
                if n == fence.open && !raw {
                    set(&mut fmts, off..end, |f| {
                        f.boxed = true;
                        f.hidden = true;
                        f.rendered = true
                    });
                } else if n + 1 == fence.end && fence.closed && !raw {
                    set(&mut fmts, off..end, |f| {
                        f.boxed = true;
                        f.folded = true
                    });
                } else {
                    set(&mut fmts, off..end, |f| f.boxed = true);
                }
            }
        }
    }
    for ImageMark { range: r, h, valid } in &input.images {
        if *valid {
            if *h < 0.0 {
                // a negative height is text that is hidden and keeps its row: a summary is painted over it (EDIT-081)
                set(&mut fmts, r.clone(), |f| {
                    f.hidden = true;
                    f.rendered = true
                });
            // a height of 0 is text that a fold hides (EDIT-071)
            } else if *h == 0.0 {
                set(&mut fmts, r.clone(), |f| f.folded = true);
            } else {
                set(&mut fmts, r.clone(), |f| f.image_h = h.round().clamp(1.0, 30000.0) as u16);
            }
        }
    }
    // the formulas of the lines the preview draws: typeset, their TeX hidden with room for them in
    // the first row; one that cannot be typeset is shown as code (EDIT-047, EDIT-049)
    let plain: Vec<(Range<usize>, f32)> = input.images.iter().map(|i| (i.range.clone(), i.h)).collect();
    let marks = math_marks(text, fonts.rendered.size, &plain, raw_lines.as_ref());
    let line_starts = markdown::lines_with_offsets(text);
    for (n, m) in marks.iter().enumerate() {
        if let (Ok(f), false) = (&m.formula, m.block) {
            let extra = crate::math::paint::room_for(f, m.em).0;
            let (from, to) = (line_starts[m.lines.start].0, line_starts.get(m.lines.end).map_or(text.len(), |(o, _)| *o));
            set(&mut fmts, from..to, |x| x.math_extra = x.math_extra.max((extra * 4.0).ceil() as u16));
        }
        if m.formula.is_ok() {
            let first_end = if m.block { line_starts.get(m.lines.start + 1).map_or(text.len(), |(o, _)| *o) } else { m.range.end };
            set(&mut fmts, m.range.start..first_end.min(m.range.end.max(first_end)), |f| *f = Fmt { math: n as u32 + 1, ..Fmt::default() });
            if m.block {
                let rest_end = line_starts.get(m.lines.end).map_or(text.len(), |(o, _)| *o);
                set(&mut fmts, first_end..rest_end.max(first_end), |f| *f = Fmt { math_rest: true, ..Fmt::default() });
            }
        } else {
            set(&mut fmts, m.range.clone(), |f| *f = Fmt { code: true, rendered: true, heading: f.heading, quote: f.quote, ..Fmt::default() });
        }
    }
    let first = fmts.first().copied();
    let last = text.char_indices().next_back().and_then(|(i, _)| fmts.get(i).copied());
    let mut sections = Vec::new();
    let mut start = 0;
    let mut cur: Option<&Fmt> = None;
    // the formats are compared where a char starts, by reference: the table has a record for every byte of the note
    for i in (0..=text.len()).filter(|&i| i == text.len() || text.is_char_boundary(i)) {
        let f = fmts.get(i);
        if f != cur || i == text.len() {
            if let Some(&c) = cur {
                if i > start {
                    let (leading_space, format) = match marks.get((c.math as usize).wrapping_sub(1)).map(|m| (m, &m.formula)) {
                        Some((m, Ok(f))) if m.block => (0.0, crate::math::paint::block_format(f, m.em)),
                        Some((m, Ok(f))) => {
                            // the first character is a section of its own; the room is before the rest
                            let at = start + crate::math::paint::room_starts(&text[start..i]);
                            sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(at), format: crate::math::paint::hidden_format(f, m.em) });
                            start = at;
                            (f.layout.width, crate::math::paint::hidden_format(f, m.em))
                        }
                        _ if c.math_rest => (0.0, TextFormat { font_id: FontId::proportional(crate::math::paint::HIDDEN_SIZE), color: Color32::TRANSPARENT, line_height: Some(1.0), ..Default::default() }),
                        _ => (0.0, to_format(c, fonts)),
                    };
                    sections.push(egui::text::LayoutSection { leading_space, byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(i), format });
                }
            }
            start = i;
            cur = f;
        }
    }
    Out { sections, first, last, marks }
}
#[cfg(test)]
mod tests {
    use super::*;

    /// A note with what the blocks must carry over a cut: front matter, fences with blank lines in them, formulas
    /// over several lines, footnotes, links, quotes, lists, a rule, a table, tags and web links.
    const NOTE: &str = "---\ntitle: One\ntags: [a, b]\n---\n\n# Heading one\n\nA paragraph with **bold**, *italic*, `code` and a [[Link]] to [[Missing|alias]] and a #tag.\n\n- an item\n- [ ] a task\n    - a child\n\n> a quote\n> goes on\n\n```rust\nfn main() {\n\n    let x = 1;\n}\n```\n\nText with a footnote[^1] and a web link https://example.com/a and [label](https://example.org).\n\n[^1]: The note,\n    goes on here.\n\n$$\na^2 + b^2\n\n= c^2\n$$\n\nInline $x^2$ math and a ==mark== and ~~strike~~.\n\n---\n\n```math\n\\int_0^1 x\\,dx\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n## Heading two\n\nlast paragraph with \u{e9}\u{fc} and a <mark style=\"background: #ff0\">colour</mark>.\n\n```js\nlet unclosed = 1;\n\nstill code\n";

    fn fonts() -> EditorFonts {
        EditorFonts::uniform(15.0)
    }

    /// What every call of the tests gives besides the text: the links of the text (those that start at an even byte
    /// exist), a picture, a hidden range, a folded range, a hidden-but-kept range and one beyond the text.
    fn inputs(text: &str) -> (Vec<(LinkSpan, bool)>, Images) {
        let links = markdown::find_links(text).into_iter().map(|l| (l.range.start % 2 == 0, l)).map(|(e, l)| (l, e)).collect();
        let n = text.len();
        let at = |p: usize| (0..=n).find(|i| *i >= p && text.is_char_boundary(*i)).unwrap_or(n);
        let images = vec![(at(n / 5)..at(n / 5 + 9), 40.0), (at(n / 3)..at(n / 3 + 300), 0.0), (at(n / 2)..at(n / 2 + 30), -1.0), (at(n * 3 / 4)..at(n * 3 / 4 + 5), 17.5), (n + 5..n + 9, 0.0)];
        (links, images)
    }

    type Images = Vec<(Range<usize>, f32)>;
    type Marks = Vec<(Range<usize>, bool, Range<usize>, u32, Color32, Option<String>)>;

    /// The formulas of a job, for a comparison (a typeset formula has a number of its own, so it is its outcome).
    fn marks_of(marks: &[MathMark]) -> Marks {
        marks.iter().map(|m| (m.range.clone(), m.block, m.lines.clone(), m.em.to_bits(), m.color, m.formula.as_ref().err().cloned())).collect()
    }

    /// `text` formatted by `cache` and as a whole text (one block), which must be the same.
    fn check(cache: &mut FormatCache, text: &str, raw: Option<Range<usize>>) {
        let (links, images) = inputs(text);
        let (job, marks) = cache.highlight(text, &links, &fonts(), &images, raw.clone());
        let (whole, whole_marks) = FormatCache::with_min_block(usize::MAX).highlight(text, &links, &fonts(), &images, raw.clone());
        assert_eq!(job.text, whole.text);
        assert_eq!(job.sections.len(), whole.sections.len(), "{raw:?} {text:?}");
        for (a, b) in job.sections.iter().zip(&whole.sections) {
            assert_eq!(a, b, "{raw:?} {text:?}");
        }
        assert_eq!(marks_of(&marks), marks_of(&whole_marks), "{raw:?} {text:?}");
    }

    /// The notes of the tests: [`NOTE`] and what an edit of it makes, each by what it changes.
    fn variants() -> Vec<String> {
        let mut out = vec![NOTE.to_string(), String::new(), "x".to_string(), "\n\n\n".to_string(), NOTE.replace('\n', "\r\n"), format!("{NOTE}\n\n"), NOTE.repeat(3)];
        // a character typed or deleted in a paragraph, a heading, a code block, a formula, bold text and a link
        for (from, to) in [("paragraph with", "paragraph W with"), ("# Heading one", "# Heading on"), ("let x = 1", "let x = 12"), ("a^2", "a^3"), ("bold", "bol"), ("[[Link]]", "[[Lin]]")] {
            out.push(NOTE.replacen(from, to, 1));
        }
        // a blank line taken away or put in: two blocks become one, one becomes two
        out.push(NOTE.replacen("one\n\n", "one\n", 1));
        out.push(NOTE.replacen("- an item\n", "- an item\n\n", 1));
        out.push(NOTE.replacen("# Heading one\n\n", "# Heading one\n", 1));
        // a fence opened or closed above: the fence state of every later block changes
        out.push(NOTE.replacen("# Heading one\n", "# Heading one\n```\n", 1));
        out.push(NOTE.replacen("fn main() {\n", "fn main() {\n```\n", 1));
        out.push(NOTE.replacen("```rust\n", "", 1));
        out.push(NOTE.replacen("}\n```\n", "}\n", 1));
        out.push(NOTE.replacen("\n```js\n", "\n~~~js\n", 1));
        // formulas: an opening `$$` lost, one added, a closing one lost
        out.push(NOTE.replacen("$$\na^2", "a^2", 1));
        out.push(NOTE.replacen("= c^2\n$$", "= c^2", 1));
        out.push(NOTE.replacen("# Heading one\n", "# Heading one\n$$\n", 1));
        // front matter closed, opened, with a fence in it, or a rule that looks like it
        out.push(NOTE.replacen("tags: [a, b]\n---\n", "tags: [a, b]\n", 1));
        out.push(NOTE.replacen("tags: [a, b]\n", "tags: [a, b]\n```\n", 1));
        out.push(NOTE.replacen("---\ntitle", "title", 1));
        out.push(format!("intro\n\n{NOTE}"));
        // an alignment wrapper over several blocks of a note, one with its closing line lost, one in a fence
        out.push(NOTE.replacen("# Heading one\n", "<div align=\"center\">\n\n# Heading one\n", 1) + "\n</div>\n");
        out.push(NOTE.replacen("# Heading one\n", "<div align=\"right\">\n\n# Heading one\n", 1));
        out.push(NOTE.replacen("# Heading one\n", "```\n<div align=\"center\">\n</div>\n```\n# Heading one\n", 1));
        // footnotes: the definition changed or lost, a reference in another block
        out.push(NOTE.replacen("[^1]: The note,", "[^2]: The note,", 1));
        out.push(NOTE.replacen("Heading one\n\nA paragraph", "Heading one\n\nA paragraph[^1]", 1));
        out.push(NOTE.replacen("[^1]: The note,\n    goes on here.\n", "", 1));
        // a tab, an indented fence, a text without its last newline
        out.push(NOTE.replacen("- an item", "\t- an item", 1));
        out.push(NOTE.replacen("```rust", "  ```rust", 1));
        out.push(NOTE.trim_end().to_string());
        out
    }

    // spec: EDIT-109
    #[test]
    fn edit_109_the_blocks_put_together_are_the_format_of_the_whole_text() {
        let raws = [None, Some(0..0), Some(0..1), Some(7..8), Some(12..14), Some(20..60), Some(3..3), Some(200..201)];
        // blocks of every size, from one line to the whole note; the cache is kept through all the texts
        for min in [0, 1, 40, 150, 600, usize::MAX] {
            let mut cache = FormatCache::with_min_block(min);
            for text in variants() {
                for raw in &raws {
                    check(&mut cache, &text, raw.clone());
                }
            }
            // and the other way round, through the same cache
            for text in variants().into_iter().rev() {
                check(&mut cache, &text, raws[3].clone());
            }
        }
    }

    // spec: EDIT-109
    #[test]
    fn edit_109_a_cut_is_never_made_where_something_is_carried_over() {
        // every cut of every text is at the start of a line after a blank one, outside fences, front matter, formulas and links
        for text in variants() {
            let (links, _) = inputs(&text);
            let found = cuts(&text, &links, 0);
            let fenced = markdown::fenced_code_lines(&text);
            let lines = markdown::lines_with_offsets(&text);
            let math = markdown::find_math(&text);
            let front = markdown::front_matter_lines(&text);
            for c in found.iter().skip(1) {
                assert_eq!(lines[c.line].0, c.byte);
                assert!(lines[c.line - 1].1.trim().is_empty() && !fenced[c.line - 1] && c.line >= front, "{:?}", &text[..c.byte]);
                assert!(!math.iter().any(|m| m.lines.start < c.line && c.line < m.lines.end), "a cut in a formula");
                assert!(!links.iter().any(|(l, _)| l.range.start < c.byte && c.byte < l.range.end), "a cut in a link");
                assert_ne!(lines[c.line].1.trim(), "---");
            }
        }
        // and the note really is cut into blocks when they are small
        assert!(cuts(NOTE, &[], 0).len() > 8);
    }

    // spec: PDF-028, EDIT-109
    #[test]
    fn a_wrapper_longer_than_a_block_is_kept_whole_and_both_its_lines_are_dimmed() {
        let text = format!("intro\n\n<div align=\"center\">\n\n{}</div>\n\nafter\n", "paragraph in the wrapper\n\n".repeat(10));
        let (links, _) = inputs(&text);
        let lines = markdown::lines_with_offsets(&text);
        let close = lines.iter().position(|l| l.1 == "</div>").unwrap();
        // with blocks of one line, no cut falls between the two lines of the wrapper; the ones around are cut
        let found = cuts(&text, &links, 0);
        assert!(found.iter().all(|c| !(2 < c.line && c.line <= close)), "a cut inside the wrapper");
        assert!(found.len() > 1);
        // and a cache of the smallest blocks formats like the whole text: both lines dimmed, the text between not
        let mut cache = FormatCache::with_min_block(0);
        let (job, _) = cache.highlight(&text, &links, &fonts(), &[], Some(0..0));
        let dim = |at: usize| job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format.color;
        assert_eq!(dim(lines[2].0), crate::theme::palette().faint, "the opening line");
        assert_eq!(dim(lines[close].0), crate::theme::palette().faint, "the closing line");
        assert_ne!(dim(lines[4].0), crate::theme::palette().faint);
        check(&mut cache, &text, Some(0..0));
    }

    // spec: EDIT-109
    #[test]
    fn edit_109_an_edit_formats_the_block_it_is_in_and_a_cursor_move_the_blocks_it_leaves_and_enters() {
        let note: String = (0..300).map(|n| format!("Paragraph {n} with **bold** text and a [[link {n}]] in it.\n- an item\n\n")).collect();
        let mut cache = FormatCache::with_min_block(400);
        let all = |c: &mut FormatCache, t: &str, raw: Option<Range<usize>>| {
            let before = c.formatted;
            let links: Vec<_> = markdown::find_links(t).into_iter().map(|l| (l, true)).collect();
            c.highlight(t, &links, &fonts(), &[], raw);
            c.formatted - before
        };
        let first = all(&mut cache, &note, Some(10..11));
        assert!(first > 20, "{first} blocks");
        assert_eq!(all(&mut cache, &note, Some(10..11)), 0, "the same text and cursor formats nothing");
        // a typed character: the block it is in, and no more (the blocks after it still end where they did)
        let typed = note.replacen("Paragraph 150 with", "Paragraph 150x with", 1);
        let n = all(&mut cache, &typed, Some(10..11));
        assert!((1..=3).contains(&n), "{n} blocks for one typed character of {first}");
        // the cursor moves to another line: the block it was in and the one it enters
        let n = all(&mut cache, &typed, Some(500..501));
        assert!((1..=3).contains(&n), "{n} blocks for a cursor move of {first}");
        // and back
        assert_eq!(all(&mut cache, &typed, Some(10..11)), n);
    }

    // spec: EDIT-109
    #[test]
    fn edit_109_other_fonts_and_other_defined_footnotes_format_the_blocks_again() {
        let note: String = (0..40).map(|n| format!("Paragraph {n} with a note[^a] in it.\n\n")).collect();
        let mut cache = FormatCache::with_min_block(100);
        let run = |c: &mut FormatCache, t: &str, f: &EditorFonts| {
            let before = c.formatted;
            c.highlight(t, &[], f, &[], Some(0..0));
            c.formatted - before
        };
        let n = run(&mut cache, &note, &fonts());
        assert!(n > 5);
        assert_eq!(run(&mut cache, &note, &fonts()), 0);
        assert_eq!(run(&mut cache, &note, &EditorFonts::uniform(20.0)), n, "other fonts");
        // the labels the note defines are part of what a block with a mark depends on
        let defined = format!("{note}[^a]: here\n");
        for text in [&defined, &note] {
            let (job, _) = cache.highlight(text, &[], &EditorFonts::uniform(20.0), &[], Some(0..0));
            let (whole, _) = FormatCache::with_min_block(usize::MAX).highlight(text, &[], &EditorFonts::uniform(20.0), &[], Some(0..0));
            assert!(job.sections == whole.sections);
        }
    }
}
