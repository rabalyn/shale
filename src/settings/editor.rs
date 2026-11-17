//! The Editor tab: how notes are edited.

use super::section;
use crate::app::{muted, ShaleApp};
use crate::link_preview::{DEFAULT_HEIGHT, DEFAULT_WIDTH, MIN_HEIGHT, MIN_WIDTH};
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn editor_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        changed |= ui.checkbox(&mut self.cfg.vim_mode, "Vim-style editing: notes open in normal mode (h j k l w b, :line, /search, v / V, d y p); i a o to type, Esc back").changed();
        changed |= ui.checkbox(&mut self.cfg.line_numbers, "Show line numbers in the editor").changed();
        changed |= ui
            .checkbox(&mut self.cfg.numbered_headings, "Number headings")
            .on_hover_text("Headings from ## on are shown as 1, 1.1, 1.2; the title (#) is never numbered. The numbers are only drawn, never written into the note. A note's front matter numbered-headings: true or false overrides this for that note.")
            .changed();
        changed |= ui.checkbox(&mut self.cfg.live_preview, "Markdown preview in the editor (the lines around the cursor show raw markdown)").changed();
        ui.add_enabled_ui(self.cfg.live_preview, |ui| {
            ui.horizontal(|ui| {
                ui.label("Raw lines around the cursor");
                changed |= ui.add(egui::DragValue::new(&mut self.cfg.preview_raw_lines).range(0..=20)).changed();
            });
            changed |=
                ui.checkbox(&mut self.cfg.table_cell_edit, "Edit table cells in the grid").on_hover_text("Off: a click on a table shows its raw markdown. A single table can be switched by the button above its top right corner.").changed();
        });
        section(ui, "Links");
        changed |= ui.checkbox(&mut self.cfg.ctrl_to_follow, "Follow links only with Ctrl+click (plain click places the cursor)").changed();
        changed |= ui.checkbox(&mut self.cfg.new_notes_next_to_current, "Create notes from red links next to the current note (instead of the vault root)").changed();
        changed |= ui.checkbox(&mut self.cfg.update_links_on_rename, "Update links in other notes when renaming").changed();
        ui.horizontal(|ui| {
            ui.label("Link preview size (Shift+K)");
            changed |= ui.add(egui::DragValue::new(&mut self.cfg.link_preview_width).range(MIN_WIDTH as u32..=4000).suffix(" wide")).changed();
            changed |= ui.add(egui::DragValue::new(&mut self.cfg.link_preview_height).range(MIN_HEIGHT as u32..=4000).suffix(" high")).changed();
            if ui.button("Reset size").on_hover_text("The preview is 640 x 480 points; dragging its edges changes the size too").clicked() {
                self.cfg.link_preview_width = DEFAULT_WIDTH;
                self.cfg.link_preview_height = DEFAULT_HEIGHT;
                changed = true;
            }
        });
        section(ui, "Drawings");
        changed |= ui
            .checkbox(&mut self.cfg.auto_convert_canvas, "Convert Obsidian canvases to editable drawings when opening them (instead of asking)")
            .on_hover_text("Off: Shale asks, and can show the canvas read-only. The canvas file is never changed.")
            .changed();
        section(ui, "Editing");
        changed |= ui.checkbox(&mut self.cfg.format_toolbar, "Show formatting toolbar above the text of a note").changed();
        ui.add_enabled_ui(self.cfg.format_toolbar, |ui| {
            ui.horizontal(|ui| {
                ui.label("Toolbar button size");
                changed |= ui.add(egui::Slider::new(&mut self.cfg.toolbar_size, 80..=200).step_by(10.0).suffix(" %")).changed();
            });
        });
        changed |= crate::format_bar::preset_editor(ui, "Font colours", &mut self.cfg.font_colours, &crate::config::DEFAULT_FONT_COLOURS);
        changed |= crate::format_bar::preset_editor(ui, "Highlight colours", &mut self.cfg.highlight_colours, &crate::config::DEFAULT_HIGHLIGHT_COLOURS);
        ui.label(muted("The colours of the toolbar's palettes. Colours are saved in the note as <span style=\"color: #RRGGBB;\"> and <mark style=\"background: #RRGGBB;\">.").size(12.0));
        changed
    }
}
