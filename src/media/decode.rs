//! Reading an audio file (MEDIA-002): `symphonia` opens and decodes it, the samples are turned
//! into the output's channel count and rate (`rubato` when the rates differ) and pushed into the
//! engine's queue. `probe` is the quick look the player does first (length, cover, whether the
//! file can be played at all); `run` is the decoder thread's loop.

use super::engine::{Pushed, Shared};
use super::ffmpeg::{self, Proc, Tools};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, Visual};
use symphonia::core::units::Time;

/// The frames the resampler takes in at a time.
const RESAMPLE_CHUNK: usize = 1024;

/// The rate and channel count ffmpeg is asked for when it decodes a file symphonia cannot (the
/// converter after it brings them to the output's).
const FALLBACK_RATE: u32 = 48_000;
const FALLBACK_CHANNELS: usize = 2;

/// An opened audio file: the container reader and decoder of its sound track, or ffmpeg's
/// output (MEDIA-026), and what the tags say.
pub struct Source {
    backend: Backend,
    /// Length in seconds, when the file says.
    pub duration: Option<f64>,
    /// Pictures from the tags (cover art).
    pub visuals: Vec<Visual>,
}

enum Backend {
    Symphonia { format: Box<dyn FormatReader + 'static>, decoder: Box<dyn AudioDecoder>, track_id: u32 },
    Ffmpeg(Fallback),
}

/// The sound of a file as raw f32 samples from an ffmpeg process.
struct Fallback {
    tools: Tools,
    path: PathBuf,
    proc: Proc,
    /// ffmpeg gave sound since it was (re)started
    got: bool,
    carry: Vec<u8>,
    bytes: Vec<u8>,
}

/// What `open` could not do: the sentence for the player, and whether ffmpeg may try (not for a
/// file that cannot be read at all).
struct Failure {
    words: String,
    retry: bool,
}

fn describe(e: &Error) -> String {
    match e {
        Error::Unsupported(_) => "This kind of sound can't be played by Shale yet.".into(),
        Error::IoError(e) => format!("The file can't be read: {e}"),
        _ => "Shale can't read this file as a sound: it may be damaged.".into(),
    }
}

/// The words for a file symphonia could not open (MEDIA-027): a WAV file is named by what its
/// `fmt ` chunk says, so a file that VLC plays is never called damaged.
fn describe_open(path: &Path, e: &Error) -> String {
    let tag = crate::fsutil::open_read(path).ok().and_then(|mut f| super::wavfix::wav_format(&mut f));
    match (tag, e) {
        (_, Error::IoError(_)) | (None, _) => describe(e),
        (Some(0x0002 | 0x0011), _) => "WAV with ADPCM sound, which Shale can't play yet.".into(),
        (Some(t @ (0x0001 | 0x0003 | 0x0006 | 0x0007 | 0xFFFE)), _) => {
            let kind = match t {
                0x0001 => "PCM",
                0x0003 => "floating point",
                0x0006 => "A-law",
                0x0007 => "µ-law",
                _ => "multichannel",
            };
            format!("WAV with {kind} sound in a form Shale can't read yet.")
        }
        (Some(t), _) => format!("WAV with sound of a kind Shale can't play yet (format {t:#06x})."),
    }
}

/// Open a file for playing. The error is a sentence for the player. What symphonia cannot read,
/// the system's ffmpeg tries when it is installed (MEDIA-026).
pub fn open(path: &Path) -> Result<Source, String> {
    open_with(path, ffmpeg::locate)
}

fn open_with(path: &Path, locate: fn() -> Option<Tools>) -> Result<Source, String> {
    let failed = match open_symphonia(path) {
        Ok(src) => return Ok(src),
        Err(f) => f,
    };
    if failed.retry {
        if let Some(src) = locate().and_then(|tools| open_ffmpeg(tools, path)) {
            return Ok(src);
        }
    }
    Err(failed.words)
}

fn open_ffmpeg(tools: Tools, path: &Path) -> Option<Source> {
    let duration = match tools.audio_length(path) {
        Ok(d) => d,
        Err(e) => {
            tracing::info!("ffmpeg cannot read {} either: {e}", path.display());
            return None;
        }
    };
    let proc = tools.sound(path, 0.0, FALLBACK_RATE, FALLBACK_CHANNELS).ok()?;
    let fallback = Fallback { tools, path: path.to_path_buf(), proc, got: false, carry: Vec::new(), bytes: vec![0u8; 16 * 1024] };
    Some(Source { backend: Backend::Ffmpeg(fallback), duration, visuals: Vec::new() })
}

fn open_symphonia(path: &Path) -> Result<Source, Failure> {
    // the real error is for the log; the player gets words (MEDIA-027)
    let fail = |e: Error| {
        tracing::info!("symphonia cannot play {}: {e}", path.display());
        Failure { words: describe_open(path, &e), retry: !matches!(e, Error::IoError(_)) }
    };
    let none = || Failure { words: "There is no sound in this file.".to_string(), retry: true };
    let file = crate::fsutil::open_read(path).map_err(|e| Failure { words: format!("The file can't be opened: {e}"), retry: false })?;
    let source = super::wavfix::Widened::new(file).map_err(|e| fail(Error::IoError(e)))?;
    let mss = MediaSourceStream::new(Box::new(source), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_ascii_lowercase());
    }
    let mut format = symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default()).map_err(fail)?;
    let (track_id, params) = {
        let track = format.default_track(TrackType::Audio).ok_or_else(none)?;
        let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or_else(none)?.clone();
        (track.id, params)
    };
    let decoder = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).map_err(fail)?;
    let info = *format.media_info();
    let duration = info.time_base.zip(info.duration).and_then(|(tb, d)| tb.calc_duration(d)).map(|t| t.as_secs_f64()).filter(|d| *d > 0.0);
    let mut visuals = Vec::new();
    {
        let mut md = format.metadata();
        while let Some(rev) = md.current() {
            visuals.extend(rev.media.visuals.iter().cloned());
            for t in &rev.per_track {
                visuals.extend(t.metadata.visuals.iter().cloned());
            }
            if md.pop().is_none() {
                break;
            }
        }
    }
    Ok(Source { backend: Backend::Symphonia { format, decoder, track_id }, duration, visuals })
}

