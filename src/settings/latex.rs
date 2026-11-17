//! The LaTeX tab: which TeX exports to PDF, the managed TeX Live (TEX-001 to TEX-003) and the
//! template sets of the vault (TEX-024).

use crate::app::ShaleApp;
use crate::settings::Asks;
use eframe::egui::Ui;

impl ShaleApp {
    pub(super) fn latex_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        self.bind_tex_sets();
        let changed = crate::pdf::tex::settings_ui::show(ui, &mut self.tex, &mut self.cfg.tex);
        asks.tex_setup |= std::mem::take(&mut self.tex.setup_requested);
        // a set the person removed goes through the window's trash (tabs, undo of the last delete)
        if let Some(rel) = self.tex.sets.take_trash() {
            self.trash(&rel);
        }
        if self.tex.sets.take_rescan() {
            if let Some(v) = self.vault.as_mut() {
                v.rescan();
            }
        }
        changed
    }
}
