//! The Appearance tab: how notes, drawings and media look.

use crate::app::ShaleApp;
use crate::theme;
use eframe::egui::{self, Ui};

impl ShaleApp {
    pub(super) fn appearance_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = self.theme_section(ui);
        theme::section_break(ui);
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

/// The theme to choose from: dark ones, then light ones, each group in the order of the list; a
/// theme of the user's themes folder says so (CFG-062).
fn theme_combo(ui: &mut Ui, id: &str, themes: &crate::themes::Themes, name: &mut String) -> bool {
    let mut changed = false;
    let shown = match themes.get(name) {
        Some(t) => theme_label(t),
        None => format!("{name} (not found)"),
    };
    // tall enough for every theme without a scroll bar
    egui::ComboBox::from_id_salt(id).selected_text(shown).height(520.0).show_ui(ui, |ui| {
        for (dark, title) in [(true, "Dark"), (false, "Light")] {
            ui.label(egui::RichText::new(title).color(theme::palette().muted));
            for t in themes.list.iter().filter(|t| t.palette.dark == dark) {
                changed |= ui.selectable_value(name, t.name.clone(), theme_label(t)).changed();
            }
        }
    });
    changed
}

fn theme_label(t: &crate::themes::Theme) -> String {
    if t.user {
        format!("{} (yours)", t.name)
    } else {
        t.name.clone()
    }
}

/// The page of the themes: which one, the system's light and dark mode, and the user's files.
const THEME_HELP: &str = "A theme is a terminal colour scheme in the format of Alacritty: put a .toml file into the themes folder and it is listed here by its file name. A file named like one of the built-in themes replaces it.";

impl ShaleApp {
    /// The theme picker (CFG-062) and the buttons for the themes folder (CFG-065).
    fn theme_section(&mut self, ui: &mut Ui) -> bool {
        // the folder is read again whenever the tab is opened
        let pass = ui.ctx().cumulative_pass_nr();
        let id = egui::Id::unique("themes-tab-pass");
        let last = ui.ctx().data_mut(|d| {
            let last = d.get_temp::<u64>(id);
            d.insert_temp(id, pass);
            last
        });
        if last.is_none_or(|last| last + 1 < pass) {
            self.themes.reload();
        }
        let mut changed = false;
        let themes = self.themes.themes().clone();
        let cfg = &mut self.cfg.theme;
        changed |= ui.checkbox(&mut cfg.follow_system, "Follow the system's light or dark mode").on_hover_text("Use one theme while the system is in light mode and another while it is in dark mode").changed();
        if cfg.follow_system {
            ui.horizontal(|ui| {
                ui.label("Light theme");
                changed |= theme_combo(ui, "theme-light", &themes, &mut cfg.light);
            });
            ui.horizontal(|ui| {
                ui.label("Dark theme");
                changed |= theme_combo(ui, "theme-dark", &themes, &mut cfg.dark);
            });
        } else {
            ui.horizontal(|ui| {
                ui.label("Theme");
                changed |= theme_combo(ui, "theme", &themes, &mut cfg.name);
            });
        }
        let missing: Vec<&String> = if cfg.follow_system { vec![&cfg.light, &cfg.dark] } else { vec![&cfg.name] };
        for name in missing.into_iter().filter(|n| themes.get(n).is_none()) {
            theme::problem(ui, format!("There is no theme called \"{name}\", so Shale's own theme is used."));
        }
        ui.horizontal(|ui| {
            if ui.button("Open themes folder").on_hover_text(THEME_HELP).clicked() {
                match self.themes.dir().map(crate::opener::open_themes_folder) {
                    Some(Ok(())) => {}
                    Some(Err(e)) => self.toast(e),
                    None => self.toast("There is no themes folder"),
                }
            }
            if ui.button("Reload themes").on_hover_text("Read the themes folder again").clicked() {
                self.themes.reload();
            }
        });
        for e in &themes.errors {
            theme::problem(ui, format!("Theme file skipped: {e}"));
        }
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