impl Source {
    /// Seek to `secs`; the result is how many seconds before that the reader landed (to skip).
    fn seek(&mut self, secs: f64) -> Result<f64, Error> {
        let (format, decoder, track_id) = match &mut self.backend {
            Backend::Symphonia { format, decoder, track_id } => (format, decoder, *track_id),
            Backend::Ffmpeg(f) => {
                // ffmpeg is started again at the new place and lands on it
                f.proc = f.tools.sound(&f.path, secs.max(0.0), FALLBACK_RATE, FALLBACK_CHANNELS).map_err(|e| Error::IoError(std::io::Error::other(e)))?;
                (f.got, f.carry) = (false, Vec::new());
                return Ok(0.0);
            }
        };
        let time = Time::try_from_secs_f64(secs).ok_or(Error::Unsupported("seek time"))?;
        let to = format.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(track_id) })?;
        decoder.reset();
        let tb = format.tracks().iter().find(|t| t.id == track_id).and_then(|t| t.time_base);
        Ok(tb.and_then(|tb| Some(tb.calc_time(to.required_ts)?.as_secs_f64() - tb.calc_time(to.actual_ts)?.as_secs_f64())).unwrap_or(0.0).max(0.0))
    }

    /// The next decoded sound as interleaved f32 in `buf`, with its rate and channel count;
    /// None at the end of the file.
    pub(crate) fn next(&mut self, buf: &mut Vec<f32>) -> Result<Option<(u32, usize)>, String> {
        let (format, decoder, track_id) = match &mut self.backend {
            Backend::Symphonia { format, decoder, track_id } => (format, decoder, *track_id),
            Backend::Ffmpeg(f) => return f.next(buf),
        };
        loop {
            let packet = match format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(Error::ResetRequired) => return Ok(None),
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(describe(&e)),
            };
            if packet.track_id != track_id {
                continue;
            }
            match decoder.decode(&packet) {
                Ok(audio) => {
                    let spec = audio.spec().clone();
                    audio.copy_to_vec_interleaved(buf);
                    return Ok(Some((spec.rate(), spec.channels().count().max(1))));
                }
                // a bad packet is skipped
                Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
                Err(e) => return Err(describe(&e)),
            }
        }
    }
}

