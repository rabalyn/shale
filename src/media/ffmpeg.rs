//! The system `ffmpeg` and `ffprobe` (MEDIA-009, MEDIA-010): where they are, the argument lists
//! for what the video player asks of them, and the child process that is killed and reaped when
//! it is let go. They are run like git (SYNC-001): an argument list, no shell, stdin closed,
//! stderr captured. Shale links no video library (ADR-0015), so without these programs a video
//! is opened by the system's player instead.

use eframe::egui;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};

/// The widest picture the player asks ffmpeg for.
pub const MAX_WIDTH: u32 = 1280;
/// The tallest picture the player asks ffmpeg for.
pub const MAX_HEIGHT: u32 = 720;
/// Frames per second the player shows at most.
pub const MAX_FPS: f64 = 30.0;
/// How much of ffmpeg's last words are kept for messages.
const TAIL_BYTES: usize = 2000;

/// What the user is told when the programs are missing.
pub const MISSING_HELP: &str = "ffmpeg is not installed, so videos open in your system's video player. Install it (Linux: your package manager, `apt install ffmpeg`; macOS: `brew install ffmpeg`; Windows: `winget install ffmpeg`; or https://ffmpeg.org/download.html) and open the note again.";

/// The two programs, as paths to run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

/// What `ffprobe` says about a video file.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// Length in seconds, when the file says.
    pub duration: Option<f64>,
    /// The size as it is shown (turned by the rotation the file asks for).
    pub width: u32,
    pub height: u32,
    /// Frames per second of the picture.
    pub fps: f64,
    pub has_audio: bool,
}

impl Info {
    /// The frame rate the player asks for: the file's, but not above `MAX_FPS`.
    pub fn play_fps(&self) -> f64 {
        self.fps.clamp(1.0, MAX_FPS)
    }

    /// The size of the pictures ffmpeg is asked for.
    pub fn picture_size(&self) -> (u32, u32) {
        fit_size(self.width, self.height)
    }
}

/// `w` x `h` scaled down to fit `MAX_WIDTH` x `MAX_HEIGHT`, never up, and made even (the
/// pixel formats want it).
pub fn fit_size(w: u32, h: u32) -> (u32, u32) {
    let (w, h) = (w.max(2) as f64, h.max(2) as f64);
    let k = (MAX_WIDTH as f64 / w).min(MAX_HEIGHT as f64 / h).min(1.0);
    let even = |v: f64| (((v * k).round() as u32) & !1).max(2);
    (even(w), even(h))
}

/// `ffmpeg` and `ffprobe` of this machine, if both are there.
pub fn locate() -> Option<Tools> {
    locate_in(&crate::program::search_dirs(&[]))
}

pub fn locate_in(dirs: &[PathBuf]) -> Option<Tools> {
    Some(Tools { ffmpeg: crate::program::find_in("ffmpeg", dirs)?, ffprobe: crate::program::find_in("ffprobe", dirs)? })
}

fn os(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

/// The words every ffmpeg run starts with: quiet, no questions, no keyboard.
fn base() -> Vec<OsString> {
    os(&["-hide_banner", "-loglevel", "error", "-nostdin"])
}

/// `ffprobe` asked for the streams' kind, size, rotation and frame rate and the file's length, as JSON.
pub fn probe_args(file: &Path) -> Vec<OsString> {
    probe_args_with(file, PROBE_ENTRIES)
}

const PROBE_ENTRIES: &str = "stream=codec_type,width,height,avg_frame_rate,r_frame_rate:stream_side_data=rotation:stream_tags=rotate:format=duration";
/// For an ffprobe that does not know the section `stream_side_data` (4.x, as on Ubuntu 22.04) and
/// refuses the whole list: those versions give the rotation as the stream tag `rotate` (#299).
const PROBE_ENTRIES_OLD: &str = "stream=codec_type,width,height,avg_frame_rate,r_frame_rate:stream_tags=rotate:format=duration";

/// `ffprobe` asked whether a file has a sound and how long it is, as JSON (MEDIA-026).
pub fn audio_probe_args(file: &Path) -> Vec<OsString> {
    let mut a = os(&["-v", "error", "-show_entries", "stream=codec_type:format=duration", "-of", "json", "-i"]);
    a.push(file.into());
    a
}

/// Read what `ffprobe` printed (see `audio_probe_args`): the length when it says, an error when
/// the file has no sound.
pub fn parse_audio_probe(json: &str) -> Result<Option<f64>, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "ffprobe did not understand the file".to_string())?;
    if !v["streams"].as_array().is_some_and(|s| s.iter().any(|s| s["codec_type"].as_str() == Some("audio"))) {
        return Err("There is no sound in this file.".into());
    }
    Ok(v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).filter(|d| d.is_finite() && *d > 0.0))
}

