//! Citations, references and labels in the editor (EDIT-125, EDIT-126, #646): which of them the
//! text holds and whether each is fine (a link that resolves) or broken (an unknown key, a label
//! that does not exist, a label that comes twice), and the peek of one.

use crate::bibliography::format;
use crate::markdown::citations::{self, scan, Kind, Target, Token};
use crate::thesis::View;
use eframe::egui::{self, Ui};
use std::collections::HashSet;
use std::ops::Range;
use std::sync::Arc;

/// What a mark stands for, for its peek and its tooltip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Peek {
    /// a key that the bibliography has
    Citation(String),
    /// a label that exists
    Reference(Kind, String),
    /// what is wrong with it
    Problem(String),
}

/// A citation key, a reference or a duplicate label of the text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CiteMark {
    /// the bytes of `@key`, `@fig:x` or `{#fig:x}`
    pub range: Range<usize>,
    /// drawn as a link that resolves; else as a broken link
    pub ok: bool,
    pub peek: Peek,
}

/// The marks of `text` for the document `view` describes, in the order of the text. Nothing is
/// marked without a bibliography for the keys (the group is plain text then, ADR-0039 §4 rule 5);
/// a bare `@word` that is no key is no citation and is not marked (rule 4).
pub(crate) fn marks(text: &str, view: &View) -> Vec<CiteMark> {
    let has = |k: &str| view.bib.has_key(k);
    let mut seen: HashSet<(Kind, String)> = HashSet::new();
    let mut out = Vec::new();
    for token in scan(text, view.bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool)) {
        match token {
            Token::Group(group) => {
                for item in group.items {
                    match item.target {
                        Target::Citation(key) => match item.known {
                            Some(true) => out.push(CiteMark { range: item.range, ok: true, peek: Peek::Citation(key) }),
                            Some(false) => out.push(CiteMark { range: item.range, ok: false, peek: Peek::Problem(format!("“{key}” is not in the bibliography")) }),
                            None => {}
                        },
                        Target::Reference { kind, id } if kind.is_labelled() => {
                            if view.labels.get(kind, &id).is_some() {
                                out.push(CiteMark { range: item.range, ok: true, peek: Peek::Reference(kind, id) });
                            } else {
                                out.push(CiteMark { range: item.range, ok: false, peek: Peek::Problem(format!("There is no label {{#{}:{id}}} in the document", kind.prefix())) });
                            }
                        }
                        Target::Reference { .. } => {}
                    }
                }
            }
            Token::Label(label) => {
                // the first label of a kind and id wins: a second in this note, or one in an earlier note of the book, is the duplicate
                let earlier = view.labels.entries.iter().any(|e| e.kind == label.kind && e.id == label.id && e.source < view.index);
                if !seen.insert((label.kind, label.id.clone())) || earlier {
                    out.push(CiteMark { range: label.range, ok: false, peek: Peek::Problem(format!("The label {{#{}:{}}} is already used: the first one counts", label.kind.prefix(), label.id)) });
                }
            }
        }
    }
    out
}

/// The marks of the text last asked about, found again only when the text or the document changes.
#[derive(Default)]
pub(crate) struct Cache {
    last: Option<(String, Arc<View>, Arc<Vec<CiteMark>>)>,
}

impl Cache {
    pub fn get(&mut self, text: &str, view: &Arc<View>) -> Arc<Vec<CiteMark>> {
        if let Some((t, v, marks)) = &self.last {
            if t == text && Arc::ptr_eq(v, view) {
                return marks.clone();
            }
        }
        // a text with no `@` and no label has none of them
        let found = if text.contains('@') || text.contains("{#") { marks(text, view) } else { Vec::new() };
        let found = Arc::new(found);
        self.last = Some((text.to_string(), view.clone(), found.clone()));
        found
    }
}

/// The ranges and whether each is fine, for the layout (EDIT-126).
pub(crate) fn ranges(marks: &[CiteMark]) -> Vec<(Range<usize>, bool)> {
    marks.iter().map(|m| (m.range.clone(), m.ok)).collect()
}

/// `range` (an embed) with the label of a figure that follows it (`![[plot.png|Caption]]{#fig:x}`,
/// one space allowed), so that the label is hidden with the link and the picture's row (EDIT-127).
pub(crate) fn with_label(text: &str, range: Range<usize>) -> Range<usize> {
    let Some(rest) = text.get(range.end..) else { return range };
    let space = usize::from(rest.starts_with(' '));
    match citations::parse_label(&rest[space..]) {
        Some((Kind::Figure, _, len)) => range.start..range.end + space + len,
        _ => range,
    }
}

/// The mark under byte `at` of the text.
pub(crate) fn at(marks: &[CiteMark], at: usize) -> Option<&CiteMark> {
    marks.iter().find(|m| m.range.contains(&at))
}

/// The content of the peek of a mark (EDIT-125): the entry of a citation (authors, year, title,
/// venue), the number and caption of a reference, or what is wrong.
pub(crate) fn peek_ui(ui: &mut Ui, mark: &CiteMark, view: &View) {
    let palette = crate::theme::palette();
    match &mark.peek {
        Peek::Citation(key) => {
            ui.label(egui::RichText::new(key).strong().color(palette.link));
            if let Some(entry) = view.bib.entry(key) {
                ui.label(egui::RichText::new(format::summary(entry)).color(palette.text));
                if !entry.kind.is_empty() {
                    ui.label(egui::RichText::new(&entry.kind).small().color(palette.faint));
                }
            }
        }
        Peek::Reference(kind, id) => {
            if let Some(entry) = view.labels.get(*kind, id) {
                ui.label(egui::RichText::new(crate::thesis::reference(entry, view.lang)).strong().color(palette.link));
                if !entry.caption.is_empty() {
                    ui.label(egui::RichText::new(&entry.caption).color(palette.text));
                }
            }
        }
        Peek::Problem(why) => {
            ui.label(egui::RichText::new(why).color(palette.link_missing));
        }
    }
}
