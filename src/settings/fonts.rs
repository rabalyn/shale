//! The Fonts tab (CFG-030, CFG-031): the font and size of every area of the window, or one
//! font and size for all of them, each with a sample text (CFG-040).

use super::font_picker;
use crate::app::ShaleApp;
use crate::fonts::{Area, AreaFont, FontChoice, FontState, Preview, MAX_SIZE, MIN_SIZE};
use crate::theme;
use eframe::egui::{self, text::LayoutJob, FontId, TextFormat, Ui};

/// The sample text of a row (CFG-040), in pieces: regular, bold, regular, italic.
pub const SAMPLE: [&str; 4] = ["The quick brown fox, ", "bold", " and ", "italic"];

/// The largest size a sample is drawn at, and how wide it is at most.
const SAMPLE_MAX_SIZE: f32 = 28.0;
const SAMPLE_WIDTH: f32 = 300.0;

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
        let fonts = &mut self.fonts;
        egui::Grid::new("fonts-grid").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
            if self.cfg.fonts.same_everywhere {
                changed |= font_row(ui, "Font and size", &mut self.cfg.fonts.all, &families, searching, fonts);
                ui.end_row();
            } else {
                for area in Area::ALL {
                    changed |= font_row(ui, area.label(), self.cfg.fonts.slot_mut(area), &families, searching, fonts);
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

/// One row: the font, the size, a hint when the font is not installed, and the sample text in
/// the chosen font, or in the highlighted one while the list is open (CFG-040).
fn font_row(ui: &mut Ui, label: &str, choice: &mut FontChoice, families: &[String], searching: bool, fonts: &mut FontState) -> bool {
    let mut changed = false;
    ui.label(label);
    let picker = font_picker::show(ui, label, &mut choice.family, families, searching);
    changed |= picker.picked;
    ui.horizontal(|ui| {
        changed |= ui.add(egui::DragValue::new(&mut choice.size).range(MIN_SIZE..=MAX_SIZE).speed(0.1).suffix(" pt").max_decimals(1)).on_hover_text("Text size in points").changed();
        if let (Some(f), false) = (&choice.family, searching) {
            if !families.contains(f) {
                ui.label(egui::RichText::new("not installed").small().color(theme::LINK_MISSING));
            }
        }
    });
    let shown = picker.highlighted.unwrap_or_else(|| choice.family.clone());
    let size = if choice.size.is_finite() { choice.size.clamp(MIN_SIZE, SAMPLE_MAX_SIZE) } else { crate::fonts::UI_SIZE };
    match fonts.preview(shown.as_deref(), size) {
        Preview::Ready(font) => {
            let height = size * 1.4;
            ui.allocate_ui_with_layout(egui::vec2(SAMPLE_WIDTH, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(sample(&font)).truncate().selectable(false));
            });
        }
        Preview::Loading => {
            ui.label(egui::RichText::new("Loading the font…").small().color(theme::MUTED));
            ui.ctx().request_repaint();
        }
        Preview::Missing => {
            ui.label("");
        }
    }
    changed
}

/// The sample text in `font`, bold and italic in its own faces where it has them (CFG-039).
fn sample(font: &AreaFont) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, piece) in SAMPLE.iter().enumerate() {
        let st = font.styled(i == 1, i == 3);
        let fmt =
            TextFormat { font_id: FontId::new(font.size, st.family), color: if i == 1 { theme::TEXT_STRONG } else { theme::TEXT }, italics: st.fake_italic, extra_letter_spacing: if st.fake_bold { 0.3 } else { 0.0 }, ..Default::default() };
        job.append(piece, 0.0, fmt);
    }
    job
}
