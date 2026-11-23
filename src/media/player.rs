//! The one player state of the app (MEDIA-007, MEDIA-008): which file plays, where it is, the
//! volume. Every player widget in the reading view, the editor and the preview draws from it and
//! sends its clicks to it, so the state survives scrolling and a switch between the views, and
//! only one file plays at a time. The handle is cheap to clone; all work that can be slow (opening
//! the file, decoding the cover, opening the sound card) happens on threads.

use super::download::{Downloads, Notice};
use super::engine::Shared;
use super::ffmpeg::{self, Info, Tools};
use super::look::{Look, Looks};
use super::output::Output;
use super::video::{self, FrameSlot, Looked};
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

/// What a video says when the machine has no sound output (MEDIA-013).
const NO_SOUND_NOTE: &str = "No sound output: playing without sound";

/// What the output thread tells the window.
enum Status {
    Opening,
    /// the output is open; the flag says that it has no sound card behind it (a video only, MEDIA-013)
    Ready(Arc<Shared>, bool),
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
    /// a jump asked for as a share of the bar before the length was known (MEDIA-005, #258)
    start_share: Option<f64>,
    /// a video: the newest picture from ffmpeg, and the texture it is shown in
    frames: Arc<FrameSlot>,
    frame_tex: Option<egui::TextureHandle>,
    /// plays again from the start when it ends (a post's GIF, MEDIA-028)
    looping: bool,
    /// plays without sound whatever the volume says, until the person unmutes (a post's GIF)
    silent: bool,
}

/// How a file outside the vault starts when the person opened it (MEDIA-028).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    /// a video: with the sound, once
    Video,
    /// a GIF (Mastodon's "gifv"): without sound, again and again
    Gif,
}

impl Drop for Playback {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

enum LookSlot {
    Loading(Arc<Mutex<Option<Looked>>>),
    Ready { duration: Option<f64>, error: Option<String>, cover: Option<egui::TextureHandle>, info: Option<Info>, external: bool },
}

/// The look at a file, and the version of the file (its modification time) it was made of.
struct LookAt {
    mtime: Option<SystemTime>,
    slot: LookSlot,
}

struct State {
    looks: HashMap<PathBuf, LookAt>,
    current: Option<Playback>,
    /// files that are still LFS pointers, and their downloads (SYNC-020)
    downloads: Downloads,
    volume: f32,
    muted: bool,
    /// the volume the person changed in a player, for the app to save
    volume_changed: bool,
    opacity: f32,
    /// where ffmpeg is (MEDIA-009); tests put their own
    locate: fn() -> Option<Tools>,
    /// videos the person asked to open in the system's player, for the app to open (MEDIA-010)
    open_requests: Vec<PathBuf>,
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
            downloads: Downloads::default(),
            volume: 0.8,
            muted: false,
            volume_changed: false,
            opacity: 0.6,
            locate: ffmpeg::locate,
            open_requests: Vec::new(),
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
    /// a video and no ffmpeg (MEDIA-010): a click opens the system's player
    pub external: bool,
    pub volume: f32,
    pub muted: bool,
    pub opacity: f32,
    /// the picture size of a video, once ffprobe has looked (the "100%" size, MEDIA-012)
    pub video_size: Option<(u32, u32)>,
    /// the file is a Git LFS pointer: not downloaded, or downloading (SYNC-020)
    pub download: Option<Notice>,
    /// a click on the bar that waits for the length, as a share of the bar (MEDIA-005)
    pub waiting_share: Option<f64>,
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

