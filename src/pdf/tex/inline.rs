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
}

/// One line of Markdown as TeX text.
pub fn inline(look: &Look, text: &str) -> String {
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let links: Vec<(markdown::LinkSpan, bool)> = markdown::find_links(text).into_iter().map(|l| (look.vault.exists(&l.target, Some(look.rel)), l)).map(|(ok, l)| (l, ok)).collect();
    let (job, spans, paints) = crate::reader::inline_painted(text, &links, 14.0, theme::TEXT, false);
    let mut out = String::new();
    for s in &job.sections {
        let r = s.byte_range.start.0..s.byte_range.end.0;
        let f = &s.format;
        let shown = &job.text[r.clone()];
        if shown.is_empty() {
            continue;
        }
        let paint = paints.iter().find(|(pr, ..)| pr.start <= r.start && r.end <= pr.end);
        let (face_bold, face_italic) = crate::fonts::bold_italic_of(&f.font_id.family);
        let code = f.font_id.family == FontFamily::Monospace;
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
        out.push_str(&t);
    }
    lead(out)
}

/// Inline code: its text in the monospaced font, spaces kept.
pub fn code_text(code: &str) -> String {
    format!("\\texttt{{{}}}", tex_escape(code).replace(' ', "\\ "))
}
