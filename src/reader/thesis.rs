//! The thesis syntax in the reading view (EDIT-127, ADR-0039, #646): the formatted citations, the
//! references as word and number, the labels hidden, the numbers and captions of figures, tables
//! and equations, and the bibliography. The numbers and the texts come from [`crate::thesis::View`],
//! the same that the built-in PDF takes, so what is read here is what is exported.

use super::*;
use crate::markdown::citations::{self, Group, Kind, Target, Token};
use crate::markdown::labels::Entry;
use crate::thesis::{CaptionAt, View};

/// The mark of a span of a citation, a reference or a hidden label: its `target` is the text of its tooltip.
const MARK: &str = "\u{1}thesis";

fn span(range: Range<usize>, shown: String, tip: String) -> LinkSpan {
    LinkSpan { target_range: range.clone(), range, target: tip, heading: Some(MARK.to_string()), alias: Some(shown), embed: false, markdown_style: false }
}

/// The tooltip of a span that stands for a citation, a reference or a label, if it is one.
pub(super) fn tip_of(l: &LinkSpan) -> Option<&str> {
    (l.heading.as_deref() == Some(MARK)).then_some(l.target.as_str())
}

/// The spans of the citations, references and labels of the note, to be drawn in the place of their
/// text: the formatted citation, the word and number of a reference, nothing for a label. A group
/// that does not resolve is its own text, as a link that does not resolve. In the order of the text.
pub(super) fn spans(text: &str, view: &View) -> Vec<(LinkSpan, bool)> {
    let has = |k: &str| view.bib.has_key(k);
    let formatted = view.formatted.as_ref();
    let mut out = Vec::new();
    for token in citations::scan(text, view.bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool)) {
        match token {
            Token::Label(l) => out.push((span(l.range, String::new(), String::new()), true)),
            Token::Group(g) => {
                let range = g.range.clone();
                let as_written = text[range.clone()].to_string();
                if g.items.iter().any(|i| matches!(i.target, Target::Citation(_))) {
                    if let Some(shown) = formatted.and_then(|f| f.groups.iter().find(|(r, _)| *r == range)).map(|(_, t)| t.clone()) {
                        out.push((span(range, shown, entries_of(&g, view)), true));
                    } else if let Some(Target::Citation(key)) = g.items.iter().find(|i| i.known == Some(false)).map(|i| &i.target) {
                        out.push((span(range, as_written, format!("“{key}” is not in the bibliography")), false));
                    }
                } else if g.items.iter().any(|i| matches!(&i.target, Target::Reference { kind, .. } if kind.is_labelled())) {
                    match view.reference_text(&g) {
                        Some(shown) => out.push((span(range, shown, captions_of(&g, view)), true)),
                        None => {
                            let missing = g.items.iter().find_map(|i| match &i.target {
                                Target::Reference { kind, id } if kind.is_labelled() && view.labels.get(*kind, id).is_none() => Some(format!("There is no label {{#{}:{id}}} in the document", kind.prefix())),
                                _ => None,
                            });
                            out.push((span(range, as_written, missing.unwrap_or_default()), false));
                        }
                    }
                }
            }
        }
    }
    out
}

