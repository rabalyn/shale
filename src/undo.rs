//! Undo history of the notes for vim-lite (EDIT-021) and its files in the vault (EDIT-022), with
//! no UI.
//!
//! A history belongs to a note (its vault path), not to a tab, so tabs of the same note share it.
//! It stores steps as small edits (a char range, the text that went and the text that came), found
//! by comparing the text at the last checkpoint with the text now; the only full copy is the "last
//! known text". Vim-lite checkpoints before every command of normal and visual mode, so an insert
//! session (and any change from outside vim-lite) becomes one step.

mod disk;

pub use disk::{clear_files, prune_files, stats_of_files};

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// How many steps a note keeps.
pub const MAX_STEPS: usize = 200;

/// How long after a change the history is written (EDIT-022), in seconds.
pub const DEBOUNCE: u64 = 3;

/// One step: at char index `at`, `removed` was replaced by `inserted`; `time` is when it was made
/// (unix seconds).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub at: usize,
    pub removed: String,
    pub inserted: String,
    #[serde(default)]
    pub time: u64,
}

impl Edit {
    /// The edit that turns `old` into `new`, from their common prefix and suffix.
    fn between(old: &str, new: &str, time: u64) -> Edit {
        let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
        let prefix = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
        let max_suffix = a.len().min(b.len()) - prefix;
        let suffix = a.iter().rev().zip(b.iter().rev()).take(max_suffix).take_while(|(x, y)| x == y).count();
        let (mut at, mut removed, mut inserted): (usize, Vec<char>, Vec<char>) = (prefix, a[prefix..a.len() - suffix].to_vec(), b[prefix..b.len() - suffix].to_vec());
        // a pure insertion or deletion is ambiguous when its text repeats the text before it
        // ("one\nt|hree" gaining "wo\nt"): put it as far left as it goes, where a person made it
        let pure = if removed.is_empty() {
            Some(&mut inserted)
        } else if inserted.is_empty() {
            Some(&mut removed)
        } else {
            None
        };
        if let Some(moved) = pure {
            while at > 0 && moved.last() == a.get(at - 1) {
                moved.rotate_right(1);
                at -= 1;
            }
        }
        Edit { at, removed: removed.iter().collect(), inserted: inserted.iter().collect(), time }
    }

    fn reversed(&self) -> Edit {
        Edit { at: self.at, removed: self.inserted.clone(), inserted: self.removed.clone(), time: self.time }
    }

    /// Replace `removed` by `inserted` in `text`, which must have `removed` at `at`.
    fn apply(&self, text: &mut String) {
        let byte = |n: usize| text.char_indices().nth(n).map(|(i, _)| i).unwrap_or(text.len());
        let (a, b) = (byte(self.at), byte(self.at + self.removed.chars().count()));
        text.replace_range(a..b, &self.inserted);
    }

    /// Where the cursor goes after the edit: its start, on the first character of the text that
    /// came when that starts with a line break (so a restored last line is not left behind). A
    /// redone deletion of whole lines puts it on the line that took their place, as vim does;
    /// an undone one (an `o` taken back) leaves it on the line above, where the `o` was typed.
    fn landing(&self, redo: bool) -> usize {
        let breaks = |s: &str| s.starts_with('\n') && s.chars().count() > 1;
        let skip = breaks(&self.inserted) || (redo && self.inserted.is_empty() && breaks(&self.removed));
        self.at + usize::from(skip)
    }
}

/// The undo and redo steps of one note.
#[derive(Default, Debug)]
pub struct History {
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    /// the text at the last checkpoint (or after the last undo / redo)
    known: Option<String>,
    /// the time of the frame (unix seconds), set by `Histories::of`; stamps new steps
    now: u64,
    /// since when the history differs from its file (EDIT-022); `None`: nothing to write
    dirty: Option<u64>,
    /// the file is from a newer Shale or could not be read: never written
    locked: bool,
    /// the last write failed (and was reported)
    failed: bool,
}

impl History {
    fn touch(&mut self) {
        self.dirty.get_or_insert(self.now);
    }

