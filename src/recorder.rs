//! Recording mode (`--record <file.webm>`, DEV-019): the frames of a `--keys` run go, as raw RGBA, to the
//! system `ffmpeg`, which encodes a small VP9 clip. One drawn frame is one frame of the clip at a fixed
//! frame rate, so the length of a clip does not depend on how fast the machine renders.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::thread::JoinHandle;

/// The frame rate of every clip: one app frame is 1/30 s of clip time.
pub const FPS: u32 = 30;

/// The program that encodes (BUILD-018: reported in words when it is missing).
pub const FFMPEG: &str = "ffmpeg";

/// Whether `path` is a clip (`--record`) and not a picture: the one mark of the two modes.
pub fn is_clip(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("webm"))
}

/// What is said when `program` cannot be started.
pub fn missing_message(program: &str) -> String {
    format!("{program} is not installed (or not on the PATH), and --record needs it to encode the video; install it with your package manager (apt install ffmpeg, dnf install ffmpeg, pacman -S ffmpeg, brew install ffmpeg, winget install ffmpeg)")
}

/// Whether `program` can be started; `Err` is the message in words (BUILD-018).
pub fn probe(program: &str) -> Result<(), String> {
    match crate::program::command(program).arg("-version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status() {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("{program} -version ended with {s}; --record needs a working ffmpeg")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Err(missing_message(program)),
        Err(e) => Err(format!("could not start {program}: {e}")),
    }
}

/// The arguments of ffmpeg for raw RGBA frames of `size` on stdin and a VP9 clip in `out` (a list, no shell).
pub fn ffmpeg_args(size: [usize; 2], out: &Path) -> Vec<String> {
    let mut a: Vec<String> = ["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba"].map(String::from).into();
    a.extend(["-s".into(), format!("{}x{}", size[0], size[1]), "-framerate".into(), FPS.to_string(), "-i".into(), "-".into()]);
    // even sides for yuv420p; a short clip of a still window needs few bits, so a high crf is enough
    a.extend(
        [
            "-vf",
            "scale=trunc(iw/2)*2:trunc(ih/2)*2,format=yuv420p",
            "-c:v",
            "libvpx-vp9",
            "-crf",
            "42",
            "-b:v",
            "0",
            "-deadline",
            "good",
            "-cpu-used",
            "4",
            "-threads",
            "1",
            "-g",
            "90",
            "-an",
            "-fflags",
            "+bitexact",
            "-flags:v",
            "+bitexact",
            "-map_metadata",
            "-1",
            "-f",
            "webm",
        ]
        .map(String::from),
    );
    a.push(out.to_string_lossy().into_owned());
    a
}

/// The file ffmpeg writes to before the clip is complete.
fn part_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".part");
    PathBuf::from(s)
}

/// A running ffmpeg and the frames given to it.
pub struct Recorder {
    path: PathBuf,
    size: [usize; 2],
    child: Child,
    stdin: Option<ChildStdin>,
    out: Option<JoinHandle<String>>,
    err: Option<JoinHandle<String>>,
    frames: u32,
}

fn drain(mut r: impl Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut s = String::new();
        let _ = r.read_to_string(&mut s);
        s
    })
}

