//! The vault: a folder of markdown notes and drawings, plus an index of links, tasks and tags.

use crate::fsutil;
use crate::markdown::{self, LinkSpan, Task};
use crate::vault_path::VaultPath;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Note,
    Drawing,
    Other,
}

#[derive(Clone, Debug)]
pub struct FileEntry {
    /// Path relative to the vault root, always with '/' separators when displayed.
    pub rel: PathBuf,
    pub kind: FileKind,
    /// What links use to find this file: "Note" for Note.md, "Sketch.excalidraw" for drawings,
    /// the full file name for everything else.
    pub link_name: String,
    pub created: Option<SystemTime>,
    pub modified: Option<SystemTime>,
}

impl FileEntry {
    pub fn display_name(&self) -> String {
        match self.kind {
            FileKind::Note => self.link_name.clone(),
            FileKind::Drawing => crate::drawing::excalidraw_md::drawing_stem(&self.link_name).to_string(),
            FileKind::Other => self.link_name.clone(),
        }
    }
}

impl FileEntry {
    /// The title line of the file (VAULT-025): its name without `.md` and without a date prefix.
    pub fn title(&self) -> String {
        title_of(&self.display_name()).to_string()
    }
}

/// The length of a date prefix: `YYMMDD_`.
const DATE_PREFIX_LEN: usize = 7;

/// The date prefix a name starts with (`261117_` of `261117_Plan`): exactly six ASCII digits and `_`.
pub fn date_prefix_of(name: &str) -> Option<&str> {
    let b = name.as_bytes();
    (b.len() >= DATE_PREFIX_LEN && b[..6].iter().all(u8::is_ascii_digit) && b[6] == b'_').then(|| &name[..DATE_PREFIX_LEN])
}

/// The title of a note called `name` (VAULT-025): without `.md` and without a leading `YYMMDD_`.
/// `261117_Plan.md` → `Plan`; `12345_Plan` and `1234567_Plan` have none to strip. A name that is
/// only a prefix keeps it.
pub fn title_of(name: &str) -> &str {
    let name = markdown::strip_md_ext(name);
    match date_prefix_of(name) {
        Some(p) if name.len() > p.len() => &name[p.len()..],
        _ => name,
    }
}

/// Today's date prefix for a new note, `YYMMDD_` in local time (CFG-043).
pub fn date_prefix_today() -> String {
    let (y, m, d) = crate::media::local_date();
    format!("{:02}{m:02}{d:02}_", y.rem_euclid(100))
}

/// What a file looked like on disk when it was last read: modification time, size, and (on
/// Unix) the inode and the inode change time. The time alone is not enough: two writes inside
/// one timestamp tick of the file system (coarse on some systems) leave it the same, and an
/// outside change would be missed (DATA-002). An atomic replace gets a new inode, and any write
/// moves the change time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FileStamp {
    modified: Option<SystemTime>,
    len: u64,
    #[cfg(unix)]
    ino: u64,
    #[cfg(unix)]
    ctime: (i64, i64),
    /// Windows: the volume and file index (no inode there)
    #[cfg(windows)]
    id: Option<(u32, u64)>,
}

impl FileStamp {
    /// None when the file's metadata cannot be read (then it is always read again).
    pub fn of(path: &Path) -> Option<FileStamp> {
        let m = fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(FileStamp {
            modified: m.modified().ok(),
            len: m.len(),
            #[cfg(unix)]
            ino: m.ino(),
            #[cfg(unix)]
            ctime: (m.ctime(), m.ctime_nsec()),
            #[cfg(windows)]
            id: crate::fsutil::file_identity(path),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoteIndex {
    /// The file as it was when `text` was read; None = read it again at the next scan.
    pub mtime: Option<FileStamp>,
    pub text: String,
    pub links: Vec<LinkSpan>,
    /// Resolution of each link in `links` (same order).
    pub resolved: Vec<Option<PathBuf>>,
    pub tasks: Vec<Task>,
    pub tags: Vec<String>,
    /// Why the file could not be read (invalid UTF-8, no permission). `text` is empty then, and
    /// the file is never written (DATA-008).
    pub unreadable: Option<String>,
}

/// A fingerprint of a file's content (length and a 64-bit hash). A buffer remembers the stamp of
/// what it loaded or saved last; a different stamp on disk at save time means another program
/// changed the file (DATA-003). Content, not the modification time, so a sync tool that only
/// touches a file causes no conflict, and a change within the same clock tick is still seen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stamp {
    len: u64,
    hash: u64,
}

impl Stamp {
    pub fn of_bytes(bytes: &[u8]) -> Stamp {
        // FNV-1a, 64 bit
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Stamp { len: bytes.len() as u64, hash: h }
    }

    pub fn of(text: &str) -> Stamp {
        Stamp::of_bytes(text.as_bytes())
    }
}

/// The result of [`Vault::save_checked`].
#[derive(Debug)]
pub enum Saved {
    /// The file now holds the text (or already did).
    Written(Stamp),
    /// The file had been changed by another program. The buffer's text is in `copy`; the other
    /// program's version stays at the path and is `theirs` (`Err` with the reason when it is not
    /// readable text).
    Conflict { copy: PathBuf, theirs: Result<String, String> },
}

/// Write `bytes` as `Name (conflict YYYY-MM-DD HHMM).ext` next to `rel` (under `root`), with a
/// number added when the name is taken. Never replaces a file. Returns the path (relative).
pub fn write_conflict_copy(root: &Path, rel: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let stamp = crate::gitsync::stamp_now();
    let mut tried: Vec<PathBuf> = Vec::new();
    loop {
        let copy = crate::gitsync::conflict_name(rel, &stamp, |p| root.join(p).exists() || tried.contains(&p.to_path_buf()));
        match fsutil::create_new(&root.join(&copy), bytes) {
            Ok(()) => return Ok(copy),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => tried.push(copy),
            Err(e) => return Err(e),
        }
    }
}

#[derive(Default)]
pub struct Vault {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    pub folders: Vec<PathBuf>,
    /// (created, modified) of each folder, for sorting the file tree.
    pub folder_times: HashMap<PathBuf, (Option<SystemTime>, Option<SystemTime>)>,
    pub notes: HashMap<PathBuf, NoteIndex>,
    /// Links of drawings and canvases: `text` holds one line per element text or link that has a
    /// link (see `drawing::link_lines`); `tasks` and `tags` stay empty.
    pub drawings: HashMap<PathBuf, NoteIndex>,
    name_index: HashMap<String, Vec<PathBuf>>,
    /// Lowercase vault path (`projects/plan.md` and `projects/plan`) -> file.
    path_index: HashMap<String, PathBuf>,
    /// Path -> position in `files`, for [`Vault::file`] (PERF-002). Built with the name index, which
    /// is rebuilt whenever the set of files changes; a rescan that only changes times keeps the order.
    file_pos: HashMap<PathBuf, usize>,
    /// Bumped whenever anything in the index changes; UI caches key off this (with `uid`, which
    /// tells one opened vault from another).
    pub generation: u64,
    /// A number of its own for every vault that was opened in this run.
    pub uid: u64,
    /// Notes the last rename or move could not update: (path after the change, reason). Reset by
    /// every rename and move; the notes that were updated are the call's result (VAULT-016).
    pub failed_rewrites: Vec<(PathBuf, String)>,
}

/// From this many notes on, a vault is parsed on several threads.
const PARALLEL_FROM: usize = 256;

/// The index of one note's text, made apart from the vault so that notes can be parsed together.
#[derive(Default)]
struct Parsed {
    links: Vec<LinkSpan>,
    tasks: Vec<Task>,
    tags: Vec<String>,
    resolved: Vec<Option<PathBuf>>,
}

impl Parsed {
    fn store(self, n: &mut NoteIndex) {
        n.links = self.links;
        n.tasks = self.tasks;
        n.tags = self.tags;
        n.resolved = self.resolved;
    }
}

/// Replace the targets of links in `text`: each (old target text, new target text) pair.
fn replace_link_targets(text: &str, pairs: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for l in markdown::find_links(text).into_iter().rev() {
        if let Some((_, new)) = pairs.iter().find(|(old, _)| *old == text[l.target_range.clone()]) {
            out.replace_range(l.target_range, new);
        }
    }
    out
}

/// What a rename or move did to links elsewhere.
#[derive(Debug, Default)]
pub struct LinkUpdate {
    /// notes and drawings whose links were rewritten (their new paths)
    pub changed: Vec<PathBuf>,
    /// canvases that still link to the old name or place (canvases are never written)
    pub canvases: Vec<PathBuf>,
}

/// The key the file tree and the lists sort paths by: the path in lower case. Sort with
/// `sort_by_cached_key`, so the string is made once per element and not once per comparison.
pub fn sort_key(p: &Path) -> String {
    rel_to_string(p).to_lowercase()
}

pub fn rel_to_string(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
}

/// The vault's trash folder. Shale never empties it.
pub const TRASH_DIR: &str = ".trash";

/// `rel` with ` (n)` before the extension of its name (a folder: at the end); `n` 1 is `rel`.
fn numbered(rel: &Path, n: u32, is_dir: bool) -> PathBuf {
    if n <= 1 {
        return rel.to_path_buf();
    }
    let Some(name) = rel.file_name().map(|s| s.to_string_lossy().into_owned()) else { return rel.to_path_buf() };
    let ext = rel.extension().map(|e| e.to_string_lossy().into_owned()).filter(|_| !is_dir);
    let new = match ext {
        Some(e) => format!("{} ({n}).{e}", &name[..name.len() - e.len() - 1]),
        None => format!("{name} ({n})"),
    };
    rel.with_file_name(new)
}

/// Where `.trash/<YYYY-MM-DD>/<path>` came from: `<path>`. None for anything else (an old
/// entry directly in `.trash/`, a path outside the trash).
pub fn trash_origin(trash_rel: &Path) -> Option<PathBuf> {
    let mut c = trash_rel.components();
    if c.next()?.as_os_str() != TRASH_DIR {
        return None;
    }
    let day = c.next()?.as_os_str().to_str()?;
    let b = day.as_bytes();
    if b.len() != 10 || !b.iter().enumerate().all(|(i, x)| if i == 4 || i == 7 { *x == b'-' } else { x.is_ascii_digit() }) {
        return None;
    }
    let rest = c.as_path();
    (!rest.as_os_str().is_empty()).then(|| rest.to_path_buf())
}

/// [`Vault::trash`] for a vault folder that is not held as a `Vault` (a background job of the TeX
/// template sets, TEX-006): the move into `.trash/<date>/`, without any index to bring up to date.
pub fn move_to_trash(root: &Path, rel: &Path) -> io::Result<PathBuf> {
    let checked = VaultPath::new(rel).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("“{}”: {e}", rel.display())))?;
    if checked.as_path().as_os_str().is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "that is the vault itself"));
    }
    let day = Path::new(TRASH_DIR).join(crate::mail::store::today_string());
    let is_dir = fs::symlink_metadata(root.join(rel)).map(|m| m.is_dir()).unwrap_or(false);
    for n in 1..=1000 {
        let dest = day.join(numbered(rel, n, is_dir));
        if let Some(parent) = root.join(&dest).parent() {
            fs::create_dir_all(parent)?;
        }
        match fsutil::rename_noreplace(&root.join(rel), &root.join(&dest)) {
            Ok(()) => return Ok(dest),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other("there are too many files with that name in today's trash"))
}

/// A note with search hits: (note, [(line, byte offset of the match in the line, line text)]).
pub type SearchHit = (PathBuf, Vec<(usize, usize, String)>);

/// For one note, during a rename or move: its path after the change, its text, and each link
/// that resolved with the file it must point to after the change.
type RewritePlan = (PathBuf, String, Vec<(LinkSpan, PathBuf)>);

/// Does a link target start with `./` or `../` (relative to the linking note)?
fn is_relative_link(target: &str) -> bool {
    target.starts_with("./") || target.starts_with("../")
}

/// `base` joined with the `/`-separated `target`, with `.` and `..` worked out, as a
/// `/`-separated string. `None` if it climbs above the vault root.
pub fn normalize_path(base: &Path, target: &str) -> Option<String> {
    let mut parts: Vec<String> = base.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    for p in target.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p.to_string()),
        }
    }
    Some(parts.join("/"))
}

