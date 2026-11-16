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
use std::sync::Arc;

/// An open output stream; the sound stops when it is dropped.
pub struct Output {
    /// the cpal stream (or, in tests, the thread that keeps time)
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

/// Tests: an output that plays into nothing, in real time, so that a player can be driven
/// without a sound card (and without noise on the machine that runs the tests).
#[cfg(test)]
impl Output {
    pub fn silent() -> Output {
        struct Clock(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Clock {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
        let shared = Shared::new(48_000, 2);
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (sh, st) = (shared.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut mixer = Mixer::new(sh);
            let mut buf = vec![0.0; 480 * 2];
            while !st.load(std::sync::atomic::Ordering::Relaxed) {
                mixer.render(&mut buf);
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        Output { _keep: Box::new(Clock(stop)), shared }
    }
}

#[cfg(test)]
mod tests {
    // spec: MEDIA-011
    #[test]
    fn opening_the_output_without_a_sound_card_is_an_error_not_a_panic() {
        match super::open() {
            Ok(o) => assert!(o.shared.rate > 0 && o.shared.channels > 0),
            Err(e) => assert!(!e.is_empty()),
        }
    }
}
