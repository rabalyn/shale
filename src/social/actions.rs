//! Writing to the networks: like / favourite and boost / repost a post (and undo it), and the
//! types the compose box and the two protocols share (`Draft`, `InstanceInfo`, `ActionError`).
//! The protocol parts are in `mastodon.rs` and `bluesky.rs`, the compose box in `compose.rs`.
//!
//! A reaction runs in a background job (`jobs::run`, SOC-010). Until it ends the button shows a
//! spinner and cannot be pressed again; the counts and the state of the buttons change only
//! when it went well (SOC-050). Mastodon accounts that signed in with `read` only are asked to
//! sign in again before anything is sent (SOC-056).

use super::poll::{DraftPoll, Poll, PollLimits};
use super::schedule::Scheduled;
use super::state::{run, Msg, SocialState};
use super::video::DraftVideo;
use super::*;
use eframe::egui::Context;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// What the toast and the compose box say when the Mastodon token cannot write.
pub const SIGN_IN_AGAIN: &str = "Sign in again to allow writing";

/// The two reactions to a post.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Reaction {
    /// Mastodon favourite, Bluesky like
    Like,
    /// Mastodon boost, Bluesky repost
    Boost,
}

impl Reaction {
    /// The word for messages ("favourite", "like", "boost", "repost").
    pub fn verb(self, network: Network) -> &'static str {
        match (self, network) {
            (Reaction::Like, Network::Mastodon) => "favourite",
            (Reaction::Like, Network::Bluesky) => "like",
            (Reaction::Boost, Network::Mastodon) => "boost",
            (Reaction::Boost, Network::Bluesky) => "repost",
        }
    }
}

/// The server's answer to a reaction.
#[derive(Clone, Debug, PartialEq)]
pub struct Reacted {
    /// the post is now liked / boosted (false: it was undone)
    pub on: bool,
    /// Bluesky: the record that undoes it
    pub record: Option<String>,
    /// the count the server named (Mastodon); None: one more or less than before
    pub count: Option<u32>,
    /// Bluesky: the state of the post as the server has it, read first because the post's own was
    /// not known (SOC-060)
    pub fresh: Option<super::Mine>,
}

/// Why a write failed.
#[derive(Clone, Debug, PartialEq)]
pub enum ActionError {
    /// the Mastodon token has no permission to write (SOC-056)
    NeedsWrite,
    Failed(String),
}

impl From<String> for ActionError {
    fn from(m: String) -> Self {
        ActionError::Failed(m)
    }
}

impl From<&str> for ActionError {
    fn from(m: &str) -> Self {
        ActionError::Failed(m.to_string())
    }
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ActionError::NeedsWrite => f.write_str(SIGN_IN_AGAIN),
            ActionError::Failed(m) => f.write_str(m),
        }
    }
}

/// A Mastodon answer to a write: 403 "outside the authorized scopes" means the token is read-only.
pub(crate) fn write_error(e: ApiError) -> ActionError {
    if e.status == 403 && e.message.to_ascii_lowercase().contains("scope") {
        ActionError::NeedsWrite
    } else {
        ActionError::Failed(e.message)
    }
}

/// Who may see a Mastodon post.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Visibility {
    #[default]
    Public,
    Unlisted,
    Private,
    Direct,
}

impl Visibility {
    pub const ALL: [Visibility; 4] = [Visibility::Public, Visibility::Unlisted, Visibility::Private, Visibility::Direct];

    /// The word the API uses.
    pub fn key(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::Unlisted => "unlisted",
            Visibility::Private => "private",
            Visibility::Direct => "direct",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Visibility::Public => "Public",
            Visibility::Unlisted => "Unlisted",
            Visibility::Private => "Followers only",
            Visibility::Direct => "Mentioned people only",
        }
    }

    pub fn from_key(k: &str) -> Option<Visibility> {
        Visibility::ALL.into_iter().find(|v| v.key() == k)
    }
}

