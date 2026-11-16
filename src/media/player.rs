//! The one player state of the app (MEDIA-007, MEDIA-008): which file plays, where it is, the
//! volume. Every player widget in the reading view, the editor and the preview draws from it and
//! sends its clicks to it, so the state survives scrolling and a switch between the views, and
//! only one file plays at a time. The handle is cheap to clone; all work that can be slow (opening
//! the file, decoding the cover, opening the sound card) happens on threads.

use super::cover::Look;
use super::engine::Shared;
use super::output::Output;
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime};

/// How often the window repaints while something plays (the progress bar).
const REPAINT: Duration = Duration::from_millis(100);
/// How many files' covers are kept.
const LOOKS_KEPT: usize = 48;

/// What the output thread tells the window.
enum Status {
    Opening,
    Ready(Arc<Shared>),
    NoOutput,
    Failed(String),
}

/// The file that is loaded in the output (playing or paused).
struct Playback {
    path: PathBuf,
    /// the note whose player started it: closing that note's tab stops the sound
    owner: PathBuf,
    status: Arc<Mutex<Status>>,
    stop: Arc<AtomicBool>,
    /// what the person wants: play or pause (applied when the output is ready)
    want_play: bool,
    /// a jump asked for before the output was ready
    start_at: Option<f64>,
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

enum LookSlot {
    Loading(Arc<Mutex<Option<Look>>>),
    Ready { duration: Option<f64>, error: Option<String>, cover: Option<egui::TextureHandle> },
}

struct State {
    looks: HashMap<(PathBuf, Option<SystemTime>), LookSlot>,
    current: Option<Playback>,
    volume: f32,
    muted: bool,
    /// the volume the person changed in a player, for the app to save
    volume_changed: bool,
    opacity: f32,
    /// tests: what opens the output instead of the sound card
    #[cfg(test)]
    open_output: Option<fn() -> Result<Output, String>>,
}

/// A handle to the app's one player state.
#[derive(Clone)]
pub struct MediaPlayer(Arc<Mutex<State>>);

impl Default for MediaPlayer {
    fn default() -> Self {
        MediaPlayer(Arc::new(Mutex::new(State {
            looks: HashMap::new(),
            current: None,
            volume: 0.8,
            muted: false,
            volume_changed: false,
            opacity: 0.6,
            #[cfg(test)]
            open_output: None,
        })))
    }
}

/// What a player widget draws.
pub struct View {
    pub file_name: String,
    pub cover: Option<egui::TextureHandle>,
    pub duration: Option<f64>,
    pub position: f64,
    pub playing: bool,
    /// the file or the sound output is being opened
    pub busy: bool,
    /// why the file does not play (damaged, no sound output), for the player to say
    pub message: Option<String>,
    /// a file that cannot play at all: the button is dimmed
    pub unplayable: bool,
    pub volume: f32,
    pub muted: bool,
    pub opacity: f32,
}

fn key(abs: &Path) -> (PathBuf, Option<SystemTime>) {
    (abs.to_path_buf(), std::fs::metadata(abs).and_then(|m| m.modified()).ok())
}

impl MediaPlayer {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The settings (MEDIA-004, MEDIA-006), from the global config each frame.
    pub fn configure(&self, volume: f32, opacity: f32) {
        let mut s = self.lock();
        if !s.volume_changed {
            s.volume = volume.clamp(0.0, 1.0);
        }
        s.opacity = opacity.clamp(0.0, 1.0);
        s.apply_state();
    }

    /// The volume a player's slider set since the last call, to be saved in the config.
    pub fn take_volume_change(&self) -> Option<f32> {
        let mut s = self.lock();
        std::mem::take(&mut s.volume_changed).then_some(s.volume)
    }

    /// Stop whatever plays (the vault was closed, MEDIA-007).
    pub fn stop(&self) {
        self.lock().current = None;
    }

    /// The tab of this note was closed: stop its sound (MEDIA-007).
    pub fn stop_for_note(&self, note: &Path) {
        let mut s = self.lock();
        if s.current.as_ref().is_some_and(|c| c.owner == note) {
            s.current = None;
        }
    }

    /// Tests: the file that is loaded in the output, if any.
    #[cfg(test)]
    pub fn current(&self) -> Option<PathBuf> {
        self.lock().current.as_ref().map(|c| c.path.clone())
    }

    pub fn set_volume(&self, v: f32) {
        let mut s = self.lock();
        s.volume = v.clamp(0.0, 1.0);
        s.muted = false;
        s.volume_changed = true;
        s.apply_state();
    }

