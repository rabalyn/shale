//! The window's side of the fonts of the exports (PDF-034, JOB-040): the step that runs the font
//! scan, and the line of the export dialog that says which fonts the note gets (PDF-019).

use super::ShaleApp;
use crate::fonts::resolve::{self, Engine, Request, Resolved, Role, Values};
use crate::pdf::export_dialog::{ExportDialog, FontsNote};

impl ShaleApp {
    /// Look the fonts up in the background when they are wanted and not yet known (a task of the
    /// busy registry), and take in the result.
    pub fn font_scan_tick(&mut self, ctx: &eframe::egui::Context) {
        let ctx = ctx.clone();
        self.font_scan.tick(self.vault.as_ref(), &self.busy, move || ctx.request_repaint());
    }

    /// Make the fonts line of a note's export dialog again when the scan has a new result or the
    /// engine of the dialog changed.
    pub(crate) fn export_fonts_tick(&self, d: &mut ExportDialog) {
        if d.drawing || d.book.is_some() {
            return;
        }
        let catalogue = self.font_scan.catalogue();
        let set = d.tex.as_deref().filter(|_| d.latex()).and_then(|t| t.sets.get(t.set)).map(|s| s.id.clone());
        let stamp = (catalogue.generation, d.latex(), d.tex.as_deref().map_or(0, |t| t.set));
        if d.fonts.as_ref().is_some_and(|f| f.stamp == stamp) {
            return;
        }
        let Some(vault) = &self.vault else { return };
        let request = Request {
            engine: if d.latex() { Engine::Tex } else { Engine::Pdf },
            vault_root: &vault.root,
            note: Values::of_note(&self.note_text(&d.rel)),
            book: false,
            chapters: Vec::new(),
            // the values of a set's manifest are read by the LaTeX export (TEX-059, phase 4)
            set: set.map(|id| (id, Values::default())),
            vault: Values::of_settings(&self.vdata.settings),
        };
        let resolved = resolve::resolve(&request, &catalogue);
        d.fonts = Some(FontsNote { stamp, line: line(&resolved), warnings: resolved.warnings.clone(), pending: resolved.pending || (!catalogue.system_ready && self.font_scan.running()) });
    }
}

/// The roles that something was chosen for, in words: "text Garamond (system), code Hack".
fn line(r: &Resolved) -> String {
    let set: Vec<String> = Role::ALL.iter().map(|role| r.pick(*role)).filter(|p| p.entry != resolve::Entry::Default).map(|p| format!("{} {} (from {})", p.role.label(), p.describe(), p.entry.label())).collect();
    set.join(", ")
}
