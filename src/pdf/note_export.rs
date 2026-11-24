//! The PDF export of one note or drawing with the built-in writer (PDF-001), off the window's
//! thread and a task of the busy registry (JOB-017): the layout of a long note with many pictures
//! took the window's time, so it runs as a job that looks at Cancel before each picture. It shares
//! the key of the export of a folder ([`PDF_EXPORT`]): one built-in PDF export at a time. Nothing is
//! written by the job: the window saves the bytes it hands over, so a stopped export leaves no file.

use crate::busy::{self, Busy, Cancel};
use crate::drawing::Scene;
use crate::pdf::book_dialog::PDF_EXPORT;
use crate::pdf::export::{self, ExportOptions};
use crate::pdf::export_fonts::Made;
use crate::vault::Vault;
use std::path::PathBuf;

/// What to export, with everything the job needs: it opens the vault at `root` again (as the
/// export of a folder does), so the window does not wait for pictures to be read.
pub struct Work {
    pub root: PathBuf,
    pub rel: PathBuf,
    pub name: String,
    pub opts: ExportOptions,
    pub what: What,
}

pub enum What {
    /// the text of a note as the window has it
    Note(String),
    /// a drawing as the window has it
    Drawing(Box<Scene>),
}

/// A running export. Dropping it (the dialog closed, or Cancel) stops it before the next picture.
pub struct NoteExport(busy::Job<Result<Made, String>>, std::cell::RefCell<Vec<String>>);

impl Drop for NoteExport {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

impl NoteExport {
    /// Start the export as a task "Exporting “name” to PDF…". `None`: a built-in PDF export already
    /// runs, and nothing starts (JOB-005). `notify` is called when the job has ended.
    pub fn start(busy: &Busy, work: Work, notify: impl FnOnce() + Send + 'static) -> Option<NoteExport> {
        let mut task = busy.begin(PDF_EXPORT, format!("Exporting “{}” to PDF…", work.name))?;
        let cancel = task.cancellable();
        Some(NoteExport(task.run(move || make(&work, &cancel), notify), Default::default()))
    }

    /// What the finished job made, once: the PDF or why not.
    pub fn poll(&self) -> Option<Result<Vec<u8>, String>> {
        let done = self.0.try_recv().ok().map(|r| r.and_then(|r| r))?;
        Some(done.map(|made| {
            *self.1.borrow_mut() = made.warnings;
            made.bytes
        }))
    }

    /// What the export that has just ended has to say about fonts (PDF-037), once.
    pub fn take_warnings(&self) -> Vec<String> {
        std::mem::take(&mut self.1.borrow_mut())
    }
}

fn make(work: &Work, cancel: &Cancel) -> Result<Made, String> {
    let stop = || if cancel.is_cancelled() { Err(export::CANCELLED.to_string()) } else { Ok(()) };
    stop()?;
    let vault = Vault::open(&work.root).map_err(|e| format!("Could not read the vault: {e}"))?;
    stop()?;
    match &work.what {
        What::Note(text) => export::export_note_until(&vault, &work.rel, text, &work.opts, Some(cancel)),
        What::Drawing(scene) => {
            let pictures = crate::drawing::pictures::images(scene, &vault, &work.rel);
            stop()?;
            export::export_drawing(scene, &work.name, &work.opts, &pictures).map(|bytes| Made { bytes, warnings: Vec::new() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn temp_vault(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shale-note-export-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbaImage::from_pixel(40, 20, image::Rgba([200, 30, 30, 255])).save(dir.join("a.png")).unwrap();
        std::fs::write(dir.join("Pics.md"), "# Pics\n\nBefore.\n\n![[a.png]]\n\n![[a.png]]\n\nAfter.\n").unwrap();
        dir
    }

    fn wait(job: &NoteExport) -> Result<Vec<u8>, String> {
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(done) = job.poll() {
                return done;
            }
            assert!(Instant::now() < end, "the export ends");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn work(dir: &Path, what: What, name: &str) -> Work {
        Work { root: dir.to_path_buf(), rel: PathBuf::from(if matches!(what, What::Note(_)) { "Pics.md" } else { "Map.excalidraw" }), name: name.into(), opts: ExportOptions::for_note(), what }
    }

    fn wait_idle(busy: &Busy) {
        let end = Instant::now() + Duration::from_secs(10);
        while busy.is_running(&PDF_EXPORT) {
            assert!(Instant::now() < end, "the task ends with its job");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // spec: PDF-001, JOB-017
    #[test]
    fn a_cancel_before_a_picture_ends_the_layout_with_an_error_and_no_pdf_and_without_it_the_pdf_is_made() {
        let dir = temp_vault("cancel");
        let v = Vault::open(&dir).unwrap();
        let text = std::fs::read_to_string(dir.join("Pics.md")).unwrap();
        let stopped = Cancel::default();
        stopped.cancel();
        let got = export::export_note_until(&v, Path::new("Pics.md"), &text, &ExportOptions::for_note(), Some(&stopped));
        assert_eq!(got.unwrap_err(), export::CANCELLED, "the layout looked at Cancel at the first picture");
        // a Cancel asked for before the layout ends is never a PDF, with pictures or without
        let plain = export::export_note_until(&v, Path::new("Pics.md"), "# Pics\n\nNo pictures.\n", &ExportOptions::for_note(), Some(&stopped));
        assert_eq!(plain.unwrap_err(), export::CANCELLED);
        let free = Cancel::default();
        let made = export::export_note_until(&v, Path::new("Pics.md"), &text, &ExportOptions::for_note(), Some(&free)).unwrap().bytes;
        assert!(made.starts_with(b"%PDF"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: PDF-001, JOB-017, JOB-005
    #[test]
    fn the_export_is_a_task_that_ends_with_its_job_and_a_second_one_is_refused() {
        let dir = temp_vault("task");
        let busy = Busy::default();
        let text = std::fs::read_to_string(dir.join("Pics.md")).unwrap();
        // a task of the key held elsewhere (the export of a folder, or a dialog that was closed while it stopped) refuses it
        let held = busy.begin(PDF_EXPORT, "Exporting “Handbook” to PDF…").unwrap();
        assert!(NoteExport::start(&busy, work(&dir, What::Note(text.clone()), "Pics"), || {}).is_none());
        drop(held);
        let job = NoteExport::start(&busy, work(&dir, What::Note(text), "Pics"), || {}).unwrap();
        let listed = busy.info(&PDF_EXPORT);
        assert!(listed.as_ref().is_some_and(|t| t.cancellable && t.label == "Exporting “Pics” to PDF…"), "{listed:?}");
        assert!(wait(&job).unwrap().starts_with(b"%PDF"));
        drop(job);
        wait_idle(&busy);
        // a drawing runs as the same task and is made too
        let scene = Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap();
        let job = NoteExport::start(&busy, work(&dir, What::Drawing(Box::new(scene.clone())), "Map"), || {}).unwrap();
        assert!(wait(&job).unwrap().starts_with(b"%PDF"));
        drop(job);
        wait_idle(&busy);
        // a drawing that is stopped is not laid out, and a vault that cannot be read is an error
        let cancel = Cancel::default();
        cancel.cancel();
        assert_eq!(make(&work(&dir, What::Drawing(Box::new(scene.clone())), "Map"), &cancel).err().unwrap(), export::CANCELLED);
        let live = Cancel::default();
        let gone = make(&Work { root: dir.join("Pics.md"), ..work(&dir, What::Drawing(Box::new(scene)), "Map") }, &live);
        assert!(gone.err().unwrap().starts_with("Could not read the vault"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
