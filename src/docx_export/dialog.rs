//! The "Export to Word" dialog (DOCX-001): where to save, the original as the template, the table
//! of contents, opening the file afterwards, and the export as a background job (DOCX-002, JOB-001).

use super::Job;
use crate::busy::{self, Busy, Key};
use crate::theme;
use eframe::egui::{self, Rect, Ui};
use std::path::PathBuf;

/// The export of a note to Word, as a task of the busy registry (JOB-019).
pub const DOCX_EXPORT: Key = Key::new("docx-export");

/// A running export. Dropping it (the dialog closed, or Cancel) stops pandoc.
struct Running(busy::Job<Result<Vec<u8>, String>>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub struct DocxDialog {
    pub rel: PathBuf,
    pub name: String,
    pub path: String,
    /// where pandoc was found; none: the export cannot run
    pub pandoc: Option<PathBuf>,
    /// the original Word file of the note, a vault path: the box is shown only with one
    pub original: Option<PathBuf>,
    pub use_original: bool,
    pub toc: bool,
    /// a page break after the table of contents (DOCX-013)
    pub toc_break: bool,
    pub open_after: bool,
    /// what the running export says about the thesis syntax of the note (DOCX-016), told when the file is saved
    pub warnings: Vec<String>,
    pub error: Option<String>,
    job: Option<Running>,
    busy: Busy,
    /// screen rect of every control (used by the UI tests)
    pub rects: Vec<(String, Rect)>,
}

#[derive(PartialEq, Eq, Debug)]
pub enum DialogResult {
    Open,
    Export,
    Cancel,
}

impl DocxDialog {
    pub fn new(rel: PathBuf, name: String, path: PathBuf, pandoc: Option<PathBuf>, original: Option<PathBuf>, toc: bool, toc_break: bool) -> Self {
        DocxDialog {
            rel,
            name,
            path: path.to_string_lossy().into_owned(),
            pandoc,
            use_original: original.is_some(),
            original,
            toc,
            toc_break,
            open_after: false,
            warnings: Vec::new(),
            error: None,
            job: None,
            busy: Busy::default(),
            rects: vec![],
        }
    }

    /// The registry the export is a task of (the window's; a dialog made without one has its own).
    pub fn with_busy(mut self, busy: &Busy) -> Self {
        self.busy = busy.clone();
        self
    }

    pub fn working(&self) -> bool {
        self.job.is_some()
    }

    /// Whether Export can be pressed: pandoc is there and no Word export runs (in this dialog or in
    /// one that was closed while its export was still stopping, JOB-005).
    pub fn can_export(&self) -> bool {
        self.pandoc.is_some() && self.job.is_none() && !self.busy.is_running(&DOCX_EXPORT)
    }

    /// Run the export as a task of the registry (JOB-019); `notify` is called when it has ended (to
    /// repaint the window). False: a Word export already runs, and nothing starts.
    pub fn start(&mut self, job: Job, notify: impl FnOnce() + Send + 'static) -> bool {
        let Some(mut task) = self.busy.begin(DOCX_EXPORT, format!("Exporting “{}” to Word…", self.name)) else { return false };
        self.warnings = job.warnings.clone();
        let cancel = task.cancellable().flag();
        self.job = Some(Running(task.run(move || super::export(&job, &cancel), notify)));
        true
    }

    /// Stop the export that is running.
    pub fn cancel(&mut self) {
        self.job = None;
    }

    /// What the finished job made, once: the Word file or why not.
    pub fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        let got = self.job.as_ref()?.0.try_recv().ok()?;
        self.job = None;
        Some(got.and_then(|r| r))
    }
}

pub fn show(ui: &mut Ui, d: &mut DocxDialog) {
    d.rects.clear();
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: &str, r: egui::Response| -> egui::Response {
        rects.push((k.to_string(), r.rect));
        r
    };
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::palette().muted);
    let busy = d.working();
    ui.add_enabled_ui(!busy, |ui| {
        egui::Grid::new("export-docx-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            ui.label(muted("Save as"));
            rec("path", ui.add(egui::TextEdit::singleline(&mut d.path).desired_width(250.0)));
            ui.end_row();
            ui.label(muted("Include"));
            ui.vertical(|ui| {
                if let Some(original) = &d.original {
                    let hint = format!("Its styles, fonts, headers and footers come from {}", crate::vault::rel_to_string(original));
                    rec("use-original", crate::clickable::pointer(ui.checkbox(&mut d.use_original, "Use the original Word file as the template"))).on_hover_text(hint);
                }
                rec("toc", crate::clickable::pointer(ui.checkbox(&mut d.toc, "Table of contents")));
                ui.add_enabled_ui(d.toc, |ui| rec("toc-break", crate::clickable::pointer(ui.checkbox(&mut d.toc_break, "Page break after the table of contents"))));
            });
            ui.end_row();
        });
        rec("open-after", crate::clickable::pointer(ui.checkbox(&mut d.open_after, "Open the file afterwards")));
    });
    match &d.pandoc {
        Some(p) => rec("pandoc", ui.label(muted(&format!("pandoc: {}", p.display())))),
        None => rec("pandoc", theme::problem(ui, crate::import::docx::MISSING)),
    };
    if busy {
        ui.horizontal(|ui| {
            ui.add(egui::Spinner::new().size(14.0));
            rec("working", ui.label(muted("Exporting to Word… Cancel stops it.")));
        });
    }
    d.rects = rects;
}

/// The dialog's footer: the error of the last try, Export and Cancel. Call it after [`show`] in the same frame.
pub fn footer(ui: &mut Ui, d: &mut DocxDialog) -> DialogResult {
    let mut result = DialogResult::Open;
    if let Some(e) = &d.error {
        theme::problem(ui, e);
    }
    let ready = d.can_export();
    ui.horizontal(|ui| {
        let mut rec = |k: &str, r: egui::Response| -> egui::Response {
            d.rects.push((k.to_string(), r.rect));
            r
        };
        if rec("export", ui.add_enabled(ready, egui::Button::new(egui::RichText::new("Export").strong()))).clicked() {
            result = DialogResult::Export;
        }
        if rec("cancel", ui.button("Cancel")).clicked() {
            result = DialogResult::Cancel;
        }
    });
    result
}
