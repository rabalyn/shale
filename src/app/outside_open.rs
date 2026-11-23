//! A click on an outside link (MEDIA-017): the file is looked for and handed to the system's program
//! in a background job, because a drive that is slow or not there would freeze the window
//! (MEDIA-024); a toast says why when nothing was opened.

use super::ShaleApp;
use crate::busy::{Job, Key};
use crate::media::outside::{self, Library, Outside, Spot};
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;

/// The task of opening one outside file (MEDIA-017, JOB-023). Several run side by side, one per
/// file; a second click on the same file while it opens does nothing (JOB-005).
const OUTSIDE_OPEN: &str = "outside-open";

/// An outside file that is being opened: the job, whose result says in words why it failed. It
/// cannot be stopped (the system's program is started or it is not), so it has no Cancel.
pub type Opening = Job<Result<(), String>>;

/// Look the file of an outside link up and open it: what the job of a click does.
fn open(spot: &Spot, libraries: &[Library]) -> Result<(), String> {
    match outside::resolve(spot, libraries) {
        Outside::Found { abs, .. } => crate::opener::open_outside_media(&abs),
        other => Err(other.notice().unwrap_or_default()),
    }
}

/// The name of a file to show.
fn file_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

impl ShaleApp {
    /// Open the file of the library link or file link `spot` with the system's program (MEDIA-017).
    pub(super) fn open_outside(&mut self, spot: Spot) {
        let Some(v) = self.vault.as_ref() else { return };
        let libraries = v.libraries().to_vec();
        let name = match &spot {
            Spot::Library { name, rest } => format!("@{name}/{rest}"),
            Spot::File(p) => p.to_string_lossy().into_owned(),
        };
        let shown = match &spot {
            Spot::Library { rest, .. } => rest.rsplit('/').next().unwrap_or(rest).to_string(),
            Spot::File(p) => file_name(p),
        };
        self.open_in_background(&name, &shown, move || open(&spot, &libraries));
    }

    /// Open the outside media file `abs` with the system's program (a video that cannot play here, MEDIA-010).
    pub(super) fn open_outside_file(&mut self, abs: PathBuf) {
        let (address, shown) = (abs.to_string_lossy().into_owned(), file_name(&abs));
        self.open_in_background(&address, &shown, move || crate::opener::open_outside_media(&abs));
    }

    /// Run `f` as the task "Opening `shown`…"; `address` tells which file it is.
    fn open_in_background(&mut self, address: &str, shown: &str, f: impl FnOnce() -> Result<(), String> + Send + 'static) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        address.hash(&mut h);
        let ctx = self.ctx.clone();
        let notify = move || {
            if let Some(c) = ctx {
                c.request_repaint();
            }
        };
        if let Some(job) = self.busy.spawn(Key::new(OUTSIDE_OPEN).with(&h.finish().to_string()), format!("Opening {shown}…"), f, notify) {
            self.outside_opens.push(job);
        }
    }

    /// Every frame: say what went wrong with the files that were to be opened.
    pub(crate) fn outside_open_tick(&mut self) {
        let mut done = Vec::new();
        self.outside_opens.retain(|job: &Opening| match job.try_recv() {
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