/// The way from the folder `from_dir` to `to`, written with `../` (or `./` when it goes down).
fn relative_link(from_dir: &Path, to: &Path) -> String {
    let a: Vec<_> = from_dir.components().collect();
    let b: Vec<_> = to.components().collect();
    let same = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut out: Vec<String> = vec!["..".into(); a.len() - same];
    out.extend(b[same..].iter().map(|c| c.as_os_str().to_string_lossy().into_owned()));
    let s = out.join("/");
    if s.starts_with("../") {
        s
    } else {
        format!("./{s}")
    }
}

/// `edits` (range in `text` -> new text) applied to `text`; ranges that don't fit are skipped.
fn apply_edits(text: &str, mut edits: Vec<(std::ops::Range<usize>, String)>) -> String {
    edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
    let mut out = text.to_string();
    for (r, t) in edits {
        if r.end <= out.len() && out.is_char_boundary(r.start) && out.is_char_boundary(r.end) {
            out.replace_range(r, &t);
        }
    }
    out
}

pub fn kind_of(path: &Path) -> FileKind {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if crate::drawing::excalidraw_md::is_drawing_path(path) {
        FileKind::Drawing
    } else if name.ends_with(".md") {
        FileKind::Note
    } else {
        FileKind::Other
    }
}

fn link_name_of(path: &Path, kind: FileKind) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match kind {
        FileKind::Note => markdown::strip_md_ext(&name).to_string(),
        // Obsidian links to `Plan.excalidraw.md` as `[[Plan.excalidraw]]`
        FileKind::Drawing => crate::drawing::excalidraw_md::strip_suffix_ci(&name, ".md").unwrap_or(&name).to_string(),
        _ => name,
    }
}

impl Vault {
    pub fn open(root: &Path) -> io::Result<Vault> {
        if !root.is_dir() {
            fs::create_dir_all(root)?;
        }
        static NEXT_UID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let mut v = Vault { root: root.to_path_buf(), uid: NEXT_UID.fetch_add(1, std::sync::atomic::Ordering::Relaxed), ..Default::default() };
        v.rescan();
        Ok(v)
    }

    pub fn abs(&self, rel: &Path) -> PathBuf {
        self.root.join(rel)
    }

