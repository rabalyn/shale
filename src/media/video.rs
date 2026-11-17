//! Playing a video (MEDIA-009): two ffmpeg processes, one with the pictures (raw RGBA frames) and
//! one with the sound (f32 samples). The sound goes into the same engine queue as every audio
//! file, so fades, volume and pause work as they do there, and its clock (`Shared::position_secs`)
//! leads: a frame waits until the clock reaches it and is dropped when the clock is far past it.
//! A video without sound gets silence from a source of its own, which makes the engine's clock
//! the frame clock. A seek (the engine tells both sources through its epoch) restarts both
//! processes at the new place; a process that is let go is killed and reaped.
//!
//! Nothing here runs on the window's thread: `look` and `run` are called from threads of the
//! player, the newest frame is handed over in a `FrameSlot` and the window is woken.

use super::cover::Look;
use super::decode::push_all;
use super::engine::Shared;
use super::ffmpeg::{Info, Tools};
use eframe::egui;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

/// A frame later than this behind the clock is not shown (when one has been shown since the seek).
const LATE_DROP: f64 = 0.15;
/// The longest a frame waits before the clock is looked at again.
const WAIT_STEP: f64 = 0.01;
/// Silence for a video without sound is made in pieces of this many frames.
const SILENCE_CHUNK: usize = 2400;

/// The newest frame the video thread has for the window; the window takes it.
pub type FrameSlot = Mutex<Option<egui::ColorImage>>;

/// What a quick look at a video file found.
pub struct Looked {
    pub look: Look,
    pub info: Option<Info>,
    /// ffmpeg is not installed (MEDIA-010)
    pub missing: bool,
}

/// Length and first frame of a video (MEDIA-009). Slow: not for the window's thread.
pub fn look(path: &Path, locate: fn() -> Option<Tools>) -> Looked {
    let Some(tools) = locate() else {
        return Looked { look: Look { duration: None, cover: None, error: None }, info: None, missing: true };
    };
    match tools.probe(path) {
        Ok(info) => {
            let cover = tools.thumbnail(path, &info).ok();
            Looked { look: Look { duration: info.duration, cover, error: None }, info: Some(info), missing: false }
        }
        Err(e) => Looked { look: Look { duration: None, cover: None, error: Some(e) }, info: None, missing: false },
    }
}

/// What to do with a frame that belongs at `pts` while the clock says `clock`.
#[derive(Debug, PartialEq)]
pub enum Step {
    Show,
    Drop,
    Wait(Duration),
}

/// The frame rule (MEDIA-009): early frames wait for the clock, frames that are on time or a
/// little late are shown, frames that are very late are dropped to catch up, unless nothing has
/// been shown since the last seek (then the picture is better late than never).
pub fn step(pts: f64, clock: f64, shown: bool) -> Step {
    let ahead = pts - clock;
    if ahead > 0.001 {
        Step::Wait(Duration::from_secs_f64(ahead.min(WAIT_STEP)))
    } else if shown && -ahead > LATE_DROP {
        Step::Drop
    } else {
        Step::Show
    }
}

/// The time of frame number `index` of a run that began at `start`.
pub fn frame_time(start: f64, index: u64, fps: f64) -> f64 {
    start + index as f64 / fps
}

/// Play `path` into `shared` until `stop` is set: the pictures into `slot`, the sound (or the
/// silence of a video without sound) into the engine. Seeks come through `shared`.
pub fn run(tools: &Tools, path: &Path, info: &Info, shared: &Shared, stop: &AtomicBool, slot: &FrameSlot, ctx: &egui::Context) -> Result<(), String> {
    let session = Session { tools, path, info, shared, stop, slot, ctx, video_ended: AtomicBool::new(false) };
    std::thread::scope(|s| {
        let pictures = s.spawn(|| {
            let r = picture_loop(&session);
            stop.store(true, Ordering::Relaxed);
            r
        });
        let sound = if info.has_audio { sound_loop(tools, path, shared, stop) } else { silence_loop(info, shared, stop, &session.video_ended) };
        // whichever ends first (on an error) ends the other
        stop.store(true, Ordering::Relaxed);
        let pictures = pictures.join().unwrap_or_else(|_| Err("The video player stopped unexpectedly.".into()));
        sound.and(pictures)
    })
}

