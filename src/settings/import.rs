//! The Import tab: the Word import through pandoc (IMP-036). Which pandoc is used is looked up in
//! a background job (`pandoc --version` is a program run), once when the page is first shown and
//! again on **Check again**.

use super::anchor::anchor;
use crate::app::{muted, ShaleApp};
use crate::import::docx;
use crate::theme;
use eframe::egui::{self, Ui};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

/// What the look for pandoc found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// pandoc is there and says its version
    Found {
        path: PathBuf,
        version: String,
    },
    /// a program is there but `--version` did not work
    Broken {
        path: PathBuf,
        error: String,
    },
    Missing,
}

/// The state of the page: the last result and the look that is running.
#[derive(Default)]
pub struct WordProbe {
    pub result: Option<Probe>,
    job: Option<Receiver<Result<Probe, String>>>,
}

impl WordProbe {
    /// Look for pandoc (the program `chosen` in the settings, empty: the search) in a job.
    pub fn start(&mut self, chosen: String) {
        self.job = Some(crate::jobs::spawn(
            move || match docx::locate(&chosen) {
                None => Probe::Missing,
                Some(path) => match docx::version(&path) {
                    Ok(version) => Probe::Found { path, version },
                    Err(error) => Probe::Broken { path, error },
                },
            },
            || {},
        ));
    }

    /// Take the result of the job when it has ended.
    pub fn poll(&mut self) {
        let Some(rx) = &self.job else { return };
        match rx.try_recv() {
            Ok(r) => {
                self.result = Some(r.unwrap_or_else(|e| Probe::Broken { path: PathBuf::new(), error: e }));
                self.job = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => self.job = None,
        }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }
}

impl ShaleApp {
    /// "Files from outside the vault" (MEDIA-019): what a pick, paste or import does with a picture,
    /// PDF, sound or video from outside the vault, and the library folders (CFG-069, CFG-070).
    fn outside_files_ui(&mut self, ui: &mut Ui) -> bool {
        use crate::media::intake::OutsideFiles;
        let mut changed = false;
        crate::theme::section(ui, "Files from outside the vault");
        crate::theme::hint(ui, "For a picture, PDF, sound or video that is not in the vault when you pick it with [!, paste it or import it. Other files are always copied.");
        changed |= anchor(ui, "import.outside_files", |ui| {
            let mut changed = false;
            for value in OutsideFiles::ALL {
                changed |= ui.radio_value(&mut self.cfg.outside_files, value, value.label()).on_hover_text(value.tip()).changed();
            }
            changed
        });
        crate::theme::section(ui, "Library folders");
        crate::theme::hint(ui, "A library folder gives a folder of this computer a name. A link like ![[@Videos/2024/a.mp4]] means that file on every computer where Videos is set up, so a big collection can stay out of the vault. Each computer has its own list, here.");
        changed |= anchor(ui, "import.libraries", |ui| self.library_folders_ui(ui));
        changed
    }

