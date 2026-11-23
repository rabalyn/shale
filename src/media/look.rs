//! What the window knows about a shown media file without touching the file system (MEDIA-024,
//! ADR-0030, #504). A frame never asks the disk anything about a file it shows: whether the file is
//! there, its modification time and whether it is a Git LFS pointer are looked up on a worker and
//! kept per path, and a picture is decoded on a worker too. Files outside the vault often sit on a
//! NAS, an SMB or NFS share or a USB disk, where one `stat` can hang for seconds.
//!
//! The frames call [`Looks::facts`] and [`Looks::picture`]: both answer at once from what was
//! looked up before, start a check when there is none or when it is older than `RECHECK`, and the
//! worker asks for a repaint when it has something new. A path has at most one check in flight. A
//! path that is looked up again after `RECHECK` keeps its old answer until the new one is there, so
//! a slow disk makes pictures neither flicker nor wait. Jobs (exports, copy-in, the vault check)
//! have no frames to protect and ask the disk themselves (`outside::resolve`).

use crate::gitsync::lfs;
use eframe::egui;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime};

/// How long an answer is trusted before the next frame asks again (in the background).
pub const RECHECK: Duration = Duration::from_secs(5);
/// How many pictures are decoded at the same time.
const DECODERS: usize = 3;
/// How many paths are remembered before the ones not shown for a while are forgotten.
const KEPT: usize = 1024;
/// How long a path that is not shown any more is remembered.
const IDLE: Duration = Duration::from_secs(120);
/// How many bytes of decoded pictures (RGBA, 4 bytes a pixel) are kept as textures before the ones
/// shown longest ago are let go (MEDIA-029): 16 pictures of 2048 × 2048, or about 32 of a
/// 1920 × 1080 screenshot.
const PICTURE_BYTES: usize = 256 << 20;
/// The longest side of a decoded picture; a larger one is scaled down with its proportions.
const MAX_SIDE: u32 = 2048;

/// What the disk says about a file that is there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Facts {
    pub is_file: bool,
    pub len: u64,
    pub mtime: Option<SystemTime>,
    /// the size of the real file when this one is a Git LFS pointer (SYNC-020)
    pub pointer: Option<u64>,
}

impl Facts {
    /// Ask the disk: None when there is nothing at `path`. This waits for the file system: only
    /// workers and jobs call it.
    pub fn of(path: &Path) -> Option<Facts> {
        let meta = std::fs::metadata(path).ok()?;
        let small = meta.is_file() && meta.len() <= 1024;
        // a pointer is a few lines; nothing to read for a real media file
        let pointer = small.then(|| crate::fsutil::read(path).ok().and_then(|b| lfs::parse_pointer(&b)).map(|p| p.size)).flatten();
        Some(Facts { is_file: meta.is_file(), len: meta.len(), mtime: meta.modified().ok(), pointer })
    }
}

/// The answer of [`Looks::facts`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Look {
    /// The first check of this path is on its way.
    Checking,
    /// What the last check found: None when nothing is there.
    Known(Option<Facts>),
}

/// A picture as the frames see it (see [`Looks::picture`]).
#[derive(Clone)]
pub enum Pic {
    /// Being looked up or decoded; the frame shows a placeholder. The size the picture will have
    /// (its pixels as the texture will be, MEDIA-028) when it is known from the file's header or
    /// from an earlier decode, else None.
    Loading(Option<egui::Vec2>),
    Ready(egui::TextureHandle),
    /// Not there, or not a picture that can be read.
    Broken,
}

impl Pic {
    pub fn ready(self) -> Option<egui::TextureHandle> {
        match self {
            Pic::Ready(t) => Some(t),
            _ => None,
        }
    }
}

type Stat = Arc<dyn Fn(&Path) -> Option<Facts> + Send + Sync>;
type Decode = Arc<dyn Fn(&Path) -> Option<egui::ColorImage> + Send + Sync>;
/// The pixels a picture will have once decoded, read from its header.
type Measure = Arc<dyn Fn(&Path) -> Option<[usize; 2]> + Send + Sync>;
type Task = Box<dyn FnOnce() + Send>;

struct Entry {
    /// the last answer of a check
    known: Option<Option<Facts>>,
    asked: Instant,
    shown: Instant,
    flying: bool,
    /// counts what made the answer stale on purpose (`Looks::forget`): a check that started before is not trusted
    epoch: u64,
}

struct Slot {
    /// the file version that was asked for (None: nothing yet, a file that is not there)
    version: Option<Option<SystemTime>>,
    loading: bool,
    pic: Pic,
    /// the size the decoded picture has (from the header or the texture), kept when the texture is let go
    size: Option<egui::Vec2>,
    /// a header read of this path is on its way
    measuring: bool,
    /// when a frame last asked for this picture: `Inner::tick` and the egui pass
    used: (u64, u64),
    /// bytes of the texture held (0 when there is none)
    bytes: usize,
}

