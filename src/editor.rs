//! The markdown editor: a text box that colours markdown as you type, shows links in
//! blue (note exists) or red (note missing), follows links on click and suggests note
//! names after `[[`.

use crate::colour_tags;
use crate::fonts::EditorFonts;
#[cfg(test)]
use crate::link_assist::link_query;
use crate::markdown::{self, LinkSpan};
use crate::syntax;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Galley, Key, Modifiers, Pos2, Rect, Shape, Stroke, TextFormat, Ui, Vec2};
use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Default, Hash)]
struct Fmt {
    heading: u8,
    bold: bool,
    italic: bool,
    code: bool,
    fence: bool,
    /// a line of a code block that the preview draws as a box (EDIT-080): no background of its own
    boxed: bool,
    /// 0 none, 1 existing link, 2 missing link, 3 web link
    link: u8,
    dim: bool,
    tag: bool,
    strike: bool,
    quote: bool,
    marker: bool,
    highlight: bool,
    /// markup (`#`, `**`, `[[`, …) that the markdown preview hides; cleared again in the layout
    syn: bool,
    /// markup hidden by the markdown preview (EDIT-016)
    hidden: bool,
    /// > 0: this text is an image link shown as a picture; the row is this many points tall
    image_h: u16,
    /// the text of a line that a fold hides: rows of no height (EDIT-071)
    folded: bool,
    /// the background of `<mark style="background: …">` text (EDIT-028)
    bg: Option<[u8; 4]>,
    /// the colour of `<span style="color: …">` text
    fg: Option<[u8; 4]>,
    /// a `<mark …>` or `<span …>` tag, long markup that the preview folds up to almost nothing
    ctag: bool,
    /// the text of a line the preview draws rendered: it has the rendered font (CFG-032)
    rendered: bool,
    /// highlighted code: what this piece of a code block or code span is (EDIT-032)
    syn_kind: Option<crate::syntax::Kind>,
    /// text inside `<u>…</u>` (EDIT-051)
    under: bool,
    /// text inside `<sub>` (1) or `<sup>` (2): smaller and lowered or raised (EDIT-051)
    script: u8,
    /// footnotes (EDIT-050): 1 the label of a reference, 2 the label of a definition (both raised
    /// once the line is drawn rendered), 3 the text of a definition (a little smaller)
    fnote: u8,
    /// the TeX of a formula that the preview draws typeset: 1 + its number among the marks; the
    /// text is hidden and the section has room for the formula (EDIT-048)
    math: u32,
    /// the second and later lines of a display formula that takes several lines: one point tall
    math_rest: bool,
    /// the text of a line with a formula that reaches below it has line boxes this much taller, in
    /// quarter points (EDIT-048)
    math_extra: u16,
}

pub fn heading_scale(level: u8) -> f32 {
    match level {
        1 => 1.75,
        2 => 1.5,
        3 => 1.3,
        4 => 1.15,
        5 | 6 => 1.05,
        _ => 1.0,
    }
}

fn set(fmts: &mut [Fmt], r: Range<usize>, f: impl Fn(&mut Fmt)) {
    let end = r.end.min(fmts.len());
    for x in &mut fmts[r.start.min(end)..end] {
        f(x);
    }
}

/// Toggle-style inline markers inside one line (`**bold**`, `*it*`, `~~s~~`, `==h==`).
fn inline_pairs(line: &str, off: usize, fmts: &mut [Fmt], code: &[Range<usize>]) {
    let b = line.as_bytes();
    let in_code = |i: usize| code.iter().any(|r| r.contains(&(off + i)));
    for (marker, kind) in [("**", 0u8), ("__", 0), ("~~", 1), ("==", 2)] {
        let mut i = 0;
        while let Some(p) = line[i..].find(marker) {
            let s = i + p;
            if in_code(s) {
                i = s + 2;
                continue;
            }
            if let Some(q) = line[s + 2..].find(marker) {
                let e = s + 2 + q;
                if q > 0 && !in_code(e) {
                    set(fmts, off + s..off + s + 2, |f| {
                        f.dim = true;
                        f.syn = true
                    });
                    set(fmts, off + e..off + e + 2, |f| {
                        f.dim = true;
                        f.syn = true
                    });
                    set(fmts, off + s + 2..off + e, |f| match kind {
                        0 => f.bold = true,
                        1 => f.strike = true,
                        _ => f.highlight = true,
                    });
                    i = e + 2;
                    continue;
                }
            }
            break;
        }
    }
    // single * or _ italics (not part of ** and not list markers)
    for m in *b"*_" {
        let mut i = 0;
        while i < b.len() {
            let is_single = |j: usize| b[j] == m && (j == 0 || b[j - 1] != m) && (j + 1 >= b.len() || b[j + 1] != m);
            if is_single(i) && !in_code(i) && i + 1 < b.len() && b[i + 1] != b' ' {
                if m == b'_' && i > 0 && b[i - 1].is_ascii_alphanumeric() {
                    i += 1;
                    continue;
                }
                let mut j = i + 1;
                let mut found = None;
                while j < b.len() {
                    if is_single(j) && b[j - 1] != b' ' {
                        found = Some(j);
                        break;
                    }
                    j += 1;
                }
                if let Some(j) = found {
                    set(fmts, off + i..off + i + 1, |f| {
                        f.dim = true;
                        f.syn = true
                    });
                    set(fmts, off + j..off + j + 1, |f| {
                        f.dim = true;
                        f.syn = true
                    });
                    set(fmts, off + i + 1..off + j, |f| f.italic = true);
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
}

/// The tags of coloured highlights and font colours (EDIT-028): the tags are dimmed markup, the text
/// between them gets the colour. Outer runs come first so that inner ones win.
fn colour_pairs(line: &str, off: usize, fmts: &mut [Fmt]) {
    let mut runs = colour_tags::find(line);
    runs.sort_by_key(|r| std::cmp::Reverse(r.inner().len()));
    for r in runs {
        for t in [r.open.clone(), r.close.clone()] {
            set(fmts, off + t.start..off + t.end, |f| {
                f.dim = true;
                f.syn = true;
                f.ctag = true
            });
        }
        let inner = off + r.inner().start..off + r.inner().end;
        match r.kind {
            colour_tags::Kind::Mark => set(fmts, inner, |f| f.bg = Some(r.rgba)),
            colour_tags::Kind::Font => set(fmts, inner, |f| f.fg = Some(r.rgba)),
            colour_tags::Kind::Underline => set(fmts, inner, |f| f.under = true),
            colour_tags::Kind::Sub => set(fmts, inner, |f| f.script = 1),
            colour_tags::Kind::Sup => set(fmts, inner, |f| f.script = 2),
        }
    }
}

/// Footnotes in a line (EDIT-050): `[^` and `]` of a reference to a defined footnote are markup,
/// its label is a footnote mark; the marker of a definition is the same with `]:`, and the text after
/// it (and the indented lines that go on with it) is footnote text. `defined` are the labels that
/// have a definition; `continuation` says the whole line goes on with a definition.
fn footnote_pairs(line: &str, off: usize, fmts: &mut [Fmt], defined: &std::collections::HashSet<String>, continuation: bool) {
    let markup = |fmts: &mut [Fmt], r: Range<usize>| {
        set(fmts, r, |f| {
            f.dim = true;
            f.syn = true
        })
    };
    for (r, label) in markdown::footnote_refs_in_line(line) {
        if !defined.contains(&label) {
            continue;
        }
        markup(fmts, off + r.start..off + r.start + 2);
        markup(fmts, off + r.end - 1..off + r.end);
        set(fmts, off + r.start + 2..off + r.end - 1, |f| f.fnote = 1);
    }
    if let Some((_, m)) = markdown::footnote_def_marker(line) {
        markup(fmts, off + m.start..off + m.start + 2);
        markup(fmts, off + m.end - 2..off + m.end);
        set(fmts, off + m.start + 2..off + m.end - 2, |f| f.fnote = 2);
        set(fmts, off + m.end..off + line.len(), |f| f.fnote = 3);
    } else if continuation {
        set(fmts, off..off + line.len(), |f| f.fnote = 3);
    }
}

/// Build the coloured layout for the editor (without inline pictures).
#[cfg(test)]
pub fn highlight(text: &str, links: &[(LinkSpan, bool)], base: f32) -> LayoutJob {
    highlight_preview(text, links, &EditorFonts::uniform(base), &[], None)
}

/// The coloured layout for the editor: the given byte ranges (image links) are shrunk to
/// invisible text on a row tall enough for the picture that is painted there afterwards. With
/// `raw_lines` (line numbers) the markdown preview is on (EDIT-016): the markup of every line
/// outside that range is hidden (invisible, about one point wide); the text itself stays the
/// same so that cursor, clicks and vim keep their indices. A height of 0 in `images` is text that
/// a fold hides: its rows have no height (EDIT-071); a negative one is text that is hidden and keeps its row (EDIT-081).
pub fn highlight_preview(text: &str, links: &[(LinkSpan, bool)], fonts: &EditorFonts, images: &[(Range<usize>, f32)], raw_lines: Option<Range<usize>>) -> LayoutJob {
    highlight_with_math(text, links, fonts, images, raw_lines).0
}

/// A formula of the note that the Markdown preview draws (EDIT-048): its lines are not raw.
#[derive(Clone)]
pub(crate) struct MathMark {
    /// the formula with its delimiters, bytes of the text
    pub range: Range<usize>,
    /// it takes whole lines: centred in a row of its own
    pub block: bool,
    pub lines: Range<usize>,
    /// the size of the text it stands in
    pub em: f32,
    pub color: Color32,
    pub formula: crate::math::Typeset,
}

/// The formulas that the preview draws (EDIT-048): those of the note whose lines are not raw (with
/// `raw_lines` the preview is on; without it every line is raw), left alone where a table's grid or
/// a picture takes the text's place. The size is that of the text of the line (a heading's is bigger).
pub(crate) fn math_marks(text: &str, base: f32, images: &[(Range<usize>, f32)], raw_lines: Option<&Range<usize>>) -> Vec<MathMark> {
    let Some(raw) = raw_lines else { return Vec::new() };
    let lines = markdown::lines_with_offsets(text);
    let mut out = Vec::new();
    for m in markdown::find_math(text) {
        if (m.lines.start < raw.end && raw.start < m.lines.end) || images.iter().any(|(r, _)| r.start < m.range.end && m.range.start < r.end) {
            continue;
        }
        let line = lines.get(m.lines.start).map_or("", |(_, l)| *l);
        let (em, color) = match markdown::heading_level(line) {
            _ if m.block => (base * crate::math::DISPLAY_SCALE, theme::TEXT),
            Some(level) => (base * heading_scale(level as u8), theme::HEADING),
            _ if line.trim_start().starts_with('>') => (base, theme::QUOTE),
            _ => (base, theme::TEXT),
        };
        let formula = crate::math::typeset(&text[m.tex.clone()], em, m.display);
        out.push(MathMark { range: m.range, block: m.block, lines: m.lines, em, color, formula });
    }
    out
}

/// [`highlight_preview`] and the formulas it drew typeset, which are painted over their hidden TeX.
pub(crate) fn highlight_with_math(text: &str, links: &[(LinkSpan, bool)], fonts: &EditorFonts, images: &[(Range<usize>, f32)], raw_lines: Option<Range<usize>>) -> (LayoutJob, Vec<MathMark>) {
    let mut fmts = vec![Fmt::default(); text.len()];
    let fenced = markdown::fenced_code_lines(text);
    for block in syntax::fenced_blocks(text) {
        let body = &text[block.body.clone()];
        for (r, kind) in syntax::highlight(&block.lang, body).iter() {
            set(&mut fmts, block.body.start + r.start..block.body.start + r.end, |f| f.syn_kind = Some(*kind));
        }
    }
    let mut code_ranges: Vec<Range<usize>> = Vec::new();
    let notes = markdown::find_footnotes(text);
    let mut defined = notes.defined();
    defined.extend(markdown::defined_footnotes_in_scope());
    let mut in_front = false;
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
        footnote_pairs(line, off, &mut fmts, &defined, notes.is_continuation(li));
        code_ranges.extend(local_code);
    }
    for (l, exists) in links {
        set(&mut fmts, l.range.clone(), |f| {
            f.dim = true;
            f.syn = true;
            f.italic = false;
        });
        let kind = if *exists { 1 } else { 2 };
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
    for (r, h) in images {
        if r.end <= text.len() && text.is_char_boundary(r.start) && text.is_char_boundary(r.end) {
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
    let marks = math_marks(text, fonts.rendered.size, images, raw_lines.as_ref());
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
    let mut job = LayoutJob { text: text.to_string(), ..Default::default() };
    let mut start = 0;
    let mut cur: Option<Fmt> = None;
    for (i, _) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        let f = if i < text.len() { Some(fmts[i]) } else { None };
        if f != cur || i == text.len() {
            if let Some(c) = cur {
                if i > start {
                    let (leading_space, format) = match marks.get((c.math as usize).wrapping_sub(1)).map(|m| (m, &m.formula)) {
                        Some((m, Ok(f))) if m.block => (0.0, crate::math::paint::block_format(f, m.em)),
                        Some((m, Ok(f))) => {
                            // the first character is a section of its own; the room is before the rest
                            let at = start + crate::math::paint::room_starts(&text[start..i]);
                            job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(at), format: crate::math::paint::hidden_format(f, m.em) });
                            start = at;
                            (f.layout.width, crate::math::paint::hidden_format(f, m.em))
                        }
                        _ if c.math_rest => (0.0, TextFormat { font_id: FontId::proportional(crate::math::paint::HIDDEN_SIZE), color: Color32::TRANSPARENT, line_height: Some(1.0), ..Default::default() }),
                        _ => (0.0, to_format(c, fonts)),
                    };
                    job.sections.push(egui::text::LayoutSection { leading_space, byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(i), format });
                }
            }
            start = i;
            cur = f;
        }
    }
    if job.sections.is_empty() {
        job.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len()), format: to_format(Fmt::default(), fonts) });
    }
    (job, marks)
}

/// A heading number to draw (LINK-014): where its title starts in the text, the number and the font.
pub(crate) struct NumberMark {
    pub title: usize,
    pub number: String,
    pub font: FontId,
}

/// Make room for the numbers of the headings in `job`: the section where a heading's title starts
/// gets leading space as wide as the number and a space, so the text is the buffer's text only and
/// every cursor position stays what it is. `raw_lines` are the lines drawn raw (as in
/// [`highlight_preview`]). The marks say where to draw the numbers.
pub(crate) fn make_room_for_numbers(ui: &Ui, job: &mut LayoutJob, fonts: &EditorFonts, vault_default: bool, raw_lines: Option<&Range<usize>>) -> Vec<NumberMark> {
    let mut marks = Vec::new();
    if !markdown::headings_numbered(&job.text, vault_default) {
        return marks;
    }
    for (line, level, _, number) in markdown::heading_numbers(&job.text) {
        let Some(number) = number else { continue };
        let at = markdown::line_start_byte(&job.text, line) + level + 1;
        let line_end = job.text[at..].find('\n').map_or(job.text.len(), |p| at + p);
        let Some(i) = job.sections.iter().position(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).filter(|_| at < line_end) else { continue };
        if job.sections[i].byte_range.start.0 < at {
            let mut tail = job.sections[i].clone();
            tail.byte_range.start = egui::text::ByteIndex(at);
            job.sections[i].byte_range.end = egui::text::ByteIndex(at);
            job.sections.insert(i + 1, tail);
        }
        let raw = raw_lines.is_none_or(|r| r.contains(&line));
        let font = to_format(Fmt { heading: level as u8, rendered: !raw, ..Fmt::default() }, fonts).font_id;
        let i = if job.sections[i].byte_range.start.0 == at { i } else { i + 1 };
        job.sections[i].leading_space = ui.painter().layout_no_wrap(format!("{number} "), font.clone(), Color32::WHITE).size().x;
        marks.push(NumberMark { title: at, number, font });
    }
    marks
}

/// Paint the numbers of the headings in the room [`make_room_for_numbers`] made, muted, on the
/// baseline of the heading's text.
fn paint_numbers(ui: &Ui, marks: &[NumberMark], galley: &Galley, galley_pos: Pos2, hidden: &[Range<usize>]) {
    let painter = ui.painter().with_clip_rect(ui.clip_rect());
    for m in marks.iter().filter(|m| !hidden.iter().any(|h| h.contains(&m.title))) {
        let c = markdown::byte_to_char(&galley.job.text, m.title.min(galley.job.text.len()));
        let r = galley.pos_from_cursor(CCursor::new(c));
        let mut first = 0;
        let Some(row) = galley.rows.iter().find(|row| {
            first += row.char_count_including_newline().0;
            c < first
        }) else {
            continue;
        };
        let baseline = row.row.glyphs.iter().filter(|g| g.font_height > 2.0).map(|g| g.pos.y).reduce(f32::max).unwrap_or(row.rect().height() * 0.78);
        let number = painter.layout_no_wrap(m.number.clone(), m.font.clone(), theme::MUTED);
        let ascent = number.rows.first().and_then(|r| r.row.glyphs.first()).map(|gl| gl.pos.y).unwrap_or(number.size().y * 0.8);
        let room = painter.layout_no_wrap(format!("{} ", m.number), m.font.clone(), theme::MUTED).size().x;
        painter.galley(galley_pos + Vec2::new(r.min.x - room, row.pos.y + baseline - ascent), number, theme::MUTED);
    }
}

/// Draw the formulas of the preview over the room their hidden TeX has (EDIT-048), and say why one
/// that could not be typeset is shown as code when the pointer is on it (EDIT-049).
fn paint_math(ui: &Ui, marks: &[MathMark], galley: &Galley, pos: Pos2, resp: &egui::Response) {
    let painter = ui.painter().with_clip_rect(ui.clip_rect());
    let text = &galley.job.text;
    let pointer = ui.input(|i| i.pointer.hover_pos()).filter(|_| resp.hovered());
    let under = pointer.map(|p| galley.cursor_from_pos(p - pos).index.0);
    for m in marks {
        if m.range.end > text.len() {
            continue;
        }
        let at = markdown::byte_to_char(text, m.range.start);
        match &m.formula {
            Ok(f) if m.block => crate::math::paint::paint_block_in_galley(&painter, galley, pos, at, (pos.x, resp.rect.right()), f, m.color),
            Ok(f) => crate::math::paint::paint_in_galley(&painter, galley, pos, at, f, m.color),
            Err(why) => {
                if under.is_some_and(|c| (at..markdown::byte_to_char(text, m.range.end)).contains(&c)) {
                    resp.clone().on_hover_text(format!("This formula cannot be typeset: {why}"));
                }
            }
        }
    }
}

/// A heading title as the outline shows it (LINK-013): the editor's inline styling (bold, italic,
/// code, strike, highlight, `<mark>` colours, links by their shown text) with the markup left out
/// of the text altogether, not just hidden.
pub fn outline_job(title: &str, links: &[(LinkSpan, bool)], base: f32) -> LayoutJob {
    let job = highlight_preview(title, links, &EditorFonts::uniform(base), &[], Some(1..1));
    let mut out = LayoutJob::default();
    for s in &job.sections {
        let hidden = s.format.color == Color32::TRANSPARENT && s.format.font_id.size <= 1.0;
        // a formula keeps its hidden TeX and the room for it, to be painted over (EDIT-048)
        if hidden && !crate::math::paint::is_hidden_formula(&s.format) {
            continue;
        }
        let start = out.text.len();
        out.text.push_str(&job.text[s.byte_range.start.0..s.byte_range.end.0]);
        out.sections.push(egui::text::LayoutSection { leading_space: s.leading_space, byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(out.text.len()), format: s.format.clone() });
    }
    out
}

/// A heading as the outline shows it: [`outline_job`] laid out as a clickable label, with its
/// formulas painted typeset over the room their hidden TeX has (EDIT-048).
pub fn outline_label(ui: &mut Ui, job: LayoutJob, base: f32) -> egui::Response {
    let mut job = job;
    job.wrap.max_width = ui.available_width();
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    ui.painter().galley(rect.min, galley.clone(), theme::TEXT);
    // a formula is two sections: its first character, and the rest with the room before it
    let sections = &galley.job.sections;
    for (n, s) in sections.iter().enumerate().filter(|(n, s)| *n > 0 && s.leading_space > 0.0 && crate::math::paint::is_hidden_formula(&s.format)) {
        let start = sections[n - 1].byte_range.start.0;
        let shown = &galley.job.text[start..s.byte_range.end.0];
        if let Ok(f) = crate::math::typeset(shown.trim_matches('$'), base, shown.starts_with("$$")) {
            let at = markdown::byte_to_char(&galley.job.text, start);
            crate::math::paint::paint_in_galley(ui.painter(), &galley, rect.min, at, &f, theme::TEXT);
        }
    }
    resp
}

fn to_format(f: Fmt, fonts: &EditorFonts) -> TextFormat {
    let area = if f.rendered { &fonts.rendered } else { &fonts.raw };
    let (family, base) = (area.family.clone(), area.size);
    if f.folded {
        return TextFormat { font_id: FontId::new(1.0, family), color: Color32::TRANSPARENT, line_height: Some(0.0), ..Default::default() };
    }
    if f.image_h > 0 {
        return TextFormat { font_id: FontId::new(1.0, family), color: Color32::TRANSPARENT, line_height: Some(f.image_h as f32), ..Default::default() };
    }
    let size = base * heading_scale(f.heading) * if f.fnote == 3 { FOOTNOTE_TEXT } else { 1.0 };
    if f.hidden {
        // the colour tags are far longer than the other markup: folded up smaller, so no gap shows
        let tiny = if f.ctag { 0.12 } else { 1.0 };
        return TextFormat { font_id: FontId::new(tiny, family), color: Color32::TRANSPARENT, line_height: Some(size * 1.5), ..Default::default() };
    }
    let mono = f.code || f.fence;
    // bold and italic in the font's own faces where it has them (CFG-039)
    let styled = area.styled(f.bold, f.italic || f.quote);
    let font_id = if mono { FontId::new(base * 0.92 * heading_scale(f.heading), FontFamily::Monospace) } else { FontId::new(size, styled.family.clone()) };
    let mut color = if f.heading > 0 { theme::HEADING } else { theme::TEXT };
    if f.quote {
        color = theme::QUOTE;
    }
    if f.bold {
        color = theme::TEXT_STRONG;
    }
    if mono {
        color = f.syn_kind.map_or(theme::CODE, theme::syntax_colour);
    }
    if f.marker {
        color = theme::ACCENT;
    }
    if f.tag {
        color = theme::TAG;
    }
    if f.fnote == 1 || f.fnote == 2 {
        color = theme::LINK;
    }
    if f.strike {
        color = theme::DONE;
    }
    // a font colour, or readable text on a coloured highlight (EDIT-029)
    if let Some(bg) = f.bg {
        color = if colour_tags::dark_text_on(bg) { Color32::BLACK } else { Color32::WHITE };
    }
    if let Some([r, g, b, a]) = f.fg {
        color = Color32::from_rgba_unmultiplied(r, g, b, a);
    }
    match f.link {
        1 | 3 => color = theme::LINK,
        2 => color = theme::LINK_MISSING,
        _ => {}
    }
    if f.dim {
        color = if f.link == 1 || f.link == 3 {
            theme::LINK.gamma_multiply(0.45)
        } else if f.link == 2 {
            theme::LINK_MISSING.gamma_multiply(0.45)
        } else {
            theme::FAINT
        };
    }
    let mut tf = TextFormat { font_id, color, italics: if mono { f.italic || f.quote } else { styled.fake_italic }, line_height: Some(size * 1.5 + f.math_extra as f32 / 4.0), ..Default::default() };
    if (f.fence && !f.boxed) || (f.code && !f.dim) {
        tf.background = theme::CODE_BG;
    }
    if f.highlight {
        tf.background = Color32::from_rgba_unmultiplied(200, 170, 40, 60);
    }
    if let Some([r, g, b, a]) = f.bg {
        tf.background = Color32::from_rgba_unmultiplied(r, g, b, a);
    }
    if f.tag {
        tf.background = theme::TAG_BG;
    }
    if f.link > 0 && !f.dim {
        tf.underline = Stroke::new(1.0, color.gamma_multiply(0.5));
    }
    if f.strike && !f.dim {
        tf.strikethrough = Stroke::new(1.0, theme::DONE);
    }
    if f.bold && (mono || styled.fake_bold) {
        tf.extra_letter_spacing = 0.3;
    }
    if f.under && !f.dim {
        tf.underline = Stroke::new(1.0, color);
    }
    // sub and superscript, and the mark of a footnote on a rendered line: smaller, and placed at the
    // bottom or the top of the row, which needs the text not to have a line height of its own
    let script = if f.script > 0 {
        f.script
    } else if f.rendered && (f.fnote == 1 || f.fnote == 2) {
        2
    } else {
        0
    };
    if script > 0 && !f.dim {
        tf.font_id.size *= SCRIPT_SCALE;
        tf.line_height = None;
        tf.valign = if script == 2 { egui::Align::TOP } else { egui::Align::BOTTOM };
    }
    tf
}

/// The size of sub and superscript and of the mark of a footnote, in parts of the text (EDIT-051, EDIT-050).
pub const SCRIPT_SCALE: f32 = 0.7;
/// The size of the text of a footnote definition, in parts of the text (EDIT-050).
pub const FOOTNOTE_TEXT: f32 = 0.9;

/// Something shown in place of an embed link in the editor.
enum Art {
    Tex(egui::TextureHandle),
    /// a drawing and its path (its pictures are found from there)
    Scene(Arc<crate::drawing::Scene>, std::path::PathBuf),
    /// first page of a PDF, and its page count
    Pdf(egui::TextureHandle, usize),
    /// the player of a sound file (MEDIA-001): the file, and the card's number among the embeds
    Audio(std::path::PathBuf),
    /// a picture that is still a Git LFS pointer: the "Download" strip in its place (SYNC-020)
    Download(std::path::PathBuf),
}

/// What the editor asks the app to do.
pub enum EditorAction {
    OpenLink {
        target: String,
        heading: Option<String>,
        new_tab: bool,
    },
    /// A web link: open it in the default browser.
    OpenUrl(String),
    /// A file was copied into the vault; the file list should be refreshed.
    FilesChanged,
    /// `[[p Name` picked "new contact": make a contact note in the people folder
    CreatePerson(String),
    /// pasted pictures / files were saved in the media folder (EDIT-019): refresh the file list and say what was saved
    Pasted(String),
    /// the pasted text has headings, all of them `##` or deeper: ask whether to lift them (EDIT-038);
    /// the text is held back and comes back with [`NoteEditor::pending_paste`]
    AskLiftHeadings(String),
}

