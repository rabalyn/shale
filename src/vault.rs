//! The vault: a folder of markdown notes and drawings, plus an index of links, tasks and tags.

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
            FileKind::Drawing => crate::excalidraw_md::drawing_stem(&self.link_name).to_string(),
            FileKind::Other => self.link_name.clone(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct NoteIndex {
    pub mtime: Option<SystemTime>,
    pub text: String,
    pub links: Vec<LinkSpan>,
    /// Resolution of each link in `links` (same order).
    pub resolved: Vec<Option<PathBuf>>,
    pub tasks: Vec<Task>,
    pub tags: Vec<String>,
}

#[derive(Default)]
pub struct Vault {
    pub root: PathBuf,
    pub files: Vec<FileEntry>,
    pub folders: Vec<PathBuf>,
    /// (created, modified) of each folder, for sorting the file tree.
    pub folder_times: HashMap<PathBuf, (Option<SystemTime>, Option<SystemTime>)>,
    pub notes: HashMap<PathBuf, NoteIndex>,
    name_index: HashMap<String, Vec<PathBuf>>,
    /// Lowercase vault path (`projects/plan.md` and `projects/plan`) -> file.
    path_index: HashMap<String, PathBuf>,
    /// Bumped whenever anything in the index changes; UI caches key off this.
    pub generation: u64,
}

pub fn rel_to_string(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
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

/// Replace a file in one step: the new text goes to a hidden temporary file next to it, is
/// flushed to disk and then renamed over the old one, so a crash leaves the old or the new
/// note, never half of one. The file keeps its permissions; a symbolic link keeps pointing at
/// the file it points to.
// TODO(merge): use `fsutil::write_atomic` (DATA-004) once branch
// claude/imap-client-planning-306725 is merged; this is the same pattern.
fn replace_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let path = match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => fs::canonicalize(path)?,
        _ => path.to_path_buf(),
    };
    let dir = path.parent().ok_or_else(|| io::Error::other("no folder"))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.shale-tmp-{}", std::process::id()));
    let result = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        if let Ok(m) = fs::metadata(&path) {
            fs::set_permissions(&tmp, m.permissions())?;
        }
        drop(f);
        fs::rename(&tmp, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn kind_of(path: &Path) -> FileKind {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if crate::excalidraw_md::is_drawing_path(path) {
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
        FileKind::Drawing => crate::excalidraw_md::strip_suffix_ci(&name, ".md").unwrap_or(&name).to_string(),
        _ => name,
    }
}

impl Vault {
    pub fn open(root: &Path) -> io::Result<Vault> {
        if !root.is_dir() {
            fs::create_dir_all(root)?;
        }
        let mut v = Vault { root: root.to_path_buf(), ..Default::default() };
        v.rescan();
        Ok(v)
    }

    pub fn abs(&self, rel: &Path) -> PathBuf {
        self.root.join(rel)
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
        files.sort_by_key(|f: &FileEntry| rel_to_string(&f.rel).to_lowercase());
        folders.sort_by_key(|f| rel_to_string(f).to_lowercase());
        let mut changed = Vec::new();
        let mut seen = HashSet::new();
        let mut structure_changed = files.len() != self.files.len() || folders != self.folders;
        for f in &files {
            if !structure_changed && !self.files.iter().any(|o| o.rel == f.rel) {
                structure_changed = true;
            }
            if f.kind != FileKind::Note {
                continue;
            }
            seen.insert(f.rel.clone());
            let abs = self.root.join(&f.rel);
            let mtime = fs::metadata(&abs).and_then(|m| m.modified()).ok();
            let up_to_date = self.notes.get(&f.rel).map(|n| n.mtime == mtime && mtime.is_some()).unwrap_or(false);
            if !up_to_date {
                let text = fs::read_to_string(&abs).unwrap_or_default();
                let differs = self.notes.get(&f.rel).map(|n| n.text != text).unwrap_or(true);
                let entry = self.notes.entry(f.rel.clone()).or_default();
                entry.mtime = mtime;
                if differs {
                    entry.text = text;
                    changed.push(f.rel.clone());
                }
            }
        }
        let before = self.notes.len();
        self.notes.retain(|k, _| seen.contains(k));
        if before != self.notes.len() {
            structure_changed = true;
        }
        self.files = files;
        self.folders = folders;
        if structure_changed || !changed.is_empty() {
            self.rebuild_name_index();
            if structure_changed {
                self.reparse_all();
            } else {
                for c in &changed {
                    self.reparse(c);
                }
            }
            self.generation += 1;
        }
        changed
    }

    fn rebuild_name_index(&mut self) {
        self.name_index.clear();
        self.path_index.clear();
        for f in &self.files {
            self.name_index.entry(f.link_name.to_lowercase()).or_default().push(f.rel.clone());
            // also allow the full file name (`Note.md`, `Plan.excalidraw.md`)
            let full = f.rel.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if full != f.link_name.to_lowercase() {
                self.name_index.entry(full).or_default().push(f.rel.clone());
            }
            let path = rel_to_string(&f.rel).to_lowercase();
            if let Some(short) = crate::excalidraw_md::strip_suffix_ci(&path, ".md") {
                // an exact file name wins over `X.md` written without its `.md`
                self.path_index.entry(short.to_string()).or_insert_with(|| f.rel.clone());
            }
            self.path_index.insert(path, f.rel.clone());
        }
    }

    fn reparse_all(&mut self) {
        let keys: Vec<PathBuf> = self.notes.keys().cloned().collect();
        for k in keys {
            self.reparse(&k);
        }
    }

    fn reparse(&mut self, rel: &Path) {
        let Some(n) = self.notes.get(rel) else { return };
        let links = markdown::find_links(&n.text);
        let tasks = markdown::find_tasks(&n.text);
        let mut tags: Vec<String> = markdown::find_tags(&n.text).into_iter().map(|t| t.1).collect();
        tags.extend(markdown::frontmatter_tags(&n.text));
        tags.sort();
        tags.dedup();
        let resolved = links.iter().map(|l| self.resolve(&l.target, Some(rel))).collect();
        let n = self.notes.get_mut(rel).unwrap();
        n.links = links;
        n.tasks = tasks;
        n.tags = tags;
        n.resolved = resolved;
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
        self.files.iter().find(|f| f.rel == rel)
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
        c.sort_by_key(|p| rel_to_string(p).to_lowercase());
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

    /// Notes linking to `rel`: (source, [(line number, line text)]).
    pub fn backlinks(&self, rel: &Path) -> Vec<(PathBuf, Vec<(usize, String)>)> {
        let mut out = Vec::new();
        for (src, n) in &self.notes {
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
        out.sort_by_key(|(p, _)| rel_to_string(p).to_lowercase());
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
        out.sort_by_key(|(p, _)| rel_to_string(p).to_lowercase());
        out
    }

    /// Outgoing links of a note: (display text, target, resolved path).
    pub fn outgoing(&self, rel: &Path) -> Vec<(String, String, Option<PathBuf>)> {
        let Some(n) = self.notes.get(rel) else { return vec![] };
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
        let mut out: Vec<(PathBuf, Task)> = self.notes.iter().flat_map(|(p, n)| n.tasks.iter().map(move |t| (p.clone(), t.clone()))).collect();
        out.sort_by(|a, b| rel_to_string(&a.0).to_lowercase().cmp(&rel_to_string(&b.0).to_lowercase()).then(a.1.line.cmp(&b.1.line)));
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
    /// dropped, and a `folder` outside the vault is replaced by the vault root.
    pub fn new_note_path(&self, folder: &Path, name: &str) -> PathBuf {
        let folder = VaultPath::new(folder).unwrap_or_default();
        let folder = folder.as_path();
        let name = name.trim().trim_start_matches('/');
        let mut parts: Vec<String> = name.split('/').map(markdown::sanitize_note_name).filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            parts.push("Untitled".into());
        }
        let file = parts.pop().unwrap();
        let mut dir = if name.contains('/') { PathBuf::new() } else { folder.to_path_buf() };
        for p in parts {
            dir.push(p);
        }
        let (stem, ext) = if let Some(s) = crate::excalidraw_md::strip_suffix_ci(&file, ".excalidraw.md") {
            (s.to_string(), "excalidraw.md")
        } else if let Some(s) = crate::excalidraw_md::strip_suffix_ci(&file, ".excalidraw") {
            (s.to_string(), "excalidraw")
        } else {
            (markdown::strip_md_ext(&file).to_string(), "md")
        };
        let mut candidate = dir.join(format!("{stem}.{ext}"));
        let mut i = 1;
        while self.abs(&candidate).exists() {
            candidate = dir.join(format!("{stem} {i}.{ext}"));
            i += 1;
        }
        candidate
    }

    /// Create a file with content; makes parent folders. Returns its relative path.
    pub fn create_file(&mut self, rel: &Path, content: &str) -> io::Result<PathBuf> {
        let abs = self.abs(rel);
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(&abs, content)?;
        self.rescan();
        Ok(rel.to_path_buf())
    }

    pub fn create_folder(&mut self, rel: &VaultPath) -> io::Result<()> {
        fs::create_dir_all(self.abs(rel.as_path()))?;
        self.rescan();
        Ok(())
    }

    /// Write a note to disk and keep the index in step (so the write is not seen as an outside change).
    pub fn write_note(&mut self, rel: &Path, text: &str) -> io::Result<()> {
        let abs = self.abs(rel);
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(&abs, text)?;
        let mtime = fs::metadata(&abs).and_then(|m| m.modified()).ok();
        self.set_note_text(rel, text);
        if let Some(n) = self.notes.get_mut(rel) {
            n.mtime = mtime;
        }
        Ok(())
    }

    pub fn write_raw(&mut self, rel: &Path, data: &str) -> io::Result<()> {
        let abs = self.abs(rel);
        if let Some(p) = abs.parent() {
            fs::create_dir_all(p)?;
        }
        fs::write(abs, data)
    }

    /// Move a file or a whole folder into the folder `into` (keeping its name). Links are kept
    /// pointing at what they pointed to (VAULT-015, VAULT-016). Returns the new path and the
    /// notes whose text changed.
    pub fn move_into(&mut self, from: &Path, into: &Path) -> io::Result<(PathBuf, Vec<PathBuf>)> {
        let name = from.file_name().ok_or_else(|| io::Error::other("nothing to move"))?;
        let to = into.join(name);
        if to == from {
            return Ok((to, vec![]));
        }
        if into.starts_with(from) {
            return Err(io::Error::other("a folder can't go into itself"));
        }
        if self.abs(&to).exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("“{}” already has a “{}”", if into.as_os_str().is_empty() { "the vault".into() } else { rel_to_string(into) }, name.to_string_lossy())));
        }
        let changed = self.relocate(from, &to, true)?;
        Ok((to, changed))
    }

    /// Rename a file or folder (`to` is its new vault path). When `update_links`, links are
    /// kept pointing at what they pointed to (VAULT-013, VAULT-016). Returns the notes whose
    /// text was rewritten.
    pub fn rename(&mut self, from: &Path, to: &Path, update_links: bool) -> io::Result<Vec<PathBuf>> {
        self.relocate(from, to, update_links)
    }

    /// The one rewrite routine for rename and move (VAULT-016). Before the change, every link
    /// that resolves is noted with the file it points to (after the change: its new place if it
    /// moved). After the change, each link that would now resolve somewhere else is rewritten to
    /// the shortest form that resolves to that file. Links that were red are left alone.
    fn relocate(&mut self, from: &Path, to: &Path, update_links: bool) -> io::Result<Vec<PathBuf>> {
        if self.abs(to).exists() {
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
            for (src, n) in &self.notes {
                let links: Vec<(LinkSpan, PathBuf)> = n.links.iter().zip(&n.resolved).filter_map(|(l, r)| r.as_ref().map(|r| (l.clone(), moved(r).unwrap_or_else(|| r.clone())))).collect();
                if !links.is_empty() {
                    plan.push((moved(src).unwrap_or_else(|| src.clone()), n.text.clone(), links));
                }
            }
        }
        if let Some(p) = self.abs(to).parent() {
            fs::create_dir_all(p)?;
        }
        fs::rename(self.abs(from), self.abs(to))?;
        self.rescan();
        let mut changed = Vec::new();
        for (src, text, links) in plan {
            // changed on disk in the meantime: leave it alone rather than edit the wrong place
            if self.notes.get(&src).map(|n| n.text != text).unwrap_or(true) {
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
            if self.rewrite_note(&src, &text, edits)? {
                changed.push(src);
            }
        }
        Ok(changed)
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

    /// Apply `edits` (range in `text` -> new text) to the note `rel`, whose text is `text`, write
    /// it in one step and update the index. Returns whether anything changed.
    fn rewrite_note(&mut self, rel: &Path, text: &str, mut edits: Vec<(std::ops::Range<usize>, String)>) -> io::Result<bool> {
        edits.sort_by_key(|e| std::cmp::Reverse(e.0.start));
        let mut out = text.to_string();
        for (r, t) in edits {
            if r.end <= out.len() && out.is_char_boundary(r.start) && out.is_char_boundary(r.end) {
                out.replace_range(r, &t);
            }
        }
        if out == text {
            return Ok(false);
        }
        let abs = self.abs(rel);
        replace_file(&abs, out.as_bytes())?;
        let mtime = fs::metadata(&abs).and_then(|m| m.modified()).ok();
        self.set_note_text(rel, &out);
        if let Some(n) = self.notes.get_mut(rel) {
            n.mtime = mtime;
        }
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

    /// Move a file or folder to `<vault>/.trash/` instead of deleting it for good.
    pub fn trash(&mut self, rel: &Path) -> io::Result<()> {
        let trash = self.root.join(".trash");
        fs::create_dir_all(&trash)?;
        let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "item".into());
        let mut dest = trash.join(&name);
        let mut i = 1;
        while dest.exists() {
            dest = trash.join(format!("{i} {name}"));
            i += 1;
        }
        fs::rename(self.abs(rel), dest)?;
        self.rescan();
        Ok(())
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
        for (src, n) in &self.notes {
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

/// Write a small welcome vault that shows off links, tasks and a drawing.
pub fn write_sample_vault(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join("Projects"))?;
    let files: [(&str, &str); 5] = [
        (
            "Welcome.md",
            "# Welcome to your vault\n\nThis vault is just a folder of plain **markdown** files.\n\n- Links to notes that exist are [[Ideas|blue]] like [[Ideas]].\n- Links to notes that don't exist yet are red, like [[Reading list]]. Click one to create it.\n- Type `[[` to get suggestions while you write.\n- Web links open in your browser: [an example site](https://example.com)\n\n![[z_media/sample-gradient.png]]\n\n## Things to try\n- [ ] Click a red link to create a note\n- [x] Open the vault\n- [ ] Draw something in [[Sketch.excalidraw]]\n- [ ] Open the task list from the left ribbon\n\n## Lists help you type\n1. [x] Press Enter at the end of a list item to start the next one\n2. [ ] Press Tab to indent it, Shift+Tab to bring it back\n3. [ ] In a heading, Tab adds a level and Shift+Tab removes one\n4. [ ] Alt+Up / Alt+Down moves the current line\n5. [ ] Type [! then ~/ to pick a picture from your home folder (Tab completes)\n\n![[Sketch.excalidraw]]\n\n#getting-started\n",
        ),
        (
            "Ideas.md",
            "# Ideas\n\nA place for loose thoughts. Back to [[Welcome]].\n\n- [ ] Plan the [[Projects/Garden plan|garden]]\n- [ ] Write about [[Rust]] 📅 2026-12-01\n\n> Notes get more useful the more they link to each other.\n\n#ideas\n",
        ),
        (
            "Projects/Garden plan.md",
            "# Garden plan\n\nLinked from [[Ideas]].\n\n## Tasks\n- [x] Order seeds\n- [ ] Build raised bed\n- [ ] Ask about [[Compost]]\n\n#project\n",
        ),
        ("Daily log.md", "# Daily log\n\n- Met with team, see [[Ideas]]\n- [ ] Follow up on [[Garden plan]]\n"),
        ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE),
    ];
    for (name, content) in files {
        let p = root.join(name);
        if !p.exists() {
            fs::write(p, content)?;
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
        img.save(&pic).map_err(io::Error::other)?;
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
        assert!(!changed.is_empty());
        let welcome = &v.notes[Path::new("Welcome.md")].text;
        assert!(welcome.contains("[[Thoughts|blue]]"));
        assert!(welcome.contains("[[Thoughts]]"));
        assert!(!v.exists("Ideas", None));
        // duplicate name gets a number
        let p2 = v.new_note_path(Path::new(""), "Thoughts");
        assert_eq!(p2, PathBuf::from("Thoughts 1.md"));
        // trash
        v.trash(Path::new("Reading list.md")).unwrap();
        assert!(!v.exists("Reading list", None));
        assert!(d.join(".trash/Reading list.md").exists());
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
        assert_eq!(changed.len(), 2);
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
        assert_eq!(changed, vec![PathBuf::from("Top.md")]);
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
        assert_eq!(changed, vec![PathBuf::from("b/Note.md")]);
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
}
