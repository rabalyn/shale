//! The rescan of the vault folder (DATA-002) on a worker thread (NET-013): the walk, the stats and
//! the reads of changed files happen there, the result is applied on the UI thread.

use super::{DrawingDoc, ShaleApp, View};
use crate::jobs;
use crate::vault::{rescan_interval, Scan, ScanInput, Stamp, RESCAN_EVERY};
use eframe::egui;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Size and modification time of a drawing file (what `DrawingDoc::seen` holds).
type Meta = Option<(Option<std::time::SystemTime>, u64)>;

/// An open drawing without unsaved edits, as the scan has to know it.
pub(super) struct DrawingProbe {
    rel: PathBuf,
    seen: Meta,
    base: Stamp,
    failed: bool,
}

/// What the scan found of an open drawing whose file changed.
struct DrawingRead {
    probe: DrawingProbe,
    meta: Meta,
    text: Result<String, String>,
}

/// What one scan brings back: the vault walk and the open drawings that changed on disk.
pub(super) struct Outcome {
    scan: Scan,
    drawings: Vec<DrawingRead>,
}

/// How a scan is made; a test replaces it with a slow or gated one.
type Runner = Arc<dyn Fn(ScanInput, Vec<DrawingProbe>) -> Outcome + Send + Sync>;

/// The scan that is running: for the vault with this `uid`.
struct Flight {
    uid: u64,
    rx: Receiver<Result<Outcome, String>>,
}

/// When the vault is scanned next, and the scan that is running.
pub struct RescanState {
    /// When the last scan started
    last: Instant,
    /// How long to wait after `last` for the next one (NET-013)
    interval: Duration,
    flight: Option<Flight>,
    runner: Runner,
}

impl RescanState {
    pub fn new() -> Self {
        Self { last: Instant::now(), interval: RESCAN_EVERY, flight: None, runner: Arc::new(scan) }
    }

    /// The vault changed (VAULT-037): the scan of the old one is dropped (its result is never
    /// applied) and the timer starts again. The way a scan is made stays.
    pub(super) fn reset(&mut self) {
        self.last = Instant::now();
        self.interval = RESCAN_EVERY;
        self.flight = None;
    }

    /// A scan is running.
    #[cfg(test)]
    pub(super) fn in_flight(&self) -> bool {
        self.flight.is_some()
    }

    /// Make the next frame start a scan.
    #[cfg(test)]
    pub(super) fn due_now(&mut self) {
        self.last = Instant::now() - self.interval - Duration::from_millis(1);
    }

    /// Make the next scan wait for its time again.
    #[cfg(test)]
    pub(super) fn postpone(&mut self) {
        self.last = Instant::now();
    }

    #[cfg(test)]
    pub(super) fn with_runner(&mut self, runner: impl Fn(ScanInput, Vec<DrawingProbe>) -> Outcome + Send + Sync + 'static) {
        self.runner = Arc::new(runner);
    }
}

/// The scan itself: the file system work of a rescan, nothing of the app's state.
pub(super) fn scan(input: ScanInput, probes: Vec<DrawingProbe>) -> Outcome {
    let root = input.root().to_path_buf();
    let scan = input.run();
    let drawings = probes
        .into_iter()
        .filter_map(|probe| {
            let abs = root.join(&probe.rel);
            let meta = std::fs::metadata(&abs).ok().map(|m| (m.modified().ok(), m.len()));
            if meta == probe.seen && !probe.failed {
                return None;
            }
            let text = crate::fsutil::read_to_string(&abs).map_err(|e| e.to_string());
            Some(DrawingRead { probe, meta, text })
        })
        .collect();
    Outcome { scan, drawings }
}