#[derive(Default)]
struct Cache {
    key: u64,
    links: Vec<(LinkSpan, bool)>,
}

/// The note an editor shows: its vault path and the undo histories (the note's is found by the path).
pub struct Note<'a> {
    pub rel: &'a Path,
    pub undo: &'a mut crate::undo::Histories,
}

pub struct NoteEditor {
    pub id: egui::Id,
    /// The width of the text column (points) of the last frame the editor laid out: the PDF export
    /// sizes embeds in proportion to it (PDF-005).
    pub column: Option<f32>,
    cache: Cache,
    /// `[[`, `[[p ` and `[!` helpers of the text box
    pub assist: crate::link_assist::LinkAssist,
    /// Put the cursor here (char index) on the next frame and scroll to it.
    pub pending_cursor: Option<usize>,
    /// The next scroll to `pending_cursor` puts its line at the top of the view (a heading link, LINK-015).
    pub scroll_to_top: bool,
    /// Draw the numbers of the headings (LINK-014, CFG-042): the setting; a note's front matter overrides it.
    pub numbered_headings: bool,
    pub request_focus: bool,
    /// Text to paste at the cursor in the next frame: the answer to [`EditorAction::AskLiftHeadings`] (EDIT-038).
    pub pending_paste: Option<String>,
    /// Frames left to ask again when the request was not granted (a dialog that has just closed
    /// still blocks the keyboard for the window behind it for a frame).
    focus_retry: u8,
    /// Where the text was drawn last frame (used by tests and for scrolling).
    pub last_layout: Option<(Arc<Galley>, Pos2)>,
    /// Draw embedded images, drawings and PDFs inside the editor (set from the app's settings).
    pub inline_images: bool,
    /// Draw embedded drawings on a dark canvas (set from the app's settings).
    pub dark_canvas: bool,
    /// The app's media player, which the players of sound embeds draw from (MEDIA-001).
    pub media: crate::media::MediaPlayer,
    /// Resizing and fullscreen of shown pictures and videos (EDIT-052, MEDIA-012).
    pub embeds: crate::media::embed::Embeds,
    /// Show a gutter with line numbers (set from the app's settings, EDIT-018).
    pub line_numbers: bool,
    /// Draw the markdown rendered except around the cursor (EDIT-016, set from the app's settings).
    pub live_preview: bool,
    /// Lines before and after the cursor line that stay raw in the preview (EDIT-017).
    pub preview_raw_lines: u32,
    /// The raw lines of the last frame (kept while the mouse button is down, so a click lands on
    /// the text it was aimed at).
    preview_raw: Range<usize>,
    last_cursor: Option<(usize, usize)>,
    /// vim-lite editing is on (set from the app's settings)
    pub vim_enabled: bool,
    pub vim: crate::vim::Vim,
    /// the editor had the keyboard last frame / when it lost it (seconds)
    vim_was_focused: bool,
    vim_lost_at: Option<f64>,
    /// the cursor range vim put into the text box last (a different one means a mouse click)
    vim_expected: Option<(usize, usize)>,
    /// scroll this char into view after the text box was drawn
    vim_scroll: Option<usize>,
    /// `gt` was typed this frame: follow the link under vim's cursor (EDIT-023)
    vim_follow: bool,
    /// where vim's block cursor was drawn this frame (normal mode with the keyboard): the link preview hangs below it (EDIT-033)
    pub vim_cursor_rect: Option<Rect>,
    /// the shapes and the blinking of the text cursor (EDIT-039…EDIT-041)
    pub cursor_style: crate::cursor_style::CursorStyle,
    /// where Esc from insert mode puts the cursor (EDIT-043)
    pub esc_cursor: crate::vim::EscCursor,
    /// where the cursor was drawn last frame (char, normal mode) and when it moved there (seconds)
    cursor_seen: Option<((usize, bool), f64)>,
    /// The formatting toolbar above the text (EDIT-026)
    bar: crate::format_bar::FormatBar,
    /// A toolbar command to do to the text at the start of the next frame of the editor (EDIT-027)
    format_request: Option<crate::format::Cmd>,
    /// The selection (char indices, as the text box has them) when the editor last had the keyboard:
    /// a press on the toolbar takes the keyboard away, and the text box then drops its selection
    last_selection: Option<(usize, usize)>,
    /// The selection a toolbar command left and the frames it is still put into the text box for: the
    /// text box drops it in the frame it loses the keyboard, and the keyboard comes back a frame later
    keep_selection: Option<((usize, usize), u8)>,
    /// a button of a table grid was clicked: the table (by its number in the note) and the change
    /// to make at the start of the next frame (EDIT-025, EDIT-030)
    table_edit: Option<(usize, crate::table::Change)>,
    /// editing cells and resizing in the grids of tables (EDIT-030, EDIT-031)
    grid: crate::table_grid::TableGrid,
    /// a click on a table's grid edits a cell, for tables that do not say otherwise (set from the app's settings, EDIT-030)
    pub table_cell_edit: bool,
    /// the text box had the keyboard before the pointer went down (a click on a table button takes
    /// it away, and it is given back)
    focus_before_press: bool,
    /// the folded blocks of the notes this editor showed, and what is drawn for them (EDIT-070…EDIT-079)
    pub(crate) fold: crate::fold_ui::FoldUi,
    /// a folded front matter shows a summary line, and a note opens with it folded: the setting
    /// "properties in the editor" is not Raw (EDIT-081, set from the app's settings)
    pub properties_summary: bool,
}

impl NoteEditor {
    pub fn new(id: egui::Id) -> Self {
        NoteEditor {
            id,
            column: None,
            cache: Cache::default(),
            assist: crate::link_assist::LinkAssist::new(id),
            pending_cursor: None,
            scroll_to_top: false,
            numbered_headings: false,
            request_focus: false,
            pending_paste: None,
            focus_retry: 0,
            last_layout: None,
            inline_images: false,
            dark_canvas: true,
            media: crate::media::MediaPlayer::default(),
            embeds: Default::default(),
            line_numbers: false,
            live_preview: false,
            preview_raw_lines: 0,
            preview_raw: 0..0,
            last_cursor: None,
            vim_enabled: false,
            vim: crate::vim::Vim::default(),
            vim_was_focused: false,
            vim_lost_at: None,
            vim_expected: None,
            vim_scroll: None,
            table_edit: None,
            grid: Default::default(),
            table_cell_edit: true,
            focus_before_press: false,
            fold: Default::default(),
            properties_summary: true,
            vim_follow: false,
            vim_cursor_rect: None,
            cursor_style: crate::cursor_style::CursorStyle::default(),
            esc_cursor: crate::vim::EscCursor::default(),
            cursor_seen: None,
            bar: Default::default(),
            format_request: None,
            last_selection: None,
            keep_selection: None,
        }
    }

    /// The selection of the text box as bytes (a cursor is an empty range); the whole text's end when it has none yet.
    pub fn selection_bytes(&self, ctx: &egui::Context, text: &str) -> Range<usize> {
        let focused = ctx.memory(|m| m.has_focus(self.id));
        let in_box = egui::text_edit::TextEditState::load(ctx, self.id).and_then(|st| st.cursor.char_range()).map(|r| (r.primary.index.0, r.secondary.index.0));
        match if focused { in_box } else { self.last_selection.or(in_box) } {
            Some((p, s)) => {
                let (a, b) = (markdown::char_to_byte(text, p), markdown::char_to_byte(text, s));
                a.min(b)..a.max(b)
            }
            None => text.len()..text.len(),
        }
    }

    /// Draw the formatting toolbar (EDIT-026); a click is done to the text by the next `show`.
    /// While a table cell is edited in the grid the bar works on the text of the cell (EDIT-030).
    pub fn toolbar(&mut self, ui: &mut Ui, text: &str, palettes: &crate::format_bar::Palettes) {
        let in_cell = self.grid.cell.is_some();
        let (shown, sel) = match &self.grid.cell {
            Some(c) => (c.buf.as_str(), c.selection_bytes()),
            None => (text, self.selection_bytes(ui.ctx(), text)),
        };
        let bar = ui.scope(|ui| self.bar.show(ui, shown, sel, in_cell, palettes));
        // a press on the bar (or on its popups) takes the keyboard from the cell's box: that does not end the editing
        let pressed = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
        self.grid.hold = in_cell && pressed && (ui.rect_contains_pointer(bar.response.rect) || egui::Popup::is_any_open(ui.ctx()));
        if let Some(cmd) = bar.inner {
            self.format_request = Some(cmd);
            // the click took the keyboard from the text box: give it back
            self.request_focus = !in_cell;
            ui.ctx().request_repaint();
        }
    }

