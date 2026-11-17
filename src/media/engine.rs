//! The sound engine (MEDIA-002, MEDIA-005, MEDIA-006): a queue of f32 samples that any source
//! can fill (the decoder thread of an audio file, later the sound of a video), and the mixer that
//! the output callback runs. The mixer does everything that decides how the sound is heard
//! (fades, volume, clipping guard, position) and knows nothing about sound cards, so the tests
//! render it into a buffer (MEDIA-011); `output` is the thin part that opens the device.
//!
//! The samples in the queue are interleaved f32 in the device's channel count and sample rate:
//! the source converts before it pushes.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Start, pause and seek ramp the gain over this long (MEDIA-005).
pub const FADE_SECS: f32 = 0.005;
/// The volume follows its target with about this time constant (MEDIA-006).
const VOLUME_SMOOTH_SECS: f32 = 0.01;
/// How much sound the queue holds ahead of the output.
const QUEUE_SECS: usize = 2;

struct Inner {
    samples: VecDeque<f32>,
    /// Counts the seeks that were carried out; a source tags what it pushes with the epoch it
    /// read, so sound of before a seek never reaches the output after it.
    epoch: u64,
    /// A seek the mixer has not carried out yet: it first fades the old sound out.
    pending_seek: Option<f64>,
    /// Where the source has to continue (seconds), valid for `epoch`.
    seek_target: f64,
    /// The position of the first frame after the last seek, in frames, and the frames played since.
    base_frames: u64,
    consumed_frames: u64,
    eof: bool,
}

/// What the mixer, the source and the player share.
pub struct Shared {
    pub rate: u32,
    pub channels: usize,
    inner: Mutex<Inner>,
    playing: AtomicBool,
    /// target gain 0..=1 as f32 bits (the volume, 0 when muted)
    volume: AtomicU32,
    /// the position in frames, for the player's progress bar
    position_frames: AtomicU64,
    /// the source ran out of sound and the queue is empty
    finished: AtomicBool,
}

/// What `Shared::push` did with the samples.
#[derive(Debug, PartialEq, Eq)]
pub enum Pushed {
    Accepted,
    /// The queue is full: try again soon.
    Full,
    /// A seek happened since the source read the epoch: drop this, `poll_seek` has the new place.
    Stale,
}

impl Shared {
    pub fn new(rate: u32, channels: usize) -> Arc<Shared> {
        Arc::new(Shared {
            rate,
            channels: channels.max(1),
            inner: Mutex::new(Inner { samples: VecDeque::new(), epoch: 0, pending_seek: None, seek_target: 0.0, base_frames: 0, consumed_frames: 0, eof: false }),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            position_frames: AtomicU64::new(0),
            finished: AtomicBool::new(false),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn capacity(&self) -> usize {
        self.rate as usize * self.channels * QUEUE_SECS
    }

    // --- the player's side

    pub fn set_playing(&self, on: bool) {
        self.playing.store(on, Ordering::Relaxed);
    }

    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Relaxed)
    }

    /// The volume to play with, 0..=1; anything else is brought into that range.
    pub fn set_volume(&self, gain: f32) {
        self.volume.store(clamp_gain(gain).to_bits(), Ordering::Relaxed);
    }

    /// Ask for a jump to `secs`: the mixer fades out, then the source continues from there.
    pub fn seek(&self, secs: f64) {
        self.lock().pending_seek = Some(secs.max(0.0));
        self.finished.store(false, Ordering::Relaxed);
    }

    /// Where the sound is now, in seconds (a seek that is still fading out already counts).
    pub fn position_secs(&self) -> f64 {
        if let Some(p) = self.lock().pending_seek {
            return p;
        }
        self.position_frames.load(Ordering::Relaxed) as f64 / self.rate as f64
    }

    /// Tests: a seek was asked for and the mixer has not taken it up yet.
    #[cfg(test)]
    pub fn seek_pending(&self) -> bool {
        self.lock().pending_seek.is_some()
    }

    /// The source is at its end and everything it gave has been played.
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }

    // --- the source's side

    /// The epoch to tag pushes with, and where the source starts (seconds).
    pub fn epoch(&self) -> u64 {
        self.lock().epoch
    }

    /// If a seek was carried out since the source last looked (`seen` is the epoch it knows):
    /// the new epoch and the place to continue from.
    pub fn poll_seek(&self, seen: u64) -> Option<(u64, f64)> {
        let g = self.lock();
        (g.epoch != seen).then_some((g.epoch, g.seek_target))
    }

