//! Tests of thinning a local-only file history (SYNC-067…SYNC-071), against real `git` in
//! temporary folders. Every commit gets its date through `GIT_AUTHOR_DATE` and
//! `GIT_COMMITTER_DATE`, so nothing sleeps and nothing depends on the clock.

use super::*;
use crate::gitsync::history::{content_at, deleted, deleted_content, ensure, locate, snapshot, versions, Request};
use crate::gitsync::tests::{write, Env};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const WAIT: Duration = Duration::from_secs(5);
/// A midnight (UTC), so that the buckets of the tests are easy to count.
const NOW: i64 = 20_000 * DAY;

fn c(id: &str, age: i64, reason: Option<Reason>, deletes: bool) -> Commit {
    Commit { id: id.into(), time: NOW - age, reason, deletes }
}

/// The ids that stay.
fn kept(commits: &[Commit]) -> Vec<String> {
    keep(commits, NOW).into_iter().zip(commits).filter(|(k, _)| *k).map(|(_, c)| c.id.clone()).collect()
}

// spec: SYNC-067
#[test]
fn keep_takes_all_of_the_last_day_and_the_boundary_of_24_hours_is_exact() {
    let commits = [c("newest", 0, None, false), c("a", 60, None, false), c("b", DAY - 1, None, false), c("c", DAY, None, false), c("root", 5 * DAY, None, false)];
    // 23 h 59 min 59 s is all of the day; exactly 24 h is hourly, and "c" is the newest of its hour
    assert_eq!(kept(&commits), ["newest", "a", "b", "c", "root"]);
    // two in one hour past 24 h: the newer one stays, the older goes
    let commits = [c("newest", 0, None, false), c("x", DAY + 100, None, false), c("y", DAY + 200, None, false), c("root", 5 * DAY, None, false)];
    assert_eq!(kept(&commits), ["newest", "x", "root"]);
}

// spec: SYNC-067
#[test]
fn keep_takes_the_newest_of_each_hour_up_to_30_days_and_the_newest_of_each_day_after() {
    let two = 2 * DAY;
    let commits = [
        c("newest", 0, None, false),
        c("h1-new", two + 60, None, false),
        c("h1-old", two + 120, None, false),
        c("h2", two + HOUR + 60, None, false), // the hour before
        c("h2-old", two + HOUR + 120, None, false),
        c("d0", MONTH, None, false), // exactly 30 days: the daily class, alone in its day
        c("d1-new", MONTH + 60, None, false),
        c("d1-old", MONTH + 3 * HOUR, None, false),
        c("d2-new", MONTH + DAY + 60, None, false),
        c("d2-old", MONTH + DAY + 120, None, false),
        c("root", 90 * DAY, None, false),
    ];
    assert_eq!(kept(&commits), ["newest", "h1-new", "h2", "d0", "d1-new", "d2-new", "root"]);
    // just under 30 days is still hourly: z and y are in different hours, x is in the hour of y
    let commits = [c("newest", 0, None, false), c("z", MONTH - HOUR - 2, None, false), c("y", MONTH - 2, None, false), c("x", MONTH - 1, None, false), c("root", 90 * DAY, None, false)];
    assert_eq!(kept(&commits), ["newest", "z", "y", "root"]);
}

// spec: SYNC-067
#[test]
fn keep_always_takes_the_newest_and_the_root_and_copes_with_nothing_and_one() {
    assert!(keep(&[], NOW).is_empty());
    assert_eq!(kept(&[c("only", 99 * DAY, None, false)]), ["only"]);
    // all in one old day: the newest (also the newest of the day) and the root stay
    let commits = [c("n", 50 * DAY + 10, None, false), c("m", 50 * DAY + 20, None, false), c("r", 50 * DAY + 30, None, false)];
    assert_eq!(kept(&commits), ["n", "r"]);
    // a commit from the future (the clock was wrong) counts as young
    assert_eq!(kept(&[c("future", -5 * DAY, None, false), c("root", 60 * DAY, None, false)]), ["future", "root"]);
}