    /// Drop the steps older than `keep_days` (`None`: forever, EDIT-022); true when some went.
    fn prune(&mut self, now: u64, keep_days: Option<u32>) -> bool {
        let Some(oldest) = disk::oldest_kept(now, keep_days) else { return false };
        let before = self.undo.len() + self.redo.len();
        self.undo.retain(|e| e.time >= oldest);
        self.redo.retain(|e| e.time >= oldest);
        self.undo.len() + self.redo.len() < before
    }

    fn is_empty(&self) -> bool {
        self.undo.is_empty() && self.redo.is_empty()
    }

    /// Record what changed since the last checkpoint as one step.
    pub fn checkpoint(&mut self, text: &str) {
        match &self.known {
            Some(k) if k == text => return,
            Some(k) => {
                self.undo.push_back(Edit::between(k, text, self.now));
                if self.undo.len() > MAX_STEPS {
                    self.undo.pop_front();
                }
                self.redo.clear();
                self.touch();
            }
            None => {}
        }
        self.known = Some(text.to_string());
    }

    /// Undo the last step in `text`; returns the char index for the cursor, `None` when there is
    /// nothing to undo.
    pub fn undo(&mut self, text: &mut String) -> Option<usize> {
        self.checkpoint(text);
        let back = self.undo.pop_back()?.reversed();
        back.apply(text);
        self.known = Some(text.clone());
        let at = back.landing(false);
        self.redo.push(back.reversed());
        self.touch();
        Some(at)
    }

    /// Redo the last undone step in `text`; returns the char index for the cursor.
    pub fn redo(&mut self, text: &mut String) -> Option<usize> {
        self.checkpoint(text);
        let again = self.redo.pop()?;
        again.apply(text);
        self.known = Some(text.clone());
        let at = again.landing(true);
        self.undo.push_back(again);
        self.touch();
        Some(at)
    }
}

/// The histories of all notes, by vault path (app-level state). They are read from the vault when
/// a note's editor first asks for its history and written back by `tick` and `flush_all`.
#[derive(Debug)]
pub struct Histories {
    /// the vault folder; empty: nothing is read or written (tests of the editor)
    root: PathBuf,
    now: u64,
    /// how long steps are kept, in days (USE-020); `None`: forever
    keep_days: Option<u32>,
    by_note: HashMap<PathBuf, History>,
    problems: Vec<String>,
}

impl Default for Histories {
    fn default() -> Self {
        Histories { root: PathBuf::new(), now: 0, keep_days: crate::config::CacheRetention::default().undo_days, by_note: HashMap::new(), problems: vec![] }
    }
}

impl Histories {
    /// Switch to the vault at `root`: the histories of the vault being left are written first.
    /// Old files are not purged here, the cleanup of the caches does it (USE-017).
    pub fn open_vault(&mut self, root: &Path, now: u64) {
        self.flush_all();
        self.root = root.to_path_buf();
        self.now = now;
        self.by_note.clear();
    }

    /// Follow the *keep for* setting of the undo histories (USE-020): the loaded histories lose
    /// the steps older than that at once, and what is read and written from now on follows it.
    pub fn set_keep_days(&mut self, days: Option<u32>) {
        if self.keep_days != days {
            self.keep_days = days;
            self.prune_loaded();
        }
    }

    /// Drop the steps older than the setting from the loaded histories (the cleanup, USE-017);
    /// a history that lost steps is written again.
    pub fn prune_loaded(&mut self) {
        let (now, keep) = (self.now, self.keep_days);
        for h in self.by_note.values_mut() {
            h.now = now;
            if h.prune(now, keep) {
                h.touch();
            }
        }
    }

    /// Forget every step of the loaded histories (Clear, USE-017). The files are deleted by the
    /// job; the text at the last checkpoint stays, so the next change is a step as before.
    pub fn clear_loaded(&mut self) {
        for h in self.by_note.values_mut() {
            h.undo.clear();
            h.redo.clear();
            h.dirty = None;
        }
    }

    /// Write every loaded history that has steps again: after a clear or prune ended, a step
    /// made and written while its job ran may have been deleted by it (USE-017).
    pub fn rewrite_loaded(&mut self) {
        for h in self.by_note.values_mut().filter(|h| !h.is_empty()) {
            h.touch();
        }
    }