    pub fn push(&self, epoch: u64, data: &[f32]) -> Pushed {
        let mut g = self.lock();
        if g.epoch != epoch {
            return Pushed::Stale;
        }
        if g.samples.len() >= self.capacity() {
            return Pushed::Full;
        }
        g.samples.extend(data.iter().copied());
        Pushed::Accepted
    }

    /// The source has nothing more (for this epoch).
    pub fn set_eof(&self, epoch: u64) {
        let mut g = self.lock();
        if g.epoch == epoch {
            g.eof = true;
        }
    }
}

fn clamp_gain(g: f32) -> f32 {
    if g.is_finite() {
        g.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Turns the queue into output samples. Lives in the output callback.
pub struct Mixer {
    shared: Arc<Shared>,
    /// the start/pause/seek ramp, 0..=1
    play_gain: f32,
    /// the smoothed volume, 0..=1
    vol_gain: f32,
    fade_step: f32,
    vol_coef: f32,
    /// one frame of the queue (kept here: the callback must not allocate)
    frame: Vec<f32>,
}

impl Mixer {
    pub fn new(shared: Arc<Shared>) -> Mixer {
        let rate = shared.rate as f32;
        let ch = shared.channels;
        let vol_gain = f32::from_bits(shared.volume.load(Ordering::Relaxed));
        Mixer { shared, play_gain: 0.0, vol_gain, fade_step: 1.0 / (FADE_SECS * rate), vol_coef: 1.0 - (-1.0 / (VOLUME_SMOOTH_SECS * rate)).exp(), frame: vec![0.0; ch] }
    }

    /// Fill `out` (interleaved, `shared.channels` per frame) with the next sound.
    pub fn render(&mut self, out: &mut [f32]) {
        let ch = self.shared.channels;
        let playing = self.shared.playing.load(Ordering::Relaxed);
        let vol_target = f32::from_bits(self.shared.volume.load(Ordering::Relaxed));
        let mut g = self.shared.lock();
        for chunk in out.chunks_mut(ch) {
            // a seek waits until the old sound has faded out
            if self.play_gain <= 0.0 {
                if let Some(to) = g.pending_seek.take() {
                    g.epoch += 1;
                    g.samples.clear();
                    g.eof = false;
                    g.seek_target = to;
                    g.base_frames = (to * self.shared.rate as f64).round() as u64;
                    g.consumed_frames = 0;
                }
            }
            let want = playing && g.pending_seek.is_none();
            self.vol_gain += (vol_target - self.vol_gain) * self.vol_coef;
            let have = g.samples.len() >= ch;
            if !want && self.play_gain <= 0.0 {
                chunk.fill(0.0);
                continue;
            }
            if have {
                for s in self.frame.iter_mut() {
                    *s = g.samples.pop_front().unwrap_or(0.0);
                }
                g.consumed_frames += 1;
                self.play_gain = if want { (self.play_gain + self.fade_step).min(1.0) } else { (self.play_gain - self.fade_step).max(0.0) };
            } else {
                // nothing to play: the sound comes back with a fade-in
                if g.eof && g.samples.is_empty() {
                    self.shared.finished.store(true, Ordering::Relaxed);
                }
                self.play_gain = 0.0;
                chunk.fill(0.0);
                continue;
            }
            let k = self.play_gain * self.vol_gain;
            for (o, s) in chunk.iter_mut().zip(self.frame.iter()) {
                let v = *s * k;
                *o = if v.is_finite() { v.clamp(-1.0, 1.0) } else { 0.0 };
            }
        }
        self.shared.position_frames.store(g.base_frames + g.consumed_frames, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 48_000;

    /// `secs` of a sine of 440 Hz at this amplitude, in `ch` channels.
    fn sine(secs: f32, amp: f32, ch: usize) -> Vec<f32> {
        let n = (secs * RATE as f32) as usize;
        (0..n).flat_map(|i| std::iter::repeat_n(amp * (2.0 * std::f32::consts::PI * 440.0 * i as f32 / RATE as f32).sin(), ch)).collect()
    }

    fn engine(ch: usize) -> (Arc<Shared>, Mixer) {
        let s = Shared::new(RATE, ch);
        (s.clone(), Mixer::new(s))
    }

    /// Render `frames` frames in blocks of 256 and return the first channel.
    fn run(m: &mut Mixer, ch: usize, frames: usize) -> Vec<f32> {
        let mut all = Vec::new();
        let mut left = frames;
        while left > 0 {
            let n = left.min(256);
            let mut buf = vec![0.0; n * ch];
            m.render(&mut buf);
            all.extend(buf.chunks(ch).map(|f| f[0]));
            left -= n;
        }
        all
    }

    /// The biggest step between two neighbouring samples.
    fn max_step(v: &[f32]) -> f32 {
        v.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max)
    }

    /// What the loudest sine of the tests changes from one sample to the next, with a margin.
    const NATURAL_STEP: f32 = 0.07;

    // spec: MEDIA-005
    #[test]
    fn start_fades_in_without_a_jump() {
        let (s, mut m) = engine(1);
        s.push(0, &sine(1.0, 0.9, 1));
        let silent = run(&mut m, 1, 1000);
        assert!(silent.iter().all(|x| *x == 0.0), "nothing plays before play");
        s.set_playing(true);
        let out = run(&mut m, 1, 4800);
        assert!(max_step(&out) < NATURAL_STEP, "no click at the start: {}", max_step(&out));
        assert!(out.iter().take(50).all(|x| x.abs() < 0.2), "starts quiet");
        assert!(out.iter().skip(1000).map(|x| x.abs()).fold(0.0, f32::max) > 0.8, "and reaches full level");
    }

    // spec: MEDIA-005
    #[test]
    fn pause_fades_out_within_a_few_milliseconds_and_resumes_where_it_was() {
        let (s, mut m) = engine(1);
        s.push(0, &sine(1.0, 0.9, 1));
        s.set_playing(true);
        run(&mut m, 1, 4800);
        s.set_playing(false);
        let out = run(&mut m, 1, 2400);
        assert!(max_step(&out) < NATURAL_STEP, "no click at the pause: {}", max_step(&out));
        assert!(out[(FADE_SECS * RATE as f32) as usize + 8..].iter().all(|x| *x == 0.0), "silent after the fade");
        let paused_at = s.position_secs();
        run(&mut m, 1, 2400);
        assert_eq!(s.position_secs(), paused_at, "the position stands still while paused");
        s.set_playing(true);
        let out = run(&mut m, 1, 2400);
        assert!(max_step(&out) < NATURAL_STEP, "no click at the resume");
        assert!(out.iter().any(|x| x.abs() > 0.5));
    }

    // spec: MEDIA-005
    #[test]
    fn a_seek_fades_out_then_the_source_continues_from_the_new_place_and_fades_in() {
        let (s, mut m) = engine(1);
        s.push(0, &sine(1.0, 0.9, 1));
        s.set_playing(true);
        run(&mut m, 1, 4800);
        s.seek(10.0);
        assert_eq!(s.position_secs(), 10.0, "the new place counts at once");
        let before = run(&mut m, 1, 600);
        assert!(max_step(&before) < NATURAL_STEP, "no click when the old sound leaves");
        let (epoch, to) = s.poll_seek(0).expect("the source is told");
        assert_eq!((epoch, to), (1, 10.0));
        assert_eq!(s.push(0, &[0.5; 10]), Pushed::Stale, "sound of before the seek is dropped");
        assert_eq!(s.push(1, &sine(1.0, 0.9, 1)), Pushed::Accepted);
        let after = run(&mut m, 1, 4800);
        assert!(max_step(&after) < NATURAL_STEP, "no click when the new sound comes in: {}", max_step(&after));
        assert!(after.iter().any(|x| x.abs() > 0.8));
        assert!((s.position_secs() - (10.0 + 4800.0 / RATE as f64)).abs() < 0.01, "the position goes on from the new place: {}", s.position_secs());
    }

    // spec: MEDIA-005
    #[test]
    fn seeking_before_the_new_sound_has_arrived_waits_with_silence_and_fades_in_later() {
        let (s, mut m) = engine(1);
        s.set_playing(true);
        s.push(0, &sine(0.5, 0.9, 1));
        run(&mut m, 1, 1000);
        s.seek(2.0);
        let gap = run(&mut m, 1, 4800);
        assert!(gap.iter().skip(300).all(|x| *x == 0.0), "silence while the source seeks");
        s.push(1, &sine(0.5, 0.9, 1));
        let out = run(&mut m, 1, 2400);
        assert!(max_step(&out) < NATURAL_STEP, "the first sound after the wait is faded in: {}", max_step(&out));
    }

    // spec: MEDIA-005
    #[test]
    fn nothing_goes_beyond_full_scale_even_when_the_source_does() {
        let (s, mut m) = engine(2);
        s.set_volume(1.0);
        let loud: Vec<f32> = sine(0.5, 3.0, 2);
        s.push(0, &loud);
        s.set_playing(true);
        let mut buf = vec![0.0; 2 * 4800];
        m.render(&mut buf);
        assert!(buf.iter().all(|x| x.abs() <= 1.0), "clamped");
        assert!(buf.iter().any(|x| x.abs() == 1.0), "and actually reached by the loud input");
        let (s, mut m) = engine(1);
        s.push(0, &[f32::NAN, f32::INFINITY, -9.0, 9.0].repeat(2000));
        s.set_playing(true);
        let out = run(&mut m, 1, 8000);
        assert!(out.iter().all(|x| x.abs() <= 1.0), "also for odd values");
    }

    // spec: MEDIA-006
    #[test]
    fn the_volume_scales_the_sound_and_zero_is_silent() {
        let s = Shared::new(RATE, 1);
        s.set_volume(0.5);
        let mut m = Mixer::new(s.clone());
        s.push(0, &sine(1.0, 0.8, 1));
        s.set_playing(true);
        let out = run(&mut m, 1, 9600);
        let peak = out.iter().skip(2000).map(|x| x.abs()).fold(0.0, f32::max);
        assert!((peak - 0.4).abs() < 0.02, "half volume halves the level: {peak}");
        s.set_volume(0.0);
        let out = run(&mut m, 1, 9600);
        assert!(out[4800..].iter().all(|x| x.abs() < 1e-3), "muted is silent");
    }

    // spec: MEDIA-006
    #[test]
    fn a_volume_change_is_smoothed_and_the_gain_never_exceeds_one() {
        let s = Shared::new(RATE, 1);
        s.set_volume(0.0);
        let mut m = Mixer::new(s.clone());
        // a constant signal shows the gain itself
        s.push(0, &vec![0.5; RATE as usize]);
        s.set_playing(true);
        run(&mut m, 1, 2400);
        s.set_volume(1.0);
        let out = run(&mut m, 1, 2400);
        assert!(max_step(&out) < 0.02, "the change comes as a slope, not a step: {}", max_step(&out));
        assert!(out.last().copied().unwrap() > 0.45, "and arrives");
        s.set_volume(7.0);
        assert_eq!(f32::from_bits(s.volume.load(Ordering::Relaxed)), 1.0, "a gain above one is brought down");
        s.set_volume(-1.0);
        assert_eq!(f32::from_bits(s.volume.load(Ordering::Relaxed)), 0.0);
        s.set_volume(f32::NAN);
        assert_eq!(f32::from_bits(s.volume.load(Ordering::Relaxed)), 0.0);
    }

    // spec: MEDIA-002
    #[test]
    fn the_end_of_the_source_is_reported_once_the_queue_is_played_out() {
        let (s, mut m) = engine(2);
        s.push(0, &sine(0.1, 0.5, 2));
        s.set_eof(0);
        s.set_playing(true);
        run(&mut m, 2, 2000);
        assert!(!s.is_finished(), "still playing the queue");
        run(&mut m, 2, 4000);
        assert!(s.is_finished());
        assert!((s.position_secs() - 0.1).abs() < 0.001, "the position is the length: {}", s.position_secs());
        s.seek(0.0);
        assert!(!s.is_finished(), "a seek starts over");
    }

    // spec: MEDIA-002
    #[test]
    fn the_queue_holds_back_a_source_that_is_too_fast() {
        let (s, _m) = engine(2);
        let chunk = sine(1.0, 0.1, 2);
        assert_eq!(s.push(0, &chunk), Pushed::Accepted);
        assert_eq!(s.push(0, &chunk), Pushed::Accepted);
        assert_eq!(s.push(0, &chunk), Pushed::Full, "two seconds are enough");
    }

    // spec: MEDIA-005
    #[test]
    fn a_pause_in_the_middle_of_a_seek_does_not_lose_the_seek() {
        let (s, mut m) = engine(1);
        s.push(0, &sine(0.5, 0.9, 1));
        s.set_playing(true);
        run(&mut m, 1, 2400);
        s.seek(3.0);
        s.set_playing(false);
        run(&mut m, 1, 2400);
        assert_eq!(s.poll_seek(0), Some((1, 3.0)), "the seek happened although it is paused");
        assert!(!s.is_playing());
        assert!((s.position_secs() - 3.0).abs() < 1e-6);
    }
}
