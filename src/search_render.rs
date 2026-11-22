//! How a hit of the search is drawn (NAV-056…NAV-061): the line of a note rendered like the
//! editor draws a line away from its cursor, or raw, and the found text marked in both. The
//! inline rendering is the editor's own ([`crate::editor::inline_job`]); this module only turns
//! its sections into one job with the task and list markers as drawn here and the marks as a
//! background, and keeps the job with the hit so that it is made once, not in every frame.

use crate::app::Action;
use crate::editor;
use crate::markdown;
use crate::picker::Hit;
use crate::theme;
use eframe::egui::text::{ByteIndex, LayoutJob, LayoutSection};
use eframe::egui::{Color32, FontId, TextFormat};
use std::cell::RefCell;
use std::ops::Range;
use std::sync::Arc;

/// A hit shows at most this many characters of its text (NAV-024).
pub const MAX_SHOWN_CHARS: usize = 120;

/// What a job is made for: rendered or raw, in which size, and the colour of raw text
/// (`Color32::PLACEHOLDER`: the colour of the widget it is drawn in).
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub rendered: bool,
    pub size: f32,
    pub raw_colour: Color32,
}

/// The job of a hit, kept with it: made when the hit is first drawn for a look and again when
/// the look or the colours of the theme change, not in every frame.
#[derive(Default)]
pub struct Jobs(RefCell<Option<(Key, Arc<LayoutJob>)>>);

type Key = (bool, u32, Color32, [Color32; 3]);

impl Hit {
    /// The title of the hit as a job (NAV-056): the line of a note rendered or raw as `look`
    /// says, anything else raw; the found text (`marks`) has a background in all of them.
    pub fn job(&self, look: Look) -> Arc<LayoutJob> {
        let rendered = look.rendered && matches!(self.action, Action::GotoMatch { .. });
        let p = theme::palette();
        let key: Key = (rendered, look.size.to_bits(), look.raw_colour, [p.text, p.selection_bg, p.link]);
        if let Some((k, job)) = &*self.jobs.0.borrow() {
            if *k == key {
                return job.clone();
            }
        }
        let job = Arc::new(if rendered { rendered_job(&self.title, &self.marks, look.size) } else { raw_job(&self.title, &self.marks, look.size, look.raw_colour) });
        *self.jobs.0.borrow_mut() = Some((key, job.clone()));
        job
    }
}

/// A piece of the text of a line: the bytes of the line it is drawn from, what is drawn and how.
struct Piece {
    source: Range<usize>,
    text: String,
    /// the text is the bytes of the line (not a glyph that stands for them): it can be cut
    cuttable: bool,
    format: TextFormat,
}

/// `text` as it is written, the marks with a background (NAV-056).
pub fn raw_job(text: &str, marks: &[Range<usize>], size: f32, colour: Color32) -> LayoutJob {
    let format = TextFormat { font_id: FontId::proportional(size), color: colour, ..Default::default() };
    let piece = Piece { source: 0..text.len(), text: text.to_string(), cuttable: true, format };
    job_of(vec![piece], marks)
}

