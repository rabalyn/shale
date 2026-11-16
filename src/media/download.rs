//! Media that are still Git LFS pointer files (SYNC-020, ADR-0015 step D).
//!
//! A vault synced with `--skip-smudge` holds a few lines of text where a song or a picture should
//! be. The players and pictures ask this state about a file: if it is a pointer, they say "Not
//! downloaded (12 MB)" with a Download link instead of failing to decode it. A request made here
//! is taken by the app, which runs `git lfs pull` in a background job (`app/sync.rs`) and reports
//! back; a play button that asked for it starts the sound when the file has arrived.

use crate::gitsync::lfs;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// How many files' answers are kept.
const KEPT: usize = 256;

/// What the person sees about a file that is not downloaded.
#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    /// The size of the real file, from the pointer.
    pub size: u64,
    /// A download is running.
    pub running: bool,
    /// Why the last download failed, in words.
    pub error: Option<String>,
}

impl Notice {
    /// The line the player shows: "Not downloaded (12 MB)", or what went wrong.
    pub fn line(&self) -> String {
        match (&self.error, self.running) {
            (_, true) => "Downloading…".to_string(),
            (Some(e), false) => format!("Download failed: {e}"),
            (None, false) => format!("Not downloaded ({})", size_text(self.size)),
        }
    }
}

/// `12_345_678` → `12 MB` (decimal units, as file managers show them).
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.0} kB", b / 1e3)
    } else if bytes < 1_000_000_000 {
        format!("{:.0} MB", b / 1e6)
    } else {
        format!("{:.1} GB", b / 1e9)
    }
}

#[derive(Default)]
struct Job {
    running: bool,
    error: Option<String>,
    /// The note whose play button asked: the sound starts when the file is here.
    play_for: Option<PathBuf>,
}

/// The pointer files the views have met, and their downloads.
#[derive(Default)]
pub struct Downloads {
    /// The size a pointer announces (None: the file is real), per file version.
    seen: HashMap<(PathBuf, Option<SystemTime>), Option<u64>>,
    jobs: HashMap<PathBuf, Job>,
    /// Asked for, not yet taken by the app.
    requests: Vec<PathBuf>,
}

impl Downloads {
    /// The size of the real file when `abs` holds a pointer; None for a real file or none at all.
    fn pointer_size(&mut self, abs: &Path) -> Option<u64> {
        let meta = std::fs::metadata(abs).ok()?;
        if !meta.is_file() || meta.len() > 1024 {
            return None; // a pointer is a few lines; nothing to read for a real media file
        }
        let key = (abs.to_path_buf(), meta.modified().ok());
        if let Some(known) = self.seen.get(&key) {
            return *known;
        }
        if self.seen.len() >= KEPT {
            self.seen.clear();
        }
        let size = crate::fsutil::read(abs).ok().and_then(|b| lfs::parse_pointer(&b)).map(|p| p.size);
        self.seen.insert(key, size);
        size
    }

    /// What to say about `abs`: None when it is a real file.
    pub fn notice(&mut self, abs: &Path) -> Option<Notice> {
        let size = self.pointer_size(abs)?;
        let job = self.jobs.get(abs);
        Some(Notice { size, running: job.is_some_and(|j| j.running), error: job.and_then(|j| j.error.clone()) })
    }

    /// Ask for the file: the app starts the download. `play_for`: the note whose play button asked.
    pub fn request(&mut self, abs: &Path, play_for: Option<&Path>) {
        if self.pointer_size(abs).is_none() {
            return;
        }
        let job = self.jobs.entry(abs.to_path_buf()).or_default();
        if play_for.is_some() {
            job.play_for = play_for.map(Path::to_path_buf);
        }
        if !job.running {
            job.running = true;
            job.error = None;
            self.requests.push(abs.to_path_buf());
        }
    }

    /// The files asked for since the last call.
    pub fn take_requests(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.requests)
    }

    /// A download ended. Returns the note whose play button asked, when the file is here now.
    pub fn finish(&mut self, abs: &Path, result: Result<(), String>) -> Option<PathBuf> {
        self.seen.retain(|(p, _), _| p != abs);
        let job = self.jobs.get_mut(abs)?;
        job.running = false;
        match result {
            Ok(()) => {
                job.error = None;
                job.play_for.take()
            }
            Err(e) => {
                job.error = Some(e);
                job.play_for = None;
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pointer(size: u64) -> String {
        format!("version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize {size}\n", "a".repeat(64))
    }

    // spec: SYNC-020
    #[test]
    fn sizes_are_written_the_way_file_managers_do() {
        assert_eq!(size_text(999), "999 B");
        assert_eq!(size_text(12_345), "12 kB");
        assert_eq!(size_text(12_345_678), "12 MB");
        assert_eq!(size_text(3_400_000_000), "3.4 GB");
    }

    // spec: SYNC-020
    #[test]
    fn a_pointer_is_not_downloaded_and_a_real_file_or_a_missing_one_is_nothing_to_say() {
        let dir = std::env::temp_dir().join(format!("shale-media-pointer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (song, real, gone) = (dir.join("Song.mp3"), dir.join("Real.mp3"), dir.join("Gone.mp3"));
        std::fs::write(&song, pointer(12_345_678)).unwrap();
        std::fs::write(&real, vec![7u8; 4000]).unwrap();
        let mut d = Downloads::default();
        let n = d.notice(&song).expect("a pointer");
        assert_eq!((n.size, n.running, n.line()), (12_345_678, false, "Not downloaded (12 MB)".to_string()));
        assert_eq!(d.notice(&real), None);
        assert_eq!(d.notice(&gone), None);
        // a small file that is no pointer is none either
        std::fs::write(&real, "ID3 and a few bytes").unwrap();
        assert_eq!(Downloads::default().notice(&real), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: SYNC-020
    #[test]
    fn a_request_runs_once_and_the_play_button_that_asked_gets_its_sound_when_the_file_is_here() {
        let dir = std::env::temp_dir().join(format!("shale-media-request-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let song = dir.join("Song.mp3");
        std::fs::write(&song, pointer(10)).unwrap();
        let mut d = Downloads::default();
        d.request(&song, Some(Path::new("Note.md")));
        d.request(&song, None);
        assert_eq!(d.take_requests(), vec![song.clone()], "asked twice, one download");
        assert!(d.take_requests().is_empty());
        assert!(d.notice(&song).unwrap().running);
        // it failed: the words are shown, nothing plays, and "Download" asks again
        assert_eq!(d.finish(&song, Err("no network".into())), None);
        let n = d.notice(&song).unwrap();
        assert!(!n.running && n.line().contains("no network"), "{n:?}");
        d.request(&song, Some(Path::new("Note.md")));
        assert_eq!(d.take_requests().len(), 1);
        assert!(d.notice(&song).unwrap().error.is_none());
        // the file arrived
        std::fs::write(&song, vec![1u8; 2000]).unwrap();
        assert_eq!(d.finish(&song, Ok(())), Some(PathBuf::from("Note.md")));
        assert_eq!(d.notice(&song), None);
        // a real file is never requested
        d.request(&song, None);
        assert!(d.take_requests().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