struct Inner {
    stat: Stat,
    decode: Decode,
    measure: Measure,
    /// how many bytes of textures are kept (`PICTURE_BYTES`)
    cap: usize,
    /// counts the asks for a picture, to tell which was asked for longest ago
    tick: u64,
    recheck: Duration,
    facts: HashMap<PathBuf, Entry>,
    pictures: HashMap<PathBuf, Slot>,
    queue: VecDeque<Task>,
    decoders: usize,
}

impl Slot {
    /// What a frame is given: the picture, or `Loading` with the size known so far.
    fn shown(&self) -> Pic {
        match &self.pic {
            Pic::Loading(_) => Pic::Loading(self.size),
            other => other.clone(),
        }
    }
}

impl Inner {
    /// Lets go of textures, the ones asked for longest ago first, until at most `cap` bytes are
    /// held. A picture that a frame asked for in pass `pass` or the one before is kept (it is
    /// drawn now), and so is one that is being decoded again. The size of a picture stays, so the
    /// placeholder of one that comes back has its size at once.
    fn evict(&mut self, pass: u64) {
        let mut held: usize = self.pictures.values().map(|s| s.bytes).sum();
        while held > self.cap {
            let oldest = self.pictures.values_mut().filter(|s| s.bytes > 0 && !s.loading && s.used.1 + 1 < pass).min_by_key(|s| s.used.0);
            let Some(slot) = oldest else { return };
            held -= slot.bytes;
            slot.bytes = 0;
            slot.version = None;
            slot.pic = Pic::Loading(None);
        }
    }
}

/// The app's look-ups of media files; one per egui context (`Looks::of`), cheap to clone.
#[derive(Clone)]
pub struct Looks(Arc<Mutex<Inner>>);

impl Default for Looks {
    fn default() -> Self {
        Looks(Arc::new(Mutex::new(Inner {
            stat: Arc::new(Facts::of),
            decode: Arc::new(decode_file),
            measure: Arc::new(measure_file),
            cap: PICTURE_BYTES,
            tick: 0,
            recheck: RECHECK,
            facts: HashMap::new(),
            pictures: HashMap::new(),
            queue: VecDeque::new(),
            decoders: 0,
        })))
    }
}

