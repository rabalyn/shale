//! Screenshot mode (`--screenshot`): the state of the run that draws some frames, saves a picture and exits.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// The state of a `--screenshot` run: where the picture goes, and whether it was asked for and saved.
pub(super) struct ScreenshotRun {
    /// where the picture goes (none: a normal run)
    pub(super) path: Option<PathBuf>,
    pub(super) requested: bool,
    pub(super) done: Arc<AtomicBool>,
}

impl ScreenshotRun {
    pub(super) fn new(path: Option<PathBuf>) -> Self {
        Self { path, requested: false, done: Arc::new(AtomicBool::new(false)) }
    }
}
