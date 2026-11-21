//! The list dialog of the file history (SYNC-045): the versions of a note, the deleted notes and
//! an old text read-only. A plain list that the history panel of #472 replaces; it calls the same
//! actions (`history_restore_version` and the others).

use super::history::{reason_label, when, Tab};
use super::ShaleApp;
use crate::gitsync::history::{Deleted, Version};
use crate::theme;
use eframe::egui::{self, Ui};

/// What a click in the dialog asked for.
enum Do {
    Restore(Version),
    Copy(Version),
    View(Version),
    RestoreDeleted(Deleted),
    Show(Tab),
    Back,
    CopyShown(String),
}

impl ShaleApp {
    /// Draw the dialog and do what was clicked; false: close it.
    pub(crate) fn history_dialog(&mut self, ctx: &egui::Context) -> bool {
        let mut dlg = std::mem::take(&mut self.history.dialog);
        dlg.rects.clear();
        let mut asked: Option<Do> = None;
        let title = match &dlg.tab {
            Some(Tab::Note(rel)) => format!("Versions of {}", crate::vault::rel_to_string(rel)),
            _ => "Deleted notes".to_string(),
        };
        let note = dlg.note.clone();
        let resp = crate::dialog::Dialog::new("history", title).width(620.0).show(ctx, |ui| {
            asked = history_body(ui, &mut dlg, note.as_deref());
        });
        self.history.dialog = dlg;
        match asked {
            Some(Do::Restore(v)) => {
                if let Some(Tab::Note(rel)) = self.history.dialog.tab.clone() {
                    self.history_restore_version(&rel, &v);
                }
            }
            Some(Do::Copy(v)) => self.history_copy_version(&v),
            Some(Do::View(v)) => self.history_view_version(&v),
            Some(Do::RestoreDeleted(d)) => self.history_restore_deleted(&d),
            Some(Do::Show(Tab::Note(rel))) => self.open_history_versions(&rel),
            Some(Do::Show(Tab::Deleted)) => self.open_history_deleted(),
            Some(Do::Back) => self.history.dialog.view = None,
            Some(Do::CopyShown(text)) => ctx.copy_text(text),
            None => {}
        }
        !resp.should_close()
    }
}

