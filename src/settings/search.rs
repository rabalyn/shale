//! The Search tab (CFG-067, CFG-068): the sources a search looks in by default, how the hits of notes
//! are drawn and the letters of the prefixes that name one source in the query (`f:`, `m:`, `s:`,
//! NAV-050…NAV-054).

use crate::app::ShaleApp;
use crate::search_sources::{check_prefixes, HitDisplay, Kind};
use eframe::egui::{self, Id, Ui};

impl ShaleApp {
    /// Returns whether a setting changed.
    pub(super) fn search_tab(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        crate::theme::section(ui, "Sources");
        crate::theme::hint(ui, "A search without a prefix looks in and lists these sources. The buttons above the results change what is listed for a moment (a click) or this selection (Shift+click).");
        let selection = self.cfg.search.selection();
        for kind in Kind::ALL {
            let mut on = selection.contains(kind);
            // the last source stays: a search with no source finds nothing
            let r = ui.add_enabled(!on || selection.toggled(kind).is_some(), egui::Checkbox::new(&mut on, kind.group()));
            if r.changed() {
                if let Some(sel) = selection.toggled(kind) {
                    self.cfg.search.set_selection(sel);
                    changed = true;
                }
            }
        }
        crate::theme::section(ui, "Hits of notes");
        crate::theme::hint(ui, "How the lines of notes that a search found are drawn: rendered like the editor draws a line (bold, links by their text, tags, task boxes), as the markdown is written, or like the editor shows notes now (rendered when the Markdown preview is on or the note in view is in the reading view). The found text is marked in all of them.");
        let shown = self.cfg.search.display();
        for display in HitDisplay::ALL {
            if ui.radio(shown == display, display.label()).clicked() && shown != display {
                self.cfg.search.hit_display = display.id().into();
                changed = true;
            }
        }
        crate::theme::section(ui, "Prefixes");
        crate::theme::hint(
            ui,
            "A query that starts with a letter and a colon looks in one source only: f:banana in notes, m:banana in mail, s:banana in posts. Letters combine: fm:banana looks in notes and mail. Each prefix is one letter.",
        );
        let id = Id::unique("settings-search-prefixes");
        let cfg = &self.cfg.search;
        let (mut letters, mut error): ([String; 3], Option<String>) = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_else(|| ([cfg.prefix_notes.clone(), cfg.prefix_mail.clone(), cfg.prefix_posts.clone()], None));
        if error.is_none() {
            // follows the settings file when it is edited there
            letters = [cfg.prefix_notes.clone(), cfg.prefix_mail.clone(), cfg.prefix_posts.clone()];
        }
        let mut edited = false;
        ui.horizontal(|ui| {
            for (label, letter) in ["Notes", "Mail", "Posts"].into_iter().zip(letters.iter_mut()) {
                ui.label(label);
                edited |= ui.add(egui::TextEdit::singleline(letter).desired_width(24.0)).changed();
            }
        });
        if edited {
            match check_prefixes(&letters[0], &letters[1], &letters[2]) {
                Ok(_) => {
                    let c = &mut self.cfg.search;
                    [c.prefix_notes, c.prefix_mail, c.prefix_posts] = letters.clone().map(|l| l.trim().to_string());
                    error = None;
                    changed = true;
                }
                Err(e) => error = Some(e),
            }
        }
        if let Some(e) = &error {
            ui.colored_label(ui.visuals().error_fg_color, format!("{e} The letters stay as they were."));
        }
        ui.ctx().data_mut(|d| d.insert_temp(id, (letters, error)));
        changed
    }
}
