//! What a search reads of the vault's notes, taken once per query (QUERY-015): the files in the
//! vault's order and, for each note, the `Arc` of its index. A snapshot copies pointers to the
//! texts, not the texts, so it is cheap to make on the window's thread and can go to the search
//! worker. An edit of a note replaces the vault's `Arc` of that note (`Arc::make_mut`: a copy
//! while a snapshot still holds the old one), so the worker goes on reading the text as it was
//! when the query was asked.

use crate::vault::{FileEntry, NoteIndex, Vault};
use std::sync::{Arc, Mutex, PoisonError};

/// One file of the snapshot: its entry and, for a note, the index that holds its text.
pub struct SnapNote {
    pub file: FileEntry,
    pub note: Option<Arc<NoteIndex>>,
}

/// The notes of a vault at one moment.
#[derive(Default)]
pub struct Snapshot {
    /// the files in the order of the vault's list, without the conflict copies that have an original
    pub notes: Vec<SnapNote>,
}

impl Snapshot {
    pub fn of(vault: &Vault) -> Snapshot {
        let notes = vault.files.iter().filter(|f| !vault.is_shadow_copy(&f.rel)).map(|f| SnapNote { file: f.clone(), note: vault.notes.get(&f.rel).cloned() }).collect();
        Snapshot { notes }
    }
}

/// The snapshot of a vault, kept for the state of the vault it was made at ([`Vault::search_snapshot`]).
/// The vault empties it before every change of its notes or files (so that an edit does not copy a
/// note the cache still holds); the stamp is a second check for a change that did not.
#[derive(Default)]
pub struct SnapCache(Mutex<Option<Kept>>);

/// A snapshot and the state of the vault (scans merged, generation) it was made at.
type Kept = ((u64, u64), Arc<Snapshot>);

impl SnapCache {
    pub fn get(&self, vault: &Vault) -> Arc<Snapshot> {
        let stamp = (vault.tree_state().1, vault.generation);
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        match &*held {
            Some((at, snap)) if *at == stamp => snap.clone(),
            _ => {
                let snap = Arc::new(Snapshot::of(vault));
                *held = Some((stamp, snap.clone()));
                snap
            }
        }
    }

    pub fn clear(&mut self) {
        *self.0.get_mut().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn vault(tag: &str) -> (Vault, std::path::PathBuf) {
        let d = std::env::temp_dir().join(format!("shale-snapshot-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("A.md"), "alpha\n").unwrap();
        std::fs::write(d.join("B.md"), "beta\n").unwrap();
        (Vault::open(&d).unwrap(), d)
    }

    fn note<'a>(s: &'a Snapshot, rel: &str) -> &'a Arc<NoteIndex> {
        s.notes.iter().find(|n| n.file.rel == Path::new(rel)).and_then(|n| n.note.as_ref()).unwrap()
    }

    // spec: QUERY-015
    #[test]
    fn a_snapshot_shares_the_notes_and_an_edit_does_not_reach_it() {
        let (mut v, d) = vault("share");
        let snap = Snapshot::of(&v);
        assert!(Arc::ptr_eq(note(&snap, "A.md"), &v.notes[Path::new("A.md")]), "pointers are copied, not texts");
        v.set_note_text(Path::new("A.md"), "alpha edited\n");
        assert_eq!(note(&snap, "A.md").text, "alpha\n", "the snapshot keeps the text of the query");
        assert_eq!(v.notes[Path::new("A.md")].text, "alpha edited\n");
        assert!(Arc::ptr_eq(note(&snap, "B.md"), &v.notes[Path::new("B.md")]), "a note that was not edited is still shared");
        // a snapshot taken after the edit sees it, and a note made later is not in the old one
        v.set_note_text(Path::new("C.md"), "gamma\n");
        assert_eq!(note(&Snapshot::of(&v), "A.md").text, "alpha edited\n");
        assert!(v.notes.contains_key(Path::new("C.md")) && snap.notes.iter().all(|n| n.file.rel != Path::new("C.md")));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-015
    #[test]
    fn a_snapshot_leaves_out_the_conflict_copies_that_have_an_original() {
        let d = std::env::temp_dir().join(format!("shale-snapshot-shadow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Plan.md"), "zebrafish original\n").unwrap();
        std::fs::write(d.join("Plan.sync-conflict-20261121-142440-7JJWLPP.md"), "zebrafish copy\n").unwrap();
        let v = Vault::open(&d).unwrap();
        let rels: Vec<String> = Snapshot::of(&v).notes.iter().map(|n| n.file.rel.to_string_lossy().into_owned()).collect();
        assert_eq!(rels, ["Plan.md"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-015
    #[test]
    fn a_search_of_an_unchanged_vault_reuses_the_snapshot_and_every_change_makes_a_new_one() {
        let (mut v, d) = vault("cache");
        let first = v.search_snapshot();
        assert!(Arc::ptr_eq(&first, &v.search_snapshot()), "nothing changed: the same snapshot");
        v.set_note_text(Path::new("A.md"), "alpha edited\n");
        let second = v.search_snapshot();
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(note(&second, "A.md").text, "alpha edited\n");
        assert_eq!(note(&first, "A.md").text, "alpha\n", "the old snapshot keeps its text");
        // the cache does not hold the note, so the next edit of it is not a copy
        let held = Arc::strong_count(&v.notes[Path::new("A.md")]);
        assert_eq!(held, 2, "the vault and the snapshot the test holds");
        drop(second);
        v.set_note_text(Path::new("A.md"), "alpha again\n");
        assert_eq!(note(&v.search_snapshot(), "A.md").text, "alpha again\n");
        // a rescan that finds a new file
        std::fs::write(d.join("C.md"), "gamma\n").unwrap();
        v.rescan();
        assert!(v.search_snapshot().notes.iter().any(|n| n.file.rel == Path::new("C.md")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_change_the_vault_did_not_clear_is_caught_by_the_stamp() {
        let (mut v, d) = vault("stamp");
        let first = v.search_snapshot();
        v.generation += 1;
        assert!(!Arc::ptr_eq(&first, &v.search_snapshot()));
        let _ = std::fs::remove_dir_all(&d);
    }
}
