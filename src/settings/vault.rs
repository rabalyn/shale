//! The Vault tab: the folder, closing it and the people folder.

use super::anchor::anchor;
use super::Asks;
use crate::app::{Action, ShaleApp};
use eframe::egui::{self, Ui};
use std::path::PathBuf;

impl ShaleApp {
    pub(super) fn vault_tab(&mut self, ui: &mut Ui, vault_path: &mut String, asks: &mut Asks) -> bool {
        let mut changed = false;
        anchor(ui, "vault.folder", |ui| {
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
        });
        crate::theme::section_break(ui);
        if anchor(ui, "vault.close", |ui| ui.button("Close this vault (back to the start screen)").clicked()) {
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
        changed |= anchor(ui, "vault.people_dir", |ui| {
            ui.horizontal(|ui| {
                ui.label("People folder");
                ui.add(egui::TextEdit::singleline(&mut self.vdata.settings.people_dir).hint_text(crate::import::people::DEFAULT_DIR).desired_width(220.0))
                    .on_hover_text("Contact notes live here. Type [[p and a name to link a person; imported e-mails and events find their people here.")
                    .changed()
            })
            .inner
        });
        crate::theme::hint(ui, "Type [[p followed by a name to pick a person from this folder (or make a new contact note).");
        crate::theme::section(ui, "Bibliography");
        changed |= anchor(ui, "vault.bibliography", |ui| {
            ui.horizontal(|ui| {
                ui.label("Bibliography file");
                ui.add(egui::TextEdit::singleline(&mut self.vdata.settings.bibliography).hint_text("refs.bib").desired_width(220.0))
                    .on_hover_text("The .bib file that notes use for citations such as [@key] when they have no property bibliography: of their own.")
                    .changed()
            })
            .inner
        });
        crate::theme::hint(ui, "A BibLaTeX or BibTeX file of the vault, as Zotero, JabRef or Citavi export it: a path, a file name or [[refs.bib]]. A note's own property bibliography: wins.");
        self.bibliography_status_ui(ui);
        self.names_check_ui(ui);
        self.outside_check_ui(ui);
        changed
    }

    /// What the bibliography of the default and of the open note is: the files, their entries, the
    /// skipped ones, the keys of the note that the file lacks (EDIT-123).
    fn bibliography_status_ui(&self, ui: &mut Ui) {
        let mut lines = if self.vdata.settings.bibliography.trim().is_empty() { vec![] } else { crate::bibliography::report::default_lines(&self.bib_default_effect()) };
        if let Some((name, text, effect)) = self.bib_note_effect() {
            lines.extend(crate::bibliography::report::note_lines(&name, &text, &effect));
        }
        lines.dedup();
        for line in lines {
            crate::theme::hint(ui, &line);
        }
    }

    /// "Check names for Windows and macOS…" and its list (VAULT-028).
    fn names_check_ui(&mut self, ui: &mut Ui) {
        crate::theme::section(ui, "Names on other systems");
        if self.vault_check.poll() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        let running = self.vault_check.running();
        anchor(ui, "vault.names", |ui| {
            ui.horizontal(|ui| {
                let has_vault = self.vault.is_some();
                let button = ui.add_enabled_ui(has_vault, |ui| crate::busy::ui::button(ui, &self.busy, &[&crate::vault_check::VAULT_CHECK], egui::Button::new("Check names for Windows and macOS…"))).inner;
                if button.on_hover_text("Lists the files and folders whose names would not work on Windows or macOS").clicked() {
                    self.start_vault_check();
                }
                crate::busy::ui::status(ui, &self.busy, &crate::vault_check::VAULT_CHECK);
            });
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

    /// The links to media outside the vault, from the same check (MEDIA-022): the libraries that
    /// cannot be used on this device and, by note, the links that need a look.
    fn outside_check_ui(&mut self, ui: &mut Ui) {
        use crate::vault_check::FolderState;
        let Some(check) = self.vault_check.outside.clone() else { return };
        crate::theme::section(ui, "Media outside the vault");
        if check.links == 0 {
            ui.label("No note links a picture, PDF, sound or video from outside the vault.");
            return;
        }
        let looks = check.problems();
        ui.label(format!(
            "{} {} to media outside the vault; the files are not synced. {}",
            check.links,
            if check.links == 1 { "link" } else { "links" },
            if looks == 0 { "All of them work on this device.".to_string() } else { format!("{looks} need a look:") }
        ));
        for lib in &check.libraries {
            let what = match &lib.state {
                FolderState::NotSetUp => "is not set up on this device (Settings → Import)".to_string(),
                FolderState::NotThere(folder) => format!("has its folder {} set, but it is not there", folder.display()),
            };
            crate::theme::problem(ui, format!("Library “{}” {what}: {} {}", lib.name, lib.links, if lib.links == 1 { "link" } else { "links" }));
        }
        egui::ScrollArea::vertical().id_salt("outside-check").max_height(260.0).auto_shrink([false, true]).show(ui, |ui| {
            for note in &check.notes {
                ui.label(egui::RichText::new(crate::vault::rel_to_string(&note.note)).strong());
                for e in &note.entries {
                    ui.horizontal_wrapped(|ui| {
                        ui.add_space(12.0);
                        ui.label(&e.target);
                        crate::theme::hint(ui, e.problem.text());
                    });
                }
            }
        });
    }
}