// spec: SYNC-068
#[test]
fn keep_never_drops_a_restore_a_delete_a_rename_a_deleting_commit_or_the_commit_before_a_deletion() {
    let old = |day: i64, k: i64| (30 + day) * DAY + k * 60; // all of one day for one `day`
    let commits = [
        c("newest", 0, None, false),
        // a deletion by Shale: its parent stays
        c("d-after", old(1, 1), Some(Reason::Save), false),
        c("d-del", old(1, 2), Some(Reason::Delete), true),
        c("d-before", old(1, 3), Some(Reason::Save), false),
        c("d-older", old(1, 4), Some(Reason::Save), false),
        // a deletion by the sync (no Delete reason, but the commit deletes a path)
        c("s-after", old(2, 1), Some(Reason::Sync), false),
        c("s-del", old(2, 2), Some(Reason::Sync), true),
        c("s-before", old(2, 3), Some(Reason::Save), false),
        c("s-older", old(2, 4), Some(Reason::Save), false),
        // a restore and a rename: themselves, not their parents
        c("r-after", old(3, 1), Some(Reason::Save), false),
        c("r-restore", old(3, 2), Some(Reason::Restore), false),
        c("r-before", old(3, 3), Some(Reason::Save), false),
        c("n-after", old(4, 1), Some(Reason::Save), false),
        c("n-rename", old(4, 2), Some(Reason::Rename), false),
        c("n-before", old(4, 3), Some(Reason::Save), false),
        c("root", 90 * DAY, Some(Reason::Initial), false),
    ];
    assert_eq!(kept(&commits), ["newest", "d-after", "d-del", "d-before", "s-after", "s-del", "s-before", "r-after", "r-restore", "n-after", "n-rename", "root"]);
}

/// A vault without `.git` and its local history, with commits at the dates the test names.
struct H {
    env: Env,
    vault: PathBuf,
    root: PathBuf,
    repo: Repo,
}

fn setup(tag: &str) -> H {
    let env = Env::new(tag);
    let vault = env.folder("vault");
    let root = env.folder("data/history");
    let repo = locate(&env.git, &vault, &root).unwrap();
    H { env, vault, root, repo }
}

impl H {
    /// The repository as it commits at `secs`.
    fn at(&self, secs: i64) -> Repo {
        let mut r = self.repo.clone();
        let date = format!("{secs} +0000");
        for k in ["GIT_AUTHOR_DATE", "GIT_COMMITTER_DATE"] {
            r.git.env.push((k.into(), date.clone().into()));
        }
        r
    }

    fn snap(&self, secs: i64, reason: Reason, paths: &[&str]) {
        let req = Request::new(reason, paths.iter().map(PathBuf::from).collect());
        snapshot(&self.at(secs), &self.env.opts("laptop"), &req, &TreeLock::default(), WAIT).unwrap();
    }

    /// Write a note and snapshot it at `secs`.
    fn save(&self, secs: i64, rel: &str, text: &str) {
        write(&self.vault, rel, text);
        self.snap(secs, Reason::Save, &[rel]);
    }

    fn git(&self, args: &[&str]) -> String {
        self.repo.run_for_test(args).trim().to_string()
    }

    fn count(&self) -> usize {
        self.git(&["rev-list", "--count", "refs/heads/main"]).parse().unwrap()
    }

    fn tip(&self) -> String {
        self.git(&["rev-parse", "refs/heads/main"])
    }

    fn gd(&self) -> PathBuf {
        self.repo.git_dir.clone().unwrap()
    }

    fn thin(&self, now: i64) -> Result<Outcome, String> {
        thin(&self.repo, &TreeLock::default(), WAIT, now as u64)
    }

    /// The committer times of the history, newest first.
    fn times(&self) -> Vec<i64> {
        self.git(&["log", "--format=%ct"]).lines().map(|l| l.parse().unwrap()).collect()
    }