    /// The history of the note `rel`: read from the vault the first time, an empty one for a note
    /// that has none yet. Steps made through it carry the time of the last `tick`.
    pub fn of(&mut self, rel: &Path) -> &mut History {
        let (root, now, keep) = (&self.root, self.now, self.keep_days);
        let problems = &mut self.problems;
        let h = self.by_note.entry(rel.to_path_buf()).or_insert_with(|| {
            if root.as_os_str().is_empty() {
                return History::default();
            }
            let (h, problem) = disk::read(root, rel, now, keep);
            problems.extend(problem);
            h
        });
        h.now = now;
        h
    }

    /// A frame at `now`: write the histories that changed more than `DEBOUNCE` seconds ago.
    pub fn tick(&mut self, now: u64) {
        self.now = now;
        self.flush(false);
    }

    /// Write every changed history now (a note closed, the app quits, the vault is left).
    pub fn flush_all(&mut self) {
        self.flush(true);
    }

    /// Some history has changes that are not written yet.
    pub fn has_pending(&self) -> bool {
        self.by_note.values().any(|h| h.dirty.is_some())
    }

    /// What went wrong since the last call, for the user.
    pub fn take_problems(&mut self) -> Vec<String> {
        std::mem::take(&mut self.problems)
    }

    fn flush(&mut self, force: bool) {
        if self.root.as_os_str().is_empty() {
            return;
        }
        let (now, keep) = (self.now, self.keep_days);
        for (rel, h) in &mut self.by_note {
            let Some(since) = h.dirty else { continue };
            if h.locked {
                h.dirty = None;
            } else if force || now >= since + DEBOUNCE {
                match disk::write(&self.root, rel, h, now, keep, force) {
                    Ok(true) => (h.dirty, h.failed) = (None, false),
                    // a cleanup job holds the folder: stays dirty, tried again next frame
                    Ok(false) => {}
                    Err(e) => {
                        // kept in memory and tried again later; said once
                        h.dirty = Some(now);
                        if !std::mem::replace(&mut h.failed, true) {
                            self.problems.push(format!("The undo history of “{}” could not be saved: {e}", rel.display()));
                        }
                    }
                }
            }
        }
    }

