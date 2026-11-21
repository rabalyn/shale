//! Inline Markdown as LaTeX (TEX-008, TEX-015): the text of one line with its emphasis, strong text,
//! strike-through, inline code, colours, links, pictures in the line (TEX-031) and tags. The reading
//! view's own parser (`reader::inline_painted`) finds all of that, so the PDF says what the screen
//! says; this module turns the styled pieces it returns into TeX. Everything that is text goes
//! through [`tex_escape`], so no text of a note is ever read as a command.

use super::sets::tex_escape;
use crate::markdown;
use crate::pdf::export::{over_white, print_bg, print_color};
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{Color32, FontFamily};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

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

/// The files the document needs in its work folder and what could not be used (TEX-015): one
/// list for the embeds on a line of their own and the pictures inside a line (TEX-031), so a file
/// used twice is one file.
#[derive(Default)]
pub struct Files {
    pub list: Vec<(String, Vec<u8>)>,
    /// What went wrong without stopping the export (an embed that is missing).
    pub warnings: Vec<String>,
    /// The vault file of an embed → the name it has in the work folder.
    named: HashMap<PathBuf, String>,
}

impl Files {
    /// The name a vault file already has in the work folder.
    pub fn named(&self, rel: &Path) -> Option<String> {
        self.named.get(rel).cloned()
    }

    /// Remember a file for the work folder under a name made here (never one from the note).
    pub fn keep(&mut self, rel: &Path, what: &str, ext: &str, bytes: Vec<u8>) -> String {
        let name = format!("{what}-{}.{ext}", self.list.len() + 1);
        self.list.push((name.clone(), bytes));
        self.named.insert(rel.to_path_buf(), name.clone());
        name
    }

    /// The red italic text that stands where a file could not be used, and its warning.
    pub fn missing(&mut self, what: &str) -> String {
        self.warnings.push(what.to_string());
        let c = print_color(theme::Palette::shale().link_missing);
        format!("\\textcolor[RGB]{{{},{},{}}}{{\\textit{{[{}]}}}}", c.0, c.1, c.2, tex_escape(what))
    }
}

/// The name of the anchor of the heading on this line of the note (TEX-032).
pub fn anchor_name(line: usize) -> String {
    format!("shale-h{line}")
}

/// The anchor of a heading: a link to it lands here (TEX-032; `\shaleanchor` is a named
/// `\phantomsection`, defined by the writer's helpers).
pub fn anchor(line: usize) -> String {
    anchor_named(&anchor_name(line))
}

/// An anchor of this name (a book's, BOOK-016, are named by their place in the book).
pub fn anchor_named(name: &str) -> String {
    format!("\\shaleanchor{{{name}}}")
}

/// Where the wiki links of a book go (BOOK-016): the anchor of each note of the book and of its
/// headings.
#[derive(Default, Debug)]
pub struct Targets {
    /// note → its anchor and its headings' anchors by their plain text in lower case
    pub notes: HashMap<PathBuf, (String, HashMap<String, String>)>,
}

impl Targets {
    /// The anchor a link goes to: the heading's when the note of the book has it, else the note's;
    /// none for a note outside the book.
    fn anchor(&self, vault: &Vault, from: &Path, link: &markdown::LinkSpan) -> Option<&str> {
        let (note, heads) = self.notes.get(&vault.resolve(&link.target, Some(from))?)?;
        let head = link.heading.as_ref().and_then(|h| heads.get(&markdown::heading_plain(h).to_lowercase()));
        Some(head.unwrap_or(note))
    }
}

/// What the pieces of a line need to know about the note.
pub struct Look<'a> {
    pub vault: &'a Vault,
    pub rel: &'a Path,
    /// The note's text: a link to one of its headings is found in it (TEX-032).
    pub text: &'a str,
    /// The lines of the headings that have an anchor in the document.
    pub anchors: &'a HashSet<usize>,
    /// Whether embeds are drawn (the dialog's box); off, every embed is its link text.
    pub embeds: bool,
    pub files: &'a RefCell<Files>,
    /// The footnotes the note defines (TEX-029): label → the Markdown of the definition.
    pub notes: HashMap<String, String>,
    /// Inside a table, where `\footnote` prints no text: the texts of the footnotes of the cells
    /// written so far, set down below the table (`\footnotemark` is written in their place).
    pub deferred: RefCell<Option<Vec<String>>>,
    /// In a book: where its wiki links go (BOOK-016), in the place of the headings of this note.
    pub targets: Option<&'a Targets>,
}

