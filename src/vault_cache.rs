//! What the window works out from the whole vault and keeps while the vault does not change
//! (PERF-001): the backlinks and unlinked mentions of the open note, the task list and the number
//! of open tasks. Each is worked out when it is first asked for and again only when the vault's
//! `generation` (which every change of the index bumps), the vault itself or the note is another,
//! never for the next frame of the same state.

use crate::vault::{SearchHit, Vault};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The notes that mention a note: (source, [(line number, line text)]).
pub type Mentions = Vec<(PathBuf, Vec<(usize, String)>)>;
/// Every task of the vault, sorted by note and line.
pub type Tasks = Vec<(PathBuf, crate::markdown::Task)>;

/// One vault in one state: which vault, which generation of it.
type State = (u64, u64);

fn state(v: &Vault) -> State {
    (v.uid, v.generation)
}

#[derive(Default)]
pub struct VaultCache {
    backlinks: Option<(State, PathBuf, Arc<Mentions>)>,
    unlinked: Option<(State, PathBuf, Arc<Mentions>)>,
    tasks: Option<(State, Arc<Tasks>, usize)>,
    search: Option<(State, String, Arc<Vec<SearchHit>>)>,
}

impl VaultCache {
    /// The notes and drawings that link to `rel` ([`Vault::backlinks`]).
    pub fn backlinks(&mut self, v: &Vault, rel: &Path) -> Arc<Mentions> {
        match &self.backlinks {
            Some((s, r, m)) if *s == state(v) && r == rel => m.clone(),
            _ => {
                let m = Arc::new(v.backlinks(rel));
                self.backlinks = Some((state(v), rel.to_path_buf(), m.clone()));
                m
            }
        }
    }

    /// The notes that mention the name of `rel` without a link ([`Vault::unlinked_mentions`]).
    pub fn unlinked(&mut self, v: &Vault, rel: &Path) -> Arc<Mentions> {
        match &self.unlinked {
            Some((s, r, m)) if *s == state(v) && r == rel => m.clone(),
            _ => {
                let m = Arc::new(v.unlinked_mentions(rel));
                self.unlinked = Some((state(v), rel.to_path_buf(), m.clone()));
                m
            }
        }
    }

    /// The sidebar search for `query` ([`Vault::search`]): read through the vault again only for
    /// another query or another state of the vault, not for the next frame of the same.
    pub fn search(&mut self, v: &Vault, query: &str) -> Arc<Vec<SearchHit>> {
        match &self.search {
            Some((s, q, hits)) if *s == state(v) && q == query => hits.clone(),
            _ => {
                let hits = Arc::new(v.search(query));
                self.search = Some((state(v), query.to_string(), hits.clone()));
                hits
            }
        }
    }

    /// Every task of the vault ([`Vault::all_tasks`]).
    pub fn tasks(&mut self, v: &Vault) -> Arc<Tasks> {
        self.tasks_and_open(v).0
    }

    /// How many tasks of the vault are open.
    pub fn open_tasks(&mut self, v: &Vault) -> usize {
        self.tasks_and_open(v).1
    }

    fn tasks_and_open(&mut self, v: &Vault) -> (Arc<Tasks>, usize) {
        match &self.tasks {
            Some((s, t, open)) if *s == state(v) => (t.clone(), *open),
            _ => {
                let t = Arc::new(v.all_tasks());
                let open = t.iter().filter(|(_, t)| !t.done).count();
                self.tasks = Some((state(v), t.clone(), open));
                (t, open)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
        let dir = std::env::temp_dir().join(format!("shale-vcache-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (p, t) in files {
            std::fs::write(dir.join(p), t).unwrap();
        }
        (Vault::open(&dir).unwrap(), dir)
    }

    // spec: PERF-001, EDIT-107
    #[test]
    fn perf_001_the_lists_are_made_once_per_vault_state_and_follow_every_change() {
        let (mut v, dir) = vault("once", &[("Alpha.md", "# Alpha\n- [ ] one\n"), ("B.md", "# B\nsee [[Alpha]]\n"), ("C.md", "I like a thing\n")]);
        let mut c = VaultCache::default();
        let a = Path::new("Alpha.md");
        let (b1, t1, u1) = (c.backlinks(&v, a), c.tasks(&v), c.unlinked(&v, a));
        assert!(Arc::ptr_eq(&b1, &c.backlinks(&v, a)) && Arc::ptr_eq(&t1, &c.tasks(&v)) && Arc::ptr_eq(&u1, &c.unlinked(&v, a)), "the next frame gets the same lists");
        assert_eq!((b1.len(), c.open_tasks(&v), u1.len()), (1, 1, 0));
        // another note is another list
        assert!(!Arc::ptr_eq(&b1, &c.backlinks(&v, Path::new("B.md"))));
        let b1 = c.backlinks(&v, a);
        // an edit of the vault is seen by all three
        std::fs::write(dir.join("C.md"), "# C\n[[Alpha]]\n- [ ] two\n- [x] done\nand a mention of Alpha in text\n").unwrap();
        v.rescan();
        let (b2, t2) = (c.backlinks(&v, a), c.tasks(&v));
        assert!(!Arc::ptr_eq(&b1, &b2) && !Arc::ptr_eq(&t1, &t2));
        assert_eq!((b2.len(), c.open_tasks(&v), t2.len()), (2, 2, 3));
        assert_eq!(c.unlinked(&v, a).iter().map(|(p, _)| p.clone()).collect::<Vec<_>>(), [PathBuf::from("C.md")]);
        // a change on disk, found by the rescan
        std::fs::write(dir.join("B.md"), "# B\nnothing\n- [ ] three\n").unwrap();
        v.rescan();
        assert_eq!((c.backlinks(&v, a).len(), c.open_tasks(&v)), (1, 3));
        // a rename that rewrites the links, and a deletion
        v.rename(a, Path::new("Z.md"), true).unwrap();
        assert_eq!(c.backlinks(&v, Path::new("Z.md")).len(), 1);
        assert_eq!(c.backlinks(&v, a).len(), 0);
        v.trash(Path::new("C.md")).unwrap();
        assert_eq!((c.backlinks(&v, Path::new("Z.md")).len(), c.open_tasks(&v)), (0, 2));
        // the search follows the query and the vault
        let found = |c: &mut VaultCache, v: &Vault, q: &str| c.search(v, q).iter().map(|(p, _)| p.to_string_lossy().into_owned()).collect::<Vec<_>>();
        assert!(found(&mut c, &v, "mention").is_empty());
        let s1 = c.search(&v, "three");
        assert!(Arc::ptr_eq(&s1, &c.search(&v, "three")), "the next frame gets the same result");
        assert_eq!(found(&mut c, &v, "three"), ["B.md"]);
        assert!(!Arc::ptr_eq(&s1, &c.search(&v, "THREE")), "another query");
        v.set_note_text(Path::new("B.md"), "# B\nthree and four\n");
        assert_eq!(c.search(&v, "four").len(), 1);
        assert_eq!(c.search(&v, "three").len(), 1);
        v.set_note_text(Path::new("B.md"), "# B\nnothing\n");
        assert_eq!(c.search(&v, "three").len(), 0, "an edit is seen");
        // another vault with the same generation is not the same state
        let (v2, dir2) = vault("other", &[("Alpha.md", "# Alpha\n- [ ] x\n- [ ] y\n- [ ] z\n")]);
        let mut c2 = VaultCache::default();
        assert_eq!(c2.open_tasks(&v), 1);
        v.generation = v2.generation;
        assert_eq!(c2.open_tasks(&v2), 3, "the same generation of another vault");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&dir2);
    }
}