    /// Every file of the vault with its bytes.
    fn files(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, base: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in fs::read_dir(dir).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, base, out);
                } else {
                    out.insert(p.strip_prefix(base).unwrap().to_string_lossy().into_owned(), fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(&self.vault, &self.vault, &mut out);
        out
    }

    /// The line of Data usage for this history (USE-025): its size in bytes.
    fn usage_bytes(&self) -> u64 {
        let mut p = crate::data_usage::Places::in_folder(&self.env.root.join("usage"));
        p.history = self.root.clone();
        p.vault = Some(self.vault.clone());
        let lines = crate::data_usage::scan::saved(&p).outside;
        lines.into_iter().find(|l| l.label == "File history of this vault").expect("the line of the history").bytes
    }

    /// Several commits a day for some days, all older than 30 days, of one note.
    fn many_old(&self, days: i64, per_day: i64) {
        for d in (0..days).rev() {
            for k in 0..per_day {
                self.save(NOW - (40 + d) * DAY + k * 600, "A.md", &format!("day {d} edit {k}\n"));
            }
        }
    }
}

/// The versions of a note as (time, subject, reason, text).
fn view(h: &H, rel: &str) -> Vec<(i64, String, Option<Reason>, Vec<u8>)> {
    versions(&h.repo, Path::new(rel), 1000).unwrap().into_iter().map(|v| (v.time, v.meta.subject.clone(), v.meta.reason, content_at(&h.repo, &v.commit, &v.path).unwrap())).collect()
}

// spec: SYNC-067, SYNC-068, SYNC-069, SYNC-071, USE-025
#[test]
fn thinning_200_commits_over_90_days_keeps_the_versions_the_deleted_note_the_files_and_shrinks_the_history() {
    let h = setup("thin-big");
    let t = |d: i64, hours: f64| NOW - d * DAY + (hours * 3600.0) as i64;
    write(&h.vault, "Gone.md", "gone text\n");
    write(&h.vault, "B.md", "b\n");
    // 89 to 60 days ago: four commits a day (the first is the start of the history), a deletion on day 60
    for d in (60..=89).rev() {
        for hour in [1.0, 7.0, 13.0, 19.0] {
            h.save(t(d, hour), "A.md", &format!("old {d} {hour}\n"));
            if d == 60 && hour == 13.0 {
                fs::remove_file(h.vault.join("Gone.md")).unwrap();
                h.snap(t(60, 15.0), Reason::Delete, &["Gone.md"]);
            }
        }
    }
    // 29 to 2 days ago: two commits in one hour a day
    for d in (2..=29).rev() {
        for hour in [5.0, 5.5] {
            h.save(t(d, hour), "A.md", &format!("mid {d} {hour}\n"));
        }
    }
    // yesterday: eleven commits; the last six hours: twelve commits
    for k in 0..11 {
        h.save(NOW - (20 - k) * HOUR, "A.md", &format!("yesterday {k}\n"));
    }
    for k in 0..12 {
        h.save(NOW - 6 * HOUR + k * 1800, "B.md", &format!("today {k}\n"));
    }
    assert_eq!(h.count(), 200);

    let versions_before = view(&h, "A.md");
    let deleted_before = deleted(&h.repo, 50).unwrap();
    assert_eq!(deleted_before.len(), 1);
    let files_before = h.files();
    let size_before = h.usage_bytes();

    // old: one a day (30 days, the first with the start of the history, the 60th day with the deletion and its parent);
    // mid: one an hour (28); last day and yesterday: all (23)
    let outcome = h.thin(NOW).unwrap();
    assert_eq!(outcome, Outcome::Thinned { kept: 84, dropped: 116 });
    assert_eq!(h.count(), 84);
    assert_eq!(last_thinned(&h.repo), Some(NOW as u64), "the time of the run is kept in the git directory");

    // the versions of a note: the kept ones, with the same time, message and text
    let kept_times: HashSet<i64> = h.times().into_iter().collect();
    let expected: Vec<_> = versions_before.into_iter().filter(|v| kept_times.contains(&v.0)).collect();
    let after = view(&h, "A.md");
    assert_eq!(after, expected);
    assert!(after.len() > 50 && after.len() < 84);
    assert_eq!(after.last().unwrap().2, Some(Reason::Initial), "the start of the history is still the oldest");

    // the note deleted 60 days ago is listed and its last text is found
    let deleted_after = deleted(&h.repo, 50).unwrap();
    assert_eq!(deleted_after.len(), 1);
    assert_eq!((&deleted_after[0].path, deleted_after[0].time, &deleted_after[0].meta), (&deleted_before[0].path, deleted_before[0].time, &deleted_before[0].meta));
    assert_eq!(deleted_content(&h.repo, &deleted_after[0]).unwrap(), b"gone text\n");

    assert_eq!(h.files(), files_before, "the vault's files are untouched");
    assert!(!h.vault.join(".git").exists());
    let size_after = h.usage_bytes();
    assert!(size_after < size_before, "the history is smaller: {size_before} -> {size_after}");
    // the old commits are really gone, not only unreferenced
    assert_eq!(h.git(&["count-objects", "-v"]).lines().find(|l| l.starts_with("garbage:")), Some("garbage: 0"));
    assert_eq!(h.git(&["status", "--porcelain", "--", "*.md"]), "", "the notes still equal the newest version");

    // a second run finds nothing more to drop, and the day is not over
    assert_eq!(h.thin(NOW + 60).unwrap(), Outcome::Unchanged);
    assert_eq!(run_if_due(&h.repo, &TreeLock::default(), WAIT, (NOW + 120) as u64).unwrap(), Outcome::NotDue);
}