    /// The keyboard goes back to a text box that has none (EDIT-042): it dropped its selection while it had
    /// none, so the one it had last is put back (vim-lite keeps its own cursor).
    fn restore_selection(&self, ctx: &egui::Context, text: &str) {
        if self.vim_enabled || self.keep_selection.is_some() || ctx.memory(|m| m.has_focus(self.id)) {
            return;
        }
        if let Some((a, b)) = self.last_selection {
            let n = text.chars().count();
            let mut state = egui::text_edit::TextEditState::load(ctx, self.id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a.min(n)), CCursor::new(b.min(n)))));
            state.store(ctx, self.id);
        }
    }

    /// The key of a toolbar button (KEY-015): the same as a click on it. A button that is greyed out in a cell does nothing.
    pub fn key_format(&mut self, cmd: crate::format::Cmd) {
        let in_cell = self.grid.cell.is_some();
        if crate::format_bar::enabled_in(cmd, in_cell) {
            self.format_request = Some(cmd);
            self.request_focus = !in_cell;
        }
    }

    /// The key of a fold command (KEY-030…KEY-033): done to the folds at the start of the next frame.
    pub fn key_fold(&mut self, cmd: crate::fold::FoldCmd) {
        self.fold.request = Some(cmd);
    }

    /// The line (from 0) the cursor is in: vim's cursor in normal mode, else the text box's.
    fn cursor_line(&self, ui: &Ui, text: &str) -> usize {
        let normal = self.vim_enabled && self.vim.mode != crate::vim::Mode::Insert && !self.vim.visual();
        let char = if normal { Some(self.vim.cursor) } else { self.text_cursor(ui).map(|c| c.0) };
        crate::editor_preview::line_of(text, markdown::char_to_byte(text, char.unwrap_or(0)))
    }

    /// The key of a toolbar button that opens a popup (KEY-015); the heading menu is greyed out in a cell.
    pub fn key_menu(&mut self, menu: crate::format_bar::Menu) {
        if menu != crate::format_bar::Menu::Heading || self.grid.cell.is_none() {
            self.bar.open(menu);
        }
    }

    /// Does the note's text (or the cell being edited) have the keyboard, or does no text field
    /// have it? Then the Alt keys of the note view are for the note (KEY-014).
    pub fn owns_keyboard(&self, ctx: &egui::Context) -> bool {
        !ctx.egui_wants_keyboard_input() || ctx.memory(|m| m.has_focus(self.id)) || self.grid.cell.is_some()
    }

    /// Do a toolbar command to the text: one undo step (EDIT-021), the result selected, the keyboard
    /// back in the text box, and vim-lite in a mode that takes keys (EDIT-027). True when the text changed.
    fn apply_format(&mut self, ui: &Ui, text: &mut String, cmd: crate::format::Cmd, hist: &mut crate::undo::History) -> bool {
        self.request_focus = true;
        let sel = self.selection_bytes(ui.ctx(), text);
        let Some((new, r)) = crate::format::apply(cmd, text, sel) else { return false };
        hist.checkpoint(text);
        *text = new;
        hist.checkpoint(text);
        let (cs, ce) = (markdown::byte_to_char(text, r.start), markdown::byte_to_char(text, r.end));
        let normal = self.vim_enabled && self.vim.mode != crate::vim::Mode::Insert;
        // normal mode has a cursor, not a selection
        let sel = if normal { (cs, cs) } else { (cs, ce) };
        self.last_selection = Some(sel);
        self.keep_selection = Some((sel, 3));
        if normal {
            self.vim.reset(text, cs);
            self.vim_apply_cursor(ui, text);
        }
        true
    }

    /// The table cell being edited in the grid: (row, column) (testing aid).
    #[cfg(test)]
    pub(crate) fn editing_cell(&self) -> Option<(usize, usize)> {
        self.grid.cell.as_ref().map(|c| (c.row, c.col))
    }

    /// The cells of the block selected with Shift+arrow keys, as (row, column) (testing aid).
    #[cfg(test)]
    pub(crate) fn selected_cells(&self) -> Vec<(usize, usize)> {
        self.grid.cell.as_ref().and_then(|c| c.block).map_or(vec![], |b| b.cells())
    }

    /// The id of the box of the cell being edited (testing aid).
    #[cfg(test)]
    pub(crate) fn cell_box_id(&self) -> egui::Id {
        crate::table_grid::TableGrid::cell_id(self.id)
    }

    /// Is a cell of a table grid being edited, or a block of cells selected? The grid then has
    /// the keyboard even when no widget has it (NAV-037).
    pub fn grid_has_keyboard(&self) -> bool {
        self.grid.cell.is_some()
    }

    /// Is the `[!` file picker popup on screen?
    pub fn picker_visible(&self) -> bool {
        self.assist.picker_visible()
    }

    /// Short description of the picker state (testing aid).
    pub fn debug_state(&self) -> String {
        self.assist.debug_state()
    }

    /// The raw lines of the markdown preview for this frame.
    fn preview_lines(&mut self, ui: &Ui, text: &str) -> Range<usize> {
        let focused = ui.ctx().memory(|m| m.has_focus(self.id));
        let sel = focused.then(|| egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range())).flatten().map(|r| {
            let a = markdown::char_to_byte(text, r.primary.index.0);
            let b = markdown::char_to_byte(text, r.secondary.index.0);
            a.min(b)..a.max(b)
        });
        let want = crate::editor_preview::raw_lines(text, sel, self.preview_raw_lines as usize);
        if want != self.preview_raw && ui.input(|i| i.pointer.primary_down() || i.pointer.primary_released()) {
            // a click or drag must not move the text under the pointer; catch up afterwards
            ui.ctx().request_repaint();
        } else {
            self.preview_raw = want;
        }
        self.preview_raw.clone()
    }

    /// Typing goes into the text (vim-lite off, or in insert mode).
    fn typing(&self) -> bool {
        !self.vim_enabled || self.vim.mode == crate::vim::Mode::Insert
    }

    /// This editor has the keyboard in vim normal mode, not typing a `:` or `/` command: the
    /// keys of the "Editor normal mode" scope (EDIT-015) are looked up before vim sees them.
    pub fn vim_normal_keys(&self, ctx: &egui::Context) -> bool {
        self.vim_enabled && self.vim.mode == crate::vim::Mode::Normal && !self.vim.typing_command() && ctx.memory(|m| m.has_focus(self.id))
    }

    /// The vim mode (or command) for the status bar, while this editor has the keyboard.
    pub fn vim_status(&self) -> Option<String> {
        (self.vim_enabled && self.vim_was_focused).then(|| self.vim.status())
    }

    fn text_cursor(&self, ui: &Ui) -> Option<(usize, usize)> {
        egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).map(|r| (r.primary.index.0, r.secondary.index.0))
    }

    /// Put vim's cursor or selection into the text box.
    fn vim_apply_cursor(&mut self, ui: &Ui, text: &str) {
        let range = if self.vim.visual() {
            let (a, b) = self.vim.selection(text);
            // the cursor is drawn at the moving end
            if self.vim.cursor >= self.vim.anchor() {
                CCursorRange::two(CCursor::new(a), CCursor::new(b))
            } else {
                CCursorRange::two(CCursor::new(b), CCursor::new(a))
            }
        } else {
            CCursorRange::one(CCursor::new(self.vim.cursor))
        };
        let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
        state.cursor.set_char_range(Some(range));
        state.store(ui.ctx(), self.id);
        self.vim_expected = Some((range.primary.index.0, range.secondary.index.0));
    }

    /// Vim-lite for this frame: outside insert mode every key goes to vim instead of the text.
    /// Returns true when the text changed.
    fn vim_frame(&mut self, ui: &mut Ui, text: &mut String, hist: &mut crate::undo::History) -> bool {
        use crate::vim::{Effects, Mode, VimKey};
        let focused = ui.ctx().memory(|m| m.has_focus(self.id));
        let now = ui.input(|i| i.time);
        if focused && !self.vim_was_focused {
            // the note got the keyboard: normal mode (a blink of lost focus keeps insert mode)
            let blink = self.vim.mode == Mode::Insert && self.vim_lost_at.is_some_and(|t| now - t < 0.5);
            if !blink {
                hist.checkpoint(text);
                let cur = self.text_cursor(ui).map(|c| c.0).unwrap_or(0);
                self.vim.reset(text, cur);
                self.vim_apply_cursor(ui, text);
            }
        }
        if !focused && self.vim_was_focused {
            self.vim_lost_at = Some(now);
        }
        self.vim_was_focused = focused;
        if !focused {
            return false;
        }
        if self.vim.mode == Mode::Insert {
            // Esc back to normal mode (an open popup took its Esc already, above)
            if !self.assist.is_open() && !self.assist.picker_visible() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
                let cur = self.text_cursor(ui).map(|c| c.0).unwrap_or(0);
                self.vim.leave_insert(text, cur, self.esc_cursor);
                self.vim_apply_cursor(ui, text);
            }
            return false;
        }
        // where the text box has the cursor: a click there moves vim's cursor (and ends visual)
        if let Some(tc) = self.text_cursor(ui) {
            if Some(tc) != self.vim_expected {
                if self.vim.visual() {
                    self.vim.reset(text, tc.0);
                } else {
                    self.vim.set_cursor(text, tc.0);
                }
                self.vim_apply_cursor(ui, text);
            }
        }
        // take every key that would type or move in the text box; Ctrl/Alt chords stay
        let paste_pending = self.vim.paste_pending;
        let typing_command = self.vim.typing_command();
        let (keys, pastes) = ui.input_mut(|i| {
            let (mut keys, mut pastes) = (Vec::new(), Vec::new());
            i.events.retain(|e| match e {
                egui::Event::Text(t) => {
                    keys.extend(t.chars().map(VimKey::Char));
                    false
                }
                // Ctrl+R redoes (EDIT-021); the other Ctrl chords stay
                egui::Event::Key { key: Key::R, pressed, modifiers, .. } if (modifiers.ctrl || modifiers.command) && !modifiers.alt && !modifiers.shift && !typing_command => {
                    if *pressed {
                        keys.push(VimKey::Redo);
                    }
                    false
                }
                egui::Event::Key { key, pressed, modifiers, .. } if !(modifiers.ctrl || modifiers.command || modifiers.alt || modifiers.mac_cmd) => {
                    if *pressed {
                        let k = match key {
                            Key::Escape => Some(VimKey::Esc),
                            Key::Enter => Some(VimKey::Enter),
                            Key::Backspace => Some(VimKey::Backspace),
                            Key::ArrowLeft => Some(VimKey::Left),
                            Key::ArrowRight => Some(VimKey::Right),
                            Key::ArrowUp => Some(VimKey::Up),
                            Key::ArrowDown => Some(VimKey::Down),
                            _ => None,
                        };
                        keys.extend(k);
                    }
                    false
                }
                egui::Event::Paste(p) if paste_pending => {
                    pastes.push(p.clone());
                    false
                }
                egui::Event::Ime(_) => false,
                _ => true,
            });
            (keys, pastes)
        });
        let mut fx = Effects::default();
        for p in pastes {
            self.vim.paste(&p, text, &mut fx);
        }
        let mut typed_after = String::new();
        for k in keys {
            if self.vim.mode == Mode::Insert {
                // keys after `i` in the same frame are typed (scripted input sends them together)
                if let VimKey::Char(ch) = k {
                    typed_after.push(ch);
                }
                continue;
            }
            self.vim.key(k, text, hist, &mut fx);
        }
        if let Some(c) = fx.copy.take() {
            ui.ctx().copy_text(c);
        }
        if fx.request_paste {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
        self.vim_apply_cursor(ui, text);
        self.vim_follow = fx.follow_link;
        if !typed_after.is_empty() {
            ui.input_mut(|i| i.events.push(egui::Event::Text(typed_after)));
        }
        if fx.moved || fx.changed {
            self.vim_scroll = Some(self.vim.cursor);
        }
        if fx.fold.is_some() {
            self.fold.request = fx.fold;
        }
        if self.vim.mode != Mode::Insert {
            // no input method outside insert mode: keys must arrive as they are
            ui.ctx().output_mut(|o| o.ime = None);
        }
        fx.changed
    }

    /// A pasted picture or list of media files becomes a link to the file saved in the media folder
    /// (EDIT-019): the paste event is turned into a paste of the link, so the text box (or vim's `p`)
    /// puts it in as any other paste. Other pastes are left alone.
    fn paste_media(&mut self, ui: &mut Ui, vault: &Vault) -> Option<EditorAction> {
        self.paste_media_with(ui, vault, crate::media::clipboard_image)
    }

    /// `paste_media` with the clipboard's picture given: a pasted text that is only an `<img>` tag
    /// (a picture copied in a browser) is replaced by that picture if the clipboard has one.
    fn paste_media_with(&mut self, ui: &mut Ui, vault: &Vault, clipboard_image: impl Fn() -> Option<egui::ColorImage>) -> Option<EditorAction> {
        if !ui.ctx().memory(|m| m.has_focus(self.id)) {
            return None;
        }
        // vim's `p` puts a link on a line of its own below the cursor line (as it does with a line of text)
        let linewise = self.vim_enabled && self.vim.paste_pending;
        let date = crate::media::local_date();
        let mut messages: Vec<String> = Vec::new();
        ui.input_mut(|i| {
            i.events.retain_mut(|e| {
                if let egui::Event::Paste(t) = e {
                    if crate::media::is_lone_img_tag(t) {
                        if let Some(img) = clipboard_image() {
                            *e = egui::Event::PasteImage(std::sync::Arc::new(img));
                        }
                    }
                }
                let links = match e {
                    egui::Event::PasteImage(img) => match crate::media::paste_image_to_link(&vault.root, img, date) {
                        Ok(link) => {
                            messages.push("Saved the pasted picture to the media folder".into());
                            Some(link)
                        }
                        Err(err) => {
                            messages.push(format!("Could not paste the picture: {err}"));
                            return false;
                        }
                    },
                    egui::Event::Paste(t) => crate::media::paste_text_to_links(&vault.root, t, date).map(|(links, msg)| {
                        messages.push(msg);
                        links
                    }),
                    _ => None,
                };
                if let Some(mut links) = links {
                    if linewise {
                        links.push('\n');
                    }
                    *e = egui::Event::Paste(links);
                }
                true
            })
        });
        (!messages.is_empty()).then(|| EditorAction::Pasted(messages.join(". ")))
    }

    /// A pasted text with headings, all `##` or deeper, is not pasted yet: the editor asks the app
    /// whether to lift them (EDIT-038). Text without headings or with a `#` is left alone.
    fn hold_paste_without_h1(&mut self, ui: &mut Ui) -> Option<EditorAction> {
        if !ui.ctx().memory(|m| m.has_focus(self.id)) || (self.vim_enabled && self.vim.paste_pending) {
            return None;
        }
        let mut held = None;
        ui.input_mut(|i| {
            i.events.retain(|e| match e {
                egui::Event::Paste(t) if held.is_none() && markdown::lift_headings(t).is_some() => {
                    held = Some(t.clone());
                    false
                }
                _ => true,
            })
        });
        held.map(EditorAction::AskLiftHeadings)
    }

    fn refresh_links(&mut self, text: &str, vault: &Vault, rel: &Path) {
        let mut h = DefaultHasher::new();
        text.hash(&mut h);
        vault.generation.hash(&mut h);
        rel.hash(&mut h);
        let key = h.finish();
        if key != self.cache.key {
            self.cache.key = key;
            self.cache.links = markdown::find_links(text)
                .into_iter()
                .map(|l| {
                    let ok = vault.exists(&l.target, Some(rel));
                    (l, ok)
                })
                .collect();
        }
    }

    /// The wiki link (with whether its note exists) or else the web link at a byte of the text;
    /// embedded images (`is_image` says which ranges are drawn as one) are not links.
    fn link_at(&self, text: &str, byte: usize, is_image: &dyn Fn(&Range<usize>) -> bool) -> (Option<(LinkSpan, bool)>, Option<crate::lists::WebLink>) {
        let link = self.cache.links.iter().find(|(l, _)| l.range.contains(&byte) && !is_image(&l.range)).cloned();
        let web = if link.is_none() { crate::lists::find_web_links(text).into_iter().find(|w| w.range.contains(&byte)) } else { None };
        (link, web)
    }

    /// The caption of an embed shown inside the editor (EDIT-053), one line of muted text below
    /// `art` (the picture's rectangle), shortened to the room it has. A click on it opens the
    /// link's target.
    fn paint_caption(ui: &mut Ui, link: &LinkSpan, art: Rect, (base_size, max_w): (f32, f32), id: egui::Id) -> Option<EditorAction> {
        let size = crate::caption::size(base_size);
        let font = egui::FontId::proportional(size);
        let room = art.width().max(160.0).min(max_w);
        let shown = crate::caption::fit(&crate::caption::text(link), room, |t| ui.painter().layout_no_wrap(t.to_string(), font.clone(), Color32::WHITE).size().x);
        let galley = ui.painter().layout_no_wrap(shown, font, theme::MUTED);
        let rect = Rect::from_min_size(Pos2::new(art.min.x, art.max.y + crate::caption::GAP), galley.size());
        // click and drag, as the player's controls: the text box under it would take a plain click
        let resp = ui.interact(rect, id, egui::Sense::click_and_drag());
        let hovered = resp.hovered();
        if hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        let colour = if hovered { theme::LINK } else { theme::MUTED };
        ui.painter().with_clip_rect(ui.clip_rect()).galley(rect.min, galley, colour);
        resp.clicked().then(|| EditorAction::OpenLink { target: link.target.clone(), heading: link.heading.clone(), new_tab: false })
    }

    /// What following a link does: the one place a click and `gt` (EDIT-023) share.
    fn follow_action(link: Option<&(LinkSpan, bool)>, web: Option<&crate::lists::WebLink>, new_tab: bool) -> Option<EditorAction> {
        match (web, link) {
            (Some(w), _) => Some(EditorAction::OpenUrl(w.url.clone())),
            (None, Some((l, _))) => Some(EditorAction::OpenLink { target: l.target.clone(), heading: l.heading.clone(), new_tab }),
            (None, None) => None,
        }
    }

    /// Change the table with this number in the note (EDIT-025, EDIT-030, EDIT-031). With `step`
    /// the change is one undoable step of its own; without, it is part of the editing of a cell,
    /// whose step starts before its first change (and is made when it ends). The cursor stays
    /// where it was in the text: what moved is the text behind the table.
    fn apply_table_edit(&mut self, ui: &Ui, text: &mut String, hist: &mut crate::undo::History, n: usize, change: &crate::table::Change, step: bool) -> bool {
        let Some(b) = crate::table::find_tables(text).into_iter().nth(n) else { return false };
        let r = crate::table::byte_range(text, &b.lines);
        let Some(new) = crate::table::change_block(&text[r.clone()], b.has_meta, &b.meta, change) else { return false };
        if new == text[r.clone()] {
            return false;
        }
        if step {
            hist.checkpoint(text);
        } else if let Some(c) = self.grid.cell.as_mut().filter(|c| c.before.is_none()) {
            hist.checkpoint(text);
            c.before = Some(text.clone());
        }
        let before = text.clone();
        let (end, grown) = (markdown::byte_to_char(text, r.end), new.chars().count() as isize - text[r.clone()].chars().count() as isize);
        text.replace_range(r, &new);
        // the cursor stays where it was in the text: what moved is the text behind the table
        let at = |c: CCursor| if c.index.0 >= end { CCursor::new((c.index.0 as isize + grown).max(0) as usize) } else { c };
        let cursor = TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).map_or(CCursorRange::one(CCursor::new(markdown::byte_to_char(&before, before.len()))), |r| CCursorRange::two(at(r.primary), at(r.secondary)));
        if step {
            hist.checkpoint(text);
            self.record_edit(ui.ctx(), &before, text, cursor);
        } else {
            let mut state = TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
            state.cursor.set_char_range(Some(cursor));
            state.store(ui.ctx(), self.id);
        }
        self.last_cursor = None;
        true
    }

    /// The editing of a cell ended (EDIT-030): everything it typed becomes one undo step.
    fn end_cell_edit(&mut self, ui: &Ui, text: &str, hist: &mut crate::undo::History, before: Option<String>, give_back: bool) {
        if let Some(b) = before.filter(|b| b != text) {
            hist.checkpoint(text);
            let cursor = TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).unwrap_or(CCursorRange::one(CCursor::new(0)));
            self.record_edit(ui.ctx(), &b, text, cursor);
        }
        if give_back {
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
        }
    }

    /// Make a change that the program made to the text (not the person typing it) a step of its
    /// own in the text box's undo history (Ctrl+Z, Ctrl+Y): the text before it and the text after
    /// it become undo points, so the change neither merges with the typing around it nor goes
    /// unnoticed by the undo. The cursor is set to `cursor` (EDIT-025).
    fn record_edit(&self, ctx: &egui::Context, before: &str, after: &str, cursor: CCursorRange) {
        let mut state = egui::text_edit::TextEditState::load(ctx, self.id).unwrap_or_default();
        let was = state.cursor.char_range().unwrap_or(CCursorRange::one(CCursor::new(before.chars().count())));
        let mut undoer = state.undoer();
        undoer.add_undo(&(was, before.to_string()));
        undoer.add_undo(&(cursor, after.to_string()));
        state.set_undoer(undoer);
        state.cursor.set_char_range(Some(cursor));
        state.store(ctx, self.id);
    }

    /// The buttons of a table grid while the pointer is over it (EDIT-025): add a column after the
    /// last one (right of the header) and a row after the last (under the grid); for the hovered
    /// column (above the grid) and row (right of the grid) insert after it and delete it. A click
    /// only sets `table_edit`; the cursor does not enter the table.
    fn table_buttons(&mut self, ui: &mut Ui, origin: Pos2, laid: &crate::table::Laid, ordinal: usize, meta: &crate::table::Meta, cells: bool) {
        use crate::table::{Change, Edit};
        let size = laid.size();
        let key = self.id.with("table-buttons");
        let grid = Rect::from_min_size(origin, size);
        let zone = Rect::from_min_max(grid.min - Vec2::new(0.0, TABLE_BTN + 4.0), grid.max + Vec2::new(TABLE_SIDE, TABLE_BTN + 6.0));
        let Some(p) = ui.input(|i| i.pointer.hover_pos()).filter(|p| zone.contains(*p)) else {
            ui.ctx().data_mut(|d| d.insert_temp(key, Vec::<(String, Rect)>::new()));
            return;
        };
        let at = |sizes: &[f32], v: f32| {
            let mut sum = 0.0;
            sizes
                .iter()
                .position(|s| {
                    sum += s;
                    v < sum
                })
                .unwrap_or(sizes.len() - 1)
        };
        let (row, col) = (at(&laid.heights, p.y - origin.y), at(&laid.widths, p.x - origin.x));
        let (ncols, data) = (laid.widths.len(), laid.heights.len() - 1);
        let row_top = origin.y + laid.heights[..row].iter().sum::<f32>();
        let col_mid = origin.x + laid.widths[..col].iter().sum::<f32>() + laid.widths[col] / 2.0;
        let side = grid.right() + 4.0;
        let centre = |top: f32, h: f32| top + (h - TABLE_BTN) / 2.0;
        let button = |x: f32, y: f32| Rect::from_min_size(Pos2::new(x, y), Vec2::splat(TABLE_BTN));
        let last_row = if data == 0 { Edit::InsertRow(None) } else { Edit::InsertRow(Some(data - 1)) };
        let after_row = if row == 0 { Edit::InsertRow(None) } else { Edit::InsertRow(Some(row - 1)) };
        // (name, where, glyph, tooltip, the change; none: not possible)
        let buttons: [(&str, Rect, &str, &str, Option<Edit>); 6] = [
            ("add-column", button(side, centre(origin.y, laid.heights[0])), "+", "Add a column at the end", Some(Edit::InsertColumn(ncols - 1))),
            ("add-row", Rect::from_min_size(Pos2::new(grid.left(), grid.bottom() + 2.0), Vec2::new(size.x, TABLE_BTN)), "+", "Add a row at the end", Some(last_row)),
            ("insert-row", button(side + TABLE_BTN + 4.0, centre(row_top, laid.heights[row])), "+", "Add a row below this one", Some(after_row)),
            (
                "delete-row",
                button(side + 2.0 * TABLE_BTN + 6.0, centre(row_top, laid.heights[row])),
                "\u{d7}",
                if row == 0 {
                    "The header row cannot be deleted"
                } else if data <= 1 {
                    "The last row cannot be deleted"
                } else {
                    "Delete this row"
                },
                (row > 0 && data > 1).then(|| Edit::DeleteRow(row - 1)),
            ),
            ("insert-column", button(col_mid - TABLE_BTN - 1.0, grid.top() - TABLE_BTN - 2.0), "+", "Add a column after this one", Some(Edit::InsertColumn(col))),
            ("delete-column", button(col_mid + 1.0, grid.top() - TABLE_BTN - 2.0), "\u{d7}", if ncols <= 1 { "The last column cannot be deleted" } else { "Delete this column" }, (ncols > 1).then_some(Edit::DeleteColumn(col))),
        ];
        let mut placed = Vec::new();
        for (name, rect, glyph, tip, edit) in buttons {
            // click and drag: else the text box under the button would take the drag (and the cursor)
            let resp = ui.interact(rect, self.id.with(("table-button", ordinal, name)), egui::Sense::CLICK | egui::Sense::DRAG);
            let fill = if edit.is_some() && resp.hovered() { theme::BG_ACTIVE } else { theme::BG_SECONDARY };
            let colour = match (edit.is_some(), glyph == "+") {
                (false, _) => theme::FAINT,
                (true, true) => theme::TEXT,
                (true, false) => theme::LINK_MISSING,
            };
            let painter = ui.painter();
            painter.rect(rect, 3.0, fill, Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, glyph, egui::FontId::proportional(13.0), colour);
            resp.clone().on_hover_text(tip);
            if (resp.is_pointer_button_down_on() || resp.clicked()) && self.focus_before_press {
                // a click on a button must not take the keyboard from the text
                ui.ctx().memory_mut(|m| m.request_focus(self.id));
            }
            if let (true, Some(e)) = (resp.clicked(), edit) {
                self.table_edit = Some((ordinal, Change::Edit(e)));
                ui.ctx().request_repaint();
            }
            placed.push((name.to_string(), rect));
        }
        // the table's own mode (EDIT-030): top right, above the button that adds a column
        let rect = button(side, grid.top() - TABLE_BTN - 2.0);
        let resp = ui.interact(rect, self.id.with(("table-button", ordinal, "mode")), egui::Sense::CLICK | egui::Sense::DRAG);
        let painter = ui.painter();
        painter.rect(rect, 3.0, if resp.hovered() { theme::BG_ACTIVE } else { theme::BG_SECONDARY }, Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Inside);
        if cells {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, "md", egui::FontId::proportional(9.0), theme::TEXT);
        } else {
            // a small grid
            let r = rect.shrink(4.0);
            let line = Stroke::new(1.0, theme::TEXT);
            painter.rect_stroke(r, 0.0, line, egui::StrokeKind::Inside);
            painter.line_segment([Pos2::new(r.center().x, r.top()), Pos2::new(r.center().x, r.bottom())], line);
            painter.line_segment([Pos2::new(r.left(), r.center().y), Pos2::new(r.right(), r.center().y)], line);
        }
        resp.clone().on_hover_text(if cells { "Edit the table as raw markdown" } else { "Edit the cells in the grid" });
        if (resp.is_pointer_button_down_on() || resp.clicked()) && self.focus_before_press {
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
        }
        if resp.clicked() {
            self.table_edit = Some((ordinal, Change::Meta(crate::table::Meta { cells: Some(!cells), sizes: meta.sizes.clone() })));
            ui.ctx().request_repaint();
        }
        placed.push(("mode".to_string(), rect));
        ui.ctx().data_mut(|d| d.insert_temp(key, placed));
    }

    /// Show the editor. Returns (text changed, action).
    pub fn show(&mut self, ui: &mut Ui, text: &mut String, vault: &Vault, note: Note, fonts: &EditorFonts, ctrl_to_follow: bool) -> (bool, Option<EditorAction>) {
        let Note { rel, undo } = note;
        // the size of what is not text of a line: pictures' captions, tables, the line numbers
        let base_size = if self.live_preview { fonts.rendered.size } else { fonts.raw.size };
        let mut action = None;
        let mut changed = false;
        // the footnotes with a definition are marks in the grids of tables too (EDIT-050)
        let _defined = markdown::DefinedFootnotes::set(markdown::find_footnotes(text).defined());
        let focused = ui.ctx().memory(|m| m.has_focus(self.id));
        if focused || !ui.input(|i| i.pointer.any_down()) {
            self.focus_before_press = focused;
        }
        if let Some((n, change)) = self.table_edit.take() {
            changed |= self.apply_table_edit(ui, text, undo.of(rel), n, &change, true);
        }
        // a toolbar command goes to the cell being edited (EDIT-030); else to the note below
        if let Some(n) = self.grid.cell.as_ref().map(|c| c.table) {
            if let Some(cmd) = self.format_request.take() {
                self.grid.format(ui.ctx(), self.id, n, cmd);
            }
        }
        for (n, change, step) in std::mem::take(&mut self.grid.pending) {
            changed |= self.apply_table_edit(ui, text, undo.of(rel), n, &change, step);
        }
        if let Some((before, give_back)) = self.grid.ended.take() {
            self.end_cell_edit(ui, text, undo.of(rel), before, give_back);
        }
        self.refresh_links(text, vault, rel);
        if let Some(cmd) = self.format_request.take() {
            if self.apply_format(ui, text, cmd, undo.of(rel)) {
                changed = true;
                self.refresh_links(text, vault, rel);
            }
        }

        // the selection a toolbar command left: the text box drops it while it has no keyboard
        if let Some(((a, b), left)) = self.keep_selection {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(a), CCursor::new(b))));
            state.store(ui.ctx(), self.id);
            let focused = ui.ctx().memory(|m| m.has_focus(self.id));
            self.keep_selection = (left > 1 && !(focused && left < 3)).then_some(((a, b), left - 1));
        }

        // Keys for the `[[` list and the `[!` picker must be taken before the text box sees them.
        self.assist.id = self.id;
        let mut keys = self.assist.take_keys(ui, text, vault);
        changed |= keys.changed;
        if keys.action.is_some() {
            action = keys.action.take();
        }
        let media_handled = keys.handled;
        if let Some(a) = self.paste_media(ui, vault) {
            action = Some(a);
        }
        if let Some(a) = self.hold_paste_without_h1(ui) {
            action = Some(a);
        }
        if let Some(t) = self.pending_paste.take() {
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
            ui.input_mut(|i| i.events.push(egui::Event::Paste(t)));
        }

        if self.vim_enabled && self.vim_frame(ui, text, undo.of(rel)) {
            changed = true;
            self.refresh_links(text, vault, rel);
        }

        // Smart lists and headings: Enter continues a list, Tab / Shift+Tab indent or change levels,
        // Alt+Up / Alt+Down move lines.
        if !media_handled && !keys.picker_open && !keys.picking && ui.ctx().memory(|m| m.has_focus(self.id)) {
            if let Some(range) = egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()) {
                let a = markdown::char_to_byte(text, range.primary.index.0);
                let b = markdown::char_to_byte(text, range.secondary.index.0);
                let sel = a.min(b)..a.max(b);
                let (alt_up, alt_down) = ui.input_mut(|i| (i.consume_key(Modifiers::ALT, Key::ArrowUp), i.consume_key(Modifiers::ALT, Key::ArrowDown)));
                let (enter, tab, shift_tab) = ui.input(|i| {
                    (
                        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none())),
                        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if modifiers.is_none())),
                        i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if modifiers.shift_only())),
                    )
                });
                let mut result: Option<(String, std::ops::Range<usize>)> = None;
                if alt_up || alt_down {
                    result = crate::lists::move_lines(text, sel.clone(), alt_up);
                } else if enter && sel.is_empty() {
                    if let Some((t, c)) = crate::lists::continue_list(text, sel.start) {
                        ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                        result = Some((t, c..c));
                    }
                } else if tab || shift_tab {
                    if let Some(r) = crate::lists::indent(text, sel.clone(), shift_tab) {
                        ui.input_mut(|i| {
                            i.consume_key(Modifiers::NONE, Key::Tab);
                            i.consume_key(Modifiers::SHIFT, Key::Tab);
                        });
                        result = Some(r);
                    }
                }
                if let Some((t, r)) = result {
                    let before = std::mem::replace(text, t);
                    changed = true;
                    let cs = markdown::byte_to_char(text, r.start);
                    let ce = markdown::byte_to_char(text, r.end);
                    // a line move, a list item or an indent is a step of its own for Ctrl+Z
                    self.record_edit(ui.ctx(), &before, text, CCursorRange::two(CCursor::new(cs), CCursor::new(ce)));
                    self.refresh_links(text, vault, rel);
                }
            }
        }

        // the folds of this frame; a fold command first (EDIT-073): one that hides the cursor's line puts the cursor on the header
        let mut fold = self.fold.begin(rel, text, self.properties_summary);
        let mut had_cmd = false;
        if let Some(cmd) = self.fold.request.take() {
            had_cmd = true;
            let line = self.cursor_line(ui, text);
            if let Some(header) = self.fold.apply(rel, &fold, cmd, line) {
                let at = markdown::line_start_byte(text, header);
                let end = text[at..].find('\n').map_or(text.len(), |p| at + p);
                self.pending_cursor = Some(markdown::byte_to_char(text, end));
            }
            fold = self.fold.begin(rel, text, self.properties_summary);
            ui.ctx().request_repaint();
        }

        if let Some(c) = self.pending_cursor {
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), self.id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            state.store(ui.ctx(), self.id);
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
            // a note opened just now may not have the keyboard in the first frame: ask again
            self.focus_retry = 3;
        } else if self.request_focus {
            self.restore_selection(ui.ctx(), text);
            ui.ctx().memory_mut(|m| m.request_focus(self.id));
            self.focus_retry = 3;
        } else if self.focus_retry > 0 {
            if ui.ctx().memory(|m| m.has_focus(self.id)) {
                self.focus_retry = 0;
            } else {
                self.restore_selection(ui.ctx(), text);
                ui.ctx().memory_mut(|m| m.request_focus(self.id));
                self.focus_retry -= 1;
            }
        }
        self.request_focus = false;

        // Inline embeds: pictures, drawings and PDFs are shown in place of their link unless the
        // cursor is inside the link.
        let mut images: Vec<(Range<usize>, f32, Art, Vec2)> = Vec::new();
        // the caption under each shown embed (EDIT-053) is part of its row
        let caption_h = crate::caption::height(base_size);
        // line numbers (EDIT-018): a gutter wide enough for the last number, left of the text
        let number_font = egui::FontId::monospace((base_size * 0.8).round());
        let gutter = if self.line_numbers {
            let digits = (text.matches('\n').count() + 1).to_string().len().max(2);
            ui.painter().layout_no_wrap("0".repeat(digits), number_font.clone(), theme::FAINT).size().x + GUTTER_GAP
        } else {
            0.0
        };
        // the widest a shown embed can be (where the text can go)
        let max_w = (ui.available_width() - gutter - 8.0).max(80.0);
        self.column = Some(max_w);
        // the raw lines of the markdown preview (EDIT-016, EDIT-017); embeds on them are raw too
        let preview = self.live_preview.then(|| self.preview_lines(ui, text));
        // the tables away from the cursor are drawn as grids (EDIT-024); their text is hidden
        let mut tables: Vec<Shown> = Vec::new();
        if let Some(raw) = &preview {
            let width = (ui.available_width() - gutter - 8.0 - TABLE_SIDE).max(120.0);
            for (ordinal, b) in crate::table::find_tables(text).into_iter().enumerate() {
                if lines_overlap(&b.lines, raw) || fold.view.hides_any(&b.lines) {
                    continue;
                }
                let src = text[crate::table::byte_range(text, &b.lines)].to_string();
                let sizes = self.grid.sizes(ordinal, &b.meta.sizes);
                let laid = crate::table::layout(ui.ctx(), &b.table, sizes, base_size, width, &|l| vault.exists(&l.target, Some(rel)));
                let cells = b.meta.cells.unwrap_or(self.table_cell_edit);
                tables.push(Shown { height: laid.size().y + TABLE_GAP, src, laid, cells, block: b });
            }
        }
        let table_keys: Vec<(String, f32)> = tables.iter().map(|t| (t.src.clone(), t.height)).collect();
        if self.inline_images {
            let focused = ui.ctx().memory(|m| m.has_focus(self.id));
            let sel = egui::text_edit::TextEditState::load(ui.ctx(), self.id).and_then(|st| st.cursor.char_range()).map(|r| {
                let a = markdown::char_to_byte(text, r.primary.index.0);
                let b = markdown::char_to_byte(text, r.secondary.index.0);
                a.min(b)..a.max(b)
            });
            for (l, ok) in &self.cache.links {
                if !l.embed || !*ok {
                    continue;
                }
                let inside = focused && sel.as_ref().map(|s| s.start <= l.range.end && s.end >= l.range.start).unwrap_or(false);
                let line = crate::editor_preview::line_of(text, l.range.start);
                let raw_line = preview.as_ref().is_some_and(|r| r.contains(&line));
                if inside || raw_line || fold.view.is_hidden(line) {
                    continue;
                }
                let Some(p) = vault.resolve(&l.target, Some(rel)) else { continue };
                let abs = vault.abs(&p);
                // the width the link asks for, or the one a drag is giving it right now
                let asked = self.embeds.dragging(l.range.start).or(l.embed_width());
                if crate::media::is_image(&p) {
                    if self.media.notice(&abs).is_some() {
                        let size = crate::embed_size::not_downloaded(max_w);
                        images.push((l.range.clone(), size.y + 6.0 + caption_h, Art::Download(abs.clone()), size));
                        continue;
                    }
                    let Some(tex) = crate::media::texture(ui.ctx(), &abs) else { continue };
                    let size = crate::embed_size::picture(tex.size_vec2(), asked, max_w);
                    images.push((l.range.clone(), size.y + 6.0 + caption_h, Art::Tex(tex), size));
                } else if crate::drawing::excalidraw_md::is_drawing_path(&p) {
                    let Some(scene) = crate::reader::scene_cached(ui.ctx(), &abs) else { continue };
                    let Some(b) = scene.bounds() else { continue };
                    let b = b.expand(20.0);
                    let size = crate::embed_size::drawing(Vec2::new(b.width(), b.height()), max_w);
                    images.push((l.range.clone(), size.y + 6.0 + caption_h, Art::Scene(scene, p.clone()), size));
                } else if crate::media::is_media(&p) {
                    let size = crate::media::widget::card_size(&abs, max_w, asked);
                    images.push((l.range.clone(), size.y + 6.0 + caption_h, Art::Audio(abs.clone()), size));
                } else if crate::pdf::view::is_pdf(&p) {
                    match crate::pdf::view::pdf_pages(ui.ctx(), &abs, 1) {
                        crate::pdf::view::PdfState::Ready { pages, total } if !pages.is_empty() => {
                            let size = crate::embed_size::pdf_page(pages[0].size_vec2(), max_w);
                            images.push((l.range.clone(), size.y + 6.0 + caption_h, Art::Pdf(pages[0].clone(), total), size));
                        }
                        crate::pdf::view::PdfState::Loading => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
                        _ => {}
                    }
                }
            }
        }
        // The embeds shown as pictures, by their link text (in order). When typing changes the
        // text in the middle of a frame, the pictures are found again in the new text, so they
        // stay pictures instead of flashing back to link text for a frame.
        let image_keys: Vec<(String, f32)> = images.iter().map(|(r, h, _, _)| (text[r.clone()].to_string(), *h)).collect();
        let links = self.cache.links.clone();
        let links_text = text.clone();
        let numbered_default = self.numbered_headings;
        let number_marks = std::cell::RefCell::new(Vec::new());
        let formula_marks = std::cell::RefCell::new(Vec::new());
        self.assist.arm(text, media_handled);
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| -> Arc<Galley> {
            let s = buf.as_str();
            // The cached links belong to the text from the start of the frame; if the text box
            // changed it this frame, find them again (cheap for normal notes).
            let fresh;
            let use_links: &[(LinkSpan, bool)] = if s == links_text {
                &links
            } else {
                fresh = markdown::find_links(s)
                    .into_iter()
                    .map(|l| {
                        let ok = vault.exists(&l.target, Some(rel));
                        (l, ok)
                    })
                    .collect::<Vec<_>>();
                &fresh
            };
            let mut ranges: Vec<(Range<usize>, f32)> = remap_images(s, &image_keys).into_iter().map(|(r, h, _)| (r, h)).collect();
            // the lines a fold hides (EDIT-071): rows of no height
            let folds = fold.at(s);
            if let Some(raw) = &preview {
                ranges.extend(table_rows(s, &table_keys, raw, &folds.view).into_iter().flat_map(|g| g.rows));
            }
            ranges.extend(folds.view.byte_ranges(s).into_iter().map(|r| (r, 0.0)));
            // a folded front matter: its first line hidden, the summary is painted over its row
            ranges.extend(folds.veiled(s).map(|r| (r, -1.0)));
            let (mut job, formulas) = highlight_with_math(s, use_links, fonts, &ranges, preview.clone());
            *formula_marks.borrow_mut() = formulas;
            *number_marks.borrow_mut() = make_room_for_numbers(ui, &mut job, fonts, numbered_default, preview.as_ref());
            job.wrap.max_width = wrap_width;
            ui.ctx().fonts_mut(|f| f.layout_job(job))
        };

        // the boxes of the code blocks go behind the text: their place in the layer is taken now, filled below
        let box_slot = (preview.is_some() && fold.blocks.iter().any(|b| b.kind == crate::fold::FoldKind::Code)).then(|| ui.painter().add(Shape::Noop));
        let edit = egui::TextEdit::multiline(text)
            .id(self.id)
            .frame(egui::Frame::NONE)
            .desired_width(f32::INFINITY)
            .desired_rows(20)
            .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
            .layouter(&mut layouter);
        // egui's pipe is not drawn: the one cursor is painted below, in the chosen shape (EDIT-039)
        let pipe = std::mem::replace(&mut ui.visuals_mut().text_cursor.stroke, Stroke::NONE);
        let output = if gutter > 0.0 {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.add_space(gutter);
                edit.show(ui)
            })
            .inner
        } else {
            edit.show(ui)
        };
        ui.visuals_mut().text_cursor.stroke = pipe;
        let resp = &output.response.response;
        if resp.changed() {
            changed = true;
        }

        self.last_layout = Some((output.galley.clone(), output.galley_pos));
        let shown_folds = fold.at(&output.galley.job.text);
        let hidden_bytes = shown_folds.view.byte_ranges(&output.galley.job.text);
        paint_numbers(ui, &number_marks.borrow(), &output.galley, output.galley_pos, &hidden_bytes);
        paint_math(ui, &formula_marks.borrow(), &output.galley, output.galley_pos, resp);
        if let Some(r) = output.cursor_range.filter(|_| resp.has_focus() && self.keep_selection.is_none()) {
            self.last_selection = Some((r.primary.index.0, r.secondary.index.0));
        }
        // paint the inline pictures over their (invisible) link text
        if !images.is_empty() {
            let painter = ui.painter().with_clip_rect(ui.clip_rect());
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            let now_text = output.galley.job.text.clone();
            // sizes the user chose this frame: (link range, width in points)
            let mut resized: Vec<(Range<usize>, u32)> = Vec::new();
            for (r, _, idx) in remap_images(&now_text, &image_keys) {
                let (_, _, art, size) = &images[idx];
                let id = self.id.with(("media-card", idx));
                let c = output.galley.pos_from_cursor(CCursor::new(markdown::byte_to_char(&now_text, r.start)));
                let min = output.galley_pos + Vec2::new(c.min.x, c.min.y + 3.0);
                let rect = Rect::from_min_size(min, *size);
                match art {
                    Art::Tex(tex) => {
                        painter.image(tex.id(), rect, uv, Color32::WHITE);
                        let shown = crate::media::embed::Shown { rect, at: r.start, aspect: size.y / size.x, min_w: crate::media::embed::MIN_WIDTH, max_w, buttons: rect };
                        let did = self.embeds.controls(ui, id, self.media.opacity(), &shown);
                        if let Some(w) = did.resized {
                            resized.push((r.clone(), w.round() as u32));
                        }
                        if did.natural {
                            resized.push((r.clone(), tex.size_vec2().x.round() as u32));
                        }
                        if did.fullscreen {
                            self.embeds.full = Some(crate::media::embed::Full::Image(tex.clone()));
                        }
                    }
                    Art::Scene(scene, at) => {
                        let bg = if self.dark_canvas { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE };
                        painter.rect_filled(rect, 6.0, bg);
                        let pics = crate::drawing::pictures::textures(ui.ctx(), scene, vault, at);
                        crate::drawing::paint_thumbnail(&painter, scene, rect.shrink(4.0), self.dark_canvas, &pics);
                    }
                    Art::Audio(abs) => {
                        crate::media::widget::paint(ui, rect, &self.media, abs, rel, id);
                        if crate::media::is_video(abs) {
                            let shown = crate::media::embed::Shown { rect, at: r.start, aspect: size.y / size.x, min_w: crate::media::widget::VIDEO_MIN_WIDTH, max_w, buttons: crate::media::widget::video_art_rect(rect) };
                            let did = self.embeds.controls(ui, id.with("embed"), self.media.opacity(), &shown);
                            if let Some(w) = did.resized {
                                resized.push((r.clone(), w.round() as u32));
                            }
                            if did.natural {
                                let natural = self.media.view(ui.ctx(), abs).video_size.map_or(crate::media::widget::VIDEO_MAX_WIDTH, |(w, _)| crate::media::widget::video_natural_width(w));
                                resized.push((r.clone(), natural.round() as u32));
                            }
                            if did.fullscreen {
                                self.embeds.full = Some(crate::media::embed::Full::Video(abs.clone()));
                            }
                        }
                    }
                    Art::Download(abs) => {
                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| crate::media::widget::show_not_downloaded(ui, &self.media, abs));
                    }
                    Art::Pdf(tex, total) => {
                        painter.image(tex.id(), rect, uv, Color32::WHITE);
                        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::BORDER), egui::StrokeKind::Outside);
                        let label = if *total > 1 { format!("PDF · page 1 of {total}") } else { "PDF".to_string() };
                        let g = painter.layout_no_wrap(label, egui::FontId::proportional(11.0), Color32::WHITE);
                        let tag = Rect::from_min_size(rect.min + Vec2::new(6.0, 6.0), g.size() + Vec2::new(10.0, 4.0));
                        painter.rect_filled(tag, 4.0, Color32::from_black_alpha(170));
                        painter.galley(tag.min + Vec2::new(5.0, 2.0), g, Color32::WHITE);
                    }
                }
                // the link as the caption below the picture; a click on it follows the link (EDIT-053)
                if let Some(link) = markdown::find_links(&now_text[r.clone()]).into_iter().next() {
                    if let Some(a) = Self::paint_caption(ui, &link, rect, (base_size, max_w), id.with("caption")) {
                        action = Some(a);
                    }
                }
            }
            // the size goes into the link (`![[pic.png|300]]`, EDIT-052)
            for (range, w) in resized.into_iter().rev() {
                if text.get(range.clone()) == now_text.get(range.clone()) {
                    let link = markdown::embed_with_width(&text[range.clone()], w);
                    text.replace_range(range, &link);
                    changed = true;
                }
            }
        }
        // paint the tables over their hidden text
        self.grid.begin_frame();
        if let Some(raw) = preview.as_ref().filter(|_| !tables.is_empty()) {
            let painter = ui.painter().with_clip_rect(ui.clip_rect());
            let now_text = output.galley.job.text.clone();
            for g in table_rows(&now_text, &table_keys, raw, &shown_folds.view) {
                let c = output.galley.pos_from_cursor(CCursor::new(markdown::byte_to_char(&now_text, g.rows[0].0.start)));
                let origin = output.galley_pos + Vec2::new(c.min.x, c.min.y + 1.0);
                let t = &tables[g.key];
                crate::table::paint(&painter, origin, &t.laid);
                let shown = crate::table_grid::Shown { id: self.id, ordinal: g.ordinal, origin, laid: &t.laid, table: &t.block.table, meta: &t.block.meta, cells: t.cells };
                self.grid.show(ui, &shown);
                self.table_buttons(ui, origin, &t.laid, g.ordinal, &t.block.meta, t.cells);
            }
        }
        self.grid.end_frame();
        self.embeds.show_full(self.id, ui.ctx(), &self.media, rel);
        if gutter > 0.0 {
            let now_text = &output.galley.job.text;
            let cursor_line = output.cursor_range.filter(|_| resp.has_focus()).map(|r| {
                let b = markdown::char_to_byte(now_text, r.primary.index.0);
                now_text[..b].matches('\n').count() + 1
            });
            let painter = ui.painter().with_clip_rect(ui.clip_rect());
            for (n, at) in gutter_numbers(&output.galley) {
                let colour = if Some(n) == cursor_line { theme::MUTED } else { theme::FAINT };
                let g = painter.layout_no_wrap(n.to_string(), number_font.clone(), colour);
                let ascent = g.rows.first().and_then(|r| r.row.glyphs.first()).map(|gl| gl.pos.y).unwrap_or(g.size().y * 0.8);
                let y = match at {
                    NumberAt::Baseline(y) => y - ascent,
                    NumberAt::Top(y) => y + 3.0,
                };
                let x = output.galley_pos.x - GUTTER_GAP - g.size().x;
                painter.galley(Pos2::new(x, output.galley_pos.y + y), g, colour);
            }
        }
        // when the cursor moved into or out of an image link, lay out again next frame
        if self.inline_images || self.live_preview {
            let now = output.cursor_range.map(|r| (r.primary.index.0, r.secondary.index.0));
            if now != self.last_cursor {
                self.last_cursor = now;
                ui.ctx().request_repaint();
            }
        }
        // the one cursor, in the shape of the mode (EDIT-039, EDIT-040), blinking (EDIT-041)
        self.vim_cursor_rect = None;
        let normal = self.vim_enabled && self.vim.mode == crate::vim::Mode::Normal;
        let at = if normal { Some(self.vim.cursor) } else { output.cursor_range.map(|r| r.primary.index.0) };
        if let Some(c) = at.filter(|_| resp.has_focus()) {
            let here = output.galley.pos_from_cursor(CCursor::new(c));
            let next = output.galley.pos_from_cursor(CCursor::new(c + 1));
            let w = if (next.min.y - here.min.y).abs() < 1.0 && next.min.x > here.min.x { next.min.x - here.min.x } else { base_size * 0.55 };
            let ch = Rect::from_min_size(output.galley_pos + here.min.to_vec2(), Vec2::new(w, here.height()));
            let now = ui.input(|i| i.time);
            let moved = match self.cursor_seen {
                Some((seen, t)) if seen == (c, normal) && !resp.changed() => t,
                _ => now,
            };
            self.cursor_seen = Some(((c, normal), moved));
            let cur = ui.visuals().text_cursor.clone();
            let (shown, next_toggle) = crate::cursor_style::blink_phase(self.cursor_style.blink && cur.blink, now - moved, cur.on_duration, cur.off_duration);
            if shown {
                let shape = if normal { self.cursor_style.normal_shape } else { self.cursor_style.shape };
                shape.paint(ui.painter(), ch);
            }
            if let Some(t) = next_toggle {
                ui.ctx().request_repaint_after_secs(t as f32);
            }
            if normal {
                self.vim_cursor_rect = Some(ch);
            }
        } else {
            self.cursor_seen = None;
        }
        if let Some(c) = self.pending_cursor.take().or(self.vim_scroll.take()) {
            let r = output.galley.pos_from_cursor(CCursor::new(c));
            let screen = r.translate(output.galley_pos.to_vec2());
            if std::mem::take(&mut self.scroll_to_top) {
                ui.scroll_to_rect(screen, Some(egui::Align::TOP));
            } else {
                ui.scroll_to_rect(screen.expand(60.0), Some(egui::Align::Center));
            }
        }

        // the cursor never stays in text a fold hides (EDIT-075): what hides it is unfolded, and the
        // next frame scrolls to the cursor
        if let Some(r) = output.cursor_range.filter(|_| resp.has_focus()) {
            let now = &output.galley.job.text;
            let (p, q) = (markdown::char_to_byte(now, r.primary.index.0), markdown::char_to_byte(now, r.secondary.index.0));
            if self.fold.reveal(rel, &fold, now, p, p.min(q)..p.max(q)) {
                self.vim_scroll = Some(r.primary.index.0);
                ui.ctx().request_repaint();
            }
        }
        // the front matter and the cursor (EDIT-081)
        let typed = !had_cmd && ui.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Key { key: Key::Backspace | Key::Delete | Key::Enter, pressed: true, .. })));
        let cursor_line = resp.has_focus().then(|| self.cursor_line(ui, &output.galley.job.text));
        self.fold.follow_cursor(rel, &fold, cursor_line, typed);
        // the boxes of the code blocks (EDIT-080)
        if let Some(slot) = box_slot {
            let shapes = crate::code_box::shapes(ui, &shown_folds, &output.galley, output.galley_pos, resp.rect.right(), base_size);
            ui.painter().set(slot, Shape::Vec(shapes));
        }
        // the markers and the badges of the folds (EDIT-071, EDIT-074)
        self.fold.paint(ui, &fold, &crate::fold_ui::Painted { id: self.id, rel, galley: &output.galley, pos: output.galley_pos, area: resp.rect, base: base_size });

        if let Some(line) = self.fold.goto.take() {
            let now = &output.galley.job.text;
            self.pending_cursor = Some(markdown::byte_to_char(now, markdown::line_start_byte(now, line)));
        }

        // --- hover / click on links and checkboxes
        let hover = ui.input(|i| i.pointer.hover_pos());
        let modifiers = ui.input(|i| i.modifiers);
        let follow_ok = !ctrl_to_follow || modifiers.command;
        if let Some(hp) = hover.filter(|p| resp.rect.contains(*p)) {
            if let Some(byte) = char_under(&output.galley, output.galley_pos, hp).map(|c| markdown::char_to_byte(text, c)) {
                let (link, web) = self.link_at(text, byte, &|r| images.iter().any(|(i, ..)| i == r));
                if let Some(w) = &web {
                    if follow_ok {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let hint = if ctrl_to_follow { "Ctrl+click to open in your browser" } else { "Click to open in your browser · Ctrl+click to edit" };
                    resp.clone().on_hover_ui_at_pointer(|ui| {
                        ui.label(egui::RichText::new(&w.url).color(theme::LINK));
                        ui.label(egui::RichText::new(hint).small().color(theme::MUTED));
                    });
                }
                if let Some((l, exists)) = &link {
                    if follow_ok {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    let tip = if *exists { format!("Open “{}”", l.target) } else { format!("Create note “{}”", l.target) };
                    let hint = if ctrl_to_follow { "Ctrl+click to follow" } else { "Ctrl+click to edit the link text" };
                    let colour = if *exists { theme::LINK } else { theme::LINK_MISSING };
                    let image = vault.resolve(&l.target, Some(rel)).filter(|p| crate::media::is_image(p)).map(|p| vault.abs(&p));
                    // several files share the name, or a red path has several candidates (LINK-008, LINK-012)
                    let note = vault.link_hint(&l.target, Some(rel));
                    resp.clone().on_hover_ui_at_pointer(|ui| {
                        if let Some(img) = image.as_ref().and_then(|p| crate::media::image_widget(ui.ctx(), p, 320.0, 240.0)) {
                            ui.add(img);
                        }
                        ui.label(egui::RichText::new(tip).color(colour));
                        if let Some(n) = &note {
                            ui.label(egui::RichText::new(n).small().color(theme::TEXT));
                        }
                        ui.label(egui::RichText::new(hint).small().color(theme::MUTED));
                    });
                }
                if resp.clicked() && (link.is_some() || web.is_some()) {
                    // Plain click follows (unless the setting asks for Ctrl); with the default
                    // setting, Ctrl+click just places the cursor so the link can be edited.
                    let follow = if ctrl_to_follow { modifiers.command } else { !modifiers.command };
                    if follow {
                        action = Self::follow_action(link.as_ref(), web.as_ref(), modifiers.shift || ui.input(|i| i.pointer.middle_down()));
                    }
                } else if resp.clicked() {
                    if let Some(t) = markdown::find_tasks(text).into_iter().find(|t| byte + 1 >= t.check_offset && byte <= t.check_offset + 1) {
                        if markdown::toggle_task_at(text, t.check_offset) {
                            changed = true;
                        }
                    }
                }
            }
        }
        // `gt` in vim normal mode: the click's action for the link under vim's cursor (EDIT-023)
        if std::mem::take(&mut self.vim_follow) {
            let byte = markdown::char_to_byte(text, self.vim.cursor);
            let (link, web) = self.link_at(text, byte, &|r| images.iter().any(|(i, ..)| i == r));
            if let Some(a) = Self::follow_action(link.as_ref(), web.as_ref(), false) {
                action = Some(a);
            }
        }
        if resp.middle_clicked() {
            if let Some(hp) = hover {
                if let Some(byte) = char_under(&output.galley, output.galley_pos, hp).map(|c| markdown::char_to_byte(text, c)) {
                    if let Some((l, _)) = self.cache.links.iter().find(|(l, _)| l.range.contains(&byte)) {
                        action = Some(EditorAction::OpenLink { target: l.target.clone(), heading: l.heading.clone(), new_tab: true });
                    }
                }
            }
        }

        let (c, a) = self.assist.after(ui, text, vault, &output, self.typing(), media_handled);
        changed |= c;
        if a.is_some() {
            action = a;
        }
        (changed, action)
    }
}

