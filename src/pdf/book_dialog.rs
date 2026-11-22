//! The folder part of the export dialog (BOOK-010, #306): the reading order of the book, to check
//! before exporting, and the export itself as a background job with progress and Cancel (JOB-001),
//! a task of the busy registry (JOB-013): while it runs, no second export starts and the corner
//! indicator lists it with its own Cancel.

use crate::busy::{self, Busy, Key};
use crate::pdf::book::{self, Plan};
use crate::pdf::book_export::{export_book, Progress};
use crate::pdf::export::ExportOptions;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Ui};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

/// The export of a folder as one PDF, as a task (JOB-013).
pub const PDF_EXPORT: Key = Key::new("pdf-export");

/// A running export. Dropping it (the dialog closed, or Cancel) stops it between two notes.
pub struct Job {
    job: busy::Job<Result<Vec<u8>, String>>,
    progress: Arc<Progress>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.job.cancel();
    }
}

/// The choices of the dialog of a folder that a note's dialog has not.
pub struct BookExport {
    /// The folder: `""` is the vault.
    pub folder: PathBuf,
    /// The reading order, or why there is no book.
    pub plan: Result<Plan, String>,
    pub job: Option<Job>,
    busy: Busy,
}

impl BookExport {
    pub fn new(vault: &Vault, folder: &Path, busy: &Busy) -> Self {
        BookExport { folder: folder.to_path_buf(), plan: book::plan(vault, folder), job: None, busy: busy.clone() }
    }

    pub fn working(&self) -> bool {
        self.job.is_some()
    }

    /// Whether Export can be pressed: there is a book and no export of a book runs (in this dialog
    /// or in one that was closed while its export was still stopping).
    pub fn can_export(&self) -> bool {
        self.plan.is_ok() && self.job.is_none() && !self.busy.is_running(&PDF_EXPORT)
    }

    /// Lay the book out as a job: the vault at `root` is read again there, so the window stays free.
    /// False: an export of a book already runs, and nothing starts.
    pub fn start(&mut self, root: PathBuf, opts: ExportOptions, notify: impl FnOnce() + Send + 'static) -> bool {
        let label = match &self.plan {
            Ok(plan) => format!("Exporting “{}” to PDF…", plan.title),
            Err(_) => "Exporting to PDF…".to_string(),
        };
        let Some(mut task) = self.busy.begin(PDF_EXPORT, label) else { return false };
        let progress = Arc::new(Progress::with_cancel(task.cancellable()));
        let (p, folder) = (progress.clone(), self.folder.clone());
        let job = task.run(
            move || {
                let vault = Vault::open(&root).map_err(|e| format!("Could not read the vault: {e}"))?;
                export_book(&vault, &folder, &opts, Some(&p))
            },
            notify,
        );
        self.job = Some(Job { job, progress });
        true
    }

    /// Stop the export that is running.
    pub fn cancel(&mut self) {
        self.job = None;
    }

    /// What the finished job made, once: the PDF or why not.
    pub fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        let got = self.job.as_ref()?.job.try_recv().ok()?;
        self.job = None;
        Some(got.and_then(|r| r))
    }

    /// The notes laid out so far and all of them, while a job runs.
    pub fn progress(&self) -> Option<(usize, usize)> {
        let p = &self.job.as_ref()?.progress;
        Some((p.done.load(Ordering::SeqCst), p.total.load(Ordering::SeqCst)))
    }

    /// The collapsible *Reading order* and the state of the export.
    pub fn show(&mut self, ui: &mut Ui, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
        let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::palette().muted);
        match &self.plan {
            Err(e) => {
                rec("no-book", crate::theme::problem(ui, e));
            }
            Ok(plan) => {
                let head = egui::CollapsingHeader::new("Reading order").id_salt("book-order").default_open(false).show(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        for (n, l) in plan.lines().iter().enumerate() {
                            let indent = l.depth as f32 * 14.0;
                            let text = match &l.number {
                                Some(num) => format!("{num}  {}", l.title),
                                None => format!("{}  (left out: export: false)", l.title),
                            };
                            let mut rich = egui::RichText::new(text).size(12.5);
                            rich = if l.left_out {
                                rich.color(theme::palette().muted)
                            } else if l.part {
                                rich.strong()
                            } else {
                                rich
                            };
                            ui.horizontal(|ui| {
                                ui.add_space(indent);
                                rec(&format!("order:{n}"), ui.label(rich));
                            });
                        }
                    });
                });
                rec("order", head.header_response);
            }
        }
        if let Some((done, total)) = self.progress() {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(14.0));
                rec("working", ui.label(muted(&format!("Laying out the book: {done} of {total} notes. Cancel stops it."))));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_until_idle(busy: &Busy) {
        let start = std::time::Instant::now();
        while busy.is_running(&PDF_EXPORT) {
            assert!(start.elapsed().as_secs() < 30, "the job ends");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    // spec: BOOK-010, JOB-013
    #[test]
    fn cancel_stops_the_job_and_dropping_the_dialog_does_too() {
        let dir = std::env::temp_dir().join(format!("shale-book-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("B")).unwrap();
        std::fs::write(dir.join("B/a.md"), "x\n").unwrap();
        let vault = Vault::open(&dir).unwrap();
        let busy = Busy::default();
        let mut book = BookExport::new(&vault, Path::new("B"), &busy);
        assert!(book.can_export() && !book.working() && book.progress().is_none());
        assert!(book.start(dir.clone(), ExportOptions::for_note(), || {}));
        assert!(book.working() && !book.can_export(), "no second export beside the running one");
        let listed = busy.info(&PDF_EXPORT);
        assert!(listed.as_ref().is_some_and(|t| t.cancellable && t.label.starts_with("Exporting “") && t.label.ends_with("” to PDF…")), "{listed:?}");
        let other = BookExport::new(&vault, Path::new("B"), &busy);
        assert!(!other.can_export(), "another dialog waits too (JOB-005)");
        assert!(other.busy.begin(PDF_EXPORT, "x").is_none());
        drop(other);
        let progress = book.job.as_ref().unwrap().progress.clone();
        book.cancel();
        assert!(!book.working() && book.poll().is_none() && progress.cancel.is_cancelled(), "Cancel tells the job to stop");
        wait_until_idle(&busy);
        assert!(book.start(dir.clone(), ExportOptions::for_note(), || {}));
        let progress = book.job.as_ref().unwrap().progress.clone();
        drop(book);
        assert!(progress.cancel.is_cancelled(), "a closed dialog stops it too");
        wait_until_idle(&busy);
        // a book that was done is handed over once
        let mut book = BookExport::new(&vault, Path::new("B"), &busy);
        assert!(book.start(dir.clone(), ExportOptions::for_note(), || {}));
        let start = std::time::Instant::now();
        let made = loop {
            if let Some(m) = book.poll() {
                break m;
            }
            assert!(start.elapsed().as_secs() < 30, "the job ends");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        assert!(made.unwrap().starts_with(b"%PDF") && book.poll().is_none() && !book.working());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
