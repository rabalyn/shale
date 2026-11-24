//! The rows of the file list (PERF-011): every row the open folders show, top to bottom, in the
//! order of the sort setting, made once per state of the file list and not for every frame. The
//! state is the vault (`uid`, `rescan_epoch`, `generation`), the sort setting and the open
//! folders. The list draws only the rows in view (`panels.rs`), so a frame costs the rows in view,
//! not the notes of the vault.

use crate::config::FileSort;
use crate::vault::{FileEntry, FileKind, Vault};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// One row of the file list.
pub struct Row {
    pub rel: PathBuf,
    /// what the row shows: the folder's name, or the file's display name
    pub name: String,
    pub depth: usize,
    /// `None` for a folder
    pub kind: Option<FileKind>,
    /// the tag at the right end of a file row ("canvas", "drawing", an extension, "conflict")
    pub badge: String,
}

impl Row {
    pub fn is_folder(&self) -> bool {
        self.kind.is_none()
    }
}

type Time = Option<std::time::SystemTime>;
/// An entry of a folder with what its order depends on: the lower-cased name and (created, modified).
type Entry<'a, T> = (String, (Time, Time), &'a T);

/// The entries of every folder, each list in the order of `sort`.
struct Groups<'a> {
    files: HashMap<&'a Path, Vec<Entry<'a, FileEntry>>>,
    folders: HashMap<&'a Path, Vec<Entry<'a, PathBuf>>>,
}

fn groups(v: &Vault, sort: FileSort) -> Groups<'_> {
    // one pass over the vault, not one per folder; each name is lower-cased once
    // a note's times are its `created:` and `modified:` properties, with the fallbacks of VAULT-045 and VAULT-046;
    // read once per entry, and only for the sorts that use them
    let by_time = !matches!(sort, FileSort::NameAsc | FileSort::NameDesc);
    let times = |f: &FileEntry| {
        if f.kind != FileKind::Note || !by_time {
            return (f.created, f.modified);
        }
        let text = v.notes.get(&f.rel).map_or("", |n| n.text.as_str());
        let time = |s: i64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(s.max(0) as u64);
        // the file times stay as exact as the file system has them, so they are the fallback here
        (crate::note_props::created(&f.rel, text, None).map(time).or(f.created), crate::note_props::modified(text, None).map(time).or(f.modified))
    };
    let mut files: HashMap<&Path, Vec<Entry<FileEntry>>> = HashMap::new();
    for f in &v.files {
        files.entry(f.rel.parent().unwrap_or(Path::new(""))).or_default().push((f.display_name().to_lowercase(), times(f), f));
    }
    let mut folders: HashMap<&Path, Vec<Entry<PathBuf>>> = HashMap::new();
    for f in &v.folders {
        let name = f.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        folders.entry(f.parent().unwrap_or(Path::new(""))).or_default().push((name, v.folder_times.get(f).copied().unwrap_or_default(), f));
    }
    for list in files.values_mut() {
        list.sort_by(|a, b| crate::panels::sort_order(sort, &a.0, &b.0, a.1, b.1));
    }
    for list in folders.values_mut() {
        list.sort_by(|a, b| crate::panels::sort_order(sort, &a.0, &b.0, a.1, b.1));
    }
    Groups { files, folders }
}

/// The rows of the open folders, made now ([`TreeRows`] keeps them).
pub fn build(v: &Vault, sort: FileSort, expanded: &HashSet<PathBuf>) -> Vec<Row> {
    fn walk(v: &Vault, g: &Groups, dir: &Path, depth: usize, expanded: &HashSet<PathBuf>, out: &mut Vec<Row>) {
        for (_, _, f) in g.folders.get(dir).into_iter().flatten() {
            let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            out.push(Row { rel: (*f).clone(), name, depth, kind: None, badge: String::new() });
            if expanded.contains(*f) {
                walk(v, g, f, depth + 1, expanded, out);
            }
        }
        for (_, _, f) in g.files.get(dir).into_iter().flatten() {
            let badge = match f.kind {
                FileKind::Drawing if crate::drawing::canvas::is_canvas_path(&f.rel) => "canvas",
                FileKind::Drawing => "drawing",
                FileKind::Other => f.rel.extension().and_then(|e| e.to_str()).unwrap_or(""),
                FileKind::Note if v.conflict_of(&f.rel).is_some() || !v.copies_of(&f.rel).is_empty() => "conflict",
                FileKind::Note => "",
            };
            out.push(Row { rel: f.rel.clone(), name: f.display_name(), depth, kind: Some(f.kind), badge: badge.to_string() });
        }
    }
    let g = groups(v, sort);
    let mut out = Vec::new();
    walk(v, &g, Path::new(""), 0, expanded, &mut out);
    out
}

/// What the rows were made from.
struct Key {
    state: (u64, u64, u64),
    sort: FileSort,
    expanded: HashSet<PathBuf>,
}

/// The rows of the file list, kept while their state stays the same.
#[derive(Default)]
pub struct TreeRows {
    made: Option<(Key, Arc<Vec<Row>>)>,
    /// how many times the rows were made (tests)
    #[cfg(test)]
    pub(crate) builds: usize,
}

impl TreeRows {
    /// The rows for this state; made again only when the state changed since the last call.
    pub fn get(&mut self, v: &Vault, sort: FileSort, expanded: &HashSet<PathBuf>) -> Arc<Vec<Row>> {
        let state = v.tree_state();
        if let Some((k, rows)) = &self.made {
            if k.state == state && k.sort == sort && k.expanded == *expanded {
                return rows.clone();
            }
        }
        #[cfg(test)]
        {
            self.builds += 1;
        }
        let rows = Arc::new(build(v, sort, expanded));
        self.made = Some((Key { state, sort, expanded: expanded.clone() }, rows.clone()));
        rows
    }
}