/// A line of a note as the editor draws it away from its cursor (NAV-056): bold, italic, strike,
/// highlight, inline code, links and wiki links by their shown text, tags in their colour, a
/// heading styled without its `#`, a task as `[ ]` / `[x]` (without its list marker) and a list item with a bullet. Markup the
/// editor hides is not in the text. A line with a typeset formula is raw: the formula is painted
/// over its hidden TeX by the editor, which a hit does not do.
pub fn rendered_job(line: &str, marks: &[Range<usize>], size: f32) -> LayoutJob {
    // (a link counts as existing: which notes exist is not asked for every hit)
    let links: Vec<_> = markdown::find_links(line).into_iter().map(|l| (l, true)).collect();
    let (job, sources) = editor::inline_job(line, &links, size);
    if job.sections.iter().any(|s| crate::math::paint::is_hidden_formula(&s.format)) {
        return raw_job(line, marks, size, theme::palette().text);
    }
    // the markers that are drawn another way: where they are written and what is drawn
    let marker = markdown::parse_task_line(line).map(|(check, done, _, indent)| (indent..check + 2, if done { "[x]" } else { "[ ]" })).or_else(|| {
        let indent = line.len() - line.trim_start().len();
        ["- ", "* ", "+ "].iter().any(|m| line[indent..].starts_with(m)).then_some((indent..indent + 1, "\u{2022}"))
    });
    let mut pieces = Vec::new();
    let mut glyph_done = false;
    for (s, source) in job.sections.iter().zip(sources) {
        let text = &job.text[s.byte_range.start.0..s.byte_range.end.0];
        // a section is cut where the marker starts and ends, so that the glyph replaces the marker only
        let mut cuts = vec![source.start];
        cuts.extend(marker.iter().flat_map(|(m, _)| [m.start, m.end]).filter(|c| *c > source.start && *c < source.end));
        cuts.push(source.end);
        for w in cuts.windows(2) {
            let (a, b) = (w[0], w[1]);
            let piece = &text[a - source.start..b - source.start];
            let inside = marker.as_ref().is_some_and(|(m, _)| a >= m.start && b <= m.end);
            // (the first piece of the marker is the glyph, the others have no text)
            let (text, cuttable) = match &marker {
                Some((_, glyph)) if inside => (if std::mem::replace(&mut glyph_done, true) { "" } else { glyph }, false),
                _ => (piece, true),
            };
            pieces.push(Piece { source: a..b, text: text.to_string(), cuttable, format: s.format.clone() });
        }
    }
    job_of(pieces, marks)
}

/// The pieces as one job; a piece is cut where a mark starts or ends and the part inside a mark
/// gets the background of a found text. A mark over markup that is not drawn marks what of it is.
fn job_of(pieces: Vec<Piece>, marks: &[Range<usize>]) -> LayoutJob {
    let mut job = LayoutJob::default();
    let colour = theme::palette().selection_bg;
    for piece in pieces {
        let mut at = piece.source.start;
        let mut text = piece.text.as_str();
        // the cuts, in the order of the line: where a mark starts or ends inside the piece
        let mut cuts: Vec<usize> = marks.iter().flat_map(|m| [m.start, m.end]).filter(|c| *c > piece.source.start && *c < piece.source.end).collect();
        cuts.sort_unstable();
        cuts.dedup();
        let marked = |from: usize, to: usize| marks.iter().any(|m| m.start < to && from < m.end);
        let mut push = |text: &str, from: usize, to: usize| {
            if text.is_empty() {
                return;
            }
            let start = job.text.len();
            job.text.push_str(text);
            let mut format = piece.format.clone();
            if marked(from, to) {
                format.background = colour;
            }
            job.sections.push(LayoutSection { leading_space: 0.0, byte_range: ByteIndex(start)..ByteIndex(job.text.len()), format });
        };
        if piece.cuttable {
            for cut in cuts.into_iter().chain(std::iter::once(piece.source.end)) {
                let (head, tail) = text.split_at(cut - at);
                push(head, at, cut);
                text = tail;
                at = cut;
            }
        } else {
            push(text, piece.source.start, piece.source.end);
        }
    }
    job
}

/// The marks of the words of a query in a text of mail or posts (NAV-059): each word of the
/// query (a field prefix such as `from:` is left out, quotes too) is marked where it is a whole
/// word of the text, a word with a wildcard where a word of the text starts with it; case is
/// ignored. This is what the full-text search matches, simplified: no stemming.
pub fn word_marks(text: &str, query: &str) -> Vec<Range<usize>> {
    let mut marks = Vec::new();
    for raw in query.split_whitespace() {
        let word = raw.trim_matches('"');
        let word = match word.split_once(':') {
            Some((field, rest)) if !rest.is_empty() && field.chars().all(char::is_alphabetic) => rest,
            _ => word,
        };
        let (word, prefix) = match word.find(['*', '?']) {
            Some(at) => (&word[..at], true),
            None => (word, false),
        };
        let word = word.trim_matches('"').to_lowercase();
        if word.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(at) = markdown::find_ci(&text[from..], &word) {
            let start = from + at;
            let end = start + matched_len(&text[start..], &word);
            let before = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
            let after = prefix || text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
            if before && after {
                marks.push(start..end);
            }
            from = end.max(start + text[start..].chars().next().map_or(1, char::len_utf8));
            if from >= text.len() {
                break;
            }
        }
    }
    marks.sort_by_key(|m| (m.start, m.end));
    marks
}