fn probe_args_with(file: &Path, entries: &str) -> Vec<OsString> {
    let mut a = os(&["-v", "error", "-show_entries", entries, "-of", "json", "-i"]);
    a.push(file.into());
    a
}

/// The first picture of the file as a PNG on stdout, scaled to `w` x `h`.
pub fn thumbnail_args(file: &Path, w: u32, h: u32) -> Vec<OsString> {
    let mut a = base();
    a.extend(os(&["-ss", "0", "-i"]));
    a.push(file.into());
    a.extend(os(&["-an", "-sn", "-dn", "-frames:v", "1", "-vf"]));
    a.push(format!("scale={w}:{h}").into());
    a.extend(os(&["-f", "image2pipe", "-c:v", "png", "-"]));
    a
}

/// The pictures from `start` seconds on, as raw RGBA frames of `w` x `h` at a steady `fps` on stdout.
pub fn video_args(file: &Path, start: f64, w: u32, h: u32, fps: f64) -> Vec<OsString> {
    let mut a = base();
    a.push("-ss".into());
    a.push(format!("{:.3}", start.max(0.0)).into());
    a.push("-i".into());
    a.push(file.into());
    a.extend(os(&["-an", "-sn", "-dn", "-vf"]));
    a.push(format!("fps={fps:.3},scale={w}:{h}").into());
    a.extend(os(&["-pix_fmt", "rgba", "-f", "rawvideo", "-"]));
    a
}

/// The sound from `start` seconds on, as f32 little-endian samples in `channels` and at `rate` on stdout.
pub fn audio_args(file: &Path, start: f64, rate: u32, channels: usize) -> Vec<OsString> {
    let mut a = base();
    a.push("-ss".into());
    a.push(format!("{:.3}", start.max(0.0)).into());
    a.push("-i".into());
    a.push(file.into());
    a.extend(os(&["-vn", "-sn", "-dn", "-ac"]));
    a.push(channels.to_string().into());
    a.push("-ar".into());
    a.push(rate.to_string().into());
    a.extend(os(&["-f", "f32le", "-acodec", "pcm_f32le", "-"]));
    a
}

/// `30000/1001` as a number; None for `0/0` and nonsense.
fn fraction(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/').unwrap_or((s, "1"));
    let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
    (d != 0.0 && n > 0.0 && (n / d).is_finite()).then_some(n / d)
}

/// Read what `ffprobe` printed (see `probe_args`).
pub fn parse_probe(json: &str) -> Result<Info, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| "ffprobe did not understand the file".to_string())?;
    let streams = v["streams"].as_array().cloned().unwrap_or_default();
    let kind = |s: &serde_json::Value| s["codec_type"].as_str().unwrap_or("").to_string();
    let video = streams.iter().find(|s| kind(s) == "video" && s["width"].as_u64().unwrap_or(0) > 0).ok_or("There is no picture in this file.")?;
    let (mut width, mut height) = (video["width"].as_u64().unwrap_or(0) as u32, video["height"].as_u64().unwrap_or(0) as u32);
    let rotation = video["side_data_list"].as_array().and_then(|l| l.iter().find_map(|d| d["rotation"].as_f64())).or_else(|| video["tags"]["rotate"].as_str().and_then(|r| r.trim().parse::<f64>().ok())).unwrap_or(0.0);
    if (rotation.rem_euclid(180.0) - 90.0).abs() < 1.0 {
        std::mem::swap(&mut width, &mut height);
    }
    let fps = video["avg_frame_rate"].as_str().and_then(fraction).or_else(|| video["r_frame_rate"].as_str().and_then(fraction)).filter(|f| *f <= 240.0).unwrap_or(30.0);
    let duration = v["format"]["duration"].as_str().and_then(|d| d.parse::<f64>().ok()).filter(|d| d.is_finite() && *d > 0.0);
    Ok(Info { duration, width, height, fps, has_audio: streams.iter().any(|s| kind(s) == "audio") })
}