    /// The path of `rel` on disk, for something that changes files: an error unless `rel` stays
    /// inside the vault (no `..`, not absolute), as a last guard behind VAULT-018.
    fn abs_checked(&self, rel: &Path) -> io::Result<PathBuf> {
        let checked = VaultPath::new(rel).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, format!("“{}”: {e}", rel.display())))?;
        if checked.as_path().as_os_str().is_empty() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "that is the vault itself"));
        }
        Ok(self.root.join(rel))
    }

    /// Walk the folder again. Files whose modification time did not change are not re-read.
    /// Returns the notes whose content changed on disk.
    pub fn rescan(&mut self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        let mut folders = Vec::new();
        walk(&self.root, Path::new(""), &mut files, &mut folders);
        self.folder_times = folders
            .iter()
            .map(|f: &PathBuf| {
                let m = fs::metadata(self.root.join(f)).ok();
                let modified = m.as_ref().and_then(|m| m.modified().ok());
                (f.clone(), (m.as_ref().and_then(|m| m.created().ok()).or(modified), modified))
            })
            .collect();
        files.sort_by_cached_key(|f: &FileEntry| sort_key(&f.rel));
        folders.sort_by_cached_key(|f| sort_key(f));
        let mut changed = Vec::new();
        let mut seen = HashSet::new();
        let mut drawings_changed = Vec::new();
        let mut seen_drawings = HashSet::new();
        // the same paths in the same places: the position index of `file` stays right
        let mut structure_changed = files.len() != self.files.len() || folders != self.folders || files.iter().zip(&self.files).any(|(a, b)| a.rel != b.rel);
        for f in &files {
            if f.kind == FileKind::Drawing {
                seen_drawings.insert(f.rel.clone());
                if self.index_drawing(&f.rel) {
                    drawings_changed.push(f.rel.clone());
                }
                continue;
            }
            if f.kind != FileKind::Note {
                continue;
            }
            seen.insert(f.rel.clone());
            let abs = self.root.join(&f.rel);
            let mtime = FileStamp::of(&abs);
            // a note that could not be read is tried again every time (the cause may be gone)
            let up_to_date = self.notes.get(&f.rel).map(|n| n.mtime == mtime && mtime.is_some() && n.unreadable.is_none()).unwrap_or(false);
            if !up_to_date {
                // never an empty text for a file that could not be read (DATA-008)
                let (text, unreadable) = match fsutil::read_to_string(&abs) {
                    Ok(t) => (t, None),
                    Err(e) => (String::new(), Some(e.to_string())),
                };
                let differs = self.notes.get(&f.rel).map(|n| n.text != text || n.unreadable != unreadable).unwrap_or(true);
                let entry = self.notes.entry(f.rel.clone()).or_default();
                entry.mtime = mtime;
                if differs {
                    entry.text = text;
                    entry.unreadable = unreadable;
                    changed.push(f.rel.clone());
                }
            }
        }
        let before = self.notes.len() + self.drawings.len();
        self.notes.retain(|k, _| seen.contains(k));
        self.drawings.retain(|k, _| seen_drawings.contains(k));
        if before != self.notes.len() + self.drawings.len() {
            structure_changed = true;
        }
        self.files = files;
        self.folders = folders;
        if structure_changed || !changed.is_empty() || !drawings_changed.is_empty() {
            self.rebuild_name_index();
            if structure_changed {
                self.reparse_all();
            } else {
                for c in &changed {
                    self.reparse(c);
                }
                for c in &drawings_changed {
                    self.reparse_drawing(c);
                }
            }
            self.generation += 1;
        }
        changed
    }

    fn rebuild_name_index(&mut self) {
        self.name_index.clear();
        self.path_index.clear();
        self.file_pos = self.files.iter().enumerate().map(|(i, f)| (f.rel.clone(), i)).collect();
        for f in &self.files {
            self.name_index.entry(f.link_name.to_lowercase()).or_default().push(f.rel.clone());
            // also allow the full file name (`Note.md`, `Plan.excalidraw.md`)
            let full = f.rel.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if full != f.link_name.to_lowercase() {
                self.name_index.entry(full).or_default().push(f.rel.clone());
            }
            let path = rel_to_string(&f.rel).to_lowercase();
            if let Some(short) = crate::drawing::excalidraw_md::strip_suffix_ci(&path, ".md") {
                // an exact file name wins over `X.md` written without its `.md`
                self.path_index.entry(short.to_string()).or_insert_with(|| f.rel.clone());
            }
            self.path_index.insert(path, f.rel.clone());
        }
    }

    fn reparse_all(&mut self) {
        // the notes are independent: many of them are parsed on several threads (PERF-008)
        let mut keys: Vec<&PathBuf> = self.notes.keys().collect();
        let threads = if keys.len() >= PARALLEL_FROM { std::thread::available_parallelism().map_or(1, |n| n.get()).min(8) } else { 1 };
        let chunk = keys.len().div_ceil(threads).max(1);
        let this = &*self;
        let parsed: Vec<(PathBuf, Parsed)> = if threads == 1 {
            keys.iter().map(|k| ((*k).clone(), this.parse_note(k))).collect()
        } else {
            std::thread::scope(|scope| {
                let handles: Vec<_> = keys.chunks_mut(chunk).map(|part| scope.spawn(move || part.iter().map(|k| ((*k).clone(), this.parse_note(k))).collect::<Vec<_>>())).collect();
                handles.into_iter().flat_map(|h| h.join().expect("a parsing thread")).collect()
            })
        };
        for (rel, p) in parsed {
            if let Some(n) = self.notes.get_mut(&rel) {
                p.store(n);
            }
        }
        let keys: Vec<PathBuf> = self.drawings.keys().cloned().collect();
        for k in keys {
            self.reparse_drawing(&k);
        }
    }

    /// Read a drawing's links again if the file changed. Returns true if its links changed.
    fn index_drawing(&mut self, rel: &Path) -> bool {
        let abs = self.root.join(rel);
        let mtime = FileStamp::of(&abs);
        if self.drawings.get(rel).is_some_and(|d| d.mtime == mtime && mtime.is_some()) {
            return false;
        }
        // an unreadable drawing simply has no links
        let text = fsutil::read_to_string(&abs).ok().and_then(|t| crate::drawing::Scene::from_file_text(rel, &t).ok()).map(|s| crate::drawing::link_lines(&s)).unwrap_or_default();
        let entry = self.drawings.entry(rel.to_path_buf()).or_default();
        entry.mtime = mtime;
        let differs = entry.text != text;
        entry.text = text;
        differs
    }

    fn reparse_drawing(&mut self, rel: &Path) {
        let Some(d) = self.drawings.get(rel) else { return };
        let links = markdown::find_links(&d.text);
        let resolved = links.iter().map(|l| self.resolve(&l.target, Some(rel))).collect();
        let d = self.drawings.get_mut(rel).unwrap();
        d.links = links;
        d.resolved = resolved;
    }

    /// Notes and drawings with their link index.
    fn linkers(&self) -> impl Iterator<Item = (&PathBuf, &NoteIndex)> {
        self.notes.iter().chain(self.drawings.iter())
    }

    fn reparse(&mut self, rel: &Path) {
        let parsed = self.parse_note(rel);
        if let Some(n) = self.notes.get_mut(rel) {
            parsed.store(n);
        }
    }

    /// What the index holds of the text of the note `rel`: its links (and what they resolve to),
    /// tasks and tags.
    fn parse_note(&self, rel: &Path) -> Parsed {
        let Some(n) = self.notes.get(rel) else { return Parsed::default() };
        let links = markdown::find_links(&n.text);
        let tasks = markdown::find_tasks(&n.text);
        let mut tags: Vec<String> = markdown::find_tags(&n.text).into_iter().map(|t| t.1).collect();
        tags.extend(markdown::frontmatter_tags(&n.text));
        tags.sort();
        tags.dedup();
        let resolved = links.iter().map(|l| self.resolve(&l.target, Some(rel))).collect();
        Parsed { links, tasks, tags, resolved }
    }

    /// Update the index from an editor buffer (the file itself is written separately).
    pub fn set_note_text(&mut self, rel: &Path, text: &str) {
        let n = self.notes.entry(rel.to_path_buf()).or_default();
        if n.text == text {
            return;
        }
        n.text = text.to_string();
        self.reparse(rel);
        self.generation += 1;
    }

    pub fn file(&self, rel: &Path) -> Option<&FileEntry> {
        self.file_pos.get(rel).and_then(|&i| self.files.get(i))
    }

    /// Find the file a link target points to. `from` is the note containing the link.
    ///
    /// A target without `/` is a name (LINK-002). A target with `/` is a path (LINK-010): from the
    /// vault root, or from the linking note when it starts with `./` or `../`; the linking note's
    /// folder is tried as a base only when the root path does not exist. A path never falls back
    /// to the name.
    pub fn resolve(&self, target: &str, from: Option<&Path>) -> Option<PathBuf> {
        let target = target.trim();
        if target.is_empty() {
            return from.map(|p| p.to_path_buf());
        }
        let from_dir = from.and_then(|f| f.parent()).unwrap_or(Path::new(""));
        if target.contains('/') {
            let find = |p: Option<String>| p.and_then(|p| self.path_index.get(&p.to_lowercase()).cloned());
            if is_relative_link(target) {
                return find(normalize_path(from_dir, target));
            }
            let t = target.trim_start_matches('/');
            return find(normalize_path(Path::new(""), t)).or_else(|| if from.is_some() && !target.starts_with('/') { find(normalize_path(from_dir, t)) } else { None });
        }
        let cands = self.name_index.get(&target.to_lowercase())?;
        if cands.len() == 1 {
            return Some(cands[0].clone());
        }
        let mut best = cands.clone();
        best.sort_by_key(|c| {
            let same = c.parent().map(|p| p == from_dir).unwrap_or(false);
            (!same, c.components().count(), rel_to_string(c))
        });
        best.into_iter().next()
    }

    /// For a link by name that several files share: how many, and the one it opens (LINK-008).
    pub fn ambiguity(&self, target: &str, from: Option<&Path>) -> Option<(usize, PathBuf)> {
        let t = target.trim();
        if t.is_empty() || t.contains('/') {
            return None;
        }
        let n = self.name_index.get(&t.to_lowercase())?.len();
        if n < 2 {
            return None;
        }
        Some((n, self.resolve(t, from)?))
    }

    /// Files a red path link might have meant: those whose name is the link's last part
    /// (LINK-012). Empty for links that resolve or are not paths.
    pub fn fix_candidates(&self, target: &str, from: Option<&Path>) -> Vec<PathBuf> {
        let t = target.trim();
        if !t.contains('/') || self.resolve(t, from).is_some() {
            return vec![];
        }
        let name = t.rsplit('/').next().unwrap_or(t);
        let mut c = self.name_index.get(&name.to_lowercase()).cloned().unwrap_or_default();
        c.sort_by_cached_key(|p| sort_key(p));
        c
    }

    /// Extra line for a link's tooltip: the name is shared (LINK-008), or a red path link has
    /// several possible targets (LINK-012).
    pub fn link_hint(&self, target: &str, from: Option<&Path>) -> Option<String> {
        let what = |paths: &[PathBuf]| if paths.iter().all(|p| self.file(p).map(|f| f.kind == FileKind::Note).unwrap_or(false)) { "notes" } else { "files" };
        if let Some((n, opens)) = self.ambiguity(target, from) {
            let all = self.name_index.get(&target.trim().to_lowercase()).cloned().unwrap_or_default();
            let shown = self.file(&opens).map(|f| markdown::strip_md_ext(&rel_to_string(&f.rel)).to_string()).unwrap_or_default();
            return Some(format!("{n} {} are called “{}” – opens {shown}", what(&all), target.trim()));
        }
        let c = self.fix_candidates(target, from);
        if c.len() > 1 {
            let name = target.trim().rsplit('/').next().unwrap_or("");
            return Some(format!("Nothing at this path; {} {} are called “{name}” – pick one with “Fix link” in Outgoing links", c.len(), what(&c)));
        }
        None
    }

    pub fn exists(&self, target: &str, from: Option<&Path>) -> bool {
        self.resolve(target, from).is_some()
    }

    /// Notes and drawings linking to `rel`: (source, [(line number, line text)]). For a drawing
    /// the line is the text of the element with the link.
    pub fn backlinks(&self, rel: &Path) -> Vec<(PathBuf, Vec<(usize, String)>)> {
        let mut out = Vec::new();
        for (src, n) in self.linkers() {
            let mut lines = Vec::new();
            for (l, r) in n.links.iter().zip(n.resolved.iter()) {
                if r.as_deref() == Some(rel) && src != rel {
                    let line_no = n.text[..l.range.start].matches('\n').count();
                    let line = n.text.lines().nth(line_no).unwrap_or("").trim().to_string();
                    if !lines.iter().any(|(ln, _)| *ln == line_no) {
                        lines.push((line_no, line));
                    }
                }
            }
            if !lines.is_empty() {
                out.push((src.clone(), lines));
            }
        }
        out.sort_by_cached_key(|(p, _)| sort_key(p));
        out
    }

    /// Notes that mention the name of `rel` in plain text without linking it.
    pub fn unlinked_mentions(&self, rel: &Path) -> Vec<(PathBuf, Vec<(usize, String)>)> {
        let Some(f) = self.file(rel) else { return vec![] };
        let name = f.display_name().to_lowercase();
        if name.len() < 3 {
            return vec![];
        }
        let mut out = Vec::new();
        for (src, n) in &self.notes {
            if src == rel {
                continue;
            }
            // most notes do not have the name anywhere: one pass over an ASCII text finds that out
            if n.text.is_ascii() && !n.text.to_ascii_lowercase().contains(&name) {
                continue;
            }
            let link_ranges: Vec<_> = n.links.iter().map(|l| l.range.clone()).collect();
            let mut lines = Vec::new();
            for (li, (off, line)) in markdown::lines_with_offsets(&n.text).into_iter().enumerate() {
                let lower = line.to_lowercase();
                if lower.len() != line.len() {
                    // non-ASCII case folding changed lengths; fall back to a simple contains check
                    if lower.contains(&name) && !n.links.iter().any(|l| l.range.start >= off && l.range.end <= off + line.len()) {
                        lines.push((li, line.trim().to_string()));
                    }
                    continue;
                }
                let mut start = 0;
                while let Some(p) = lower[start..].find(&name) {
                    let abs = off + start + p;
                    if !link_ranges.iter().any(|r| r.contains(&abs)) {
                        lines.push((li, line.trim().to_string()));
                        break;
                    }
                    start += p + name.len();
                }
            }
            if !lines.is_empty() {
                out.push((src.clone(), lines));
            }
        }
        out.sort_by_cached_key(|(p, _)| sort_key(p));
        out
    }

    /// Outgoing links of a note or drawing: (display text, target, resolved path).
    pub fn outgoing(&self, rel: &Path) -> Vec<(String, String, Option<PathBuf>)> {
        let Some(n) = self.notes.get(rel).or_else(|| self.drawings.get(rel)) else { return vec![] };
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for (l, r) in n.links.iter().zip(n.resolved.iter()) {
            if l.target.is_empty() {
                continue;
            }
            if seen.insert(l.target.to_lowercase()) {
                out.push((l.display(), l.target.clone(), r.clone()));
            }
        }
        out
    }

    /// Every task in every note, sorted by note path.
    pub fn all_tasks(&self) -> Vec<(PathBuf, Task)> {
        let mut notes: Vec<(String, &PathBuf, &NoteIndex)> = self.notes.iter().filter(|(_, n)| !n.tasks.is_empty()).map(|(p, n)| (sort_key(p), p, n)).collect();
        notes.sort_by(|a, b| a.0.cmp(&b.0));
        let mut out = Vec::with_capacity(notes.iter().map(|n| n.2.tasks.len()).sum());
        for (_, p, n) in notes {
            let first = out.len();
            out.extend(n.tasks.iter().map(|t| (p.clone(), t.clone())));
            out[first..].sort_by_key(|(_, t): &(PathBuf, Task)| t.line);
        }
        out
    }

    pub fn all_tags(&self) -> Vec<(String, usize)> {
        let mut m: HashMap<String, usize> = HashMap::new();
        for n in self.notes.values() {
            for t in &n.tags {
                *m.entry(t.clone()).or_default() += 1;
            }
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v
    }

    /// Full-text search, ignoring case: (note, [(line, byte offset of the match in that line,
    /// the line's text)]), at most 20 lines per note.
    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() {
            return vec![];
        }
        let mut out = Vec::new();
        for f in &self.files {
            let name_hit = f.display_name().to_lowercase().contains(&q);
            let mut lines = Vec::new();
            if let Some(n) = self.notes.get(&f.rel) {
                for (i, line) in n.text.lines().enumerate() {
                    if let Some(col) = markdown::find_ci(line, &q) {
                        lines.push((i, col, line.trim().to_string()));
                        if lines.len() >= 20 {
                            break;
                        }
                    }
                }
            }
            if name_hit || !lines.is_empty() {
                out.push((f.rel.clone(), lines));
            }
        }
        out
    }

    /// Pick a path for a new note called `name` (may contain folders) inside `folder`.
    /// The result is always inside the vault (VAULT-018): `..` and empty parts of `name` are
    /// dropped, and a `folder` outside the vault is replaced by the vault root. Each part is made
    /// portable (VAULT-027) and the file name is at most 200 bytes.
    pub fn new_note_path(&self, folder: &Path, name: &str) -> PathBuf {
        self.new_note_path_max(folder, name, fsutil::TYPED_NAME_MAX)
    }

    /// [`Vault::new_note_path`] with the file name (extension and a number included) cut to
    /// `max_bytes`: 127 for names Shale makes itself (VAULT-027).
    pub fn new_note_path_max(&self, folder: &Path, name: &str, max_bytes: usize) -> PathBuf {
        let folder = VaultPath::new(folder).unwrap_or_default();
        let folder = folder.as_path();
        let name = name.trim().trim_start_matches('/');
        // the file name is cut below, when the extension and a number are known
        let count = name.split('/').count();
        let mut parts: Vec<String> = name.split('/').enumerate().map(|(i, p)| markdown::sanitize_note_name_max(p, if i + 1 == count { usize::MAX } else { fsutil::TYPED_NAME_MAX })).filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            parts.push("Untitled".into());
        }
        let file = parts.pop().unwrap();
        let mut dir = if name.contains('/') { PathBuf::new() } else { folder.to_path_buf() };
        for p in parts {
            dir.push(p);
        }
        let (stem, ext) = if let Some(s) = crate::drawing::excalidraw_md::strip_suffix_ci(&file, ".excalidraw.md") {
            (s.to_string(), "excalidraw.md")
        } else if let Some(s) = crate::drawing::excalidraw_md::strip_suffix_ci(&file, ".excalidraw") {
            (s.to_string(), "excalidraw")
        } else {
            (markdown::strip_md_ext(&file).to_string(), "md")
        };
        // the stem is cut so that the number and the extension still fit
        let fit = |suffix: &str| {
            let budget = max_bytes.saturating_sub(ext.len() + 1 + suffix.len());
            let stem = fsutil::portable_name(&stem, budget);
            let stem = if stem.is_empty() { "Untitled".to_string() } else { stem };
            dir.join(format!("{stem}{suffix}.{ext}"))
        };
        let mut candidate = fit("");
        let mut i = 1;
        while self.abs(&candidate).exists() {
            candidate = fit(&format!(" {i}"));
            i += 1;
        }
        candidate
    }

    /// Create a file with content; makes parent folders. Never replaces an existing file
    /// (`AlreadyExists`). Returns its relative path.
    pub fn create_file(&mut self, rel: &Path, content: &str) -> io::Result<PathBuf> {
        let abs = self.abs_checked(rel)?;
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p)?;
        }
        fsutil::create_new(&abs, content.as_bytes())?;
        self.rescan();
        Ok(rel.to_path_buf())
    }

    pub fn create_folder(&mut self, rel: &VaultPath) -> io::Result<()> {
        fs::create_dir_all(self.abs(rel.as_path()))?;
        self.rescan();
        Ok(())
    }

    /// Save a buffer (a note, or a drawing's file text) whose last known disk content was `base`
    /// (DATA-003, DATA-008). The file is read first:
    /// - it can't be read (invalid UTF-8 is fine for the check, a permission error is not): an
    ///   error, nothing is written, so bytes Shale could not read are never replaced;
    /// - it is missing, unchanged since `base`, or already holds `text`: the text is written;
    /// - it was changed by another program: the buffer goes to a conflict copy next to it, the
    ///   other version stays at the path (`Saved::Conflict`), nothing is overwritten.
    ///
    /// The index follows for notes. (Another program can still write between the check and the
    /// rename; that window is microseconds, and the loser is then a conflict at the next save.)
    pub fn save_checked(&mut self, rel: &Path, text: &str, base: Stamp) -> io::Result<Saved> {
        let abs = self.abs_checked(rel)?;
        let disk = match fsutil::read(&abs) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if let Some(bytes) = &disk {
            if Stamp::of_bytes(bytes) != base && bytes != text.as_bytes() {
                let copy = write_conflict_copy(&self.root, rel, text.as_bytes())?;
                let theirs = String::from_utf8(bytes.clone()).map_err(|_| "it is not readable text".to_string());
                self.rescan();
                return Ok(Saved::Conflict { copy, theirs });
            }
        }
        if disk.as_deref() != Some(text.as_bytes()) {
            if let Some(p) = abs.parent() {
                fs::create_dir_all(p)?;
            }
            fsutil::write_atomic(&abs, text.as_bytes())?;
        }
        self.index_written(rel, text);
        Ok(Saved::Written(Stamp::of(text)))
    }

    /// Change a note that has no open buffer: `f` gets the text on disk (read now, not from the
    /// index) and returns the new text, or `None` for no change. A note that can't be read is
    /// an error and stays as it is. Returns whether the file changed.
    pub fn edit_note(&mut self, rel: &Path, f: impl FnOnce(&str) -> Option<String>) -> io::Result<bool> {
        let abs = self.abs_checked(rel)?;
        let old = fsutil::read_to_string(&abs)?;
        let Some(new) = f(&old) else { return Ok(false) };
        if new == old {
            return Ok(false);
        }
        fsutil::write_atomic(&abs, new.as_bytes())?;
        self.index_written(rel, &new);
        Ok(true)
    }

    /// Keep the index in step with a note just written (so the write is not seen as an outside change).
    fn index_written(&mut self, rel: &Path, text: &str) {
        if kind_of(rel) != FileKind::Note {
            return;
        }
        let mtime = FileStamp::of(&self.abs(rel));
        self.set_note_text(rel, text);
        if let Some(n) = self.notes.get_mut(rel) {
            n.mtime = mtime;
            n.unreadable = None;
        }
    }

    /// Move a file or a whole folder into the folder `into` (keeping its name). Links are kept
    /// pointing at what they pointed to (VAULT-015, VAULT-016), in notes and drawings. Returns the
    /// new path and what happened to links elsewhere.
    pub fn move_into(&mut self, from: &Path, into: &Path) -> io::Result<(PathBuf, LinkUpdate)> {
        let name = from.file_name().ok_or_else(|| io::Error::other("nothing to move"))?;
        let to = into.join(name);
        if to == from {
            return Ok((to, LinkUpdate::default()));
        }
        if into.starts_with(from) {
            return Err(io::Error::other("a folder can't go into itself"));
        }
        if self.abs(&to).exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("“{}” already has a “{}”", if into.as_os_str().is_empty() { "the vault".into() } else { rel_to_string(into) }, name.to_string_lossy())));
        }
        let done = self.relocate(from, &to, true)?;
        Ok((to, done))
    }

    /// Rename a file or folder (`to` is its new vault path). When `update_links`, links in notes
    /// and drawings are kept pointing at what they pointed to (VAULT-013, VAULT-016). Returns the
    /// files whose text was rewritten and the canvases that still point to the old name (a canvas
    /// is never written).
    pub fn rename(&mut self, from: &Path, to: &Path, update_links: bool) -> io::Result<LinkUpdate> {
        self.relocate(from, to, update_links)
    }

    /// The one rewrite routine for rename and move (VAULT-016). Before the change, every link
    /// that resolves is noted with the file it points to (after the change: its new place if it
    /// moved). After the change, each link that would now resolve somewhere else is rewritten to
    /// the shortest form that resolves to that file. Links that were red are left alone.
    fn relocate(&mut self, from: &Path, to: &Path, update_links: bool) -> io::Result<LinkUpdate> {
        self.abs_checked(from)?;
        self.abs_checked(to)?;
        // a new letter case of the same name "exists" where letter case is ignored (macOS, Windows)
        if self.abs(to).exists() && !fsutil::is_case_only_rename(&self.abs(from), &self.abs(to)) {
            // never replace anything (a rename onto an empty folder would succeed otherwise)
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file with that name already exists"));
        }
        let is_dir = self.abs(from).is_dir();
        let moved = |p: &Path| -> Option<PathBuf> {
            if p == from {
                Some(to.to_path_buf())
            } else if is_dir {
                p.strip_prefix(from).ok().map(|rest| to.join(rest))
            } else {
                None
            }
        };
        let mut plan: Vec<RewritePlan> = Vec::new();
        if update_links {
            for (src, n) in self.linkers() {
                let links: Vec<(LinkSpan, PathBuf)> = n.links.iter().zip(&n.resolved).filter_map(|(l, r)| r.as_ref().map(|r| (l.clone(), moved(r).unwrap_or_else(|| r.clone())))).collect();
                if !links.is_empty() {
                    plan.push((moved(src).unwrap_or_else(|| src.clone()), n.text.clone(), links));
                }
            }
        }
        self.failed_rewrites.clear();
        if let Some(p) = self.abs(to).parent() {
            fs::create_dir_all(p)?;
        }
        fsutil::rename_noreplace(&self.abs(from), &self.abs(to))?;
        self.rescan();
        // 1. work out every new text (nothing is written yet)
        enum Prepared {
            Note(PathBuf, String, String),
            /// a drawing: its link targets to change (old text, new text)
            Drawing(PathBuf, Vec<(String, String)>),
        }
        let mut prepared: Vec<Prepared> = Vec::new();
        for (src, text, links) in plan {
            let is_drawing = self.drawings.contains_key(&src);
            // changed on disk in the meantime: leave it alone rather than edit the wrong place
            if self.notes.get(&src).or_else(|| self.drawings.get(&src)).map(|n| n.text != text).unwrap_or(true) {
                self.failed_rewrites.push((src, "it was changed by another program meanwhile".into()));
                continue;
            }
            let edits: Vec<_> = links
                .iter()
                .filter(|(l, want)| self.resolve(&l.target, Some(&src)).as_deref() != Some(want.as_path()))
                .map(|(l, want)| {
                    let written = self.link_text_for(&markdown::written_target(&text, l), want, &src);
                    (l.target_range.clone(), markdown::replace_target(&text, l, &written))
                })
                .collect();
            if edits.is_empty() {
                continue;
            }
            if is_drawing {
                let pairs = edits.iter().filter(|(r, _)| text.is_char_boundary(r.start) && text.is_char_boundary(r.end)).map(|(r, new)| (text[r.clone()].to_string(), new.clone())).collect();
                prepared.push(Prepared::Drawing(src, pairs));
                continue;
            }
            let new = apply_edits(&text, edits);
            if new != text {
                prepared.push(Prepared::Note(src, text, new));
            }
        }
        // 2. check that every note is still what the plan was made from
        let mut ready = Vec::new();
        for p in prepared {
            match p {
                Prepared::Note(src, old, new) => match fsutil::read_to_string(self.abs(&src)) {
                    Ok(d) if d == old => ready.push(Prepared::Note(src, old, new)),
                    Ok(_) => self.failed_rewrites.push((src, "it was changed by another program meanwhile".into())),
                    Err(e) => self.failed_rewrites.push((src, e.to_string())),
                },
                d => ready.push(d),
            }
        }
        // 3. write them; one failing does not stop the others, and the caller is told which
        let mut done = LinkUpdate::default();
        for p in ready {
            match p {
                Prepared::Note(src, old, new) => match self.write_rewrite(&src, &old, &new) {
                    Ok(()) => done.changed.push(src),
                    Err(e) => self.failed_rewrites.push((src, e.to_string())),
                },
                Prepared::Drawing(src, pairs) => match self.rewrite_drawing(&src, &mut done, &|t: &str| replace_link_targets(t, &pairs)) {
                    Ok(()) => {}
                    Err(e) => self.failed_rewrites.push((src, e.to_string())),
                },
            }
        }
        Ok(done)
    }

    /// Rewrite the link texts of the drawing at `rel` with `edit` (canvases are only noted, never
    /// written).
    fn rewrite_drawing(&mut self, rel: &Path, done: &mut LinkUpdate, edit: &dyn Fn(&str) -> String) -> io::Result<()> {
        if crate::drawing::canvas::is_canvas_path(rel) {
            done.canvases.push(rel.to_path_buf());
            return Ok(());
        }
        let abs = self.abs_checked(rel)?;
        let original = fsutil::read_to_string(&abs)?;
        if let Some(text) = crate::drawing::rewrite_link_texts(rel, &original, edit).map_err(io::Error::other)? {
            fsutil::write_atomic(&abs, text.as_bytes())?;
            done.changed.push(rel.to_path_buf());
            if self.index_drawing(rel) {
                self.reparse_drawing(rel);
                self.generation += 1;
            }
        }
        Ok(())
    }

    /// How a link that was written as `written` in the note `src` should be written so it
    /// resolves to `want`: the shortest such form. A name stays a name (unless another file of
    /// that name would win), a path keeps its folder part, a `./`/`../` path stays relative,
    /// and a `.md` is kept or left out as before.
    pub fn link_text_for(&self, written: &str, want: &Path, src: &Path) -> String {
        let had_md = written.len() > 3 && written.is_char_boundary(written.len() - 3) && written[written.len() - 3..].eq_ignore_ascii_case(".md");
        let shape = |s: String| if had_md { s } else { markdown::strip_md_ext(&s).to_string() };
        let ok = |s: &str| self.resolve(s, Some(src)).as_deref() == Some(want);
        let full = shape(rel_to_string(want));
        if is_relative_link(written.trim()) {
            let rel = shape(relative_link(src.parent().unwrap_or(Path::new("")), want));
            if ok(&rel) {
                return rel;
            }
        } else if !written.contains('/') || !full.contains('/') {
            // by name: the file name alone, if that finds the right file from here
            let name = shape(want.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
            if ok(&name) {
                return name;
            }
        }
        if full.contains('/') {
            full
        } else {
            // a note at the top of the vault, whose name finds another file: `/Plan`
            format!("/{full}")
        }
    }

    /// Write `new` over the note `rel` if it still holds `old` on disk, and update the index.
    fn write_rewrite(&mut self, rel: &Path, old: &str, new: &str) -> io::Result<()> {
        let abs = self.abs(rel);
        if fsutil::read_to_string(&abs)? != old {
            return Err(io::Error::other("it was changed by another program meanwhile"));
        }
        fsutil::write_atomic(&abs, new.as_bytes())?;
        self.index_written(rel, new);
        Ok(())
    }

    /// Apply `edits` (range in `text` -> new text) to the note `rel`, whose text is `text`, write
    /// it in one step and update the index. Returns whether anything changed.
    fn rewrite_note(&mut self, rel: &Path, text: &str, edits: Vec<(std::ops::Range<usize>, String)>) -> io::Result<bool> {
        let out = apply_edits(text, edits);
        if out == text {
            return Ok(false);
        }
        self.write_rewrite(rel, text, &out)?;
        Ok(true)
    }

    /// Fix the red path links of the note `rel` whose file name exists exactly once in the vault
    /// (LINK-012): each is rewritten to point at that file. Returns how many were fixed.
    pub fn auto_fix_links(&mut self, rel: &Path) -> io::Result<usize> {
        let Some(n) = self.notes.get(rel) else { return Ok(0) };
        let text = n.text.clone();
        let mut edits = Vec::new();
        for (l, r) in n.links.iter().zip(&n.resolved) {
            if r.is_some() {
                continue;
            }
            if let [only] = self.fix_candidates(&l.target, Some(rel)).as_slice() {
                let written = self.link_text_for(&markdown::written_target(&text, l), only, rel);
                edits.push((l.target_range.clone(), markdown::replace_target(&text, l, &written)));
            }
        }
        let count = edits.len();
        Ok(if self.rewrite_note(rel, &text, edits)? { count } else { 0 })
    }

    /// Point every red link with the target `target` in the note `rel` at the file `to` (the
    /// "Fix link" action of LINK-012). Returns how many links changed.
    pub fn fix_link(&mut self, rel: &Path, target: &str, to: &Path) -> io::Result<usize> {
        let Some(n) = self.notes.get(rel) else { return Ok(0) };
        let text = n.text.clone();
        let edits: Vec<_> = n
            .links
            .iter()
            .zip(&n.resolved)
            .filter(|(l, r)| r.is_none() && l.target == target)
            .map(|(l, _)| {
                let written = self.link_text_for(&markdown::written_target(&text, l), to, rel);
                (l.target_range.clone(), markdown::replace_target(&text, l, &written))
            })
            .collect();
        let count = edits.len();
        Ok(if self.rewrite_note(rel, &text, edits)? { count } else { 0 })
    }

    /// Move a file or folder to `<vault>/.trash/<date>/<its path in the vault>` instead of
    /// deleting it for good (VAULT-017). Nothing there is replaced: a taken path gets ` (2)`,
    /// ` (3)` … before the extension. Returns where it went, relative to the vault.
    pub fn trash(&mut self, rel: &Path) -> io::Result<PathBuf> {
        let dest = move_to_trash(&self.root, rel)?;
        self.rescan();
        Ok(dest)
    }

    /// Put an entry of the dated trash (`.trash/<date>/<path>`) back at its original path
    /// (VAULT-020). Never replaces: if something is there, it stays and nothing moves.
    /// Returns the restored path, relative to the vault.
    pub fn restore(&mut self, trash_rel: &Path) -> io::Result<PathBuf> {
        let orig = trash_origin(trash_rel).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "that is not an entry of the dated trash"))?;
        self.abs_checked(trash_rel)?;
        let to = self.abs_checked(&orig)?;
        if fs::symlink_metadata(self.abs(trash_rel)).is_err() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "it is no longer in the trash"));
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        match fsutil::rename_noreplace(&self.abs(trash_rel), &to) {
            Ok(()) => {
                self.rescan();
                Ok(orig)
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("“{}” is already there; the trashed copy stays in the trash", rel_to_string(&orig)))),
            Err(e) => Err(e),
        }
    }

    /// Nodes and edges for the graph view. Unresolved link targets become extra nodes.
    pub fn graph(&self, include_missing: bool) -> (Vec<GraphNode>, Vec<(usize, usize)>) {
        let mut nodes: Vec<GraphNode> = Vec::new();
        let mut idx: HashMap<String, usize> = HashMap::new();
        for f in &self.files {
            if f.kind == FileKind::Other {
                continue;
            }
            idx.insert(rel_to_string(&f.rel), nodes.len());
            nodes.push(GraphNode { key: rel_to_string(&f.rel), label: f.display_name(), rel: Some(f.rel.clone()), kind: f.kind });
        }
        let mut edges = HashSet::new();
        // in the order of the paths: the missing notes get the same places in the graph every start
        let mut linkers: Vec<_> = self.linkers().collect();
        linkers.sort_by(|a, b| a.0.cmp(b.0));
        for (src, n) in linkers {
            let Some(&a) = idx.get(&rel_to_string(src)) else { continue };
            for (l, r) in n.links.iter().zip(n.resolved.iter()) {
                let b = match r {
                    Some(p) => match idx.get(&rel_to_string(p)) {
                        Some(&b) => b,
                        None => continue,
                    },
                    None => {
                        if !include_missing || l.target.is_empty() {
                            continue;
                        }
                        let key = format!("missing:{}", l.target.to_lowercase());
                        *idx.entry(key.clone()).or_insert_with(|| {
                            nodes.push(GraphNode { key, label: l.target.clone(), rel: None, kind: FileKind::Other });
                            nodes.len() - 1
                        })
                    }
                };
                if a != b {
                    edges.insert((a.min(b), a.max(b)));
                }
            }
        }
        let mut e: Vec<_> = edges.into_iter().collect();
        e.sort();
        (nodes, e)
    }
}

