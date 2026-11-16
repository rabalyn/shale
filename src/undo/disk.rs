//! The undo histories in the vault (EDIT-021): one versioned file per note in
//! `<vault>/.shale/undo/`, named by a hash of the note's vault path.

use super::History;
use crate::data_usage::{files, CacheStats, Freed};
use crate::vault_state::{self, UndoFile};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex, MutexGuard};

/// The undo folders being worked on. A folder is held while one of its files is read,
/// written, moved or deleted, and for the whole run of a clear or prune of the cache (USE-017): a
/// step written meanwhile is never half-seen by the job, and the job's rewrite never replaces a
/// newer file. One gate per folder, so different vaults never wait for each other.
static BUSY: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
static FREED: Condvar = Condvar::new();

/// The gate of one undo folder; dropping it frees the folder.
pub(super) struct Gate(PathBuf);

impl Drop for Gate {
    fn drop(&mut self) {
        busy().retain(|d| d != &self.0);
        FREED.notify_all();
    }
}

fn busy() -> MutexGuard<'static, Vec<PathBuf>> {
    BUSY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Hold the undo folder of `root`, waiting while someone else holds it.
pub(super) fn gate(root: &Path) -> Gate {
    let d = dir(root);
    let mut held = busy();
    while held.contains(&d) {
        held = FREED.wait(held).unwrap_or_else(|e| e.into_inner());
    }
    held.push(d.clone());
    Gate(d)
}

/// The gate if the folder is free now, `None` while a cleanup job holds it.
fn try_gate(root: &Path) -> Option<Gate> {
    let d = dir(root);
    let mut held = busy();
    if held.contains(&d) {
        return None;
    }
    held.push(d.clone());
    Some(Gate(d))
}

/// The time before which a step is too old, `None` when steps are kept forever.
pub(super) fn oldest_kept(now: u64, keep_days: Option<u32>) -> Option<u64> {
    keep_days.map(|d| now.saturating_sub(u64::from(d) * 24 * 3600))
}

fn dir(root: &Path) -> PathBuf {
    root.join(".shale").join("undo")
}

/// The path as the file holds it: vault-relative with '/' on every system.
fn key(rel: &Path) -> String {
    rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
}

/// FNV-1a, 64 bit: the same on every Rust version and system (unlike `DefaultHasher`).
pub(super) fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

pub(super) fn file_of(root: &Path, rel: &Path) -> PathBuf {
    dir(root).join(format!("{:016x}.json", fnv1a(&key(rel))))
}

/// Read the history of `rel`: an empty one when there is no file, and a problem to tell the user
/// when the file was broken or from a newer Shale. Old steps are dropped.
pub(super) fn read(root: &Path, rel: &Path, now: u64, keep_days: Option<u32>) -> (History, Option<String>) {
    let _gate = gate(root);
    let loaded = vault_state::load::<UndoFile>(&file_of(root, rel));
    let mut h = History::default();
    if loaded.value.path != key(rel) && loaded.existed {
        // another note with the same hash: leave its file alone
        h.locked = true;
        return (h, loaded.problem);
    }
    let f = loaded.value;
    h.undo = f.undo.into();
    h.redo = f.redo;
    h.known = f.known;
    h.locked = !loaded.writable;
    h.prune(now, keep_days);
    (h, loaded.problem)
}

/// Write the history of `rel` atomically (without the steps older than the retention); a history
/// without steps removes the file. Without `wait`, nothing is done while a cleanup job runs
/// (`Ok(false)`), so the window never waits for one; the next frame tries again.
pub(super) fn write(root: &Path, rel: &Path, h: &mut History, now: u64, keep_days: Option<u32>, wait: bool) -> std::io::Result<bool> {
    let Some(_gate) = (if wait { Some(gate(root)) } else { try_gate(root) }) else { return Ok(false) };
    h.prune(now, keep_days);
    let path = file_of(root, rel);
    if h.is_empty() {
        return remove_file(&path).map(|()| true);
    }
    let f = UndoFile { path: key(rel), undo: h.undo.iter().cloned().collect(), redo: h.redo.clone(), known: h.known.clone(), ..UndoFile::default() };
    vault_state::save(&path, &f, true).map(|_| true)
}

