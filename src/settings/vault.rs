//! The Vault tab: the folder, closing it and the people folder.

use super::Asks;
use crate::app::{Action, ShaleApp};
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
        crate::theme::section_break(ui);
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
        crate::theme::section(ui, "People");
        ui.horizontal(|ui| {
            ui.label("People folder");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut self.vdata.settings.people_dir).hint_text(crate::import::people::DEFAULT_DIR).desired_width(220.0))
                .on_hover_text("Contact notes live here. Type [[p and a name to link a person; imported e-mails and events find their people here.")
                .changed();
        });
        crate::theme::hint(ui, "Type [[p followed by a name to pick a person from this folder (or make a new contact note).");
        self.names_check_ui(ui);
        changed
    }

    /// "Check names for Windows and macOS…" and its list (VAULT-028).
    fn names_check_ui(&mut self, ui: &mut Ui) {
        crate::theme::section(ui, "Names on other systems");
        if self.vault_check.poll() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let running = self.vault_check.running();
        ui.horizontal(|ui| {
            if ui.add_enabled(!running && self.vault.is_some(), egui::Button::new("Check names for Windows and macOS…")).on_hover_text("Lists the files and folders whose names would not work on Windows or macOS").clicked() {
                self.start_vault_check();
            }
            if running {
                ui.spinner();
                crate::theme::hint(ui, "Checking…");
            }
        });
        if let Some(r) = self.vault_check.report.clone() {
            crate::theme::hint(ui, r);
        }
        let found = match &self.vault_check.result {
            None => return,
            Some(Err(e)) => {
                crate::theme::problem(ui, e.as_str());
                return;
            }
            Some(Ok(found)) if found.is_empty() => {
                ui.label("All names work on Windows and macOS.");
                return;
            }
            Some(Ok(found)) => found.clone(),
        };
        let fixable: Vec<usize> = (0..found.len()).filter(|i| found[*i].proposed.is_some()).collect();
        ui.label(format!("{} {} would not work on Windows or macOS:", found.len(), if found.len() == 1 { "name" } else { "names" }));
        if self.vault_check.confirm_all {
            ui.horizontal(|ui| {
                ui.label(format!("Rename {} {}? Links to them are updated.", fixable.len(), if fixable.len() == 1 { "file or folder" } else { "files and folders" }));
                if ui.button("Rename all").clicked() {
                    self.rename_findings(&fixable);
                }
                if ui.button("Cancel").clicked() {
                    self.vault_check.confirm_all = false;
                }
            });
        } else if ui.add_enabled(!fixable.is_empty() && !running, egui::Button::new("Rename all…")).clicked() {
            self.vault_check.confirm_all = true;
        }
        let mut one = None;
        egui::ScrollArea::vertical().id_salt("names-check").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
            for (i, f) in found.iter().enumerate() {
                ui.horizontal_wrapped(|ui| {
                    if ui.add_enabled(f.proposed.is_some() && !running, egui::Button::new("Rename").small()).clicked() {
                        one = Some(i);
                    }
                    ui.label(crate::vault::rel_to_string(&f.rel) + if f.is_dir { "/" } else { "" });
                    crate::theme::hint(ui, f.reasons.join("; "));
                    match &f.proposed {
                        Some(p) => ui.label(format!("→ {p}")),
                        None => crate::theme::hint(ui, "no name fits: shorten the folders above it"),
                    };
                });
            }
        });
        if let Some(i) = one {
            self.rename_findings(&[i]);
        }
    }
}