/// The entries a citation group cites, as the style writes them, one to a line: its tooltip.
fn entries_of(g: &Group, view: &View) -> String {
    let Some(f) = &view.formatted else { return String::new() };
    g.items
        .iter()
        .filter_map(|i| match &i.target {
            Target::Citation(key) => f.references.iter().find(|r| r.key == *key).map(|r| format!("{}{}", r.label.as_deref().map(|l| format!("{l} ")).unwrap_or_default(), r.text)),
            Target::Reference { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The captions of the labels a group of references names: its tooltip.
fn captions_of(g: &Group, view: &View) -> String {
    g.items
        .iter()
        .filter_map(|i| match &i.target {
            Target::Reference { kind, id } => view.labels.get(*kind, id).filter(|e| !e.caption.is_empty()).map(|e| e.caption.clone()),
            Target::Citation(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `links` and the spans of the thesis syntax in the order of the text; a span that lies on a link is dropped (the link wins).
pub(super) fn merge(mut links: Vec<(LinkSpan, bool)>, spans: Vec<(LinkSpan, bool)>) -> Vec<(LinkSpan, bool)> {
    let taken: Vec<Range<usize>> = links.iter().map(|(l, _)| l.range.clone()).collect();
    links.extend(spans.into_iter().filter(|(s, _)| !taken.iter().any(|r| r.start < s.range.end && s.range.start < r.end)));
    links.sort_by_key(|(l, _)| l.range.start);
    links
}

/// The label that ends the line `line` (a figure's embed has it after it), if the document has it
/// as a label of `kind` of this note.
pub(super) fn label_on(view: &View, kind: Kind, line: usize) -> Option<&Entry> {
    view.labels.entries.iter().find(|e| e.kind == kind && e.source == view.index && e.line == line)
}

/// The number of the display equation on `lines`, in brackets, as it stands at the right margin.
pub(super) fn equation_number(view: &View, lines: &Range<usize>) -> Option<String> {
    view.labels.entries.iter().find(|e| e.kind == Kind::Equation && e.source == view.index && lines.contains(&e.line)).and_then(|e| e.number.as_ref()).map(|n| format!("({n})"))
}

/// The line of text a figure's or table's caption is: "Figure 3: Results of run 2".
pub(super) fn caption_text(entry: &Entry, view: &View) -> String {
    let label = crate::thesis::caption_label(entry, view.lang);
    if entry.caption.is_empty() {
        label
    } else {
        format!("{label}: {}", entry.caption)
    }
}

/// Draw a caption line.
pub(super) fn caption(ui: &mut Ui, entry: &Entry, view: &View, base: f32) {
    ui.label(egui::RichText::new(caption_text(entry, view)).size(crate::caption::size(base)).color(theme::palette().muted));
}

/// Whether figures' or tables' captions go above (the properties `figure-caption:` and `table-caption:`).
pub(super) fn above(view: &View, kind: Kind) -> bool {
    (if kind == Kind::Table { view.captions.table } else { view.captions.figure }) == CaptionAt::Above
}

/// The line is the marker that puts the bibliography here: `<!-- shale: bibliography -->`.
pub(super) fn is_bibliography_marker(trimmed: &str) -> bool {
    trimmed.strip_prefix("<!--").and_then(|r| r.strip_suffix("-->")).is_some_and(|inner| inner.split_whitespace().collect::<Vec<_>>() == ["shale:", "bibliography"])
}

/// Draw the bibliography: an unnumbered heading in the language of the document, and the entries as
/// the style writes them, each with its label (`[1]`) when the style has one.
pub(super) fn bibliography(ui: &mut Ui, view: &View, base: f32) {
    let Some(f) = view.formatted.as_ref().filter(|f| !f.references.is_empty()) else { return };
    ui.add_space(base * 0.5);
    ui.label(egui::RichText::new(view.lang.bibliography_heading()).size(base * heading_scale(1)).color(theme::palette().heading));
    ui.add_space(base * 0.4);
    for r in &f.references {
        ui.horizontal_top(|ui| {
            if let Some(label) = &r.label {
                ui.label(egui::RichText::new(label).size(base).color(theme::palette().muted));
            }
            ui.add(egui::Label::new(egui::RichText::new(&r.text).size(base).color(theme::palette().text)).wrap());
        });
        ui.add_space(base * 0.3);
    }
}

/// The number of an equation at the right margin of the row `rect` of a displayed formula.
pub(super) fn paint_equation_number(ui: &Ui, rect: egui::Rect, number: &str, base: f32) {
    ui.painter().text(egui::Pos2::new(rect.right(), rect.center().y), egui::Align2::RIGHT_CENTER, number, FontId::proportional(base), theme::palette().text);
}
