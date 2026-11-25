//! An image carousel (EDIT-142, EDIT-143): a quote whose first line is `> [!carousel] Title` and
//! whose other lines are each one image embed, `> ![[a.png|Caption]]` or `> ![alt](a.png)`. Other
//! editors show it as a quote with all the images; the reading view shows one image at a time
//! (`view`), the exports show every image in order with its caption (`flatten`, PDF-040). The
//! markdown stays as written. Only this callout is known; any other quote is a quote.

use crate::markdown::{self, LinkSpan};
use std::ops::Range;

mod view;
pub use view::{arrow_keys, bar, show, BAR_HEIGHT};
#[cfg(test)]
mod tests;
#[cfg(test)]
mod ui_tests;

/// One image of a carousel.
#[derive(Clone, Debug, PartialEq)]
pub struct Slide {
    /// the line number (from 0) of the slide in the note
    pub line: usize,
    /// the link target (`a.png`)
    pub target: String,
    /// the text under the image: the alias of a wiki embed without its size, the alt text of a `![alt](a.png)`
    pub caption: Option<String>,
    /// the width the embed asks for (`|300`)
    pub width: Option<f32>,
    /// the embed alone, without the caption (`![[a.png|300]]`, `![](a.png)`): what an export draws
    pub bare: String,
}

/// A carousel of a note.
#[derive(Clone, Debug, PartialEq)]
pub struct Carousel {
    /// the line numbers (from 0) of the whole quote, the `[!carousel]` line first
    pub lines: Range<usize>,
    pub title: Option<String>,
    pub slides: Vec<Slide>,
}

/// The text after the `>` of a quote line, or none for another line.
fn quote_content(line: &str) -> Option<&str> {
    line.trim_start().strip_prefix('>').map(str::trim)
}

/// The title of a `[!carousel] Title` line (`[!carousel]-` and `+` are the fold marks of other
/// editors and are accepted): `Some(title)` for such a line, whose title may be empty.
fn head(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("[!")?;
    let (word, rest) = rest.split_once(']')?;
    word.eq_ignore_ascii_case("carousel").then(|| rest.trim_start_matches(['+', '-']).trim())
}

/// The slide a quote line is: one image embed and nothing else.
fn slide(content: &str, line: usize) -> Option<Slide> {
    let links = markdown::find_links(content);
    let [l] = links.as_slice() else { return None };
    if !l.embed || l.range != (0..content.len()) || l.target.is_empty() {
        return None;
    }
    let (caption, width, bare) = parts(l, content);
    Some(Slide { line, target: l.target.clone(), caption, width, bare })
}

/// The caption, the asked width and the bare embed of the link `source` (the text of `l`).
fn parts(l: &LinkSpan, source: &str) -> (Option<String>, Option<f32>, String) {
    if l.markdown_style {
        let dest = source.split_once("](").map_or("", |(_, d)| d);
        return (l.alias.clone(), None, format!("![]({dest}"));
    }
    let width = l.embed_width();
    let caption = l.alias.as_deref().and_then(|a| {
        let words = match (width, a.rsplit_once('|')) {
            (Some(_), Some((rest, _))) => rest,
            (Some(_), None) => "",
            (None, _) => a,
        };
        Some(words.trim().to_string()).filter(|w| !w.is_empty())
    });
    let name = source.trim_start_matches("![[").split(['|', ']']).next().unwrap_or("");
    let bare = match width {
        Some(w) => format!("![[{name}|{}]]", w as u32),
        None => format!("![[{name}]]"),
    };
    (caption, width, bare)
}

/// The carousels of `text`, in order. A quote that has no image line after its `[!carousel]` line
/// is no carousel; the carousel ends with the last image line (or empty quote line) of the quote.
pub fn find(text: &str) -> Vec<Carousel> {
    let fenced = markdown::fenced_code_lines(text);
    let lines = markdown::lines_with_offsets(text);
    let mut out = Vec::new();
    let mut i = markdown::front_matter_lines(text);
    while i < lines.len() {
        let title = if fenced[i] { None } else { quote_content(lines[i].1).and_then(head) };
        let Some(title) = title else {
            i += 1;
            continue;
        };
        let mut slides = Vec::new();
        let mut end = i + 1;
        let mut j = i + 1;
        while j < lines.len() && !fenced[j] {
            let Some(content) = quote_content(lines[j].1) else { break };
            if !content.is_empty() {
                let Some(s) = slide(content, j) else { break };
                slides.push(s);
                end = j + 1;
            }
            j += 1;
        }
        if !slides.is_empty() {
            out.push(Carousel { lines: i..end, title: Some(title.to_string()).filter(|t| !t.is_empty()), slides });
        }
        i = end;
    }
    out
}

/// The lines of `text` the editor hides while it shows one image of each carousel (EDIT-144): the
/// slides but the one `shown` says, a byte range each with its line break. `shown` has an entry
/// per carousel of the note, in order: `None` for one that is raw (the cursor is in it), which
/// hides nothing. A text with another number of carousels (it was edited this frame) hides nothing.
pub fn hidden_lines(text: &str, shown: &[Option<usize>]) -> Vec<Range<usize>> {
    let found = find(text);
    if found.len() != shown.len() {
        return Vec::new();
    }
    let lines = markdown::lines_with_offsets(text);
    let mut out = Vec::new();
    for (c, shown) in found.iter().zip(shown) {
        let Some(shown) = *shown else { continue };
        for s in c.slides.iter().enumerate().filter(|(n, _)| *n != shown).map(|(_, s)| s) {
            out.push(lines[s.line].0..lines.get(s.line + 1).map_or(text.len(), |l| l.0));
        }
    }
    out
}

/// `text` with every carousel written out for an export that shows all images (PDF-040): the
/// title in bold, then each image on a line of its own with its caption in italics below, blank
/// lines between.
pub fn flatten(text: &str) -> std::borrow::Cow<'_, str> {
    if !text.contains("[!") {
        return text.into();
    }
    let found = find(text);
    if found.is_empty() {
        return text.into();
    }
    let lines = markdown::lines_with_offsets(text);
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for c in &found {
        let (from, last) = (lines[c.lines.start].0, &lines[c.lines.end - 1]);
        out.push_str(&text[at..from]);
        let mut parts: Vec<String> = c.title.iter().map(|t| format!("**{t}**")).collect();
        for s in &c.slides {
            parts.push(s.bare.clone());
            parts.extend(s.caption.iter().map(|c| format!("*{c}*")));
        }
        // a blank line apart from the lines around, else a paragraph or quote would take the images
        let before = c.lines.start > 0 && !lines[c.lines.start - 1].1.trim().is_empty();
        let after = lines.get(c.lines.end).is_some_and(|l| !l.1.trim().is_empty());
        out.push_str(if before { "\n" } else { "" });
        out.push_str(&parts.join("\n\n"));
        out.push_str(if after { "\n" } else { "" });
        at = last.0 + last.1.len();
    }
    out.push_str(&text[at..]);
    out.into()
}
