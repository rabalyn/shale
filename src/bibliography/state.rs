//! The bibliographies the window has read (EDIT-123, JOB-039): one slot per `.bib` file of the
//! vault that a note or the vault's default asks for. A file is read in a background job of the
//! busy registry, and read again when its modified time (the vault's index has it, NET-013)
//! differs from the one of the last read.

use super::reader::{self, Entry, FileIndex};
use super::resolve::Wanted;
use crate::busy::{Busy, Job, Key, LISTED_AFTER};
use crate::vault::Vault;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::sync::Arc;
use std::time::SystemTime;

/// The task of reading one `.bib` file ("Reading the bibliography “refs.bib”…").
pub const BIB_READ: Key = Key::new("bibliography-read");

/// What a read of a file made: its index, or the sentence that says why there is none.
type Read = Result<Arc<FileIndex>, String>;

struct Loaded {
    /// the modified time of the file that this read was made for
    modified: Option<SystemTime>,
    read: Read,
}

struct Reading {
    job: Job<Read>,
    modified: Option<SystemTime>,
}

#[derive(Default)]
struct Slot {
    loaded: Option<Loaded>,
    reading: Option<Reading>,
}

/// The bibliographies of the open vault.
#[derive(Default)]
pub struct Bibliographies {
    root: Option<PathBuf>,
    slots: BTreeMap<PathBuf, Slot>,
}

/// The bibliography in effect for a note: the indexes of the files that could be read, and what
/// to tell about the ones that could not.
#[derive(Default)]
pub struct Effect {
    pub files: Vec<(PathBuf, Arc<FileIndex>)>,
    /// a file is still being read
    pub loading: bool,
    /// a name that is no file, a file that cannot be read: one sentence each, "no bibliography in effect" for it
    pub messages: Vec<String>,
}

impl Effect {
    /// Whether a bibliography is in effect: at least one file was read.
    pub fn in_effect(&self) -> bool {
        !self.files.is_empty()
    }

    /// The entry of a key that can be cited; the first file that has it wins.
    pub fn entry(&self, key: &str) -> Option<&Entry> {
        self.files.iter().find_map(|(_, f)| f.get(key)).filter(|e| e.problem.is_none())
    }

    pub fn has_key(&self, key: &str) -> bool {
        self.entry(key).is_some()
    }

    /// The citable entries of every file, in the order of the files and keys; a key twice is listed once.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        let mut seen = std::collections::HashSet::new();
        self.files.iter().flat_map(|(_, f)| f.entries.iter()).filter(move |e| e.problem.is_none() && seen.insert(e.key.as_str()))
    }

    /// How many entries were skipped as bad, over all files.
    pub fn skipped(&self) -> usize {
        self.files.iter().map(|(_, f)| f.skipped).sum()
    }
}

impl Bibliographies {
    /// Whether a file is being read.
    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.slots.values().any(|s| s.reading.is_some())
    }

    /// Forget everything (another vault opens).
    pub fn reset(&mut self) {
        *self = Bibliographies::default();
    }

    /// One step: take in the finished reads, and start a read for each wanted file that was never
    /// read or whose modified time changed since. `notify` repaints the window when a job ends.
    pub fn tick(&mut self, vault: &Vault, busy: &Busy, wanted: &[&Path], notify: &(impl Fn() + Clone + Send + 'static)) {
        if self.root.as_deref() != Some(vault.root.as_path()) {
            self.reset();
            self.root = Some(vault.root.clone());
        }
        for slot in self.slots.values_mut() {
            let Some(r) = &slot.reading else { continue };
            let read = match r.job.try_recv() {
                Ok(Ok(read)) => read,
                Ok(Err(panic)) => Err(format!("could not be read: {panic}")),
                Err(TryRecvError::Empty) => continue,
                Err(TryRecvError::Disconnected) => Err("could not be read".into()),
            };
            slot.loaded = Some(Loaded { modified: r.modified, read });
            slot.reading = None;
        }
        for rel in wanted {
            let modified = vault.file(rel).and_then(|f| f.modified);
            let slot = self.slots.entry(rel.to_path_buf()).or_default();
            if slot.reading.is_some() || slot.loaded.as_ref().is_some_and(|l| l.modified == modified) {
                continue;
            }
            let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let Some(task) = busy.begin_listed_after(BIB_READ.with(&rel.to_string_lossy()), format!("Reading the bibliography “{name}”…"), LISTED_AFTER) else { continue };
            let abs = vault.abs(rel);
            let notify = notify.clone();
            slot.reading = Some(Reading { job: task.run(move || reader::read_file(&abs, modified), notify), modified });
        }
    }

    /// The bibliography in effect for the files that [`super::resolve::wanted`] found.
    pub fn effect(&self, wanted: &Wanted) -> Effect {
        let mut effect = Effect { messages: wanted.problems.clone(), ..Effect::default() };
        for rel in &wanted.files {
            let slot = self.slots.get(rel);
            match slot.and_then(|s| s.loaded.as_ref()).map(|l| &l.read) {
                Some(Ok(index)) => effect.files.push((rel.clone(), index.clone())),
                Some(Err(why)) => effect.messages.push(format!("The bibliography “{}” {why}. No bibliography is in effect from it.", rel.display())),
                None => effect.loading = true,
            }
        }
        effect
    }
}