/// What the threads of one playing video share.
struct Session<'a> {
    tools: &'a Tools,
    path: &'a Path,
    info: &'a Info,
    shared: &'a Shared,
    stop: &'a AtomicBool,
    slot: &'a FrameSlot,
    ctx: &'a egui::Context,
    /// the pictures are at their end (for a video of unknown length without sound)
    video_ended: AtomicBool,
}

/// The frames: read them from ffmpeg and show each at its time.
fn picture_loop(session: &Session) -> Result<(), String> {
    let Session { tools, path, info, shared, stop, slot, ctx, video_ended: ended } = session;
    let (w, h) = info.picture_size();
    let fps = info.play_fps();
    let mut buf = vec![0u8; w as usize * h as usize * 4];
    let mut epoch = shared.epoch();
    let mut start = 0.0;
    let mut proc = Some(tools.frames(path, start, info)?);
    let (mut index, mut shown, mut eof) = (0u64, false, false);
    while !stop.load(Ordering::Relaxed) {
        if let Some((e, to)) = shared.poll_seek(epoch) {
            epoch = e;
            start = to;
            drop(proc.take()); // the old process is killed before the new one starts
            proc = Some(tools.frames(path, start, info)?);
            (index, shown, eof) = (0, false, false);
            ended.store(false, Ordering::Relaxed);
            continue;
        }
        let Some(p) = proc.as_mut().filter(|_| !eof) else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        if p.stdout().read_exact(&mut buf).is_err() {
            if index == 0 {
                return Err(p.last_words());
            }
            eof = true;
            ended.store(true, Ordering::Relaxed);
            continue;
        }
        let pts = frame_time(start, index, fps);
        index += 1;
        loop {
            if stop.load(Ordering::Relaxed) || shared.poll_seek(epoch).is_some() {
                break;
            }
            match step(pts, shared.position_secs(), shown) {
                Step::Wait(d) => std::thread::sleep(d),
                Step::Drop => break,
                Step::Show => {
                    let image = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &buf);
                    *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(image);
                    ctx.request_repaint();
                    shown = true;
                    break;
                }
            }
        }
    }
    Ok(())
}

/// The sound of the file from ffmpeg into the engine's queue.
fn sound_loop(tools: &Tools, path: &Path, shared: &Shared, stop: &AtomicBool) -> Result<(), String> {
    let mut epoch = shared.epoch();
    let mut proc = Some(tools.sound(path, 0.0, shared.rate, shared.channels)?);
    let (mut bytes, mut samples) = (vec![0u8; 16 * 1024], Vec::<f32>::new());
    let (mut carry, mut got, mut eof) = (Vec::<u8>::new(), false, false);
    while !stop.load(Ordering::Relaxed) {
        if let Some((e, to)) = shared.poll_seek(epoch) {
            epoch = e;
            drop(proc.take());
            proc = Some(tools.sound(path, to, shared.rate, shared.channels)?);
            (carry, got, eof) = (Vec::new(), false, false);
            continue;
        }
        let Some(p) = proc.as_mut().filter(|_| !eof) else {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        };
        let n = p.stdout().read(&mut bytes).unwrap_or(0);
        if n == 0 {
            if !got {
                return Err(p.last_words());
            }
            shared.set_eof(epoch);
            eof = true;
            continue;
        }
        got = true;
        carry.extend_from_slice(&bytes[..n]);
        let whole = carry.len() / 4 * 4;
        samples.clear();
        samples.extend(carry[..whole].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)));
        carry.drain(..whole);
        // out of date (a seek came) or stopped: the loop above looks at that
        push_all(shared, epoch, &samples, stop);
    }
    Ok(())
}