/// What a server allows in a post (Mastodon: from `/api/v2/instance`; Bluesky: fixed).
#[derive(Clone, Debug, PartialEq)]
pub struct InstanceInfo {
    /// characters (Mastodon) / graphemes (Bluesky) of a post
    pub max_chars: usize,
    pub max_pictures: usize,
    /// the biggest picture file in bytes
    pub image_bytes: u64,
    /// the server can quote a post (Mastodon 4.5, API version 7)
    pub quotes: bool,
    /// what a poll may be (SOC-061)
    pub poll: PollLimits,
    /// the biggest video file in bytes (SOC-066)
    pub video_bytes: u64,
}

/// What a Mastodon server that does not say gets (the defaults of Mastodon).
pub const MASTODON_MAX_CHARS: usize = 500;
/// Bluesky counts graphemes.
pub const BLUESKY_MAX_CHARS: usize = 300;
/// The most pictures of a post, on both networks.
pub const MAX_PICTURES: usize = 4;
/// Bluesky takes a picture of at most this many bytes (its lexicon); bigger ones are made smaller.
pub const BLUESKY_IMAGE_BYTES: u64 = 1_000_000;

impl InstanceInfo {
    /// Before the server has answered, and for a server that names nothing.
    pub fn defaults(network: Network) -> InstanceInfo {
        match network {
            Network::Mastodon => InstanceInfo { max_chars: MASTODON_MAX_CHARS, max_pictures: MAX_PICTURES, image_bytes: 16 * 1024 * 1024, quotes: false, poll: PollLimits::default(), video_bytes: video::MASTODON_VIDEO_BYTES },
            Network::Bluesky => InstanceInfo { max_chars: BLUESKY_MAX_CHARS, max_pictures: MAX_PICTURES, image_bytes: BLUESKY_IMAGE_BYTES, quotes: true, poll: PollLimits::default(), video_bytes: video::MAX_VIDEO_BYTES },
        }
    }
}

/// A picture of a draft: the file and its description.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftPicture {
    pub path: PathBuf,
    pub alt: String,
}

/// What the server made of a send: a post, or on Mastodon a scheduled post (SOC-063), which is
/// no post yet and goes nowhere in the feed.
#[derive(Clone, Debug, PartialEq)]
pub enum Sent {
    Posted(Box<Post>),
    Scheduled(Scheduled),
}

/// A post as the compose box makes it, ready to be sent.
#[derive(Clone, Debug)]
pub struct Draft {
    /// the text as it is sent (a quote that the server cannot make carries the post's address, SOC-054)
    pub text: String,
    /// Mastodon: the content warning ("" = none)
    pub cw: String,
    pub visibility: Visibility,
    pub reply_to: Option<Post>,
    pub quote: Option<Post>,
    pub pictures: Vec<DraftPicture>,
    /// Mastodon: the poll (SOC-061; not with pictures or a video)
    pub poll: Option<DraftPoll>,
    /// Mastodon: the moment (UTC seconds) the server is to send the post (SOC-063)
    pub schedule: Option<i64>,
    /// the language of the post, ISO 639-1; "" names none (SOC-065)
    pub language: String,
    /// the video (SOC-066, SOC-067; not with pictures)
    pub video: Option<DraftVideo>,
    pub info: InstanceInfo,
    /// names this draft for the server: the same for a retry of the same draft, another one after a
    /// change (a TID, `new_key`); Mastodon sends it as `Idempotency-Key`, Bluesky as the `rkey` (SOC-057)
    pub key: String,
}

/// A fresh key: a TID (13 characters of the base32-sortable alphabet, 53 bits of microseconds and
/// 10 of a counter), which is also what Bluesky wants as a record key.
pub fn new_key() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let micros = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_micros() as u64).unwrap_or(0);
    // always above the one before, also within one microsecond
    let mut id = (micros & ((1 << 53) - 1)) << 10;
    let prev = LAST.fetch_max(id, Ordering::SeqCst);
    if id <= prev {
        id = prev + 1;
        LAST.store(id, Ordering::SeqCst);
    }
    const ALPHABET: &[u8; 32] = b"234567abcdefghijklmnopqrstuvwxyz";
    (0..13).rev().map(|i| ALPHABET[((id >> (5 * i)) & 31) as usize] as char).collect()
}

