//! The Editor tab: how notes are edited. The Vim keys are on the Vim tab (`vim.rs`).

use super::anchor::anchor;
use super::SettingsTab;
use crate::app::ShaleApp;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn editor_tab(&mut self, ui: &mut Ui, tab: &mut SettingsTab) -> bool {
        let mut changed = false;
        crate::theme::section(ui, "Cursor");
        changed |= anchor(ui, "editor.cursor_shape", |ui| cursor_combo(ui, "Cursor while typing", "cursor-shape", &mut self.cfg.cursor_shape));
        changed |= anchor(ui, "editor.cursor_blink", |ui| ui.checkbox(&mut self.cfg.cursor_blink, "Blinking cursor").changed());
        ui.horizontal(|ui| {
            crate::theme::hint(ui, format!("Vim-style editing: {} · set it up in the", if self.cfg.vim_mode { "on" } else { "off" }));
            if ui.link("Vim tab").clicked() {
                *tab = SettingsTab::Vim;
            }
        });
        changed |= anchor(ui, "editor.line_numbers", |ui| ui.checkbox(&mut self.cfg.line_numbers, "Show line numbers in the editor").changed());
        changed |= anchor(ui, "editor.numbered_headings", |ui| {
            ui.checkbox(&mut self.cfg.numbered_headings, "Number headings")
                .on_hover_text(
                    "Headings from # on are shown as 1, 1.1, 1.2; the note's title is the title line above the text, not a heading. The numbers are only drawn, never written into the note. A note's front matter numbered-headings: true or false overrides this for that note.",
                )
                .changed()
        });
        changed |= anchor(ui, "editor.date_prefix", |ui| {
            ui.checkbox(&mut self.cfg.date_prefix_new_notes, "Date prefix for new notes")
                .on_hover_text("New notes are named YYMMDD_name.md with today's date, so the file list sorts by date. The title line and the tabs show the name without the prefix.")
                .changed()
        });
        changed |= anchor(ui, "editor.live_preview", |ui| ui.checkbox(&mut self.cfg.live_preview, "Markdown preview in the editor (the lines around the cursor show raw markdown)").changed());
        ui.add_enabled_ui(self.cfg.live_preview, |ui| {
            changed |= anchor(ui, "editor.preview_raw_lines", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Raw lines around the cursor");
                    ui.add(egui::DragValue::new(&mut self.cfg.preview_raw_lines).range(0..=20)).changed()
                })
                .inner
            });
            changed |= anchor(ui, "editor.table_cell_edit", |ui| {
                ui.checkbox(&mut self.cfg.table_cell_edit, "Edit table cells in the grid").on_hover_text("Off: a click on a table shows its raw markdown. A single table can be switched by the button above its top right corner.").changed()
            });
        });
        crate::theme::section(ui, "Links");
        changed |= anchor(ui, "editor.ctrl_to_follow", |ui| ui.checkbox(&mut self.cfg.ctrl_to_follow, "Follow links only with Ctrl+click (plain click places the cursor)").changed());
        changed |= anchor(ui, "editor.new_notes_next_to_current", |ui| ui.checkbox(&mut self.cfg.new_notes_next_to_current, "Create notes from red links next to the current note (instead of the vault root)").changed());
        changed |= anchor(ui, "editor.update_links_on_rename", |ui| ui.checkbox(&mut self.cfg.update_links_on_rename, "Update links in other notes when renaming").changed());
        crate::theme::section(ui, "Drawings");
        changed |= anchor(ui, "editor.auto_convert_canvas", |ui| {
            ui.checkbox(&mut self.cfg.auto_convert_canvas, "Convert Obsidian canvases to editable drawings when opening them (instead of asking)")
                .on_hover_text("Off: Shale asks, and can show the canvas read-only. The canvas file is never changed.")
                .changed()
        });
        crate::theme::section(ui, "Editing");
        changed |= anchor(ui, "editor.format_toolbar", |ui| ui.checkbox(&mut self.cfg.format_toolbar, "Show formatting toolbar above the text of a note").changed());
        ui.add_enabled_ui(self.cfg.format_toolbar, |ui| {
            changed |= anchor(ui, "editor.toolbar_size", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Toolbar button size");
                    ui.add(egui::Slider::new(&mut self.cfg.toolbar_size, 80..=200).step_by(10.0).suffix(" %")).changed()
                })
                .inner
            });
        });
        changed |= anchor(ui, "editor.font_colours", |ui| crate::format_bar::preset_editor(ui, "Font colours", &mut self.cfg.font_colours, &crate::config::DEFAULT_FONT_COLOURS));
        changed |= anchor(ui, "editor.highlight_colours", |ui| crate::format_bar::preset_editor(ui, "Highlight colours", &mut self.cfg.highlight_colours, &crate::config::DEFAULT_HIGHLIGHT_COLOURS));
        crate::theme::hint(ui, "The colours of the toolbar's palettes. Colours are saved in the note as <span style=\"color: #RRGGBB;\"> and <mark style=\"background: #RRGGBB;\">.");
        changed
    }
}

/// A choice of cursor shape (EDIT-039, EDIT-040).
pub(super) fn cursor_combo(ui: &mut Ui, label: &str, id: &str, shape: &mut crate::cursor_style::CursorShape) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id).selected_text(shape.label()).show_ui(ui, |ui| {
            for s in crate::cursor_style::CursorShape::ALL {
                changed |= ui.selectable_value(shape, s, s.label()).changed();
            }
        });
    });
    changed
}