impl Tools {
    fn command(program: &Path, args: &[OsString]) -> Command {
        let mut c = crate::program::command(program);
        c.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        c
    }

    /// Run to the end and return stdout; the error is a sentence with ffmpeg's last words.
    fn capture(program: &Path, args: &[OsString]) -> Result<Vec<u8>, String> {
        let mut cmd = Self::command(program, args);
        let started = crate::program::spawn(&mut cmd).map_err(|e| format!("{} could not be started: {e}", program.display()))?;
        let o = started.wait_with_output().map_err(|e| format!("{} could not be read: {e}", program.display()))?;
        if o.status.success() {
            Ok(o.stdout)
        } else {
            Err(last_words(&String::from_utf8_lossy(&o.stderr)))
        }
    }

    /// Length, size, frame rate and sound of a video file.
    pub fn probe(&self, file: &Path) -> Result<Info, String> {
        let out = match Self::capture(&self.ffprobe, &probe_args(file)) {
            // an older ffprobe refuses the entry list itself, before it looks at the file (#299)
            Err(e) if e.contains("'show_entries'") => Self::capture(&self.ffprobe, &probe_args_with(file, PROBE_ENTRIES_OLD)),
            r => r,
        }
        .map_err(|e| if e.contains("could not be started") { e } else { "The file is damaged or not a video that ffmpeg can read.".to_string() })?;
        parse_probe(&String::from_utf8_lossy(&out))
    }

    /// The length of a sound file that ffmpeg can read (None when it does not say); an error when
    /// ffprobe cannot read the file or it has no sound (MEDIA-026).
    pub fn audio_length(&self, file: &Path) -> Result<Option<f64>, String> {
        parse_audio_probe(&String::from_utf8_lossy(&Self::capture(&self.ffprobe, &audio_probe_args(file))?))
    }

    /// The first frame of the file, at the size the player will show.
    pub fn thumbnail(&self, file: &Path, info: &Info) -> Result<egui::ColorImage, String> {
        let (w, h) = info.picture_size();
        let png = Self::capture(&self.ffmpeg, &thumbnail_args(file, w, h))?;
        super::cover::decode(&png).ok_or_else(|| "The first picture of the video can't be read.".to_string())
    }

    /// Start ffmpeg on the frames from `start` seconds.
    pub fn frames(&self, file: &Path, start: f64, info: &Info) -> Result<Proc, String> {
        let (w, h) = info.picture_size();
        Proc::spawn(Self::command(&self.ffmpeg, &video_args(file, start, w, h, info.play_fps())))
    }

    /// Start ffmpeg on the sound from `start` seconds, in the output's rate and channel count.
    pub fn sound(&self, file: &Path, start: f64, rate: u32, channels: usize) -> Result<Proc, String> {
        Proc::spawn(Self::command(&self.ffmpeg, &audio_args(file, start, rate, channels)))
    }
}

/// The last line ffmpeg said, as a sentence for the player.
fn last_words(stderr: &str) -> String {
    match stderr.lines().rev().find(|l| !l.trim().is_empty()) {
        Some(l) => format!("ffmpeg says: {}", l.trim()),
        None => "ffmpeg could not read the file.".into(),
    }
}

/// A running ffmpeg. Dropping it kills the process and waits for it, so none is left behind.
pub struct Proc {
    child: Child,
    stdout: ChildStdout,
    tail: Arc<Mutex<String>>,
}

