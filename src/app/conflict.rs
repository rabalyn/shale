//! Conflict copies in the window (NET-015): the banner above a copy, the comparison with its
//! original, and "Keep this one" / "Keep the other". Nothing is ever deleted for good: the copy
//! goes to the vault's trash.

use super::{Action, ShaleApp};
use crate::history::compare_ui::{draw_row, source_view};
use crate::history::diff::{self, Row, Side};
use crate::history::state::Mode;
use crate::theme;
use crate::vault::{rel_to_string, Saved, Stamp};
use eframe::egui::{self, Align, Ui};
use std::path::{Path, PathBuf};

/// The comparison of a conflict copy with its original (NET-015); the modal's own state.
pub struct View {
    pub copy: PathBuf,
    pub original: PathBuf,
    pub client: &'static str,
    pub mode: Mode,
    rows: Vec<Row>,
    side: Vec<Side>,
    same: bool,
}

/// What the banner and the dialog ask for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ask {
    Compare,
    KeepThis,
    KeepOther,
}

/// The banner above a conflict copy: what it is, and the buttons when its original exists.
pub(crate) fn banner(ui: &mut Ui, client: &str, original: Option<&Path>) -> Option<Ask> {
    let mut asked = None;
    let p = theme::palette();
    egui::Frame::new().fill(p.faint_bg).stroke(egui::Stroke::new(1.0, p.border)).corner_radius(6.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match original {
            Some(o) => {
                let name = o.file_name().map(|n| crate::markdown::strip_md_ext(&n.to_string_lossy()).to_string()).unwrap_or_default();
                theme::paragraph(ui, format!("Another device changed this note at the same time. This is the copy {client} kept next to “{name}”; it is left out of search, links and the graph until you choose."));
                ui.horizontal(|ui| {
                    if ui.button("Compare").clicked() {
                        asked = Some(Ask::Compare);
                    }
                    if ui.button("Keep this one").on_hover_text("Its text replaces the original; the copy goes to the trash").clicked() {
                        asked = Some(Ask::KeepThis);
                    }
                    if ui.button("Keep the other").on_hover_text("The original stays as it is; the copy goes to the trash").clicked() {
                        asked = Some(Ask::KeepOther);
                    }
                });
            }
            None => {
                theme::paragraph(ui, format!("This looks like a conflict copy made by {client}, but the note it belongs to is not in this folder, so it is an ordinary note."));
            }
        }
    });
    ui.add_space(4.0);
    asked
}

impl ShaleApp {
    /// The text of a note: its open buffer, else the index.
    fn conflict_text(&self, rel: &Path) -> Option<String> {
        if let Some(d) = self.notes.get(rel) {
            return d.unreadable.is_none().then(|| d.text.clone());
        }
        self.vault.as_ref()?.notes.get(rel).filter(|n| n.unreadable.is_none()).map(|n| n.text.clone())
    }

    /// The copy and the original of a conflict copy, if it has one.
    fn conflict_pair(&self, copy: &Path) -> Option<(crate::conflict::Client, PathBuf)> {
        let c = self.vault.as_ref()?.conflict_of(copy)?;
        Some((c.client, c.original.clone()?))
    }

    /// Open the comparison of the copy with its original.
    pub(crate) fn open_conflict(&mut self, copy: &Path) {
        let Some((client, original)) = self.conflict_pair(copy) else { return };
        let (Some(old), Some(new)) = (self.conflict_text(&original), self.conflict_text(copy)) else {
            self.toast("One of the two notes could not be read, so they are not compared");
            return;
        };
        let view = View { copy: copy.to_path_buf(), original, client: client.label(), mode: Mode::Changes, rows: diff::compare(&old, &new), side: diff::side_by_side(&old, &new), same: old == new };
        self.modal = Some(super::Modal::Conflict(Box::new(view)));
    }

    /// "Keep the other": the original stays, the copy goes to the trash.
    pub(crate) fn conflict_keep_other(&mut self, copy: &Path) {
        if self.conflict_pair(copy).is_some() {
            self.trash(copy);
        }
    }

