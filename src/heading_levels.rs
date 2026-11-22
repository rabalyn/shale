//! The bar above a note that warns when its headings skip levels, with the button that moves them
//! all up (EDIT-115, EDIT-116, EDIT-117). Which lines are headings is the outline's rule
//! ([`markdown::find_headings`]); the shift is the paste dialog's ([`markdown::lift_headings`]).

use crate::app::ShaleApp;
use crate::markdown;
use crate::theme;
use eframe::egui::{self, CornerRadius, RichText, Ui};
use std::path::Path;

/// What is wrong with the levels of a note's headings (EDIT-115).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub message: String,
    /// how many levels the button moves every heading up: the shallowest level found minus 1
    pub shift: usize,
}

/// The warning for the headings in `text`; `None` when it has none, or their levels run from 1 to the
/// deepest found with no level missing in between.
pub fn check(text: &str) -> Option<Warning> {
    let mut found = [false; 7];
    for (_, level, _) in markdown::find_headings(text) {
        found[level] = true;
    }
    let shallowest = (1..=6).find(|&l| found[l])?;
    let deepest = (1..=6).rev().find(|&l| found[l])?;
    let skipped: Vec<usize> = (shallowest..deepest).filter(|&l| !found[l]).collect();
    let mut parts = Vec::new();
    if shallowest > 1 {
        parts.push("no level 1 heading".to_string());
    }
    match skipped.as_slice() {
        [] => {}
        [one] => parts.push(format!("level {one} is skipped")),
        [rest @ .., last] => parts.push(format!("levels {} and {last} are skipped", rest.iter().map(usize::to_string).collect::<Vec<_>>().join(", "))),
    }
    (!parts.is_empty()).then(|| Warning { message: format!("Headings: {}", parts.join(", ")), shift: shallowest - 1 })
}

/// The bar's state, kept with the note's buffer: the choice to hide it and the last text checked.
#[derive(Default)]
pub struct Bar {
    /// hidden for this note until Shale quits (EDIT-117)
    dismissed: bool,
    /// the text's length and fast hash, and the warning for it
    seen: Option<(usize, u64, Option<Warning>)>,
}

impl Bar {
    /// The warning for `text`, worked out again only for another text.
    fn warning(&mut self, text: &str) -> Option<Warning> {
        let hash = crate::fold_ui::hash_of(text);
        match &self.seen {
            Some((len, h, w)) if *len == text.len() && *h == hash => w.clone(),
            _ => {
                let w = check(text);
                self.seen = Some((text.len(), hash, w.clone()));
                w
            }
        }
    }

    /// Draw the bar when `text` needs it and it is not hidden; true when the button that moves the
    /// headings up was pressed (the change is [`ShaleApp::shift_headings`]'s).
    pub fn show(&mut self, ui: &mut Ui, text: &str) -> bool {
        if self.dismissed {
            return false;
        }
        let Some(warning) = self.warning(text) else { return false };
        let p = theme::palette();
        let mut shift = false;
        egui::Frame::new().fill(p.warn_bg).corner_radius(CornerRadius::same(4)).inner_margin(egui::Margin::symmetric(10, 2)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(&warning.message).size(12.5).color(p.warn));
                if warning.shift > 0 {
                    let by = warning.shift;
                    let moved = ui.small_button(format!("Move all up by {by}")).on_hover_text(format!("Take {by} # off the start of every heading, so the highest becomes #"));
                    shift = moved.clicked();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("×").on_hover_text("Hide this warning for this note").clicked() {
                        self.dismissed = true;
                    }
                });
            });
        });
        shift
    }
}

impl ShaleApp {
    /// The button of the bar (EDIT-116): every heading of the note `rel` moves up so that the shallowest
    /// becomes `#`, in the open buffer as one step of its undo history (EDIT-021), saved like any edit.
    pub fn shift_headings(&mut self, rel: &Path) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        let Some(doc) = self.notes.get_mut(rel).filter(|d| d.unreadable.is_none()) else { return };
        let Some(text) = markdown::lift_headings(&doc.text) else { return };
        self.undo.of(rel).checkpoint(&doc.text);
        doc.text = text;
        self.undo.of(rel).checkpoint(&doc.text);
        doc.dirty = true;
        doc.last_edit = std::time::Instant::now();
        if let Some(v) = self.vault.as_mut() {
            v.set_note_text(rel, &doc.text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(text: &str) -> Option<(String, usize)> {
        check(text).map(|w| (w.message, w.shift))
    }

    // spec: EDIT-115
    #[test]
    fn edit_115_the_warning_names_the_missing_levels_up_to_the_deepest_found() {
        assert_eq!(message("## A\n### B\n"), Some(("Headings: no level 1 heading".into(), 1)));
        assert_eq!(message("### A\n"), Some(("Headings: no level 1 heading".into(), 2)), "a lone deep heading: all levels above it are one thing");
        assert_eq!(message("# A\n### B\n"), Some(("Headings: level 2 is skipped".into(), 0)), "a gap and a `#`: no shift");
        assert_eq!(message("## A\n#### B\n"), Some(("Headings: no level 1 heading, level 3 is skipped".into(), 1)));
        assert_eq!(message("# A\n#### B\n"), Some(("Headings: levels 2 and 3 are skipped".into(), 0)));
        assert_eq!(message("# A\n###### B\n"), Some(("Headings: levels 2, 3, 4 and 5 are skipped".into(), 0)));
    }

    // spec: EDIT-115
    #[test]
    fn edit_115_no_headings_and_complete_levels_are_quiet() {
        for text in ["", "just text\n#nospace\n", "# A\n", "# A\n## B\n### C\n", "# A\n## B\n# C\n", "# A\n## B\n# C\n## D\n", "---\ntitle: x\n---\n# A\n"] {
            assert_eq!(check(text), None, "{text:?}");
        }
    }

    // spec: EDIT-115
    #[test]
    fn edit_115_headings_in_code_math_and_front_matter_are_not_counted() {
        let text = "---\n# yaml\n---\n# A\n```\n### code\n```\n$$\n### math\n$$\n~~~\n#### tilde\n~~~\n## B\n";
        assert_eq!(check(text), None, "only # and ## count");
        assert_eq!(message("```\n# code\n```\n## A\n").map(|m| m.1), Some(1), "the # in code does not make level 1");
        assert_eq!(message("$$\n# math\n$$\n### A\n").map(|m| m.1), Some(2));
        assert_eq!(message("---\n# yaml\n---\n## A\n").map(|m| m.1), Some(1));
        assert_eq!(check("A setext title\n===\n\nand another\n---\n"), None, "an underline makes no heading: the parser knows only #");
    }
}
