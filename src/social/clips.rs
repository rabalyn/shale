//! The videos and GIFs of posts (SOC-076): downloaded in the background into the cache folder of the
//! pictures (SOC-013), then played from the file by the media player. The window never waits
//! for the network; a file that is in the folder already is not asked for again.
//!
//! The way a video reaches the player is always a whole file on this computer: Mastodon's
//! videos and "gifv" files are MP4 or WebM files at an address, and a Bluesky video is fetched
//! as the blob of the real file from the author's own server, not through the HLS playlist the
//! feed shows (the playlist is a list of addresses that ffmpeg would follow, SEC-028). No login
//! goes to any media host (SOC-015); only https is used (`check_url`); at most `MAX_MEDIA_BYTES`
//! are taken; a file is written under its own name only when it is complete and is a video
//! (`video::sniff`), so a partial file is never played.

use super::images::fnv;
use eframe::egui;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The file types a clip is kept as (`video::sniff` names the type from the first bytes).
pub const EXTS: [&str; 4] = ["mp4", "mov", "webm", "mpeg"];

/// Where the file of a clip comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// an address (Mastodon: the file of the video or "gifv")
    Url(String),
    /// Bluesky: the blob of the video, from the author's own server
    Blob { did: String, cid: String },
}

impl Source {
    /// The source of a media entry of a post by the author `did`; None for a Bluesky video
    /// that names no file (only the playlist).
    pub fn of(m: &super::Media, did: &str) -> Option<Source> {
        let playlist = m.url.split(['?', '#']).next().unwrap_or("").to_ascii_lowercase().ends_with(".m3u8");
        match (&m.blob_cid, playlist) {
            (Some(cid), _) => Some(Source::Blob { did: did.to_string(), cid: cid.clone() }),
            (None, true) => None,
            (None, false) => Some(Source::Url(m.url.clone())),
        }
    }

    fn key(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::Blob { did, cid } => format!("blob:{did}:{cid}"),
        }
    }
}

/// What a clip is at the moment.
#[derive(Clone, Debug, PartialEq)]
pub enum Clip {
    Loading,
    Ready(PathBuf),
    Failed(String),
}

type Slots = Arc<Mutex<HashMap<String, Clip>>>;

/// The clips asked for in this run.
pub struct Clips {
    slots: Slots,
    dir: PathBuf,
    ctx: egui::Context,
}

fn cached(dir: &Path, key: &str) -> Option<PathBuf> {
    let stem = format!("{:016x}", fnv(key));
    EXTS.iter().map(|e| dir.join(format!("{stem}.{e}"))).find(|p| p.is_file())
}

/// Fetch one clip into the cache folder (blocking), at most `limit` bytes.
fn fetch(dir: &Path, source: &Source, limit: u64) -> Result<PathBuf, String> {
    let key = source.key();
    if let Some(p) = cached(dir, &key) {
        return Ok(p);
    }
    let (data, _) = match source {
        Source::Url(u) => super::download_up_to(u, limit)?,
        Source::Blob { did, cid } => super::bluesky::download_blob(did, cid)?,
    };
    let (_, ext) = super::video::sniff(&data[..data.len().min(16)]).ok_or("this is not a video file that can be played")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{:016x}.{ext}", fnv(&key)));
    crate::fsutil::write_atomic(&path, &data).map_err(|e| e.to_string())?;
    Ok(path)
}

impl Clips {
    /// `ctx` is asked to repaint whenever a clip arrives or fails.
    pub fn new(ctx: &egui::Context, dir: PathBuf) -> Clips {
        Clips { slots: Slots::default(), dir, ctx: ctx.clone() }
    }

    /// The clip of `source`: the first call starts the download in a background job.
    pub fn get(&self, source: &Source) -> Clip {
        self.get_up_to(source, super::MAX_MEDIA_BYTES)
    }