// spec: SYNC-068
#[test]
fn a_restore_and_a_rename_stay_and_a_note_deleted_by_the_sync_stays_restorable() {
    let h = setup("thin-pinned");
    write(&h.vault, "Sync.md", "sync text\n");
    h.save(NOW - 60 * DAY, "A.md", "start\n");
    for k in 0..4 {
        h.save(NOW - 50 * DAY + k * 60, "A.md", &format!("edit {k}\n"));
    }
    // a restore of an older text, between edits of one old day
    write(&h.vault, "A.md", "restored\n");
    let req = Request { paths: vec![PathBuf::from("A.md")], reason: Reason::Restore, restored: Some("0000".into()) };
    snapshot(&h.at(NOW - 50 * DAY + 600), &h.env.opts("laptop"), &req, &TreeLock::default(), WAIT).unwrap();
    h.save(NOW - 50 * DAY + 700, "A.md", "after restore\n");
    // a rename
    fs::rename(h.vault.join("A.md"), h.vault.join("Renamed.md")).unwrap();
    h.snap(NOW - 45 * DAY, Reason::Rename, &["A.md", "Renamed.md"]);
    // the sync trashes a note: the commit deletes the path, with the reason `sync`
    h.save(NOW - 44 * DAY, "Sync.md", "sync text 2\n");
    h.save(NOW - 44 * DAY + 60, "Sync.md", "sync text 3\n");
    fs::remove_file(h.vault.join("Sync.md")).unwrap();
    h.snap(NOW - 44 * DAY + 120, Reason::Sync, &["Sync.md"]);
    h.save(NOW - 44 * DAY + 180, "Renamed.md", "later\n");
    h.save(NOW - 2 * DAY, "Renamed.md", "recent\n");

    let before = h.messages_reasons();
    assert!(matches!(h.thin(NOW).unwrap(), Outcome::Thinned { .. }));
    let after = h.messages_reasons();
    assert!(after.contains(&Some(Reason::Restore)) && after.contains(&Some(Reason::Rename)), "{after:?}");
    assert_eq!(after.iter().filter(|r| **r == Some(Reason::Restore)).count(), before.iter().filter(|r| **r == Some(Reason::Restore)).count());
    let gone = deleted(&h.repo, 10).unwrap();
    assert_eq!(gone.iter().map(|d| d.path.clone()).collect::<Vec<_>>(), [PathBuf::from("Sync.md")]);
    assert_eq!(deleted_content(&h.repo, &gone[0]).unwrap(), b"sync text 3\n", "the text just before the deletion, not an older one");
    // the versions of the renamed note go back past the rename
    let v = versions(&h.repo, Path::new("Renamed.md"), 100).unwrap();
    assert!(v.iter().any(|v| v.path == Path::new("A.md")), "{v:?}");
}

