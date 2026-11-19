//! The Vim tab, a sub-category of Editor: the switch for Vim-style editing, the cursor of its
//! modes and the link preview of Shift+K (EDIT-044).

use super::editor::cursor_combo;
use super::SettingsTab;
use crate::app::ShaleApp;
use crate::link_preview::{DEFAULT_HEIGHT, DEFAULT_WIDTH, MIN_HEIGHT, MIN_WIDTH};
use crate::vim::EscCursor;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn vim_tab(&mut self, ui: &mut Ui, tab: &mut SettingsTab) -> bool {
        let mut changed = false;
        changed |= ui.checkbox(&mut self.cfg.vim_mode, "Vim-style editing").changed();
        crate::theme::hint(ui, "Notes open in normal mode (h j k l w b, :line, /search, v / V, d y p); i a o to type, Esc back.");
        crate::theme::section(ui, "Cursor");
        ui.add_enabled_ui(self.cfg.vim_mode, |ui| {
            changed |= cursor_combo(ui, "Cursor in normal mode", "normal-cursor-shape", &mut self.cfg.normal_cursor_shape);
            ui.horizontal(|ui| {
                ui.label("Esc from insert mode");
                egui::ComboBox::from_id_salt("esc-cursor").selected_text(self.cfg.esc_cursor.label()).show_ui(ui, |ui| {
                    for e in EscCursor::ALL {
                        changed |= ui.selectable_value(&mut self.cfg.esc_cursor, e, e.label()).changed();
                    }
                });
            });
        });
        crate::theme::section(ui, "Link preview (Shift+K)");
        ui.horizontal(|ui| {
            ui.label("Link preview size");
            changed |= ui.add(egui::DragValue::new(&mut self.cfg.link_preview_width).range(MIN_WIDTH as u32..=4000).suffix(" wide")).changed();
            changed |= ui.add(egui::DragValue::new(&mut self.cfg.link_preview_height).range(MIN_HEIGHT as u32..=4000).suffix(" high")).changed();
            if ui.button("Reset size").on_hover_text("The preview is 640 x 480 points; dragging its edges changes the size too").clicked() {
                self.cfg.link_preview_width = DEFAULT_WIDTH;
                self.cfg.link_preview_height = DEFAULT_HEIGHT;
                changed = true;
            }
        });
        crate::theme::section_break(ui);
        ui.horizontal(|ui| {
            crate::theme::hint(ui, "The keys of normal mode (Space Tab, Space f f, Space f g, Shift+K) are set in the");
            if ui.link("Keybindings tab").clicked() {
                *tab = SettingsTab::Keybindings;
            }
        });
        changed
    }
}