// ---------------------------------------------------------------- pictures

/// A picture ready for upload.
#[derive(Debug)]
pub struct Prepared {
    pub bytes: Vec<u8>,
    pub mime: &'static str,
    pub ext: &'static str,
}

/// Read a picture file for upload (SOC-053): JPEG, PNG, GIF or WebP, at most `MAX_MEDIA_BYTES`. A
/// Mastodon picture over the server's limit is refused with a message. A Bluesky picture over
/// 1 MB is made smaller (scaled down and saved as JPEG until it fits); a GIF that is too big is
/// refused, as scaling would end its animation.
pub fn prepare(path: &Path, network: Network, limit: u64) -> Result<Prepared, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "the file".into());
    let meta = std::fs::metadata(path).map_err(|e| format!("{name} cannot be read ({e})"))?;
    if !meta.is_file() {
        return Err(format!("{name} is not a file"));
    }
    if meta.len() > MAX_MEDIA_BYTES {
        return Err(format!("{name} is too big ({} MB)", meta.len() / 1_000_000));
    }
    let bytes = crate::fsutil::read(path).map_err(|e| format!("{name} cannot be read ({e})"))?;
    let (mime, ext, format) = match image::guess_format(&bytes) {
        Ok(f @ image::ImageFormat::Png) => ("image/png", "png", f),
        Ok(f @ image::ImageFormat::Jpeg) => ("image/jpeg", "jpg", f),
        Ok(f @ image::ImageFormat::Gif) => ("image/gif", "gif", f),
        Ok(f @ image::ImageFormat::WebP) => ("image/webp", "webp", f),
        _ => return Err(format!("{name} is not a JPEG, PNG, GIF or WebP picture")),
    };
    if bytes.len() as u64 <= limit {
        return Ok(Prepared { bytes, mime, ext });
    }
    let mb = |n: u64| format!("{:.1} MB", n as f64 / 1_000_000.0);
    if network == Network::Mastodon || format == image::ImageFormat::Gif {
        let why = if network == Network::Mastodon { "this server takes pictures up to" } else { "Bluesky takes pictures up to" };
        return Err(format!("{name} is {}; {why} {}{}", mb(bytes.len() as u64), mb(limit), if network == Network::Bluesky { " (a GIF cannot be made smaller here)" } else { "" }));
    }
    let small = shrink(&bytes, limit).ok_or_else(|| format!("{name} cannot be made small enough for Bluesky ({})", mb(limit)))?;
    Ok(Prepared { bytes: small, mime: "image/jpeg", ext: "jpg" })
}

/// A JPEG of at most `limit` bytes made from a picture: the longest side starts at 2000 points
/// and shrinks by a quarter each round, the quality drops from 85.
fn shrink(bytes: &[u8], limit: u64) -> Option<Vec<u8>> {
    let img = image::load_from_memory(bytes).ok()?;
    // transparent parts become white (JPEG has none)
    let mut rgba = img.to_rgba8();
    for p in rgba.pixels_mut() {
        let a = u32::from(p[3]);
        for c in &mut p.0[..3] {
            *c = ((u32::from(*c) * a + 255 * (255 - a)) / 255) as u8;
        }
        p[3] = 255;
    }
    let base = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    let mut side = 2000.0_f32;
    while side >= 300.0 {
        let long = base.width().max(base.height()) as f32;
        let scaled = if long > side {
            image::imageops::resize(&base, (base.width() as f32 * side / long).round().max(1.0) as u32, (base.height() as f32 * side / long).round().max(1.0) as u32, image::imageops::FilterType::Triangle)
        } else {
            base.clone()
        };
        for q in [85u8, 75, 65, 55] {
            let mut out = Vec::new();
            if image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, q).encode_image(&scaled).is_ok() && out.len() as u64 <= limit {
                return Some(out);
            }
        }
        side *= 0.75;
    }
    None
}