/// A video without sound: silence as long as the video, so the engine's clock runs as the
/// frame clock (pause, seek and the end work as they do for sound).
fn silence_loop(info: &Info, shared: &Shared, stop: &AtomicBool, video_ended: &AtomicBool) -> Result<(), String> {
    let total = info.duration.map(|d| (d * shared.rate as f64) as usize);
    let (mut epoch, mut at, mut eof) = (shared.epoch(), 0usize, false);
    let mut chunk = vec![0.0f32; SILENCE_CHUNK * shared.channels];
    while !stop.load(Ordering::Relaxed) {
        if let Some((e, to)) = shared.poll_seek(epoch) {
            (epoch, at, eof) = (e, (to * shared.rate as f64) as usize, false);
            continue;
        }
        let end = total.unwrap_or(usize::MAX);
        if eof || at >= end || (total.is_none() && video_ended.load(Ordering::Relaxed)) {
            if !eof {
                shared.set_eof(epoch);
                eof = true;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let frames = SILENCE_CHUNK.min(end - at);
        if push_all(shared, epoch, &chunk[..frames * shared.channels], stop) {
            at += frames;
        }
        chunk.fill(0.0);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::engine::Mixer;
    use crate::media::ffmpeg::locate;
    use crate::media::testdata::fixture;
    use std::sync::Arc;

    // spec: MEDIA-009
    #[test]
    fn a_frame_waits_for_the_clock_and_a_late_one_is_dropped() {
        assert_eq!(step(1.0, 1.0, true), Step::Show, "on time");
        assert_eq!(step(1.0, 1.1, true), Step::Show, "a little late is still shown");
        assert_eq!(step(1.0, 1.5, true), Step::Drop, "far behind: dropped to catch up");
        assert_eq!(step(1.0, 1.5, false), Step::Show, "but the first picture after a seek is never dropped");
        assert_eq!(step(1.004, 1.0, true), Step::Wait(Duration::from_secs_f64(0.004)), "early: wait the difference");
        assert_eq!(step(5.0, 1.0, true), Step::Wait(Duration::from_secs_f64(WAIT_STEP)), "but look at the clock again soon");
    }

    // spec: MEDIA-009
    #[test]
    fn frames_are_timed_from_where_the_run_began() {
        assert_eq!(frame_time(0.0, 0, 25.0), 0.0);
        assert_eq!(frame_time(10.0, 50, 25.0), 12.0);
        assert!((frame_time(1.0, 3, 30.0) - 1.1).abs() < 1e-9);
    }

    /// The tools of this machine, or None with a note (BUILD-018).
    pub fn tools_or_skip() -> Option<Tools> {
        let t = locate();
        if t.is_none() {
            println!("skipping: ffmpeg and ffprobe are not installed");
        }
        t
    }

    // spec: MEDIA-009
    #[test]
    fn the_look_of_a_video_has_its_length_and_the_first_frame() {
        let Some(_) = tools_or_skip() else { return };
        for (name, sound) in [("clip.mp4", true), ("clip-silent.mp4", false)] {
            let l = look(&fixture(name), locate);
            assert_eq!((l.missing, l.look.error.as_deref()), (false, None), "{name}");
            let d = l.look.duration.expect("length");
            assert!((d - 1.0).abs() < 0.2, "{name}: {d}");
            let info = l.info.unwrap();
            assert_eq!((info.width, info.height, info.has_audio), (160, 90, sound), "{name}");
            // the fixture is red in its first frame
            let pic = l.look.cover.expect("the first frame");
            assert_eq!(pic.size, [160, 90]);
            let px = pic.pixels[45 * 160 + 80];
            assert!(px.r() > 180 && px.g() < 90 && px.b() < 90, "{name}: {px:?}");
        }
    }

    // spec: MEDIA-009
    #[test]
    fn a_file_that_is_no_video_is_an_error_and_no_ffmpeg_is_not() {
        let Some(_) = tools_or_skip() else { return };
        let l = look(&fixture("tone.wav"), locate);
        assert!(l.look.error.is_some() && !l.missing, "an audio file has no picture");
        let dir = std::env::temp_dir().join(format!("shale-video-junk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("junk.mp4"), vec![7u8; 4000]).unwrap();
        let l = look(&dir.join("junk.mp4"), locate);
        assert!(l.look.error.is_some(), "damaged: {:?}", l.look.error);
        let _ = std::fs::remove_dir_all(&dir);
        let none = look(&fixture("clip.mp4"), || None);
        assert!(none.missing && none.look.error.is_none() && none.info.is_none(), "MEDIA-010: no ffmpeg is not an error");
    }

    /// Render the mixer in the background like a sound card would, 10 ms at a time in real time.
    fn card(shared: &Arc<Shared>, stop: &Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
        let (shared, stop) = (shared.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut m = Mixer::new(shared.clone());
            let mut buf = vec![0.0f32; shared.rate as usize / 100 * shared.channels];
            while !stop.load(Ordering::Relaxed) {
                m.render(&mut buf);
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    }

    /// Sets the flag when dropped, also when the owner unwinds from a panic.
    struct StopOnDrop<'a>(&'a AtomicBool);

    impl Drop for StopOnDrop<'_> {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    // spec: MEDIA-009
    #[test]
    fn a_failing_test_body_still_stops_the_player_threads() {
        let stop = AtomicBool::new(false);
        // like `playing`: a thread that only ends when `stop` is set, and a body that panics
        let r = std::panic::catch_unwind(|| {
            std::thread::scope(|s| {
                s.spawn(|| {
                    while !stop.load(Ordering::Relaxed) {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                });
                let _stops = StopOnDrop(&stop);
                panic!("an assert failed");
            });
        });
        assert!(r.is_err() && stop.load(Ordering::Relaxed), "the panic comes out instead of a hang");
    }

    /// Wait up to `secs` for `ok`.
    fn wait_for(secs: f64, mut ok: impl FnMut() -> bool) -> bool {
        let end = std::time::Instant::now() + Duration::from_secs_f64(secs);
        while std::time::Instant::now() < end {
            if ok() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        ok()
    }

    /// Play `name` for a while into a silent card; hands the shared state and the slot to `body`.
    fn playing(name: &str, body: impl FnOnce(&Arc<Shared>, &FrameSlot, &AtomicBool)) {
        let Some(tools) = tools_or_skip() else { return };
        let path = fixture(name);
        let info = tools.probe(&path).unwrap();
        let shared = Shared::new(48_000, 2);
        let stop = Arc::new(AtomicBool::new(false));
        let slot: FrameSlot = Mutex::new(None);
        let ctx = egui::Context::default();
        let device = card(&shared, &stop);
        std::thread::scope(|s| {
            let runner = s.spawn(|| run(&tools, &path, &info, &shared, &stop, &slot, &ctx));
            // a failing assert in `body` must still stop the runner and the card, or the scope waits forever
            let _stops = StopOnDrop(&stop);
            body(&shared, &slot, &stop);
            drop(_stops);
            runner.join().unwrap().unwrap();
        });
        device.join().unwrap();
    }

    // spec: MEDIA-009
    #[test]
    fn a_video_with_sound_shows_frames_by_the_sound_clock_and_ends() {
        playing("clip.mp4", |shared, slot, _| {
            shared.set_playing(true);
            assert!(wait_for(5.0, || slot.lock().unwrap().is_some()), "a frame arrives");
            assert!(wait_for(1.0, || shared.position_secs() > 0.3), "the sound clock runs");
            let frame = slot.lock().unwrap().take();
            assert!(frame.is_none_or(|f| f.size == [160, 90]));
            assert!(wait_for(6.0, || shared.is_finished()), "the end is reported after about a second");
            assert!(shared.position_secs() > 0.8, "{}", shared.position_secs());
        });
    }

    // spec: MEDIA-009
    #[test]
    fn a_video_without_sound_is_timed_by_silence() {
        playing("clip-silent.mp4", |shared, slot, _| {
            assert!(wait_for(5.0, || slot.lock().unwrap().is_some()), "the first frame shows although it is paused");
            std::thread::sleep(Duration::from_millis(200));
            assert!(shared.position_secs() < 0.01, "paused: the clock stands");
            shared.set_playing(true);
            assert!(wait_for(1.0, || shared.position_secs() > 0.3), "the frame clock runs");
            assert!(wait_for(6.0, || shared.is_finished()), "and ends at the length of the video");
        });
    }

    // spec: MEDIA-009
    #[test]
    fn a_seek_restarts_the_pictures_at_the_new_place() {
        playing("clip.mp4", |shared, slot, _| {
            assert!(wait_for(5.0, || slot.lock().unwrap().is_some()), "the first frame");
            slot.lock().unwrap().take();
            shared.set_playing(true);
            assert!(wait_for(1.0, || shared.position_secs() > 0.1));
            shared.seek(0.7);
            // the fixture is red at the start and blue at the end: a frame after the seek is bluish
            assert!(wait_for(5.0, || slot.lock().unwrap().as_ref().is_some_and(|f| f.pixels[45 * 160 + 80].b() > f.pixels[45 * 160 + 80].r())), "a frame of the second half");
            assert!(shared.position_secs() >= 0.6);
        });
    }
}
