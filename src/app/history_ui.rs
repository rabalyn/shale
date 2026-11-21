//! The dialog of the deleted notes (SYNC-040, SYNC-045) and the history part of Settings → Sync.
//! The panel and the comparison view are in `history/` (HIST-001…).

use super::ShaleApp;
use crate::gitsync::history::Deleted;
use crate::history::state::DeletedList;
use crate::theme;
use eframe::egui::{self, Ui};

impl ShaleApp {
    /// Draw the list of the deleted notes and do what was clicked; false: close it.
    pub(crate) fn history_dialog(&mut self, ctx: &egui::Context) -> bool {
        let mut dlg = std::mem::take(&mut self.history.deleted);
        dlg.rects.clear();
        let mut asked: Option<Deleted> = None;
        let resp = crate::dialog::Dialog::new("history", "Deleted notes").width(620.0).show(ctx, |ui| {
            asked = deleted_body(ui, &mut dlg);
        });
        self.history.deleted = dlg;
        if let Some(d) = asked {
            self.history_restore_deleted(&d);
        }
        !resp.should_close()
    }
}

fn deleted_body(ui: &mut Ui, dlg: &mut DeletedList) -> Option<Deleted> {
    let mut asked = None;
    if dlg.loading {
        theme::hint(ui, "Reading the history…");
    }
    if let Some(e) = &dlg.error {
        ui.label(egui::RichText::new(e).color(theme::palette().link_missing));
    }
    let busy = dlg.loading;
    if dlg.list.is_empty() && !busy && dlg.error.is_none() {
        theme::hint(ui, "No deleted notes in the history.");
    }
    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
        for (i, d) in dlg.list.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(crate::vault::rel_to_string(&d.path));
                ui.label(egui::RichText::new(format!("deleted {}", super::history::when(d.time))).color(theme::palette().muted).size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = ui.add_enabled(!busy, egui::Button::new("Restore"));
                    dlg.rects.push((format!("restore-deleted-{i}"), b.rect));
                    if b.clicked() {
                        asked = Some(d.clone());
                    }
                });
            });
        }
    });
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
