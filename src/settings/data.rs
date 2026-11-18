//! The Data and caches tab.

use super::Asks;
use crate::app::ShaleApp;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn data_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.button("Data usage…").on_hover_text("What Shale keeps on disk, and its caches").clicked() {
                asks.usage = true;
            }
            let total = self.usage.total().map(|t| format!("caches: {}", crate::data_usage::human_bytes(t))).unwrap_or_else(|| "caches: not counted yet".into());
            crate::theme::hint(ui, total);
        });
        egui::CollapsingHeader::new("How long caches are kept").id_salt("settings-retention").show(ui, |ui| {
            for id in crate::data_usage::CacheId::ALL.into_iter().filter(|id| !id.kept_current()) {
                ui.label(id.name());
                ui.indent(("retention", id), |ui| {
                    if crate::data_usage::dialog::retention_ui(ui, id, &mut self.cfg.caches) {
                        self.usage.setting_changed(id);
                        changed = true;
                    }
                });
            }
        });
        changed
    }
}