impl H {
    fn messages_reasons(&self) -> Vec<Option<Reason>> {
        self.git(&["log", "-z", "--format=%B"]).split('\0').filter(|m| !m.trim().is_empty()).map(|m| parse_message(m).reason).collect()
    }
}

// spec: SYNC-069
#[test]
fn the_new_history_has_the_same_trees_messages_authors_and_dates() {
    let h = setup("thin-same");
    h.many_old(3, 4);
    let fields = |h: &H| h.git(&["log", "--format=%T|%an|%ae|%aD|%cn|%ce|%cD|%B", "--reverse"]);
    let before = fields(&h);
    let old_tip = h.tip();
    assert!(matches!(h.thin(NOW).unwrap(), Outcome::Thinned { .. }));
    assert_ne!(h.tip(), old_tip);
    let after = fields(&h);
    // every record of the new history is a record of the old one, in the same order
    let keep_lines = |s: &str| s.split("\n\n").map(str::to_string).collect::<Vec<_>>();
    let (old, new) = (keep_lines(&before), keep_lines(&after));
    let mut at = 0;
    for rec in &new {
        at += old[at..].iter().position(|o| o == rec).unwrap_or_else(|| panic!("a commit changed: {rec}")) + 1;
    }
    assert_eq!(h.git(&["rev-list", "--merges", "--count", "main"]), "0");
    assert_eq!(h.git(&["symbolic-ref", "HEAD"]), "refs/heads/main");
    assert_eq!(h.git(&["fsck", "--no-dangling"]), "", "the repository is whole");
}

// spec: SYNC-069
#[test]
fn a_vault_with_its_own_git_is_never_thinned() {
    let env = Env::new("thin-own");
    let vault = env.folder("vault");
    env.git(&vault, &["init", "--quiet"]);
    let repo = locate(&env.git, &vault, &env.folder("data/history")).unwrap();
    assert!(repo.git_dir.is_none());
    for (i, secs) in [NOW - 90 * DAY, NOW - 90 * DAY + 60, NOW - 90 * DAY + 120].into_iter().enumerate() {
        write(&vault, "A.md", &format!("{i}\n"));
        let mut r = repo.clone();
        for k in ["GIT_AUTHOR_DATE", "GIT_COMMITTER_DATE"] {
            r.git.env.push((k.into(), format!("{secs} +0000").into()));
        }
        snapshot(&r, &env.opts("laptop"), &Request::new(Reason::Save, vec![PathBuf::from("A.md")]), &TreeLock::default(), WAIT).unwrap();
    }
    let tip = env.git(&vault, &["rev-parse", "HEAD"]);
    assert_eq!(env.git(&vault, &["rev-list", "--count", "HEAD"]), "3");
    let err = thin(&repo, &TreeLock::default(), WAIT, NOW as u64).unwrap_err();
    assert!(err.contains("never thinned"), "{err}");
    assert!(plan(&repo, NOW).is_err());
    assert_eq!(run_if_due(&repo, &TreeLock::default(), WAIT, NOW as u64).unwrap(), Outcome::Skipped);
    assert_eq!(env.git(&vault, &["rev-parse", "HEAD"]), tip);
    assert_eq!(env.git(&vault, &["rev-list", "--count", "HEAD"]), "3");
    assert!(!vault.join(".git").join(STAMP_FILE).exists());
}

// spec: SYNC-069
#[test]
fn a_branch_that_moved_while_the_new_history_was_built_leaves_the_old_history_whole() {
    let h = setup("thin-moved");
    h.many_old(3, 4);
    let p = plan(&h.repo, NOW).unwrap().expect("something to thin");
    assert_eq!(p.old_tip, h.tip());
    // a snapshot came in between the plan and the swap
    h.save(NOW - 10, "A.md", "new text\n");
    let moved = h.tip();
    let count = h.count();
    let err = apply(&h.repo, &p).unwrap_err();
    assert!(err.contains("update-ref"), "{err}");
    assert_eq!((h.tip(), h.count()), (moved, count), "the history is as the snapshot left it");
    assert_eq!(h.git(&["fsck", "--no-dangling"]), "");
}

