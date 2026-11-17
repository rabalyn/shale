//! Inline Markdown as LaTeX (TEX-008, TEX-015): the text of one line with its emphasis, strong text,
//! strike-through, inline code, colours, links and tags. The reading view's own parser
//! (`reader::inline_painted`) finds all of that, so the PDF says what the screen says; this module
//! turns the styled pieces it returns into TeX. Everything that is text goes through
//! [`tex_escape`], so no text of a note is ever read as a command.

use super::sets::tex_escape;
use crate::markdown;
use crate::pdf::export::{over_white, print_bg};
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{Color32, FontFamily};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;

/// `[` and `*` at the start of the text after `\item`, `\\` or `&` would be read as an optional
/// argument or a star: an empty group in front of the text keeps them text.
pub fn lead(tex: String) -> String {
    if tex.starts_with(['[', '*']) {
        format!("{{}}{tex}")
    } else {
        tex
    }
}

/// `\textcolor` or `\colorbox` with an exact RGB value.
fn rgb(c: [u8; 3]) -> String {
    format!("{},{},{}", c[0], c[1], c[2])
}

/// A web address written for `\href`: `\ { } %` and the other characters TeX reads specially are
/// escaped the way hyperref reads them back, and an address without a scheme (`www.`) gets one.
fn href_url(url: &str) -> String {
    let url = if url.to_ascii_lowercase().starts_with("www.") { format!("https://{url}") } else { url.to_string() };
    let mut out = String::new();
    for c in url.chars() {
        match c {
            '%' | '#' | '&' | '_' | '~' | '$' | '^' => {
                out.push('\\');
                out.push(c);
            }
            // a brace, a backslash or a character that is no part of an address: percent-encoded,
            // as a browser would send it
            '\\' | '{' | '}' | '|' | '"' | '<' | '>' | '`' | ' ' => out.push_str(&format!("\\%{:02X}", c as u32)),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// What the pieces of a line need to know about the note.
pub struct Look<'a> {
    pub vault: &'a Vault,
    pub rel: &'a Path,
    /// The footnotes the note defines (TEX-028): label → the Markdown of the definition.
    pub notes: HashMap<String, String>,
    /// Inside a table, where `\footnote` prints no text: the texts of the footnotes of the cells
    /// written so far, set down below the table (`\footnotemark` is written in their place).
    pub deferred: RefCell<Option<Vec<String>>>,
}

impl<'a> Look<'a> {
    pub fn new(vault: &'a Vault, rel: &'a Path, notes: HashMap<String, String>) -> Self {
        Look { vault, rel, notes, deferred: RefCell::new(None) }
    }

    /// The same note without footnotes: the text of a footnote has none inside it.
    fn without_notes(&self) -> Look<'a> {
        Look::new(self.vault, self.rel, HashMap::new())
    }
}

/// One line of Markdown as TeX text.
pub fn inline(look: &Look, text: &str) -> String {
    write_line(look, text, false)
}

/// The text of a table cell: `<br>` is a line break (EDIT-045, TEX-028).
pub fn cell(look: &Look, text: &str) -> String {
    write_line(look, text, true)
}

/// A footnote's text (the lines of its definition joined) as TeX text, with no footnote in it.
fn note_text(look: &Look, label: &str) -> Option<String> {
    let md = look.notes.get(label)?;
    Some(inline(&look.without_notes(), md))
}

/// The TeX of a formula that the layout typeset: `\(…\)`, the formula's own text unchanged (every
/// command in it is one of the parser's, TEX-028). A `$$…$$` in a line of text keeps the display
/// style but stays in the line.
pub fn math_tex(hidden: &str) -> String {
    let (display, inner) = match hidden.strip_prefix("$$") {
        Some(rest) => (true, rest.strip_suffix("$$").unwrap_or(rest)),
        None => (false, hidden.strip_prefix('$').map_or(hidden, |r| r.strip_suffix('$').unwrap_or(r))),
    };
    let inner = one_line(inner);
    format!("\\({}{inner}\\)", if display { "\\displaystyle " } else { "" })
}

/// A formula's text with its line ends as spaces: a blank line would end a paragraph in TeX.
pub fn one_line(tex: &str) -> String {
    tex.split(['\n', '\r']).map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ")
}

fn write_line(look: &Look, text: &str, in_cell: bool) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let _defined = markdown::DefinedFootnotes::set(look.notes.keys().cloned().collect());
    let links: Vec<(markdown::LinkSpan, bool)> = markdown::find_links(text).into_iter().map(|l| (look.vault.exists(&l.target, Some(look.rel)), l)).map(|(ok, l)| (l, ok)).collect();
    let (job, spans, paints, maths) = if in_cell { crate::reader::cell_painted(text, &links, 14.0, theme::TEXT) } else { crate::reader::inline_painted(text, &links, 14.0, theme::TEXT, false) };
    let mut out = String::new();
    // the end of the footnote or the formula that was written: what is inside it is not written again
    let mut skip_to = 0;
    for s in &job.sections {
        let r = s.byte_range.start.0..s.byte_range.end.0;
        let f = &s.format;
        let shown = &job.text[r.clone()];
        if shown.is_empty() || r.start < skip_to {
            continue;
        }
        // a formula that was typeset: its hidden TeX once, as math
        if let Some(m) = maths.iter().find(|m| m.range.start == r.start && m.formula.is_ok()) {
            out.push_str(&math_tex(&job.text[m.range.clone()]));
            skip_to = m.range.end;
            continue;
        }
        // the mark of a footnote: the footnote itself
        if let Some((_, l, _)) = spans.iter().find(|(sr, l, _)| sr.start <= r.start && r.end <= sr.end && crate::reader::footnote_of(l).is_some_and(|(k, _)| k == crate::reader::Foot::Reference)) {
            let note = note_text(look, &l.target);
            match (note, look.deferred.borrow_mut().as_mut()) {
                (Some(t), Some(later)) => {
                    later.push(t);
                    out.push_str("\\footnotemark{}");
                }
                (Some(t), None) => out.push_str(&format!("\\footnote{{{t}}}")),
                (None, _) => out.push_str(&tex_escape(&format!("[^{}]", l.target))),
            }
            skip_to = r.end;
            continue;
        }
        let paint = paints.iter().find(|(pr, ..)| pr.start <= r.start && r.end <= pr.end);
        let (face_bold, face_italic) = crate::fonts::bold_italic_of(&f.font_id.family);
        let code = f.font_id.family == FontFamily::Monospace;
        // one piece of the section's text in all its markup; a cell's `<br>` splits a section
        let style = |shown: &str| {
            let mut t = if code { code_text(shown) } else { tex_escape(shown) };
            if f.extra_letter_spacing > 0.0 || face_bold {
                t = format!("\\textbf{{{t}}}");
            }
            if f.italics || face_italic {
                t = format!("\\textit{{{t}}}");
            }
            if f.strikethrough.width > 0.0 {
                t = format!("\\sout{{{t}}}");
            }
            if crate::reader::underlined(f) {
                t = format!("\\underline{{{t}}}");
            }
            match crate::reader::script_of(f) {
                Some(true) => t = format!("\\textsuperscript{{{t}}}"),
                Some(false) => t = format!("\\textsubscript{{{t}}}"),
                None => {}
            }
            if let Some((_, _, Some([r, g, b, _]))) = paint {
                t = format!("\\textcolor[RGB]{{{}}}{{{t}}}", rgb([*r, *g, *b]));
            }
            // a highlight: the colour the note asks for, else the reading view's (over white paper)
            let bg = match paint.and_then(|p| p.1) {
                Some(c) => Some(over_white(c)),
                None if code || f.background == Color32::TRANSPARENT || f.background == theme::TAG_BG => None,
                None => print_bg(f.background),
            };
            if let Some(c) = bg {
                t = format!("\\colorbox[RGB]{{{}}}{{{t}}}", rgb([c.0, c.1, c.2]));
            }
            if let Some((_, l, _)) = spans.iter().find(|(sr, l, _)| sr.start <= r.start && r.end <= sr.end && crate::lists::is_web_url(&l.target)) {
                t = format!("\\href{{{}}}{{{t}}}", href_url(&l.target));
            }
            t
        };
        if in_cell {
            out.push_str(&shown.split('\n').map(|p| if p.is_empty() { String::new() } else { style(p) }).collect::<Vec<_>>().join(" \\newline "));
        } else {
            out.push_str(&style(shown));
        }
    }
    lead(out)
}

/// Inline code: its text in the monospaced font, spaces kept.
pub fn code_text(code: &str) -> String {
    format!("\\texttt{{{}}}", tex_escape(code).replace(' ', "\\ "))
}
