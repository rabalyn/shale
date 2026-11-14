//! The vault: a folder of markdown notes and drawings, plus an index of links, tasks and tags.

use crate::markdown::{self, LinkSpan, Task};
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
    /// Bumped whenever anything in the index changes; UI caches key off this.
    pub generation: u64,
}

pub fn rel_to_string(p: &Path) -> String {
    p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/")
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
        for f in &self.files {
            self.name_index.entry(f.link_name.to_lowercase()).or_default().push(f.rel.clone());
            // also allow the full file name (`Note.md`, `Plan.excalidraw.md`)
            let full = f.rel.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
            if full != f.link_name.to_lowercase() {
                self.name_index.entry(full).or_default().push(f.rel.clone());
            }
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
    pub fn resolve(&self, target: &str, from: Option<&Path>) -> Option<PathBuf> {
        let target = target.trim();
        if target.is_empty() {
            return from.map(|p| p.to_path_buf());
        }
        if target.contains('/') {
            let t = target.trim_start_matches('/');
            let lower = t.to_lowercase();
            // vault-absolute path, then relative to the linking note's folder
            let mut bases = vec![PathBuf::new()];
            if let Some(dir) = from.and_then(|f| f.parent()) {
                bases.insert(0, dir.to_path_buf());
            }
            for base in bases {
                for f in &self.files {
                    let rel = rel_to_string(&f.rel).to_lowercase();
                    let b = rel_to_string(&base).to_lowercase();
                    let want = if b.is_empty() { lower.clone() } else { format!("{b}/{lower}") };
                    if rel == want || rel == format!("{want}.md") {
                        return Some(f.rel.clone());
                    }
                }
            }
            // Fall back to file-name only.
            let name = t.rsplit('/').next().unwrap_or(t);
            return self.resolve(name, from);
        }
        let cands = self.name_index.get(&target.to_lowercase())?;
        if cands.len() == 1 {
            return Some(cands[0].clone());
        }
        let from_dir = from.and_then(|f| f.parent()).map(|p| p.to_path_buf()).unwrap_or_default();
        let mut best = cands.clone();
        best.sort_by_key(|c| {
            let same = c.parent().map(|p| p == from_dir).unwrap_or(false);
            (!same, c.components().count(), rel_to_string(c))
        });
        best.into_iter().next()
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
        let mut out: Vec<(PathBuf, Task)> = self
            .notes
            .iter()
            .flat_map(|(p, n)| n.tasks.iter().map(move |t| (p.clone(), t.clone())))
            .collect();
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
    pub fn search(&self, query: &str) -> Vec<(PathBuf, Vec<(usize, usize, String)>)> {
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
    pub fn new_note_path(&self, folder: &Path, name: &str) -> PathBuf {
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

    pub fn create_folder(&mut self, rel: &Path) -> io::Result<()> {
        fs::create_dir_all(self.abs(rel))?;
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

    /// Rename/move a file or folder. When `update_links`, links in other notes are rewritten.
    /// Returns the notes whose text was rewritten.
    /// Move a file or a whole folder into the folder `into` (keeping its name). Links written as
    /// a path (`[[Projects/Plan]]`, `[x](Projects/Plan.md)`) to anything that moved are changed to
    /// the new place; links by name keep working as they are. Returns the new path and the notes
    /// whose text changed.
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
        let is_dir = self.abs(from).is_dir();
        let moved = |p: &Path| -> Option<PathBuf> {
            if p == from {
                Some(to.clone())
            } else if is_dir {
                p.strip_prefix(from).ok().map(|rest| to.join(rest))
            } else {
                None
            }
        };
        // path links to what moves, before the move: (note now, its new path, [(range, new text)])
        let mut edits: Vec<(PathBuf, PathBuf, Vec<(std::ops::Range<usize>, String)>)> = Vec::new();
        for (src, n) in &self.notes {
            let mut ranges = Vec::new();
            for (l, r) in n.links.iter().zip(&n.resolved) {
                let (Some(r), true) = (r, l.target.contains('/')) else { continue };
                let Some(new) = moved(r) else { continue };
                let old_text = &n.text[l.target_range.clone()];
                let mut t = rel_to_string(&new);
                if !old_text.to_ascii_lowercase().ends_with(".md") {
                    t = markdown::strip_md_ext(&t).to_string();
                }
                if l.markdown_style {
                    t = t.replace(' ', "%20");
                }
                ranges.push((l.target_range.clone(), t));
            }
            if !ranges.is_empty() {
                let now = moved(src).unwrap_or_else(|| src.clone());
                edits.push((src.clone(), now, ranges));
            }
        }
        if let Some(p) = self.abs(&to).parent() {
            fs::create_dir_all(p)?;
        }
        fs::rename(self.abs(from), self.abs(&to))?;
        let mut changed = Vec::new();
        for (src, now, mut ranges) in edits {
            let Some(text) = self.notes.get(&src).map(|n| n.text.clone()) else { continue };
            ranges.sort_by_key(|r| std::cmp::Reverse(r.0.start));
            let mut out = text;
            for (r, t) in ranges {
                if out.is_char_boundary(r.start) && out.is_char_boundary(r.end) && r.end <= out.len() {
                    out.replace_range(r, &t);
                }
            }
            fs::write(self.abs(&now), &out)?;
            changed.push(now);
        }
        self.rescan();
        Ok((to, changed))
    }

    pub fn rename(&mut self, from: &Path, to: &Path, update_links: bool) -> io::Result<Vec<PathBuf>> {
        let abs_from = self.abs(from);
        let abs_to = self.abs(to);
        if abs_to.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "a file with that name already exists"));
        }
        if let Some(p) = abs_to.parent() {
            fs::create_dir_all(p)?;
        }
        let old_entry = self.file(from).cloned();
        // Who links here, before the move.
        let linkers: Vec<PathBuf> = if update_links && old_entry.is_some() {
            self.notes
                .iter()
                .filter(|(_, n)| n.resolved.iter().any(|r| r.as_deref() == Some(from)))
                .map(|(p, _)| p.clone())
                .collect()
        } else {
            vec![]
        };
        fs::rename(&abs_from, &abs_to)?;
        let mut changed = Vec::new();
        if let Some(old) = old_entry {
            let new_name = link_name_of(to, old.kind);
            if new_name != old.link_name {
                for src in linkers {
                    let src_now = if src == from { to.to_path_buf() } else { src.clone() };
                    let text = match self.notes.get(&src) {
                        Some(n) => n.text.clone(),
                        None => continue,
                    };
                    let (new_text, n) = markdown::rename_links(&text, &old.link_name, &new_name);
                    if n > 0 {
                        fs::write(self.abs(&src_now), &new_text)?;
                        changed.push(src_now);
                    }
                }
            }
        }
        self.rescan();
        Ok(changed)
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

pub fn write_sample_vault(root: &Path) -> io::Result<()> {
    fs::create_dir_all(root.join("Projects"))?;
    let files: [(&str, &str); 6] = [
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
        ("Overview.canvas", SAMPLE_CANVAS),
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
}
