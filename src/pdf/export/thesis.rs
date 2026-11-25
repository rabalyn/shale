//! The thesis syntax in the built-in PDF (PDF-032, ADR-0039, #648): citations as the style formats
//! them, references as word and number (a link to the place), numbered figures, tables and
//! equations with their captions, the lists of figures and tables, and the bibliography. What each
//! says comes from [`crate::thesis::View`], the same the reading view and the site take.
//!
//! The text of the note is changed before it is laid out: a label is cut out, and a citation or a
//! reference is replaced by a mark (a private character with a number) that stands for the text
//! the view gives it, so the markdown reader of the layout cannot take it for markup. The layout
//! puts the text back when it makes the pieces of a line.

use super::*;
pub(super) use crate::markdown::citations::Kind;
use crate::markdown::labels::Entry as Label;
pub(super) use crate::thesis::Marker;
use crate::thesis::{Shown, View as ThesisView};
use std::ops::Range;
use std::sync::Arc;

const OPEN: char = '\u{E000}';
const CLOSE: char = '\u{E001}';
const GREY: Rgba = Rgba(105, 105, 110, 255);

/// What a mark of the text stands for.
struct Mark {
    text: String,
    /// where a click on it goes: the key of a landing
    goto: Option<String>,
}

/// The caption of the figure that is laid out next, and where its label lands.
#[derive(Default)]
struct Float {
    label: Option<String>,
    above: Option<String>,
    below: Option<String>,
}

/// The thesis document of the note being laid out and what the layout found out about it.
pub(super) struct Thesis {
    view: Arc<ThesisView>,
    marks: Vec<Mark>,
    /// where the labels and the entries of the bibliography are, by key
    landings: HashMap<String, Dest>,
    float: Float,
    /// the lists of figures or tables reserved, with the room for their lines
    lists: Vec<(Kind, Vec<(usize, f32)>)>,
    /// the lists the boxes of the dialog ask for, each with the line it has in the contents
    pub(super) boxes: Vec<(Marker, usize)>,
}

/// The key of a label in the landings.
fn key(kind: Kind, id: &str) -> String {
    format!("\u{1}{}:{id}", kind.prefix())
}

impl Thesis {
    /// The thesis of a note and its text with the marks in it.
    pub(super) fn of(view: Arc<ThesisView>, text: &str) -> (Thesis, String) {
        let mut marks: Vec<Mark> = Vec::new();
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for span in view.spans(text) {
            if span.range.start < at {
                continue;
            }
            out.push_str(&text[at..span.range.start]);
            let mut mark = |text: &str, goto: Option<String>| {
                marks.push(Mark { text: text.to_string(), goto });
                out.push_str(&format!("{OPEN}{}{CLOSE}", marks.len() - 1));
            };
            match &span.shown {
                Shown::Label => {}
                Shown::Citation { text, keys, .. } => mark(text, keys.first().map(|k| format!("\u{1}cite:{k}"))),
                Shown::Reference { text, targets, .. } => mark(text, targets.first().map(|(k, id)| key(*k, id))),
                Shown::Broken { .. } => out.push_str(&text[span.range.clone()]),
            }
            at = span.range.end;
        }
        out.push_str(&text[at..]);
        (Thesis { view, marks, landings: HashMap::new(), float: Float::default(), lists: Vec::new(), boxes: Vec::new() }, out)
    }

    /// The mark whose number is `digits`.
    fn mark_at(&self, digits: &str) -> Option<&Mark> {
        digits.parse::<usize>().ok().and_then(|n| self.marks.get(n))
    }
}

/// The key and the number of the numbered equation on `lines`: `(key, "(2.1)")`.
pub(super) fn equation_number(view: &ThesisView, lines: &Range<usize>) -> Option<(String, String)> {
    let e = view.labels.entries.iter().find(|e| e.kind == Kind::Equation && e.source == view.index && lines.contains(&e.line))?;
    Some((key(Kind::Equation, &e.id), format!("({})", e.number.as_ref()?)))
}

