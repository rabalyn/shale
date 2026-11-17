//! The Fonts tab (CFG-030, CFG-031): the font and size of every area of the window, or one
//! font and size for all of them.

use super::font_picker;
use crate::app::ShaleApp;
use crate::fonts::{Area, FontChoice, MAX_SIZE, MIN_SIZE};
use crate::theme;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn fonts_tab(&mut self, ui: &mut Ui) -> bool {
        // the list of the installed fonts is read the first time this page is shown
        self.fonts.system.start();
        self.fonts.system.poll();
        let mut changed = false;
        let mut same = self.cfg.fonts.same_everywhere;
        if ui.checkbox(&mut same, "Use the same font and size everywhere").on_hover_text("Off: every area below has a font and size of its own").changed() {
            if same {
                self.cfg.fonts.same_everywhere = true;
            } else {
                self.cfg.fonts.separate();
            }
            changed = true;
        }
        ui.add_space(6.0);
        let (families, searching) = (self.fonts.system.families().to_vec(), !self.fonts.system.ready());
        egui::Grid::new("fonts-grid").num_columns(3).spacing([12.0, 6.0]).show(ui, |ui| {
            if self.cfg.fonts.same_everywhere {
                changed |= font_row(ui, "Font and size", &mut self.cfg.fonts.all, &families, searching);
                ui.end_row();
            } else {
                for area in Area::ALL {
                    changed |= font_row(ui, area.label(), self.cfg.fonts.slot_mut(area), &families, searching);
                    ui.end_row();
                }
            }
        });
        if self.cfg.fonts.same_everywhere {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("The size is that of dialogs and menus; the text of the editor is drawn a little larger, as before.").small().color(theme::MUTED));
        }
        changed
    }
}

/// One row: the font, the size, and a hint when the font is not installed.
fn font_row(ui: &mut Ui, label: &str, choice: &mut FontChoice, families: &[String], searching: bool) -> bool {
    let mut changed = false;
    ui.label(label);
    changed |= font_picker::show(ui, label, &mut choice.family, families, searching);
    ui.horizontal(|ui| {
        changed |= ui.add(egui::DragValue::new(&mut choice.size).range(MIN_SIZE..=MAX_SIZE).speed(0.1).suffix(" pt").max_decimals(1)).on_hover_text("Text size in points").changed();
        if let (Some(f), false) = (&choice.family, searching) {
            if !families.contains(f) {
                ui.label(egui::RichText::new("not installed").small().color(theme::LINK_MISSING));
            }
        }
    });
    changed
}
