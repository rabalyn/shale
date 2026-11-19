//! The Import tab: the Word import through pandoc (IMP-036). Which pandoc is used is looked up in
//! a background job (`pandoc --version` is a program run), once when the page is first shown and
//! again on **Check again**.

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
    pub(super) fn import_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        crate::theme::section(ui, "E-mails, events and posts");
        changed |= ui
            .checkbox(&mut self.cfg.import_header_list, "Header list in the note body")
            .on_hover_text("An imported e-mail or event always has its From, To, Date, When, Where … in the note's properties. Ticked: they are also written as a list below the title, for programs that do not show properties. The Import dialog starts with this choice")
            .changed();
        self.word.poll();
        if self.word.result.is_none() && !self.word.running() {
            self.word.start(self.cfg.pandoc_path.clone());
        }
        crate::theme::section(ui, "Word files (.docx)");
        changed |= ui
            .checkbox(&mut self.cfg.docx_import, "Import Word files as notes (needs pandoc)")
            .on_hover_text("A .docx file dropped on the window or added in the Import dialog becomes a note; pictures are saved next to it. Off: it is saved as an attachment")
            .changed();
        match (&self.word.result, self.word.running()) {
            (_, true) => {
                ui.label(muted("Looking for pandoc…"));
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
            (Some(Probe::Found { path, version }), false) => {
                crate::theme::prose(ui, muted(format!("{version}, found at {}", path.display())));
            }
            (Some(Probe::Broken { path, error }), false) => {
                ui.add(egui::Label::new(egui::RichText::new(format!("{} does not work: {error}", path.display())).color(theme::LINK_MISSING)).wrap());
            }
            _ => {
                ui.add(egui::Label::new(egui::RichText::new("pandoc was not found. Install it, then press Check again:").color(theme::LINK_MISSING)).wrap());
            }
        }
        if !matches!(self.word.result, Some(Probe::Found { .. })) && !self.word.running() {
            for (system, how) in docx::INSTALL_HELP {
                crate::theme::hint(ui, format!("{system}: {how}"));
            }
            crate::theme::hint(ui, "or see https://pandoc.org/installing.html");
        }
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
        if ui.add_enabled(!self.word.running(), egui::Button::new("Check again")).on_hover_text("Look for pandoc again, for example after installing it").clicked() {
            self.word.start(self.cfg.pandoc_path.clone());
        }
        changed
    }
}