impl Fallback {
    fn next(&mut self, buf: &mut Vec<f32>) -> Result<Option<(u32, usize)>, String> {
        loop {
            let n = std::io::Read::read(self.proc.stdout(), &mut self.bytes).unwrap_or(0);
            if n == 0 {
                return if self.got { Ok(None) } else { Err(self.proc.last_words()) };
            }
            self.got = true;
            self.carry.extend_from_slice(&self.bytes[..n]);
            // whole frames only
            let whole = self.carry.len() / (4 * FALLBACK_CHANNELS) * (4 * FALLBACK_CHANNELS);
            if whole == 0 {
                continue;
            }
            buf.clear();
            buf.extend(self.carry[..whole].as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)));
            self.carry.drain(..whole);
            return Ok(Some((FALLBACK_RATE, FALLBACK_CHANNELS)));
        }
    }
}

/// Bring interleaved sound from `src_ch` to `dst_ch` channels: mono is copied to every channel,
/// all channels are mixed down to a mono output, more channels than the output keep the first
/// ones, fewer leave the extra output channels silent.
pub fn remap_channels(src: &[f32], src_ch: usize, dst_ch: usize, out: &mut Vec<f32>) {
    if src_ch == dst_ch {
        out.extend_from_slice(src);
    } else if dst_ch == 1 {
        out.extend(src.chunks(src_ch).map(|f| f.iter().sum::<f32>() / src_ch as f32));
    } else if src_ch == 1 {
        out.extend(src.iter().flat_map(|s| std::iter::repeat_n(*s, dst_ch)));
    } else {
        for f in src.chunks(src_ch) {
            for c in 0..dst_ch {
                out.push(f.get(c).copied().unwrap_or(0.0));
            }
        }
    }
}

/// Channel count and sample rate conversion to what the output wants.
pub struct Convert {
    out_rate: u32,
    out_ch: usize,
    resampler: Option<(Async<f32>, u32, usize)>,
    pending: Vec<f32>,
    mapped: Vec<f32>,
}

impl Convert {
    pub fn new(out_rate: u32, out_ch: usize) -> Convert {
        Convert { out_rate, out_ch, resampler: None, pending: Vec::new(), mapped: Vec::new() }
    }

    /// Forget what is half done (after a seek).
    pub fn reset(&mut self) {
        self.pending.clear();
        if let Some((r, ..)) = &mut self.resampler {
            r.reset();
        }
    }

    /// Convert `src` (interleaved, `src_ch` channels at `src_rate`); the result is appended to `out`.
    pub fn process(&mut self, src: &[f32], src_rate: u32, src_ch: usize, out: &mut Vec<f32>) -> Result<(), String> {
        self.mapped.clear();
        remap_channels(src, src_ch, self.out_ch, &mut self.mapped);
        if src_rate == self.out_rate {
            out.extend_from_slice(&self.mapped);
            return Ok(());
        }
        if self.resampler.as_ref().is_none_or(|(_, r, c)| *r != src_rate || *c != src_ch) {
            let params = SincInterpolationParameters { sinc_len: 64, f_cutoff: Some(0.95), oversampling_factor: 64, interpolation: SincInterpolationType::Linear, window: WindowFunction::BlackmanHarris2 };
            let r = Async::<f32>::new_sinc(self.out_rate as f64 / src_rate as f64, 1.1, &params, RESAMPLE_CHUNK, self.out_ch, FixedAsync::Input).map_err(|e| e.to_string())?;
            self.resampler = Some((r, src_rate, src_ch));
            self.pending.clear();
        }
        self.pending.extend_from_slice(&self.mapped);
        self.drain(out, false)
    }