#[derive(Clone, Debug)]
pub struct GraphNode {
    pub key: String,
    pub label: String,
    pub rel: Option<PathBuf>,
    pub kind: FileKind,
}

fn walk(root: &Path, rel: &Path, files: &mut Vec<FileEntry>, folders: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(root.join(rel)) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let r = rel.join(&name);
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            folders.push(r.clone());
            walk(root, &r, files, folders);
        } else if ft.is_file() {
            let kind = kind_of(&r);
            let meta = e.metadata().ok();
            let modified = meta.as_ref().and_then(|m| m.modified().ok());
            let created = meta.as_ref().and_then(|m| m.created().ok()).or(modified);
            files.push(FileEntry { link_name: link_name_of(&r, kind), rel: r, kind, created, modified });
        }
    }
}

/// An Obsidian canvas for the demo vault (cards, a group, a web link and labelled edges).
const SAMPLE_CANVAS: &str = r##"{
	"nodes":[
		{"id":"grp","type":"group","label":"This season","x":-40,"y":-60,"width":700,"height":300},
		{"id":"plan","type":"file","file":"Projects/Garden plan.md","x":0,"y":0,"width":240,"height":70,"color":"4"},
		{"id":"seeds","type":"text","text":"Order seeds\nby March","x":380,"y":0,"width":240,"height":70,"color":"3"},
		{"id":"bed","type":"text","text":"Build raised bed","x":380,"y":140,"width":240,"height":70},
		{"id":"ideas","type":"file","file":"Ideas.md","x":0,"y":320,"width":240,"height":70,"color":"6"},
		{"id":"web","type":"link","url":"https://example.com","x":380,"y":320,"width":240,"height":70}
	],
	"edges":[
		{"id":"e1","fromNode":"plan","fromSide":"right","toNode":"seeds","toSide":"left","label":"first"},
		{"id":"e2","fromNode":"seeds","fromSide":"bottom","toNode":"bed","toSide":"top"},
		{"id":"e3","fromNode":"ideas","fromSide":"top","toNode":"plan","toSide":"bottom","label":"grew from","color":"6"},
		{"id":"e4","fromNode":"ideas","fromSide":"right","toNode":"web","toSide":"left","toEnd":"none"}
	]
}"##;

