//! The folder part of the export dialog (BOOK-010, #306): the reading order of the book, to check
//! before exporting, and the export itself as a background job with progress and Cancel (JOB-001).

use crate::pdf::book::{self, Plan};
use crate::pdf::book_export::{export_book, Progress};
use crate::pdf::export::ExportOptions;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Ui};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// A running export. Dropping it (the dialog closed, or Cancel) stops it between two notes.
pub struct Job {
    rx: Receiver<Result<Result<Vec<u8>, String>, String>>,
    progress: Arc<Progress>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.progress.cancel.store(true, Ordering::SeqCst);
    }
}

/// The choices of the dialog of a folder that a note's dialog has not.
pub struct BookExport {
    /// The folder: `""` is the vault.
    pub folder: PathBuf,
    /// The reading order, or why there is no book.
    pub plan: Result<Plan, String>,
    pub job: Option<Job>,
}

impl BookExport {
    pub fn new(vault: &Vault, folder: &Path) -> Self {
        BookExport { folder: folder.to_path_buf(), plan: book::plan(vault, folder), job: None }
    }

    pub fn working(&self) -> bool {
        self.job.is_some()
    }

    /// Whether Export can be pressed: there is a book and no export running.
    pub fn can_export(&self) -> bool {
        self.plan.is_ok() && self.job.is_none()
    }

    /// Lay the book out as a job: the vault at `root` is read again there, so the window stays free.
    pub fn start(&mut self, root: PathBuf, opts: ExportOptions, notify: impl FnOnce() + Send + 'static) {
        let progress = Arc::new(Progress::default());
        let (p, folder) = (progress.clone(), self.folder.clone());
        let rx = crate::jobs::spawn(
            move || {
                let vault = Vault::open(&root).map_err(|e| format!("Could not read the vault: {e}"))?;
                export_book(&vault, &folder, &opts, Some(&p))
            },
            notify,
        );
        self.job = Some(Job { rx, progress });
    }

    /// Stop the export that is running.
    pub fn cancel(&mut self) {
        self.job = None;
    }

    /// What the finished job made, once: the PDF or why not.
    pub fn poll(&mut self) -> Option<Result<Vec<u8>, String>> {
        let got = self.job.as_ref()?.rx.try_recv().ok()?;
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
        let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
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
                                rich.color(theme::MUTED)
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
            rec("working", ui.label(muted(&format!("Laying out the book: {done} of {total} notes. Cancel stops it."))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BOOK-010
    #[test]
    fn cancel_stops_the_job_and_dropping_the_dialog_does_too() {
        let dir = std::env::temp_dir().join(format!("shale-book-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("B")).unwrap();
        std::fs::write(dir.join("B/a.md"), "x\n").unwrap();
        let vault = Vault::open(&dir).unwrap();
        let mut book = BookExport::new(&vault, Path::new("B"));
        assert!(book.can_export() && !book.working() && book.progress().is_none());
        book.start(dir.clone(), ExportOptions::for_note(), || {});
        assert!(book.working() && !book.can_export(), "no second export beside the running one");
        let progress = book.job.as_ref().unwrap().progress.clone();
        book.cancel();
        assert!(!book.working() && book.poll().is_none() && progress.cancel.load(Ordering::SeqCst), "Cancel tells the job to stop");
        book.start(dir.clone(), ExportOptions::for_note(), || {});
        let progress = book.job.as_ref().unwrap().progress.clone();
        drop(book);
        assert!(progress.cancel.load(Ordering::SeqCst), "a closed dialog stops it too");
        // a book that was done is handed over once
        let mut book = BookExport::new(&vault, Path::new("B"));
        book.start(dir.clone(), ExportOptions::for_note(), || {});
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
