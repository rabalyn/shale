//! Reading an audio file (MEDIA-002): `symphonia` opens and decodes it, the samples are turned
//! into the output's channel count and rate (`rubato` when the rates differ) and pushed into the
//! engine's queue. `probe` is the quick look the player does first (length, cover, whether the
//! file can be played at all); `run` is the decoder thread's loop.

use super::engine::{Pushed, Shared};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType, WindowFunction};
use std::path::Path;
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

/// An opened audio file: the container reader, the decoder of its sound track and what the tags say.
pub struct Source {
    format: Box<dyn FormatReader + 'static>,
    decoder: Box<dyn AudioDecoder>,
    track_id: u32,
    /// Length in seconds, when the file says.
    pub duration: Option<f64>,
    /// Pictures from the tags (cover art).
    pub visuals: Vec<Visual>,
}

fn describe(e: &Error) -> String {
    match e {
        Error::Unsupported(_) => "This kind of sound can't be played by Shale yet.".into(),
        Error::IoError(e) => format!("The file can't be read: {e}"),
        _ => "The file is damaged or not a sound file.".into(),
    }
}

/// Open a file for playing. The error is a sentence for the player.
pub fn open(path: &Path) -> Result<Source, String> {
    let file = crate::fsutil::open_read(path).map_err(|e| format!("The file can't be opened: {e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(&ext.to_ascii_lowercase());
    }
    let mut format = symphonia::default::get_probe().probe(&hint, mss, FormatOptions::default(), MetadataOptions::default()).map_err(|e| describe(&e))?;
    let (track_id, params) = {
        let track = format.default_track(TrackType::Audio).ok_or("There is no sound in this file.")?;
        let params = track.codec_params.as_ref().and_then(|p| p.audio()).ok_or("There is no sound in this file.")?.clone();
        (track.id, params)
    };
    let decoder = symphonia::default::get_codecs().make_audio_decoder(&params, &AudioDecoderOptions::default()).map_err(|e| match e {
        Error::Unsupported(_) => "This kind of sound can't be played by Shale yet (the sound format is not supported).".to_string(),
        other => describe(&other),
    })?;
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
    Ok(Source { format, decoder, track_id, duration, visuals })
}

impl Source {
    /// Seek to `secs`; the result is how many seconds before that the reader landed (to skip).
    fn seek(&mut self, secs: f64) -> Result<f64, Error> {
        let time = Time::try_from_secs_f64(secs).ok_or(Error::Unsupported("seek time"))?;
        let to = self.format.seek(SeekMode::Accurate, SeekTo::Time { time, track_id: Some(self.track_id) })?;
        self.decoder.reset();
        let tb = self.format.tracks().iter().find(|t| t.id == self.track_id).and_then(|t| t.time_base);
        Ok(tb.and_then(|tb| Some(tb.calc_time(to.required_ts)?.as_secs_f64() - tb.calc_time(to.actual_ts)?.as_secs_f64())).unwrap_or(0.0).max(0.0))
    }

    /// The next decoded sound as interleaved f32 in `buf`, with its rate and channel count;
    /// None at the end of the file.
    pub(crate) fn next(&mut self, buf: &mut Vec<f32>) -> Result<Option<(u32, usize)>, String> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(Some(p)) => p,
                Ok(None) => return Ok(None),
                Err(Error::ResetRequired) => return Ok(None),
                Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(e) => return Err(describe(&e)),
            };
            if packet.track_id != self.track_id {
                continue;
            }
            match self.decoder.decode(&packet) {
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
        let e = open(&crate::media::testdata::fixture("tone.opus")).err().expect("symphonia has no Opus decoder");
        assert!(e.contains("can't be played"), "{e}");
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
