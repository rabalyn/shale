//! Screenshot mode (`--screenshot`) and recording mode (`--record`, DEV-019): the state of the run that
//! draws some frames, saves a picture or a clip and exits.

use super::ShaleApp;
use crate::recorder::{self, Recorder};
use eframe::egui::{self, Ui};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// The frame the recording starts at: a beat before the first script step (frame 30).
const RECORD_FROM: u32 = 24;
/// The frames the clip goes on after the last script step: the end state is shown for a second.
const RECORD_TAIL: u32 = 30;
/// A clip longer than this (a minute) is a script that does not end: the run fails.
const RECORD_MAX_FRAMES: u32 = 60 * recorder::FPS;

/// What the callbacks of the frame captures share with the app.
#[derive(Default)]
pub(super) struct RecState {
    recorder: Option<Recorder>,
    error: Option<String>,
    /// the captures that have arrived
    arrived: u32,
}

/// The state of a `--screenshot` or `--record` run: where the picture or clip goes, and whether it was
/// asked for and saved.
pub(super) struct ScreenshotRun {
    /// where the picture (or, with the extension `.webm`, the clip) goes (none: a normal run)
    pub(super) path: Option<PathBuf>,
    pub(super) requested: bool,
    pub(super) done: Arc<AtomicBool>,
    /// recording: what the captures share, and how many were asked for
    rec: Arc<Mutex<RecState>>,
    asked: u32,
}

impl ScreenshotRun {
    pub(super) fn new(path: Option<PathBuf>) -> Self {
        Self { path, requested: false, done: Arc::new(AtomicBool::new(false)), rec: Arc::default(), asked: 0 }
    }

    pub(super) fn recording(&self) -> bool {
        self.path.as_deref().is_some_and(recorder::is_clip)
    }
}

/// A recording that cannot go on ends the run with exit code 1.
fn failed(message: &str) -> ! {
    eprintln!("record: {message}");
    std::process::exit(1);
}

impl ShaleApp {
    /// One frame of a `--record` run: capture it while the script runs, and when it has ended and the
    /// last captures have arrived, finish the clip and close the window.
    pub(super) fn record_tick(&mut self, ui: &Ui, path: PathBuf) {
        {
            let mut s = self.shot.rec.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(e) = s.error.take() {
                failed(&e);
            }
            if self.shot.requested && s.arrived == self.shot.asked && !self.shot.done.load(std::sync::atomic::Ordering::SeqCst) {
                match s.recorder.take().map(Recorder::finish) {
                    Some(Ok(frames)) => eprintln!("record: {} frames ({:.1} s) to {}", frames, f64::from(frames) / f64::from(recorder::FPS), path.display()),
                    Some(Err(e)) => failed(&e),
                    None => failed("no frame was captured"),
                }
                self.shot.done.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let ended = self.script.steps.is_empty() && self.frames >= self.script.last_frame.max(30) + RECORD_TAIL;
        if ended {
            self.shot.requested = true;
        }
        if self.shot.requested {
            return;
        }
        if self.shot.asked >= RECORD_MAX_FRAMES {
            failed(&format!("the script did not end within {} frames", RECORD_MAX_FRAMES));
        }
        if self.frames < RECORD_FROM {
            return;
        }
        self.shot.asked += 1;
        let shared = self.shot.rec.clone();
        ui.ctx().request_screenshot(move |img| {
            let [w, h] = img.size;
            let mut bytes = Vec::with_capacity(w * h * 4);
            for p in &img.pixels {
                bytes.extend_from_slice(&p.to_srgba_unmultiplied());
            }
            let mut s = shared.lock().unwrap_or_else(|e| e.into_inner());
            s.arrived += 1;
            if s.error.is_some() {
                return;
            }
            let pushed = match s.recorder.as_mut() {
                Some(r) => r.push([w, h], &bytes),
                None => Recorder::start(recorder::FFMPEG, &path, [w, h]).and_then(|mut r| {
                    let pushed = r.push([w, h], &bytes);
                    s.recorder = Some(r);
                    pushed
                }),
            };
            if let Err(e) = pushed {
                s.error = Some(e);
            }
        });
    }

    /// The picture or clip of this run is complete when the window may close.
    pub(super) fn send_close_when_done(&self, ui: &Ui) {
        if self.shot.done.load(std::sync::atomic::Ordering::SeqCst) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