impl ShaleApp {
    /// Each frame: take in the result of a running scan, and start the next one when its time has
    /// come. Only one runs at a time, and the wait grows with the time a scan took (NET-013).
    pub(super) fn rescan_tick(&mut self, ctx: &egui::Context) {
        if let Some(f) = &self.rescan.flight {
            match f.rx.try_recv() {
                Ok(result) => {
                    let uid = f.uid;
                    self.rescan.flight = None;
                    self.scan_arrived(uid, result);
                }
                Err(TryRecvError::Empty) if self.vault.as_ref().is_some_and(|v| v.uid == f.uid) => return,
                // the vault was closed or another one opened: its result is of no use
                Err(_) => self.rescan.flight = None,
            }
        }
        if self.rescan.flight.is_some() || self.rescan.last.elapsed() <= self.rescan.interval {
            return;
        }
        let Some(v) = self.vault.as_ref() else { return };
        let (uid, input, probes) = (v.uid, v.scan_input(), self.drawing_probes());
        let runner = self.rescan.runner.clone();
        let ctx = ctx.clone();
        self.rescan.last = Instant::now();
        self.rescan.flight = Some(Flight { uid, rx: jobs::spawn(move || runner(input, probes), move || ctx.request_repaint()) });
    }

    fn scan_arrived(&mut self, uid: u64, result: Result<Outcome, String>) {
        match result {
            Ok(outcome) => {
                self.rescan.interval = rescan_interval(outcome.scan.took);
                if self.vault.as_ref().is_some_and(|v| v.uid == uid) {
                    self.apply_outcome(outcome);
                }
            }
            Err(e) => {
                self.rescan.interval = Duration::from_secs(60);
                self.toast(format!("Could not look for changes in the vault folder: {e}"));
            }
        }
    }

    /// Scan the vault folder here and now, and apply the result. For the places that have just
    /// changed files themselves; the frame uses [`ShaleApp::rescan_tick`].
    pub(super) fn rescan(&mut self) {
        let Some(v) = self.vault.as_ref() else { return };
        let outcome = scan(v.scan_input(), self.drawing_probes());
        self.apply_outcome(outcome);
    }

    fn drawing_probes(&self) -> Vec<DrawingProbe> {
        let clean = self.drawings.iter().filter(|(_, d)| !d.dirty);
        clean.map(|(rel, d)| DrawingProbe { rel: rel.clone(), seen: d.seen, base: d.base, failed: d.error.is_some() }).collect()
    }

    /// Pick up changes made by other programs: notes and drawings without unsaved edits are
    /// reloaded (DATA-002); one with unsaved edits keeps them and becomes a conflict copy when
    /// it is saved (DATA-003). Everything is compared with the state of now: a result of a scan
    /// that started before a save or before the vault changed does not overwrite either.
    fn apply_outcome(&mut self, outcome: Outcome) {
        let Some(v) = self.vault.as_mut() else { return };
        let Some(changed) = v.apply_scan(outcome.scan) else { return };
        for rel in changed {
            if let Some(doc) = self.notes.get_mut(&rel) {
                if !doc.dirty {
                    if let Some(n) = v.notes.get(&rel) {
                        doc.reload(n.text.clone(), n.unreadable.clone());
                    }
                }
            }
        }
        for read in outcome.drawings {
            let Some(d) = self.drawings.get_mut(&read.probe.rel) else { continue };
            // saved, reloaded or edited since the scan began: the buffer is newer than what was read
            if d.dirty || d.base != read.probe.base || d.seen != read.probe.seen {
                continue;
            }
            d.seen = read.meta;
            if let Ok(text) = read.text {
                if Stamp::of(&text) != d.base || d.error.is_some() {
                    let fresh = DrawingDoc::new(read.probe.rel.clone(), &text);
                    self.scenes.put(&read.probe.rel, fresh.scene.clone());
                    *d = fresh;
                }
            }
        }
        // tabs pointing at files that vanished
        let files: HashSet<PathBuf> = v.files.iter().map(|f| f.rel.clone()).collect();
        for t in self.all_tabs_mut() {
            if let Some(p) = t.view.path() {
                if !files.contains(p) {
                    t.view = View::Empty;
                }
            }
        }
    }
}