impl<'a> Look<'a> {
    /// The same note without footnotes: the text of a footnote has none inside it.
    fn without_notes(&self) -> Look<'a> {
        Look { notes: HashMap::new(), deferred: RefCell::new(None), ..*self }
    }

    /// A link to a heading of this note that has an anchor: the anchor's name (TEX-032).
    fn heading_anchor(&self, l: &markdown::LinkSpan) -> Option<String> {
        let heading = l.heading.as_deref()?;
        if self.vault.resolve(&l.target, Some(self.rel)).as_deref() != Some(self.rel) {
            return None;
        }
        let line = markdown::find_heading(self.text, heading)?;
        self.anchors.contains(&line).then(|| anchor_name(line))
    }

    /// A picture embedded inside a line (TEX-031): as high as the line, in the line. A picture
    /// that is missing or cannot be read is the red text of a missing embed. None for an embed
    /// that is not a picture, or with embeds off: it stays its link text.
    fn picture_in_line(&self, l: &markdown::LinkSpan) -> Option<String> {
        if !self.embeds {
            return None;
        }
        let rel = self.vault.resolve(&l.target, Some(self.rel));
        let is_picture = crate::media::is_image(rel.as_deref().unwrap_or(Path::new(&l.target)));
        if !is_picture {
            return None;
        }
        let mut files = self.files.borrow_mut();
        let Some(rel) = rel else { return Some(files.missing(&format!("missing: {}", l.target))) };
        let name = match files.named(&rel) {
            Some(n) => n,
            None => match picture_file(&self.vault.abs(&rel)) {
                Some((ext, bytes)) => files.keep(&rel, "picture", ext, bytes),
                None => return Some(files.missing(&format!("not readable: {}", l.target))),
            },
        };
        Some(format!("\\raisebox{{-0.2\\baselineskip}}{{\\includegraphics[height=0.9\\baselineskip,width=\\linewidth,keepaspectratio]{{{name}}}}}"))
    }
}

/// A picture as a file LuaLaTeX can include: a PNG or JPEG as it is, any other format converted
/// to PNG. None when it cannot be read.
pub fn picture_file(abs: &Path) -> Option<(&'static str, Vec<u8>)> {
    let bytes = crate::fsutil::read(abs).ok()?;
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return Some(("png", bytes));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some(("jpg", bytes));
    }
    let img = image::load_from_memory(&bytes).ok()?;
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).ok()?;
    Some(("png", png.into_inner()))
}

/// One line of Markdown as TeX text.
pub fn inline(look: &Look, text: &str) -> String {
    write_line(look, text, false)
}

/// The text of a table cell: `<br>` is a line break (EDIT-045, TEX-029).
pub fn cell(look: &Look, text: &str) -> String {
    write_line(look, text, true)
}

/// A footnote's text (the lines of its definition joined) as TeX text, with no footnote in it.
fn note_text(look: &Look, label: &str) -> Option<String> {
    let md = look.notes.get(label)?;
    Some(inline(&look.without_notes(), md))
}

/// The TeX of a formula that the layout typeset: `\(…\)`, the formula's own text unchanged (every
/// command in it is one of the parser's, TEX-029). A `$$…$$` in a line of text keeps the display
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
    let (job, spans, paints, maths) = if in_cell { crate::reader::cell_painted(text, &links, 14.0, theme::Palette::shale().text) } else { crate::reader::inline_painted(text, &links, 14.0, theme::Palette::shale().text, false) };
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
        // a link of the vault (not a web link, not a footnote) that this piece is part of
        let wiki = spans.iter().find(|(sr, l, _)| sr.start <= r.start && r.end <= sr.end && !crate::lists::is_web_url(&l.target) && crate::reader::footnote_of(l).is_none());
        // a picture inside the line: the picture, once for the whole link (TEX-031)
        if let Some((sr, l, _)) = wiki.filter(|(_, l, _)| l.embed) {
            if let Some(pic) = look.picture_in_line(l) {
                out.push_str(&pic);
                skip_to = sr.end;
                continue;
            }
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
            // a link of the vault has the colour of links on paper, as in the built-in export:
            // a missing note the colour of missing links (TEX-032); else the colour the note gives
            if let Some((_, _, ok)) = wiki {
                let c = print_color(if *ok { theme::Palette::shale().link } else { theme::Palette::shale().link_missing });
                t = format!("\\textcolor[RGB]{{{}}}{{{t}}}", rgb([c.0, c.1, c.2]));
            } else if let Some((_, _, Some([r, g, b, _]))) = paint {
                t = format!("\\textcolor[RGB]{{{}}}{{{t}}}", rgb([*r, *g, *b]));
            }
            // a highlight: the colour the note asks for, else the reading view's (over white paper)
            let bg = match paint.and_then(|p| p.1) {
                Some(c) => Some(over_white(c)),
                None if code || f.background == Color32::TRANSPARENT || f.background == theme::Palette::shale().tag_bg => None,
                None => print_bg(f.background),
            };
            if let Some(c) = bg {
                t = format!("\\colorbox[RGB]{{{}}}{{{t}}}", rgb([c.0, c.1, c.2]));
            }
            if let Some((_, l, _)) = spans.iter().find(|(sr, l, _)| sr.start <= r.start && r.end <= sr.end && crate::lists::is_web_url(&l.target)) {
                t = format!("\\href{{{}}}{{{t}}}", href_url(&l.target));
            }
            // a link to a heading of this note goes there (TEX-032), other notes are not in the PDF;
            // in a book, a link to one of its notes goes to its chapter or heading (BOOK-016)
            let name = match look.targets {
                Some(ts) => wiki.and_then(|(_, l, _)| ts.anchor(look.vault, look.rel, l)).map(str::to_string),
                None => wiki.and_then(|(_, l, _)| look.heading_anchor(l)),
            };
            if let Some(name) = name {
                t = format!("\\hyperlink{{{name}}}{{{t}}}");
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
