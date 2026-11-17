//! The LaTeX tab: which TeX exports to PDF, and the managed TeX Live (TEX-001 to TEX-003).

use crate::app::ShaleApp;
use eframe::egui::Ui;

impl ShaleApp {
    pub(super) fn latex_tab(&mut self, ui: &mut Ui) -> bool {
        crate::pdf::tex::settings_ui::show(ui, &mut self.tex, &mut self.cfg.tex)
    }
}
