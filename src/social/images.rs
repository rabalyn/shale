//! Pictures of the feed (avatars, previews): downloaded by a few worker threads into a cache
//! folder, then shown from the file (`media::texture`). The window never waits for the network.
//!
//! Each address is asked for once (`get` queues it); a file already in the cache folder is used
//! without a download. An absolute file path instead of an address is used as it is (demo posts).

use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};

const WORKERS: usize = 4;
const EXTS: [&str; 5] = ["png", "jpg", "webp", "bmp", "gif"];

#[derive(Clone, Debug, PartialEq)]
enum Slot {
    Pending,
    Ready(PathBuf),
    Failed,
}

type Slots = Arc<Mutex<HashMap<String, Slot>>>;

pub struct ImageCache {
    slots: Slots,
    queue: Sender<String>,
}

/// FNV-1a, 64 bit: a short stable file name for an address.
pub fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The picture type by its first bytes (the file name extension decides how it is opened).
fn sniff(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG") {
        Some("png")
    } else if data.starts_with(&[0xFF, 0xD8]) {
        Some("jpg")
    } else if data.len() > 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("webp")
    } else if data.starts_with(b"GIF8") {
        Some("gif")
    } else if data.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

fn cached(dir: &Path, url: &str) -> Option<PathBuf> {
    let stem = format!("{:016x}", fnv(url));
    EXTS.iter().map(|e| dir.join(format!("{stem}.{e}"))).find(|p| p.is_file())
}

/// Fetch one address into the cache folder (blocking).
fn fetch(dir: &Path, url: &str) -> Result<PathBuf, String> {
    if let Some(p) = cached(dir, url) {
        return Ok(p);
    }
    let (data, _) = super::download(url)?;
    let ext = sniff(&data).ok_or("not a picture")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{:016x}.{ext}", fnv(url)));
    crate::fsutil::write_atomic(&path, &data).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Whether a file name is one of the cache's own (`<16 hex digits>.<picture type>`).
fn is_cache_file(name: &str) -> bool {
    name.split_once('.').is_some_and(|(stem, ext)| stem.len() == 16 && stem.bytes().all(|b| b.is_ascii_hexdigit()) && EXTS.contains(&ext))
}

/// The picture files in the cache folder, oldest fetch first (USE-010).
pub fn cache_files(dir: &Path) -> Vec<crate::data_usage::files::FileEntry> {
    crate::data_usage::files::entries(dir, is_cache_file)
}

impl ImageCache {
    /// `ctx` is asked to repaint whenever a picture arrives.
    pub fn new(ctx: &egui::Context, dir: PathBuf) -> ImageCache {
        let slots: Slots = Arc::default();
        let (tx, rx) = channel::<String>();
        let rx = Arc::new(Mutex::new(rx));
        for _ in 0..WORKERS {
            let (rx, slots, ctx, dir) = (rx.clone(), slots.clone(), ctx.clone(), dir.clone());
            crate::jobs::run(
                move || loop {
                    // one worker waits for the next address; the lock is let go before the download
                    let next = rx.lock().unwrap_or_else(|e| e.into_inner()).recv();
                    let Ok(url) = next else { return };
                    let r = crate::jobs::catch(|| fetch(&dir, &url)).and_then(|r| r);
                    slots.lock().unwrap_or_else(|e| e.into_inner()).insert(url, r.map(Slot::Ready).unwrap_or(Slot::Failed));
                    ctx.request_repaint();
                },
                |_| {},
            );
        }
        ImageCache { slots, queue: tx }
    }

    /// The file of this picture, once it is here. The first call for an address starts the download.
    pub fn get(&self, url: &str) -> Option<PathBuf> {
        if url.is_empty() {
            return None;
        }
        if url.starts_with('/') {
            return Some(PathBuf::from(url));
        }
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        match slots.get(url) {
            Some(Slot::Ready(p)) => Some(p.clone()),
            Some(_) => None,
            None => {
                slots.insert(url.to_string(), Slot::Pending);
                let _ = self.queue.send(url.to_string());
                None
            }
        }
    }

    /// Whether this address could not be fetched (show a placeholder instead of waiting).
    pub fn failed(&self, url: &str) -> bool {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).get(url) == Some(&Slot::Failed)
    }

    /// Forget the pictures found so far (after the files were cleared or pruned): each one is
    /// fetched again when it is shown next (USE-010).
    pub fn forget_all(&self) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, s| *s == Slot::Pending);
    }

    /// How many pictures are still on their way.
    pub fn pending(&self) -> usize {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).values().filter(|s| **s == Slot::Pending).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: SOC-013
    #[test]
    fn local_files_are_used_as_they_are_and_names_are_stable() {
        let ctx = egui::Context::default();
        let c = ImageCache::new(&ctx, std::env::temp_dir().join(format!("shale-img-{}", std::process::id())));
        assert_eq!(c.get("/tmp/x.png"), Some(PathBuf::from("/tmp/x.png")));
        assert_eq!(c.get(""), None);
        assert_eq!(fnv("https://a.test/x.jpg"), fnv("https://a.test/x.jpg"));
        assert_ne!(fnv("https://a.test/x.jpg"), fnv("https://a.test/y.jpg"));
        assert_eq!(sniff(b"\x89PNG\r\n"), Some("png"));
        assert_eq!(sniff(b"<html>"), None);
        // an address that is not allowed fails by itself (no network needed) and is asked only once
        assert_eq!(c.get("http://example.invalid/x.png"), None);
        for _ in 0..100 {
            if c.failed("http://example.invalid/x.png") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(c.failed("http://example.invalid/x.png"));
        assert_eq!(c.pending(), 0);
    }

    // spec: USE-010
    #[test]
    fn only_picture_files_of_the_cache_are_its_own_and_forgetting_fetches_again() {
        assert!(is_cache_file("0123456789abcdef.png"));
        assert!(!is_cache_file("0123456789abcdef.txt"));
        assert!(!is_cache_file("short.png"));
        assert!(!is_cache_file("notes.md"));
        let d = std::env::temp_dir().join(format!("shale-img-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("0123456789abcdef.png"), b"\x89PNG").unwrap();
        std::fs::write(d.join("other.txt"), b"x").unwrap();
        assert_eq!(cache_files(&d).len(), 1);
        let ctx = egui::Context::default();
        let c = ImageCache::new(&ctx, d.clone());
        c.slots.lock().unwrap().insert("https://a.test/x.png".into(), Slot::Ready(d.join("0123456789abcdef.png")));
        c.slots.lock().unwrap().insert("https://a.test/y.png".into(), Slot::Failed);
        c.forget_all();
        assert!(c.slots.lock().unwrap().is_empty(), "known and failed pictures are asked for again");
        let _ = std::fs::remove_dir_all(&d);
    }
}
