//! The thesis syntax in the reading view (EDIT-127, ADR-0039, #646): the formatted citations, the
//! references as word and number, the labels hidden, the numbers and captions of figures, tables
//! and equations, and the bibliography. The numbers and the texts come from [`crate::thesis::View`],
//! the same that the built-in PDF takes, so what is read here is what is exported.

use super::*;
use crate::markdown::labels::Entry;
use crate::thesis::{Shown, View};

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
/// What each is comes from [`View::spans`], the same the built-in PDF and the site take.
pub(super) fn spans(text: &str, view: &View) -> Vec<(LinkSpan, bool)> {
    view.spans(text)
        .into_iter()
        .map(|s| match s.shown {
            Shown::Label => (span(s.range, String::new(), String::new()), true),
            Shown::Citation { text, tip, .. } | Shown::Reference { text, tip, .. } => (span(s.range, text, tip), true),
            Shown::Broken { tip } => (span(s.range.clone(), text[s.range].to_string(), tip), false),
        })
        .collect()
}

/// `links` and the spans of the thesis syntax in the order of the text; a span that lies on a link is dropped (the link wins).
pub(super) fn merge(mut links: Vec<(LinkSpan, bool)>, spans: Vec<(LinkSpan, bool)>) -> Vec<(LinkSpan, bool)> {
    let taken: Vec<Range<usize>> = links.iter().map(|(l, _)| l.range.clone()).collect();
    links.extend(spans.into_iter().filter(|(s, _)| !taken.iter().any(|r| r.start < s.range.end && s.range.start < r.end)));
    links.sort_by_key(|(l, _)| l.range.start);
    links
}

/// Draw a caption line.
pub(super) fn caption(ui: &mut Ui, entry: &Entry, view: &View, base: f32) {
    ui.label(egui::RichText::new(view.caption_text(entry)).size(crate::caption::size(base)).color(theme::palette().muted));
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
