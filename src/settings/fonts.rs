//! The Fonts tab (CFG-030, CFG-031): the font and size of every area of the window, or one
//! font and size for all of them, each with a sample text (CFG-040).

use super::anchor::{anchor, anchor_id, mark};
use super::font_picker;
use crate::app::ShaleApp;
use crate::fonts::{Area, AreaFont, FontChoice, FontState, Preview, MAX_SIZE, MIN_SIZE};
use crate::keys::Keymap;
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
        if anchor(ui, "fonts.same", |ui| ui.checkbox(&mut same, "Use the same font and size everywhere").on_hover_text("Off: every area below has a font and size of its own").changed()) {
            if same {
                self.cfg.fonts.same_everywhere = true;
            } else {
                self.cfg.fonts.separate();
            }
            changed = true;
        }
        let (families, searching) = (self.fonts.system.families().to_vec(), !self.fonts.system.ready());
        let fonts = &mut self.fonts;
        egui::Grid::new("fonts-grid").num_columns(4).spacing([12.0, 6.0]).show(ui, |ui| {
            if self.cfg.fonts.same_everywhere {
                changed |= font_row(ui, (anchor_id("fonts.all"), "Font and size"), &mut self.cfg.fonts.all, &families, searching, fonts, &self.keymap);
                ui.end_row();
            } else {
                for area in Area::ALL {
                    let id = match area {
                        Area::Ui => anchor_id("fonts.ui"),
                        Area::EditorRaw => anchor_id("fonts.editor_raw"),
                        Area::EditorRendered => anchor_id("fonts.editor_rendered"),
                        Area::Mail => anchor_id("fonts.mail"),
                        Area::Calendar => anchor_id("fonts.calendar"),
                        Area::Social => anchor_id("fonts.social"),
                    };
                    changed |= font_row(ui, (id, area.label()), self.cfg.fonts.slot_mut(area), &families, searching, fonts, &self.keymap);
                    ui.end_row();
                }
            }
        });
        if self.cfg.fonts.same_everywhere {
            theme::prose(ui, egui::RichText::new("The size is that of dialogs and menus; the text of the editor is drawn a little larger, as before.").small().color(theme::palette().muted));
        }
        changed
    }
}

/// One row: the font, the size, a hint when the font is not installed, and the sample text in
/// the chosen font, or in the highlighted one while the list is open (CFG-040). `row` is the
/// anchor id (CFG-073) and the name of the row; the anchor is the name and the controls, not the
/// sample.
fn font_row(ui: &mut Ui, (id, label): (&str, &str), choice: &mut FontChoice, families: &[String], searching: bool, fonts: &mut FontState, keymap: &Keymap) -> bool {
    let mut changed = false;
    let name = ui.label(label);
    let picker = font_picker::show(ui, label, &mut choice.family, families, searching, keymap, fonts);
    changed |= picker.picked;
    let size_cell = ui.horizontal(|ui| {
        changed |= ui.add(egui::DragValue::new(&mut choice.size).range(MIN_SIZE..=MAX_SIZE).speed(0.1).suffix(" pt").max_decimals(1)).on_hover_text("Text size in points").changed();
        if let (Some(f), false) = (&choice.family, searching) {
            if !families.contains(f) {
                ui.label(egui::RichText::new("not installed").small().color(theme::palette().link_missing));
            }
        }
    });
    mark(ui, id, name.rect.union(picker.rect).union(size_cell.response.rect));
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
            ui.label(egui::RichText::new("Loading the font…").small().color(theme::palette().muted));
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
        let fmt = TextFormat {
            font_id: FontId::new(font.size, st.family),
            color: if i == 1 { theme::palette().text_strong } else { theme::palette().text },
            italics: st.fake_italic,
            extra_letter_spacing: if st.fake_bold { 0.3 } else { 0.0 },
            ..Default::default()
        };
        job.append(piece, 0.0, fmt);
    }
    job
}
