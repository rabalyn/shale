//! "Copy into the vault" in the window (MEDIA-021): the files that the outside links of a note
//! name are copied to `z_media/…` in a background job (the files may be big and on a slow disk,
//! JOB-001), and when the job has ended the note's links point at the copies.

use super::{ShaleApp, View};
use crate::media::copy_in::{self, Copied};
use crate::vault::rel_to_string;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};

/// The copy that runs: the note whose links it is for.
pub struct OutsideCopy {
    rel: PathBuf,
    job: Receiver<Result<Copied, String>>,
}

impl ShaleApp {
    /// Copy the files of the outside links of the note `rel` into the vault: of the one link with
    /// this target (`only`), or of all of them. One copy runs at a time.
    pub fn copy_outside(&mut self, rel: &Path, only: Option<String>) {
        if self.outside_copy.is_some() {
            self.toast("Files are being copied into the vault already");
            return;
        }
        let (Some(v), Some(doc)) = (self.vault.as_ref(), self.notes.get(rel)) else { return };
        if doc.unreadable.is_some() {
            self.toast(format!("“{}” could not be read, so it is not changed", rel_to_string(rel)));
            return;
        }
        let mut targets = v.outside_targets(&doc.text, rel);
        targets.retain(|t| only.as_ref().is_none_or(|o| o == t));
        if targets.is_empty() {
            self.toast("This note links no media from outside the vault");
            return;
        }
        let (root, libraries, date) = (v.root.clone(), v.libraries().to_vec(), crate::media::local_date());
        let ctx = self.ctx.clone();
        let count = targets.len();
        self.outside_copy = Some(OutsideCopy {
            rel: rel.to_path_buf(),
            job: crate::jobs::spawn(
                move || copy_in::copy_into(&root, &libraries, &targets, date),
                move || {
                    if let Some(c) = ctx {
                        c.request_repaint();
                    }
                },
            ),
        });
        self.toast(format!("Copying {count} {} into the vault…", if count == 1 { "file" } else { "files" }));
    }

    /// Every frame: collect a finished copy.
    pub(crate) fn outside_copy_tick(&mut self) {
        let Some(c) = &self.outside_copy else { return };
        let result = match c.job.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("the copy stopped unexpectedly".into()),
        };
        let Some(OutsideCopy { rel, .. }) = self.outside_copy.take() else { return };
        match result {
            Ok(copied) => self.copied_into_vault(&rel, copied),
            Err(e) => self.toast(format!("Could not copy into the vault: {e}")),
        }
    }

    /// The files are in the vault: the new files are indexed, and the note's links point at them.
    /// A note that is open for editing gets them in its next frame, as one step of its undo (the
    /// editor knows the cursor); in the reading view, or when no editor shows the note, the text
    /// is changed and saved here.
    fn copied_into_vault(&mut self, rel: &Path, copied: Copied) {
        let mut message = copied.message();
        if !copied.done.is_empty() {
            self.rescan();
            let mut editing = false;
            for t in self.all_tabs_mut().filter(|t| matches!(&t.view, View::Note(p) if p == rel) && !t.reading) {
                t.editor.pending_relink = Some((rel.to_path_buf(), copied.done.clone()));
                editing = true;
            }
            if !editing && self.notes.contains_key(rel) {
                self.relink_note(rel, &copied.done);
            } else if !editing {
                message.push_str(&format!(". “{}” is not open, so its links were left as they are", rel_to_string(rel)));
            }
        }
        self.toast(message);
    }

    /// Point the links of the note `rel` at the copies, without an editor: the open buffer is changed and saved
    /// like a ticked task (EDIT-007).
    fn relink_note(&mut self, rel: &Path, done: &[(String, PathBuf)]) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        let Some(doc) = self.notes.get_mut(rel) else { return };
        let Some(r) = copy_in::relink(&doc.text, done) else { return };
        doc.text = r.text;
        doc.dirty = true;
        doc.last_edit = std::time::Instant::now();
        let text = doc.text.clone();
        if let Some(v) = self.vault.as_mut() {
            v.set_note_text(rel, &text);
        }
        self.save_note(rel);
    }
}
