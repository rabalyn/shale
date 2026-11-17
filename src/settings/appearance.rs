//! The Appearance tab: how notes, drawings and media look.

use crate::app::ShaleApp;
use crate::theme;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn appearance_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        changed |= ui.checkbox(&mut self.cfg.readable_width, "Readable line length").changed();
        changed |= ui.checkbox(&mut self.cfg.dark_canvas, "Dark drawing canvas").changed();
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
            Some(p) => ui.label(egui::RichText::new(format!("Videos are played with ffmpeg ({})", p.display())).small().color(theme::MUTED)),
            None => ui.label(egui::RichText::new(crate::media::ffmpeg::MISSING_HELP).small().color(theme::LINK_MISSING)),
        };
        changed
    }
}