/// A file in the temporary folder that is removed when this is dropped: the picture that curl
/// reads for an upload (so its name is one Shale made).
#[derive(Debug)]
pub struct TempFile(pub PathBuf);

impl TempFile {
    pub fn new(bytes: &[u8], ext: &str) -> Result<TempFile, String> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        // a folder only this user can enter (the runtime folder, else a private one in the temp folder)
        let dir = crate::fsutil::private_temp_root("shale-upload").map_err(|e| format!("could not prepare the picture ({e})"))?;
        let path = dir.join(format!("{}-{}.{ext}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        crate::fsutil::create_new(&path, bytes).map_err(|e| format!("could not prepare the picture ({e})"))?;
        Ok(TempFile(path))
    }
}

impl TempFile {
    /// A private copy of a file (a video, SEC-026): written without reading it all into memory.
    pub fn copy_of(src: &Path, ext: &str) -> Result<TempFile, String> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = crate::fsutil::private_temp_root("shale-upload").map_err(|e| format!("could not prepare the file ({e})"))?;
        let path = dir.join(format!("{}-v{}.{ext}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        crate::fsutil::copy_new(src, &path).map_err(|e| format!("could not prepare the file ({e})"))?;
        Ok(TempFile(path))
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// "2026-11-23T10:30:00.000Z" for a Bluesky record.
pub fn iso_now() -> String {
    iso_at(t::now())
}

/// "2026-11-23T10:30:00.000Z" for a moment (UTC seconds).
pub fn iso_at(now: i64) -> String {
    let (y, m, d) = t::civil_from_days(now.div_euclid(86_400));
    let s = now.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.000Z", s / 3600, s / 60 % 60, s % 60)
}

// ---------------------------------------------------------------- the client

impl Client {
    /// Like / boost a post, or undo it (`on` false). A Mastodon account known to be read-only is
    /// refused before anything is sent.
    pub fn react(&mut self, post: &Post, kind: Reaction, on: bool) -> Result<Reacted, ActionError> {
        if self.account.may_write() == Some(false) {
            return Err(ActionError::NeedsWrite);
        }
        match &mut self.backend {
            Backend::Mastodon { token, .. } => mastodon::react(&self.account.server, token.expose(), &post.id, kind, on),
            Backend::Bluesky(api) => bluesky::react(api, post, kind, on),
        }
    }

    /// Send a new post, a reply or a quote; the post as the server made it.
    pub fn send(&mut self, draft: &Draft) -> Result<Sent, ActionError> {
        if self.account.may_write() == Some(false) {
            return Err(ActionError::NeedsWrite);
        }
        match &mut self.backend {
            Backend::Mastodon { token, .. } => mastodon::send(&self.account.server, token.expose(), draft, std::time::Duration::from_secs(1)),
            Backend::Bluesky(api) => bluesky::send(api, draft).map(|p| Sent::Posted(Box::new(p))),
        }
    }

    /// Vote in a poll (SOC-062); the answer is the poll as it is now.
    pub fn vote(&mut self, poll: &Poll, choices: &[usize]) -> Result<Poll, ActionError> {
        if self.account.may_write() == Some(false) {
            return Err(ActionError::NeedsWrite);
        }
        match &mut self.backend {
            Backend::Mastodon { token, .. } => mastodon::vote(&self.account.server, token.expose(), poll, choices),
            Backend::Bluesky(_) => Err("Bluesky has no polls".into()),
        }
    }

    /// The posts the server holds back (SOC-064).
    pub fn scheduled(&mut self) -> Result<Vec<Scheduled>, String> {
        match &mut self.backend {
            Backend::Mastodon { token, .. } => mastodon::scheduled(&self.account.server, token.expose()),
            Backend::Bluesky(_) => Err("Bluesky does not schedule posts".into()),
        }
    }

    /// Delete a scheduled post (SOC-064).
    pub fn cancel_scheduled(&mut self, id: &str) -> Result<(), ActionError> {
        if self.account.may_write() == Some(false) {
            return Err(ActionError::NeedsWrite);
        }
        match &mut self.backend {
            Backend::Mastodon { token, .. } => mastodon::cancel_scheduled(&self.account.server, token.expose(), id),
            Backend::Bluesky(_) => Err("Bluesky does not schedule posts".into()),
        }
    }

    /// What the server allows in a post.
    pub fn instance_info(&mut self) -> Result<InstanceInfo, String> {
        match &mut self.backend {
            Backend::Mastodon { token, .. } => mastodon::instance_info(&self.account.server, token.expose()),
            Backend::Bluesky(_) => Ok(InstanceInfo::defaults(Network::Bluesky)),
        }
    }
}

// ---------------------------------------------------------------- the state

/// What the view remembers of writing.
#[derive(Default)]
pub struct Writes {
    /// reactions on their way (`busy_key`)
    pub busy: std::collections::HashSet<String>,
    /// accounts whose token cannot write: the app opens their sign-in (SOC-056)
    pub needs_write: Vec<String>,
    /// what each Mastodon server allows
    pub info: HashMap<String, InstanceInfo>,
    /// the instance info of these accounts is being asked
    pub asking: std::collections::HashSet<String>,
    /// the visibility the last post had: the next one starts with it
    pub last_visibility: Visibility,
    /// (account, language) of posts that went out: the app keeps it in the account's entry so the next post starts with it (SOC-065)
    pub language_to_save: Vec<(String, String)>,
}

/// Identifies a reaction in `Writes::busy`.
pub fn busy_key(account: &str, post: &str, kind: Reaction) -> String {
    busy_key_of(if kind == Reaction::Like { "like" } else { "boost" }, account, post)
}

/// Identifies any job on a post in `Writes::busy` (`like`, `boost`, `vote`).
pub fn busy_key_of(what: &str, account: &str, post: &str) -> String {
    format!("{what}|{account}|{post}")
}

/// Apply `f` to the post when it is the one with `id`, and to the post it quotes when that is.
/// True when something was changed.
pub(super) fn patch(p: &mut Post, id: &str, f: &impl Fn(&mut Post)) -> bool {
    let mut hit = false;
    if p.id == id {
        f(p);
        hit = true;
    }
    if let Some(q) = p.quote.as_mut() {
        hit |= patch(q, id, f);
    }
    hit
}

impl SocialState {
    /// Like / boost the post, or undo it when it is already so (SOC-050). Nothing changes on
    /// screen before the server has answered. Does nothing while the same reaction is on its way.
    pub fn react(&mut self, ctx: &Context, e: &SocialAccountEntry, post: &Post, kind: Reaction) {
        let key = busy_key(&e.account.id, &post.id, kind);
        if self.writes.busy.contains(&key) {
            return;
        }
        if e.account.may_write() == Some(false) {
            self.need_write(&e.account.id);
            return;
        }
        let on = !match kind {
            Reaction::Like => post.mine.liked,
            Reaction::Boost => post.mine.boosted,
        };
        self.writes.busy.insert(key);
        let (client, src, post, account) = (self.client_source(e), self.source.clone(), post.clone(), e.account.id.clone());
        let (account2, id2) = (account.clone(), post.id.clone());
        run(
            ctx,
            &self.tx,
            move || {
                let (client, result) = match src {
                    Some(s) => (None, s.react(&post, kind, on)),
                    None => {
                        let mut c = client();
                        let r = c.react(&post, kind, on);
                        (Some(c), r)
                    }
                };
                Msg::Reacted { account, post: post.id, kind, on, client, result }
            },
            move |m| Msg::Reacted { account: account2, post: id2, kind, on, client: None, result: Err(ActionError::Failed(m)) },
        );
    }

    /// The account cannot write: say so, and let the app open its sign-in.
    pub(super) fn need_write(&mut self, account: &str) {
        self.writes.needs_write.push(account.to_string());
        self.messages.push(SIGN_IN_AGAIN.to_string());
    }

    /// A reaction ended: the post changes in every list that holds it, or a message says why not.
    pub(super) fn reacted(&mut self, account: &str, post: &str, kind: Reaction, on: bool, result: Result<Reacted, ActionError>) {
        self.writes.busy.remove(&busy_key(account, post, kind));
        match result {
            Ok(r) => {
                let apply = |p: &mut Post| {
                    if let Some(m) = &r.fresh {
                        p.mine = m.clone();
                    }
                    let (flag, record, n) = match kind {
                        Reaction::Like => (&mut p.mine.liked, &mut p.mine.like_record, &mut p.like_count),
                        Reaction::Boost => (&mut p.mine.boosted, &mut p.mine.boost_record, &mut p.boost_count),
                    };
                    *n = r.count.unwrap_or(if r.on { n.saturating_add(1) } else { n.saturating_sub(1) });
                    *flag = r.on;
                    *record = r.record.clone();
                };
                self.update_post(account, post, apply);
                // a hit reopened from the search shows the new state too (SOC-059)
                self.index.update_post(account, post, apply);
            }
            Err(ActionError::NeedsWrite) => self.need_write(account),
            Err(ActionError::Failed(m)) => {
                let network = if account.starts_with("bluesky:") { Network::Bluesky } else { Network::Mastodon };
                let what = if on { format!("{} the post", kind.verb(network)) } else { format!("undo the {}", kind.verb(network)) };
                self.messages.push(format!("Could not {what}: {m}"));
            }
        }
    }

    /// Change a post of an account everywhere it is held: the lists of its feeds, the posts
    /// waiting, and the conversations, also where it is quoted. (The search index is changed
    /// by `PostIndex::update_post`.)
    pub fn update_post(&mut self, account: &str, id: &str, f: impl Fn(&mut Post)) {
        let each = |p: &mut Post| {
            patch(p, id, &f);
        };
        for ((a, _), t) in self.timelines.iter_mut() {
            if a == account {
                t.posts.iter_mut().chain(t.waiting.iter_mut()).for_each(each);
            }
        }
        for ((a, _), entry) in self.threads.map.iter_mut() {
            if a != account {
                continue;
            }
            each(&mut entry.post);
            if let super::thread::ThreadStatus::Ready(v) = &mut entry.status {
                let v = std::sync::Arc::make_mut(v);
                v.ancestors.iter_mut().for_each(each);
                each(&mut v.post);
                v.replies.iter_mut().for_each(|r| each(&mut r.post));
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A PNG of `w` x `h` points; `noise` makes it hard to compress (a big file).
    pub fn png(w: u32, h: u32, noise: bool) -> Vec<u8> {
        let mut x = 12345u32;
        let img = image::RgbImage::from_fn(w, h, |a, b| {
            x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if noise {
                image::Rgb([(x >> 8) as u8, (x >> 16) as u8, (x >> 24) as u8])
            } else {
                image::Rgb([(a % 256) as u8, (b % 256) as u8, 128])
            }
        });
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    pub fn file(name: &str, bytes: &[u8]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shale-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    // spec: SOC-053, SEC-026
    #[test]
    fn a_picture_is_read_for_upload_and_what_is_no_picture_or_too_big_is_refused_with_words() {
        let ok = file("small.png", &png(40, 30, false));
        let p = prepare(&ok, Network::Mastodon, 16_000_000).unwrap();
        assert_eq!((p.mime, p.ext, p.bytes.len() > 100), ("image/png", "png", true));
        assert_eq!(prepare(&ok, Network::Bluesky, BLUESKY_IMAGE_BYTES).unwrap().mime, "image/png", "a small one is sent as it is");
        let text = file("note.png", b"just words, not a picture");
        assert!(prepare(&text, Network::Mastodon, 1_000_000).unwrap_err().contains("is not a JPEG, PNG, GIF or WebP picture"));
        assert!(prepare(&ok.with_file_name("missing.png"), Network::Mastodon, 1_000_000).unwrap_err().contains("cannot be read"));
        assert!(prepare(ok.parent().unwrap(), Network::Mastodon, 1_000_000).unwrap_err().contains("is not a file"));
        // Mastodon: over the limit of the server is refused with both sizes
        let big = file("big.png", &png(700, 700, true));
        let size = std::fs::metadata(&big).unwrap().len();
        assert!(size > BLUESKY_IMAGE_BYTES, "{size}");
        let e = prepare(&big, Network::Mastodon, 1_000_000).unwrap_err();
        assert!(e.contains("big.png") && e.contains("this server takes pictures up to 1.0 MB"), "{e}");
        // Bluesky: made smaller until it fits (JPEG)
        let small = prepare(&big, Network::Bluesky, BLUESKY_IMAGE_BYTES).unwrap();
        assert!(small.bytes.len() as u64 <= BLUESKY_IMAGE_BYTES && small.mime == "image/jpeg" && small.ext == "jpg", "{}", small.bytes.len());
        assert!(image::load_from_memory(&small.bytes).is_ok());
        // a GIF that is too big would lose its animation: refused
        let mut gif = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_fn(700, 700, |a, b| image::Rgb([(a * b) as u8, a as u8, b as u8])).write_to(&mut gif, image::ImageFormat::Gif).unwrap();
        let gif = file("big.gif", gif.get_ref());
        let e = prepare(&gif, Network::Bluesky, 1000).unwrap_err();
        assert!(e.contains("a GIF cannot be made smaller"), "{e}");
        // a limit that cannot be met
        assert!(prepare(&big, Network::Bluesky, 500).unwrap_err().contains("cannot be made small enough"));
    }

    // spec: SOC-053, SEC-026
    #[test]
    fn the_file_curl_reads_is_private_and_gone_after_the_upload() {
        let t = TempFile::new(b"data", "png").unwrap();
        let path = t.0.clone();
        assert_eq!(crate::fsutil::read(&path).unwrap(), b"data");
        // it lies in a folder that only this user can enter
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(path.parent().unwrap()).unwrap().permissions()) & 0o777, 0o700);
        assert!(path.to_string_lossy().ends_with(".png") && !path.to_string_lossy().contains(['"', ';', ',']));
        drop(t);
        assert!(!path.exists());
    }

    // spec: SEC-026
    #[test]
    fn a_video_copy_is_private_has_the_bytes_of_the_file_and_is_gone_when_dropped() {
        let src = file("copy-src.mp4", b"\0\0\0\x18ftypmp42video-bytes");
        let t = TempFile::copy_of(&src, "mp4").unwrap();
        let path = t.0.clone();
        assert_ne!(path, src, "a copy of its own, not the file the person named");
        assert_eq!(crate::fsutil::read(&path).unwrap(), crate::fsutil::read(&src).unwrap());
        #[cfg(unix)]
        assert_eq!(std::os::unix::fs::PermissionsExt::mode(&std::fs::metadata(path.parent().unwrap()).unwrap().permissions()) & 0o777, 0o700);
        assert!(path.to_string_lossy().ends_with(".mp4") && !path.to_string_lossy().contains(['"', ';', ',']));
        drop(t);
        assert!(!path.exists());
        assert!(src.exists(), "the person's own file stays");
    }

    // spec: SEC-026
    #[test]
    fn a_picture_over_64_mb_is_refused_without_reading_it() {
        let big = file("over-64mb.png", &png(4, 4, false));
        std::fs::OpenOptions::new().write(true).open(&big).unwrap().set_len(64_000_001).unwrap();
        for network in [Network::Mastodon, Network::Bluesky] {
            let e = prepare(&big, network, u64::MAX).unwrap_err();
            assert!(e.contains("over-64mb.png is too big (64 MB)"), "{e}");
        }
        let _ = std::fs::remove_file(&big);
    }
}