impl Proc {
    fn spawn(mut cmd: Command) -> Result<Proc, String> {
        let program = cmd.get_program().to_string_lossy().into_owned();
        let mut child = crate::program::spawn(&mut cmd).map_err(|e| format!("{program} could not be started: {e}"))?;
        let stdout = child.stdout.take().ok_or("ffmpeg has no output")?;
        let tail = Arc::new(Mutex::new(String::new()));
        if let Some(mut err) = child.stderr.take() {
            // read all the time, so ffmpeg never blocks on a full pipe; only the end is kept
            let keep = tail.clone();
            std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                while let Ok(n) = err.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = keep.lock().unwrap_or_else(|e| e.into_inner());
                    t.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if t.len() > TAIL_BYTES {
                        let mut cut = t.len() - TAIL_BYTES;
                        while !t.is_char_boundary(cut) {
                            cut += 1;
                        }
                        t.drain(..cut);
                    }
                }
            });
        }
        Ok(Proc { child, stdout, tail })
    }

    pub fn stdout(&mut self) -> &mut ChildStdout {
        &mut self.stdout
    }

    #[cfg(all(test, unix))]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// What ffmpeg said last, as a sentence (after its output ended without any data).
    pub fn last_words(&self) -> String {
        // the process is ending: give the reader of its stderr a moment to catch up
        std::thread::sleep(std::time::Duration::from_millis(30));
        last_words(&self.tail.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(a: &[OsString]) -> Vec<String> {
        a.iter().map(|s| s.to_string_lossy().into_owned()).collect()
    }

    // spec: MEDIA-009
    // ffprobe 4.4 (Ubuntu 22.04) refuses the section stream_side_data with these words and exit 1;
    // the probe asks again without it and reads the video (#299)
    #[cfg(unix)]
    #[test]
    fn an_ffprobe_that_does_not_know_the_side_data_section_still_reads_the_video() {
        let Some(real) = locate() else {
            println!("skipping: ffmpeg and ffprobe are not installed");
            return;
        };
        let dir = std::env::temp_dir().join(format!("shale-old-ffprobe-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let old = dir.join("ffprobe");
        let script = format!(
            "#!/bin/sh\ncase \"$*\" in *stream_side_data*) echo \"No match for section 'stream_side_data'\" >&2; echo \"Failed to set value '...' for option 'show_entries': Invalid argument\" >&2; exit 1;; esac\nexec '{}' \"$@\"\n",
            real.ffprobe.display()
        );
        crate::test_exec::write_script(&old, &script);
        let tools = Tools { ffmpeg: real.ffmpeg.clone(), ffprobe: old };
        let clip = super::super::testdata::fixture("clip.mp4");
        let info = tools.probe(&clip).expect("read with the older entry list");
        assert_eq!(info, real.probe(&clip).unwrap(), "the same as with a newer ffprobe");
        // a missing file is still named as unreadable, not as a refused entry list
        assert_eq!(tools.probe(Path::new("/does/not/exist.mp4")).unwrap_err(), "The file is damaged or not a video that ffmpeg can read.");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MEDIA-026
    #[test]
    fn media_026_the_sound_probe_gives_the_length_and_refuses_a_file_without_sound() {
        let a = strs(&audio_probe_args(Path::new("x y.wav")));
        assert_eq!(a[a.len() - 2..], ["-i", "x y.wav"], "the file is one argument after -i");
        assert_eq!(parse_audio_probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"24.8"}}"#), Ok(Some(24.8)));
        assert_eq!(parse_audio_probe(r#"{"streams":[{"codec_type":"audio"}],"format":{}}"#), Ok(None), "no length is not an error");
        assert_eq!(parse_audio_probe(r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"0.0"}}"#), Ok(None));
        assert!(parse_audio_probe(r#"{"streams":[{"codec_type":"video"}],"format":{"duration":"3"}}"#).unwrap_err().contains("no sound"));
        assert!(parse_audio_probe("{}").is_err());
        assert!(parse_audio_probe("nonsense").unwrap_err().contains("did not understand"));
    }

    // spec: MEDIA-026
    #[test]
    fn media_026_the_length_of_a_sound_file_comes_from_ffprobe_and_a_file_without_sound_is_refused() {
        let Some(tools) = locate() else {
            println!("skipping: ffmpeg and ffprobe are not installed");
            return;
        };
        let d = tools.audio_length(&crate::media::testdata::fixture("tone.opus")).unwrap().expect("a length");
        assert!((0.9..1.2).contains(&d), "{d}");
        assert!(tools.audio_length(&crate::media::testdata::fixture("clip-silent.mp4")).is_err(), "a video without sound");
        assert!(tools.audio_length(Path::new("/does/not/exist.wav")).is_err());
    }

    // spec: MEDIA-009
    #[test]
    fn the_arguments_are_a_list_with_the_file_after_dash_i_and_never_a_shell_line() {
        let odd = Path::new("/vault/my video; rm -rf $HOME `x`.mp4");
        for args in [probe_args(odd), thumbnail_args(odd, 640, 360), video_args(odd, 12.5, 640, 360, 25.0), audio_args(odd, 12.5, 48_000, 2)] {
            let a = strs(&args);
            let at = a.iter().position(|s| s == "-i").expect("-i");
            assert_eq!(a[at + 1], odd.to_string_lossy(), "the name is one argument, as it is");
            assert_eq!(a.iter().filter(|s| s.contains("rm -rf")).count(), 1, "and only there");
        }
        let v = strs(&video_args(odd, 12.5, 640, 360, 29.97));
        assert!(v.windows(2).any(|w| w == ["-ss", "12.500"]), "{v:?}");
        assert!(v.iter().any(|s| s == "fps=29.970,scale=640:360"), "{v:?}");
        assert!(v.windows(2).any(|w| w == ["-f", "rawvideo"]) && v.contains(&"rgba".to_string()) && v.contains(&"-an".to_string()), "{v:?}");
        let a = strs(&audio_args(odd, 3.0, 44_100, 2));
        assert!(a.windows(2).any(|w| w == ["-ar", "44100"]) && a.windows(2).any(|w| w == ["-ac", "2"]) && a.windows(2).any(|w| w == ["-f", "f32le"]) && a.contains(&"-vn".to_string()), "{a:?}");
        let t = strs(&thumbnail_args(odd, 640, 360));
        assert!(t.windows(2).any(|w| w == ["-frames:v", "1"]) && t.windows(2).any(|w| w == ["-ss", "0"]), "{t:?}");
        assert_eq!(strs(&video_args(odd, -4.0, 2, 2, 25.0))[5], "0.000", "no start before the beginning");
        assert!(strs(&video_args(odd, 0.0, 2, 2, 25.0)).contains(&"-nostdin".to_string()));
    }

    // spec: MEDIA-009
    #[test]
    fn ffprobe_output_gives_length_size_rate_and_sound() {
        let json = r#"{"streams":[{"codec_type":"video","width":640,"height":360,"r_frame_rate":"30000/1001","avg_frame_rate":"30000/1001"},{"codec_type":"audio"}],"format":{"duration":"12.480000"}}"#;
        let i = parse_probe(json).unwrap();
        assert_eq!((i.width, i.height, i.has_audio, i.duration), (640, 360, true, Some(12.48)));
        assert!((i.fps - 29.97).abs() < 0.01);
        let silent = r#"{"streams":[{"codec_type":"video","width":320,"height":240,"avg_frame_rate":"0/0","r_frame_rate":"25/1"}],"format":{"duration":"N/A"}}"#;
        let i = parse_probe(silent).unwrap();
        assert_eq!((i.has_audio, i.duration, i.fps), (false, None, 25.0), "the other rate when the average is 0/0, no length when it says N/A");
        let nonsense = r#"{"streams":[{"codec_type":"video","width":320,"height":240}],"format":{}}"#;
        assert_eq!(parse_probe(nonsense).unwrap().fps, 30.0, "no rate: 30");
    }

    // spec: MEDIA-009
    #[test]
    fn a_phone_video_that_is_turned_is_shown_turned() {
        let side = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]}],"format":{"duration":"2"}}"#;
        let i = parse_probe(side).unwrap();
        assert_eq!((i.width, i.height), (1080, 1920));
        let tag = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","tags":{"rotate":"270"}}],"format":{"duration":"2"}}"#;
        assert_eq!(parse_probe(tag).unwrap().width, 1080);
        let upside_down = r#"{"streams":[{"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30/1","side_data_list":[{"rotation":180}]}],"format":{}}"#;
        assert_eq!(parse_probe(upside_down).unwrap().width, 1920, "180 degrees keeps the size");
    }

    // spec: MEDIA-009
    #[test]
    fn a_file_without_a_picture_or_not_json_is_an_error_with_a_sentence() {
        assert_eq!(parse_probe(r#"{"streams":[{"codec_type":"audio"}],"format":{}}"#).unwrap_err(), "There is no picture in this file.");
        assert!(parse_probe(r#"{}"#).is_err());
        assert!(parse_probe("this is not json").unwrap_err().contains("did not understand"));
    }

    // spec: MEDIA-009
    #[test]
    fn pictures_are_scaled_down_to_the_limits_never_up_and_stay_even() {
        assert_eq!(fit_size(1920, 1080), (1280, 720));
        assert_eq!(fit_size(3840, 2160), (1280, 720));
        assert_eq!(fit_size(640, 360), (640, 360), "small pictures stay as they are");
        assert_eq!(fit_size(1080, 1920), (404, 720), "a tall picture is held by the height");
        assert_eq!(fit_size(641, 361), (640, 360), "odd sizes are made even");
        assert_eq!(fit_size(0, 0), (2, 2));
        let i = Info { duration: None, width: 1920, height: 1080, fps: 60.0, has_audio: true };
        assert_eq!((i.play_fps(), i.picture_size()), (30.0, (1280, 720)), "60 frames a second are shown as 30");
    }

    // spec: MEDIA-010
    #[test]
    fn the_programs_are_found_in_a_folder_or_not_at_all() {
        let dir = std::env::temp_dir().join(format!("shale-ffmpeg-find-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ext = if cfg!(windows) { ".exe" } else { "" };
        assert_eq!(locate_in(std::slice::from_ref(&dir)), None, "an empty folder has neither");
        std::fs::write(dir.join(format!("ffmpeg{ext}")), b"").unwrap();
        assert_eq!(locate_in(std::slice::from_ref(&dir)), None, "ffmpeg without ffprobe is not enough");
        std::fs::write(dir.join(format!("ffprobe{ext}")), b"").unwrap();
        let t = locate_in(&[PathBuf::from("/does/not/exist"), dir.clone()]).unwrap();
        assert_eq!((t.ffmpeg, t.ffprobe), (dir.join(format!("ffmpeg{ext}")), dir.join(format!("ffprobe{ext}"))));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(MISSING_HELP.contains("ffmpeg.org") && MISSING_HELP.contains("brew install ffmpeg"));
    }

    // spec: MEDIA-009
    #[test]
    fn a_program_that_does_not_exist_is_an_error_not_a_panic() {
        let gone = Tools { ffmpeg: "/does/not/exist/ffmpeg".into(), ffprobe: "/does/not/exist/ffprobe".into() };
        assert!(gone.probe(Path::new("x.mp4")).unwrap_err().contains("could not be started"));
        assert!(gone.sound(Path::new("x.mp4"), 0.0, 48_000, 2).err().unwrap().contains("could not be started"));
    }

    #[test]
    fn last_words_are_the_last_line_or_a_plain_sentence() {
        assert_eq!(last_words("one\nMoov atom not found\n\n"), "ffmpeg says: Moov atom not found");
        assert_eq!(last_words(""), "ffmpeg could not read the file.");
    }

    // spec: MEDIA-009
    #[cfg(unix)]
    #[test]
    fn a_process_that_is_let_go_is_killed_and_reaped() {
        // a program that would run for a minute
        let mut c = Command::new("sleep");
        c.arg("60").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let p = Proc::spawn(c).unwrap();
        let pid = p.pid();
        let proc_dir = PathBuf::from(format!("/proc/{pid}"));
        if proc_dir.exists() {
            assert!(proc_dir.join("stat").exists(), "it runs");
        }
        drop(p);
        // reaped: not even a zombie is left
        assert!(!proc_dir.exists(), "the process {pid} is gone");
    }
}
