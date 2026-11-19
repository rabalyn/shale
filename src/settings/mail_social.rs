//! The Mail tab and the Feeds tab: the simple settings, and a button to each account dialog.

use super::Asks;
use crate::app::ShaleApp;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn mail_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.button("Mail accounts…").on_hover_text("Add IMAP accounts, sending, encryption").clicked() {
                asks.mail = true;
            }
            crate::theme::hint(ui, format!("{} configured", self.cfg.mail_accounts.len()));
        });
        crate::theme::section(ui, "Reading");
        ui.horizontal(|ui| {
            ui.label("Fetch new mail every");
            changed |= ui.add(egui::DragValue::new(&mut self.cfg.mail_sync_minutes).range(0..=1440)).changed();
            ui.label("minutes (0: only with the Sync button)");
        });
        changed |= ui.checkbox(&mut self.cfg.mail_mark_read, "Opening a message marks it read on the server").changed();
        ui.add_enabled_ui(self.cfg.mail_mark_read, |ui| {
            ui.horizontal(|ui| {
                ui.label("after");
                changed |= ui.add(egui::DragValue::new(&mut self.cfg.mail_mark_read_secs).range(0..=60)).changed();
                ui.label("seconds (0: at once)");
            });
        });
        changed
    }

    pub(super) fn social_tab(&mut self, ui: &mut Ui, asks: &mut Asks) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            if ui.button("Feed accounts…").on_hover_text("Mastodon and Bluesky accounts").clicked() {
                asks.social = true;
            }
            crate::theme::hint(ui, format!("{} configured", self.cfg.social_accounts.len()));
        });
        ui.horizontal(|ui| {
            ui.label("Look for new posts every");
            changed |= ui.add(egui::DragValue::new(&mut self.cfg.social_check_minutes).range(0..=1440)).changed();
            ui.label("minutes (0: never)");
        });
        crate::theme::section_break(ui);
        self.post_index_settings(ui);
        changed
    }
}