// spec: SYNC-069
#[test]
fn a_refused_update_ref_or_a_failure_while_building_leaves_the_old_history_intact() {
    let h = setup("thin-fail");
    h.many_old(3, 4);
    let (tip, count, shown) = (h.tip(), h.count(), view(&h, "A.md"));
    let intact = |what: &str| {
        assert_eq!((h.tip(), h.count()), (tip.clone(), count), "{what}");
        assert_eq!(view(&h, "A.md"), shown, "{what}");
    };

    // update-ref refuses: the branch is locked
    let lock = h.gd().join("refs/heads/main.lock");
    fs::write(&lock, "").unwrap();
    let err = h.thin(NOW).unwrap_err();
    assert!(err.contains("update-ref"), "{err}");
    intact("a locked branch");
    fs::remove_file(&lock).unwrap();

    // a tree object is missing: building the new history fails before any ref is touched
    let plan_ok = plan(&h.repo, NOW).unwrap().unwrap();
    let tree = h.git(&["rev-parse", &format!("{}^{{tree}}", plan_ok.old_tip)]);
    let object = h.gd().join("objects").join(&tree[..2]).join(&tree[2..]);
    let saved = fs::read(&object).unwrap();
    fs::remove_file(&object).unwrap();
    assert!(h.thin(NOW).is_err());
    assert_eq!((h.tip(), h.count()), (tip.clone(), count));
    fs::write(&object, saved).unwrap();
    intact("a missing object");

    // and then it works
    assert!(matches!(h.thin(NOW).unwrap(), Outcome::Thinned { .. }));
}

// spec: SYNC-069, SYNC-070
#[test]
fn thinning_waits_for_the_tree_and_is_busy_when_the_files_are_in_use() {
    let h = setup("thin-busy");
    h.many_old(2, 4);
    let tree = TreeLock::default();
    let tip = h.tip();
    let held = tree.try_hold().unwrap();
    assert!(matches!(thin(&h.repo, &tree, Duration::from_millis(50), NOW as u64).unwrap(), Outcome::Busy(_)));
    assert_eq!(h.tip(), tip);
    assert_eq!(last_thinned(&h.repo), None, "a run that did not start leaves no time");
    drop(held);
    assert!(matches!(thin(&h.repo, &tree, WAIT, NOW as u64).unwrap(), Outcome::Thinned { .. }));
    assert!(tree.try_hold().is_some(), "the lock is released");
}

// spec: SYNC-070
#[test]
fn a_run_is_due_when_there_was_none_or_the_last_is_a_day_old() {
    assert!(due(None, 5));
    assert!(!due(Some(1000), 1000 + INTERVAL_SECS - 1));
    assert!(due(Some(1000), 1000 + INTERVAL_SECS));
    assert!(!due(Some(5000), 100), "a clock that went back waits");
    let h = setup("thin-due");
    ensure(&h.repo).unwrap();
    assert_eq!(last_thinned(&h.repo), None);
    // a history that has nothing to drop still records the run
    assert_eq!(run_if_due(&h.repo, &TreeLock::default(), WAIT, 1000).unwrap(), Outcome::Unchanged);
    assert_eq!(last_thinned(&h.repo), Some(1000));
    assert_eq!(fs::read_to_string(h.gd().join(STAMP_FILE)).unwrap(), "1000\n");
    assert_eq!(run_if_due(&h.repo, &TreeLock::default(), WAIT, 1000 + INTERVAL_SECS - 1).unwrap(), Outcome::NotDue);
    assert_eq!(run_if_due(&h.repo, &TreeLock::default(), WAIT, 1000 + INTERVAL_SECS).unwrap(), Outcome::Unchanged);
    assert_eq!(last_thinned(&h.repo), Some(1000 + INTERVAL_SECS));
}