/// A table drawn as a grid this frame (EDIT-024): its source text, the height reserved for it, its
/// layout, whether a click edits a cell (EDIT-030) and the table.
struct Shown {
    src: String,
    height: f32,
    laid: crate::table::Laid,
    cells: bool,
    block: crate::table::Block,
}

/// Room right of a grid for its buttons (EDIT-025).
const TABLE_SIDE: f32 = 3.0 * TABLE_BTN + 14.0;
/// Side of a button of a grid.
const TABLE_BTN: f32 = 16.0;

/// Space below a table drawn as a grid (EDIT-024).
const TABLE_GAP: f32 = 8.0;

/// Whether the lines of a table and the raw lines (EDIT-017) share a line.
fn lines_overlap(a: &Range<usize>, b: &Range<usize>) -> bool {
    a.start < b.end && b.start < a.end
}

/// A line of a table drawn as a grid: its byte range and the height of its row.
type TableRow = (Range<usize>, f32);

/// A table drawn as a grid: which of the keys it is, its number among the tables of the note and
/// the rows of its lines.
struct GridTable {
    key: usize,
    ordinal: usize,
    rows: Vec<TableRow>,
}

/// The tables of `text` that are drawn as grids: those away from the raw lines whose text is one
/// of `keys` (the source text of a table and the height reserved for its grid). For each: the
/// index into `keys` and the byte range and row height of each of its lines. The first line gets
/// the whole height (less one point for every other line, which are one point tall), so the
/// grid fits where the hidden text is; the line breaks belong to their lines, so no row is
/// taller than that (EDIT-024).
fn table_rows(text: &str, keys: &[(String, f32)], raw: &Range<usize>, folds: &crate::fold::View) -> Vec<GridTable> {
    if keys.is_empty() {
        return vec![];
    }
    let all = markdown::lines_with_offsets(text);
    let mut out = Vec::new();
    for (ordinal, b) in crate::table::find_tables(text).into_iter().enumerate() {
        if lines_overlap(&b.lines, raw) || folds.hides_any(&b.lines) {
            continue;
        }
        let whole = crate::table::byte_range(text, &b.lines);
        let Some(idx) = keys.iter().position(|(k, _)| *k == text[whole.clone()]) else { continue };
        let n = b.lines.len();
        let rows = b
            .lines
            .clone()
            .map(|l| {
                let start = all[l].0;
                let end = all.get(l + 1).map_or(text.len(), |(o, _)| *o);
                (start..end, if l == b.lines.start { (keys[idx].1 - (n - 1) as f32).max(1.0) } else { 1.0 })
            })
            .collect();
        out.push(GridTable { key: idx, ordinal, rows });
    }
    out
}

/// Space between the line numbers and the text (EDIT-018).
const GUTTER_GAP: f32 = 14.0;

/// Where a line number sits on its row, relative to the galley.
#[derive(Debug, Clone, Copy, PartialEq)]
enum NumberAt {
    /// On the baseline of the row's text.
    Baseline(f32),
    /// At the top of a row without visible text (an embed shown as a picture).
    Top(f32),
}

/// The line numbers of a laid-out note (EDIT-018): one per line of the file, on the first row of
/// that line (rows that come from wrapping get none).
fn gutter_numbers(galley: &Galley) -> Vec<(usize, NumberAt)> {
    let mut out = Vec::new();
    let mut starts_line = true;
    let mut line = 0;
    for row in &galley.rows {
        if starts_line {
            line += 1;
            // the hidden lines of a table drawn as a grid are one point tall: no number
            if row.rect().height() < 2.5 {
                starts_line = row.ends_with_newline;
                continue;
            }
            let top = row.pos.y;
            // hidden text (embeds, hidden markup) is laid out in a tiny font: not a baseline
            let baseline = row.row.glyphs.iter().filter(|g| g.font_height > 2.0).map(|g| g.pos.y).reduce(f32::max);
            let at = match baseline {
                Some(b) => NumberAt::Baseline(top + b),
                None if row.row.glyphs.is_empty() => NumberAt::Baseline(top + row.rect().height() * 0.78),
                None => NumberAt::Top(top),
            };
            out.push((line, at));
        }
        starts_line = row.ends_with_newline;
    }
    out
}

/// Find the embeds that are shown as pictures again in (possibly edited) text: walks the embed
/// links of `text` in order and matches them with `keys` (link text, row height) in order.
/// Returns (byte range in `text`, row height, index into `keys`).
fn remap_images(text: &str, keys: &[(String, f32)]) -> Vec<(Range<usize>, f32, usize)> {
    if keys.is_empty() {
        return vec![];
    }
    let mut out = Vec::new();
    let mut j = 0;
    for l in markdown::find_links(text).into_iter().filter(|l| l.embed) {
        if j >= keys.len() {
            break;
        }
        let here = &text[l.range.clone()];
        // skip keys whose link was removed by the edit
        if let Some(k) = keys[j..].iter().position(|(t, _)| t == here) {
            j += k;
            out.push((l.range.clone(), keys[j].1, j));
            j += 1;
        }
    }
    out
}