fn history_body(ui: &mut Ui, dlg: &mut super::history::HistoryDialog, note: Option<&std::path::Path>) -> Option<Do> {
    let mut asked = None;
    if let Some((title, text)) = &dlg.view {
        ui.label(egui::RichText::new(title).strong());
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            let mut shown: &str = text;
            ui.add(egui::TextEdit::multiline(&mut shown).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace));
        });
        ui.horizontal(|ui| {
            let back = ui.button("Back to the list");
            dlg.rects.push(("back".into(), back.rect));
            if back.clicked() {
                asked = Some(Do::Back);
            }
            let copy = ui.button("Copy text");
            dlg.rects.push(("copy-shown".into(), copy.rect));
            if copy.clicked() {
                asked = Some(Do::CopyShown(text.clone()));
            }
        });
        return asked;
    }
    ui.horizontal(|ui| {
        if let Some(rel) = note {
            let on = matches!(&dlg.tab, Some(Tab::Note(r)) if r == rel);
            let b = ui.selectable_label(on, "This note");
            dlg.rects.push(("tab-note".into(), b.rect));
            if b.clicked() && !on {
                asked = Some(Do::Show(Tab::Note(rel.to_path_buf())));
            }
        }
        let on = dlg.tab == Some(Tab::Deleted);
        let b = ui.selectable_label(on, "Deleted notes");
        dlg.rects.push(("tab-deleted".into(), b.rect));
        if b.clicked() && !on {
            asked = Some(Do::Show(Tab::Deleted));
        }
    });
    ui.separator();
    if dlg.loading {
        theme::hint(ui, "Reading the history…");
    }
    if let Some(e) = &dlg.error {
        ui.label(egui::RichText::new(e).color(theme::palette().link_missing));
    }
    let busy = dlg.loading;
    match dlg.tab.clone() {
        Some(Tab::Note(_)) => {
            if dlg.versions.is_empty() && !busy {
                theme::hint(ui, "No versions yet. Shale keeps a version when you save a note, and when you stop typing.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, v) in dlg.versions.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(when(v.time));
                        ui.label(egui::RichText::new(reason_label(v.meta.reason)).color(theme::palette().muted));
                        if let Some(d) = &v.meta.device {
                            ui.label(egui::RichText::new(d).color(theme::palette().muted).size(12.0));
                        }
                        ui.label(match v.lines {
                            Some((a, r)) => format!("+{a} \u{2212}{r}"),
                            None => "binary".to_string(),
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            for (key, label, make) in [("view", "Read-only", Do::View as fn(Version) -> Do), ("copy", "Copy text", Do::Copy), ("restore", "Restore", Do::Restore)] {
                                let b = ui.add_enabled(!busy, egui::Button::new(label));
                                dlg.rects.push((format!("{key}-{i}"), b.rect));
                                if b.clicked() {
                                    asked = Some(make(v.clone()));
                                }
                            }
                        });
                    });
                }
            });
        }
        _ => {
            if dlg.deleted.is_empty() && !busy {
                theme::hint(ui, "No deleted notes in the history.");
            }
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for (i, d) in dlg.deleted.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(crate::vault::rel_to_string(&d.path));
                        ui.label(egui::RichText::new(format!("deleted {}", when(d.time))).color(theme::palette().muted).size(12.0));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let b = ui.add_enabled(!busy, egui::Button::new("Restore"));
                            dlg.rects.push((format!("restore-deleted-{i}"), b.rect));
                            if b.clicked() {
                                asked = Some(Do::RestoreDeleted(d.clone()));
                            }
                        });
                    });
                }
            });
        }
    }
    asked
}

impl ShaleApp {
    /// The "File history" part of Settings → Sync (SYNC-032, CFG-066). Returns whether a setting changed.
    pub(crate) fn history_settings_ui(&mut self, ui: &mut Ui, asks: &mut crate::settings::Asks) -> bool {
        let mut changed = false;
        ui.add_space(8.0);
        ui.label(egui::RichText::new("File history").color(theme::palette().muted));
        let r = ui.checkbox(&mut self.cfg.history.enabled, "Keep earlier versions of my notes, so that I can go back");
        self.sync.rects.push(("history-enabled", r.rect));
        changed |= r.changed();
        match self.history_off_reason() {
            Some(why) => {
                theme::hint(ui, why);
            }
            None => {
                let place = match self.vault.as_ref() {
                    Some(v) if crate::gitsync::is_repo(&v.root) => "in the git repository of the vault (the .git folder)".to_string(),
                    _ => format!("on this computer, outside the vault, in {}", self.usage.places.history.display()),
                };
                theme::hint(ui, format!("Versions are kept {place}. Nothing is written into the vault except a small file .shale/history-id."));
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("At most one version of a note every");
            let r = ui.add(egui::DragValue::new(&mut self.cfg.history.coalesce_minutes).range(1..=1440));
            self.sync.rects.push(("history-minutes", r.rect));
            changed |= r.changed();
            ui.label("minutes while you type, and one when a note has had no change for");
            let r = ui.add(egui::DragValue::new(&mut self.cfg.history.idle_seconds).range(5..=3600));
            self.sync.rects.push(("history-idle", r.rect));
            changed |= r.changed();
            ui.label("seconds.");
        });
        if self.vault.is_some() {
            let b = ui.add_enabled(self.cfg.history.enabled, egui::Button::new("Restore a deleted note…"));
            self.sync.rects.push(("history-deleted", b.rect));
            if b.clicked() {
                asks.history_deleted = true;
            }
        }
        changed
    }
}
