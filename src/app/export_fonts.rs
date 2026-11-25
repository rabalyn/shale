//! The window's side of the fonts of the exports (PDF-034, JOB-040): the step that runs the font
//! scan, and the line of the export dialog that says which fonts the note gets (PDF-019).

use super::ShaleApp;
use crate::fonts::resolve::{self, Engine, Request, Resolved, Role, Values};
use crate::pdf::export_dialog::{ExportDialog, FontsNote};
use crate::pdf::export_fonts::FontsAsk;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a built-in export waits for the scan of the installed fonts when it needs them (PDF-037).
pub(crate) const FONTS_WAIT: Duration = Duration::from_secs(20);

impl ShaleApp {
    /// Look the fonts up in the background when they are wanted and not yet known (a task of the
    /// busy registry), and take in the result.
    pub fn font_scan_tick(&mut self, ctx: &eframe::egui::Context) {
        let ctx = ctx.clone();
        self.font_scan.tick(self.vault.as_ref(), &self.busy, move || ctx.request_repaint());
    }

    /// What the built-in export of the dialog looks fonts up in (PDF-036, PDF-037): the scan as it
    /// is, and the vault's settings. `note` is the note's values, or the top folder note's of a
    /// book. None while the export has to wait: a font the vault does not have may be an installed
    /// one, and the scan (JOB-040, the task of the busy registry) has not finished. The dialog
    /// then says so and the window asks again every frame (`d.fonts_wait`); after
    /// [`FONTS_WAIT`] the export goes on, and says in its warnings that the installed fonts were not looked up.
    pub(crate) fn export_fonts_ask(&mut self, d: &mut ExportDialog, note: Values) -> Option<Arc<FontsAsk>> {
        self.font_scan.request();
        // the fields of the dialog say what the note says (PDF-039)
        let note = d.font_values().unwrap_or(note);
        let vault = Values::of_settings(&self.vdata.settings);
        let catalogue = self.font_scan.catalogue();
        let root = self.vault.as_ref().map(|v| v.root.clone()).unwrap_or_default();
        let request = Request { engine: Engine::Pdf, vault_root: &root, note, book: false, chapters: Vec::new(), set: None, vault: vault.clone() };
        let mut late = false;
        if resolve::resolve(&request, &catalogue).pending {
            let since = *d.fonts_wait.get_or_insert_with(Instant::now);
            if since.elapsed() < FONTS_WAIT {
                return None;
            }
            late = true;
        }
        d.fonts_wait = None;
        Some(Arc::new(FontsAsk { catalogue, vault, late }))
    }

    /// The top folder note of a folder's book, whose fonts the book has (BOOK-022).
    pub(crate) fn book_top_note(d: &ExportDialog) -> Option<std::path::PathBuf> {
        d.book.as_ref().and_then(|b| b.plan.as_ref().ok()).and_then(|p| p.note.clone().or_else(|| p.intro.clone()))
    }

    /// Keep the font fields and the fonts line of an export dialog up to date (PDF-019, PDF-039):
    /// fill the fields with the note's (a book's: its top note's) values when the dialog is new,
    /// take in the families of a new scan result, and make the line again when the scan, the
    /// engine, the set or a field changed.
    pub(crate) fn export_fonts_tick(&mut self, d: &mut ExportDialog) {
        if d.drawing {
            return;
        }
        let Some(vault) = &self.vault else { return };
        self.font_scan.request();
        let catalogue = self.font_scan.catalogue();
        let vault_values = Values::of_settings(&self.vdata.settings);
        let set = d.tex.as_deref().filter(|_| d.latex()).and_then(|t| t.sets.get(t.set)).map(|s| (s.id.clone(), s.settings.fonts()));
        if !d.font_fields.loaded {
            let note = match (d.book.is_some(), Self::book_top_note(d)) {
                (true, top) => top.map(|rel| Values::of_note(&self.note_text(&rel))).unwrap_or_default(),
                (false, _) => Values::of_note(&self.note_text(&d.rel)),
            };
            d.font_fields.fill(&note);
        }
        if d.font_fields.generation != catalogue.generation || !d.font_fields.loaded {
            d.font_fields.generation = catalogue.generation;
            d.font_fields.list = self.font_scan.families();
        }
        // an empty field comes to the set's font, else the vault's, else the default
        let outer = |role: Role| set.as_ref().and_then(|(_, v)| v.get(role)).or_else(|| vault_values.get(role)).map(String::from);
        for (n, role) in Role::ALL.into_iter().enumerate() {
            d.font_fields.hints[n] = outer(role).unwrap_or_else(|| default_hint(role, d.latex()).to_string());
        }
        let values = d.font_fields.values();
        let stamp = (catalogue.generation, d.latex(), d.tex.as_deref().map_or(0, |t| t.set), values.clone());
        if d.fonts.as_ref().is_some_and(|f| f.stamp == stamp) {
            return;
        }
        let request = Request { engine: if d.latex() { Engine::Tex } else { Engine::Pdf }, vault_root: &vault.root, note: values, book: d.book.is_some(), chapters: Vec::new(), set, vault: vault_values };
        let resolved = resolve::resolve(&request, &catalogue);
        d.fonts = Some(FontsNote { stamp, line: line(&resolved), warnings: resolved.warnings.clone(), pending: resolved.pending || (!catalogue.system_ready && self.font_scan.running()) });
    }
}

/// What an empty field comes to when neither a set nor the vault says (the hint of the field).
fn default_hint(role: Role, tex: bool) -> &'static str {
    match (tex, role) {
        (true, _) => "the template's font",
        (false, Role::Text) => "Ubuntu Light",
        (false, Role::Sans) => "the text font",
        (false, Role::Mono) => "Hack",
    }
}

/// The roles that something was chosen for, in words: "text Garamond (system), code Hack".
fn line(r: &Resolved) -> String {
    let set: Vec<String> = Role::ALL.iter().map(|role| r.pick(*role)).filter(|p| p.entry != resolve::Entry::Default).map(|p| format!("{} {} (from {})", p.role.label(), p.describe(), p.entry.label())).collect();
    set.join(", ")
}
