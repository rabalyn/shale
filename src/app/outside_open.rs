//! A click on an outside link (MEDIA-017): the file is looked for and handed to the system's program
//! in a background job, because a drive that is slow or not there would freeze the window
//! (MEDIA-024); a toast says why when nothing was opened.

use super::ShaleApp;
use crate::media::outside::{self, Library, Outside, Spot};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};

/// An outside file that is being opened: the result of the job, in words when it failed.
pub type Opening = Receiver<Result<Result<(), String>, String>>;

/// Look the file of an outside link up and open it: what the job of a click does.
fn open(spot: &Spot, libraries: &[Library]) -> Result<(), String> {
    match outside::resolve(spot, libraries) {
        Outside::Found { abs, .. } => crate::opener::open_outside_media(&abs),
        other => Err(other.notice().unwrap_or_default()),
    }
}

impl ShaleApp {
    /// Open the file of the library link or file link `spot` with the system's program (MEDIA-017).
    pub(super) fn open_outside(&mut self, spot: Spot) {
        let Some(v) = self.vault.as_ref() else { return };
        let libraries = v.libraries().to_vec();
        self.open_in_background(move || open(&spot, &libraries));
    }

    /// Open the outside media file `abs` with the system's program (a video that cannot play here, MEDIA-010).
    pub(super) fn open_outside_file(&mut self, abs: PathBuf) {
        self.open_in_background(move || crate::opener::open_outside_media(&abs));
    }

    fn open_in_background(&mut self, f: impl FnOnce() -> Result<(), String> + Send + 'static) {
        let ctx = self.ctx.clone();
        let job = crate::jobs::spawn(f, move || {
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
        self.outside_opens.push(job);
    }

    /// Every frame: say what went wrong with the files that were to be opened.
    pub(crate) fn outside_open_tick(&mut self) {
        let mut done = Vec::new();
        self.outside_opens.retain(|job| match job.try_recv() {
            Ok(result) => {
                done.push(result.and_then(|r| r));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => false,
        });
        for e in done.into_iter().filter_map(Result::err) {
            self.toast(e);
        }
    }
}