fn remove_file(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Delete the file of `rel` (best effort: a file that stays is only a stale copy).
pub(super) fn remove(root: &Path, rel: &Path) {
    let _gate = gate(root);
    let _ = remove_file(&file_of(root, rel));
}

/// What a file of the folder is.
enum Item {
    /// a history this Shale understands
    History(UndoFile),
    /// from a newer Shale: left alone
    Newer,
    /// a `.json` file that does not parse
    Unreadable,
    /// a damaged copy, `<name>.json.broken-<date>` (CFG-004)
    Damaged,
}

/// The files of the cache: the histories and the damaged copies, by their names. Nothing else
/// in the folder is listed.
fn listing(root: &Path) -> Vec<(files::FileEntry, Item)> {
    files::entries(&dir(root), |n| n.ends_with(".json") || n.contains(".json.broken-"))
        .into_iter()
        .map(|f| {
            let item = if f.path.extension().is_none_or(|x| x != "json") {
                Item::Damaged
            } else {
                match crate::fsutil::read(&f.path).map_err(|e| e.to_string()).and_then(|b| vault_state::parse::<UndoFile>(&b)) {
                    Ok((u, false)) => Item::History(u),
                    Ok((_, true)) => Item::Newer,
                    Err(_) => Item::Unreadable,
                }
            };
            (f, item)
        })
        .collect()
}

/// The history files that parse and are not from a newer Shale: (file, content).
fn histories(root: &Path) -> Vec<(PathBuf, UndoFile)> {
    listing(root).into_iter().filter_map(|(f, i)| if let Item::History(u) = i { Some((f.path, u)) } else { None }).collect()
}

/// Size, number of files and the time of the oldest step (the file's date when there is none)
/// of the histories and damaged copies of the vault (USE-006).
pub fn stats_of_files(root: &Path) -> CacheStats {
    let _gate = gate(root);
    let all = listing(root);
    let mut s = files::stats(&all.iter().map(|(f, _)| f.clone()).collect::<Vec<_>>());
    let oldest_step = all.iter().filter_map(|(_, i)| if let Item::History(u) = i { u.undo.iter().chain(&u.redo).map(|e| e.time as i64).min() } else { None }).min();
    s.oldest = oldest_step.or(s.oldest);
    s
}

/// The prune of the cache (USE-017): steps older than `keep_days` go from every file, a file
/// with no step left is deleted (its note may be long gone), every damaged copy is deleted.
/// Files from a newer Shale and unreadable ones stay. `entries` counts the files deleted or
/// rewritten, `bytes` what they took less.
pub fn prune_files(root: &Path, now: u64, keep_days: Option<u32>) -> Result<Freed, String> {
    let _gate = gate(root);
    let mut gone = vec![];
    let mut freed = Freed::default();
    let mut first_error = None;
    for (f, item) in listing(root) {
        match item {
            Item::Damaged => gone.push(f),
            Item::History(mut u) => {
                let before = u.undo.len() + u.redo.len();
                if let Some(oldest) = oldest_kept(now, keep_days) {
                    u.undo.retain(|e| e.time >= oldest);
                    u.redo.retain(|e| e.time >= oldest);
                }
                let left = u.undo.len() + u.redo.len();
                if left == 0 {
                    gone.push(f);
                } else if left < before {
                    match vault_state::save(&f.path, &u, true) {
                        Ok(_) => {
                            freed.entries += 1;
                            freed.bytes += f.bytes.saturating_sub(std::fs::metadata(&f.path).map(|m| m.len()).unwrap_or(0));
                        }
                        Err(e) => {
                            first_error.get_or_insert_with(|| format!("{}: {e}", f.path.display()));
                        }
                    }
                }
            }
            Item::Newer | Item::Unreadable => {}
        }
    }
    match (files::remove(&gone), first_error) {
        (Ok(r), None) => Ok(Freed { bytes: freed.bytes + r.bytes, entries: freed.entries + r.entries }),
        (Err(e), _) | (_, Some(e)) => Err(e),
    }
}

/// The clear of the cache (USE-017): delete every history file and damaged copy, not the files
/// from a newer Shale and nothing else in the folder.
pub fn clear_files(root: &Path) -> Result<Freed, String> {
    let _gate = gate(root);
    let mine: Vec<files::FileEntry> = listing(root).into_iter().filter(|(_, i)| !matches!(i, Item::Newer)).map(|(f, _)| f).collect();
    files::remove(&mine)
}

/// Move the files of notes that are not loaded: `map` gives the new path of a moved note.
pub(super) fn move_unloaded(root: &Path, map: &impl Fn(&Path) -> Option<PathBuf>, loaded: &HashMap<PathBuf, History>) {
    let _gate = gate(root);
    for (file, mut f) in histories(root) {
        let old = PathBuf::from(&f.path);
        let Some(new) = map(&old).filter(|_| !loaded.contains_key(&old)) else { continue };
        f.path = key(&new);
        if vault_state::save(&file_of(root, &new), &f, true).is_ok() && file_of(root, &new) != file {
            let _ = remove_file(&file);
        }
    }
}
