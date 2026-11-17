//! The sound card (MEDIA-002): opens the default output with `cpal` and runs the `Mixer` of the
//! engine in its callback. The stream must stay on the thread that opened it, so the playback
//! thread of `decode` opens it and keeps it until the file is closed.
//!
//! Nothing here can be tested without a device; the test that opens one accepts "no device" as an
//! answer (CI machines have none), and everything that decides how the sound is heard is in
//! `engine` and tested there.

use super::engine::{Mixer, Shared};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// An open output stream; the sound stops when it is dropped.
pub struct Output {
    /// the cpal stream (or the thread that keeps time when there is no device)
    _keep: Box<dyn std::any::Any>,
    pub shared: Arc<Shared>,
}

/// Open the default output device with the device's own sample rate and channel count.
pub fn open() -> Result<Output, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no sound output")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let shared = Shared::new(config.sample_rate(), config.channels() as usize);
    let format = config.sample_format();
    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, &config.config(), shared.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config.config(), shared.clone()),
        SampleFormat::U16 => build::<u16>(&device, &config.config(), shared.clone()),
        SampleFormat::I32 => build::<i32>(&device, &config.config(), shared.clone()),
        other => Err(format!("the sound output wants {other}, which Shale does not play")),
    }?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(Output { _keep: Box::new(stream), shared })
}

fn build<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: &cpal::StreamConfig, shared: Arc<Shared>) -> Result<cpal::Stream, String> {
    let mut mixer = Mixer::new(shared);
    let mut scratch: Vec<f32> = Vec::new();
    device
        .build_output_stream(
            *config,
            move |data: &mut [T], _| {
                scratch.resize(data.len(), 0.0);
                mixer.render(&mut scratch);
                for (o, s) in data.iter_mut().zip(scratch.iter()) {
                    *o = T::from_sample(*s);
                }
            },
            |_| {},
            None,
        )
        .map_err(|e| e.to_string())
}

/// A sound card that is not there: on each wakeup it renders the frames the wall clock says are
/// due since it started (in chunks of at most 10 ms), then sleeps `tick`; so the sound clock runs
/// at the speed of sound even when the machine's sleeps run long (GitHub's macOS runner, #266).
/// The thread ends when `stop` is set.
pub(crate) fn clock_card(shared: Arc<Shared>, stop: Arc<AtomicBool>, tick: std::time::Duration) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut mixer = Mixer::new(shared.clone());
        let chunk = shared.rate as usize / 100;
        let mut buf = vec![0.0f32; chunk * shared.channels];
        let start = std::time::Instant::now();
        let mut rendered: u64 = 0;
        while !stop.load(Ordering::Relaxed) {
            let due = (start.elapsed().as_secs_f64() * shared.rate as f64) as u64;
            while rendered < due && !stop.load(Ordering::Relaxed) {
                let n = (due - rendered).min(chunk as u64) as usize;
                mixer.render(&mut buf[..n * shared.channels]);
                rendered += n as u64;
            }
            std::thread::sleep(tick);
        }
    })
}

impl Output {
    /// An output that plays into nothing, in real time (MEDIA-013): a video shows its pictures on
    /// a machine without a sound card, and the player tests run without a card and without noise.
    pub fn without_device() -> Output {
        struct Clock(Arc<AtomicBool>);
        impl Drop for Clock {
            fn drop(&mut self) {
                self.0.store(true, Ordering::Relaxed);
            }
        }
        let shared = Shared::new(48_000, 2);
        let stop = Arc::new(AtomicBool::new(false));
        clock_card(shared.clone(), stop.clone(), std::time::Duration::from_millis(10));
        Output { _keep: Box::new(Clock(stop)), shared }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    // spec: MEDIA-011
    #[test]
    fn opening_the_output_without_a_sound_card_is_an_error_not_a_panic() {
        match super::open() {
            Ok(o) => assert!(o.shared.rate > 0 && o.shared.channels > 0),
            Err(e) => assert!(!e.is_empty()),
        }
    }

    // spec: MEDIA-011, MEDIA-013
    #[test]
    fn the_clock_card_keeps_real_time_when_its_sleeps_run_long() {
        let shared = Shared::new(48_000, 2);
        shared.set_playing(true);
        let stop = Arc::new(AtomicBool::new(false));
        // a slow runner: every sleep of the card takes 60 ms
        let card = clock_card(shared.clone(), stop.clone(), Duration::from_millis(60));
        let epoch = shared.epoch();
        let silence = vec![0.0f32; 480 * 2];
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(600) {
            while shared.push(epoch, &silence) == crate::media::engine::Pushed::Accepted {}
            std::thread::sleep(Duration::from_millis(5));
        }
        let (pos, wall) = (shared.position_secs(), start.elapsed().as_secs_f64());
        stop.store(true, Ordering::Relaxed);
        card.join().unwrap();
        assert!(pos >= wall - 0.15, "the clock ran at the speed of sound: {pos} s of {wall} s");
        assert!(pos <= wall + 0.05, "and not ahead of it: {pos} s of {wall} s");
    }

    // spec: MEDIA-013
    #[test]
    fn an_output_without_a_device_runs_its_clock_until_it_is_dropped() {
        let out = Output::without_device();
        out.shared.set_playing(true);
        let epoch = out.shared.epoch();
        let silence = vec![0.0f32; 480 * 2];
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(300) {
            while out.shared.push(epoch, &silence) == crate::media::engine::Pushed::Accepted {}
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(out.shared.position_secs() > 0.1, "the clock runs without a card");
    }
}