    pub fn set_muted(&self, m: bool) {
        let mut s = self.lock();
        s.muted = m;
        s.apply_state();
    }

    /// Play or pause `abs`; another file that plays is stopped first (MEDIA-007).
    pub fn toggle(&self, ctx: &egui::Context, abs: &Path, owner: &Path) {
        let mut s = self.lock();
        let play = !s.current.as_ref().is_some_and(|c| c.path == abs && c.want_play);
        s.begin(ctx, abs, owner).want_play = play;
        s.apply_state();
    }

    /// Jump to `secs` in `abs`: the file is loaded (paused) if it was not the current one.
    pub fn seek_to(&self, ctx: &egui::Context, abs: &Path, owner: &Path, secs: f64) {
        let mut s = self.lock();
        let p = s.begin(ctx, abs, owner);
        let status = p.status.clone();
        let status = status.lock().unwrap_or_else(|e| e.into_inner());
        match &*status {
            Status::Ready(shared) => shared.seek(secs),
            _ => p.start_at = Some(secs),
        }
    }

    /// What to draw for `abs` now, and housekeeping: looks at the file in the background, starts
    /// the first player of a new note when asked to, asks for a repaint while something moves.
    pub fn view(&self, ctx: &egui::Context, abs: &Path) -> View {
        let mut s = self.lock();
        let slot_key = key(abs);
        if !s.looks.contains_key(&slot_key) {
            if s.looks.len() >= LOOKS_KEPT {
                s.looks.clear();
            }
            let cell = Arc::new(Mutex::new(None));
            let (path, out, ctx2) = (abs.to_path_buf(), cell.clone(), ctx.clone());
            std::thread::spawn(move || {
                let look = super::cover::look(&path);
                *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(look);
                ctx2.request_repaint();
            });
            s.looks.insert(slot_key.clone(), LookSlot::Loading(cell));
        }
        if let Some(LookSlot::Loading(cell)) = s.looks.get(&slot_key) {
            let done = cell.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(look) = done {
                let cover = look.cover.map(|img| ctx.load_texture(abs.to_string_lossy(), img, egui::TextureOptions::LINEAR));
                s.looks.insert(slot_key.clone(), LookSlot::Ready { duration: look.duration, error: look.error, cover });
            }
        }
        let (duration, look_error, cover, loading) = match s.looks.get(&slot_key) {
            Some(LookSlot::Ready { duration, error, cover }) => (*duration, error.clone(), cover.clone(), false),
            _ => (None, None, None, true),
        };

        let mut view = View {
            file_name: abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            cover,
            duration,
            position: 0.0,
            playing: false,
            busy: loading,
            message: look_error.clone(),
            unplayable: look_error.is_some(),
            volume: s.volume,
            muted: s.muted,
            opacity: s.opacity,
        };
        if let Some(c) = s.current.as_mut().filter(|c| c.path == abs) {
            view.playing = c.want_play;
            match &*c.status.lock().unwrap_or_else(|e| e.into_inner()) {
                Status::Opening => view.busy = true,
                Status::Ready(shared) => {
                    view.position = shared.position_secs();
                    if shared.is_finished() {
                        // played to the end: back to the start, paused
                        c.want_play = false;
                        shared.set_playing(false);
                        shared.seek(0.0);
                        view.playing = false;
                        view.position = 0.0;
                    }
                }
                Status::NoOutput => {
                    view.message = Some("No sound output".into());
                    view.playing = false;
                }
                Status::Failed(e) => {
                    view.message = Some(e.clone());
                    view.unplayable = true;
                    view.playing = false;
                }
            }
            if let Some(at) = c.start_at {
                view.position = at;
            }
            if view.playing || view.busy {
                ctx.request_repaint_after(if view.busy { Duration::from_millis(40) } else { REPAINT });
            }
        } else if loading {
            ctx.request_repaint_after(Duration::from_millis(40));
        }
        s.apply_state();
        view
    }

    /// Tests: play into a thread that only keeps time, never into a sound card.
    #[cfg(test)]
    pub fn use_silent_output(&self) {
        self.lock().open_output = Some(|| Ok(Output::silent()));
    }

    /// Tests: a machine without a sound card.
    #[cfg(test)]
    pub fn use_no_output(&self) {
        self.lock().open_output = Some(|| Err("no sound output".into()));
    }