/// Write a small welcome vault that shows off links, tasks and a drawing.
pub fn write_sample_vault(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join("Projects"))?;
    let files: [(&str, &str); 6] = [
        (
            "Welcome.md",
            "# Welcome to your vault\n\nThis vault is just a folder of plain **markdown** files.\n\n- Links to notes that exist are [[Ideas|blue]] like [[Ideas]].\n- Links to notes that don't exist yet are red, like [[Reading list]]. Click one to create it.\n- Type `[[` to get suggestions while you write.\n- Web links open in your browser: [an example site](https://example.com)\n\n![[z_media/sample-gradient.png]]\n\n## Things to try\n- [ ] Click a red link to create a note\n- [x] Open the vault\n- [ ] Draw something in [[Sketch.excalidraw]]\n- [ ] Open the task list from the left ribbon\n\n## Lists help you type\n1. [x] Press Enter at the end of a list item to start the next one\n2. [ ] Press Tab to indent it, Shift+Tab to bring it back\n3. [ ] In a heading, Tab adds a level and Shift+Tab removes one\n4. [ ] Alt+Up / Alt+Down moves the current line\n5. [ ] Type [! then ~/ to pick a picture from your home folder (Tab completes)\n\n![[Sketch.excalidraw]]\n\n#getting-started\n",
        ),
        (
            "Ideas.md",
            "A place for loose thoughts. Back to [[Welcome]].\n\n- [ ] Plan the [[Projects/Garden plan|garden]]\n- [ ] Write about [[Rust]] 📅 2026-12-01\n\n> Notes get more useful the more they link to each other.\n\n#ideas\n",
        ),
        (
            "Projects/Garden plan.md",
            "Linked from [[Ideas]].\n\n## Tasks\n- [x] Order seeds\n- [ ] Build raised bed\n- [ ] Ask about [[Compost]]\n\n#project\n",
        ),
        ("Daily log.md", "- Met with team, see [[Ideas]]\n- [ ] Follow up on [[Garden plan]]\n"),
        ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE),
        ("Overview.canvas", SAMPLE_CANVAS),
    ];
    for (name, content) in files {
        let p = root.join(name);
        if !p.exists() {
            match fsutil::create_new(&p, content.as_bytes()) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                r => r?,
            }
        }
    }
    // a small generated picture, embedded in the welcome note
    let pic = root.join("z_media/sample-gradient.png");
    if !pic.exists() {
        fs::create_dir_all(root.join("z_media"))?;
        let img = image::RgbaImage::from_fn(480, 120, |x, y| {
            let t = x as f32 / 480.0;
            let wave = ((x as f32 / 30.0).sin() * 18.0 + 60.0) as u32;
            if y.abs_diff(wave) < 3 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([(90.0 + 120.0 * t) as u8, (80.0 + 40.0 * (1.0 - t)) as u8, 230, 255])
            }
        });
        let mut png = std::io::Cursor::new(Vec::new());
        img.write_to(&mut png, image::ImageFormat::Png).map_err(io::Error::other)?;
        match fsutil::create_new(&pic, png.get_ref()) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            r => r?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_vault(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// The sample vault plus `Plain.excalidraw` (label "see [[Ideas]]", a `link` field to the
    /// garden plan by path) and `Plugin.excalidraw.md` (the same, in the plugin's format).
    fn vault_with_drawings(tag: &str) -> (PathBuf, Vault) {
        let d = temp_vault(tag);
        write_sample_vault(&d).unwrap();
        let mut scene = crate::drawing::Scene::parse(&crate::drawing::SAMPLE_SCENE.replace("\"Idea\"", "\"see [[Ideas]]\"")).unwrap();
        scene.elements[3].extra.insert("link".into(), serde_json::json!("[[Projects/Garden plan]]"));
        fs::write(d.join("Plain.excalidraw"), scene.to_json()).unwrap();
        fs::write(d.join("Plugin.excalidraw.md"), crate::drawing::excalidraw_md::new_file(&scene)).unwrap();
        let v = Vault::open(&d).unwrap();
        (d, v)
    }

    fn texts(v: &Vault, rel: &str) -> Vec<String> {
        crate::drawing::Scene::read_file(&v.abs(Path::new(rel))).unwrap().live().filter_map(|e| e.text.clone()).collect()
    }

    // spec: DRAW-016
    #[test]
    fn drawings_and_canvases_link_like_notes() {
        let (d, v) = vault_with_drawings("drawing-links");
        let bl = v.backlinks(Path::new("Ideas.md"));
        let from = |rel: &str| bl.iter().find(|(p, _)| p == Path::new(rel)).map(|(_, lines)| lines.iter().map(|l| l.1.clone()).collect::<Vec<_>>());
        assert_eq!(from("Plain.excalidraw"), Some(vec!["see [[Ideas]]".to_string()]), "the label's text is the line");
        assert_eq!(from("Plugin.excalidraw.md"), Some(vec!["see [[Ideas]]".to_string()]));
        assert_eq!(from("Overview.canvas"), Some(vec!["[[Ideas.md]]".to_string()]), "canvas file cards count");
        // the link field counts too, and the drawing has outgoing links
        assert!(v.backlinks(Path::new("Projects/Garden plan.md")).iter().any(|(p, _)| p == Path::new("Plain.excalidraw")));
        let out: Vec<String> = v.outgoing(Path::new("Plain.excalidraw")).into_iter().map(|o| o.1).collect();
        assert_eq!(out, vec!["Ideas".to_string(), "Projects/Garden plan".into()]);
        // graph edges from drawings and canvases
        let (nodes, edges) = v.graph(false);
        let at = |rel: &str| nodes.iter().position(|n| n.rel.as_deref() == Some(Path::new(rel))).unwrap();
        let edge = |a: &str, b: &str| edges.contains(&(at(a).min(at(b)), at(a).max(at(b))));
        assert!(edge("Plain.excalidraw", "Ideas.md") && edge("Overview.canvas", "Ideas.md") && edge("Plugin.excalidraw.md", "Projects/Garden plan.md"));
        // a drawing changed on disk is read again
        let mut v = v;
        let changed = fs::read_to_string(d.join("Plain.excalidraw")).unwrap().replace("see [[Ideas]]", "see [[Daily log]]");
        fs::write(d.join("Plain.excalidraw"), changed).unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        fs::File::options().write(true).open(d.join("Plain.excalidraw")).unwrap().set_modified(later).unwrap();
        v.rescan();
        assert!(v.backlinks(Path::new("Daily log.md")).iter().any(|(p, _)| p == Path::new("Plain.excalidraw")));
        assert!(!v.backlinks(Path::new("Ideas.md")).iter().any(|(p, _)| p == Path::new("Plain.excalidraw")));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DRAW-017
    #[test]
    fn renaming_and_moving_update_drawings_but_not_canvases() {
        let (d, mut v) = vault_with_drawings("drawing-rename");
        let canvas_before = fs::read_to_string(d.join("Overview.canvas")).unwrap();
        let done = v.rename(Path::new("Ideas.md"), Path::new("Thoughts.md"), true).unwrap();
        for rel in ["Plain.excalidraw", "Plugin.excalidraw.md"] {
            assert!(done.changed.contains(&PathBuf::from(rel)), "{rel}: {done:?}");
            assert!(texts(&v, rel).contains(&"see [[Thoughts]]".to_string()), "{rel}: {:?}", texts(&v, rel));
        }
        // the plugin file keeps its layout, and its text list has the new link
        let plugin = fs::read_to_string(d.join("Plugin.excalidraw.md")).unwrap();
        assert!(plugin.starts_with("---\n\nexcalidraw-plugin: parsed") && plugin.contains("```compressed-json") && plugin.contains("see [[Thoughts]] ^"));
        // the canvas is reported, not written
        assert_eq!(done.canvases, vec![PathBuf::from("Overview.canvas")]);
        assert_eq!(fs::read_to_string(d.join("Overview.canvas")).unwrap(), canvas_before);
        assert!(v.backlinks(Path::new("Thoughts.md")).iter().any(|(p, _)| p == Path::new("Plain.excalidraw")));
        // moving the garden plan: path links in drawings follow, the canvas is reported again
        fs::create_dir_all(d.join("Archive")).unwrap();
        v.rescan();
        let (_, done) = v.move_into(Path::new("Projects/Garden plan.md"), Path::new("Archive")).unwrap();
        assert!(done.changed.contains(&PathBuf::from("Plain.excalidraw")), "{done:?}");
        let link = |rel: &str| crate::drawing::Scene::read_file(&v.abs(Path::new(rel))).unwrap().elements[3].extra["link"].clone();
        assert_eq!(link("Plain.excalidraw"), serde_json::json!("[[Archive/Garden plan]]"));
        assert_eq!(link("Plugin.excalidraw.md"), serde_json::json!("[[Archive/Garden plan]]"));
        assert_eq!(done.canvases, vec![PathBuf::from("Overview.canvas")]);
        assert_eq!(fs::read_to_string(d.join("Overview.canvas")).unwrap(), canvas_before);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-002, LINK-003, LINK-007, VAULT-002
    #[test]
    fn resolve_and_backlinks() {
        let d = temp_vault("resolve");
        write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        assert!(v.exists("Ideas", None));
        assert!(v.exists("ideas", None));
        assert!(v.exists("Garden plan", None));
        assert!(v.exists("Projects/Garden plan", None));
        assert!(v.exists("Sketch.excalidraw", None));
        assert!(!v.exists("Reading list", None));
        let bl = v.backlinks(Path::new("Ideas.md"));
        let srcs: Vec<_> = bl.iter().map(|b| rel_to_string(&b.0)).collect();
        assert!(srcs.contains(&"Welcome.md".to_string()));
        assert!(srcs.contains(&"Daily log.md".to_string()));
        assert!(srcs.contains(&"Projects/Garden plan.md".to_string()));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: TASK-003
    #[test]
    fn tasks_across_vault() {
        let d = temp_vault("tasks");
        write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        let tasks = v.all_tasks();
        assert!(tasks.len() >= 8);
        assert!(tasks.iter().any(|(_, t)| t.done));
        assert!(tasks.iter().any(|(_, t)| t.due.is_some()));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-011, VAULT-013, VAULT-017
    #[test]
    fn create_from_missing_link_and_rename() {
        let d = temp_vault("rename");
        write_sample_vault(&d).unwrap();
        let mut v = Vault::open(&d).unwrap();
        let p = v.new_note_path(Path::new(""), "Reading list");
        assert_eq!(p, PathBuf::from("Reading list.md"));
        v.create_file(&p, "# Reading list\n").unwrap();
        assert!(v.exists("Reading list", None));
        // rename Ideas -> Thoughts updates links elsewhere
        let changed = v.rename(Path::new("Ideas.md"), Path::new("Thoughts.md"), true).unwrap();
        assert!(!changed.changed.is_empty());
        let welcome = &v.notes[Path::new("Welcome.md")].text;
        assert!(welcome.contains("[[Thoughts|blue]]"));
        assert!(welcome.contains("[[Thoughts]]"));
        assert!(!v.exists("Ideas", None));
        // duplicate name gets a number
        let p2 = v.new_note_path(Path::new(""), "Thoughts");
        assert_eq!(p2, PathBuf::from("Thoughts 1.md"));
        // trash
        let gone = v.trash(Path::new("Reading list.md")).unwrap();
        assert!(!v.exists("Reading list", None));
        assert!(d.join(&gone).exists());
        assert_eq!(gone, Path::new(".trash").join(crate::mail::store::today_string()).join("Reading list.md"));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-011
    #[test]
    fn graph_has_edges() {
        let d = temp_vault("graph");
        write_sample_vault(&d).unwrap();
        let v = Vault::open(&d).unwrap();
        let (nodes, edges) = v.graph(true);
        assert!(nodes.iter().any(|n| n.rel.is_none() && n.label == "Reading list"));
        assert!(edges.len() >= 5);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DEV-007
    #[test]
    fn dev_007_the_missing_notes_of_the_graph_come_in_the_order_of_the_notes_that_link_them() {
        let d = temp_vault("graph-order");
        fs::create_dir_all(&d).unwrap();
        for (name, target) in [("A.md", "Zeta"), ("B.md", "Mu"), ("C.md", "Alpha"), ("D.md", "Kappa"), ("E.md", "Beta")] {
            fs::write(d.join(name), format!("see [[{target}]]")).unwrap();
        }
        let v = Vault::open(&d).unwrap();
        for _ in 0..5 {
            let (nodes, _) = v.graph(true);
            let missing: Vec<&str> = nodes.iter().filter(|n| n.rel.is_none()).map(|n| n.label.as_str()).collect();
            assert_eq!(missing, ["Zeta", "Mu", "Alpha", "Kappa", "Beta"]);
        }
        let _ = fs::remove_dir_all(&d);
    }

    // spec: DATA-002
    #[test]
    fn an_outside_change_inside_one_timestamp_tick_is_detected() {
        let d = temp_vault("same-tick");
        fs::create_dir_all(&d).unwrap();
        fs::write(d.join("N.md"), "aaaa").unwrap();
        let mut v = Vault::open(&d).unwrap();
        let before = fs::metadata(d.join("N.md")).unwrap().modified().unwrap();
        // another program replaces the file with text of the same length, and the file system
        // gives it the same modification time (coarse timestamps)
        crate::fsutil::write_atomic(&d.join("N.md"), b"bbbb").unwrap();
        fs::File::options().write(true).open(d.join("N.md")).unwrap().set_modified(before).unwrap();
        assert_eq!(fs::metadata(d.join("N.md")).unwrap().modified().unwrap(), before);
        assert_eq!(v.rescan(), vec![PathBuf::from("N.md")]);
        assert_eq!(v.notes[Path::new("N.md")].text, "bbbb");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-005, DATA-002
    #[test]
    fn outside_change_is_detected() {
        let d = temp_vault("outside");
        write_sample_vault(&d).unwrap();
        let mut v = Vault::open(&d).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(d.join("Daily log.md"), "changed [[New]]").unwrap();
        // force mtime difference on coarse file systems
        if let Some(n) = v.notes.get_mut(Path::new("Daily log.md")) {
            n.mtime = None;
        }
        let changed = v.rescan();
        assert_eq!(changed, vec![PathBuf::from("Daily log.md")]);
        assert_eq!(v.notes[Path::new("Daily log.md")].links[0].target, "New");
        let _ = fs::remove_dir_all(&d);
    }
    /// A vault made of `(path, text)` files.
    fn vault_with(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
        let d = temp_vault(tag);
        for (p, t) in files {
            let abs = d.join(p);
            fs::create_dir_all(abs.parent().unwrap()).unwrap();
            fs::write(abs, t).unwrap();
        }
        (Vault::open(&d).unwrap(), d)
    }

    fn text(v: &Vault, rel: &str) -> String {
        v.notes[Path::new(rel)].text.clone()
    }

    fn res(v: &Vault, target: &str, from: &str) -> Option<String> {
        v.resolve(target, Some(Path::new(from))).map(|p| rel_to_string(&p))
    }

    // spec: LINK-002
    #[test]
    fn name_ties_prefer_same_folder_then_fewest_levels_then_path() {
        let (v, d) = vault_with("ties", &[("b/Plan.md", ""), ("a/Plan.md", ""), ("x/y/Plan.md", ""), ("x/y/Note.md", ""), ("Top.md", ""), ("q/Note.md", "")]);
        assert_eq!(res(&v, "plan", "x/y/Note.md").as_deref(), Some("x/y/Plan.md"), "same folder first");
        assert_eq!(res(&v, "Plan", "Top.md").as_deref(), Some("a/Plan.md"), "then fewest levels, then path");
        assert_eq!(res(&v, "Plan.md", "Top.md").as_deref(), Some("a/Plan.md"), ".md is ignored");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-003, LINK-010
    #[test]
    fn path_links_resolve_from_the_root_and_never_fall_back_to_the_name() {
        let (v, d) = vault_with("paths", &[("Projects/Plan.md", ""), ("Projects/Sub/Plan.md", ""), ("Sub/Plan.md", ""), ("Projects/Note.md", ""), ("Top.md", "")]);
        // from the vault root first, even when the linking note's folder has a match too
        assert_eq!(res(&v, "Sub/Plan", "Projects/Note.md").as_deref(), Some("Sub/Plan.md"));
        assert_eq!(res(&v, "projects/plan.md", "Top.md").as_deref(), Some("Projects/Plan.md"), "case and .md");
        assert_eq!(res(&v, "/Projects/Plan", "Top.md").as_deref(), Some("Projects/Plan.md"));
        // the linking note's folder only when the root path does not exist
        assert_eq!(res(&v, "Sub/Plan", "Top.md").as_deref(), Some("Sub/Plan.md"));
        fs::remove_dir_all(d.join("Sub")).unwrap();
        let mut v = v;
        v.rescan();
        assert_eq!(res(&v, "Sub/Plan", "Projects/Note.md").as_deref(), Some("Projects/Sub/Plan.md"));
        // no fallback to the name: `Archive/Plan` is red although notes called Plan exist
        assert_eq!(res(&v, "Archive/Plan", "Top.md"), None);
        assert_eq!(res(&v, "/Sub/Plan", "Projects/Note.md"), None, "a leading / is the vault root only");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-003, LINK-010
    #[test]
    fn dot_paths_resolve_relative_to_the_linking_note() {
        let (v, d) = vault_with("dots", &[("a/b/Note.md", ""), ("a/Other.md", ""), ("a/b/Same.md", ""), ("Same.md", ""), ("Root.md", "")]);
        assert_eq!(res(&v, "../Other", "a/b/Note.md").as_deref(), Some("a/Other.md"));
        assert_eq!(res(&v, "./Same", "a/b/Note.md").as_deref(), Some("a/b/Same.md"));
        assert_eq!(res(&v, "../../Root.md", "a/b/Note.md").as_deref(), Some("Root.md"));
        assert_eq!(res(&v, "./x/../Same", "a/b/Note.md").as_deref(), Some("a/b/Same.md"), "`..` in the middle");
        assert_eq!(res(&v, "../../../Root", "a/b/Note.md"), None, "above the vault is red");
        assert_eq!(res(&v, "./Other", "a/b/Note.md"), None, "no fallback to the name");
        assert_eq!(normalize_path(Path::new("a/b"), "../../.."), None);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-008
    #[test]
    fn ambiguous_names_are_explained() {
        let (v, d) = vault_with("ambig", &[("Projects/Plan.md", ""), ("Archive/Old/Plan.md", ""), ("Solo.md", ""), ("Top.md", "")]);
        let (n, opens) = v.ambiguity("plan", Some(Path::new("Top.md"))).unwrap();
        assert_eq!((n, rel_to_string(&opens).as_str()), (2, "Projects/Plan.md"));
        assert_eq!(v.link_hint("Plan", Some(Path::new("Top.md"))).unwrap(), "2 notes are called “Plan” – opens Projects/Plan");
        assert_eq!(v.ambiguity("Solo", None), None);
        assert_eq!(v.link_hint("Solo", None), None);
        assert_eq!(v.ambiguity("Projects/Plan", None), None, "a path is never ambiguous");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-013, VAULT-016
    #[test]
    fn rename_rewrites_links_by_what_they_resolve_to() {
        let (mut v, d) = vault_with("byres", &[("Projects/Old.md", ""), ("Archive/Old.md", ""), ("Projects/Index.md", "[[Old]] [[Archive/Old]] [[Projects/Old]] [[ÄRGER]]"), ("Ärger.md", ""), ("Top.md", "[[Projects/Old]] [[ärger|a]]")]);
        // `[[Old]]` in Projects/Index means Projects/Old (same folder); `[[Archive/Old]]` does not
        let changed = v.rename(Path::new("Projects/Old.md"), Path::new("Projects/New.md"), true).unwrap();
        assert_eq!(text(&v, "Projects/Index.md"), "[[New]] [[Archive/Old]] [[Projects/New]] [[ÄRGER]]");
        assert_eq!(text(&v, "Top.md"), "[[Projects/New]] [[ärger|a]]");
        assert_eq!(changed.changed.len(), 2);
        // Unicode case: `[[ÄRGER]]` and `[[ärger]]` point at Ärger.md
        v.rename(Path::new("Ärger.md"), Path::new("Groll.md"), true).unwrap();
        assert_eq!(text(&v, "Projects/Index.md"), "[[New]] [[Archive/Old]] [[Projects/New]] [[Groll]]");
        assert_eq!(text(&v, "Top.md"), "[[Projects/New]] [[Groll|a]]");
        // the setting off: nothing is rewritten
        v.rename(Path::new("Groll.md"), Path::new("Wut.md"), false).unwrap();
        assert!(text(&v, "Top.md").contains("[[Groll|a]]"));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-016
    #[test]
    fn rename_keeps_folder_alias_heading_brackets_and_md() {
        let (mut v, d) = vault_with("keep", &[("Projects/Old.md", ""), ("Top.md", "[[Projects/Old#Goal|the plan]] [[Projects/Old.md]] ![[Projects/Old]] [x](Projects/Old.md#Intro) [y](<Projects/Old.md>) [z](Projects/Old)")]);
        v.rename(Path::new("Projects/Old.md"), Path::new("Projects/New Plan.md"), true).unwrap();
        assert_eq!(text(&v, "Top.md"), "[[Projects/New Plan#Goal|the plan]] [[Projects/New Plan.md]] ![[Projects/New Plan]] [x](Projects/New%20Plan.md#Intro) [y](<Projects/New Plan.md>) [z](Projects/New%20Plan)");
        let n = &v.notes[Path::new("Top.md")];
        assert!(n.resolved.iter().all(|r| r.as_deref() == Some(Path::new("Projects/New Plan.md"))), "every link still finds the note");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-016
    #[test]
    fn rename_keeps_other_links_pointing_where_they_did() {
        // renaming Projects/Draft to Plan would make `[[Plan]]` in Projects/ find it instead of
        // the top-level Plan: that link becomes a path
        let (mut v, d) = vault_with("steal", &[("Plan.md", ""), ("Projects/Draft.md", ""), ("Projects/Index.md", "[[Plan]] [[Draft]]"), ("Top.md", "[[Draft]] [[Plan]]")]);
        v.rename(Path::new("Projects/Draft.md"), Path::new("Projects/Plan.md"), true).unwrap();
        assert_eq!(text(&v, "Projects/Index.md"), "[[/Plan]] [[Plan]]");
        // from the top level, `Plan` still finds Plan.md; the renamed note needs its folder
        assert_eq!(text(&v, "Top.md"), "[[Projects/Plan]] [[Plan]]");
        assert_eq!(res(&v, "/Plan", "Projects/Index.md").as_deref(), Some("Plan.md"));
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-016
    #[test]
    fn folder_rename_rewrites_links_to_its_contents() {
        let (mut v, d) = vault_with("folder", &[("Projects/Plan.md", "[[./Sub/Deep]]"), ("Projects/Sub/Deep.md", ""), ("Top.md", "[[Projects/Plan]] [[Plan]] [x](Projects/Sub/Deep.md) [[Projects]]")]);
        let changed = v.rename(Path::new("Projects"), Path::new("Work"), true).unwrap();
        assert_eq!(text(&v, "Top.md"), "[[Work/Plan]] [[Plan]] [x](Work/Sub/Deep.md) [[Projects]]");
        assert_eq!(text(&v, "Work/Plan.md"), "[[./Sub/Deep]]", "relative links inside the folder still work");
        assert_eq!(changed.changed, vec![PathBuf::from("Top.md")]);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-015, VAULT-016
    #[test]
    fn moving_a_note_keeps_its_relative_links_working() {
        let (mut v, d) = vault_with("relmove", &[("a/Note.md", "[[../Root]] [[./Sib]] [[Sib]] [[a/Sib]]"), ("a/Sib.md", ""), ("Root.md", ""), ("b/Sib.md", "")]);
        let (to, changed) = v.move_into(Path::new("a/Note.md"), Path::new("b")).unwrap();
        assert_eq!(to, PathBuf::from("b/Note.md"));
        // `[[Sib]]` would now find b/Sib: it keeps pointing at a/Sib with a path
        assert_eq!(text(&v, "b/Note.md"), "[[../Root]] [[../a/Sib]] [[a/Sib]] [[a/Sib]]");
        assert_eq!(changed.changed, vec![PathBuf::from("b/Note.md")]);
        // into a deeper folder
        v.create_folder(&VaultPath::parse("c/d").unwrap()).unwrap();
        v.move_into(Path::new("b/Note.md"), Path::new("c/d")).unwrap();
        assert_eq!(text(&v, "c/d/Note.md"), "[[../../Root]] [[../../a/Sib]] [[a/Sib]] [[a/Sib]]");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-014
    #[test]
    fn rename_never_replaces_an_existing_file_or_folder() {
        let (mut v, d) = vault_with("noreplace", &[("A.md", "a"), ("B.md", "b")]);
        fs::create_dir_all(d.join("Empty")).unwrap();
        assert!(v.rename(Path::new("A.md"), Path::new("B.md"), true).is_err());
        v.rescan();
        assert!(v.rename(Path::new("A.md"), Path::new("Empty"), true).is_err());
        assert_eq!(fs::read_to_string(d.join("B.md")).unwrap(), "b");
        assert_eq!(fs::read_to_string(d.join("A.md")).unwrap(), "a");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-012
    #[test]
    fn broken_path_links_with_one_candidate_are_fixed() {
        let (mut v, d) = vault_with("fix1", &[("Projects/Plan.md", ""), ("Notes/Index.md", "[[Archive/Plan#Goal|plan]] [x](../Archive/Plan.md) [[Archive/Gone]] [[Plan]]")]);
        let rel = Path::new("Notes/Index.md");
        assert_eq!(v.fix_candidates("Archive/Plan", Some(rel)), vec![PathBuf::from("Projects/Plan.md")]);
        assert_eq!(v.auto_fix_links(rel).unwrap(), 2);
        assert_eq!(text(&v, "Notes/Index.md"), "[[Projects/Plan#Goal|plan]] [x](../Projects/Plan.md) [[Archive/Gone]] [[Plan]]");
        assert_eq!(fs::read_to_string(d.join(rel)).unwrap(), text(&v, "Notes/Index.md"), "written to disk");
        assert_eq!(v.auto_fix_links(rel).unwrap(), 0, "nothing left to fix");
        assert!(fs::read_dir(d.join("Notes")).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("shale-tmp")), "no temporary file left");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: LINK-012
    #[test]
    fn broken_path_links_with_several_candidates_are_left_for_the_user() {
        let (mut v, d) = vault_with("fix2", &[("a/Plan.md", ""), ("b/Plan.md", ""), ("Index.md", "[[Archive/Plan]]")]);
        let rel = Path::new("Index.md");
        assert_eq!(v.auto_fix_links(rel).unwrap(), 0);
        assert_eq!(text(&v, "Index.md"), "[[Archive/Plan]]");
        assert_eq!(v.fix_candidates("Archive/Plan", Some(rel)).len(), 2);
        assert!(v.link_hint("Archive/Plan", Some(rel)).unwrap().contains("Fix link"));
        // the user picks one
        assert_eq!(v.fix_link(rel, "Archive/Plan", Path::new("b/Plan.md")).unwrap(), 1);
        assert_eq!(text(&v, "Index.md"), "[[b/Plan]]");
        assert_eq!(v.fix_candidates("b/Plan", Some(rel)), Vec::<PathBuf>::new());
        let _ = fs::remove_dir_all(&d);
    }

    fn day() -> PathBuf {
        Path::new(".trash").join(crate::mail::store::today_string())
    }

    fn put(d: &Path, rel: &str, text: &str) {
        let p = d.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fsutil::write_atomic(&p, text.as_bytes()).unwrap();
    }

    // spec: VAULT-017
    #[test]
    fn trash_keeps_the_original_path_so_same_names_from_different_folders_do_not_mix() {
        let d = temp_vault("trash-origin");
        put(&d, "a/Note.md", "from a\n");
        put(&d, "b/Note.md", "from b\n");
        let mut v = Vault::open(&d).unwrap();
        let ta = v.trash(Path::new("a/Note.md")).unwrap();
        let tb = v.trash(Path::new("b/Note.md")).unwrap();
        assert_eq!(ta, day().join("a/Note.md"));
        assert_eq!(tb, day().join("b/Note.md"));
        assert_eq!(fsutil::read_to_string(d.join(&ta)).unwrap(), "from a\n");
        assert_eq!(fsutil::read_to_string(d.join(&tb)).unwrap(), "from b\n");
        assert!(!d.join("a/Note.md").exists());
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-017
    #[test]
    fn trash_takes_a_folder_with_its_content() {
        let d = temp_vault("trash-folder");
        put(&d, "proj/one.md", "1\n");
        put(&d, "proj/deep/two.md", "2\n");
        let mut v = Vault::open(&d).unwrap();
        let t = v.trash(Path::new("proj")).unwrap();
        assert_eq!(t, day().join("proj"));
        assert_eq!(fsutil::read_to_string(d.join(&t).join("one.md")).unwrap(), "1\n");
        assert_eq!(fsutil::read_to_string(d.join(&t).join("deep/two.md")).unwrap(), "2\n");
        assert!(!d.join("proj").exists());
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-017
    #[test]
    fn a_name_clash_on_the_same_day_gets_a_number_before_the_extension_and_nothing_is_replaced() {
        let d = temp_vault("trash-clash");
        let mut v = Vault::open(&d).unwrap();
        let mut got = vec![];
        for i in 1..=3 {
            put(&d, "Note.md", &format!("version {i}\n"));
            v.rescan();
            got.push(v.trash(Path::new("Note.md")).unwrap());
        }
        assert_eq!(got, vec![day().join("Note.md"), day().join("Note (2).md"), day().join("Note (3).md")]);
        for (i, t) in got.iter().enumerate() {
            assert_eq!(fsutil::read_to_string(d.join(t)).unwrap(), format!("version {}\n", i + 1));
        }
        // folders: the number goes at the end of the name, even when it has a dot
        for _ in 0..2 {
            put(&d, "v1.2/x.md", "x\n");
            v.rescan();
            v.trash(Path::new("v1.2")).unwrap();
        }
        assert!(d.join(day()).join("v1.2 (2)/x.md").exists());
        // an old entry directly in .trash/ stays where it is
        put(&d, ".trash/Old.md", "old\n");
        put(&d, "Old.md", "new\n");
        v.rescan();
        v.trash(Path::new("Old.md")).unwrap();
        assert_eq!(fsutil::read_to_string(d.join(".trash/Old.md")).unwrap(), "old\n");
        assert_eq!(fsutil::read_to_string(d.join(day()).join("Old.md")).unwrap(), "new\n");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-020
    #[test]
    fn restore_brings_a_file_back_and_refuses_when_the_place_is_taken() {
        let d = temp_vault("trash-restore");
        put(&d, "a/b/Note.md", "precious\n");
        let mut v = Vault::open(&d).unwrap();
        let t = v.trash(Path::new("a/b/Note.md")).unwrap();
        fs::remove_dir_all(d.join("a")).unwrap(); // the folder is gone too: restore makes it again
        let back = v.restore(&t).unwrap();
        assert_eq!(back, PathBuf::from("a/b/Note.md"));
        assert_eq!(fsutil::read_to_string(d.join(&back)).unwrap(), "precious\n");
        assert!(!d.join(&t).exists());
        assert!(v.notes.contains_key(Path::new("a/b/Note.md")));
        // trash it again, then put something else in its place
        let t2 = v.trash(Path::new("a/b/Note.md")).unwrap();
        assert_eq!(t2, t, "restore moved the first trashed copy away, so the path is free again");
        put(&d, "a/b/Note.md", "newer\n");
        v.rescan();
        let e = v.restore(&t2).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert!(e.to_string().contains("already there"), "{e}");
        assert_eq!(fsutil::read_to_string(d.join("a/b/Note.md")).unwrap(), "newer\n");
        assert_eq!(fsutil::read_to_string(d.join(&t2)).unwrap(), "precious\n");
        // only entries of the dated trash are restored
        assert!(v.restore(Path::new("Note.md")).is_err());
        assert!(v.restore(Path::new(".trash/Old.md")).is_err());
        assert!(v.restore(&day().join("../../x")).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    /// Make the index believe `rel` is as before: write `text`, then put the old time stamp back,
    /// so a rescan does not read the file again.
    fn change_unnoticed(d: &Path, rel: &str, text: &str) {
        let p = d.join(rel);
        let before = fs::metadata(&p).unwrap().modified().unwrap();
        fs::write(&p, text).unwrap();
        fs::File::options().write(true).open(&p).unwrap().set_modified(before).unwrap();
    }

    // spec: VAULT-016
    #[test]
    fn a_note_changed_after_the_scan_is_not_rewritten_and_is_reported() {
        let (mut v, d) = vault_with("relocate-stale", &[("Target.md", "t\n"), ("A.md", "see [[Target]]\n"), ("B.md", "see [[Target]]\n")]);
        // same time stamp: the rescan after the rename keeps the old text, the check right before
        // writing reads the new one
        change_unnoticed(&d, "B.md", "see [[Target]] +\n");
        let done = v.rename(Path::new("Target.md"), Path::new("Goal.md"), true).unwrap();
        assert_eq!(done.changed, vec![PathBuf::from("A.md")]);
        assert_eq!(v.failed_rewrites.len(), 1);
        assert_eq!(v.failed_rewrites[0].0, PathBuf::from("B.md"));
        assert!(v.failed_rewrites[0].1.contains("another program"), "{:?}", v.failed_rewrites);
        assert_eq!(fsutil::read_to_string(d.join("B.md")).unwrap(), "see [[Target]] +\n", "the other program's text is kept");
        assert_eq!(fsutil::read_to_string(d.join("A.md")).unwrap(), "see [[Goal]]\n");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-016
    #[cfg(unix)]
    #[test]
    fn a_note_that_cannot_be_read_when_the_links_are_rewritten_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let (mut v, d) = vault_with("relocate-unreadable", &[("Target.md", "t\n"), ("A.md", "see [[Target]]\n"), ("B.md", "see [[Target]]\n")]);
        fs::set_permissions(d.join("B.md"), fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(d.join("B.md")).is_ok() {
            return; // running as root: permissions do not hold
        }
        let done = v.rename(Path::new("Target.md"), Path::new("Goal.md"), true).unwrap();
        fs::set_permissions(d.join("B.md"), fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(done.changed, vec![PathBuf::from("A.md")]);
        assert_eq!(v.failed_rewrites.iter().map(|f| f.0.clone()).collect::<Vec<_>>(), vec![PathBuf::from("B.md")]);
        assert_eq!(fsutil::read_to_string(d.join("B.md")).unwrap(), "see [[Target]]\n");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-016
    #[cfg(unix)]
    #[test]
    fn a_drawing_that_cannot_be_written_when_the_links_are_rewritten_is_reported() {
        use std::os::unix::fs::PermissionsExt;
        let (d, mut v) = vault_with_drawings("drawing-rename-fail");
        // a folder that cannot take the temporary file of the atomic write
        fs::create_dir_all(d.join("Locked")).unwrap();
        fs::rename(d.join("Plain.excalidraw"), d.join("Locked/Plain.excalidraw")).unwrap();
        v.rescan();
        fs::set_permissions(d.join("Locked"), fs::Permissions::from_mode(0o500)).unwrap();
        if fs::File::create(d.join("Locked/probe")).is_ok() {
            return; // running as root: permissions do not hold
        }
        let done = v.rename(Path::new("Ideas.md"), Path::new("Thoughts.md"), true).unwrap();
        fs::set_permissions(d.join("Locked"), fs::Permissions::from_mode(0o700)).unwrap();
        assert!(done.changed.contains(&PathBuf::from("Plugin.excalidraw.md")), "{done:?}");
        assert!(!done.changed.contains(&PathBuf::from("Locked/Plain.excalidraw")));
        assert_eq!(v.failed_rewrites.iter().map(|f| f.0.clone()).collect::<Vec<_>>(), vec![PathBuf::from("Locked/Plain.excalidraw")]);
        assert!(texts(&v, "Locked/Plain.excalidraw").contains(&"see [[Ideas]]".to_string()), "the drawing is as it was");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-017
    #[test]
    fn trash_numbers_a_taken_name_on_the_same_day_and_a_missing_item_is_an_error() {
        let d = temp_vault("trash-taken");
        put(&d, "Note.md", "one\n");
        let mut v = Vault::open(&d).unwrap();
        let first = v.trash(Path::new("Note.md")).unwrap();
        put(&d, "Note.md", "two\n");
        v.rescan();
        let second = v.trash(Path::new("Note.md")).unwrap();
        assert_ne!(first, second);
        assert_eq!(fsutil::read_to_string(d.join(&first)).unwrap(), "one\n");
        assert_eq!(fsutil::read_to_string(d.join(&second)).unwrap(), "two\n");
        assert!(v.trash(Path::new("Gone.md")).is_err(), "a missing item is not silently ignored");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-020
    #[test]
    fn restore_of_a_missing_trash_entry_says_so() {
        let d = temp_vault("trash-restore-missing");
        put(&d, "Note.md", "x\n");
        let mut v = Vault::open(&d).unwrap();
        let t = v.trash(Path::new("Note.md")).unwrap();
        v.restore(&t).unwrap();
        assert_eq!(v.restore(&t).unwrap_err().kind(), io::ErrorKind::NotFound, "the entry is gone after the first restore");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-020
    #[test]
    fn restore_brings_a_folder_back_with_its_content() {
        let d = temp_vault("trash-restore-dir");
        put(&d, "proj/deep/two.md", "2\n");
        let mut v = Vault::open(&d).unwrap();
        let t = v.trash(Path::new("proj")).unwrap();
        assert_eq!(v.restore(&t).unwrap(), PathBuf::from("proj"));
        assert_eq!(fsutil::read_to_string(d.join("proj/deep/two.md")).unwrap(), "2\n");
        let _ = fs::remove_dir_all(&d);
    }

    // spec: VAULT-027
    #[test]
    fn new_note_paths_are_portable_and_within_their_limit() {
        let d = temp_vault("portable-new");
        let mut v = Vault::open(&d).unwrap();
        let name = |p: PathBuf| p.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name(v.new_note_path(Path::new(""), "Plan: v2?")), "Plan- v2-.md");
        assert_eq!(name(v.new_note_path(Path::new(""), "CON")), "CON_.md");
        assert_eq!(name(v.new_note_path(Path::new(""), "Ends with dots...")), "Ends with dots.md");
        assert_eq!(v.new_note_path(Path::new(""), "Dir: a/Sub*: b/Note"), PathBuf::from("Dir- a/Sub-- b/Note.md"), "every part");
        assert_eq!(name(v.new_note_path(Path::new(""), "Zeichnung: 1.excalidraw")), "Zeichnung- 1.excalidraw");
        assert_eq!(name(v.new_note_path(Path::new(""), "Plan.excalidraw.md")), "Plan.excalidraw.md");
        // typed names: 200 bytes with the extension, Shale's own: 127
        let long = "ü".repeat(300);
        let typed = v.new_note_path(Path::new(""), &long);
        assert!(name(typed.clone()).len() <= 200 && name(typed).len() > 190);
        let made = v.new_note_path_max(Path::new(""), &long, crate::fsutil::MADE_NAME_MAX);
        assert!(name(made.clone()).len() <= 127 && name(made).ends_with("ü.md"));
        // a number for a taken name still fits
        let first = v.new_note_path_max(Path::new(""), &long, 127);
        v.create_file(&first, "").unwrap();
        let second = v.new_note_path_max(Path::new(""), &long, 127);
        assert_ne!(first, second);
        assert!(name(second.clone()).len() <= 127 && name(second).ends_with(" 1.md"));
        let _ = fs::remove_dir_all(&d);
    }
    // spec: PERF-002
    #[test]
    fn perf_002_file_finds_what_the_linear_search_finds_through_every_change() {
        let (mut v, d) = vault_with("perf2-file", &[("A.md", "a"), ("Folder/B.md", "b"), ("folder/c.md", "c"), ("z.png", "x")]);
        let check = |v: &Vault| {
            for f in &v.files {
                assert_eq!(v.file(&f.rel).map(|e| e.rel.clone()), Some(f.rel.clone()), "{:?}", f.rel);
            }
            assert!(v.file(Path::new("nothing.md")).is_none());
            assert!(v.file(Path::new("")).is_none());
        };
        check(&v);
        // a new file in the middle moves the ones after it
        fs::write(d.join("B2.md"), "new").unwrap();
        v.rescan();
        check(&v);
        assert!(v.file(Path::new("B2.md")).is_some());
        // a file that goes
        fs::remove_file(d.join("A.md")).unwrap();
        v.rescan();
        check(&v);
        assert!(v.file(Path::new("A.md")).is_none());
        // a rename through the vault
        v.rename(Path::new("B2.md"), Path::new("Folder/Moved.md"), true).unwrap();
        check(&v);
        assert!(v.file(Path::new("B2.md")).is_none() && v.file(Path::new("Folder/Moved.md")).is_some());
        // an unchanged rescan changes nothing
        let g = v.generation;
        v.rescan();
        assert_eq!(v.generation, g);
        check(&v);
        let _ = fs::remove_dir_all(&d);
    }

    // spec: PERF-002
    #[test]
    fn perf_002_the_file_list_and_the_lists_made_of_it_keep_their_order() {
        let (v, d) = vault_with("perf2-order", &[("b.md", "- [ ] one\n- [x] two"), ("A.md", "- [ ] three"), ("Ä.md", "x"), ("c/D.md", "[[b]] [[A]]\n- [ ] four"), ("e.md", "[[b]]")]);
        let names: Vec<String> = v.files.iter().map(|f| rel_to_string(&f.rel)).collect();
        let mut expected = names.clone();
        expected.sort_by_key(|n| n.to_lowercase());
        assert_eq!(names, expected, "files sorted by their lower-case path");
        let tasks: Vec<(String, usize, bool)> = v.all_tasks().into_iter().map(|(p, t)| (rel_to_string(&p), t.line, t.done)).collect();
        assert_eq!(tasks, [("A.md".to_string(), 0, false), ("b.md".into(), 0, false), ("b.md".into(), 1, true), ("c/D.md".into(), 1, false)]);
        let back: Vec<String> = v.backlinks(Path::new("b.md")).into_iter().map(|(p, _)| rel_to_string(&p)).collect();
        assert_eq!(back, ["c/D.md", "e.md"]);
        let _ = fs::remove_dir_all(&d);
    }
    // spec: PERF-008
    #[test]
    fn perf_008_a_big_vault_parsed_on_several_threads_has_the_index_of_one_note_at_a_time() {
        let n = PARALLEL_FROM * 2 + 7;
        let files: Vec<(String, String)> = (0..n)
            .map(|i| (format!("d{}/Note {i}.md", i % 5), format!("---\ntags: [t{}]\n---\n# Note {i}\nSee [[Note {}]] and [[Note {}|x]] #tag{} and [[Missing {i}]]\n- [ ] task {i}\n- [x] done {i}\n", i % 3, (i + 1) % n, (i * 7) % n, i % 11)))
            .collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let (mut v, d) = vault_with("perf8-parse", &refs);
        let snapshot = |v: &Vault| {
            let mut all: Vec<_> =
                v.notes.iter().map(|(k, n)| (k.clone(), n.links.iter().map(|l| l.target.clone()).collect::<Vec<_>>(), n.resolved.clone(), n.tasks.iter().map(|t| (t.line, t.done)).collect::<Vec<_>>(), n.tags.clone())).collect();
            all.sort();
            all
        };
        let parallel = snapshot(&v);
        assert_eq!(parallel.len(), n);
        assert!(parallel.iter().all(|(_, l, r, t, tags)| l.len() == 3 && r.len() == 3 && t.len() == 2 && !tags.is_empty()));
        assert!(parallel.iter().all(|(_, _, r, _, _)| r.iter().filter(|x| x.is_none()).count() == 1), "one link of three is to a missing note");
        // the same index one note at a time
        let keys: Vec<PathBuf> = v.notes.keys().cloned().collect();
        for k in &keys {
            v.reparse(k);
        }
        assert_eq!(snapshot(&v), parallel);
        let _ = fs::remove_dir_all(&d);
    }
}