/// The character index whose glyph is under `p`, if the pointer is really over text.
pub fn char_under(galley: &Galley, galley_pos: Pos2, p: Pos2) -> Option<usize> {
    let local = p - galley_pos;
    let c = galley.cursor_from_pos(local);
    let idx = c.index.0;
    for k in [idx, idx.saturating_sub(1)] {
        let a = galley.pos_from_cursor(CCursor::new(k));
        let b = galley.pos_from_cursor(CCursor::new(k + 1));
        if (a.min.y - b.min.y).abs() > 0.5 {
            continue;
        }
        let r = Rect::from_min_max(Pos2::new(a.min.x.min(b.min.x), a.min.y), Pos2::new(a.min.x.max(b.min.x), a.max.y));
        if r.expand2(Vec2::new(0.5, 0.0)).contains(local.to_pos2()) {
            return Some(k);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, PointerButton, RawInput};

    struct Harness {
        ctx: egui::Context,
        time: f64,
        texts: Vec<String>,
        /// where each text of the last frame was painted (to click on it)
        rects: Vec<(String, Rect)>,
    }

    /// The texts painted in the last frame (to see whether line numbers were drawn).
    fn painted_texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    /// The painted texts of the last frame with their screen rectangles.
    fn painted_rects(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Rect)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, Rect)>) {
            match s {
                egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = Vec::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    impl Harness {
        fn frame(&mut self, ed: &mut NoteEditor, text: &mut String, vault: &Vault, rel: &Path, events: Vec<Event>) -> (bool, Option<EditorAction>) {
            self.time += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(self.time), events, ..Default::default() };
            let mut changed = false;
            let mut action = None;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    // another focusable widget, like the real app's title box and sidebars
                    let mut other = String::from("title");
                    ui.add(egui::TextEdit::singleline(&mut other).id(egui::Id::unique("other-widget")));
                    let (c, a) = ed.show(ui, text, vault, Note { rel, undo: &mut crate::undo::Histories::default() }, &EditorFonts::uniform(15.0), false);
                    let mut other2 = String::from("after");
                    ui.add(egui::TextEdit::singleline(&mut other2).id(egui::Id::unique("other-widget-2")));
                    changed |= c;
                    if a.is_some() {
                        action = a;
                    }
                });
            });
            out.textures_delta.clear();
            self.texts = painted_texts(&out.shapes);
            self.rects = painted_rects(&out.shapes);
            (changed, action)
        }

        /// Press and release the mouse on the text painted last frame as `label` (two frames).
        fn click_text(&mut self, ed: &mut NoteEditor, text: &mut String, vault: &Vault, rel: &Path, label: &str) -> (bool, Option<EditorAction>) {
            let p = self.rects.iter().find(|(t, _)| t == label).unwrap_or_else(|| panic!("“{label}” is not on screen: {:?}", self.texts)).1.center();
            self.frame(ed, text, vault, rel, click_events(p, true));
            self.frame(ed, text, vault, rel, click_events(p, false))
        }
    }

    fn click_events(p: Pos2, pressed: bool) -> Vec<Event> {
        vec![Event::PointerMoved(p), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }]
    }

    fn vault_for_tests(tag: &str) -> (Vault, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-ed-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        crate::vault::write_sample_vault(&d).unwrap();
        (Vault::open(&d).unwrap(), d)
    }

    fn click_word(h: &mut Harness, ed: &mut NoteEditor, text: &mut String, v: &Vault, rel: &Path, word: &str) -> Option<EditorAction> {
        let (galley, pos) = ed.last_layout.clone().unwrap();
        let ch = markdown::byte_to_char(text, text.find(word).unwrap()) + 1;
        let p = pos + galley.pos_from_cursor(CCursor::new(ch)).center().to_vec2() + Vec2::new(3.0, 0.0);
        h.frame(ed, text, v, rel, click_events(p, true));
        let (_, a) = h.frame(ed, text, v, rel, click_events(p, false));
        a
    }

    /// Run one frame of the editor alone; returns what it copied and whether it asked for a paste.
    fn vim_frame(ctx: &egui::Context, t: &mut f64, ed: &mut NoteEditor, text: &mut String, v: &Vault, undo: &mut crate::undo::Histories, events: Vec<Event>) -> (Option<String>, bool) {
        *t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(*t), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ed.show(ui, text, v, Note { rel: Path::new("Welcome.md"), undo }, &EditorFonts::uniform(15.0), false);
            });
        });
        out.textures_delta.clear();
        let copied = out.platform_output.commands.iter().find_map(|c| match c {
            egui::OutputCommand::CopyText(s) => Some(s.clone()),
            _ => None,
        });
        let paste = out.viewport_output.values().any(|vo| vo.commands.iter().any(|c| matches!(c, egui::ViewportCommand::RequestPaste)));
        (copied, paste)
    }

    // spec: EDIT-010, EDIT-011, EDIT-012, EDIT-013, EDIT-014
    #[test]
    fn vim_lite_in_the_real_editor() {
        let (v, d) = vault_for_tests("vim");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut undo = crate::undo::Histories::default();
        let mut ed = NoteEditor::new(egui::Id::unique("vim-ed"));
        ed.vim_enabled = true;
        ed.esc_cursor = crate::vim::EscCursor::StepBack;
        let mut text = String::from("alpha beta\ngamma delta\nsee [[Ide");
        let cursor = |ctx: &egui::Context, ed: &NoteEditor| egui::text_edit::TextEditState::load(ctx, ed.id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0).unwrap();
        let typed = |s: &str| vec![Event::Text(s.into())];
        let key = |k: Key| vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }];
        ed.pending_cursor = Some(0);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        assert_eq!(ed.vim_status().as_deref(), Some("NORMAL"), "a note that gets the keyboard is in normal mode");
        // motions move the text box's cursor and type nothing
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("w"));
        assert_eq!(cursor(&ctx, &ed), 6);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("j"));
        assert_eq!(cursor(&ctx, &ed), 17, "same column on the next line");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::ArrowUp));
        assert_eq!(cursor(&ctx, &ed), 6, "arrow keys are motions too");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::Enter));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::Backspace));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("q z"));
        assert_eq!(text, "alpha beta\ngamma delta\nsee [[Ide", "no key types in normal mode");
        assert_eq!(cursor(&ctx, &ed), 16, "Enter = j, Backspace = h");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("l"));
        // i: typing; Esc: back to normal on the typed character
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("i"));
        assert_eq!(ed.vim_status().as_deref(), Some("INSERT"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("X"));
        assert_eq!(text, "alpha beta\ngamma Xdelta\nsee [[Ide");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::Escape));
        assert_eq!((ed.vim_status().as_deref(), cursor(&ctx, &ed)), (Some("NORMAL"), 17));
        // visual + d: the selection goes to the clipboard
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("0"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("v"));
        assert_eq!(ed.vim_status().as_deref(), Some("VISUAL"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("w"));
        let (copied, _) = vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("d"));
        assert_eq!(copied.as_deref(), Some("gamma X"));
        assert_eq!(text, "alpha beta\ndelta\nsee [[Ide");
        // p asks the window for the clipboard and puts what arrives after the cursor
        let (_, asked) = vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("p"));
        assert!(asked, "p sends RequestPaste");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![Event::Paste("ZZ".into())]);
        assert_eq!(text, "alpha beta\ndZZelta\nsee [[Ide");
        // :line and /search
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed(":3"));
        assert_eq!(ed.vim_status().as_deref(), Some(":3"), "the command shows while it is typed");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::Enter));
        assert_eq!(cursor(&ctx, &ed), 19, "start of line 3 (\"see [[Ide\")");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("/BETA"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::Enter));
        assert_eq!(cursor(&ctx, &ed), 6);
        // the [[ suggestions stay closed in normal mode and open while typing
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("G$"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        assert!(!ed.assist.ac_active, "no suggestions in normal mode, although the cursor is in [[Ide");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("A"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        assert!(ed.assist.ac_active, "insert mode: the suggestions open");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// One frame of the editor alone in vim mode; returns the action it asked for.
    fn vim_action_frame(ctx: &egui::Context, t: &mut f64, ed: &mut NoteEditor, text: &mut String, v: &Vault, undo: &mut crate::undo::Histories, events: Vec<Event>) -> Option<EditorAction> {
        *t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(*t), events, ..Default::default() };
        let mut action = None;
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                action = ed.show(ui, text, v, Note { rel: Path::new("Welcome.md"), undo }, &EditorFonts::uniform(15.0), false).1;
            });
        });
        out.textures_delta.clear();
        action
    }

    // spec: EDIT-023
    #[test]
    fn vim_gt_follows_the_link_under_the_cursor_like_a_click() {
        let (v, d) = vault_for_tests("vim-gt");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut undo = crate::undo::Histories::default();
        let mut ed = NoteEditor::new(egui::Id::unique("vim-gt"));
        ed.vim_enabled = true;
        let mut text = String::from("plain [[Ideas#Top]] and [site](https://example.org/x) or https://bare.example\n\ngg here");
        let typed = |s: &str| vec![Event::Text(s.into())];
        // the text box's cursor, as a click would leave it
        let put_cursor = |ctx: &egui::Context, ed: &NoteEditor, c: usize| {
            let mut st = egui::text_edit::TextEditState::load(ctx, ed.id).unwrap_or_default();
            st.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(c))));
            st.store(ctx, ed.id);
        };
        let at = |text: &str, w: &str| markdown::byte_to_char(text, text.find(w).unwrap()) + 1;
        ed.pending_cursor = Some(0);
        vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        // a wiki link: the same action a click gives, heading included
        put_cursor(&ctx, &ed, at(&text, "Ideas"));
        match vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("gt")) {
            Some(EditorAction::OpenLink { target, heading, new_tab }) => assert_eq!((target.as_str(), heading.as_deref(), new_tab), ("Ideas", Some("Top"), false)),
            _ => panic!("gt on a wiki link did not open it"),
        }
        // the markdown link and the bare URL
        put_cursor(&ctx, &ed, at(&text, "site"));
        match vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("gt")) {
            Some(EditorAction::OpenUrl(u)) => assert_eq!(u, "https://example.org/x"),
            _ => panic!("gt on a web link did not open it"),
        }
        put_cursor(&ctx, &ed, at(&text, "bare"));
        match vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("gt")) {
            Some(EditorAction::OpenUrl(u)) => assert_eq!(u, "https://bare.example"),
            _ => panic!("gt on a bare URL did not open it"),
        }
        // off a link: nothing happens, no `g` is left waiting, and gg still goes to the top
        put_cursor(&ctx, &ed, at(&text, "plain"));
        assert!(vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("gt")).is_none());
        assert_eq!(ed.vim_status().as_deref(), Some("NORMAL"));
        put_cursor(&ctx, &ed, at(&text, "here"));
        assert!(vim_action_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("gg")).is_none());
        assert_eq!(ed.vim.cursor, 0, "gg still goes to the first line");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-020, EDIT-021
    #[test]
    fn dd_u_and_ctrl_r_in_the_real_editor() {
        let (v, d) = vault_for_tests("vim-undo");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut undo = crate::undo::Histories::default();
        let mut ed = NoteEditor::new(egui::Id::unique("vim-undo-ed"));
        ed.vim_enabled = true;
        let mut text = String::from("one\ntwo\nthree");
        let cursor = |ctx: &egui::Context, ed: &NoteEditor| egui::text_edit::TextEditState::load(ctx, ed.id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0).unwrap();
        let typed = |s: &str| vec![Event::Text(s.into())];
        let key = |k: Key, modifiers: Modifiers| vec![Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers }];
        ed.pending_cursor = Some(0);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, vec![]);
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("j"));
        // dd: the line goes to the clipboard, the cursor to the line that took its place
        let (copied, _) = vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("dd"));
        assert_eq!((text.as_str(), copied.as_deref(), cursor(&ctx, &ed)), ("one\nthree", Some("two\n"), 4));
        // u brings it back, Ctrl+R deletes it again
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("u"));
        assert_eq!((text.as_str(), cursor(&ctx, &ed)), ("one\ntwo\nthree", 4));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::R, Modifiers::CTRL));
        assert_eq!(text, "one\nthree", "Ctrl+R redoes and types nothing");
        // an insert session, with everything typed in it, is one step
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("j"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("A"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("X"));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("Y"));
        assert_eq!(text, "one\nthreeXY");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, key(Key::Escape, Modifiers::NONE));
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("u"));
        assert_eq!(text, "one\nthree", "the whole session is undone at once");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("u"));
        assert_eq!(text, "one\ntwo\nthree");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("u"));
        assert_eq!(ed.vim_status().as_deref(), Some("Already at oldest change"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-022
    #[test]
    fn u_still_undoes_after_the_app_was_restarted() {
        let (v, d) = vault_for_tests("vim-undo-restart");
        let typed = |s: &str| vec![Event::Text(s.into())];
        let start = |root: &Path, now: u64| {
            let mut undo = crate::undo::Histories::default();
            undo.open_vault(root, now);
            let mut ed = NoteEditor::new(egui::Id::unique("vim-restart"));
            ed.vim_enabled = true;
            ed.pending_cursor = Some(0);
            (undo, ed, egui::Context::default())
        };
        let now = 1_800_000_000;
        let (mut undo, mut ed, ctx) = start(&d, now);
        let mut text = String::from("one\ntwo\nthree");
        let mut t = 0.0;
        for events in [vec![], vec![], typed("j"), typed("dd"), typed("x")] {
            vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, events);
        }
        assert_eq!(text, "one\nhree");
        // the app quits (the note is saved by its own path) and starts again
        undo.flush_all();
        let (mut undo, mut ed, ctx) = start(&d, now + 3600);
        let mut t = 0.0;
        for events in [vec![], vec![], typed("u")] {
            vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, events);
        }
        assert_eq!(text, "one\nthree", "the x of the first session is undone");
        vim_frame(&ctx, &mut t, &mut ed, &mut text, &v, &mut undo, typed("u"));
        assert_eq!(text, "one\ntwo\nthree", "and the dd");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-021
    #[test]
    fn two_editors_of_the_same_note_share_the_history() {
        let (v, d) = vault_for_tests("vim-undo-tabs");
        let mut undo = crate::undo::Histories::default();
        let mut text = String::from("one two");
        let typed = |s: &str| vec![Event::Text(s.into())];
        let (ctx1, ctx2) = (egui::Context::default(), egui::Context::default());
        let (mut t1, mut t2) = (0.0, 0.0);
        let (mut ed1, mut ed2) = (NoteEditor::new(egui::Id::unique("tab1")), NoteEditor::new(egui::Id::unique("tab2")));
        for ed in [&mut ed1, &mut ed2] {
            ed.vim_enabled = true;
            ed.pending_cursor = Some(0);
        }
        for _ in 0..2 {
            vim_frame(&ctx1, &mut t1, &mut ed1, &mut text, &v, &mut undo, vec![]);
            vim_frame(&ctx2, &mut t2, &mut ed2, &mut text, &v, &mut undo, vec![]);
        }
        vim_frame(&ctx1, &mut t1, &mut ed1, &mut text, &v, &mut undo, typed("x"));
        assert_eq!(text, "ne two");
        // the other tab undoes what this one did, then this one redoes it
        vim_frame(&ctx2, &mut t2, &mut ed2, &mut text, &v, &mut undo, typed("u"));
        assert_eq!(text, "one two");
        vim_frame(&ctx1, &mut t1, &mut ed1, &mut text, &v, &mut undo, vec![Event::Key { key: Key::R, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL }]);
        assert_eq!(text, "ne two");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-005
    #[test]
    fn clicking_links_in_the_editor() {
        let (v, d) = vault_for_tests("click");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("Go to [[Ideas]] or make [[Brand new]] now");
        let mut ed = NoteEditor::new(egui::Id::unique("t"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "Ideas") {
            Some(EditorAction::OpenLink { target, .. }) => assert_eq!(target, "Ideas"),
            _ => panic!("clicking a blue link did nothing"),
        }
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "Brand new") {
            Some(EditorAction::OpenLink { target, .. }) => assert_eq!(target, "Brand new"),
            _ => panic!("clicking a red link did nothing"),
        }
        // clicking plain text does not follow anything
        assert!(click_word(&mut h, &mut ed, &mut text, &v, rel, "make").is_none());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-018
    #[test]
    fn edit_018_one_number_per_file_line_on_its_first_row() {
        let (v, d) = vault_for_tests("gutter");
        let rel = Path::new("Welcome.md");
        let long = "a long line that wraps ".repeat(30);
        let mut text = format!("# Title\n{long}\n\nlast");
        let mut ed = NoteEditor::new(egui::Id::unique("gutter-rows"));
        ed.line_numbers = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (galley, _) = ed.last_layout.clone().unwrap();
        assert!(galley.rows.len() > 5, "the long line wraps");
        let numbers = gutter_numbers(&galley);
        assert_eq!(numbers.iter().map(|(n, _)| *n).collect::<Vec<_>>(), vec![1, 2, 3, 4], "one number per line of the file");
        // each number sits on the first row of its line (the empty line too)
        let starts = [0, "# Title\n".len(), "# Title\n".len() + long.len() + 1, text.chars().count() - 4];
        for ((n, at), start) in numbers.iter().zip(starts) {
            let row = galley.pos_from_cursor(CCursor::new(start));
            let NumberAt::Baseline(y) = at else { panic!("line {n} has text") };
            assert!(*y > row.min.y && *y <= row.max.y + 0.5, "line {n}: baseline {y} in row {row:?}");
        }
        for n in ["1", "2", "3", "4"] {
            assert!(h.texts.iter().any(|t| t == n), "number {n} is painted: {:?}", h.texts);
        }
        assert!(!h.texts.iter().any(|t| t == "5"), "wrapped rows get no number");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-018
    #[test]
    fn edit_018_the_gutter_moves_the_text_and_numbers_go_away_when_off() {
        let (v, d) = vault_for_tests("gutter-off");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("one\ntwo\nthree");
        let mut ed = NoteEditor::new(egui::Id::unique("gutter-off"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        ed.line_numbers = true;
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let on_x = ed.last_layout.as_ref().unwrap().1.x;
        assert!(h.texts.iter().any(|t| t == "3"));
        ed.line_numbers = false;
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let off_x = ed.last_layout.as_ref().unwrap().1.x;
        assert!(on_x > off_x + 10.0, "the gutter is left of the text: {on_x} vs {off_x}");
        assert!(!h.texts.iter().any(|t| ["1", "2", "3"].contains(&t.as_str())), "no numbers when off: {:?}", h.texts);
        // typing still goes into the text with the gutter shown
        ed.line_numbers = true;
        ed.pending_cursor = Some(text.chars().count());
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("!".into())]);
        assert_eq!(text, "one\ntwo\nthree!");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-007
    #[test]
    fn clicking_a_checkbox_in_the_editor_ticks_it() {
        let (v, d) = vault_for_tests("check");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("- [ ] buy milk");
        let mut ed = NoteEditor::new(egui::Id::unique("t2"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (galley, pos) = ed.last_layout.clone().unwrap();
        let p = pos + galley.pos_from_cursor(CCursor::new(3)).center().to_vec2() + Vec2::new(2.0, 0.0);
        h.frame(&mut ed, &mut text, &v, rel, click_events(p, true));
        h.frame(&mut ed, &mut text, &v, rel, click_events(p, false));
        assert_eq!(text, "- [x] buy milk");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-005
    #[test]
    fn enter_and_tab_keep_lists_going() {
        let (v, d) = vault_for_tests("lists");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t4"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key, modifiers: Modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("1. [ ] buy milk".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(text, "1. [ ] buy milk\n2. [ ] ");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::NONE)]);
        assert_eq!(text, "1. [ ] buy milk\n    1. [ ] ");
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("oat".into())]);
        assert_eq!(text, "1. [ ] buy milk\n    1. [ ] oat");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::SHIFT)]);
        assert_eq!(text, "1. [ ] buy milk\n2. [ ] oat");
        // empty item + Enter ends the list
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter, Modifiers::NONE)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!(text, "1. [ ] buy milk\n2. [ ] oat\n");
        // headings: Tab adds a level, Shift+Tab removes one
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("# Title".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::NONE)]);
        assert!(text.ends_with("\n## Title"));
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab, Modifiers::SHIFT)]);
        assert!(text.ends_with("\n# Title"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-005
    #[test]
    fn alt_arrows_move_lines() {
        let (v, d) = vault_for_tests("move");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t6"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key, modifiers: Modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("- a\n    - b".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowUp, Modifiers::ALT)]);
        assert_eq!(text, "- b\n- a");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowDown, Modifiers::ALT)]);
        assert_eq!(text, "- a\n- b");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn file_picker_completes_paths_and_links_the_file() {
        let (v, d) = vault_for_tests("picker");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t7"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("pic: [!/Pro".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        assert_eq!(text, "pic: [!/Projects/");
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("G".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        assert_eq!(text, "pic: [!/Projects/Garden plan.md");
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (_, a) = h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
        assert_eq!(text, "pic: [[Projects/Garden plan.md]]");
        assert!(matches!(a, Some(EditorAction::FilesChanged)));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn file_picker_arrows_and_tab_keep_working() {
        let (v, d) = vault_for_tests("picker2");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t8"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.media.items.len() > 2, "items: {:?}", ed.assist.media.items);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowDown)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 1, "arrow down should move the selection");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowUp)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 0);
        // Tab with several matches and nothing in common: the highlighted entry goes into the path,
        // the picker stays open and the editor keeps focus
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[!/Projects/");
        assert!(ed.assist.media.active, "picker should stay open after Tab");
        assert!(h.ctx.memory(|m| m.has_focus(ed.id)), "editor should keep focus after Tab");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn file_picker_ctrl_j_k_and_click() {
        let (v, d) = vault_for_tests("picker-ctrl");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("picker-ctrl"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let ctrl = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.media.items.len() > 2, "items: {:?}", ed.assist.media.items);
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::J)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::J)]);
        assert_eq!(ed.assist.media.sel, 2, "Ctrl+J moves down");
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::K)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 1, "Ctrl+K moves up");
        assert_eq!(text, "[!/");
        // a click on a folder opens it in the path, like Enter
        let folder = ed.assist.media.items.iter().find(|e| e.is_dir).expect("a folder in the vault").name.clone();
        h.click_text(&mut ed, &mut text, &v, rel, &format!("▸ {folder}/"));
        assert_eq!(text, format!("[!/{folder}/"));
        assert!(h.ctx.memory(|m| m.has_focus(ed.id)), "the editor keeps the keyboard");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Where the first embed of "intro\n![[…]]" is drawn.
    fn embed_rect(ed: &NoteEditor, size: Vec2) -> Rect {
        let (galley, pos) = ed.last_layout.clone().unwrap();
        let c = galley.pos_from_cursor(CCursor::new(6));
        Rect::from_min_size(pos + Vec2::new(c.min.x, c.min.y + 3.0), size)
    }

    fn press(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE }
    }

    // spec: EDIT-053
    #[test]
    fn a_shown_embed_has_its_link_as_a_caption_below_it_and_a_click_on_it_follows_the_link() {
        let (v, d) = vault_for_tests("caption");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        image::RgbaImage::from_pixel(200, 120, image::Rgba([30, 30, 200, 255])).save(d.join("other.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[pic.png]]\n![[other.png|Sea at dusk|200]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("caption"));
        ed.inline_images = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let find = |h: &Harness, t: &str| h.rects.iter().find(|(s, _)| s == t).map(|(_, r)| *r);
        let pic = find(&h, "pic.png").expect("the link target is the caption");
        let other = find(&h, "Sea at dusk").expect("the alias words are the caption");
        assert!(other.top() > pic.bottom() + 100.0, "the second picture comes below the first one and its caption");
        // the caption is below its picture, which is 3 points under the row's top
        let (galley, pos) = ed.last_layout.clone().unwrap();
        let top = pos.y + galley.pos_from_cursor(CCursor::new(6)).min.y + 3.0;
        assert!(pic.top() >= top + 120.0, "below the picture: {} vs {}", pic.top(), top);
        // a click on the caption opens the file instead of showing the link text
        let at = pic.center();
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(at)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(at, true)]);
        let (_, action) = h.frame(&mut ed, &mut text, &v, rel, vec![press(at, false)]);
        assert!(matches!(action, Some(EditorAction::OpenLink { ref target, .. }) if target == "pic.png"), "the caption opens the target");
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(find(&h, "pic.png").is_some(), "the click did not put the cursor into the link");
        // with the cursor in the link there is no picture and no caption: the link text is shown
        ed.pending_cursor = Some(9);
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        assert!(find(&h, "pic.png").is_none(), "no caption while the link is edited");
        // a picture that cannot be shown keeps its link text, without a caption
        let mut broken = String::from("![[missing.png]]");
        let mut ed2 = NoteEditor::new(egui::Id::unique("caption2"));
        ed2.inline_images = true;
        for _ in 0..3 {
            h.frame(&mut ed2, &mut broken, &v, rel, vec![]);
        }
        assert!(find(&h, "missing.png").is_none() && h.texts.iter().any(|t| t.contains("![[missing.png]]")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-052
    #[test]
    fn dragging_a_corner_sets_the_width_in_the_link_and_the_buttons_do_their_work() {
        let (v, d) = vault_for_tests("resize");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[pic.png]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("resize"));
        ed.inline_images = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let r = embed_rect(&ed, Vec2::new(200.0, 120.0));
        // the bottom right corner, dragged 100 points to the right
        let corner = r.max - Vec2::splat(3.0);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(corner)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(corner, true)]);
        for i in 1..=5 {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(corner + Vec2::new(20.0 * i as f32, 0.0))]);
        }
        assert_eq!(text, "intro\n![[pic.png]]\nafter", "the link is written when the mouse is let go");
        let (changed, _) = h.frame(&mut ed, &mut text, &v, rel, vec![press(corner + Vec2::new(100.0, 0.0), false)]);
        assert!(changed, "the editor reports the edit");
        let width: u32 = text.strip_prefix("intro\n![[pic.png|").and_then(|t| t.split_once("]]")).map(|(w, _)| w.parse().unwrap()).expect(&text);
        assert!((280..=310).contains(&width), "dragged to about 300: {width}");
        assert!(text.ends_with("]]\nafter"));
        // a width in the link is the size shown
        for _ in 0..2 {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(Pos2::new(800.0, 650.0))]);
        }
        let shown = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(shown > 120.0 * 1.4, "the row is taller for the bigger picture: {shown}");
        // the 100 % button: natural size
        let r = embed_rect(&ed, Vec2::new(width as f32, width as f32 * 0.6));
        let fullscreen = Pos2::new(r.right() - 27.0, r.top() + 27.0);
        let hundred = fullscreen - Vec2::new(42.0, 0.0);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(hundred)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(hundred, true)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(hundred, false)]);
        assert_eq!(text, "intro\n![[pic.png|200]]\nafter");
        // fullscreen: opens, and Escape closes it
        for _ in 0..2 {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(Pos2::new(800.0, 650.0))]);
        }
        let r = embed_rect(&ed, Vec2::new(200.0, 120.0));
        let fullscreen = Pos2::new(r.right() - 27.0, r.top() + 27.0);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(fullscreen)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(fullscreen, true)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(fullscreen, false)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.embeds.full.is_some(), "the fullscreen button opened the view");
        let esc = Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![esc]);
        assert!(ed.embeds.full.is_none(), "Escape closes it");
        assert_eq!(text, "intro\n![[pic.png|200]]\nafter");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: MEDIA-012
    #[test]
    fn a_video_is_resized_by_its_corner_and_has_the_buttons_too() {
        let (v, d) = vault_for_tests("resize-video");
        std::fs::write(d.join("clip.mp4"), b"not really a video").unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[clip.mp4]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("resize-video"));
        ed.inline_images = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let card = crate::media::widget::card_size(&d.join("clip.mp4"), 880.0, None);
        let r = embed_rect(&ed, card);
        let corner = r.max - Vec2::splat(3.0);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(corner)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(corner, true)]);
        for i in 1..=5 {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(corner - Vec2::new(20.0 * i as f32, 0.0))]);
        }
        h.frame(&mut ed, &mut text, &v, rel, vec![press(corner - Vec2::new(100.0, 0.0), false)]);
        let width: u32 = text.strip_prefix("intro\n![[clip.mp4|").and_then(|t| t.split_once("]]")).map(|(w, _)| w.parse().unwrap()).expect(&text);
        assert!((520..=560).contains(&width), "narrowed from 640 by about 100: {width}");
        // fullscreen: the view opens over the whole window
        for _ in 0..2 {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(Pos2::new(800.0, 650.0))]);
        }
        let r = embed_rect(&ed, crate::media::widget::card_size(&d.join("clip.mp4"), 880.0, Some(width as f32)));
        let art = crate::media::widget::video_art_rect(r);
        let fullscreen = Pos2::new(art.right() - 27.0, art.top() + 27.0);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(fullscreen)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(fullscreen, true)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![press(fullscreen, false)]);
        assert!(matches!(ed.embeds.full, Some(crate::media::embed::Full::Video(_))));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn inline_images_show_until_the_cursor_enters_the_link() {
        let (v, d) = vault_for_tests("inline");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[pic.png]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("t9"));
        ed.inline_images = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let tall = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(tall > 120.0, "image row should make room for the picture, height {tall}");
        // put the cursor inside the link: the text comes back, the row is normal again
        ed.pending_cursor = Some(9);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let short = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(short < 100.0, "with the cursor in the link it should be plain text, height {short}");
        assert_eq!(text, "intro\n![[pic.png]]\nafter");
        // setting off: never a picture
        let mut ed2 = NoteEditor::new(egui::Id::unique("t10"));
        h.frame(&mut ed2, &mut text, &v, rel, vec![]);
        assert!(ed2.last_layout.as_ref().unwrap().0.size().y < 100.0);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-017
    #[test]
    fn edit_017_embeds_on_the_raw_lines_around_the_cursor_show_their_link_text() {
        let (v, d) = vault_for_tests("preview-embed");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n\n![[pic.png]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("preview-embed"));
        ed.inline_images = true;
        ed.live_preview = true;
        ed.preview_raw_lines = 1;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let height = |ed: &NoteEditor| ed.last_layout.as_ref().unwrap().0.size().y;
        let mut settle = |ed: &mut NoteEditor, text: &mut String, cursor: usize| {
            ed.pending_cursor = Some(cursor);
            for _ in 0..3 {
                h.frame(ed, text, &v, rel, vec![]);
            }
        };
        // cursor on line 0: the embed (line 2) is two lines away, it stays a picture
        settle(&mut ed, &mut text, 0);
        assert!(height(&ed) > 120.0, "a picture two lines away, height {}", height(&ed));
        // cursor on the empty line 1, next to the embed: with one raw line around it, link text
        settle(&mut ed, &mut text, 6);
        assert!(height(&ed) < 100.0, "the embed next to the cursor is raw, height {}", height(&ed));
        // the same from the line after it
        let end = text.len() - 1;
        settle(&mut ed, &mut text, end);
        assert!(height(&ed) < 100.0, "the embed before the cursor line is raw, height {}", height(&ed));
        // no raw lines around the cursor: only the cursor's own line counts
        ed.preview_raw_lines = 0;
        settle(&mut ed, &mut text, 6);
        assert!(height(&ed) > 120.0, "with 0 the neighbouring embed is a picture, height {}", height(&ed));
        assert_eq!(text, "intro\n\n![[pic.png]]\nafter");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn escape_closes_the_file_picker() {
        let (v, d) = vault_for_tests("esc");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t11"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.media.active);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.assist.media.active, "Esc should close the picker");
        assert!(!ed.picker_visible(), "the popup should not be drawn");
        assert_eq!(text, "[!/");
        // losing focus for a moment (e.g. clicking elsewhere) doesn't bring it back
        h.ctx.memory_mut(|m| m.surrender_focus(ed.id));
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.ctx.memory_mut(|m| m.request_focus(ed.id));
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.picker_visible(), "a closed picker should stay closed");
        // typing on keeps it closed for this [! ...
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("P".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.picker_visible());
        // ... but a new [! opens it again
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn picker_safety_net_when_keys_reach_the_text_box() {
        // On some desktops the keys arrive as plain typed text instead of key presses.
        // Then a tab or newline lands in the text: the picker must take it back and do its job.
        let (v, d) = vault_for_tests("net");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("start\n");
        let mut ed = NoteEditor::new(egui::Id::unique("t12"));
        ed.pending_cursor = Some(6);
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[!/Pro".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("\t".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[!/Projects/", "a typed tab should complete, not stay in the text");
        assert!(ed.picker_visible());
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("\t".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[!/Projects/Garden plan.md", "the only entry gets completed");
        // a second tab with nothing left to complete changes nothing and doesn't finish the link
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("\t".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[!/Projects/Garden plan.md");
        assert!(ed.picker_visible());
        // Enter picks the file
        let enter = Event::Key { key: Key::Enter, physical_key: Some(Key::Enter), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "start\n[[Projects/Garden plan.md]]");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-002
    #[test]
    fn double_bracket_bang_opens_only_the_file_picker() {
        // `[[!` must not also open the note suggestions (they took the keys away from the picker)
        assert_eq!(link_query("[[!../x", 7), None);
        assert_eq!(link_query("[[!", 3), None);
        let (v, d) = vault_for_tests("bb");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t13"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key| Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        assert!(!ed.assist.ac_active, "note suggestions must stay closed for [[!");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowDown)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.media.sel, 1);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowUp)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Pro".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[[!/Projects/");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Tab)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[[!/Projects/Garden plan.md");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(text, "[[Projects/Garden plan.md]]");
        // Esc closes it
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [[!/".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.picker_visible());
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.picker_visible());
        assert!(!ed.assist.ac_active);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn drawings_and_pdfs_show_inline_like_pictures() {
        let (v, d) = vault_for_tests("inline-art");
        std::fs::write(d.join("doc.pdf"), crate::pdf::view::sample_pdf()).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        crate::theme::install_fonts(&h.ctx);
        for (text, what) in [("intro\n![[Sketch.excalidraw]]\nafter", "drawing"), ("intro\n![[doc.pdf]]\nafter", "pdf")] {
            let mut text = text.to_string();
            let mut ed = NoteEditor::new(egui::Id::unique(("inline-art", what)));
            ed.inline_images = true;
            let mut tall = 0.0;
            for _ in 0..200 {
                h.frame(&mut ed, &mut text, &v, rel, vec![]);
                tall = ed.last_layout.as_ref().unwrap().0.size().y;
                if tall > 120.0 {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10)); // PDFs render in the background
            }
            assert!(tall > 120.0, "{what} is shown inline, height {tall}");
            // with the cursor in the link, the link text is back
            ed.pending_cursor = Some(9);
            for _ in 0..3 {
                h.frame(&mut ed, &mut text, &v, rel, vec![]);
            }
            assert!(ed.last_layout.as_ref().unwrap().0.size().y < 100.0, "{what}: link text while editing it");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn typing_keeps_pictures_in_place() {
        // regression: on every keypress the picture became link text for one frame, so it
        // flickered and the view jumped
        let (v, d) = vault_for_tests("noflicker");
        image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("pic.png")).unwrap();
        let mut v = v;
        v.rescan();
        let rel = Path::new("Welcome.md");
        let mut text = String::from("intro\n![[pic.png]]\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("t-noflicker"));
        ed.inline_images = true;
        ed.pending_cursor = Some(text.chars().count()); // cursor at the end, outside the link
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let tall = ed.last_layout.as_ref().unwrap().0.size().y;
        assert!(tall > 120.0);
        for (i, ch) in ["x", "y", "z"].iter().enumerate() {
            h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(ch.to_string())]);
            let now = ed.last_layout.as_ref().unwrap().0.size().y;
            assert!(now > 120.0, "keypress {i}: the picture must stay a picture in the frame of the keypress (height {now})");
        }
        assert_eq!(text, "intro\n![[pic.png]]\nafterxyz");
        // typing in front of the picture moves it, it still stays a picture
        ed.pending_cursor = Some(0);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Q".into())]);
        assert!(ed.last_layout.as_ref().unwrap().0.size().y > 120.0);
        assert!(text.starts_with("Qintro"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-006
    #[test]
    fn remapping_follows_edits() {
        let keys = vec![("![[a.png]]".to_string(), 50.0), ("![[b.png]]".to_string(), 60.0)];
        let t = "x ![[a.png]] y ![[b.png]]";
        let r = remap_images(t, &keys);
        assert_eq!(r.len(), 2);
        assert_eq!(&t[r[1].0.clone()], "![[b.png]]");
        let t2 = "NEW TEXT x ![[a.png]] y ![[b.png]]";
        let r2 = remap_images(t2, &keys);
        assert_eq!(r2[0].0.start, 11);
        assert_eq!(r2[1].2, 1);
        // an embed removed by the edit is skipped, the rest still match
        let r3 = remap_images("only ![[b.png]]", &keys);
        assert_eq!(r3.len(), 1);
        assert_eq!(r3[0].2, 1);
        // a link that was shown as text (cursor inside) is not a key and stays text
        assert!(remap_images("![[c.png]]", &keys).is_empty());
    }

    // spec: EDIT-001
    #[test]
    fn escape_closes_the_note_suggestions() {
        let (v, d) = vault_for_tests("acesc");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t-acesc"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        let key = |key: Key| Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("see [[Ide".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.ac_active, "suggestions open");
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Escape)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.assist.ac_active, "Esc closes the note suggestions");
        assert_eq!(text, "see [[Ide", "text unchanged");
        // typing on keeps them closed for this [[
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("a".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!ed.assist.ac_active);
        // Enter after Esc is a normal new line, not a pick
        h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::Enter)]);
        assert!(!text.contains("]]"), "nothing was picked: {text:?}");
        // a new [[ opens them again
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" [[We".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(ed.assist.ac_active, "a new [[ opens the suggestions");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-009
    #[test]
    fn clicking_a_web_link_opens_the_browser_action() {
        let (v, d) = vault_for_tests("web");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("see [this is google](https://google.com) ok");
        let mut ed = NoteEditor::new(egui::Id::unique("t5"));
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "google]") {
            Some(EditorAction::OpenUrl(u)) => assert_eq!(u, "https://google.com"),
            _ => panic!("clicking a web link should open it"),
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-001
    #[test]
    fn typing_double_brackets_suggests_and_completes() {
        let (v, d) = vault_for_tests("ac");
        let rel = Path::new("Welcome.md");
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("t3"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("See [[Ide".into())]);
        assert_eq!(text, "See [[Ide");
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        assert_eq!(text, "See [[Ideas]]");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-001
    #[test]
    fn clicking_or_ctrl_j_k_in_the_note_suggestions() {
        let (v, d) = vault_for_tests("ac-click");
        let rel = Path::new("Welcome.md");
        let ctrl = |key: Key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::CTRL };
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("ac-click"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("See [[".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let n = ed.assist.ac_labels.len();
        assert!(n > 2, "several entries: {:?}", ed.assist.ac_labels);
        // Ctrl+J / Ctrl+K move the highlight like ↓ / ↑ and type nothing
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::J)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::J)]);
        assert_eq!(ed.assist.ac_sel, 2);
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::K)]);
        assert_eq!(ed.assist.ac_sel, 1);
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::K)]);
        h.frame(&mut ed, &mut text, &v, rel, vec![ctrl(Key::K)]);
        assert_eq!(ed.assist.ac_sel, n - 1, "Ctrl+K wraps round like ↑");
        assert_eq!(text, "See [[");
        // a click on an entry picks it like Enter, and the editor keeps the keyboard
        let label = ed.assist.ac_labels[1].clone();
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.click_text(&mut ed, &mut text, &v, rel, &label);
        assert_eq!(text, format!("See [[{label}]]"));
        assert!(!ed.assist.ac_active, "the list closes after the pick");
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(h.ctx.memory(|m| m.has_focus(ed.id)), "the editor keeps the keyboard");
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("!".into())]);
        assert_eq!(text, format!("See [[{label}]]!"), "typing goes on after the link");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: PPL-001, EDIT-004
    #[test]
    fn double_brackets_p_lists_people() {
        let (mut v, d) = vault_for_tests("people");
        std::fs::create_dir_all(d.join("Adressbuch")).unwrap();
        std::fs::write(d.join("Adressbuch/Hans Müller.md"), "# Hans Müller\n").unwrap();
        std::fs::write(d.join("Adressbuch/Anna Schmidt.md"), "# Anna\n").unwrap();
        std::fs::write(d.join("Müllabfuhr.md"), "not a person\n").unwrap();
        v.rescan();
        let rel = Path::new("Welcome.md");
        let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("tp"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("Met [[p Mü".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.ac_labels, vec!["Hans Müller".to_string(), "＋ New contact “Mü” in Adressbuch".into()], "only the people folder");
        h.frame(&mut ed, &mut text, &v, rel, vec![enter.clone()]);
        assert_eq!(text, "Met [[Hans Müller]]", "a normal link (the p is only for the list)");
        // a new person: the link plus a request to make the contact note
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("tp2"));
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[[p Jörg Weiß".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        let (_, act) = h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        assert_eq!(text, "[[Jörg Weiß]]");
        assert!(matches!(act, Some(EditorAction::CreatePerson(n)) if n == "Jörg Weiß"));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-016
    #[test]
    fn typing_a_hash_in_a_link_lists_the_headings_of_that_note_numbered_and_filtered_and_picks_the_plain_text() {
        let (mut v, d) = vault_for_tests("heading-completion");
        std::fs::write(d.join("Roadmap.md"), "# Roadmap\n## **Plan** one\n### Step a\n### Step b\n## Review\n```\n## Not a heading\n```\n").unwrap();
        v.rescan();
        let rel = Path::new("Welcome.md");
        let enter = Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
        let list = |typed: &str, text: &mut String| {
            let mut ed = NoteEditor::new(egui::Id::unique("hc"));
            ed.request_focus = true;
            let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
            h.frame(&mut ed, text, &v, rel, vec![]);
            h.frame(&mut ed, text, &v, rel, vec![Event::Text(typed.into())]);
            h.frame(&mut ed, text, &v, rel, vec![]);
            (ed, h)
        };
        let mut text = String::new();
        let (ed, _) = list("[[Roadmap#", &mut text);
        let em = "\u{2003}";
        assert_eq!(ed.assist.ac_labels, vec!["1 Roadmap".to_string(), format!("{em}1.1 Plan one"), format!("{em}{em}1.1.1 Step a"), format!("{em}{em}1.1.2 Step b"), format!("{em}1.2 Review")], "in order, by level, numbered");
        let (ed, _) = list("[[Roadmap#ste", &mut text);
        assert_eq!(ed.assist.ac_labels.len(), 2, "filtered by what is typed after the #: {:?}", ed.assist.ac_labels);
        // picking inserts the plain text of the heading, no number and no markup
        let mut text = String::new();
        let (mut ed, mut h) = list("[[Roadmap#Pl", &mut text);
        h.frame(&mut ed, &mut text, &v, rel, vec![enter.clone()]);
        assert_eq!(text, "[[Roadmap#Plan one]]");
        // [[# lists the headings of the open note (the text being edited)
        let mut text = String::from("## First\n## Second\n");
        let end = text.chars().count();
        let mut ed = NoteEditor::new(egui::Id::unique("hc-own"));
        ed.request_focus = true;
        ed.pending_cursor = Some(end);
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[[#Sec".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.ac_labels, vec![format!("{em}2 Second")]);
        h.frame(&mut ed, &mut text, &v, rel, vec![enter]);
        assert!(text.ends_with("[[#Second]]"), "{text:?}");
        // numbering off: no numbers in the list; a note that does not exist lists nothing
        let mut text = String::new();
        let mut ed = NoteEditor::new(egui::Id::unique("hc-off"));
        ed.assist.numbered_headings = false;
        ed.request_focus = true;
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text("[[Roadmap#Rev".into())]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert_eq!(ed.assist.ac_labels, vec![format!("{em}Review")]);
        let mut text = String::new();
        let (ed, _) = list("[[Nowhere#", &mut text);
        assert!(ed.assist.ac_labels.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-001
    #[test]
    fn link_query_detects_open_brackets() {
        let t = "hello [[Wor";
        assert_eq!(link_query(t, t.chars().count()), Some((8, "Wor".to_string())));
        let t2 = "hello [[World]] x";
        assert_eq!(link_query(t2, t2.chars().count()), None);
        let t3 = "a [[b|alias";
        assert_eq!(link_query(t3, t3.chars().count()), None);
        let t4 = "[[";
        assert_eq!(link_query(t4, 2), Some((2, String::new())));
    }

    // spec: LINK-004
    #[test]
    fn highlight_colours_links() {
        let text = "See [[Yes]] and [[No]]";
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(text)
            .into_iter()
            .map(|l| {
                let ok = l.target == "Yes";
                (l, ok)
            })
            .collect();
        let job = highlight(text, &links, 15.0);
        let color_at = |byte: usize| job.sections.iter().find(|s| s.byte_range.start.0 <= byte && byte < s.byte_range.end.0).unwrap().format.color;
        assert_eq!(color_at(text.find("Yes").unwrap()), theme::LINK);
        assert_eq!(color_at(text.find("No").unwrap()), theme::LINK_MISSING);
        assert_eq!(color_at(0), theme::TEXT);
        // sections cover the whole text without gaps
        let mut pos = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start.0, pos);
            pos = s.byte_range.end.0;
        }
        assert_eq!(pos, text.len());
    }

    // spec: LINK-004
    #[test]
    fn highlight_handles_unicode_and_headings() {
        let text = "# Überschrift\n- [x] erledigt **fett** `code`\n```\n[[nolink]]\n```";
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(text).into_iter().map(|l| (l, true)).collect();
        assert!(links.is_empty());
        let job = highlight(text, &links, 15.0);
        assert!(job.sections[0].format.font_id.size > 20.0);
        let mut pos = 0;
        for s in &job.sections {
            assert_eq!(s.byte_range.start.0, pos);
            assert!(text.is_char_boundary(s.byte_range.end.0));
            pos = s.byte_range.end.0;
        }
    }

    /// The bytes of `text` that the layout makes invisible, as a string of the same length
    /// (`_` hidden, `.` shown).
    fn hidden_map(text: &str, raw: Option<Range<usize>>) -> String {
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(text).into_iter().map(|l| (l, true)).collect();
        let job = highlight_preview(text, &links, &EditorFonts::uniform(15.0), &[], raw);
        assert_eq!(job.text, text, "the galley text is the buffer");
        text.char_indices()
            .map(|(i, c)| {
                let f = &job.sections.iter().find(|s| s.byte_range.start.0 <= i && i < s.byte_range.end.0).unwrap().format;
                let hidden = f.color == Color32::TRANSPARENT;
                if c == '\n' {
                    return "\n".to_string();
                }
                std::iter::repeat_n(if hidden { '_' } else { '.' }, c.len_utf8()).collect::<String>()
            })
            .collect()
    }

    // spec: EDIT-016
    #[test]
    fn preview_hides_the_markup_of_lines_away_from_the_cursor() {
        let text = "# Title\n**bold** and *it* and ~~gone~~ and `code`\nsee [[Ideas|my idea]] or [[Plain]] or [web](http://x.org)\nplain";
        // the cursor is in the last line: the rest is rendered
        let m = hidden_map(text, Some(3..4));
        let lines: Vec<&str> = m.split('\n').collect();
        assert_eq!(lines[0], "__.....", "heading without its # and the space");
        let (it, rest) = ("_.._", ".....");
        assert_eq!(lines[1], format!("__....__{rest}{it}{rest}__....__{rest}_...._"), "bold, italic, strike and code without their markers");
        let (h, v) = (|n: usize| "_".repeat(n), |n: usize| ".".repeat(n));
        let expected = [v(4), h(8), v(7), h(2), v(4), h(2), v(5), h(2), v(4), h(1), v(3), h(15)].concat();
        assert_eq!(lines[2], expected, "links show only their text");
        assert_eq!(lines[3], ".....", "an ordinary line has nothing to hide");
        // no preview: nothing is hidden; the raw lines look exactly like the raw layout
        assert!(!hidden_map(text, None).contains('_'));
        let raw_all = hidden_map(text, Some(0..4));
        assert!(!raw_all.contains('_'));
        let links: Vec<(LinkSpan, bool)> = markdown::find_links(text).into_iter().map(|l| (l, true)).collect();
        let a = highlight_preview(text, &links, &EditorFonts::uniform(15.0), &[], None);
        let b = highlight_preview(text, &links, &EditorFonts::uniform(15.0), &[], Some(0..4));
        assert_eq!(a.sections.len(), b.sections.len());
        assert!(a.sections.iter().zip(&b.sections).all(|(x, y)| x.byte_range == y.byte_range && x.format == y.format));
    }

    // spec: EDIT-016, EDIT-017
    #[test]
    fn preview_keeps_the_cursor_lines_and_the_ones_around_them_raw() {
        let text = "# a\n# b\n# c\n# d\n# e";
        let shown = |raw: Range<usize>| -> Vec<bool> { hidden_map(text, Some(raw)).split('\n').map(|l| !l.contains('_')).collect() };
        assert_eq!(shown(crate::editor_preview::raw_lines(text, Some(8..8), 0)), [false, false, true, false, false]);
        assert_eq!(shown(crate::editor_preview::raw_lines(text, Some(8..8), 1)), [false, true, true, true, false]);
        // a selection from the second to the fourth line
        assert_eq!(shown(crate::editor_preview::raw_lines(text, Some(4..14), 0)), [false, true, true, true, false]);
        // no keyboard: everything rendered
        assert_eq!(shown(crate::editor_preview::raw_lines(text, None, 2)), [false; 5]);
    }

    // spec: EDIT-016
    #[test]
    fn preview_leaves_code_blocks_and_front_matter_alone() {
        let text = "---\ntitle: x\n---\n```\n**not bold** [[x]]\n```\n---";
        let m = hidden_map(text, Some(100..100));
        let lines: Vec<&str> = m.split('\n').collect();
        // the front matter and the code are drawn as they are; only the fences of the code box are hidden (EDIT-080)
        assert!(!lines[..3].concat().contains('_') && !lines[4].contains('_') && !lines[6].contains('_'), "{m}");
        assert!(lines[3].chars().all(|c| c == '_'), "the opening fence is hidden: {m}");
        // with the cursor in the block the fence is raw again
        assert!(!hidden_map(text, Some(4..5)).contains('_'));
    }

    // spec: EDIT-016
    #[test]
    fn preview_links_still_work_and_typing_edits_the_text() {
        let (v, d) = vault_for_tests("preview");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("first line\nGo to [[Ideas|my idea]] now\nlast");
        let mut ed = NoteEditor::new(egui::Id::unique("preview-ed"));
        ed.live_preview = true;
        ed.pending_cursor = Some(0);
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        // line 2 is rendered: "[[Ideas|" has no width, the alias is where the link starts
        let (galley, _) = ed.last_layout.clone().unwrap();
        let start = markdown::byte_to_char(&text, text.find("my idea").unwrap());
        let (a, b) = (galley.pos_from_cursor(CCursor::new(start - 8)), galley.pos_from_cursor(CCursor::new(start)));
        assert!(b.min.x - a.min.x < 12.0, "the hidden markup is narrow: {}", b.min.x - a.min.x);
        match click_word(&mut h, &mut ed, &mut text, &v, rel, "my idea") {
            Some(EditorAction::OpenLink { target, .. }) => assert_eq!(target, "Ideas"),
            _ => panic!("clicking the rendered link did nothing"),
        }
        // typing goes into the buffer, markup and all
        ed.pending_cursor = Some(markdown::byte_to_char(&text, text.len()));
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        h.frame(&mut ed, &mut text, &v, rel, vec![Event::Text(" **x**".into())]);
        assert_eq!(text, "first line\nGo to [[Ideas|my idea]] now\nlast **x**");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-024
    #[test]
    fn a_table_away_from_the_cursor_is_a_grid_and_raw_text_when_the_cursor_is_in_it() {
        let (v, d) = vault_for_tests("preview-table");
        let rel = Path::new("Welcome.md");
        let long = "many words in one cell ".repeat(30);
        let mut text = format!("before\n\n| Where | What |\n|:--|--:|\n| `a.json` | {long}|\n| [[Ideas]] | **b** |\n\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("preview-table"));
        ed.live_preview = true;
        // the raw mode of EDIT-030: a click shows the markdown
        ed.table_cell_edit = false;
        ed.pending_cursor = Some(0);
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        for _ in 0..4 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        // the cursor is in the first line: the table is a grid; its cells are painted on their own
        for cell in ["Where", "What", "a.json", "Ideas", "b"] {
            assert!(h.texts.iter().any(|t| t == cell), "cell {cell} is painted: {:?}", h.texts);
        }
        let hidden = |ed: &NoteEditor, text: &str| {
            let (g, _) = ed.last_layout.clone().unwrap();
            let at = text.find("|:--").unwrap();
            g.job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format.color == Color32::TRANSPARENT
        };
        assert!(hidden(&ed, &text), "the table's own text is invisible");
        let (galley, _) = ed.last_layout.clone().unwrap();
        let start = markdown::byte_to_char(&text, text.find("| Where").unwrap());
        let after = markdown::byte_to_char(&text, text.find("after").unwrap());
        let table_h = galley.pos_from_cursor(CCursor::new(after)).min.y - galley.pos_from_cursor(CCursor::new(start)).min.y;
        assert!(table_h > 100.0, "the long cell wraps, the text box reserves room for the grid: {table_h}");
        assert_eq!(text.matches('\n').count(), 7, "the text is untouched");
        // a click on the grid puts the cursor into the table, which turns into raw text
        h.click_text(&mut ed, &mut text, &v, rel, "What");
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let (galley, _) = ed.last_layout.clone().unwrap();
        let cursor = egui::text_edit::TextEditState::load(&h.ctx, ed.id).and_then(|st| st.cursor.char_range()).unwrap().primary.index.0;
        let lo = markdown::byte_to_char(&text, text.find("| Where").unwrap());
        let hi = markdown::byte_to_char(&text, text.find("\n\nafter").unwrap());
        assert!((lo..=hi).contains(&cursor), "the click landed in the table: {cursor} not in {lo}..{hi}");
        assert!(!h.texts.iter().any(|t| t == "Where"), "no grid cell any more: {:?}", h.texts);
        assert!(!hidden(&ed, &text), "the raw source is shown");
        let raw_h = galley.pos_from_cursor(CCursor::new(after)).min.y - galley.pos_from_cursor(CCursor::new(start)).min.y;
        assert!(raw_h < table_h, "raw lines take their own room");
        // the cursor leaves: the grid is back
        ed.pending_cursor = Some(after);
        for _ in 0..3 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        assert!(h.texts.iter().any(|t| t == "Where") && hidden(&ed, &text), "the grid is back: {:?}", h.texts);
        // the preview off: raw, always
        ed.live_preview = false;
        h.frame(&mut ed, &mut text, &v, rel, vec![]);
        assert!(!h.texts.iter().any(|t| t == "Where") && !hidden(&ed, &text), "the preview off: raw");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-018, EDIT-024
    #[test]
    fn the_hidden_lines_of_a_grid_get_no_line_number() {
        let (v, d) = vault_for_tests("table-gutter");
        let rel = Path::new("Welcome.md");
        let mut text = String::from("before\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\nafter");
        let mut ed = NoteEditor::new(egui::Id::unique("table-gutter"));
        ed.live_preview = true;
        ed.line_numbers = true;
        ed.pending_cursor = Some(0);
        let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
        for _ in 0..4 {
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
        }
        let (galley, _) = ed.last_layout.clone().unwrap();
        let numbers: Vec<usize> = gutter_numbers(&galley).iter().map(|(n, _)| *n).collect();
        assert_eq!(numbers, [1, 2, 3, 6, 7], "the delimiter row and the data row are one point tall: no number");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// An editor with a note, driven through input events, for the buttons of table grids.
    struct Grid {
        ed: NoteEditor,
        text: String,
        undo: crate::undo::Histories,
        ctx: egui::Context,
        v: Vault,
        dir: std::path::PathBuf,
        t: f64,
        /// where each text was painted in the last frame
        painted: Vec<(String, Rect)>,
        /// the buttons shown in the last frame
        buttons: Vec<(String, Rect)>,
        /// whether the text box reported a change in the last frame
        changed: bool,
    }

    const REL: &str = "Welcome.md";

    impl Grid {
        fn new(tag: &str, text: &str, vim: bool) -> Grid {
            let (v, dir) = vault_for_tests(tag);
            let mut ed = NoteEditor::new(egui::Id::unique(("grid", tag)));
            ed.live_preview = true;
            ed.vim_enabled = vim;
            ed.pending_cursor = Some(0);
            let mut g = Grid { ed, text: text.to_string(), undo: Default::default(), ctx: egui::Context::default(), v, dir, t: 0.0, painted: vec![], buttons: vec![], changed: false };
            for _ in 0..4 {
                g.frame(vec![]);
            }
            g
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(self.t), events, ..Default::default() };
            let (ed, text, undo, v) = (&mut self.ed, &mut self.text, &mut self.undo, &self.v);
            let mut changed = false;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    changed = ed.show(ui, text, v, Note { rel: Path::new(REL), undo }, &EditorFonts::uniform(15.0), false).0;
                });
            });
            out.textures_delta.clear();
            self.changed = changed;
            self.painted = painted_rects(&out.shapes);
            self.buttons = self.ctx.data(|d| d.get_temp::<Vec<(String, Rect)>>(self.ed.id.with("table-buttons"))).unwrap_or_default();
        }

        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(vec![]);
            }
        }

        fn centre_of(&self, label: &str) -> Pos2 {
            self.painted.iter().find(|(t, _)| t == label).unwrap_or_else(|| panic!("{label} is not painted")).1.center()
        }

        fn button(&self, name: &str) -> Rect {
            self.buttons.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no button {name}: {:?}", self.buttons.iter().map(|b| &b.0).collect::<Vec<_>>())).1
        }

        /// Move the pointer to `p` and let the buttons register (they are hit-tested a frame late).
        fn hover(&mut self, p: Pos2) {
            self.frame(vec![Event::PointerMoved(p)]);
            self.frames(2);
        }

        /// Hover the cell with this text and click a button of the grid, then let the edit happen.
        fn press_at(&mut self, cell: Pos2, name: &str) {
            self.hover(cell);
            let at = self.button(name).center();
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(click_events(at, true));
            self.frame(click_events(at, false));
            self.frames(2);
        }

        fn key(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers }]);
            self.frames(2);
        }

        fn type_text(&mut self, s: &str) {
            self.frame(vec![Event::Text(s.into())]);
            self.frames(1);
        }

        /// Undo every step of the note's history (the one vim-lite uses) and say how many there were.
        fn history_steps(&mut self) -> usize {
            let h = self.undo.of(Path::new(REL));
            let mut n = 0;
            while h.undo(&mut self.text).is_some() {
                n += 1;
            }
            n
        }

        /// Whether the grid of the first table is drawn (its text is invisible), not raw.
        fn is_grid(&self) -> bool {
            let (g, _) = self.ed.last_layout.clone().unwrap();
            let at = self.text.find("\n|").or_else(|| self.text.find("| A")).unwrap() + 1;
            g.job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format.color == Color32::TRANSPARENT
        }

        fn cursor_char(&self) -> usize {
            egui::text_edit::TextEditState::load(&self.ctx, self.ed.id).and_then(|st| st.cursor.char_range()).unwrap().primary.index.0
        }
    }

    impl Drop for Grid {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const TABLE: &str = "| A | B |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |\n";

    fn note_with(table: &str) -> String {
        format!("before\n\n{table}\nafter")
    }

    // spec: EDIT-025
    #[test]
    fn the_buttons_of_a_grid_edit_the_table_as_one_undoable_step_and_keep_the_grid() {
        let original = note_with(TABLE);
        let mut g = Grid::new("table-buttons", &original, false);
        assert!(g.buttons.is_empty(), "no buttons while the pointer is elsewhere");
        let cell = g.centre_of("3");
        g.hover(cell);
        let names: Vec<&str> = g.buttons.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["add-column", "add-row", "insert-row", "delete-row", "insert-column", "delete-column", "mode"]);
        // the pointer is on the second data row ("3"), in the first column
        g.press_at(cell, "add-column");
        assert_eq!(g.text, original.replace(TABLE, "| A | B |  |\n|:--|--:| --- |\n| 1 | 2 |  |\n| 3 | 4 |  |\n"));
        assert!(g.is_grid(), "the grid stays a grid: the cursor did not enter the table");
        g.press_at(cell, "delete-column");
        assert_eq!(g.text, "before\n\n| B |  |\n|--:| --- |\n| 2 |  |\n| 4 |  |\n\nafter", "the hovered column (the first one) is gone, the rest as written");
        g.press_at(cell, "insert-row");
        assert!(g.text.contains("| 4 |  |\n|   |   |\n"), "a row below the hovered one: {}", g.text);
        g.press_at(cell, "add-row");
        assert!(g.text.contains("|   |   |\n|   |   |\n"), "and one at the end: {}", g.text);
        g.press_at(cell, "delete-row");
        assert!(!g.text.contains("| 4 |  |"), "the hovered row is gone: {}", g.text);
        assert!(g.is_grid() && g.text.starts_with("before\n\n| B |") && g.text.ends_with("\nafter"), "text outside the table is untouched");
        assert_eq!(g.history_steps(), 5, "one step in the note's history for each click");
        assert_eq!(g.text, original);
    }

    const BUTTONS: [&str; 5] = ["add-column", "delete-column", "insert-row", "add-row", "delete-row"];

    // spec: EDIT-025, EDIT-021
    #[test]
    fn ctrl_z_without_vim_undoes_one_button_click_per_press_and_ctrl_y_and_ctrl_shift_z_redo() {
        let original = note_with(TABLE);
        for redo_with in [(Key::Y, Modifiers::COMMAND), (Key::Z, Modifiers::COMMAND | Modifiers::SHIFT)] {
            let mut g = Grid::new("ctrl-z", &original, false);
            let cell = g.centre_of("3");
            let mut texts = vec![g.text.clone()];
            for b in BUTTONS {
                g.press_at(cell, b);
                assert_ne!(&g.text, texts.last().unwrap(), "{b} changed the text");
                texts.push(g.text.clone());
            }
            for i in (0..BUTTONS.len()).rev() {
                g.key(Key::Z, Modifiers::COMMAND);
                assert_eq!(g.text, texts[i], "one Ctrl+Z goes back over {}", BUTTONS[i]);
            }
            g.key(Key::Z, Modifiers::COMMAND);
            assert_eq!(g.text, original, "nothing before the first click to undo");
            for (i, b) in BUTTONS.iter().enumerate() {
                g.key(redo_with.0, redo_with.1);
                assert_eq!(g.text, texts[i + 1], "one redo does {b} again ({:?})", redo_with.0);
            }
            assert!(g.is_grid(), "the grid is still a grid");
        }
    }

    // spec: EDIT-025, EDIT-021
    #[test]
    fn a_button_click_is_its_own_undo_step_between_typing_in_both_modes() {
        let original = note_with(TABLE);
        // without vim: typing, a click and typing again are three steps, all within one second
        let mut g = Grid::new("click-typing", &original, false);
        let cell = g.centre_of("3");
        g.type_text("X");
        assert!(g.text.starts_with("Xbefore"));
        g.press_at(cell, "add-column");
        let with_column = g.text.clone();
        assert!(with_column.contains("| A | B |  |"));
        g.type_text("Y");
        assert!(g.text.starts_with("XYbefore"), "typing goes on at the cursor: {}", g.text);
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, with_column, "the first Ctrl+Z takes back the typing after the click only");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, format!("X{original}"), "the second takes back the click only");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, original, "the third takes back the typing before it");

        // with vim: the same with insert sessions and `u`
        let mut g = Grid::new("click-typing-vim", &original, true);
        let cell = g.centre_of("3");
        for k in ["i", "X"] {
            g.type_text(k);
        }
        g.key(Key::Escape, Modifiers::NONE);
        g.press_at(cell, "add-column");
        assert!(g.text.contains("| A | B |  |") && g.text.starts_with("Xbefore"), "{}", g.text);
        for k in ["i", "Y"] {
            g.type_text(k);
        }
        g.key(Key::Escape, Modifiers::NONE);
        assert!(g.text.contains('Y'));
        g.type_text("u");
        assert!(g.text.contains("| A | B |  |") && !g.text.contains('Y'), "u takes back the typing after the click only: {}", g.text);
        g.type_text("u");
        assert_eq!(g.text, format!("X{original}"), "u takes back the click only");
        g.type_text("u");
        assert_eq!(g.text, original, "u takes back the typing before it");
    }

    // spec: EDIT-025, EDIT-021
    #[test]
    fn line_moves_enter_in_a_list_and_tab_are_their_own_ctrl_z_steps_without_vim() {
        let mut g = Grid::new("program-edits", "one\ntwo\nthree", false);
        g.ed.pending_cursor = Some(1);
        g.frames(2);
        g.key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(g.text, "two\none\nthree");
        g.key(Key::ArrowDown, Modifiers::ALT);
        assert_eq!(g.text, "two\nthree\none");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, "two\none\nthree", "one Ctrl+Z takes back one line move");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, "one\ntwo\nthree");
        g.key(Key::Y, Modifiers::COMMAND);
        assert_eq!(g.text, "two\none\nthree");

        let mut g = Grid::new("program-edits-list", "- item", false);
        g.ed.pending_cursor = Some(6);
        g.frames(2);
        g.type_text("s");
        g.key(Key::Enter, Modifiers::NONE);
        assert_eq!(g.text, "- items\n- ", "Enter goes on with the list");
        g.key(Key::Tab, Modifiers::NONE);
        assert_eq!(g.text, "- items\n    - ", "Tab indents it");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, "- items\n- ", "Ctrl+Z takes back the indent only");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, "- items", "then the new list item only");
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, "- item", "then the typing");
    }

    // spec: EDIT-025
    #[test]
    fn buttons_that_cannot_work_do_nothing_and_leave_text_history_and_grid_alone() {
        // (table, a cell to hover, the greyed button)
        let cases = [(TABLE, "A", "delete-row"), ("| A | B |\n|---|---|\n| 1 | 2 |\n", "1", "delete-row"), ("| A |\n|---|\n| 1 |\n| 2 |\n", "1", "delete-column")];
        for (i, (table, cell, button)) in cases.into_iter().enumerate() {
            let original = note_with(table);
            let mut g = Grid::new(&format!("greyed-{i}"), &original, false);
            let at = g.centre_of(cell);
            g.press_at(at, button);
            assert_eq!(g.text, original, "{button} on {cell} changed nothing");
            assert!(g.ed.table_edit.is_none() && !g.changed);
            assert!(g.is_grid(), "and the click did not put the cursor into the table");
            assert_eq!(g.history_steps(), 0, "and made no step");
            // the buttons that work still work afterwards
            g.press_at(at, "add-row");
            assert_ne!(g.text, original);
        }
    }

    impl Grid {
        /// Click at `p` (the pointer moves there first, so the grid's strips are hit-tested).
        fn click(&mut self, p: Pos2) {
            self.hover(p);
            self.frame(click_events(p, true));
            self.frame(click_events(p, false));
            self.frames(2);
        }

        /// The resize strips of the first grid (EDIT-031).
        fn border(&self, name: &str) -> Rect {
            let all = self.ctx.data(|d| d.get_temp::<Vec<(String, Rect)>>(self.ed.id.with(("table-borders", 0usize)))).unwrap_or_default();
            all.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no border {name}: {all:?}")).1
        }

        /// Drag from `from` by `by`, in a few steps.
        fn drag(&mut self, from: Pos2, by: Vec2) {
            self.hover(from);
            self.frame(click_events(from, true));
            for k in 1..=4 {
                self.frame(vec![Event::PointerMoved(from + by * (k as f32 / 4.0))]);
            }
            self.frame(click_events(from + by, false));
            self.frames(2);
        }

        fn meta(&self) -> crate::table::Meta {
            crate::table::find_tables(&self.text)[0].meta.clone()
        }

        fn editing(&self) -> Option<(usize, usize)> {
            self.ed.grid.cell.as_ref().map(|c| (c.row, c.col))
        }
    }

    // spec: EDIT-030
    #[test]
    fn a_click_on_a_cell_edits_it_in_the_grid_and_the_keys_move_between_cells() {
        let original = note_with(TABLE);
        let mut g = Grid::new("cell-edit", &original, false);
        let three = g.centre_of("3");
        g.click(three);
        assert_eq!(g.editing(), Some((2, 0)), "the cell of 3 is being edited");
        assert!(g.is_grid(), "the table stays a grid");
        g.type_text("x|");
        g.frames(1);
        assert!(g.text.contains("| 3x\\| |"), "what is typed is in the note at once, the pipe escaped: {}", g.text);
        assert!(g.is_grid());
        g.key(Key::Tab, Modifiers::NONE);
        assert_eq!(g.editing(), Some((2, 1)));
        g.type_text("y");
        g.key(Key::Enter, Modifiers::SHIFT);
        assert_eq!(g.editing(), Some((1, 1)), "Shift+Enter: the cell above");
        g.key(Key::Tab, Modifiers::SHIFT);
        assert_eq!(g.editing(), Some((1, 0)));
        g.key(Key::Enter, Modifiers::NONE);
        assert_eq!(g.editing(), Some((2, 0)), "Enter: the cell below");
        g.key(Key::Escape, Modifiers::NONE);
        g.frames(2);
        assert_eq!(g.editing(), None, "Esc leaves");
        assert!(g.ctx.memory(|m| m.has_focus(g.ed.id)), "and the note has the keyboard again");
        assert!(g.is_grid(), "still a grid: the cursor did not enter the table");
        assert_eq!(g.text, note_with("| A | B |\n|:--|--:|\n| 1 | 2 |\n| 3x\\| | 4y |\n"));
        assert_eq!(g.history_steps(), 1, "the whole editing is one step");
        assert_eq!(g.text, original);
    }

    impl Grid {
        /// The cursor in the box of the cell being edited (chars), and whether it selects.
        fn cell_caret(&self) -> (usize, bool) {
            let r = egui::text_edit::TextEditState::load(&self.ctx, crate::table_grid::TableGrid::cell_id(self.ed.id)).and_then(|st| st.cursor.char_range()).unwrap();
            (r.primary.index.0, r.primary != r.secondary)
        }
    }

    // spec: EDIT-030
    #[test]
    fn the_arrow_keys_move_inside_a_cell_and_leave_it_at_the_edge_of_its_text() {
        let table = "| A | B |\n|:--|--:|\n| 1 | 22 |\n| 3 | 4 |\n";
        let mut g = Grid::new("cell-arrows", &note_with(table), false);
        let one = g.centre_of("1");
        g.click(one);
        assert_eq!((g.editing(), g.cell_caret()), (Some((1, 0)), (1, false)), "the cursor is behind the text");
        g.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((1, 1)), 0), "Right at the end: the start of the next cell");
        g.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((1, 1)), 1), "inside the text Right moves the cursor");
        g.key(Key::ArrowRight, Modifiers::NONE);
        g.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((2, 0)), 0), "from the last column to the next row");
        g.key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((1, 1)), 2), "Left at the start: the end of the previous cell, a row back");
        g.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(g.editing(), Some((2, 1)), "Down");
        g.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(g.editing(), Some((2, 1)), "the last row: Down stays");
        g.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!(g.editing(), Some((2, 1)), "the last cell: Right stays");
        g.key(Key::ArrowUp, Modifiers::NONE);
        g.key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((0, 1)), 1), "Up to the header, the cursor at the end");
        g.key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((0, 1)), 0), "the header: Up stays, the cursor goes to the start of the text");
        g.key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((0, 0)), 1), "the end of the first cell");
        g.key(Key::ArrowLeft, Modifiers::NONE);
        g.key(Key::ArrowLeft, Modifiers::NONE);
        assert_eq!((g.editing(), g.cell_caret().0), (Some((0, 0)), 0), "the first cell: Left stays");
        // Shift+arrow selects text inside the cell, and the cells at the edge of the text (raba/shale#243)
        g.key(Key::ArrowRight, Modifiers::SHIFT);
        assert_eq!((g.editing(), g.cell_caret(), g.ed.selected_cells()), (Some((0, 0)), (1, true), vec![]));
        g.key(Key::ArrowRight, Modifiers::SHIFT);
        assert_eq!((g.editing(), g.ed.selected_cells()), (Some((0, 0)), vec![(0, 0), (0, 1)]));
        // a plain arrow ends that and moves on from the cell
        g.key(Key::ArrowRight, Modifiers::NONE);
        assert_eq!((g.editing(), g.ed.selected_cells()), (Some((0, 1)), vec![]));
        // typing after a move goes where the cursor is
        g.type_text("x");
        g.frames(1);
        assert_eq!(g.editing(), Some((0, 1)));
        assert!(g.text.contains("| A | xB |"), "{}", g.text);
    }

    // spec: EDIT-030
    #[test]
    fn up_and_down_move_between_the_lines_of_a_wrapped_cell_first() {
        let long = "word ".repeat(80);
        let table = format!("| A | B |\n|---|---|\n| x | {long}|\n| y | z |\n");
        let mut g = Grid::new("cell-arrows-wrap", &note_with(&table), false);
        let z = g.centre_of("z");
        g.click(z);
        g.key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(g.editing(), Some((1, 1)), "into the long cell, at the end of its text: its last line");
        g.key(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(g.editing(), Some((1, 1)), "Up goes a line up inside the wrapped text first");
        g.key(Key::ArrowDown, Modifiers::NONE);
        g.key(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(g.editing(), Some((2, 1)), "and on the last line Down leaves");
    }

    // spec: EDIT-030
    #[test]
    fn the_editing_ends_past_the_last_cell_and_with_a_click_elsewhere_and_ctrl_z_takes_it_back() {
        let original = note_with(TABLE);
        let mut g = Grid::new("cell-edit-end", &original, false);
        let four = g.centre_of("4");
        g.click(four);
        assert_eq!(g.editing(), Some((2, 1)));
        g.type_text("!\n?");
        g.key(Key::Tab, Modifiers::NONE);
        assert_eq!(g.editing(), None, "Tab in the last cell ends the editing");
        assert!(g.text.contains("| 4! ? |"), "a pasted line break is a blank: {}", g.text);
        // the text box's own Ctrl+Z takes the whole editing back (EDIT-025's undo)
        g.key(Key::Z, Modifiers::COMMAND);
        assert_eq!(g.text, original);
        // a click into the text outside the table ends an editing as well
        let one = g.centre_of("1");
        g.click(one);
        assert_eq!(g.editing(), Some((1, 0)));
        let (_, pos) = g.ed.last_layout.clone().unwrap();
        g.click(pos + Vec2::new(10.0, 6.0));
        assert_eq!(g.editing(), None);
        assert!(g.ctx.memory(|m| m.has_focus(g.ed.id)));
    }

    // spec: EDIT-030
    #[test]
    fn the_setting_and_the_button_of_a_table_choose_between_cells_and_raw() {
        let original = note_with(TABLE);
        let mut g = Grid::new("cell-mode", &original, false);
        let three = g.centre_of("3");
        g.press_at(three, "mode");
        assert_eq!(g.meta().cells, Some(false), "the button stores raw for this table");
        assert!(g.text.starts_with("before\n\n<!-- shale-table edit=raw -->\n| A |"), "{}", g.text);
        assert!(g.is_grid(), "the comment line is hidden with the table, which stays a grid");
        assert_eq!(g.history_steps(), 1, "one step");
        assert_eq!(g.text, original);
        // the setting off: a click shows the raw markdown, as before EDIT-030
        g.ed.table_cell_edit = false;
        g.frames(2);
        g.click(three);
        assert_eq!(g.editing(), None);
        assert!(!g.is_grid(), "the click put the cursor into the table");
        // a table that says cells is edited in the grid whatever the setting
        g.text = note_with(&format!("<!-- shale-table edit=cells -->\n{TABLE}"));
        g.ed.pending_cursor = Some(0);
        g.frames(3);
        let three = g.centre_of("3");
        g.click(three);
        assert_eq!(g.editing(), Some((2, 0)));
        // and its button turns it back to raw (the comment line stays in sync)
        g.key(Key::Escape, Modifiers::NONE);
        g.press_at(three, "mode");
        assert_eq!(g.meta().cells, Some(false));
    }

    // spec: EDIT-031
    #[test]
    fn dragging_a_border_resizes_and_a_double_click_shares_the_room_equally() {
        let original = note_with(TABLE);
        let mut g = Grid::new("table-resize", &original, false);
        let border = g.border("Col(0)");
        g.drag(border.center(), Vec2::new(60.0, 0.0));
        let m = g.meta();
        assert_eq!(m.sizes.cols.len(), 2, "{}", g.text);
        assert!(m.sizes.rows.len() == 3 && m.cells.is_none());
        let (a, b) = (m.sizes.cols[0], m.sizes.cols[1]);
        assert!(a > b + 40.0, "the first column got wider: {:?}", m.sizes.cols);
        assert!(g.is_grid() && g.editing().is_none(), "a drag does not edit a cell");
        assert_eq!(g.history_steps(), 1, "one step for the drag");
        assert_eq!(g.text, original);
        // the right edge: the whole table, in proportion
        g.frames(2);
        let right = g.border("Right");
        g.drag(right.center(), Vec2::new(100.0, 0.0));
        let wide = g.meta().sizes.cols;
        g.frames(2);
        let col = g.border("Col(0)");
        g.hover(col.center());
        for down in [true, false, true, false] {
            g.frame(click_events(col.center(), down));
        }
        g.frames(2);
        let even = g.meta().sizes.cols;
        assert!((even[0] - even[1]).abs() < 1.0, "a double click shares the width equally: {even:?}");
        assert!((even.iter().sum::<f32>() - wide.iter().sum::<f32>()).abs() < 2.0, "of the whole width: {wide:?} {even:?}");
        // a row: the bottom edge makes the rows taller, a double click on a row border evens them
        let bottom = g.border("Bottom");
        g.drag(bottom.center(), Vec2::new(0.0, 60.0));
        let rows = g.meta().sizes.rows;
        let row = g.border("Row(0)");
        g.hover(row.center());
        for down in [true, false, true, false] {
            g.frame(click_events(row.center(), down));
        }
        g.frames(2);
        let even_rows = g.meta().sizes.rows;
        assert!(even_rows.windows(2).all(|w| (w[0] - w[1]).abs() < 1.0), "{rows:?} -> {even_rows:?}");
        assert!((even_rows.iter().sum::<f32>() - rows.iter().sum::<f32>()).abs() < 2.0);
    }

    // spec: EDIT-031
    #[test]
    fn the_buttons_keep_the_sizes_with_their_columns_and_rows() {
        let mut g = Grid::new("table-resize-buttons", &note_with(&format!("<!-- shale-table cols=100,50 rows=30,30,30 -->\n{TABLE}")), false);
        let three = g.centre_of("3");
        g.press_at(three, "insert-column");
        assert_eq!(g.meta().sizes.cols, [100.0, 100.0, 50.0]);
        let three = g.centre_of("3");
        g.press_at(three, "delete-row");
        assert_eq!(g.meta().sizes.rows, [30.0, 30.0]);
    }

    // spec: EDIT-025
    #[test]
    fn a_change_that_no_longer_fits_the_note_is_dropped_without_a_step() {
        let original = note_with(TABLE);
        let mut g = Grid::new("stale-edit", &original, false);
        use crate::table::Edit;
        for stale in [(5, Edit::InsertColumn(0)), (0, Edit::DeleteRow(9)), (0, Edit::InsertColumn(7)), (0, Edit::DeleteColumn(7))] {
            g.ed.table_edit = Some((stale.0, crate::table::Change::Edit(stale.1)));
            g.frame(vec![]);
            assert_eq!(g.text, original, "{stale:?} is dropped");
            assert!(!g.changed && g.ed.table_edit.is_none());
        }
        assert_eq!(g.history_steps(), 0, "none of them is a step of the history");
    }

    // spec: EDIT-025
    #[test]
    fn the_cursor_stays_on_its_text_when_a_table_before_it_grows_or_shrinks() {
        let original = note_with(TABLE);
        let at = |g: &Grid| markdown::char_to_byte(&g.text, g.cursor_char());
        // behind the table: "af|ter"
        let mut g = Grid::new("cursor-after", &original, false);
        g.ed.pending_cursor = Some(original.chars().count() - 3);
        g.frames(3);
        let cell = g.centre_of("3");
        g.press_at(cell, "add-column");
        assert!(g.text.len() > original.len() && g.text[at(&g)..] == *"ter", "the cursor moved with the text: {:?}", &g.text[at(&g)..]);
        g.press_at(cell, "delete-column");
        g.press_at(cell, "delete-row");
        assert_eq!(&g.text[at(&g)..], "ter", "and again when the table shrinks");
        // before the table the cursor does not move
        let mut g = Grid::new("cursor-before", &original, false);
        g.ed.pending_cursor = Some(3);
        g.frames(3);
        let cell = g.centre_of("3");
        g.press_at(cell, "add-column");
        assert_eq!(g.cursor_char(), 3);
        assert!(g.is_grid());
    }

    // spec: EDIT-025
    #[test]
    fn the_buttons_follow_the_pointer_and_go_away_when_it_leaves() {
        let mut g = Grid::new("zones", &note_with(TABLE), false);
        let (one, four) = (g.centre_of("1"), g.centre_of("4"));
        // over a cell: the buttons of its row (at the right) and its column (above)
        g.hover(four);
        let (row, col) = (g.button("insert-row"), g.button("insert-column"));
        assert!((row.center().y - four.y).abs() < 20.0 && col.center().x > four.x - 25.0 && col.center().x < four.x + 25.0, "{row:?} {col:?} for {four:?}");
        // another cell: other buttons
        g.hover(one);
        assert!(g.button("insert-row").center().y < row.center().y, "the row buttons moved up with the pointer");
        assert!(g.button("insert-column").center().x < col.center().x, "the column buttons moved left");
        // in the strip right of the grid only the row counts (the last column stays the column)
        let right = g.button("add-column");
        g.hover(Pos2::new(right.center().x, four.y));
        assert!((g.button("insert-row").center().y - four.y).abs() < 20.0);
        assert!((g.button("insert-column").center().x - four.x).abs() < 25.0, "the column is the last one");
        // above the grid only the column counts (the header row stays the row)
        let above = g.button("insert-column").center();
        g.hover(above);
        assert!(g.button("insert-row").center().y < one.y - 20.0, "the row is the header");
        // away from the grid: no buttons
        g.hover(Pos2::new(600.0, 600.0));
        assert!(g.buttons.is_empty());
        g.hover(four);
        assert!(!g.buttons.is_empty());
        g.hover(Pos2::new(right.max.x + TABLE_SIDE, four.y));
        assert!(g.buttons.is_empty(), "just outside the buttons' margin");
    }

    // spec: EDIT-025
    #[test]
    fn tables_without_outer_pipes_and_header_only_tables_are_edited_through_the_buttons() {
        let table = "A | B\n:-|-:\n1 | 2\n";
        let mut g = Grid::new("bare-table", &note_with(table), false);
        let cell = g.centre_of("1");
        g.press_at(cell, "add-column");
        assert_eq!(g.text, note_with("A | B |  |\n:-|-: | --- |\n1 | 2 |  |\n"), "a pipe, the cell and a closing pipe; the colons stay");
        g.press_at(cell, "insert-row");
        assert_eq!(g.text, note_with("A | B |  |\n:-|-: | --- |\n1 | 2 |  |\n|   |   |   |\n"));
        assert!(g.is_grid());
        g.press_at(cell, "delete-column");
        assert_eq!(g.text, note_with(" B |  |\n-: | --- |\n 2 |  |\n|   |   |\n"), "the first column is gone, B keeps its alignment");
        assert_eq!(crate::table::find_tables(&g.text)[0].table.aligns, [crate::table::Col::Right, crate::table::Col::Left]);

        // a table with only a header: add-row and insert-row (below the header) work
        let mut g = Grid::new("header-only", &note_with("| A | B |\n|---|---|\n"), false);
        let cell = g.centre_of("A");
        g.press_at(cell, "add-row");
        assert_eq!(g.text, note_with("| A | B |\n|---|---|\n|   |   |\n"));
        g.press_at(cell, "insert-row");
        assert_eq!(g.text, note_with("| A | B |\n|---|---|\n|   |   |\n|   |   |\n"), "below the header, above the first row");
        g.press_at(cell, "insert-column");
        assert!(g.text.contains("| A |  | B |"), "after the hovered column: {}", g.text);
    }

    /// One frame of the editor alone with these events; returns the editor's action.
    fn paste_frame(ctx: &egui::Context, t: &mut f64, ed: &mut NoteEditor, text: &mut String, v: &Vault, events: Vec<Event>) -> Option<EditorAction> {
        *t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(*t), events, ..Default::default() };
        let mut action = None;
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                action = ed.show(ui, text, v, Note { rel: Path::new("Welcome.md"), undo: &mut crate::undo::Histories::default() }, &EditorFonts::uniform(15.0), false).1;
            });
        });
        out.textures_delta.clear();
        action
    }

    /// The `file://` address of a file, as a file manager writes it (spaces as `%20`).
    fn file_uri(path: &Path) -> String {
        let p = path.to_string_lossy().replace('\\', "/").replace(' ', "%20");
        if p.starts_with('/') {
            format!("file://{p}")
        } else {
            format!("file:///{p}")
        }
    }

    fn pasted_toast(a: Option<EditorAction>) -> String {
        match a {
            Some(EditorAction::Pasted(m)) => m,
            _ => panic!("no Pasted action"),
        }
    }

    // spec: EDIT-038
    #[test]
    fn a_pasted_text_with_headings_none_of_them_h1_is_held_back_and_asked_about() {
        let (v, d) = vault_for_tests("paste-lift");
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut ed = NoteEditor::new(egui::Id::unique("paste-lift-ed"));
        let mut text = String::from("ab cd");
        ed.pending_cursor = Some(2);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        // headings, all ## or deeper: nothing is pasted yet, the app is asked with the text
        let deep = "## A\ntext\n### B\n";
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste(deep.into())]);
        assert!(matches!(&a, Some(EditorAction::AskLiftHeadings(held)) if held == deep), "asked");
        assert_eq!(text, "ab cd", "held back");
        // the answer comes back as a paste at the cursor, one step of undo like any paste
        ed.pending_paste = Some("# A\ntext\n## B\n".into());
        let mut undo = crate::undo::Histories::default();
        undo.of(Path::new("Welcome.md")).checkpoint(&text);
        let raw = |time| RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(time), ..Default::default() };
        t += 0.05;
        let mut out = ctx.run_ui(raw(t), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                let (changed, action) = ed.show(ui, &mut text, &v, Note { rel: Path::new("Welcome.md"), undo: &mut undo }, &EditorFonts::uniform(15.0), false);
                assert!(changed && action.is_none(), "pasted without asking again");
            });
        });
        out.textures_delta.clear();
        assert_eq!(text, "ab# A\ntext\n## B\n cd");
        assert!(ed.pending_paste.is_none());
        let mut undone = text.clone();
        assert!(undo.of(Path::new("Welcome.md")).undo(&mut undone).is_some());
        assert_eq!(undone, "ab cd", "the paste is one step of undo");
        // text with a # heading, without headings, or with headings only in code: pasted at once
        for plain in ["# A\n## B\n", "just text", "```\n## not a heading\n```\n", "## A\n# B\n"] {
            let mut text = String::from("x");
            ed.pending_cursor = Some(1);
            paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
            let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste(plain.into())]);
            assert!(a.is_none(), "{plain:?} is pasted without asking");
            assert_eq!(text, format!("x{plain}"));
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-019
    #[test]
    fn pasting_a_picture_or_media_files_saves_them_to_the_media_folder() {
        let (v, d) = vault_for_tests("paste");
        let (y, m, day) = crate::media::local_date();
        let stamp = format!("z_media/{y:04}/{m:02}/{:02}{m:02}{day:02}_", y.rem_euclid(100));
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut ed = NoteEditor::new(egui::Id::unique("paste-ed"));
        let mut text = String::from("ab cd");
        ed.pending_cursor = Some(2);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        // a picture: PNG in the media folder, the link at the cursor
        let img = egui::ColorImage::from_rgba_unmultiplied([2, 1], &[255, 0, 0, 255, 0, 255, 0, 255]);
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::PasteImage(Arc::new(img))]);
        assert_eq!(pasted_toast(a), "Saved the pasted picture to the media folder");
        assert_eq!(text, format!("ab![[{stamp}Pasted image.png]] cd"));
        let saved = image::open(d.join(format!("{stamp}Pasted image.png"))).unwrap().to_rgba8();
        assert_eq!((saved.width(), saved.height(), saved.get_pixel(1, 0).0), (2, 1, [0, 255, 0, 255]));
        // copied files (a PDF, a video, a picture): one link per line; videos and PDFs are linked, pictures embedded
        let src = d.join("..").join(format!("shale-paste-src-{}", std::process::id()));
        std::fs::create_dir_all(&src).unwrap();
        for n in ["a b.pdf", "clip.mp4", "p.png"] {
            std::fs::write(src.join(n), n).unwrap();
        }
        let list: String = ["a b.pdf", "clip.mp4", "p.png"].iter().map(|n| format!("{}\r\n", file_uri(&src.join(n)))).collect();
        let mut text = String::new();
        ed.pending_cursor = Some(0);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste(list.clone())]);
        assert_eq!(pasted_toast(a), "Saved 3 files to the media folder");
        assert_eq!(text, format!("[[{stamp}a b.pdf]]\n![[{stamp}clip.mp4]]\n![[{stamp}p.png]]"));
        assert_eq!(std::fs::read(d.join(format!("{stamp}clip.mp4"))).unwrap(), b"clip.mp4");
        // any other text pastes as it is, a missing file too
        let mut text = String::new();
        ed.pending_cursor = Some(0);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste("just text\nfile:///no/such.png".into())]);
        assert!(a.is_none() && text == "just text\nfile:///no/such.png", "{text}");
        let _ = std::fs::remove_dir_all(&src);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-019
    #[test]
    fn a_pasted_img_tag_becomes_the_clipboards_picture() {
        let (v, d) = vault_for_tests("imgtag");
        let (y, m, day) = crate::media::local_date();
        let stamp = format!("z_media/{y:04}/{m:02}/{:02}{m:02}{day:02}_", y.rem_euclid(100));
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut ed = NoteEditor::new(egui::Id::unique("imgtag-ed"));
        let mut text = String::from("ab");
        ed.pending_cursor = Some(2);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        let mut paste = |ed: &mut NoteEditor, pic: bool| {
            t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(t), events: vec![Event::Paste("<img src=\"image.png\">".into())], ..Default::default() };
            let mut action = None;
            let _ = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.memory_mut(|m| m.request_focus(ed.id));
                    action = ed.paste_media_with(ui, &v, || pic.then(|| egui::ColorImage::from_rgba_unmultiplied([1, 1], &[1, 2, 3, 255])));
                });
            });
            action
        };
        // no picture on the clipboard: the text stays a text paste
        assert!(paste(&mut ed, false).is_none());
        // a picture: saved, and the paste is the link
        assert_eq!(pasted_toast(paste(&mut ed, true)), "Saved the pasted picture to the media folder");
        assert!(d.join(format!("{stamp}Pasted image.png")).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: EDIT-019
    #[test]
    fn vim_p_pastes_a_picture_or_media_files_like_ctrl_v() {
        let (v, d) = vault_for_tests("vimpaste");
        let (y, m, day) = crate::media::local_date();
        let stamp = format!("z_media/{y:04}/{m:02}/{:02}{m:02}{day:02}_", y.rem_euclid(100));
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let mut ed = NoteEditor::new(egui::Id::unique("vimpaste-ed"));
        ed.vim_enabled = true;
        let mut text = String::from("alpha\nbeta");
        ed.pending_cursor = Some(0);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![]);
        assert_eq!(ed.vim_status().as_deref(), Some("NORMAL"));
        // p, then the window answers with a picture: a line of its own below the cursor line
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Text("p".into())]);
        assert!(ed.vim.paste_pending);
        let img = egui::ColorImage::from_rgba_unmultiplied([1, 1], &[1, 2, 3, 255]);
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::PasteImage(Arc::new(img))]);
        assert_eq!(pasted_toast(a), "Saved the pasted picture to the media folder");
        assert_eq!(text, format!("alpha\n![[{stamp}Pasted image.png]]\nbeta"));
        assert!(!ed.vim.paste_pending && d.join(format!("{stamp}Pasted image.png")).exists());
        // p with copied files
        let f = d.join("..").join(format!("shale-vimpaste-{}.pdf", std::process::id()));
        std::fs::write(&f, b"%PDF").unwrap();
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Text("gg".into())]);
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Text("p".into())]);
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste(file_uri(&f))]);
        assert_eq!(pasted_toast(a), "Saved 1 file to the media folder");
        assert!(text.starts_with(&format!("alpha\n[[{stamp}shale-vimpaste-")), "{text}");
        // plain text with p stays as before
        paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Text("p".into())]);
        let a = paste_frame(&ctx, &mut t, &mut ed, &mut text, &v, vec![Event::Paste("ZZ".into())]);
        assert!(a.is_none() && text.contains("ZZ"), "{text}");
        let _ = std::fs::remove_file(&f);
        let _ = std::fs::remove_dir_all(&d);
    }

    /// The format of the section holding the first `needle` of `text`.
    fn format_of<'a>(job: &'a LayoutJob, text: &str, needle: &str) -> &'a TextFormat {
        let at = text.find(needle).unwrap_or_else(|| panic!("{needle} is not in the text"));
        &job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format
    }

    // spec: LINK-014, CFG-042
    #[test]
    fn headings_show_their_numbers_before_the_text_and_the_buffer_stays_as_it_is() {
        let (v, d) = vault_for_tests("numbers");
        let rel = Path::new("Welcome.md");
        let original = "# Title\n## Plan\n### Step\n## Next\n";
        for preview in [false, true] {
            let mut text = original.to_string();
            let mut ed = NoteEditor::new(egui::Id::unique("numbers"));
            let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
            ed.numbered_headings = true;
            ed.live_preview = preview;
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            assert_eq!(text, original, "the numbers are drawn, never written");
            for n in ["1", "1.1", "1.1.1", "1.2"] {
                assert!(h.texts.iter().any(|t| t == n), "number {n} is painted (preview {preview}): {:?}", h.texts);
            }
            assert!(!h.texts.iter().any(|t| t == "0" || t == "2"), "# is numbered like the others: {:?}", h.texts);
            // the number sits left of its heading's text, on its row
            let (galley, pos) = ed.last_layout.clone().unwrap();
            let step = galley.pos_from_cursor(CCursor::new(original.find("Step").unwrap()));
            let number = h.rects.iter().find(|(t, _)| t == "1.1.1").unwrap().1;
            assert!(number.max.x <= pos.x + step.min.x && number.max.x > pos.x + step.min.x - 20.0, "{number:?} before {step:?}");
            assert!(number.center().y > pos.y + step.min.y && number.center().y < pos.y + step.max.y, "{number:?} on the row {step:?}");
            // setting off: no numbers; the front matter overrides it both ways
            ed.numbered_headings = false;
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            assert!(!h.texts.iter().any(|t| t == "1.1"), "no numbers when off: {:?}", h.texts);
            let mut text = "---\nnumbered-headings: true\n---\n## A\n".to_string();
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            assert!(h.texts.iter().any(|t| t == "1"), "front matter turns them on: {:?}", h.texts);
            ed.numbered_headings = true;
            let mut text = "---\nnumbered-headings: no\n---\n## A\n".to_string();
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            h.frame(&mut ed, &mut text, &v, rel, vec![]);
            assert!(!h.texts.iter().any(|t| t == "1"), "front matter turns them off: {:?}", h.texts);
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: LINK-014
    #[test]
    fn room_for_a_number_is_made_in_the_section_where_the_title_starts_and_adds_no_text() {
        let ctx = egui::Context::default();
        let fonts = EditorFonts::uniform(15.0);
        let text = "# T\n## **Bold** one\n\n##\n### Sub\n";
        let mut job = highlight_preview(text, &[], &fonts, &[], Some(0..0));
        let mut out = ctx.run_ui(RawInput::default(), |ui| {
            let marks = make_room_for_numbers(ui, &mut job, &fonts, true, Some(&(0..0)));
            let numbers: Vec<&str> = marks.iter().map(|m| m.number.as_str()).collect();
            assert_eq!(numbers, vec!["1", "1.1", "1.1.1"], "an empty heading gets no number");
            assert!(make_room_for_numbers(ui, &mut job.clone(), &fonts, false, None).is_empty());
        });
        out.textures_delta.clear();
        assert_eq!(job.text, text);
        let room: Vec<usize> = job.sections.iter().filter(|s| s.leading_space > 0.0).map(|s| s.byte_range.start.0).collect();
        assert_eq!(room, vec![text.find("T").unwrap(), text.find("**Bold").unwrap(), text.find("Sub").unwrap()]);
        assert!(job.sections.windows(2).all(|w| w[0].byte_range.end == w[1].byte_range.start), "the sections still tile the text");
    }

    // spec: LINK-013
    #[test]
    fn the_outline_shows_heading_text_formatted_without_markup() {
        let plain = |t: &str, links: &[(LinkSpan, bool)]| outline_job(t, links, 14.0);
        let job = plain("<mark style=\"background: #FFFF00;\">Audio</mark>", &[]);
        assert_eq!(job.text, "Audio");
        assert_eq!(job.sections[0].format.background, Color32::from_rgb(255, 255, 0));
        assert_eq!(plain("**B** and *i* ~~s~~ `c` ==h==", &[]).text, "B and i s c h");
        let job = plain("**B** x", &[]);
        assert_eq!(job.sections[0].format.color, theme::TEXT_STRONG);
        let job = plain("~~s~~", &[]);
        assert_eq!(job.sections[0].format.strikethrough.width, 1.0);
        let job = plain("`c`", &[]);
        assert_eq!(job.sections[0].format.font_id.family, FontFamily::Monospace);
        for t in ["See [[Note|Alias]] now", "See [Alias](Note.md) now"] {
            let links: Vec<(LinkSpan, bool)> = markdown::find_links(t).into_iter().map(|l| (l, true)).collect();
            let job = plain(t, &links);
            assert_eq!(job.text, "See Alias now", "{t}");
            let at = job.text.find("Alias").unwrap();
            let f = &job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format;
            assert_eq!(f.color, theme::LINK);
        }
    }

    fn is_hidden(f: &TextFormat) -> bool {
        f.color == Color32::TRANSPARENT && f.font_id.size <= 1.0
    }

    // spec: EDIT-051
    #[test]
    fn underline_sub_and_sup_are_drawn_and_their_tags_are_hidden_in_the_preview() {
        let text = "a <u>b</u> H<sub>2</sub>O x<sup>2</sup> <U>c<SUP>d</SUP></U>\n";
        let fonts = EditorFonts::uniform(15.0);
        let job = highlight_preview(text, &[], &fonts, &[], Some(0..0));
        let f = |n: &str| format_of(&job, text, n).clone();
        assert_eq!(f("b").underline.width, 1.0);
        assert_eq!(f("a ").underline, Stroke::NONE);
        let (sub, sup) = (f("2</sub>"), f("2</sup>"));
        assert_eq!((sub.font_id.size, sub.valign, sub.line_height), (15.0 * SCRIPT_SCALE, egui::Align::BOTTOM, None));
        assert_eq!((sup.font_id.size, sup.valign, sup.line_height), (15.0 * SCRIPT_SCALE, egui::Align::TOP, None));
        assert_eq!(f("O").font_id.size, 15.0, "the text around stays as it was");
        // nested: underlined and raised
        let d = f("d");
        assert_eq!((d.underline.width, d.valign), (1.0, egui::Align::TOP));
        for tag in ["<u>", "</u>", "<sub>", "</sub>", "<sup>", "</sup>", "<U>", "</U>", "<SUP>", "</SUP>"] {
            assert!(is_hidden(format_of(&job, text, tag)), "{tag} is hidden");
        }
        // on a raw line the tags show, dimmed, and the text is still formatted
        let raw = highlight_preview(text, &[], &fonts, &[], Some(0..1));
        assert!(!is_hidden(format_of(&raw, text, "<u>")));
        assert_eq!(format_of(&raw, text, "<u>").color, theme::FAINT);
        assert_eq!(format_of(&raw, text, "b").underline.width, 1.0);
        assert_eq!(format_of(&raw, text, "2</sup>").valign, egui::Align::TOP);
        // in code they are plain
        let code = "`<u>x</u>` <u>y";
        let job = highlight_preview(code, &[], &fonts, &[], Some(0..0));
        assert_eq!(format_of(&job, code, "x").underline, Stroke::NONE);
        assert_eq!(format_of(&job, code, "y").underline, Stroke::NONE);
    }

    // spec: EDIT-050
    #[test]
    fn footnote_marks_are_raised_in_the_preview_and_show_their_text_on_the_raw_line() {
        let text = "A[^1] b[^2] c\n\n[^1]: Body\n    more\nplain\n";
        let fonts = EditorFonts::uniform(15.0);
        let defined = markdown::find_footnotes(text).defined();
        assert_eq!(defined.len(), 1, "only [^1] has a definition");
        let job = highlight_preview(text, &[], &fonts, &[], Some(0..0));
        let at = |n: &str| text.find(n).unwrap();
        let fmt = |n: &str| format_of(&job, text, n).clone();
        // the reference: `[^` and `]` hidden, the label raised and smaller in the link colour
        assert!(is_hidden(format_of(&job, text, "[^1]")) && is_hidden(&job.sections.iter().find(|s| s.byte_range.start.0 <= at("]") && at("]") < s.byte_range.end.0).unwrap().format));
        let label = format_of(&job, text, "1] b");
        assert_eq!((label.valign, label.font_id.size, label.color, label.line_height), (egui::Align::TOP, 15.0 * SCRIPT_SCALE, theme::LINK, None));
        // a reference without a definition is shown as written
        let two = fmt("[^2]");
        assert!(!is_hidden(&two) && two.valign != egui::Align::TOP && two.color == theme::TEXT);
        // the definition: its label raised, its text and its continuation lines a little smaller
        let def = format_of(&job, text, "1]: Body");
        assert_eq!((def.valign, def.color), (egui::Align::TOP, theme::LINK));
        assert_eq!(fmt("Body").font_id.size, 15.0 * FOOTNOTE_TEXT);
        assert_eq!(fmt("    more").font_id.size, 15.0 * FOOTNOTE_TEXT);
        assert_eq!(fmt("plain").font_id.size, 15.0);
        assert!(is_hidden(format_of(&job, text, "]: Body")), "the markup of the definition is hidden");
        // with the cursor in the reference the text shows (EDIT-016): the brackets are dim and the label is not raised
        let raw = highlight_preview(text, &[], &fonts, &[], Some(0..1));
        assert!(!is_hidden(format_of(&raw, text, "[^1]")));
        assert_eq!(format_of(&raw, text, "[^1]").color, theme::FAINT);
        let label = format_of(&raw, text, "1] b");
        assert_eq!((label.valign, label.font_id.size, label.color), (egui::Align::BOTTOM, 15.0, theme::LINK));
        // nothing happens in code
        let code = "`a[^1]` x\n[^1]: d\n";
        let job = highlight_preview(code, &[], &fonts, &[], Some(0..0));
        let in_code = format_of(&job, code, "[^1]");
        assert!(!is_hidden(in_code) && in_code.valign == egui::Align::BOTTOM, "a reference in code is plain");
    }

    // spec: EDIT-050, EDIT-051, LINK-013
    #[test]
    fn the_outline_shows_footnote_marks_and_script_without_their_markup() {
        let _defined = markdown::DefinedFootnotes::set(["1".to_string()].into());
        let job = outline_job("H<sub>2</sub>O<u>x</u>[^1][^2]", &[], 14.0);
        assert_eq!(job.text, "H2Ox1[^2]", "the tags and the brackets of a defined footnote are left out, an undefined one stays");
        let sec = |n: &str| job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains(n)).unwrap().format.clone();
        assert_eq!((sec("2").font_id.size, sec("2").valign), (14.0 * SCRIPT_SCALE, egui::Align::BOTTOM));
        assert_eq!(sec("x").underline.width, 1.0);
        let mark = job.sections.iter().find(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0] == "1").unwrap();
        assert_eq!((mark.format.valign, mark.format.color), (egui::Align::TOP, theme::LINK));
    }

    // spec: EDIT-032
    #[test]
    fn fenced_code_is_coloured_by_the_language_of_its_fence_and_stays_plain_without_one() {
        let js = "```js\nconsole.log(\"foooo\")\n```";
        let job = highlight(js, &[], 15.0);
        let string = format_of(&job, js, "\"foooo\"");
        assert_eq!(string.color, theme::syntax_colour(syntax::Kind::String));
        assert_eq!((string.background, string.font_id.family.clone()), (theme::CODE_BG, FontFamily::Monospace), "still a code block");
        assert_ne!(format_of(&job, js, "console").color, string.color, "more than one colour in the block");
        for plain in ["```\nlet a = \"x\"\n```", "```klingon\nlet a = \"x\"\n```"] {
            let job = highlight(plain, &[], 15.0);
            assert_eq!(format_of(&job, plain, "\"x\"").color, theme::CODE, "{plain}");
        }
        // the markdown preview draws code blocks as they are
        let job = highlight_preview(js, &[], &crate::fonts::EditorFonts::uniform(15.0), &[], Some(5..5));
        assert_eq!(format_of(&job, js, "\"foooo\"").color, theme::syntax_colour(syntax::Kind::String));
    }

    // spec: EDIT-032
    #[test]
    fn inline_code_colours_its_strings_numbers_and_calls_and_keeps_the_backticks_dim_code() {
        let text = "call `console.log(\"fooo\", 7)` now and `plain` too";
        let job = highlight(text, &[], 15.0);
        assert_eq!(format_of(&job, text, "\"fooo\"").color, theme::syntax_colour(syntax::Kind::String));
        assert_eq!(format_of(&job, text, "7)").color, theme::syntax_colour(syntax::Kind::Number));
        assert_eq!(format_of(&job, text, "log").color, theme::syntax_colour(syntax::Kind::Function));
        assert_eq!(format_of(&job, text, "plain").color, theme::CODE);
        assert_eq!(format_of(&job, text, "\"fooo\"").background, theme::CODE_BG);
    }

    // spec: EDIT-029
    #[test]
    fn marks_and_spans_colour_the_text_between_their_dimmed_tags() {
        let text = "a <mark style=\"background: #FFFF00;\">hi <span style=\"color: #FF0000;\">there</span></mark> b";
        let job = highlight(text, &[], 15.0);
        let yellow = Color32::from_rgb(255, 255, 0);
        let hi = format_of(&job, text, "hi");
        assert_eq!((hi.background, hi.color), (yellow, Color32::BLACK), "the highlight, with dark text on a bright background");
        let there = format_of(&job, text, "there");
        assert_eq!((there.background, there.color), (yellow, Color32::from_rgb(255, 0, 0)), "a span inside a mark: both, the font colour wins");
        for tag in ["<mark", "</span>", "</mark>", "<span"] {
            assert_eq!(format_of(&job, text, tag).color, theme::FAINT, "{tag} is dimmed markup");
        }
        assert_eq!(format_of(&job, text, "<mark").background, Color32::TRANSPARENT, "the tag is not highlighted itself");
        let a = format_of(&job, text, "a ");
        assert_eq!(a.background, Color32::TRANSPARENT, "text outside has no colour");
        // the colour in the tag is no #tag
        assert!(job.sections.iter().all(|s| s.format.background != theme::TAG_BG), "#FFFF00 is not a tag");
        // a dark highlight gets light text; a font colour with an alpha channel keeps it
        let text = "<mark style=\"background: #000080;\">Q</mark> <span style='color:#00FF0080'>G</span>";
        let job = highlight(text, &[], 15.0);
        assert_eq!(format_of(&job, text, "Q").color, Color32::WHITE);
        assert_eq!(format_of(&job, text, "G").color, Color32::from_rgba_unmultiplied(0, 255, 0, 128));
    }

    // spec: EDIT-029
    #[test]
    fn colours_nest_with_bold_and_italic_and_garbled_tags_are_plain_text() {
        let text = "<mark style=\"background: #39FF14;\">**bold** and *it*</mark>";
        let job = highlight(text, &[], 15.0);
        let bold = format_of(&job, text, "bold");
        assert_eq!(bold.background, Color32::from_rgb(0x39, 0xff, 0x14));
        assert!(bold.extra_letter_spacing > 0.0, "still bold inside the mark");
        assert!(format_of(&job, text, "it").italics);
        for text in ["<mark style=\"background: #FFFF00;\">never closed", "</mark> alone", "<span style=\"color: red\">x</span>", "<mark style=\"background: #FFFF0\">y</mark>", "`<mark style=\"background: #FFFF00;\">`x</mark>"] {
            let job = highlight(text, &[], 15.0);
            assert!(job.sections.iter().all(|s| s.format.background != Color32::from_rgb(255, 255, 0) && s.format.color != Color32::from_rgb(255, 0, 0)), "{text} is drawn as text");
            assert_eq!(job.text, text, "and nothing is lost");
        }
    }

    // spec: EDIT-029, EDIT-016
    #[test]
    fn the_preview_hides_the_colour_tags_away_from_the_cursor_and_shows_them_raw_at_it() {
        let text = "a <mark style=\"background: #FFFF00;\">hi</mark> <span style=\"color: #F00\">x</span>\nraw";
        let m = hidden_map(text, Some(1..2));
        let first = m.split('\n').next().unwrap();
        let hidden: usize = ["<mark style=\"background: #FFFF00;\">", "</mark>", "<span style=\"color: #F00\">", "</span>"].iter().map(|t| t.len()).sum();
        assert_eq!(first.matches('_').count(), hidden, "all four tags are hidden and nothing else: {first}");
        // the line of the cursor is raw
        assert!(!hidden_map(text, Some(0..1)).contains('_'));
        // hidden or not, the colour stays on the text
        let links = vec![];
        let job = highlight_preview(text, &links, &EditorFonts::uniform(15.0), &[], Some(1..2));
        assert_eq!(format_of(&job, text, "hi").background, Color32::from_rgb(255, 255, 0));
    }

    // spec: CFG-039
    #[test]
    fn cfg_039_bold_and_italic_use_the_font_s_own_faces_in_the_raw_and_rendered_editor() {
        use crate::fonts::{AreaFont, Faces};
        let fam = |n: &str| FontFamily::Name(n.into());
        let font = |faces| AreaFont { family: fam("system:X"), size: 15.0, faces };
        let text = "**bold** *it* **b *both* b** `**code**`\n> quoted";
        let all = Faces { bold: true, italic: true, bold_italic: true };
        for fonts in [EditorFonts { raw: font(all), rendered: font(Faces::default()) }, EditorFonts { raw: font(Faces::default()), rendered: font(all) }] {
            for preview in [false, true] {
                // the raw editor, or the preview with every line drawn rendered (no line raw)
                let job = highlight_preview(text, &[], &fonts, &[], preview.then_some(99..99));
                let real = if preview { fonts.rendered.faces == all } else { fonts.raw.faces == all };
                let f = |n: &str| format_of(&job, text, n).clone();
                if real {
                    assert_eq!((f("bold").font_id.family, f("bold").extra_letter_spacing), (fam("system:X:bold"), 0.0));
                    assert_eq!((f("it").font_id.family, f("it").italics), (fam("system:X:italic"), false));
                    assert_eq!((f("both").font_id.family, f("both").italics, f("both").extra_letter_spacing), (fam("system:X:bold-italic"), false, 0.0));
                    assert_eq!((f("quoted").font_id.family, f("quoted").italics), (fam("system:X:italic"), false), "a quote is italic too");
                } else {
                    assert_eq!(f("bold").font_id.family, fam("system:X"));
                    assert!(f("bold").extra_letter_spacing > 0.0 && f("it").italics && f("quoted").italics, "imitated without the faces");
                }
                assert_eq!(f("bold").color, theme::TEXT_STRONG, "bold keeps its stronger colour");
                assert_eq!(f("code").font_id.family, FontFamily::Monospace, "code stays monospace");
            }
        }
    }

    // spec: CFG-037
    #[test]
    fn cfg_037_raw_and_rendered_lines_of_the_preview_are_laid_out_at_their_own_sizes() {
        let (v, d) = vault_for_tests("fonts-two");
        let rel = Path::new("Welcome.md");
        let fonts = EditorFonts { raw: crate::fonts::AreaFont::shale(12.0), rendered: crate::fonts::AreaFont::shale(24.0) };
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut text = String::from("rendered line\nraw line\nrendered again");
        let mut ed = NoteEditor::new(egui::Id::unique("t-fonts-two"));
        ed.live_preview = true;
        ed.pending_cursor = Some(15); // in the second line
        let rows = |ed: &mut NoteEditor, text: &mut String| -> Vec<f32> {
            let mut t = 0.0;
            let mut heights = vec![];
            for _ in 0..4 {
                t += 0.05;
                let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0))), time: Some(t), ..Default::default() };
                let mut out = ctx.run_ui(raw, |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        ed.show(ui, text, &v, Note { rel, undo: &mut crate::undo::Histories::default() }, &fonts, false);
                    });
                });
                out.textures_delta.clear();
                heights = ed.last_layout.as_ref().unwrap().0.rows.iter().map(|r| r.height()).collect();
            }
            heights
        };
        let h = rows(&mut ed, &mut text);
        assert_eq!(h.len(), 3, "{h:?}");
        assert!(h[0] > 30.0 && h[2] > 30.0, "the rendered lines are drawn at 24 points: {h:?}");
        assert!(h[1] < 22.0, "the raw line is drawn at 12 points: {h:?}");
        // without the preview every line is raw
        ed.live_preview = false;
        let h = rows(&mut ed, &mut text);
        assert!(h.iter().all(|x| *x < 22.0), "{h:?}");
        let _ = std::fs::remove_dir_all(&d);
    }
}