impl Writer<'_> {
    /// The thesis document of the note, if it has syntax the rules read.
    pub(super) fn thesis_view(&self) -> Option<Arc<ThesisView>> {
        self.thesis.as_ref().map(|t| t.view.clone())
    }

    /// `text` with the marks it holds replaced by what they say.
    pub(super) fn unmark(&self, text: &str) -> String {
        let Some(t) = &self.thesis else { return text.to_string() };
        let mut out = String::new();
        let mut rest = text;
        while let Some(open) = rest.find(OPEN) {
            out.push_str(&rest[..open]);
            let tail = &rest[open + OPEN.len_utf8()..];
            let Some(close) = tail.find(CLOSE) else { return out };
            out.push_str(t.mark_at(&tail[..close]).map_or("", |m| m.text.as_str()));
            rest = &tail[close + CLOSE.len_utf8()..];
        }
        out + rest
    }

    /// The pieces of a line with the marks in their text put back: a reference is in the colour of
    /// links and jumps to its place.
    pub(super) fn expand_marks(&self, pieces: Vec<Piece>) -> Vec<Piece> {
        let Some(t) = &self.thesis else { return pieces };
        let mut out = Vec::with_capacity(pieces.len());
        for p in pieces {
            if p.math.is_some() || p.picture.is_some() || !p.text.contains(OPEN) {
                out.push(p);
                continue;
            }
            let mut rest = p.text.as_str();
            while let Some(open) = rest.find(OPEN) {
                if open > 0 {
                    out.push(Piece { text: rest[..open].to_string(), ..p.clone() });
                }
                let tail = &rest[open + OPEN.len_utf8()..];
                let Some(close) = tail.find(CLOSE) else {
                    rest = "";
                    break;
                };
                if let Some(m) = t.mark_at(&tail[..close]) {
                    let linked = m.goto.is_some() && !self.opts.paper_look;
                    out.push(Piece { text: m.text.clone(), jump: m.goto.clone(), color: if linked { print_color(theme::palette().link) } else { p.color }, ..p.clone() });
                }
                rest = &tail[close + CLOSE.len_utf8()..];
            }
            if !rest.is_empty() {
                out.push(Piece { text: rest.to_string(), ..p.clone() });
            }
        }
        out
    }

    /// Remember where the label `key` is: here.
    pub(super) fn land(&mut self, key: String) {
        let at = Dest { page: self.page, y: self.y };
        if let Some(t) = &mut self.thesis {
            t.landings.entry(key).or_insert(at);
        }
    }

    pub(super) fn thesis_boxes(&self) -> Vec<(Marker, usize)> {
        self.thesis.as_ref().map(|t| t.boxes.clone()).unwrap_or_default()
    }

    pub(super) fn thesis_landings(&self) -> HashMap<String, Dest> {
        self.thesis.as_ref().map(|t| t.landings.clone()).unwrap_or_default()
    }

    // ------------------------------------------------------------ captions

    fn caption_lines(&self, text: &str) -> Vec<Vec<Piece>> {
        let size = crate::caption::size(self.opts.font_pt);
        wrap_pieces(&[Piece::plain(text, size, GREY, false)], self.cw())
    }

    fn caption_height(&self, text: &str) -> f32 {
        self.caption_lines(text).iter().map(|l| self.line_height(l)).sum::<f32>() + crate::caption::GAP
    }

    /// A caption as a wrapped paragraph in the caption's size and grey.
    fn draw_caption(&mut self, text: &str) {
        let size = crate::caption::size(self.opts.font_pt);
        let (x, cw) = (self.left, self.cw());
        self.paragraph(&[Piece::plain(text, size, GREY, false)], x, cw);
    }

    /// The figure that is laid out next has this label: its caption goes above or below it.
    pub(super) fn set_float(&mut self, view: &ThesisView, entry: &Label) {
        let text = view.caption_text(entry);
        let above = view.caption_above(Kind::Figure);
        if let Some(t) = &mut self.thesis {
            t.float = Float { label: Some(key(Kind::Figure, &entry.id)), above: above.then(|| text.clone()), below: (!above).then_some(text) };
        }
    }

    pub(super) fn reset_float(&mut self) {
        if let Some(t) = &mut self.thesis {
            t.float = Float::default();
        }
    }

    /// The room the captions of the figure that is laid out next take.
    pub(super) fn float_room(&self) -> f32 {
        self.thesis.as_ref().map_or(0.0, |t| [&t.float.above, &t.float.below].into_iter().flatten().map(|c| self.caption_height(c)).sum())
    }

    /// The top of the figure: its label lands here, and its caption comes first when it stands above.
    pub(super) fn begin_float(&mut self) {
        let (label, above) = match &mut self.thesis {
            Some(t) => (t.float.label.take(), t.float.above.take()),
            None => return,
        };
        if let Some(label) = label {
            self.land(label);
        }
        if let Some(text) = above {
            self.draw_caption(&text);
            self.y += crate::caption::GAP;
        }
    }

    /// The bottom of the figure: its caption, when it stands below.
    pub(super) fn end_float(&mut self) {
        if let Some(text) = self.take_float_below() {
            self.y += crate::caption::GAP;
            self.draw_caption(&text);
        }
    }

    pub(super) fn take_float_below(&mut self) -> Option<String> {
        self.thesis.as_mut().and_then(|t| t.float.below.take())
    }

    pub(super) fn put_float_below(&mut self, text: Option<String>) {
        if let Some(t) = &mut self.thesis {
            t.float.below = text;
        }
    }

    /// A table laid out by `table`, with the caption of its label above or below it. `line` is the
    /// line under the table, where its caption line `: Caption {#tbl:x}` stands; gives whether it
    /// was one (the line is then not printed).
    pub(super) fn captioned_table(&mut self, view: Option<&ThesisView>, line: usize, table: impl FnOnce(&mut Self)) -> bool {
        let Some((v, entry)) = view.and_then(|v| v.label_on(Kind::Table, line).map(|e| (v, e))) else {
            table(self);
            return false;
        };
        let (text, above) = (v.caption_text(entry), v.caption_above(Kind::Table));
        let base = self.opts.font_pt;
        self.need(self.caption_height(&text) + base * 4.0);
        self.land(key(Kind::Table, &entry.id));
        if above {
            self.draw_caption(&text);
            self.y += crate::caption::GAP;
        }
        table(self);
        if !above {
            self.draw_caption(&text);
            self.y += base * 0.4;
        }
        true
    }

    // ------------------------------------------------------------ lists

    /// The list at a marker or at the end of the note: gives where its heading is.
    pub(super) fn thesis_block(&mut self, what: Marker, base: f32) -> Option<Dest> {
        let view = self.thesis_view()?;
        match what {
            Marker::Bibliography => self.bibliography(&view, base),
            Marker::ListOfFigures => self.reserve_float_list(&view, Kind::Figure, base),
            Marker::ListOfTables => self.reserve_float_list(&view, Kind::Table, base),
        }
    }

    /// The bibliography: an unnumbered heading in the language of the document and the entries as
    /// the style writes them, each after its label (`[1]`) when the style has one.
    fn bibliography(&mut self, view: &ThesisView, base: f32) -> Option<Dest> {
        let refs = view.references();
        if refs.is_empty() {
            return None;
        }
        let opts = self.opts;
        let scale = opts.heading_sizes.map_or_else(|| crate::editor::heading_scale(1), |s| s[0] / base);
        if self.y > self.top + 0.5 {
            self.y += base * 0.8;
        }
        self.need(base * scale * 2.0 + base * 3.0);
        let at = Dest { page: self.page, y: self.y };
        let mut head = Piece::plain(view.lang.bibliography_heading(), base * scale, opts.heading_color.unwrap_or(Rgba(0, 0, 0, 255)), opts.heading_bold).in_font(opts.headings());
        head.bold = opts.heading_bold;
        let (x, cw) = (self.left, self.cw());
        self.paragraph(&[head], x, cw);
        self.y += base * 0.4;
        let label_w = refs.iter().filter_map(|r| r.label.as_deref()).map(|l| crate::pdf::write::text_width(opts.font, base, l)).fold(0.0, f32::max);
        let indent = if label_w > 0.0 { label_w + base * 0.6 } else { 0.0 };
        let ink = Rgba(30, 30, 30, 255);
        for r in refs {
            let entry = [Piece::plain(&r.text, base, ink, false).in_font(opts.font)];
            let first = wrap_pieces(&entry, cw - indent).first().map_or(base * 1.45, |l| self.line_height(l));
            self.need(first);
            self.land(format!("\u{1}cite:{}", r.key));
            if let Some(label) = &r.label {
                let y = self.y + base * 1.08;
                self.p().text(TextStyle::plain(opts.font, base, GREY), x, y, label);
            }
            self.paragraph(&entry, x + indent, cw - indent);
            self.y += base * 0.3;
        }
        Some(at)
    }

    /// The title of a list of figures or of tables and the room for its lines, which are drawn at the end.
    fn reserve_float_list(&mut self, view: &ThesisView, kind: Kind, base: f32) -> Option<Dest> {
        let count = view.floats(kind).len();
        if count == 0 {
            return None;
        }
        self.need(base * 6.0);
        let (at, slots) = self.reserve_list(view.lang.list_heading(kind), &vec![0.0; count], false);
        if let Some(t) = &mut self.thesis {
            t.lists.push((kind, slots));
        }
        Some(at)
    }

    /// What is known when the note is laid out: the places of the section labels, and the lines of
    /// the lists of figures and tables with their pages.
    pub(super) fn finish_thesis(&mut self, view: &ThesisView, places: &HashMap<usize, Dest>) {
        let Some(t) = &mut self.thesis else { return };
        for e in view.labels.entries.iter().filter(|e| e.kind == Kind::Section && e.source == view.index) {
            if let Some(at) = places.get(&e.line) {
                t.landings.entry(key(Kind::Section, &e.id)).or_insert(*at);
            }
        }
        let lists = std::mem::take(&mut t.lists);
        let landings = t.landings.clone();
        for (kind, slots) in lists {
            let entries: Vec<Entry> = view.floats(kind).into_iter().map(|e| Entry { level: 1, label: view.caption_text(e), dest: landings.get(&key(kind, &e.id)).copied(), bold: false }).collect();
            self.draw_contents(&entries, &slots);
        }
    }
}