    /// A note or folder moved: its histories follow, and so do their files, also those of notes
    /// that were not opened this session (`map` gives the new path, if it moves).
    pub fn moved(&mut self, map: impl Fn(&Path) -> Option<PathBuf>) {
        let moving: Vec<(PathBuf, PathBuf)> = self.by_note.keys().filter_map(|k| map(k).map(|n| (k.clone(), n))).collect();
        let has_root = !self.root.as_os_str().is_empty();
        for (from, to) in moving {
            if let Some(mut h) = self.by_note.remove(&from) {
                if has_root {
                    disk::remove(&self.root, &from);
                }
                h.dirty = Some(self.now);
                self.by_note.insert(to, h);
            }
        }
        if has_root {
            disk::move_unloaded(&self.root, &map, &self.by_note);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A history that has seen `first`, then the later texts, one checkpoint each.
    fn after(texts: &[&str]) -> (History, String) {
        let mut h = History::default();
        for t in texts {
            h.checkpoint(t);
        }
        (h, texts.last().unwrap().to_string())
    }

    // spec: EDIT-021
    #[test]
    fn a_step_is_the_difference_between_two_checkpoints() {
        let e = Edit::between("one two three", "one 2 three", 7);
        assert_eq!(e, Edit { at: 4, removed: "two".into(), inserted: "2".into(), time: 7 });
        let e = Edit::between("aaa", "aa", 0);
        assert_eq!((e.at, e.removed.as_str(), e.inserted.as_str()), (0, "a", ""), "a repeat is taken at its first place");
        let e = Edit::between("one\nthree", "one\ntwo\nthree", 0);
        assert_eq!((e.at, e.inserted.as_str()), (3, "\ntwo"));
        let e = Edit::between("é ü ß", "é ü X ß", 0);
        assert_eq!((e.at, e.removed.as_str(), e.inserted.as_str()), (3, "", " X"), "characters, not bytes");
        let mut t = "é ü ß".to_string();
        e.apply(&mut t);
        assert_eq!(t, "é ü X ß");
        e.reversed().apply(&mut t);
        assert_eq!(t, "é ü ß");
    }

    // spec: EDIT-021
    #[test]
    fn undo_and_redo_walk_the_steps() {
        let (mut h, mut t) = after(&["a", "ab", "abc"]);
        assert_eq!(h.undo(&mut t), Some(2));
        assert_eq!(t, "ab");
        assert_eq!(h.undo(&mut t), Some(1));
        assert_eq!(t, "a");
        assert_eq!(h.undo(&mut t), None, "nothing older");
        assert_eq!(h.redo(&mut t), Some(1));
        assert_eq!(h.redo(&mut t), Some(2));
        assert_eq!(t, "abc");
        assert_eq!(h.redo(&mut t), None, "nothing newer");
    }

    // spec: EDIT-021
    #[test]
    fn a_new_change_drops_the_redo_steps() {
        let (mut h, mut t) = after(&["a", "ab", "abc"]);
        h.undo(&mut t);
        t.push('X');
        assert_eq!(h.redo(&mut t), None, "the change after the undo dropped the redo");
        assert_eq!(t, "abX");
        h.undo(&mut t);
        assert_eq!(t, "ab", "the new change is one step");
    }

    // spec: EDIT-021
    #[test]
    fn a_change_from_outside_is_a_step_of_its_own() {
        let (mut h, _) = after(&["one", "one two"]);
        let mut t = "ONE two".to_string(); // moved lines, a reload, another tab
        assert_eq!(h.undo(&mut t), Some(0));
        assert_eq!(t, "one two", "the outside change goes first");
        h.undo(&mut t);
        assert_eq!(t, "one");
    }

    // spec: EDIT-021
    #[test]
    fn only_the_last_200_steps_are_kept() {
        let mut h = History::default();
        let mut t = String::new();
        h.checkpoint(&t);
        for _ in 0..(MAX_STEPS + 20) {
            t.push('x');
            h.checkpoint(&t);
        }
        let mut n = 0;
        while h.undo(&mut t).is_some() {
            n += 1;
        }
        assert_eq!((n, t.len()), (MAX_STEPS, 20));
    }

    // spec: EDIT-021
    #[test]
    fn the_cursor_lands_on_a_restored_last_line() {
        let (mut h, mut t) = after(&["a\nb", "a"]);
        assert_eq!(h.undo(&mut t), Some(2), "on the b, not on the line break before it");
        assert_eq!(h.redo(&mut t), Some(2));
        assert_eq!(t, "a");
        // a middle line deleted again: the cursor is on the line that came up
        let (mut h, mut t) = after(&["a\nb\nc", "a\nc"]);
        h.undo(&mut t);
        assert_eq!(h.redo(&mut t), Some(2));
        assert_eq!(t, "a\nc");
    }

    // spec: EDIT-021
    #[test]
    fn histories_belong_to_the_path_and_follow_a_move() {
        let mut all = Histories::default();
        all.of(Path::new("a.md")).checkpoint("x");
        all.of(Path::new("a.md")).checkpoint("xy");
        assert_eq!(all.of(Path::new("b.md")).undo.len(), 0, "another note has its own");
        all.moved(|p| (p == Path::new("a.md")).then(|| PathBuf::from("c.md")));
        let mut t = "xy".to_string();
        assert_eq!(all.of(Path::new("c.md")).undo(&mut t), Some(1));
        assert_eq!(t, "x");
        assert_eq!(all.of(Path::new("a.md")).undo.len(), 0);
    }

    // --- the history in the vault (EDIT-022) ---

    const T0: u64 = 1_800_000_000;
    const DAY: u64 = 24 * 3600;

    fn vault(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-undo-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// An app session on the vault at time `now`.
    fn session(root: &Path, now: u64) -> Histories {
        let mut all = Histories::default();
        all.open_vault(root, now);
        all
    }

    /// The history of `rel` after the texts, one checkpoint each, at `now`.
    fn typed(all: &mut Histories, rel: &str, texts: &[&str]) {
        for t in texts {
            all.of(Path::new(rel)).checkpoint(t);
        }
    }

    fn files_in(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(root.join(".shale/undo")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        v.sort();
        v
    }

    // spec: NET-016
    #[test]
    fn the_histories_of_a_synced_vault_are_written_outside_the_vault_and_read_back() {
        let (root, base) = (vault("outside-synced"), vault("outside-synced-base"));
        assert_eq!(crate::device_local::place(&root, true, &base), None);
        let mut a = session(&root, T0);
        typed(&mut a, "n.md", &["a", "ab"]);
        a.flush_all();
        assert!(files_in(&root).is_empty(), "nothing in the vault's .shale/undo");
        let out = crate::device_local::local_dir(&root).join("undo");
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
        let mut t = "ab".to_string();
        assert_eq!(session(&root, T0 + 5).of(Path::new("n.md")).undo(&mut t), Some(1));
        // the cleanup of the cache looks there too
        assert_eq!(super::stats_of_files(&root).entries, 1);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&base);
    }

    // spec: EDIT-022
    #[test]
    fn the_history_is_written_and_read_back_after_a_restart() {
        let root = vault("roundtrip");
        let mut a = session(&root, T0);
        typed(&mut a, "dir/a.md", &["a", "ab", "abc"]);
        let mut t = "abc".to_string();
        a.of(Path::new("dir/a.md")).undo(&mut t);
        a.flush_all();
        let names = files_in(&root);
        assert_eq!(names.len(), 1);
        assert!(names[0].len() == 21 && names[0].ends_with(".json"), "a hash as the name: {names:?}");
        let text = std::fs::read_to_string(root.join(".shale/undo").join(&names[0])).unwrap();
        assert!(text.contains("\"path\": \"dir/a.md\"") && text.contains("\"version\": 1"), "{text}");
        // the app restarts: undo and redo steps are there
        let mut b = session(&root, T0 + 60);
        let h = b.of(Path::new("dir/a.md"));
        assert_eq!(h.undo(&mut t), Some(1));
        assert_eq!(t, "a");
        assert_eq!(h.redo(&mut t), Some(1));
        assert_eq!(h.redo(&mut t), Some(2));
        assert_eq!(t, "abc");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022
    #[test]
    fn the_hash_of_a_path_is_the_same_on_every_system() {
        assert_eq!(disk::fnv1a(""), 0xcbf2_9ce4_8422_2325, "the FNV-1a offset basis");
        assert_eq!(disk::fnv1a("a"), 0xaf63_dc4c_8601_ec8c, "a known FNV-1a value");
    }

    // spec: EDIT-022
    #[test]
    fn a_note_changed_while_shale_was_closed_gets_that_change_as_a_step() {
        let root = vault("outside");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["one", "one two"]);
        a.flush_all();
        let mut b = session(&root, T0 + 10);
        let mut t = "ONE two".to_string(); // edited in another program
        let h = b.of(Path::new("a.md"));
        assert_eq!(h.undo(&mut t), Some(0));
        assert_eq!(t, "one two", "the outside change is a step of its own");
        h.undo(&mut t);
        assert_eq!(t, "one");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022
    #[test]
    fn steps_older_than_30_days_by_default_are_dropped_when_read_and_when_written() {
        let root = vault("retention-steps");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        a.tick(T0 + 20 * DAY);
        typed(&mut a, "a.md", &["abc"]);
        a.flush_all();
        // read 31 days after the first step: only the later one is left
        let mut b = session(&root, T0 + 31 * DAY);
        let mut t = "abc".to_string();
        let h = b.of(Path::new("a.md"));
        assert_eq!(h.undo(&mut t), Some(2));
        assert_eq!(h.undo(&mut t), None, "the step of day 0 is gone");
        // written 31 days after: the file holds no old step either
        let mut c = session(&root, T0 + 25 * DAY);
        typed(&mut c, "a.md", &["abcd"]);
        c.tick(T0 + 51 * DAY);
        c.flush_all();
        let mut d = session(&root, T0 + 51 * DAY);
        let mut t = "abcd".to_string();
        let h = d.of(Path::new("a.md"));
        assert_eq!(h.undo(&mut t), Some(3), "the step made on day 25 is 26 days old");
        assert_eq!(h.undo(&mut t), None, "the step of day 20 is 31 days old");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022, USE-017
    #[test]
    fn opening_a_vault_deletes_no_file_and_forever_keeps_every_step() {
        let root = vault("retention-forever");
        let mut a = session(&root, T0);
        typed(&mut a, "old.md", &["a", "ab"]);
        a.flush_all();
        // a year later nothing is purged at the open: the cleanup of the caches does that
        let mut b = session(&root, T0 + 365 * DAY);
        assert_eq!(files_in(&root).len(), 1);
        // with 30 days the old steps are dropped on read, with "forever" they stay
        let mut t = "ab".to_string();
        assert_eq!(b.of(Path::new("old.md")).undo(&mut t), None);
        let mut c = Histories::default();
        c.set_keep_days(None);
        c.open_vault(&root, T0 + 365 * DAY);
        assert_eq!(c.of(Path::new("old.md")).undo(&mut t), Some(1), "forever: nothing is dropped");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022, USE-020
    #[test]
    fn the_loaded_histories_follow_a_changed_keep_for_setting() {
        let root = vault("retention-setting");
        let mut a = Histories::default();
        a.set_keep_days(None);
        a.open_vault(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        a.tick(T0 + 20 * DAY);
        typed(&mut a, "a.md", &["abc"]);
        a.tick(T0 + 40 * DAY);
        assert_eq!(a.of(Path::new("a.md")).undo.len(), 2, "forever keeps the step of day 0");
        a.set_keep_days(Some(30));
        assert_eq!(a.of(Path::new("a.md")).undo.len(), 1, "30 days: the step of day 0 is dropped from memory at once");
        assert!(a.has_pending(), "and the file is written without it");
        a.flush_all();
        a.set_keep_days(Some(10));
        assert!(a.of(Path::new("a.md")).undo.is_empty());
        a.flush_all();
        assert!(files_in(&root).is_empty(), "a history without steps has no file");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: USE-017
    #[test]
    fn the_cleanup_helpers_empty_and_rewrite_the_loaded_histories() {
        let root = vault("loaded");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        a.flush_all();
        a.clear_loaded();
        assert!(!a.has_pending());
        let mut t = "ab".to_string();
        assert_eq!(a.of(Path::new("a.md")).undo(&mut t), None, "nothing to undo after a clear");
        typed(&mut a, "a.md", &["abc"]);
        assert_eq!(a.of(Path::new("a.md")).undo.len(), 1, "the text at the last checkpoint stays: the next change is a step");
        a.flush_all();
        let _ = std::fs::remove_file(super::disk::file_of(&root, Path::new("a.md")));
        a.rewrite_loaded();
        assert!(a.has_pending(), "a step that a running job deleted is written again");
        a.flush_all();
        assert_eq!(files_in(&root).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022
    #[test]
    fn a_renamed_note_takes_its_file_along_loaded_or_not() {
        let root = vault("rename");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        typed(&mut a, "d/n.md", &["x", "xy"]);
        a.flush_all();
        // a.md is loaded in the session that renames it, d/n.md is not (its folder moves)
        let mut b = session(&root, T0 + 5);
        b.of(Path::new("a.md"));
        b.moved(|p| (p == Path::new("a.md")).then(|| PathBuf::from("b.md")));
        b.moved(|p| p.strip_prefix("d").ok().map(|r| Path::new("e").join(r)));
        b.flush_all();
        assert_eq!(files_in(&root).len(), 2, "{:?}", files_in(&root));
        assert!(!super::disk::file_of(&root, Path::new("a.md")).exists());
        assert!(!super::disk::file_of(&root, Path::new("d/n.md")).exists());
        let mut c = session(&root, T0 + 10);
        let mut t = "ab".to_string();
        assert_eq!(c.of(Path::new("b.md")).undo(&mut t), Some(1));
        assert_eq!(t, "a");
        let mut t = "xy".to_string();
        assert_eq!(c.of(Path::new("e/n.md")).undo(&mut t), Some(1));
        assert_eq!(c.of(Path::new("a.md")).undo.len(), 0);
        let text = std::fs::read_to_string(super::disk::file_of(&root, Path::new("e/n.md"))).unwrap();
        assert!(text.contains("\"path\": \"e/n.md\""), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022
    #[test]
    fn a_broken_file_is_set_aside_and_a_newer_one_is_never_overwritten() {
        let root = vault("broken");
        let file = super::disk::file_of(&root, Path::new("a.md"));
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "{ not json").unwrap();
        let mut a = session(&root, T0);
        assert!(a.of(Path::new("a.md")).is_empty(), "an empty history");
        let problems = a.take_problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(files_in(&root).iter().any(|n| n.contains(".json.broken-")), "{:?}", files_in(&root));
        typed(&mut a, "a.md", &["a", "ab"]);
        a.flush_all();
        assert!(file.exists(), "the note starts over with a new file");
        // a file of a newer version is used but stays as it is
        let newer = br#"{"version": 99, "path": "n.md", "undo": [{"at": 0, "removed": "", "inserted": "x", "time": 1800000000}], "redo": [], "known": "x", "future": 1}"#;
        let nf = super::disk::file_of(&root, Path::new("n.md"));
        std::fs::write(&nf, newer).unwrap();
        let mut b = session(&root, T0);
        let mut t = "x".to_string();
        assert_eq!(b.of(Path::new("n.md")).undo(&mut t), Some(0), "used as far as understood");
        typed(&mut b, "n.md", &["xy"]);
        b.flush_all();
        assert_eq!(std::fs::read(&nf).unwrap(), newer);
        assert_eq!(b.take_problems().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022
    #[test]
    fn a_change_is_written_after_the_delay_and_when_flushed() {
        let root = vault("debounce");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        assert!(a.has_pending());
        a.tick(T0 + DEBOUNCE - 1);
        assert!(files_in(&root).is_empty(), "not yet");
        a.tick(T0 + DEBOUNCE);
        assert_eq!(files_in(&root).len(), 1, "written after the delay");
        assert!(!a.has_pending());
        // a later change, then the note is closed or the app quits: written at once
        typed(&mut a, "a.md", &["abc"]);
        a.flush_all();
        let mut b = session(&root, T0 + 100);
        assert_eq!(b.of(Path::new("a.md")).undo.len(), 2);
        // looking at a note without changing it writes nothing
        let mut c = session(&root, T0 + 200);
        c.of(Path::new("other.md")).checkpoint("text");
        assert!(!c.has_pending());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: EDIT-022, USE-023
    #[test]
    fn a_running_cleanup_never_makes_the_window_wait_for_a_write() {
        let root = vault("busy");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        let job = super::disk::gate(&root);
        a.tick(T0 + DEBOUNCE);
        assert!(files_in(&root).is_empty(), "the job holds the folder: nothing written, no waiting");
        assert!(a.has_pending(), "still to be written");
        // another vault's folder is not held
        let other = vault("busy-other");
        let mut b = session(&other, T0);
        typed(&mut b, "b.md", &["b", "bc"]);
        b.tick(T0 + DEBOUNCE);
        assert_eq!(files_in(&other).len(), 1);
        drop(job);
        a.tick(T0 + DEBOUNCE + 1);
        assert_eq!(files_in(&root).len(), 1, "written once the job is done");
        assert!(!a.has_pending());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&other);
    }

    // spec: EDIT-022
    #[test]
    fn a_failed_write_keeps_the_history_in_memory_and_says_so_once() {
        let root = vault("failed");
        let mut a = session(&root, T0);
        typed(&mut a, "a.md", &["a", "ab"]);
        // the folder cannot be made: a file is in its place
        std::fs::create_dir_all(root.join(".shale")).unwrap();
        std::fs::write(root.join(".shale/undo"), "in the way").unwrap();
        a.tick(T0 + DEBOUNCE);
        a.tick(T0 + 2 * DEBOUNCE);
        a.flush_all();
        let problems = a.take_problems();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(a.has_pending(), "still to be written");
        let mut t = "ab".to_string();
        assert_eq!(a.of(Path::new("a.md")).undo(&mut t), Some(1), "the history is still there");
        let _ = std::fs::remove_dir_all(&root);
    }
}