    /// "Keep this one": the copy's text goes over the original (in its open buffer as one undo step,
    /// else written like any save, DATA-003), the original opens, the copy goes to the trash.
    pub(crate) fn conflict_keep_this(&mut self, copy: &Path) {
        let Some((_, original)) = self.conflict_pair(copy) else { return };
        let name = rel_to_string(&original);
        let Some(text) = self.conflict_text(copy) else {
            self.toast(format!("“{}” could not be read, so nothing is changed", rel_to_string(copy)));
            return;
        };
        self.flush_dirty();
        if self.notes.get(copy).is_some_and(|d| d.dirty) {
            return;
        }
        {
            let Some(_tree) = self.hold_sync_tree() else { return };
            let saved = if let Some(doc) = self.notes.get_mut(&original) {
                if doc.unreadable.is_some() {
                    self.toast(format!("“{name}” could not be read, so it is not changed"));
                    return;
                }
                self.undo.of(&original).checkpoint(&doc.text);
                doc.text = text.clone();
                self.undo.of(&original).checkpoint(&doc.text);
                doc.dirty = true;
                doc.last_edit = std::time::Instant::now();
                self.save_note(&original);
                self.notes.get(&original).is_some_and(|d| !d.dirty)
            } else {
                let Some(v) = self.vault.as_mut() else { return };
                let base = crate::fsutil::read(v.abs(&original)).map(|b| Stamp::of_bytes(&b)).unwrap_or_default();
                match v.save_checked(&original, &text, base) {
                    Ok(Saved::Written(_)) => true,
                    Ok(Saved::Conflict { .. }) => false,
                    Err(e) => {
                        self.toast(format!("Could not change “{name}”: {e}"));
                        return;
                    }
                }
            };
            if !saved {
                self.toast(format!("“{name}” could not be saved yet, so the copy stays where it is"));
                return;
            }
        }
        self.open(&original, false);
        self.trash(copy);
        self.toast(format!("“{name}” has the text of the copy now; the copy is in the trash"));
    }

    /// Draw the comparison and do what was clicked; false: close it.
    pub(crate) fn conflict_dialog(&mut self, ctx: &egui::Context, view: &mut View) -> bool {
        let mut asked = None;
        let title = format!("Compare \u{2013} {}", rel_to_string(&view.copy));
        let resp = crate::dialog::Dialog::new("conflict", title).width(980.0).show(ctx, |ui| {
            asked = body(ui, view);
        });
        match asked {
            Some(Ask::KeepThis) => self.actions.push(Action::KeepConflict { copy: view.copy.clone(), this: true }),
            Some(Ask::KeepOther) => self.actions.push(Action::KeepConflict { copy: view.copy.clone(), this: false }),
            _ => {}
        }
        !resp.should_close() && asked.is_none()
    }
}

fn body(ui: &mut Ui, v: &mut View) -> Option<Ask> {
    let mut asked = None;
    let orig = rel_to_string(&v.original);
    ui.horizontal(|ui| {
        for (label, mode) in [("Changes", Mode::Changes), ("Source, side by side", Mode::Source)] {
            if ui.add(egui::Button::selectable(v.mode == mode, label)).clicked() {
                v.mode = mode;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
            if ui.add(egui::Button::new("Keep this copy").fill(theme::palette().accent.gamma_multiply(0.35))).on_hover_text("Its text replaces the original; the copy goes to the trash").clicked() {
                asked = Some(Ask::KeepThis);
            }
            if ui.button("Keep the original").on_hover_text("The original stays as it is; the copy goes to the trash").clicked() {
                asked = Some(Ask::KeepOther);
            }
        });
    });
    theme::hint(ui, format!("Left: “{orig}”. Right: this {} copy. The marks show what the copy changed.", v.client));
    ui.separator();
    if v.same {
        theme::hint(ui, "No changes: the copy has the same text as the other note.");
    }
    match v.mode {
        Mode::Changes => {
            egui::ScrollArea::vertical().id_salt("conflict-changes").max_height(ui.available_height().max(240.0)).auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                for row in &v.rows {
                    draw_row(ui, row);
                }
            });
        }
        Mode::Source => source_view(ui, &v.side, None),
    }
    asked
}