/// A picture file as an image for a texture: scaled down to at most 2048 px, None if it can't be read.
fn decode_file(path: &Path) -> Option<egui::ColorImage> {
    let img = image::open(path).ok()?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

/// The pixels `decode_file` makes of a picture, from the header of the file alone (nothing is
/// decoded); None when the header cannot be read.
fn measure_file(path: &Path) -> Option<[usize; 2]> {
    let (w, h) = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_dimensions().ok()?;
    Some(capped([w, h]))
}

/// The pixels of a picture of `[w, h]` after it was scaled down to `MAX_SIDE` (as
/// `DynamicImage::thumbnail` does: the proportions stay, the sides are rounded, at least 1).
fn capped([w, h]: [u32; 2]) -> [usize; 2] {
    if w <= MAX_SIDE && h <= MAX_SIDE {
        return [w as usize, h as usize];
    }
    let k = f64::from(MAX_SIDE) / f64::from(w.max(h));
    [(f64::from(w) * k).round().max(1.0) as usize, (f64::from(h) * k).round().max(1.0) as usize]
}

impl Looks {
    /// The look-ups of this window (made on first use, kept in the context).
    pub fn of(ctx: &egui::Context) -> Looks {
        ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::unique("shale-media-looks"), Looks::default).clone())
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// What is known about the file at `path`, at once. The first time there is nothing yet
    /// (`Checking`) and a worker looks; later the answer is the last one, and one that is older
    /// than `RECHECK` is looked up again in the background. The window repaints when the answer is
    /// new.
    pub fn facts(&self, ctx: &egui::Context, path: &Path) -> Look {
        let now = Instant::now();
        let mut g = self.lock();
        if g.facts.len() >= KEPT {
            g.facts.retain(|_, e| e.flying || now.duration_since(e.shown) < IDLE);
        }
        let recheck = g.recheck;
        let e = g.facts.entry(path.to_path_buf()).or_insert(Entry { known: None, asked: now, shown: now, flying: false, epoch: 0 });
        e.shown = now;
        let start = !e.flying && (e.known.is_none() || now.duration_since(e.asked) >= recheck);
        e.flying |= start;
        let (look, epoch) = (e.known.clone().map_or(Look::Checking, Look::Known), e.epoch);
        let stat = g.stat.clone();
        drop(g);
        if start {
            let (me, path, ctx) = (self.clone(), path.to_path_buf(), ctx.clone());
            let asked = path.clone();
            crate::jobs::run(
                move || stat(&asked),
                move |found| {
                    let mut g = me.lock();
                    let Some(e) = g.facts.get_mut(&path) else { return };
                    e.flying = false;
                    // a check that started before `forget` says nothing about now: the next frame asks again
                    let changed = epoch != e.epoch || found.as_ref().is_ok_and(|f| e.known.as_ref() != Some(f));
                    if epoch == e.epoch {
                        if let Ok(f) = found {
                            e.known = Some(f);
                            e.asked = Instant::now();
                        }
                    }
                    drop(g);
                    if changed {
                        ctx.request_repaint();
                    }
                },
            );
        }
        look
    }

    /// The file at `path` was changed by Shale (a download): what is known about it is not true any
    /// more, the next frame looks again.
    pub fn forget(&self, path: &Path) {
        if let Some(e) = self.lock().facts.get_mut(path) {
            e.known = None;
            e.epoch += 1;
        }
    }

    /// The picture at `path` for a frame, at once: `Loading` while the file is looked up and
    /// decoded on workers (a few at a time), then the texture; a file that was changed is decoded
    /// again, the old picture stays until the new one is there. `Loading` carries the size the
    /// picture will have as soon as it is known: a worker reads it from the file's header before
    /// the decode (MEDIA-028), and it is kept when the texture is let go. The textures are kept up
    /// to `PICTURE_BYTES`; the ones asked for longest ago go first, never one a frame asked for in
    /// this pass or the one before.
    pub fn picture(&self, ctx: &egui::Context, path: &Path) -> Pic {
        let facts = self.facts(ctx, path);
        let mut g = self.lock();
        g.tick += 1;
        let used = (g.tick, ctx.cumulative_pass_nr());
        let (decode, measure) = (g.decode.clone(), g.measure.clone());
        let slot = g.pictures.entry(path.to_path_buf()).or_insert(Slot { version: None, loading: false, pic: Pic::Loading(None), size: None, measuring: false, used, bytes: 0 });
        slot.used = used;
        let version = match facts {
            Look::Checking => return slot.shown(),
            Look::Known(Some(f)) if f.is_file => f.mtime,
            Look::Known(_) => {
                *slot = Slot { version: None, loading: slot.loading, pic: Pic::Broken, size: None, measuring: slot.measuring, used, bytes: 0 };
                return Pic::Broken;
            }
        };
        if slot.version == Some(version) || slot.loading {
            return slot.shown();
        }
        slot.version = Some(version);
        slot.loading = true;
        if !matches!(slot.pic, Pic::Ready(_)) {
            slot.pic = Pic::Loading(None);
        }
        let shown = slot.shown();
        let measuring = !slot.measuring;
        slot.measuring = true;
        let (me, path, ctx) = (self.clone(), path.to_path_buf(), ctx.clone());
        if measuring {
            let (me, path, ctx) = (me.clone(), path.clone(), ctx.clone());
            let asked = path.clone();
            crate::jobs::run(
                move || measure(&asked),
                move |found| {
                    let size = found.ok().flatten().map(|[w, h]| egui::vec2(w as f32, h as f32));
                    let mut g = me.lock();
                    let Some(slot) = g.pictures.get_mut(&path) else { return };
                    slot.measuring = false;
                    let new = size.is_some() && slot.size != size && !matches!(slot.pic, Pic::Ready(_));
                    slot.size = size.or(slot.size);
                    drop(g);
                    if new {
                        ctx.request_repaint();
                    }
                },
            );
        }
        g.queue.push_back(Box::new(move || {
            let made = crate::jobs::catch(|| decode(&path)).ok().flatten().map(|img| ctx.load_texture(path.to_string_lossy(), img, egui::TextureOptions::LINEAR));
            let mut g = me.lock();
            if let Some(slot) = g.pictures.get_mut(&path) {
                slot.loading = false;
                slot.bytes = made.as_ref().map_or(0, |t| t.size()[0] * t.size()[1] * 4);
                slot.size = made.as_ref().map_or(slot.size, |t| Some(t.size_vec2()));
                slot.pic = made.map_or(Pic::Broken, Pic::Ready);
            }
            g.evict(ctx.cumulative_pass_nr());
            drop(g);
            ctx.request_repaint();
        }));
        let start = g.decoders < DECODERS;
        g.decoders += usize::from(start);
        drop(g);
        if start {
            let me = self.clone();
            crate::jobs::run(move || me.decode_queue(), |_| {});
        }
        shown
    }

    /// One decoder: takes the waiting pictures one after the other until there are none.
    fn decode_queue(&self) {
        loop {
            let task = {
                let mut g = self.lock();
                match g.queue.pop_front() {
                    Some(t) => t,
                    None => {
                        g.decoders -= 1;
                        return;
                    }
                }
            };
            task();
        }
    }

    /// Tests: whether a check or a decode is under way or waiting.
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        let g = self.lock();
        g.facts.values().any(|e| e.flying) || g.pictures.values().any(|s| s.loading || s.measuring)
    }

    /// Tests: how many bytes of textures are held.
    #[cfg(test)]
    pub fn held_bytes(&self) -> usize {
        self.lock().pictures.values().map(|s| s.bytes).sum()
    }

    /// Tests: keep at most `cap` bytes of textures instead of `PICTURE_BYTES`.
    #[cfg(test)]
    pub fn use_cap(&self, cap: usize) {
        self.lock().cap = cap;
    }

    /// Tests: read the size of a picture with `measure` instead of its header.
    #[cfg(test)]
    pub fn use_measure(&self, measure: impl Fn(&Path) -> Option<[usize; 2]> + Send + Sync + 'static) {
        self.lock().measure = Arc::new(measure);
    }

    /// Tests: run `frame` until no check and no decode has been under way for 3 frames in a row
    /// (the frames ask for what a view needs, and the workers answer a few frames later, MEDIA-024).
    /// The deadline is far beyond a slow, instrumented build; past it the test fails, naming this wait.
    #[cfg(test)]
    pub fn settle_with(&self, mut frame: impl FnMut()) {
        let start = Instant::now();
        let mut idle = 0;
        while idle < 3 {
            assert!(start.elapsed() < Duration::from_secs(60), "the file look-ups and picture decodes did not finish within 60 s");
            frame();
            idle = if self.busy() { 0 } else { idle + 1 };
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Tests: every answer is old enough to be looked up again by the next frame (a test changed
    /// files behind the window's back), the old answers stay until the new ones are there.
    #[cfg(test)]
    pub fn expire(&self) {
        let mut g = self.lock();
        let old = Instant::now().checked_sub(g.recheck).unwrap_or_else(Instant::now);
        g.facts.values_mut().for_each(|e| e.asked = old);
    }

    /// Tests: wait for the first answer about `path` (a test that has no frames to wait in).
    #[cfg(test)]
    pub fn wait_for(&self, ctx: &egui::Context, path: &Path) -> Option<Facts> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            if let Look::Known(f) = self.facts(ctx, path) {
                return f;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the look-up of {} did not answer", path.display());
    }

    /// Tests: ask the disk with `stat` instead of the file system.
    #[cfg(test)]
    pub fn use_stat(&self, stat: impl Fn(&Path) -> Option<Facts> + Send + Sync + 'static) {
        self.lock().stat = Arc::new(stat);
    }

    /// Tests: decode pictures with `decode` instead of the image reader.
    #[cfg(test)]
    pub fn use_decode(&self, decode: impl Fn(&Path) -> Option<egui::ColorImage> + Send + Sync + 'static) {
        self.lock().decode = Arc::new(decode);
    }

    /// Tests: trust an answer for `after` instead of `RECHECK`.
    #[cfg(test)]
    pub fn recheck_after(&self, after: Duration) {
        self.lock().recheck = after;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Condvar;

    /// Holds a worker back until the test lets it go: a disk that does not answer.
    #[derive(Default)]
    pub(crate) struct Gate {
        open: Mutex<bool>,
        changed: Condvar,
    }

    impl Gate {
        /// Wait until the gate is opened (a minute at most, so that a failed test does not hang a thread for ever).
        pub(crate) fn pass(&self) {
            let g = self.open.lock().unwrap();
            let _ = self.changed.wait_timeout_while(g, Duration::from_secs(60), |open| !*open).unwrap();
        }

        pub(crate) fn open(&self) {
            *self.open.lock().unwrap() = true;
            self.changed.notify_all();
        }
    }

    fn facts_of(len: u64) -> Option<Facts> {
        Some(Facts { is_file: true, len, mtime: None, pointer: None })
    }

    /// Frames of a window that count their repaint requests.
    fn ctx_counting_repaints() -> (egui::Context, Arc<AtomicUsize>) {
        let ctx = egui::Context::default();
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        ctx.set_request_repaint_callback(move |_| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        (ctx, n)
    }

    /// Ask until the look-up has something to say (the workers are quick when they are not held).
    fn until_known(looks: &Looks, ctx: &egui::Context, path: &Path, ok: impl Fn(&Option<Facts>) -> bool) -> Option<Facts> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            if let Look::Known(f) = looks.facts(ctx, path) {
                if ok(&f) {
                    return f;
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the look-up of {} did not answer as wanted", path.display());
    }

    fn until(what: &str, mut ok: impl FnMut() -> bool) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(60) {
            if ok() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("timed out waiting for: {what}");
    }

    // spec: MEDIA-024
    #[test]
    fn a_frame_asks_at_once_and_a_held_disk_makes_it_wait_for_nothing_with_one_check_per_path() {
        let (ctx, repaints) = ctx_counting_repaints();
        let (looks, gate, asked) = (Looks::default(), Arc::new(Gate::default()), Arc::new(AtomicUsize::new(0)));
        let (g, n) = (gate.clone(), asked.clone());
        looks.use_stat(move |_| {
            n.fetch_add(1, Ordering::Relaxed);
            g.pass();
            facts_of(7)
        });
        let (a, b) = (Path::new("/nas/a.mp4"), Path::new("/nas/b.mp4"));
        // the disk does not answer, the frames do: every call returns at once, and only one check runs for each path
        for _ in 0..200 {
            assert_eq!(looks.facts(&ctx, a), Look::Checking);
            assert_eq!(looks.facts(&ctx, b), Look::Checking);
        }
        until("both checks started", || asked.load(Ordering::Relaxed) == 2);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(asked.load(Ordering::Relaxed), 2, "one in flight per path, however many frames asked");
        assert!(looks.busy());
        let before = repaints.load(Ordering::Relaxed);
        gate.open();
        assert_eq!(until_known(&looks, &ctx, a, |_| true), facts_of(7));
        assert_eq!(until_known(&looks, &ctx, b, |_| true), facts_of(7));
        // the worker stores the answer first and wakes the window after it, so the wake may come a moment later
        until("the window is woken when the answer is there", || repaints.load(Ordering::Relaxed) > before);
        until("the checks end", || !looks.busy());
        assert_eq!(asked.load(Ordering::Relaxed), 2, "an answer that is young is trusted");
    }

    // spec: MEDIA-024
    #[test]
    fn an_old_answer_is_asked_again_in_the_background_and_stays_until_the_new_one_is_there() {
        let (ctx, _) = ctx_counting_repaints();
        let (looks, gate, asked) = (Looks::default(), Arc::new(Gate::default()), Arc::new(AtomicUsize::new(0)));
        let (g, n) = (gate.clone(), asked.clone());
        looks.use_stat(move |_| {
            // the first check is quick, the later ones wait for the gate, and the file grows
            let k = n.fetch_add(1, Ordering::Relaxed) as u64;
            if k > 0 {
                g.pass();
            }
            facts_of(10 + k)
        });
        let path = Path::new("/nas/a.png");
        assert_eq!(until_known(&looks, &ctx, path, |_| true), facts_of(10));
        looks.recheck_after(Duration::ZERO);
        // expired: the old answer is given at once while the new check waits for the disk
        for _ in 0..50 {
            assert_eq!(looks.facts(&ctx, path), Look::Known(facts_of(10)));
        }
        until("the second check started", || asked.load(Ordering::Relaxed) == 2);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(asked.load(Ordering::Relaxed), 2, "still one in flight");
        gate.open();
        assert_eq!(until_known(&looks, &ctx, path, |f| *f == facts_of(11)), facts_of(11), "the new answer replaces the old one");
    }

    // spec: MEDIA-024
    #[test]
    fn a_file_that_is_not_there_is_known_as_missing_and_is_found_when_it_appears() {
        let dir = std::env::temp_dir().join(format!("shale-look-appears-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let (ctx, _) = ctx_counting_repaints();
        let looks = Looks::default();
        let file = dir.join("a.png");
        assert_eq!(until_known(&looks, &ctx, &file, |_| true), None, "nothing there");
        std::fs::write(&file, b"x").unwrap();
        looks.recheck_after(Duration::from_millis(20));
        let found = until_known(&looks, &ctx, &file, |f| f.is_some()).unwrap();
        assert!(found.is_file && found.len == 1 && found.mtime.is_some() && found.pointer.is_none());
        std::fs::remove_file(&file).unwrap();
        assert_eq!(until_known(&looks, &ctx, &file, |f| f.is_none()), None, "gone again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MEDIA-024, SYNC-020
    #[test]
    fn the_facts_of_a_file_say_whether_it_is_a_pointer_and_what_a_folder_or_a_gap_is() {
        let dir = std::env::temp_dir().join(format!("shale-look-facts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("folder.png")).unwrap();
        let pointer = format!("version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 4242\n", "b".repeat(64));
        std::fs::write(dir.join("p.mp3"), pointer).unwrap();
        std::fs::write(dir.join("small.mp3"), b"ID3 a few bytes").unwrap();
        std::fs::write(dir.join("big.mp3"), vec![7u8; 5000]).unwrap();
        assert_eq!(Facts::of(&dir.join("p.mp3")).unwrap().pointer, Some(4242));
        assert_eq!(Facts::of(&dir.join("small.mp3")).unwrap().pointer, None, "small, but no pointer");
        let big = Facts::of(&dir.join("big.mp3")).unwrap();
        assert_eq!((big.len, big.pointer), (5000, None), "a big file is not read");
        let folder = Facts::of(&dir.join("folder.png")).unwrap();
        assert!(!folder.is_file && folder.pointer.is_none(), "a folder is not a file");
        assert_eq!(Facts::of(&dir.join("gone.png")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MEDIA-024, SYNC-020
    #[test]
    fn a_file_that_was_changed_by_shale_is_looked_at_again_and_an_older_check_does_not_count() {
        let (ctx, _) = ctx_counting_repaints();
        let (looks, gate, asked) = (Looks::default(), Arc::new(Gate::default()), Arc::new(AtomicUsize::new(0)));
        let (g, n) = (gate.clone(), asked.clone());
        looks.use_stat(move |_| {
            // the first check is a pointer that waits for the gate, the later ones are the real file
            if n.fetch_add(1, Ordering::Relaxed) == 0 {
                g.pass();
                return Some(Facts { is_file: true, len: 100, mtime: None, pointer: Some(5) });
            }
            facts_of(5)
        });
        let path = Path::new("/vault/Song.mp3");
        assert_eq!(looks.facts(&ctx, path), Look::Checking);
        until("the first check started", || asked.load(Ordering::Relaxed) == 1);
        looks.forget(path);
        gate.open();
        // the pointer the first check found is not kept: the answer comes from a check that began after the change
        assert_eq!(until_known(&looks, &ctx, path, |_| true), facts_of(5));
        assert_eq!(asked.load(Ordering::Relaxed), 2);
    }

    fn pixel() -> egui::ColorImage {
        egui::ColorImage::filled([4, 3], egui::Color32::RED)
    }

    // spec: MEDIA-024
    #[test]
    fn a_picture_is_loading_while_the_disk_or_the_decoder_is_held_and_a_texture_when_they_are_not() {
        let (ctx, _) = ctx_counting_repaints();
        let (looks, stat_gate, decode_gate) = (Looks::default(), Arc::new(Gate::default()), Arc::new(Gate::default()));
        let (sg, dg) = (stat_gate.clone(), decode_gate.clone());
        looks.use_stat(move |_| {
            sg.pass();
            facts_of(1)
        });
        looks.use_decode(move |_| {
            dg.pass();
            Some(pixel())
        });
        let path = Path::new("/nas/a.png");
        for _ in 0..100 {
            assert!(matches!(looks.picture(&ctx, path), Pic::Loading(_)), "the disk does not answer");
        }
        stat_gate.open();
        until("the disk answered", || !matches!(looks.facts(&ctx, path), Look::Checking));
        for _ in 0..100 {
            assert!(matches!(looks.picture(&ctx, path), Pic::Loading(_)), "the decoder is held");
        }
        decode_gate.open();
        until("the picture is decoded", || matches!(looks.picture(&ctx, path), Pic::Ready(_)));
        let Pic::Ready(tex) = looks.picture(&ctx, path) else { panic!("a texture") };
        assert_eq!(tex.size(), [4, 3]);
        until("the workers are done", || !looks.busy());
    }

    // spec: MEDIA-024
    #[test]
    fn a_picture_that_cannot_be_read_or_is_not_there_is_broken_and_a_changed_one_is_decoded_again() {
        let (ctx, _) = ctx_counting_repaints();
        let (looks, decoded, version) = (Looks::default(), Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (d, v) = (decoded.clone(), version.clone());
        looks.use_stat(move |p| {
            let k = v.load(Ordering::Relaxed) as u64;
            (!p.ends_with("gone.png")).then(|| Facts { is_file: true, len: 1, mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(k)), pointer: None })
        });
        looks.use_decode(move |p| {
            d.fetch_add(1, Ordering::Relaxed);
            (!p.ends_with("bad.png")).then(pixel)
        });
        let broken = |name: &str| {
            let path = PathBuf::from("/nas").join(name);
            until("a verdict", || !matches!(looks.picture(&ctx, &path), Pic::Loading(_)));
            matches!(looks.picture(&ctx, &path), Pic::Broken)
        };
        assert!(broken("bad.png"), "a file that cannot be decoded");
        assert!(broken("gone.png"), "a file that is not there");
        assert_eq!(decoded.load(Ordering::Relaxed), 1, "nothing is decoded for a file that is not there");
        // a good one is decoded once, and again when its modification time changed
        let good = Path::new("/nas/good.png");
        until("the picture", || matches!(looks.picture(&ctx, good), Pic::Ready(_)));
        until("the workers are done", || !looks.busy());
        let n = decoded.load(Ordering::Relaxed);
        for _ in 0..20 {
            assert!(matches!(looks.picture(&ctx, good), Pic::Ready(_)));
        }
        assert_eq!(decoded.load(Ordering::Relaxed), n, "once for each version of the file");
        version.store(1, Ordering::Relaxed);
        looks.recheck_after(Duration::ZERO);
        until("the changed file is decoded again", || {
            let shown = looks.picture(&ctx, good);
            assert!(matches!(shown, Pic::Ready(_)), "the old picture stays until the new one is there");
            decoded.load(Ordering::Relaxed) > n
        });
    }

    // spec: MEDIA-024
    #[test]
    fn only_a_few_pictures_are_decoded_at_the_same_time_and_all_of_them_arrive() {
        let (ctx, _) = ctx_counting_repaints();
        let (looks, gate) = (Looks::default(), Arc::new(Gate::default()));
        let (running, most) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
        let (g, r, m) = (gate.clone(), running.clone(), most.clone());
        looks.use_stat(|_| facts_of(1));
        looks.use_decode(move |_| {
            let now = r.fetch_add(1, Ordering::SeqCst) + 1;
            m.fetch_max(now, Ordering::SeqCst);
            g.pass();
            r.fetch_sub(1, Ordering::SeqCst);
            Some(pixel())
        });
        let paths: Vec<PathBuf> = (0..12).map(|i| PathBuf::from(format!("/nas/{i}.png"))).collect();
        until("the decoders are all busy", || {
            paths.iter().for_each(|p| drop(looks.picture(&ctx, p)));
            running.load(Ordering::SeqCst) == DECODERS
        });
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(most.load(Ordering::SeqCst), DECODERS, "no more than {DECODERS} at once");
        gate.open();
        until("every picture is there", || paths.iter().all(|p| matches!(looks.picture(&ctx, p), Pic::Ready(_))));
        assert_eq!(most.load(Ordering::SeqCst), DECODERS);
    }

    // spec: MEDIA-024
    #[test]
    fn a_decoder_that_panics_makes_a_broken_picture_and_the_next_picture_still_loads() {
        let (ctx, _) = ctx_counting_repaints();
        let looks = Looks::default();
        looks.use_stat(|_| facts_of(1));
        looks.use_decode(|p| {
            assert!(!p.ends_with("evil.png"), "a damaged file made the reader panic");
            Some(pixel())
        });
        let evil = Path::new("/nas/evil.png");
        until("a verdict", || !matches!(looks.picture(&ctx, evil), Pic::Loading(_)));
        assert!(matches!(looks.picture(&ctx, evil), Pic::Broken));
        let fine = Path::new("/nas/fine.png");
        until("the next picture", || matches!(looks.picture(&ctx, fine), Pic::Ready(_)));
    }

    // spec: MEDIA-024
    #[test]
    fn a_path_that_is_not_shown_any_more_is_forgotten_when_many_are_asked_for() {
        let (ctx, _) = ctx_counting_repaints();
        let looks = Looks::default();
        looks.use_stat(|_| facts_of(1));
        for i in 0..KEPT + 5 {
            looks.facts(&ctx, Path::new(&format!("/nas/{i}.png")));
        }
        until("the checks end", || !looks.busy());
        {
            let mut g = looks.lock();
            let old = Instant::now().checked_sub(IDLE + Duration::from_secs(1)).unwrap_or_else(Instant::now);
            g.facts.values_mut().for_each(|e| e.shown = old);
        }
        looks.facts(&ctx, Path::new("/nas/new.png"));
        assert_eq!(looks.lock().facts.len(), 1, "what was not shown for a while is gone, the new path is kept");
    }

    // spec: MEDIA-028
    #[test]
    fn the_size_of_a_picture_comes_from_its_header_and_is_the_size_the_decode_makes() {
        assert_eq!(capped([80, 40]), [80, 40]);
        assert_eq!(capped([2048, 2048]), [2048, 2048], "at the limit nothing is scaled");
        assert_eq!(capped([4096, 1024]), [2048, 512]);
        assert_eq!(capped([1000, 6000]), [341, 2048]);
        assert_eq!(capped([100_000, 1]), [2048, 1], "a side is never less than one pixel");
        for (w, h) in [(7_u32, 5_u32), (3000, 1000), (1000, 2600)] {
            let mut png = std::io::Cursor::new(Vec::new());
            image::RgbaImage::from_pixel(w, h, image::Rgba([9, 9, 9, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
            let path = crate::media::testdata::temp_file(&format!("size-{w}x{h}.png"), png.get_ref());
            let decoded = decode_file(&path).expect("decodes");
            assert_eq!(measure_file(&path), Some(decoded.size), "{w}x{h}: the header says what the decode makes");
        }
        let broken = crate::media::testdata::temp_file("size-broken.png", b"not a picture at all");
        assert_eq!(measure_file(&broken), None);
        assert_eq!(measure_file(Path::new("/nas/not-there.png")), None);
    }

    // spec: MEDIA-028
    #[test]
    fn a_picture_that_is_being_decoded_has_its_size_from_the_header_and_a_picture_without_one_has_none() {
        let (ctx, _) = ctx_counting_repaints();
        let (looks, gate) = (Looks::default(), Arc::new(Gate::default()));
        let g = gate.clone();
        looks.use_stat(|_| facts_of(1));
        looks.use_measure(|p| (!p.ends_with("odd.png")).then_some([640, 480]));
        looks.use_decode(move |_| {
            g.pass();
            Some(pixel())
        });
        let (a, odd) = (Path::new("/nas/a.png"), Path::new("/nas/odd.png"));
        assert!(matches!(looks.picture(&ctx, a), Pic::Loading(None)), "nothing is known in the first frame");
        until("the header is read", || matches!(looks.picture(&ctx, a), Pic::Loading(Some(_))));
        assert!(matches!(looks.picture(&ctx, a), Pic::Loading(Some(s)) if s == egui::vec2(640.0, 480.0)), "while the decoder is held");
        until("a header that cannot be read", || {
            drop(looks.picture(&ctx, odd));
            !looks.lock().pictures[odd].measuring
        });
        assert!(matches!(looks.picture(&ctx, odd), Pic::Loading(None)), "no size, the strip stays");
        gate.open();
        until("decoded", || matches!(looks.picture(&ctx, a), Pic::Ready(_)));
        until("the workers are done", || !looks.busy());
        let Pic::Ready(tex) = looks.picture(&ctx, a) else { panic!("a texture") };
        assert_eq!(tex.size(), [4, 3], "the texture is what the decode made");
    }

    /// One more egui pass: what a frame asked for in the passes before is not drawn any more.
    fn next_pass(ctx: &egui::Context) {
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    }

    // spec: MEDIA-029
    #[test]
    fn the_textures_kept_have_a_cap_in_bytes_the_oldest_go_first_and_a_picture_in_the_frame_stays() {
        let (ctx, _) = ctx_counting_repaints();
        let looks = Looks::default();
        looks.use_stat(|_| facts_of(1));
        looks.use_decode(|_| Some(pixel()));
        // a picture is 4 × 3 pixels of 4 bytes: 48 bytes; the cap holds two
        looks.use_cap(2 * 48);
        let paths: Vec<PathBuf> = (0..4).map(|i| PathBuf::from(format!("/nas/{i}.png"))).collect();
        next_pass(&ctx);
        // one frame draws four pictures: over the cap, and none of them is let go
        until("all four are decoded", || paths.iter().map(|p| looks.picture(&ctx, p)).filter(|p| matches!(p, Pic::Ready(_))).count() == 4);
        until("the workers are done", || !looks.busy());
        assert_eq!(looks.held_bytes(), 4 * 48, "what a frame draws is never let go");
        // the order they were asked for in the last frame is the order of use
        for p in &paths {
            assert!(matches!(looks.picture(&ctx, p), Pic::Ready(_)));
        }
        next_pass(&ctx);
        next_pass(&ctx);
        next_pass(&ctx);
        // a fifth picture arrives in a later frame: the four are not drawn any more, the oldest go until the cap is kept
        let fifth = Path::new("/nas/4.png");
        until("the fifth", || matches!(looks.picture(&ctx, fifth), Pic::Ready(_)));
        until("the cap is kept", || looks.held_bytes() <= 2 * 48);
        assert_eq!(looks.held_bytes(), 2 * 48, "the least recently asked for went first: two are left");
        assert!(matches!(looks.picture(&ctx, &paths[3]), Pic::Ready(_)), "the youngest of the old ones stays");
        assert!(matches!(looks.picture(&ctx, fifth), Pic::Ready(_)), "so does the one that came");
        // one that was let go comes back: its size is known at once, and it is decoded again
        assert!(matches!(looks.picture(&ctx, &paths[0]), Pic::Loading(Some(s)) if s == egui::vec2(4.0, 3.0)), "the placeholder has the size it had");
        until("decoded again", || matches!(looks.picture(&ctx, &paths[0]), Pic::Ready(_)));
    }
}