    /// The opacity of the round buttons (MEDIA-004), which the buttons over pictures share (MEDIA-012).
    pub fn opacity(&self) -> f32 {
        self.lock().opacity
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

    /// The videos that were asked to open in the system's player since the last call (MEDIA-010).
    pub fn take_open_requests(&self) -> Vec<PathBuf> {
        std::mem::take(&mut self.lock().open_requests)
    }

    /// Where ffmpeg is on this machine, if it is (MEDIA-010: the settings say so).
    pub fn ffmpeg(&self) -> Option<PathBuf> {
        (self.lock().locate)().map(|t| t.ffmpeg)
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
        s.unsilence();
        s.volume_changed = true;
        s.apply_state();
    }

    pub fn set_muted(&self, m: bool) {
        let mut s = self.lock();
        s.muted = m;
        if !m {
            s.unsilence();
        }
        s.apply_state();
    }

    /// Start `abs`, a file outside the vault that the person opened (a video of a post in the
    /// feed viewer, MEDIA-028): it plays at once, once or, for a GIF, muted and looped. `owner`
    /// stands in for a note (`stop_for_note` stops it). Nothing happens when it is the current
    /// file already, so a pause or the end of the clip stays.
    pub fn play_outside(&self, ctx: &egui::Context, abs: &Path, owner: &Path, how: Start) {
        let mut s = self.lock();
        if s.current.as_ref().is_some_and(|c| c.path == abs) {
            return;
        }
        let p = s.begin(ctx, abs, owner);
        p.want_play = true;
        p.looping = how == Start::Gif;
        p.silent = how == Start::Gif;
        s.apply_state();
    }

    /// Play or pause `abs`; another file that plays is stopped first (MEDIA-007).
    pub fn toggle(&self, ctx: &egui::Context, abs: &Path, owner: &Path) {
        let mut s = self.lock();
        let Look::Known(facts) = Looks::of(ctx).facts(ctx, abs) else {
            // the file is still being looked at (the card says so): the click comes too early
            return;
        };
        let pointer = facts.and_then(|f| f.pointer);
        if s.downloads.notice(abs, pointer).is_some() {
            // not downloaded: fetch first, the sound starts when it is here (SYNC-020)
            s.downloads.request(abs, Some(owner), pointer);
            ctx.request_repaint();
            return;
        }
        if s.is_external(abs) {
            // no ffmpeg: the system's player gets the file (MEDIA-010)
            s.open_requests.push(abs.to_path_buf());
            return;
        }
        let play = !s.current.as_ref().is_some_and(|c| c.path == abs && c.want_play);
        s.begin(ctx, abs, owner).want_play = play;
        s.apply_state();
    }

    /// Jump to `secs` in `abs`: the file is loaded (paused) if it was not the current one.
    pub fn seek_to(&self, ctx: &egui::Context, abs: &Path, owner: &Path, secs: f64) {
        let mut s = self.lock();
        let p = s.begin(ctx, abs, owner);
        p.start_share = None;
        let status = p.status.clone();
        let status = status.lock().unwrap_or_else(|e| e.into_inner());
        match &*status {
            Status::Ready(shared, _) => shared.seek(secs),
            _ => p.start_at = Some(secs),
        }
    }

    /// Jump to `share` (0…1) of the length of `abs`, which is not known yet: the jump waits for
    /// it (MEDIA-005, #258).
    pub fn seek_to_share(&self, ctx: &egui::Context, abs: &Path, owner: &Path, share: f64) {
        let mut s = self.lock();
        let p = s.begin(ctx, abs, owner);
        p.start_at = None;
        p.start_share = Some(share.clamp(0.0, 1.0));
        ctx.request_repaint();
    }

    /// What to draw for `abs` now, and housekeeping: looks at the file in the background, starts
    /// the first player of a new note when asked to, asks for a repaint while something moves.
    pub fn view(&self, ctx: &egui::Context, abs: &Path) -> View {
        let mut s = self.lock();
        // what the disk says about the file is looked up on a worker; until it is there the card is
        // a quiet "busy" one of its usual size (MEDIA-024)
        let Look::Known(facts) = Looks::of(ctx).facts(ctx, abs) else {
            return s.bare_view(abs, true, None, None);
        };
        if let Some(n) = s.downloads.notice(abs, facts.as_ref().and_then(|f| f.pointer)) {
            // an LFS pointer has nothing to decode: say so, with the Download link (SYNC-020)
            if n.running {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            return s.bare_view(abs, n.running, Some(n.line()), Some(n));
        }
        let mtime = facts.and_then(|f| f.mtime);
        if s.looks.get(abs).is_none_or(|l| l.mtime != mtime) {
            if s.looks.len() >= LOOKS_KEPT {
                s.looks.clear();
            }
            let cell = Arc::new(Mutex::new(None));
            let (path, out, ctx2) = (abs.to_path_buf(), cell.clone(), ctx.clone());
            let locate = s.locate;
            std::thread::spawn(move || {
                let look = if super::is_video(&path) { video::look(&path, locate) } else { Looked { look: super::cover::look(&path), info: None, missing: false } };
                *out.lock().unwrap_or_else(|e| e.into_inner()) = Some(look);
                ctx2.request_repaint();
            });
            s.looks.insert(abs.to_path_buf(), LookAt { mtime, slot: LookSlot::Loading(cell) });
        }
        if let Some(LookSlot::Loading(cell)) = s.looks.get(abs).map(|l| &l.slot) {
            let done = cell.lock().unwrap_or_else(|e| e.into_inner()).take();
            if let Some(look) = done {
                let (external, info) = (look.missing, look.info);
                let look = look.look;
                let cover = look.cover.map(|img| ctx.load_texture(abs.to_string_lossy(), img, egui::TextureOptions::LINEAR));
                s.looks.insert(abs.to_path_buf(), LookAt { mtime, slot: LookSlot::Ready { duration: look.duration, error: look.error, cover, info, external } });
            }
        }
        let (duration, look_error, cover, loading, external) = match s.looks.get(abs).map(|l| &l.slot) {
            Some(LookSlot::Ready { duration, error, cover, external, .. }) => (*duration, error.clone(), cover.clone(), false, *external),
            _ => (None, None, None, true, false),
        };

        let mut view = View {
            file_name: abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            cover,
            duration,
            position: 0.0,
            playing: false,
            busy: loading,
            message: look_error.clone().or_else(|| external.then(|| "ffmpeg is not installed: a click opens the video in your system's player".to_string())),
            unplayable: look_error.is_some(),
            external,
            volume: s.volume,
            muted: s.muted,
            opacity: s.opacity,
            video_size: match s.looks.get(abs).map(|l| &l.slot) {
                Some(LookSlot::Ready { info: Some(i), .. }) => Some((i.width, i.height)),
                _ => None,
            },
            download: None,
            waiting_share: None,
        };
        if let Some(c) = s.current.as_mut().filter(|c| c.path == abs) {
            view.playing = c.want_play;
            view.muted |= c.silent;
            if !loading {
                // the length is known now: a waiting click becomes a jump (or is dropped when
                // the file has no length)
                if let Some(share) = c.start_share.take() {
                    c.start_at = duration.filter(|_| look_error.is_none()).map(|d| share * d);
                }
            }
            view.waiting_share = c.start_share;
            match &*c.status.lock().unwrap_or_else(|e| e.into_inner()) {
                Status::Opening => view.busy = true,
                Status::Ready(shared, no_sound) => {
                    if *no_sound {
                        view.message = Some(NO_SOUND_NOTE.into());
                    }
                    view.position = shared.position_secs();
                    if shared.is_finished() && c.looping {
                        // a GIF: again from the start, still playing
                        shared.seek(0.0);
                        view.position = 0.0;
                    } else if shared.is_finished() {
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
            // a video shows its newest picture instead of the first frame
            if let Some(img) = c.frames.lock().unwrap_or_else(|e| e.into_inner()).take() {
                match &mut c.frame_tex {
                    Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                    None => c.frame_tex = Some(ctx.load_texture(format!("{}-frames", abs.to_string_lossy()), img, egui::TextureOptions::LINEAR)),
                }
            }
            if c.frame_tex.is_some() {
                view.cover = c.frame_tex.clone();
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

    /// What to say about a file that may be an LFS pointer; None for a real file (SYNC-020).
    /// What the look-up of the file says (MEDIA-024): None for a real file and while the first look is on its way.
    pub fn notice(&self, ctx: &egui::Context, abs: &Path) -> Option<Notice> {
        let Look::Known(facts) = Looks::of(ctx).facts(ctx, abs) else { return None };
        self.lock().downloads.notice(abs, facts.and_then(|f| f.pointer))
    }

    /// Ask for the real file of an LFS pointer (the Download link, SYNC-020).
    pub fn download(&self, ctx: &egui::Context, abs: &Path) {
        let Look::Known(facts) = Looks::of(ctx).facts(ctx, abs) else { return };
        self.lock().downloads.request(abs, None, facts.and_then(|f| f.pointer));
    }

    /// The files asked for since the last call; the app downloads them.
    pub fn take_download_requests(&self) -> Vec<PathBuf> {
        self.lock().downloads.take_requests()
    }

    /// A download ended; when a play button asked for it, the sound starts now.
    pub fn download_done(&self, ctx: &egui::Context, abs: &Path, result: Result<(), String>) {
        let mut s = self.lock();
        // the file is not what it was: the next frame looks at it again
        Looks::of(ctx).forget(abs);
        if let Some(owner) = s.downloads.finish(abs, result) {
            s.begin(ctx, abs, &owner).want_play = true;
            s.apply_state();
        }
        ctx.request_repaint();
    }

    /// Tests: play into a thread that only keeps time, never into a sound card.
    #[cfg(test)]
    pub fn use_silent_output(&self) {
        self.lock().open_output = Some(|| Ok(Output::without_device()));
    }

    /// Tests: the size of the first-frame (or cover) picture that was read for `abs`.
    #[cfg(test)]
    pub fn thumbnail_size(&self, abs: &Path) -> Option<[usize; 2]> {
        match self.lock().looks.get(abs).map(|l| &l.slot) {
            Some(LookSlot::Ready { cover: Some(t), .. }) => Some(t.size()),
            _ => None,
        }
    }

    /// Tests: the size of the newest picture of the playing video, once one was shown.
    #[cfg(test)]
    pub fn picture_size(&self) -> Option<[usize; 2]> {
        self.lock().current.as_ref()?.frame_tex.as_ref().map(|t| t.size())
    }

    /// Tests: ffmpeg is where `locate` says (nowhere: it returns None).
    #[cfg(test)]
    pub fn use_ffmpeg(&self, locate: fn() -> Option<Tools>) {
        self.lock().locate = locate;
    }

    /// Tests: a machine without a sound card.
    #[cfg(test)]
    pub fn use_no_output(&self) {
        self.lock().open_output = Some(|| Err("no sound output".into()));
    }

    /// Tests: the share of the bar a click is waiting at until the length is known.
    #[cfg(test)]
    pub fn waiting_share(&self) -> Option<f64> {
        self.lock().current.as_ref()?.start_share
    }

    /// Tests: a seek of the current file is still waiting for the mixer.
    #[cfg(test)]
    pub fn seek_pending(&self) -> bool {
        let s = self.lock();
        let Some(c) = s.current.as_ref() else { return false };
        let status = c.status.lock().unwrap_or_else(|e| e.into_inner());
        matches!(&*status, Status::Ready(shared, _) if shared.seek_pending())
    }

    /// Tests: how far the current file is, in seconds.
    #[cfg(test)]
    pub fn position(&self) -> Option<f64> {
        let s = self.lock();
        let c = s.current.as_ref()?;
        let status = c.status.lock().unwrap_or_else(|e| e.into_inner());
        match &*status {
            Status::Ready(shared, _) => Some(shared.position_secs()),
            _ => None,
        }
    }
}

impl State {
    /// The card of a file that has nothing to decode (yet): its name, perhaps a line of words and
    /// the download state of a pointer (SYNC-020), and a busy mark while its look-up is on its way (MEDIA-024).
    fn bare_view(&self, abs: &Path, busy: bool, message: Option<String>, download: Option<Notice>) -> View {
        View {
            file_name: abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            cover: None,
            duration: None,
            position: 0.0,
            playing: false,
            busy,
            message,
            unplayable: false,
            external: false,
            volume: self.volume,
            muted: self.muted,
            opacity: self.opacity,
            video_size: None,
            download,
            waiting_share: None,
        }
    }

    /// A video that cannot be played here because ffmpeg is missing (MEDIA-010).
    fn is_external(&self, abs: &Path) -> bool {
        matches!(self.looks.get(abs).map(|l| &l.slot), Some(LookSlot::Ready { external: true, .. }))
    }

    /// The sound of a GIF comes on when the person asks for it.
    fn unsilence(&mut self) {
        if let Some(c) = self.current.as_mut() {
            c.silent = false;
        }
    }

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
        if let Status::Ready(shared, _) = &*c.status.lock().unwrap_or_else(|e| e.into_inner()) {
            shared.set_volume(if c.silent { 0.0 } else { gain });
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
            let frames = Arc::new(FrameSlot::default());
            let video = super::is_video(abs).then(|| {
                let info = match self.looks.get(abs).map(|l| &l.slot) {
                    Some(LookSlot::Ready { info, .. }) => info.clone(),
                    _ => None,
                };
                VideoJob { info, locate: self.locate, frames: frames.clone(), ctx: ctx.clone() }
            });
            std::thread::spawn(move || {
                run_output(&path, &st, &stp, open, video);
                ctx2.request_repaint();
            });
            self.current = Some(Playback { path: abs.to_path_buf(), owner: owner.to_path_buf(), status, stop, want_play: false, start_at: None, start_share: None, frames, frame_tex: None, looping: false, silent: false });
        }
        self.current.as_mut().expect("just set")
    }
}

/// What the output thread of a video needs besides the file.
struct VideoJob {
    /// from the look, if it was done already
    info: Option<Info>,
    locate: fn() -> Option<Tools>,
    frames: Arc<FrameSlot>,
    ctx: egui::Context,
}

/// Play a video through ffmpeg (MEDIA-009).
fn play_video(path: &Path, shared: &Shared, stop: &AtomicBool, job: VideoJob) -> Result<(), String> {
    let tools = (job.locate)().ok_or("ffmpeg is not installed.")?;
    let info = match job.info {
        Some(i) => i,
        None => tools.probe(path)?,
    };
    video::run(&tools, path, &info, shared, stop, &job.frames, &job.ctx)
}

/// The output thread: opens the sound card, reports, decodes into it until stopped.
fn run_output(path: &Path, status: &Mutex<Status>, stop: &AtomicBool, open: fn() -> Result<Output, String>, video: Option<VideoJob>) {
    let set = |s: Status| *status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    // a video without a sound card still plays its pictures, timed by the wall clock (MEDIA-013)
    let (output, no_sound) = match open() {
        Ok(o) => (o, false),
        Err(_) if video.is_some() => (Output::without_device(), true),
        Err(_) => return set(Status::NoOutput),
    };
    set(Status::Ready(output.shared.clone(), no_sound));
    let played = match video {
        Some(job) => play_video(path, &output.shared, stop, job),
        None => super::decode::run(path, &output.shared, stop),
    };
    if let Err(e) = played {
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
