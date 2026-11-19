//! The Help tab: the user guide and the troubleshooting switches.

use super::{section, Asks};
use crate::app::ShaleApp;
use eframe::egui::Ui;

impl ShaleApp {
    pub(super) fn help_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        let mut changed = false;
        if ui.button("Open user guide (F1)").on_hover_text("The guide is a vault: a copy of it is made in Shale's data folder, once per version, and opened").clicked() {
            asks.guide = true;
        }
        section(ui, "Troubleshooting");
        if ui.checkbox(&mut self.cfg.input_log, "Record keyboard input to a file (to find keyboard problems)").changed() {
            if self.cfg.input_log {
                self.start_input_log();
            }
            changed = true;
        }
        if self.cfg.input_log {
            let p = crate::config::input_log_path();
            ui.horizontal(|ui| {
                crate::theme::hint(ui, p.to_string_lossy());
                if ui.small_button("Copy path").clicked() {
                    ui.ctx().copy_text(p.to_string_lossy().into_owned());
                }
            });
        }
        crate::theme::hint(ui, format!("Build {}", crate::config::build_stamp()));
        changed
    }
}