    fn get_up_to(&self, source: &Source, limit: u64) -> Clip {
        let key = source.key();
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = slots.get(&key) {
            return c.clone();
        }
        slots.insert(key.clone(), Clip::Loading);
        let (slots, dir, ctx, source) = (self.slots.clone(), self.dir.clone(), self.ctx.clone(), source.clone());
        crate::jobs::run(
            move || fetch(&dir, &source, limit),
            move |r| {
                let clip = match r.and_then(|r| r) {
                    Ok(p) => Clip::Ready(p),
                    Err(e) => Clip::Failed(e),
                };
                slots.lock().unwrap_or_else(|e| e.into_inner()).insert(key, clip);
                ctx.request_repaint();
            },
        );
        Clip::Loading
    }

    /// Forget the clips found so far (after the files were cleared or pruned): each one is
    /// fetched again when it is shown next (USE-010).
    pub fn forget_all(&self) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).retain(|_, c| *c == Clip::Loading);
    }

    /// Tests: an address that is on its way and never arrives.
    #[cfg(test)]
    pub fn hold(&self, source: &Source) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).insert(source.key(), Clip::Loading);
    }

    /// Tests: the name a clip of `source` has in the folder, for a file put there beforehand.
    #[cfg(test)]
    pub fn file_for(dir: &Path, source: &Source, ext: &str) -> PathBuf {
        dir.join(format!("{:016x}.{ext}", fnv(&source.key())))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// A file server on 127.0.0.1 (the only place `http://` is allowed): answers every request
    /// with `body`, and keeps the head of each request.
    struct Host {
        port: u16,
        heads: Arc<Mutex<Vec<String>>>,
        stop: Arc<AtomicBool>,
    }

    impl Host {
        fn start(status: u16, body: Vec<u8>) -> Host {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let port = listener.local_addr().unwrap().port();
            let (heads, stop) = (Arc::new(Mutex::new(Vec::new())), Arc::new(AtomicBool::new(false)));
            let (h, sp) = (heads.clone(), stop.clone());
            std::thread::spawn(move || {
                while !sp.load(Ordering::SeqCst) {
                    let Ok((mut s, _)) = listener.accept() else {
                        std::thread::sleep(std::time::Duration::from_millis(3));
                        continue;
                    };
                    let _ = s.set_nonblocking(false);
                    let mut buf = [0u8; 4096];
                    let n = s.read(&mut buf).unwrap_or(0);
                    h.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).into_owned());
                    let head = format!("HTTP/1.1 {status} X\r\nContent-Type: video/mp4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&body);
                }
            });
            Host { port, heads, stop }
        }

        fn url(&self, name: &str) -> String {
            format!("http://127.0.0.1:{}/{name}", self.port)
        }

        fn requests(&self) -> usize {
            self.heads.lock().unwrap().len()
        }
    }

    impl Drop for Host {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
        }
    }

    /// The first bytes of an MP4 file, enough for `video::sniff`, and some more.
    fn mp4(extra: usize) -> Vec<u8> {
        let mut v = vec![0, 0, 0, 24];
        v.extend_from_slice(b"ftypmp42");
        v.resize(24 + extra, 7);
        v
    }

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-clips-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn wait(c: &Clips, s: &Source) -> Clip {
        for _ in 0..500 {
            match c.get(s) {
                Clip::Loading => std::thread::sleep(std::time::Duration::from_millis(10)),
                done => return done,
            }
        }
        panic!("the clip never arrived");
    }

    // spec: SOC-076
    #[test]
    fn a_clip_is_downloaded_once_without_any_login_and_a_cached_one_is_not_asked_for_again() {
        let host = Host::start(200, mp4(500));
        let d = dir("once");
        let ctx = egui::Context::default();
        let c = Clips::new(&ctx, d.clone());
        let s = Source::Url(host.url("a.mp4"));
        assert_eq!(c.get(&s), Clip::Loading, "the window never waits");
        let Clip::Ready(path) = wait(&c, &s) else { panic!("failed") };
        assert_eq!(path, Clips::file_for(&d, &s, "mp4"));
        assert_eq!(std::fs::read(&path).unwrap(), mp4(500));
        assert_eq!(host.requests(), 1);
        let head = host.heads.lock().unwrap()[0].to_ascii_lowercase();
        assert!(head.starts_with("get /a.mp4 "), "{head}");
        assert!(!head.contains("authorization") && !head.contains("cookie"), "no login goes to a media host: {head}");
        // a second asker (a new run of the app, the same folder) takes the file
        let c2 = Clips::new(&ctx, d.clone());
        assert_eq!(wait(&c2, &s), Clip::Ready(path.clone()));
        assert_eq!(c.get(&s), Clip::Ready(path));
        assert_eq!(host.requests(), 1, "a file in the cache is not downloaded again");
        // after the cache was cleared the clip is fetched again
        std::fs::remove_dir_all(&d).unwrap();
        c.forget_all();
        assert!(matches!(wait(&c, &s), Clip::Ready(_)));
        assert_eq!(host.requests(), 2);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: SOC-076, SEC-028
    #[test]
    fn a_clip_over_the_size_limit_or_not_a_video_or_refused_fails_with_words_and_leaves_no_file() {
        let d = dir("fail");
        let ctx = egui::Context::default();
        let c = Clips::new(&ctx, d.clone());
        let big = Host::start(200, mp4(5000));
        let s = Source::Url(big.url("big.mp4"));
        c.get_up_to(&s, 1000);
        let Clip::Failed(e) = (loop {
            match c.get_up_to(&s, 1000) {
                Clip::Loading => std::thread::sleep(std::time::Duration::from_millis(10)),
                done => break done,
            }
        }) else {
            panic!("a clip over the limit must fail")
        };
        assert!(e.contains("bigger than"), "{e}");
        let page = Host::start(200, b"<html>no video</html>".to_vec());
        let s = Source::Url(page.url("x.mp4"));
        assert_eq!(wait(&c, &s), Clip::Failed("this is not a video file that can be played".into()));
        let gone = Host::start(404, b"gone".to_vec());
        let s = Source::Url(gone.url("y.mp4"));
        assert_eq!(wait(&c, &s), Clip::Failed("the server answered with status 404".into()));
        // not https: refused before any request
        let s = Source::Url("http://example.invalid/z.mp4".into());
        assert!(matches!(wait(&c, &s), Clip::Failed(_)));
        assert!(std::fs::read_dir(&d).map(|mut r| r.next().is_none()).unwrap_or(true), "no file, not even a partial one, is left");
        // a failure is asked for once; forgetting lets it be asked again
        assert_eq!(gone.requests(), 1);
        assert!(matches!(c.get(&s), Clip::Failed(_)));
        c.forget_all();
        assert_eq!(c.get(&s), Clip::Loading);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: SOC-076, SEC-028
    #[test]
    fn a_bluesky_video_is_the_blob_of_its_file_never_the_playlist() {
        let m = |url: &str, cid: Option<&str>| super::super::Media { kind: super::super::MediaKind::Video, url: url.into(), preview_url: String::new(), alt: String::new(), file_name_hint: "v.mp4".into(), blob_cid: cid.map(String::from) };
        assert_eq!(Source::of(&m("https://video.bsky.app/watch/did/cid/playlist.m3u8", Some("bafyvideo")), "did:plc:ada"), Some(Source::Blob { did: "did:plc:ada".into(), cid: "bafyvideo".into() }));
        assert_eq!(Source::of(&m("https://video.bsky.app/watch/did/cid/playlist.m3u8", None), "did:plc:ada"), None, "a playlist is not a file");
        assert_eq!(Source::of(&m("https://files.test/a.mp4", None), "x"), Some(Source::Url("https://files.test/a.mp4".into())));
        assert_ne!(Source::Blob { did: "a".into(), cid: "c".into() }.key(), Source::Url("a".into()).key());
    }
}