    /// The end of the file: what is left is played out (padded with silence to a whole chunk).
    pub fn flush(&mut self, out: &mut Vec<f32>) -> Result<(), String> {
        if self.resampler.is_some() && !self.pending.is_empty() {
            self.drain(out, true)?;
        }
        Ok(())
    }

    fn drain(&mut self, out: &mut Vec<f32>, pad: bool) -> Result<(), String> {
        let ch = self.out_ch;
        let Some((r, ..)) = &mut self.resampler else { return Ok(()) };
        loop {
            let need = r.input_frames_next();
            if self.pending.len() < need * ch {
                if !pad {
                    return Ok(());
                }
                self.pending.resize(need * ch, 0.0);
            }
            let input = InterleavedSlice::new(&self.pending[..need * ch], ch, need).map_err(|e| e.to_string())?;
            let max = r.output_frames_max();
            let start = out.len();
            out.resize(start + max * ch, 0.0);
            let (_, written) = {
                let mut output = InterleavedSlice::new_mut(&mut out[start..], ch, max).map_err(|e| e.to_string())?;
                r.process_into_buffer(&input, &mut output, None).map_err(|e| e.to_string())?
            };
            out.truncate(start + written * ch);
            self.pending.drain(..need * ch);
            if pad {
                return Ok(());
            }
        }
    }
}