impl Recorder {
    /// Start `program` for frames of `size`; the clip appears at `path` when `finish` succeeds.
    pub fn start(program: &str, path: &Path, size: [usize; 2]) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        let mut child = crate::program::command(program).args(ffmpeg_args(size, &part_path(path))).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                missing_message(program)
            } else {
                format!("could not start {program}: {e}")
            }
        })?;
        let stdin = child.stdin.take();
        let out = child.stdout.take().map(drain);
        let err = child.stderr.take().map(drain);
        Ok(Self { path: path.to_path_buf(), size, child, stdin, out, err, frames: 0 })
    }

    /// One frame of `size` as RGBA bytes.
    pub fn push(&mut self, size: [usize; 2], rgba: &[u8]) -> Result<(), String> {
        if size != self.size || rgba.len() != size[0] * size[1] * 4 {
            return Err(format!("a frame of {}x{} came after frames of {}x{}", size[0], size[1], self.size[0], self.size[1]));
        }
        let stdin = self.stdin.as_mut().ok_or("the video encoder is closed")?;
        self.frames += 1;
        stdin.write_all(rgba).map_err(|e| format!("the video encoder stopped: {e}"))
    }

    /// Close the input, wait for ffmpeg and move the clip into place; returns the number of frames.
    pub fn finish(mut self) -> Result<u32, String> {
        drop(self.stdin.take());
        let frames = self.frames;
        let status = self.child.wait().map_err(|e| format!("waiting for ffmpeg: {e}"))?;
        let out = self.out.take().and_then(|h| h.join().ok()).unwrap_or_default();
        let err = self.err.take().and_then(|h| h.join().ok()).unwrap_or_default();
        let part = part_path(&self.path);
        if frames == 0 {
            let _ = std::fs::remove_file(&part);
            return Err("no frame was given to ffmpeg, so there is no clip".into());
        }
        if !status.success() {
            let _ = std::fs::remove_file(&part);
            return Err(format!("ffmpeg ended with {status}: {}{}", err.trim(), out.trim()));
        }
        let clip = crate::fsutil::read(&part).map_err(|e| format!("could not read {}: {e}", part.display()))?;
        crate::fsutil::write_atomic(&self.path, &clip).map_err(|e| format!("could not write the clip {}: {e}", self.path.display()))?;
        let _ = std::fs::remove_file(&part);
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn have_ffmpeg() -> bool {
        let ok = probe(FFMPEG).is_ok();
        if !ok {
            eprintln!("skipping: ffmpeg is not installed");
        }
        ok
    }

    // spec: DEV-019
    #[test]
    fn dev_019_ffmpeg_gets_an_argument_list_for_raw_frames_at_a_fixed_rate() {
        let a = ffmpeg_args([1360, 840], Path::new("/x/c.webm.part"));
        let pos = |s: &str| a.iter().position(|x| x == s).unwrap_or_else(|| panic!("no {s} in {a:?}"));
        assert_eq!(a[pos("-s") + 1], "1360x840");
        assert_eq!(a[pos("-framerate") + 1], "30");
        assert_eq!(a[pos("-i") + 1], "-", "the frames come on stdin");
        assert_eq!(a[pos("-c:v") + 1], "libvpx-vp9");
        assert_eq!(a[pos("-f") + 1], "rawvideo");
        assert_eq!(a.last().unwrap(), "/x/c.webm.part");
        assert!(a.iter().any(|x| x.contains("trunc(iw/2)*2")), "odd sizes are made even");
        assert_eq!(part_path(Path::new("a/b.webm")), PathBuf::from("a/b.webm.part"));
    }

    // spec: DEV-019
    #[test]
    fn dev_019_only_a_webm_name_is_a_clip() {
        assert!(is_clip(Path::new("a/b.webm")) && is_clip(Path::new("B.WEBM")));
        assert!(!is_clip(Path::new("a.png")) && !is_clip(Path::new("webm")));
    }

    // spec: DEV-019, BUILD-018
    #[test]
    fn dev_019_a_missing_ffmpeg_is_reported_in_words() {
        let m = probe("shale-no-such-program-xyz").unwrap_err();
        assert!(m.contains("shale-no-such-program-xyz is not installed") && m.contains("install"), "{m}");
        let d = dir("missing");
        let m = Recorder::start("shale-no-such-program-xyz", &d.join("c.webm"), [64, 64]).err().expect("must fail");
        assert!(m.contains("is not installed"), "{m}");
    }

    // spec: DEV-019
    #[test]
    fn dev_019_frames_become_a_webm_clip_and_a_wrong_frame_is_refused() {
        if !have_ffmpeg() {
            return;
        }
        let d = dir("clip");
        let path = d.join("sub/c.webm");
        let mut r = Recorder::start(FFMPEG, &path, [65, 33]).unwrap();
        for i in 0..6u8 {
            r.push([65, 33], &vec![i * 40; 65 * 33 * 4]).unwrap();
        }
        assert!(r.push([64, 33], &[0; 64 * 33 * 4]).unwrap_err().contains("came after"));
        assert!(r.push([65, 33], &[0; 10]).is_err(), "a short frame is refused");
        assert_eq!(r.finish().unwrap(), 6);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], &[0x1A, 0x45, 0xDF, 0xA3], "an EBML (webm) header");
        assert!(!part_path(&path).exists());
    }

    // spec: DEV-019
    #[test]
    fn dev_019_a_failing_encoder_leaves_no_clip_and_says_why() {
        if !have_ffmpeg() {
            return;
        }
        let d = dir("fail");
        let path = d.join("c.webm");
        // no frame at all: nothing to encode
        let r = Recorder::start(FFMPEG, &path, [64, 64]).unwrap();
        assert!(r.finish().unwrap_err().contains("no frame"));
        // ffmpeg that cannot write its output says why (a folder where its file should be)
        std::fs::create_dir(part_path(&path)).unwrap();
        let mut bad = Recorder::start(FFMPEG, &path, [64, 64]).unwrap();
        let _ = bad.push([64, 64], &[0; 64 * 64 * 4]);
        let m = bad.finish().unwrap_err();
        assert!(m.starts_with("ffmpeg ended with"), "{m}");
        assert!(!path.exists());
    }
}