/// The bytes of `text` that the lower-case `query` matches at its start, ignoring case (a letter
/// whose lower-case form is longer or shorter than it is counts as the characters of its form).
pub fn matched_len(text: &str, query_lower: &str) -> usize {
    let mut left = query_lower.chars().count();
    let mut end = 0;
    for (i, c) in text.char_indices() {
        if left == 0 {
            break;
        }
        left = left.saturating_sub(c.to_lowercase().count());
        end = i + c.len_utf8();
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_at<'a>(job: &'a LayoutJob, needle: &str) -> &'a TextFormat {
        let at = job.text.find(needle).unwrap_or_else(|| panic!("{needle:?} not in {:?}", job.text));
        &job.sections.iter().find(|s| s.byte_range.start.0 <= at && at < s.byte_range.end.0).unwrap().format
    }

    /// One mark (an array of one range is a lint).
    fn one(r: Range<usize>) -> Vec<Range<usize>> {
        vec![r]
    }

    fn marked(job: &LayoutJob) -> String {
        job.sections.iter().filter(|s| s.format.background == theme::palette().selection_bg).map(|s| &job.text[s.byte_range.start.0..s.byte_range.end.0]).collect()
    }

    // spec: NAV-056
    #[test]
    fn rendered_lines_drop_their_markup_and_style_what_is_left() {
        let job = rendered_job("**bold** and *it* and `code` and ~~gone~~ and ==lit==", &[], 12.5);
        assert_eq!(job.text, "bold and it and code and gone and lit");
        assert_eq!(format_at(&job, "bold").color, theme::palette().text_strong);
        assert_eq!(format_at(&job, "code").font_id.family, eframe::egui::FontFamily::Monospace);
        assert_eq!(format_at(&job, "gone").strikethrough.width, 1.0);
        assert_eq!(format_at(&job, "lit").background, theme::palette().highlight_bg);
        let raw = raw_job("**bold** and *it*", &[], 12.5, Color32::PLACEHOLDER);
        assert_eq!(raw.text, "**bold** and *it*", "raw is the markdown as written");
        assert_eq!(raw.sections.len(), 1);
    }

    // spec: NAV-056
    #[test]
    fn links_show_their_text_and_tags_their_colour() {
        let job = rendered_job("see [[Some note|the alias]] and [a label](Note.md) and #idea now", &[], 12.5);
        assert_eq!(job.text, "see the alias and a label and #idea now");
        assert_eq!(format_at(&job, "the alias").color, theme::palette().link);
        assert_eq!(format_at(&job, "a label").color, theme::palette().link);
        assert_eq!(format_at(&job, "#idea").color, theme::palette().tag);
        let job = rendered_job("a [[Plain]] link", &[], 12.5);
        assert_eq!(job.text, "a Plain link");
    }

    // spec: NAV-056
    #[test]
    fn tasks_are_boxes_list_items_bullets_and_headings_lose_their_hashes() {
        let open = rendered_job("- [ ] buy milk", &[], 12.5);
        assert_eq!(open.text, "[ ] buy milk");
        let done = rendered_job("- [x] done **it**", &[], 12.5);
        assert_eq!(done.text, "[x] done it");
        assert_eq!(rendered_job("* an item", &[], 12.5).text, "\u{2022} an item");
        assert_eq!(rendered_job("1. first", &[], 12.5).text, "1. first", "a numbered item keeps its number");
        let head = rendered_job("## A heading", &[], 12.5);
        assert_eq!(head.text, "A heading");
        assert_eq!(format_at(&head, "A heading").color, theme::palette().heading);
        assert!(format_at(&head, "A heading").font_id.size > 12.5, "a heading is bigger");
    }

    // spec: NAV-056
    #[test]
    fn a_line_with_a_formula_is_shown_raw() {
        let job = rendered_job("the $x^2$ formula", &[], 12.5);
        assert_eq!(job.text, "the $x^2$ formula");
    }

    // spec: NAV-057
    #[test]
    fn the_found_text_is_marked_in_raw_and_rendered_lines() {
        let line = "a **big banana** here";
        let at = line.find("banana").unwrap();
        let raw = raw_job(line, &one(at..at + 6), 12.5, Color32::PLACEHOLDER);
        assert_eq!(marked(&raw), "banana");
        let rendered = rendered_job(line, &one(at..at + 6), 12.5);
        assert_eq!(rendered.text, "a big banana here");
        assert_eq!(marked(&rendered), "banana");
        assert_eq!(format_at(&rendered, "banana").color, theme::palette().text_strong, "the style stays under the mark");
        // a match that starts in the markup and ends in the text marks the visible part
        let at = line.find("**big").unwrap();
        let rendered = rendered_job(line, &one(at..at + 7), 12.5);
        assert_eq!(marked(&rendered), "big b");
        // a match inside the markup alone marks nothing
        let rendered = rendered_job(line, &one(at..at + 2), 12.5);
        assert_eq!(marked(&rendered), "");
        // a match over a link marks its shown text
        let line = "go [[Target|alias]] now";
        let at = line.find("rget|al").unwrap();
        assert_eq!(marked(&rendered_job(line, &one(at..at + 7), 12.5)), "al");
        // a match that includes a task marker marks the box
        let line = "- [ ] milk";
        assert_eq!(marked(&rendered_job(line, &one(0..8), 12.5)), "[ ] mi");
        assert_eq!(marked(&rendered_job(line, &one(6..10), 12.5)), "milk");
    }

    // spec: NAV-059
    #[test]
    fn the_words_of_a_query_are_marked_in_mail_and_post_text() {
        let text = "Banana bread and a BANANAS plan, no bananas";
        let words = |q: &str| word_marks(text, q).into_iter().map(|r| &text[r]).collect::<Vec<_>>();
        assert_eq!(words("banana"), ["Banana"], "a word is a whole word, case ignored");
        assert_eq!(words("banana*"), ["Banana", "BANANA", "banana"], "a wildcard makes a prefix");
        assert_eq!(words("bread plan"), ["bread", "plan"], "each word of the query");
        assert_eq!(words("from:plan"), ["plan"], "a field prefix is left out");
        assert_eq!(words("\"bread\""), ["bread"], "quotes too");
        assert!(words("*").is_empty());
        assert_eq!(word_marks("Äpfel und äpfel", "äpfel").len(), 2, "letters outside ASCII");
    }

    // spec: NAV-060
    #[test]
    fn a_job_is_kept_with_its_hit_until_the_look_changes() {
        let hit = Hit::new("Notes", "**x** y".into(), String::new(), Action::GotoMatch { rel: "a.md".into(), line: 0, col: 0 });
        let look = Look { rendered: true, size: 12.5, raw_colour: Color32::PLACEHOLDER };
        let a = hit.job(look);
        assert!(Arc::ptr_eq(&a, &hit.job(look)), "made once");
        assert_eq!(a.text, "x y");
        let raw = hit.job(Look { rendered: false, ..look });
        assert_eq!(raw.text, "**x** y");
        let other = Hit::new("Posts", "**x**".into(), String::new(), Action::Open { rel: "a.md".into(), new_tab: false });
        assert_eq!(other.job(look).text, "**x**", "only the line of a note is rendered");
    }
}