    /// Tests: how far the current file is, in seconds.
    #[cfg(test)]
    pub fn position(&self) -> Option<f64> {
        let s = self.lock();
        let c = s.current.as_ref()?;
        let status = c.status.lock().unwrap_or_else(|e| e.into_inner());
        match &*status {
            Status::Ready(shared) => Some(shared.position_secs()),
            _ => None,
        }
    }
}

impl State {
    fn gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }

    /// Hand the volume, the play flag and a waiting seek to the output once it is ready.
    fn apply_state(&mut self) {
        let gain = self.gain();
        let Some(c) = self.current.as_mut() else { return };
        if let Status::Ready(shared) = &*c.status.lock().unwrap_or_else(|e| e.into_inner()) {
            shared.set_volume(gain);
            if let Some(at) = c.start_at.take() {
                shared.seek(at);
            }
            if shared.is_playing() != c.want_play {
                shared.set_playing(c.want_play);
            }
        }
    }

    /// Make `abs` the current file (stopping the one that was); returns it.
    fn begin(&mut self, ctx: &egui::Context, abs: &Path, owner: &Path) -> &mut Playback {
        if self.current.as_ref().is_none_or(|c| c.path != abs) {
            let status = Arc::new(Mutex::new(Status::Opening));
            let stop = Arc::new(AtomicBool::new(false));
            // the old one is dropped first: only one file plays at a time
            self.current = None;
            let (path, st, stp, ctx2) = (abs.to_path_buf(), status.clone(), stop.clone(), ctx.clone());
            let open: fn() -> Result<Output, String> = super::output::open;
            #[cfg(test)]
            let open = self.open_output.unwrap_or(open);
            std::thread::spawn(move || {
                run_output(&path, &st, &stp, open);
                ctx2.request_repaint();
            });
            self.current = Some(Playback { path: abs.to_path_buf(), owner: owner.to_path_buf(), status, stop, want_play: false, start_at: None });
        }
        self.current.as_mut().expect("just set")
    }
}

/// The output thread: opens the sound card, reports, decodes into it until stopped.
fn run_output(path: &Path, status: &Mutex<Status>, stop: &AtomicBool, open: fn() -> Result<Output, String>) {
    let set = |s: Status| *status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    let output = match open() {
        Ok(o) => o,
        Err(_) => return set(Status::NoOutput),
    };
    set(Status::Ready(output.shared.clone()));
    if let Err(e) = super::decode::run(path, &output.shared, stop) {
        set(Status::Failed(e));
    }
}

/// `73.2` → `1:13`, `3725` → `1:02:05`.
pub fn clock(secs: f64) -> String {
    let t = secs.max(0.0) as u64;
    if t >= 3600 {
        format!("{}:{:02}:{:02}", t / 3600, t / 60 % 60, t % 60)
    } else {
        format!("{}:{:02}", t / 60, t % 60)
    }
}

/// The opacity of the round button over the artwork (MEDIA-004): the setting, fully opaque on hover.
pub fn button_alpha(opacity: f32, hovered: bool) -> u8 {
    if hovered {
        255
    } else {
        (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: MEDIA-005
    #[test]
    fn the_clock_shows_minutes_and_hours() {
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(73.9), "1:13");
        assert_eq!(clock(3725.0), "1:02:05");
        assert_eq!(clock(-4.0), "0:00");
    }

    // spec: MEDIA-004
    #[test]
    fn the_button_is_as_opaque_as_the_setting_and_fully_on_hover() {
        assert_eq!(button_alpha(0.6, false), 153);
        assert_eq!(button_alpha(0.0, false), 0);
        assert_eq!(button_alpha(1.0, false), 255);
        assert_eq!(button_alpha(0.6, true), 255);
        assert_eq!(button_alpha(7.0, false), 255, "a setting out of range is brought into it");
    }

    // spec: MEDIA-006
    #[test]
    fn the_volume_setting_is_kept_until_a_player_changes_it_and_the_app_takes_the_change() {
        let m = MediaPlayer::default();
        m.configure(0.3, 0.6);
        assert_eq!(m.take_volume_change(), None);
        m.set_volume(0.9);
        assert_eq!(m.take_volume_change(), Some(0.9));
        assert_eq!(m.take_volume_change(), None, "taken once");
        m.configure(0.5, 0.6);
        assert_eq!(m.lock().volume, 0.5, "the config is the truth again");
        m.configure(7.0, 2.0);
        let s = m.lock();
        assert_eq!((s.volume, s.opacity), (1.0, 1.0), "out of range values are brought into range");
    }
}
