//! The Keymap tab of the main area (KEY-058): the keyboard view as a view of a tab, opened by its
//! ribbon item, by its key (`global.keymap`) and by the screenshot option.

use crate::app::{Modal, SettingsTab, ShaleApp};
use crate::keys::Action;
use crate::theme;
use eframe::egui::{self, Ui};

impl ShaleApp {
    /// The content of the Keymap tab.
    pub(crate) fn keymap_view_tab(&mut self, ui: &mut Ui) {
        let mut change = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 8)).show(ui, |ui| {
                theme::hint(ui, "Every shortcut on the keyboard, by system and layer. Click a key to see its actions; hold Ctrl, Alt or Shift to look at that layer.");
                change = crate::keymap_view::show(ui, &mut self.keymap_view, &self.keymap);
            });
        });
        if let Some(a) = change {
            self.change_keybinding(ui.ctx(), a);
        }
    }

    /// Settings, Keybindings, at the row of `a`, with its new keys being recorded (KEY-058).
    pub(crate) fn change_keybinding(&mut self, ctx: &egui::Context, a: Action) {
        self.open_settings(SettingsTab::Keybindings);
        if let Some(Modal::Settings { keys, .. }) = &mut self.modal {
            keys.go_to(ctx, a, &self.keymap);
        }
    }
}