    /// The list of the library folders with a row to add one (CFG-069).
    fn library_folders_ui(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        let names: Vec<String> = self.cfg.media_libraries.iter().map(|l| l.name.clone()).collect();
        let mut remove = None;
        for (i, lib) in self.cfg.media_libraries.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                changed |= ui.add(egui::TextEdit::singleline(&mut lib.name).hint_text("Name").desired_width(120.0)).on_hover_text("The name links use after the @").changed();
                let mut path = lib.path.to_string_lossy().into_owned();
                let r = ui.add(egui::TextEdit::singleline(&mut path).hint_text("Folder on this computer").desired_width(300.0));
                if r.lost_focus() {
                    path = crate::layout::shellexpand(&path);
                }
                if r.changed() || r.lost_focus() {
                    changed |= lib.path.to_string_lossy() != path.as_str();
                    lib.path = PathBuf::from(path);
                }
                if ui.button("Remove").on_hover_text("Forget this library on this computer; notes that link into it show “not set up”").clicked() {
                    remove = Some(i);
                }
            });
            let others = names.iter().enumerate().filter(|(k, _)| *k != i).map(|(_, n)| n.as_str());
            if let Some(p) = crate::media::outside::Library::name_problem(&lib.name, others) {
                crate::theme::problem(ui, p);
            } else if lib.path.as_os_str().is_empty() {
                crate::theme::problem(ui, "Give the folder");
            } else if !lib.path.is_dir() {
                crate::theme::problem(ui, "This folder is not there");
            }
        }
        if let Some(i) = remove {
            self.cfg.media_libraries.remove(i);
            changed = true;
        }
        // a new library: made when the name works and there is a folder
        let id = ui.make_persistent_id("new-library");
        let (mut name, mut folder): (String, String) = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
        let problem = crate::media::outside::Library::name_problem(&name, names.iter().map(String::as_str));
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut name).hint_text("New name").desired_width(120.0));
            let r = ui.add(egui::TextEdit::singleline(&mut folder).hint_text("Folder, for example ~/Videos").desired_width(300.0));
            if r.lost_focus() {
                folder = crate::layout::shellexpand(&folder);
            }
            if ui.add_enabled(problem.is_none() && !folder.trim().is_empty(), egui::Button::new("Add library")).clicked() {
                self.cfg.media_libraries.push(crate::media::outside::Library { name: std::mem::take(&mut name), path: PathBuf::from(crate::layout::shellexpand(&std::mem::take(&mut folder))) });
                changed = true;
            }
        });
        if let Some(p) = problem.filter(|_| !name.is_empty()) {
            crate::theme::problem(ui, p);
        }
        ui.data_mut(|d| d.insert_temp(id, (name, folder)));
        changed
    }

    pub(super) fn import_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = self.outside_files_ui(ui);
        crate::theme::section(ui, "E-mails, events and posts");
        changed |= anchor(ui, "import.header_list", |ui| {
            ui.checkbox(&mut self.cfg.import_header_list, "Header list in the note body")
                .on_hover_text("An imported e-mail or event always has its From, To, Date, When, Where … in the note's properties. Ticked: they are also written as a list below the title, for programs that do not show properties. The Import dialog starts with this choice")
                .changed()
        });
        changed |= anchor(ui, "import.skip_signatures", |ui| {
            ui.checkbox(&mut self.cfg.import_skip_signatures, "Skip signatures")
                .on_hover_text("A signed e-mail has a signature file (smime.p7s, signature.asc), and a signature block has logos and social icons. Ticked: the Import dialog starts with them unticked, and you can tick each one. The Import dialog starts with this choice")
                .changed()
        });
        self.word.poll();
        if self.word.result.is_none() && !self.word.running() {
            self.word.start(self.cfg.pandoc_path.clone());
        }
        crate::theme::section(ui, "Word files (.docx)");
        changed |= anchor(ui, "import.docx", |ui| {
            ui.checkbox(&mut self.cfg.docx_import, "Import Word files as notes (needs pandoc)")
                .on_hover_text("A .docx file dropped on the window or added in the Import dialog becomes a note; pictures are saved next to it. Off: it is saved as an attachment")
                .changed()
        });
        match (&self.word.result, self.word.running()) {
            (_, true) => {
                ui.label(muted("Looking for pandoc…"));
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            (Some(Probe::Found { path, version }), false) => {
                crate::theme::prose(ui, muted(format!("{version}, found at {}", path.display())));
            }
            (Some(Probe::Broken { path, error }), false) => {
                ui.add(egui::Label::new(egui::RichText::new(format!("{} does not work: {error}", path.display())).color(theme::palette().link_missing)).wrap());
            }
            _ => {
                ui.add(egui::Label::new(egui::RichText::new("pandoc was not found. Install it, then press Check again:").color(theme::palette().link_missing)).wrap());
            }
        }
        if !matches!(self.word.result, Some(Probe::Found { .. })) && !self.word.running() {
            for (system, how) in docx::INSTALL_HELP {
                crate::theme::hint(ui, format!("{system}: {how}"));
            }
            crate::theme::hint(ui, "or see https://pandoc.org/installing.html");
        }
        anchor(ui, "import.pandoc", |ui| {
            ui.horizontal(|ui| {
                ui.label("pandoc program");
                let r = ui.add(egui::TextEdit::singleline(&mut self.cfg.pandoc_path).hint_text("empty: search PATH and the usual places").desired_width(320.0));
                if r.changed() {
                    changed = true;
                }
                if r.lost_focus() && r.changed() {
                    self.word.start(self.cfg.pandoc_path.clone());
                }
            });
        });
        if ui.add_enabled(!self.word.running(), egui::Button::new("Check again")).on_hover_text("Look for pandoc again, for example after installing it").clicked() {
            self.word.start(self.cfg.pandoc_path.clone());
        }
        changed
    }
}
