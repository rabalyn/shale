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

    /// A screenshot or recording is being made (the text cursor then stays steady).
    pub(super) fn active(&self) -> bool {
        self.path.is_some()
    }

    pub(super) fn recording(&self) -> bool {
        self.path.as_deref().is_some_and(recorder::is_clip)
    }
}

/// The PNG of a `--screenshot` picture from its `w`×`h` sRGB RGBA pixels: lossless and small, because
/// every version of a guide picture stays in the history (DEV-007): RGB when every pixel is opaque,
/// and the encoder's default compression instead of the image crate's fast default.
pub(super) fn encode_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{ExtendedColorType, ImageEncoder};
    if rgba.len() as u64 != u64::from(w) * u64::from(h) * 4 {
        return Err(format!("{} bytes are not {w}x{h} RGBA pixels", rgba.len()));
    }
    let (pixels4, _) = rgba.as_chunks::<4>();
    let opaque = pixels4.iter().all(|p| p[3] == 255);
    let rgb: Vec<u8>;
    let (pixels, kind) = if opaque {
        rgb = pixels4.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        (&rgb[..], ExtendedColorType::Rgb8)
    } else {
        (rgba, ExtendedColorType::Rgba8)
    };
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Default, FilterType::Adaptive).write_image(pixels, w, h, kind).map_err(|e| e.to_string())?;
    Ok(png)
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

#[cfg(test)]
mod tests {
    use super::encode_png;

    /// A made-up screen: flat panels, a gradient and a few lines of "text" (sharp, many colours).
    fn screen(w: u32, h: u32, alpha: impl Fn(u32, u32) -> u8) -> Vec<u8> {
        let mut px = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let text = y % 16 < 10 && (x * 7 + y * 3) % 11 < 4 && x > w / 4;
                let rgb = if text {
                    [20, 20, 30]
                } else if x < w / 4 {
                    [40, 44, 52]
                } else {
                    [250, 250, (200 + y % 50) as u8]
                };
                px.extend_from_slice(&[rgb[0], rgb[1], rgb[2], alpha(x, y)]);
            }
        }
        px
    }

    fn fast_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_raw(w, h, rgba.to_vec()).unwrap().write_to(&mut png, image::ImageFormat::Png).unwrap();
        png.into_inner()
    }

    // spec: DEV-007
    #[test]
    fn dev_007_an_opaque_screenshot_is_a_small_lossless_rgb_png() {
        let (w, h) = (320, 200);
        let rgba = screen(w, h, |_, _| 255);
        let png = encode_png(w, h, &rgba).unwrap();
        let back = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgb8, "an opaque picture needs no alpha channel");
        assert_eq!(back.to_rgba8().into_raw(), rgba, "the pixels come back unchanged");
        let fast = fast_png(w, h, &rgba);
        assert!(png.len() * 10 < fast.len() * 9, "at least 10% smaller than the fast RGBA encoding: {} vs {}", png.len(), fast.len());
        assert_eq!(encode_png(w, h, &rgba).unwrap(), png, "two runs give the same bytes");
    }

    // spec: DEV-007
    #[test]
    fn dev_007_a_translucent_screenshot_keeps_its_alpha() {
        let (w, h) = (64, 40);
        let rgba = screen(w, h, |x, _| if x == 3 { 128 } else { 255 });
        let back = image::load_from_memory(&encode_png(w, h, &rgba).unwrap()).unwrap();
        assert_eq!(back.color(), image::ColorType::Rgba8);
        assert_eq!(back.to_rgba8().into_raw(), rgba);
        assert!(encode_png(w, h, &rgba[4..]).is_err(), "too few pixels for the size is an error, not a broken file");
    }
}