/// Push to the queue; waits while it is full. False when the data is out of date (a seek came)
/// or playing was stopped.
pub(super) fn push_all(shared: &Shared, epoch: u64, data: &[f32], stop: &AtomicBool) -> bool {
    loop {
        match shared.push(epoch, data) {
            Pushed::Accepted => return true,
            Pushed::Stale => return false,
            Pushed::Full => {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }
}

/// The decoder thread's loop: play `path` into `shared` until `stop` is set. Seeks come from
/// the player through `shared`; at the end of the file it waits for the next seek.
pub fn run(path: &Path, shared: &Shared, stop: &AtomicBool) -> Result<(), String> {
    let mut src = open(path)?;
    let mut conv = Convert::new(shared.rate, shared.channels);
    let mut epoch = shared.epoch();
    let (mut raw, mut out) = (Vec::new(), Vec::new());
    let mut skip_frames = 0usize;
    let mut eof = false;
    while !stop.load(Ordering::Relaxed) {
        if let Some((e, to)) = shared.poll_seek(epoch) {
            epoch = e;
            eof = false;
            conv.reset();
            skip_frames = match src.seek(to) {
                Ok(early) => (early * shared.rate as f64) as usize,
                // a place behind the end: nothing more to play
                Err(_) => {
                    eof = true;
                    shared.set_eof(epoch);
                    0
                }
            };
            continue;
        }
        if eof {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        out.clear();
        match src.next(&mut raw)? {
            Some((rate, ch)) => conv.process(&raw, rate, ch, &mut out)?,
            None => {
                conv.flush(&mut out)?;
                eof = true;
            }
        }
        if skip_frames > 0 {
            let cut = (skip_frames * shared.channels).min(out.len());
            out.drain(..cut);
            skip_frames -= cut / shared.channels;
        }
        if !out.is_empty() && !push_all(shared, epoch, &out, stop) {
            continue;
        }
        if eof {
            shared.set_eof(epoch);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: MEDIA-002
    #[test]
    fn channels_are_mapped_to_the_outputs() {
        let mut o = Vec::new();
        remap_channels(&[0.1, 0.2], 1, 2, &mut o);
        assert_eq!(o, [0.1, 0.1, 0.2, 0.2], "mono is played on both sides");
        o.clear();
        remap_channels(&[0.2, 0.4, 0.6, 0.0], 2, 1, &mut o);
        assert_eq!(o, [0.3, 0.3], "stereo to mono is the mean");
        o.clear();
        remap_channels(&[1.0, 2.0], 2, 2, &mut o);
        assert_eq!(o, [1.0, 2.0]);
        o.clear();
        remap_channels(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 6, 2, &mut o);
        assert_eq!(o, [1.0, 2.0], "more channels than the output: the first ones");
        o.clear();
        remap_channels(&[1.0, 2.0], 2, 4, &mut o);
        assert_eq!(o, [1.0, 2.0, 0.0, 0.0], "extra output channels stay silent");
    }

    // spec: MEDIA-002
    #[test]
    fn resampling_changes_the_length_by_the_ratio() {
        let mut c = Convert::new(48_000, 2);
        let src: Vec<f32> = (0..44_100).flat_map(|i| [(i as f32 * 0.05).sin() * 0.5; 2]).collect();
        let mut out = Vec::new();
        for chunk in src.chunks(2 * 1000) {
            c.process(chunk, 44_100, 2, &mut out).unwrap();
        }
        c.flush(&mut out).unwrap();
        let frames = out.len() / 2;
        assert!((47_500..49_500).contains(&frames), "one second stays about one second: {frames}");
        assert!(out.iter().all(|x| x.abs() < 0.6));
        // the same rate is passed on as it is
        let mut same = Convert::new(44_100, 2);
        let mut o2 = Vec::new();
        same.process(&src[..200], 44_100, 2, &mut o2).unwrap();
        assert_eq!(o2, &src[..200]);
    }

    /// Play a file into a mixer without a sound card; (seconds played, loudest sample, error).
    fn play_to_the_end(path: &Path, out_rate: u32, channels: usize) -> (f64, f32, Option<String>) {
        use super::super::engine::Mixer;
        let shared = Shared::new(out_rate, channels);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let thread = {
            let (shared, stop, path) = (shared.clone(), stop.clone(), path.to_path_buf());
            std::thread::spawn(move || run(&path, &shared, &stop))
        };
        let mut mixer = Mixer::new(shared.clone());
        shared.set_playing(true);
        let mut peak = 0.0f32;
        let mut buf = vec![0.0; 1024 * channels];
        let start = std::time::Instant::now();
        while !shared.is_finished() && !thread.is_finished() && start.elapsed() < Duration::from_secs(20) {
            mixer.render(&mut buf);
            peak = buf.iter().fold(peak, |p, x| p.max(x.abs()));
            std::thread::sleep(Duration::from_millis(1));
        }
        let secs = shared.position_secs();
        stop.store(true, Ordering::Relaxed);
        let result = thread.join().expect("the decoder thread does not panic").err();
        (secs, peak, result)
    }

    // spec: MEDIA-002
    // spec: MEDIA-011
    #[test]
    fn every_format_decodes_to_a_second_of_tone_at_any_output_rate() {
        use crate::media::testdata::{fixture, FORMATS};
        for f in FORMATS.iter().filter(|f| f.playable) {
            let path = fixture(&format!("tone.{}", f.ext));
            let src = open(&path).unwrap_or_else(|e| panic!("{}: {e}", f.ext));
            let d = src.duration.unwrap_or_else(|| panic!("{} has a duration", f.ext));
            assert!((0.9..1.2).contains(&d), "{}: duration {d}", f.ext);
            for (rate, ch) in [(44_100, 2), (48_000, 2), (48_000, 1)] {
                let (secs, peak, err) = play_to_the_end(&path, rate, ch);
                assert_eq!(err, None, "{} at {rate}", f.ext);
                assert!((0.9..1.25).contains(&secs), "{} at {rate}/{ch}: played {secs} s", f.ext);
                assert!(peak > 0.08 && peak <= 1.0, "{} at {rate}/{ch}: loudest {peak} (the tone is at 0.125)", f.ext);
            }
        }
    }

    // spec: MEDIA-002
    #[test]
    fn opus_is_recognised_but_reported_as_not_playable() {
        // without ffmpeg (MEDIA-026)
        let e = open_with(&crate::media::testdata::fixture("tone.opus"), no_tools).err().expect("symphonia has no Opus decoder");
        assert!(e.contains("can't be played"), "{e}");
    }

    fn no_tools() -> Option<Tools> {
        None
    }

    fn tools_or_skip() -> Option<Tools> {
        let t = ffmpeg::locate();
        if t.is_none() {
            println!("skipping: ffmpeg and ffprobe are not installed");
        }
        t
    }

    // spec: MEDIA-025
    #[test]
    fn media_025_phone_recordings_with_a_16_or_18_byte_fmt_chunk_play_without_ffmpeg() {
        use crate::media::testdata::{temp_file, wav_bytes};
        for (tag, len, name) in [(6, 16, "fritz-alaw16.wav"), (6, 18, "alaw18.wav"), (7, 16, "mulaw16.wav"), (7, 18, "mulaw18.wav")] {
            let path = temp_file(name, &wav_bytes(tag, len, 1.0));
            let src = open_with(&path, no_tools).unwrap_or_else(|e| panic!("{name}: {e}"));
            let d = src.duration.unwrap_or_else(|| panic!("{name} has a duration"));
            assert!((0.9..1.1).contains(&d), "{name}: duration {d}");
            drop(src);
            let (secs, peak, err) = play_to_the_end(&path, 48_000, 2);
            assert_eq!(err, None, "{name}");
            assert!((0.9..1.25).contains(&secs), "{name}: played {secs} s");
            assert!((0.15..0.3).contains(&peak), "{name}: loudest {peak} (the tone is at 0.25)");
            let _ = std::fs::remove_dir_all(path.parent().unwrap());
        }
    }

    // spec: MEDIA-026
    #[test]
    fn media_026_ffmpeg_decodes_a_file_symphonia_refuses_and_a_seek_restarts_it() {
        let Some(_) = tools_or_skip() else { return };
        // Opus: symphonia has no decoder for it
        let path = crate::media::testdata::fixture("tone.opus");
        assert!(open_with(&path, no_tools).is_err());
        let mut src = open_with(&path, ffmpeg::locate).expect("ffmpeg reads it");
        let d = src.duration.expect("ffprobe gives the length");
        assert!((0.9..1.2).contains(&d), "duration {d}");
        let (secs, peak, err) = play_to_the_end(&path, 44_100, 2);
        assert_eq!(err, None);
        assert!((0.9..1.25).contains(&secs), "played {secs} s");
        assert!(peak > 0.08 && peak <= 1.0, "loudest {peak}");
        // the sound again from the middle: about half of it is left
        assert_eq!(src.seek(0.5).unwrap(), 0.0);
        let (mut rest, mut buf) = (0usize, Vec::new());
        while let Some((rate, ch)) = src.next(&mut buf).unwrap() {
            assert_eq!((rate, ch), (FALLBACK_RATE, FALLBACK_CHANNELS));
            rest += buf.len() / ch;
        }
        let left = rest as f64 / FALLBACK_RATE as f64;
        assert!((0.4..0.6).contains(&left), "{left} s left after the seek");
    }

    // spec: MEDIA-026
    #[test]
    fn media_026_a_file_ffmpeg_cannot_read_either_keeps_the_words_and_a_missing_ffmpeg_changes_nothing() {
        let bad = crate::media::testdata::temp_file("noise.mp3", &[7u8; 4000]);
        let without = open_with(&bad, no_tools).err().expect("not playable");
        if tools_or_skip().is_some() {
            let with = open_with(&bad, ffmpeg::locate).err().expect("ffmpeg cannot read noise either");
            assert_eq!(with, without, "the words are the same");
        }
        // a file that cannot be opened is not given to ffmpeg
        let missing = bad.with_file_name("missing.mp3");
        assert!(open_with(&missing, ffmpeg::locate).err().unwrap().contains("can't be opened"));
        let _ = std::fs::remove_dir_all(bad.parent().unwrap());
    }

    // spec: MEDIA-027
    #[test]
    fn media_027_a_wav_file_that_cannot_be_played_is_named_by_what_it_holds_and_never_called_damaged() {
        use crate::media::testdata::{temp_file, wav_bytes};
        // A-law with a chunk of 20 bytes: neither the short nor the extended form
        let alaw = temp_file("alaw20.wav", &wav_bytes(6, 20, 0.1));
        let e = open_with(&alaw, no_tools).err().expect("symphonia refuses it");
        assert_eq!(e, "WAV with A-law sound in a form Shale can't read yet.");
        let adpcm = temp_file("adpcm.wav", &wav_bytes(0x11, 20, 0.1));
        let e = open_with(&adpcm, no_tools).err().expect("symphonia is built without ADPCM");
        assert_eq!(e, "WAV with ADPCM sound, which Shale can't play yet.");
        let other = temp_file("mp3-in-wav.wav", &wav_bytes(0x55, 16, 0.1));
        let e = open_with(&other, no_tools).err().expect("not a sound symphonia reads in a WAV file");
        assert_eq!(e, "WAV with sound of a kind Shale can't play yet (format 0x0055).");
        for p in [alaw, adpcm, other] {
            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
    }

    // spec: MEDIA-027
    #[test]
    fn media_027_the_words_for_the_other_kinds_of_failure() {
        use crate::media::testdata::temp_file;
        let io = std::io::Error::other("disk gone");
        assert_eq!(describe(&Error::IoError(io)), "The file can't be read: disk gone");
        assert!(describe(&Error::Unsupported("x")).contains("can't be played"));
        let words = describe(&Error::DecodeError("x"));
        assert!(words.contains("may be damaged") && !words.contains("is damaged"), "a file that may be fine is not called damaged: {words}");
        // the tag of a WAV file decides, the kind of error only for the plain cases
        let pcm = temp_file("pcm.wav", &crate::media::testdata::wav_bytes(1, 16, 0.01));
        assert_eq!(describe_open(&pcm, &Error::DecodeError("x")), "WAV with PCM sound in a form Shale can't read yet.");
        assert_eq!(describe_open(&pcm, &Error::IoError(std::io::Error::other("gone"))), "The file can't be read: gone");
        assert_eq!(describe_open(&pcm.with_file_name("missing.wav"), &Error::DecodeError("x")), words);
        for (tag, kind) in [(3u16, "floating point"), (7, "µ-law"), (0xFFFE, "multichannel")] {
            let p = temp_file(&format!("t{tag}.wav"), &crate::media::testdata::wav_bytes(tag, 16, 0.01));
            assert_eq!(describe_open(&p, &Error::DecodeError("x")), format!("WAV with {kind} sound in a form Shale can't read yet."));
            let _ = std::fs::remove_dir_all(p.parent().unwrap());
        }
        let _ = std::fs::remove_dir_all(pcm.parent().unwrap());
    }

    // spec: MEDIA-005
    #[test]
    fn seeking_continues_from_the_new_place() {
        use super::super::engine::Mixer;
        let shared = Shared::new(48_000, 2);
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let path = crate::media::testdata::fixture("tone.flac");
        let thread = {
            let (shared, stop) = (shared.clone(), stop.clone());
            std::thread::spawn(move || run(&path, &shared, &stop))
        };
        let mut mixer = Mixer::new(shared.clone());
        shared.set_playing(true);
        let mut buf = vec![0.0; 2048];
        let start = std::time::Instant::now();
        while shared.position_secs() < 0.1 && start.elapsed() < Duration::from_secs(10) {
            mixer.render(&mut buf);
            std::thread::sleep(Duration::from_millis(1));
        }
        shared.seek(0.7);
        while !shared.is_finished() && start.elapsed() < Duration::from_secs(10) {
            mixer.render(&mut buf);
            std::thread::sleep(Duration::from_millis(1));
        }
        let end = shared.position_secs();
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap().unwrap();
        assert!((0.95..1.1).contains(&end), "after the seek only the last 0.3 s remain, so the end is the length: {end}");
    }

    // spec: MEDIA-002
    #[test]
    fn a_file_that_is_not_a_sound_is_reported_in_words() {
        let dir = std::env::temp_dir().join(format!("shale-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bad = dir.join("noise.mp3");
        std::fs::write(&bad, vec![7u8; 4000]).unwrap();
        let e = open(&bad).err().expect("not playable");
        assert!(e.contains("damaged") || e.contains("can't"), "{e}");
        let e = open(&dir.join("missing.mp3")).err().expect("missing");
        assert!(e.contains("can't be opened"), "{e}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
