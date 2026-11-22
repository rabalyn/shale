//! The "Export to Word" dialog in the window (DOCX-001, DOCX-002): opening it for a note, the export
//! as a job, saving the file it makes, and what the person sees afterwards.

use super::{Modal, ShaleApp};
use crate::docx_export::dialog::{self, DialogResult, DocxDialog};
use crate::vault::FileKind;
use eframe::egui;
use std::path::{Path, PathBuf};

impl ShaleApp {
    /// Show the dialog of the Word export of a note, saved to `<vault>/exports/<name>.docx`. It starts with the
    /// table of contents the PDF export would have (PDF-008, PDF-013), and with the original as the template
    /// when the note has one.
    pub fn open_docx_export_dialog(&mut self, rel: &Path) {
        let Some(v) = &self.vault else { return };
        let Some(f) = v.file(rel).filter(|f| f.kind == FileKind::Note) else { return };
        let name = f.display_name();
        let text = self.note_text(rel);
        let toc = crate::pdf::export::ExportOptions { numbered_headings: self.cfg.numbered_headings, ..crate::pdf::export::ExportOptions::for_note() }.with_note_properties(&text).toc;
        let original = crate::docx_export::original_of(v, rel, &text);
        let path = v.root.join("exports").join(format!("{name}.docx"));
        let pandoc = crate::import::docx::locate(&self.cfg.pandoc_path);
        self.modal = Some(Modal::ExportDocx(Box::new(DocxDialog::new(rel.to_path_buf(), name, path, pandoc, original, toc))));
    }

    /// Run the export of the dialog as a job (DOCX-002). The text is the note as the window has it.
    fn start_docx_export(&mut self, d: &mut DocxDialog, ctx: &egui::Context) -> Result<(), String> {
        self.flush_all();
        if d.path.trim().is_empty() {
            return Err("Choose where to save the Word file".into());
        }
        let pandoc = d.pandoc.clone().ok_or(crate::import::docx::MISSING)?;
        let v = self.vault.as_ref().ok_or("No vault is open")?;
        let original = d.original.as_deref().filter(|_| d.use_original);
        let job = crate::docx_export::Job::new(pandoc, v, &d.rel, &self.note_text(&d.rel), original, d.toc);
        let ctx = ctx.clone();
        d.start(job, move || ctx.request_repaint());
        Ok(())
    }

    /// Where the window is with the export: when the job has ended, the file is saved (or why not).
    fn docx_export_tick(&mut self, d: &mut DocxDialog) -> Option<Result<PathBuf, String>> {
        let made = d.poll()?;
        Some(made.and_then(|bytes| self.save_file(&d.path, &bytes, "Word file")))
    }

    /// The dialog of the Word export, once a frame. Returns whether it stays open.
    pub(crate) fn docx_export_modal(&mut self, ctx: &egui::Context, d: &mut DocxDialog) -> bool {
        let mut keep = true;
        let finished = self.docx_export_tick(d);
        if d.working() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        let mut st = (&mut *d, DialogResult::Open);
        let resp = crate::dialog::Dialog::new("export-docx", format!("Export “{}” to Word", st.0.name)).width(460.0).show_with_footer(ctx, &mut st, |ui, (d, _)| dialog::show(ui, d), |ui, (d, out)| *out = dialog::footer(ui, d));
        let (close_asked, result) = (resp.should_close(), st.1);
        match finished {
            Some(Ok(out)) => {
                self.toast(format!("Exported to {}", out.display()));
                if d.open_after {
                    if let Err(e) = crate::opener::open_exported_docx(&out) {
                        self.toast(e);
                    }
                }
                keep = false;
            }
            Some(Err(e)) => d.error = Some(e),
            None => {}
        }
        match result {
            DialogResult::Export => d.error = self.start_docx_export(d, ctx).err(),
            // Cancel stops the export that is running, and a second one closes the dialog
            DialogResult::Cancel if d.working() => d.cancel(),
            DialogResult::Cancel => keep = false,
            DialogResult::Open => {}
        }
        keep && !close_asked
    }
}
