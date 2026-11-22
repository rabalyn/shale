//! The Sync tab (git) and the Calendar tab.

use super::Asks;
use crate::app::ShaleApp;
use eframe::egui::Ui;

impl ShaleApp {
    pub(super) fn sync_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        let sync = self.sync_settings_ui(ui);
        self.outside_media_note(ui);
        self.history_settings_ui(ui, asks) | sync
    }

    pub(super) fn calendar_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        ui.horizontal(|ui| {
            if ui.button("Calendars…").on_hover_text("Add local calendars or CalDAV accounts").clicked() {
                asks.calendars = true;
            }
            crate::theme::hint(ui, format!("{} configured", self.cfg.calendars.len()));
        });
        ui.checkbox(&mut self.cfg.week_starts_monday, "Calendar weeks start on Monday (otherwise Sunday)").changed()
    }
}
