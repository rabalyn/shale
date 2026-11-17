//! The Vault tab: the folder, closing it and the people folder.

use super::{section, Asks};
use crate::app::{muted, Action, ShaleApp};
use eframe::egui::{self, Ui};
use std::path::PathBuf;

impl ShaleApp {
    pub(super) fn vault_tab(&mut self, ui: &mut Ui, vault_path: &mut String, asks: &mut Asks) -> bool {
        let mut changed = false;
        ui.label("Vault folder");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(vault_path).desired_width(320.0));
            if ui.button("Open").clicked() {
                let p = PathBuf::from(crate::layout::shellexpand(vault_path));
                if p.is_dir() {
                    self.actions.push(Action::OpenVault(p));
                    asks.close = true;
                } else {
                    self.toasts.push(("That folder doesn't exist".into(), std::time::Instant::now()));
                }
            }
        });
        ui.add_space(8.0);
        if ui.button("Close this vault (back to the start screen)").clicked() {
            self.flush_all();
            self.remember_open_files();
            self.save_config();
            self.vdata = crate::vault_state::VaultData::default();
            self.media.stop();
            self.vault = None;
            self.tabbar.tabs.clear();
            self.cfg.vault = None;
            asks.close = true;
        }
        section(ui, "People");
        ui.horizontal(|ui| {
            ui.label("People folder");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.vdata.settings.people_dir).hint_text(crate::import::people::DEFAULT_DIR).desired_width(220.0))
                .on_hover_text("Contact notes live here. Type [[p and a name to link a person; imported e-mails and events find their people here.")
                .changed();
        });
        ui.label(muted("Type [[p followed by a name to pick a person from this folder (or make a new contact note).").size(12.0));
        changed
    }
}
