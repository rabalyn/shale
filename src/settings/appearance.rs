//! The Appearance tab: how notes, drawings and media look.

use crate::app::ShaleApp;
use crate::theme;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn appearance_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        changed |= ui.checkbox(&mut self.cfg.readable_width, "Readable line length").changed();
        changed |= ui.checkbox(&mut self.cfg.dark_canvas, "Dark drawing canvas").changed();
        changed |= properties_combo(ui, "Properties in the editor", "properties-editor", &mut self.cfg.properties_in_editor);
        changed |= properties_combo(ui, "Properties in the reading view", "properties-reader", &mut self.cfg.properties_in_reader);
        changed |= ui.checkbox(&mut self.cfg.inline_images, "Show embedded images and players inside the editor (the link text appears while the cursor is on it)").changed();
        ui.horizontal(|ui| {
            ui.label("Play button over the artwork of a sound or video");
            changed |= ui
                .add(
                    egui::Slider::new(&mut self.cfg.media_play_button_opacity, 0.0..=1.0)
                        .custom_formatter(|v, _| format!("{:.0} %", v * 100.0))
                        .custom_parser(|t| t.trim().trim_end_matches('%').trim().parse::<f64>().ok().map(|v| v / 100.0)),
                )
                .on_hover_text("How opaque the round button is; it is fully opaque while the mouse is over it")
                .changed();
        });
        // videos are decoded by the system's ffmpeg (MEDIA-010)
        match self.media.ffmpeg() {
            Some(p) => ui.label(egui::RichText::new(format!("Videos are played with ffmpeg ({})", p.display())).small().color(theme::palette().muted)),
            None => ui.label(egui::RichText::new(crate::media::ffmpeg::MISSING_HELP).small().color(theme::palette().link_missing)),
        };
        changed
    }
}

/// A choice of how the properties of a note are shown (CFG-050, CFG-051).
fn properties_combo(ui: &mut Ui, label: &str, id: &str, view: &mut crate::config::PropertiesView) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id).selected_text(view.label()).show_ui(ui, |ui| {
            for v in crate::config::PropertiesView::ALL {
                changed |= ui.selectable_value(view, v, v.label()).changed();
            }
        });
    })
    .response
    .on_hover_text(
        "The properties are the lines between the two --- at the top of a note. Folded: one line with the type, the most telling values and the tags. Card: a header made for the type of the note (mail, event, Word file). Raw: every line.",
    );
    changed
}
