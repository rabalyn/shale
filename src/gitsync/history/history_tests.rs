//! Tests of the file history engine, against real `git` in temporary folders.

use super::*;
use crate::gitsync::tests::{read, write, Env};
use std::fs;

const WAIT: Duration = Duration::from_secs(5);

struct H {
    env: Env,
    vault: PathBuf,
    root: PathBuf,
}

/// A vault without `.git` and an empty history folder.
fn setup(tag: &str) -> H {
    let env = Env::new(tag);
    let vault = env.folder("vault");
    let root = env.folder("data/history");
    H { env, vault, root }
}

impl H {
    /// The repository, made and empty: the first snapshot is not the start of a history.
    fn repo(&self) -> Repo {
        let repo = self.locate_only();
        ensure(&repo).unwrap();
        repo
    }

    /// The repository as `locate` says it: nothing is made yet.
    fn locate_only(&self) -> Repo {
        locate(&self.env.git, &self.vault, &self.root).unwrap()
    }

    fn opts(&self) -> Options {
        self.env.opts("laptop")
    }

    fn snap(&self, repo: &Repo, reason: Reason, paths: &[&str]) -> Outcome {
        snapshot(repo, &self.opts(), &Request::new(reason, paths.iter().map(PathBuf::from).collect()), &TreeLock::default(), WAIT).unwrap()
    }

    /// The messages of the history, newest first.
    fn messages(&self, repo: &Repo) -> Vec<String> {
        let o = repo.run(&osv!["log", "-z", "--format=%B"]).unwrap();
        String::from_utf8_lossy(&o.stdout).split('\0').filter(|m| !m.trim().is_empty()).map(|m| m.trim().to_string()).collect()
    }

    fn files_in_head(&self, repo: &Repo) -> Vec<String> {
        let o = repo.run(&osv!["ls-tree", "-r", "--name-only", "HEAD"]).unwrap();
        o.text().lines().map(str::to_string).collect()
    }
}

fn committed(o: Outcome) -> String {
    match o {
        Outcome::Committed(id) => id,
        other => panic!("expected a commit, got {other:?}"),
    }
}

// spec: SYNC-030, SYNC-031
#[test]
fn a_vault_without_git_gets_a_separate_git_dir_and_only_the_id_file_in_the_vault() {
    let h = setup("hist-layout");
    write(&h.vault, "Note.md", "one\n");
    let repo = h.repo();
    let id = vault_id(&h.vault).expect("the vault got an ID");
    assert_eq!(id.len(), 32);
    assert_eq!(repo.git_dir.as_deref(), Some(h.root.join(&id).as_path()));
    committed(h.snap(&repo, Reason::Save, &["Note.md"]));

    let mut names: Vec<String> = fs::read_dir(&h.vault).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    assert_eq!(names, [".shale", "Note.md"], "no .git in the vault");
    let inner: Vec<_> = fs::read_dir(h.vault.join(".shale")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(inner, ["history-id"]);
    let gd = repo.git_dir.clone().unwrap();
    assert!(gd.join("HEAD").exists() && gd.join("objects").is_dir(), "a normal git directory");
    assert_eq!(fs::read_to_string(gd.join("HEAD")).unwrap().trim(), "ref: refs/heads/main");
    assert_eq!(repo.run(&osv!["config", "core.bare"]).unwrap().text(), "false");
    assert_eq!(h.files_in_head(&repo), ["Note.md"]);
    assert!(!is_repo_here(&h.vault));
}

fn is_repo_here(v: &Path) -> bool {
    v.join(".git").exists()
}

// spec: SYNC-030
#[test]
fn a_vault_with_git_uses_its_own_repository() {
    let h = setup("hist-own");
    h.env.git(&h.vault, &["init", "--quiet"]);
    write(&h.vault, "Note.md", "one\n");
    let repo = h.repo();
    assert!(repo.git_dir.is_none());
    committed(h.snap(&repo, Reason::Save, &["Note.md"]));
    assert_eq!(h.env.git(&h.vault, &["log", "--format=%s"]), "Snapshot of Note.md");
    assert!(vault_id(&h.vault).is_none(), "no ID is needed");
    assert!(!h.vault.join(".shale").exists());
    assert_eq!(fs::read_dir(&h.root).unwrap().count(), 0, "nothing in the data folder");
}

// spec: SYNC-031, SYNC-064
#[test]
fn a_moved_vault_keeps_its_history_and_a_copy_gets_a_new_id() {
    let h = setup("hist-moved");
    write(&h.vault, "Note.md", "one\n");
    let repo = h.repo();
    committed(h.snap(&repo, Reason::Save, &["Note.md"]));
    let id = vault_id(&h.vault).unwrap();

    // a copy while the original is still there: a new ID, an empty history of its own
    let copy = h.env.folder("copy");
    write(&copy, "Note.md", "one\n");
    fs::create_dir_all(copy.join(".shale")).unwrap();
    fs::copy(h.vault.join(ID_FILE), copy.join(ID_FILE)).unwrap();
    let copy_repo = locate(&h.env.git, &copy, &h.root).unwrap();
    ensure(&copy_repo).unwrap();
    let new_id = vault_id(&copy).unwrap();
    assert_ne!(new_id, id, "the copy has its own ID");
    assert_eq!(copy_repo.git_dir.as_deref(), Some(h.root.join(&new_id).as_path()));
    assert_eq!(copy_repo.run(&osv!["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap().stdout.len(), 0, "an empty history of its own");
    assert_eq!(vault_id(&h.vault).unwrap(), id, "the original keeps its ID");
    // and it stays so
    assert_eq!(locate(&h.env.git, &copy, &h.root).unwrap().git_dir, copy_repo.git_dir);
    committed(h.snap(&copy_repo, Reason::Save, &["Note.md"]));
    assert_eq!(locate(&h.env.git, &copy, &h.root).unwrap().git_dir, copy_repo.git_dir);

    // the original moves: the folder is gone, the history follows
    let moved = h.env.root.join("renamed vault");
    fs::rename(&h.vault, &moved).unwrap();
    let again = locate(&h.env.git, &moved, &h.root).unwrap();
    assert_eq!(again.git_dir, repo.git_dir);
    write(&moved, "Note.md", "two\n");
    committed(h.snap(&again, Reason::Save, &["Note.md"]));
    let o = again.run(&osv!["log", "--format=%s"]).unwrap();
    assert_eq!(o.text().lines().count(), 2, "the old snapshot and the new one: {}", o.text());
    assert_eq!(fs::read_to_string(again.git_dir.unwrap().join("worktree")).unwrap().trim(), moved.canonicalize().unwrap().to_string_lossy());
}

// spec: SYNC-034
#[test]
fn no_commit_when_the_content_equals_the_history() {
    let h = setup("hist-unchanged");
    write(&h.vault, "Note.md", "one\n");
    let repo = h.repo();
    committed(h.snap(&repo, Reason::Save, &["Note.md"]));
    assert_eq!(h.snap(&repo, Reason::Save, &["Note.md"]), Outcome::Unchanged);
    // touched, same text: the content counts, not the time (NET-003)
    write(&h.vault, "Note.md", "one\n");
    assert_eq!(h.snap(&repo, Reason::Idle, &["Note.md"]), Outcome::Unchanged);
    assert_eq!(h.messages(&repo).len(), 1, "one snapshot");
    write(&h.vault, "Note.md", "two\n");
    committed(h.snap(&repo, Reason::Idle, &["Note.md"]));
    assert_eq!(h.messages(&repo).len(), 2);
}

// spec: SYNC-034
#[test]
fn skipped_names_and_untracked_kinds_are_never_snapshotted() {
    for name in [
        ".shale-tmp-1-2-3",
        ".syncthing.Note.md.tmp",
        ".stfolder",
        ".stversions/Note.md",
        ".stignore",
        ".~lock.Note.md#",
        "~$Note.md",
        "Note.tmp",
        "Note.md.part",
        ".nfs0001",
        ".fuse_hidden01",
        "Note.md.icloud",
        "desktop.ini",
        ".DS_Store",
        "Thumbs.db",
        "sub/Thumbs.db",
        ".shale/undo/ab.json",
        ".shale/state.json",
    ] {
        assert!(skipped(Path::new(name)), "{name} is skipped");
        assert!(!tracked(Path::new(name)), "{name} is not tracked");
    }
    for name in ["Note.md", "a/b/Note.md", "Plan.excalidraw.md", "Plan.excalidraw", "Board.canvas", "Résumé.MD"] {
        assert!(tracked(Path::new(name)), "{name} is tracked");
    }
    for name in ["pic.png", "song.mp3", "doc.pdf", ".trash/Note.md", ".hidden/Note.md", ".shale/settings.json"] {
        assert!(!tracked(Path::new(name)), "{name} is not tracked");
    }

    let h = setup("hist-skipped");
    write(&h.vault, "Note.md", "one\n");
    write(&h.vault, "~$Note.md", "lock\n");
    write(&h.vault, ".syncthing.Other.md.tmp", "tmp\n");
    write(&h.vault, ".shale/undo/ab.json", "{}\n");
    write(&h.vault, ".trash/Gone.md", "trash\n");
    write(&h.vault, "pic.png", "png\n");
    write(&h.vault, "sub/Deep.md", "deep\n");
    let repo = h.locate_only();
    // the first snapshot of a new history is the vault as it is: notes only (SYNC-044)
    assert_eq!(h.snap(&repo, Reason::Save, &["Note.md"]), Outcome::Unchanged, "the start of the history has it");
    assert_eq!(h.files_in_head(&repo), ["Note.md", "sub/Deep.md"]);
    for p in ["~$Note.md", ".syncthing.Other.md.tmp", ".shale/undo/ab.json", ".trash/Gone.md", "pic.png"] {
        assert_eq!(h.snap(&repo, Reason::Save, &[p]), Outcome::Unchanged, "{p}");
    }
    assert_eq!(h.files_in_head(&repo), ["Note.md", "sub/Deep.md"]);
}

// spec: SYNC-044
#[test]
fn a_new_local_history_starts_with_the_vault_as_it_is() {
    let h = setup("hist-initial");
    write(&h.vault, "A.md", "a\n");
    write(&h.vault, "sub/B.excalidraw.md", "b\n");
    write(&h.vault, "C.canvas", "{}\n");
    write(&h.vault, "pic.png", "png\n");
    let repo = h.locate_only();
    assert!(!repo.git_dir.as_ref().unwrap().exists(), "nothing is made by locating");
    committed(h.snap(&repo, Reason::Initial, &[]));
    write(&h.vault, "A.md", "a changed\n");
    committed(h.snap(&repo, Reason::Save, &["A.md"]));
    let msgs = h.messages(&repo);
    assert_eq!(msgs.len(), 2);
    assert!(msgs[1].starts_with("Start of the history") && msgs[1].contains("Shale-Reason: initial"), "{msgs:?}");
    assert!(!msgs[1].contains("Shale-Path"), "{msgs:?}");
    assert_eq!(h.files_in_head(&repo), ["A.md", "C.canvas", "sub/B.excalidraw.md"]);
    // the first version of A.md is the one that was there at the start
    let first = repo.ok(&osv!["rev-list", "--max-parents=0", "HEAD"]).unwrap().text();
    assert_eq!(content_at(&repo, &first, Path::new("A.md")).unwrap(), b"a\n");
    // a vault with a repository of its own makes no initial snapshot
    let g = setup("hist-initial-git");
    g.env.git(&g.vault, &["init", "--quiet"]);
    write(&g.vault, "A.md", "a\n");
    write(&g.vault, "B.md", "b\n");
    let r = g.repo();
    committed(g.snap(&r, Reason::Save, &["A.md"]));
    assert_eq!(g.files_in_head(&r), ["A.md"]);
}

// spec: MEDIA-023, SYNC-034
#[test]
fn the_file_history_never_takes_a_file_that_a_note_links_from_outside_the_vault() {
    let h = setup("hist-outside-media");
    let outside = h.env.folder("Videos");
    write(&outside, "clip.mp4", "outside video");
    write(&outside, "pic.png", "outside picture");
    write(&h.vault, "Trip.md", "![[@Videos/clip.mp4]] ![](<file:///elsewhere/pic.png>)\n");
    let repo = h.repo();
    committed(h.snap(&repo, Reason::Save, &["Trip.md"]));
    // asked for by a path out of the vault, or an absolute one, nothing is committed
    let absolute = outside.join("clip.mp4").to_string_lossy().into_owned();
    for p in ["../Videos/clip.mp4", "../Videos/pic.png", absolute.as_str()] {
        assert_eq!(h.snap(&repo, Reason::Save, &[p]), Outcome::Unchanged, "{p}");
    }
    assert_eq!(h.files_in_head(&repo), ["Trip.md"]);
    assert_eq!(h.messages(&repo).len(), 1, "one version, of the note");
    assert!(!outside.join(".git").exists() && fs::read_dir(&outside).unwrap().count() == 2, "nothing is written next to the outside files");
}

// spec: SYNC-034
#[test]
fn a_snapshot_commits_only_its_paths_and_leaves_other_staged_changes_alone() {
    let h = setup("hist-only");
    h.env.git(&h.vault, &["init", "--quiet"]);
    write(&h.vault, "Other.md", "other\n");
    write(&h.vault, "Mine.md", "mine\n");
    write(&h.vault, "pic.png", "png\n");
    h.env.git(&h.vault, &["add", "Other.md", "pic.png"]);
    let repo = h.repo();
    committed(h.snap(&repo, Reason::Save, &["Mine.md"]));
    assert_eq!(h.files_in_head(&repo), ["Mine.md"]);
    let status = h.env.git(&h.vault, &["status", "--short"]);
    assert!(status.contains("A  Other.md") && status.contains("A  pic.png"), "still staged: {status}");
    // a path that is a media file is not a snapshot's business, even in a vault repository
    assert_eq!(h.snap(&repo, Reason::Save, &["pic.png"]), Outcome::Unchanged);
    assert_eq!(h.files_in_head(&repo), ["Mine.md"]);
}

// spec: SYNC-036
#[test]
fn messages_carry_trailers_that_parse_and_a_message_without_them_gives_its_subject() {
    let h = setup("hist-trailers");
    write(&h.vault, "Note.md", "one\n");
    write(&h.vault, "Other.md", "two\n");
    let repo = h.repo();
    let mut req = Request::new(Reason::Restore, vec!["Note.md".into(), "Other.md".into()]);
    req.restored = Some("abc123".into());
    assert!(matches!(snapshot(&repo, &h.opts(), &req, &TreeLock::default(), WAIT).unwrap(), Outcome::Committed(_)));
    let msg = h.messages(&repo).remove(0);
    let meta = parse_message(&msg);
    assert_eq!(meta.subject, "Restore 2 file(s)");
    assert_eq!(meta.reason, Some(Reason::Restore));
    assert_eq!(meta.device.as_deref(), Some("laptop"));
    assert_eq!(meta.paths, ["Note.md", "Other.md"]);
    assert_eq!(meta.restored.as_deref(), Some("abc123"));

    // a commit of another tool, and an unknown reason
    let plain = parse_message("Fixed the typo\n\nLonger text.\nCo-authored-by: x <y@z>\n");
    assert_eq!(plain, Meta { subject: "Fixed the typo".into(), ..Default::default() });
    assert_eq!(parse_message("").subject, "");
    assert_eq!(parse_message("s\n\nShale-Reason: something-new\n").reason, None);
    for r in [Reason::Save, Reason::Idle, Reason::Close, Reason::Initial, Reason::Sync, Reason::Restore, Reason::Merge, Reason::Rename, Reason::Delete, Reason::Import] {
        assert_eq!(Reason::parse(r.as_str()), Some(r));
    }
}

// spec: SYNC-036, SYNC-006
#[test]
fn the_syncs_own_commits_carry_trailers_after_prefix_and_suffix() {
    let env = Env::new("hist-sync-trailers");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    let b = env.vault("b", &remote);
    write(&a, "N.md", "one\n");
    let opts = Options { commit_prefix: "[n] ".into(), commit_suffix: " (auto)".into(), ..env.opts("laptop") };
    crate::gitsync::sync(&env.git, &a, &opts, &TreeLock::default()).unwrap();
    let msg = env.git(&a, &["log", "-1", "--format=%B"]);
    let meta = parse_message(&msg);
    assert_eq!(meta.subject, "[n] Sync from laptop: 1 file(s) (auto)");
    assert_eq!((meta.reason, meta.device.as_deref()), (Some(Reason::Sync), Some("laptop")));
    // a merge
    write(&b, "N.md", "two\n");
    write(&a, "M.md", "m\n");
    env.sync(&b);
    crate::gitsync::sync(&env.git, &a, &opts, &TreeLock::default()).unwrap();
    let merge = env.git(&a, &["log", "-1", "--merges", "--format=%B"]);
    assert_eq!(parse_message(&merge).reason, Some(Reason::Merge), "{merge}");
}

// spec: SYNC-037, SYNC-012
#[test]
fn the_sync_pushes_snapshots_as_they_are() {
    let env = Env::new("hist-push");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    let b = env.vault("b", &remote);
    let h = H { vault: a.clone(), root: env.folder("data/history"), env };
    write(&a, "N.md", "one\n");
    let repo = h.repo();
    assert!(repo.git_dir.is_none());
    let first = committed(h.snap(&repo, Reason::Save, &["N.md"]));
    write(&a, "N.md", "two\n");
    let second = committed(h.snap(&repo, Reason::Idle, &["N.md"]));
    write(&a, "N.md", "three\n");
    h.env.sync(&a);
    // the remote has both snapshots, with their ids and messages, and the sync's own commit on top
    let log = h.env.git(&remote, &["log", "--format=%H", "main"]);
    let ids: Vec<&str> = log.lines().collect();
    assert_eq!(ids.len(), 3, "{ids:?}");
    assert_eq!(&ids[1..], [second.as_str(), first.as_str()]);
    // another device sees the versions
    h.env.sync(&b);
    let theirs = locate(&h.env.git, &b, &h.root).unwrap();
    let versions = versions(&theirs, Path::new("N.md"), 10).unwrap();
    assert_eq!(versions.iter().map(|v| v.meta.reason).collect::<Vec<_>>(), [Some(Reason::Sync), Some(Reason::Idle), Some(Reason::Save)]);
    assert_eq!(read(&b, "N.md"), "three\n");
}

// spec: SYNC-035
#[test]
fn a_snapshot_waits_for_the_tree_and_a_busy_repository_is_asked_again_later() {
    let h = setup("hist-busy");
    h.env.git(&h.vault, &["init", "--quiet"]);
    write(&h.vault, "N.md", "one\n");
    let repo = h.repo();
    let req = Request::new(Reason::Save, vec!["N.md".into()]);
    // a running sync holds the tree lock: the snapshot gives up after its wait
    let tree = TreeLock::default();
    let held = tree.try_hold().unwrap();
    let out = snapshot(&repo, &h.opts(), &req, &tree, Duration::from_millis(50)).unwrap();
    assert!(matches!(out, Outcome::Busy(_)), "{out:?}");
    assert!(repo.run(&osv!["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap().stdout.is_empty(), "nothing was committed");
    drop(held);
    // a sync that lets go during the wait: the snapshot goes on
    let held = tree.try_hold().unwrap();
    let t2 = tree.clone();
    let waiter = std::thread::spawn({
        let (repo, opts, req) = (repo.clone(), h.opts(), req.clone());
        move || snapshot(&repo, &opts, &req, &t2, Duration::from_secs(10)).unwrap()
    });
    std::thread::sleep(Duration::from_millis(100));
    drop(held);
    assert!(matches!(waiter.join().unwrap(), Outcome::Committed(_)));

    // git's own lock on the index
    write(&h.vault, "N.md", "two\n");
    let lock = h.vault.join(".git/index.lock");
    fs::write(&lock, "").unwrap();
    let out = snapshot(&repo, &h.opts(), &req, &TreeLock::default(), WAIT).unwrap();
    assert!(matches!(out, Outcome::Busy(_)), "{out:?}");
    fs::remove_file(&lock).unwrap();
    assert!(matches!(snapshot(&repo, &h.opts(), &req, &TreeLock::default(), WAIT).unwrap(), Outcome::Committed(_)));

    // a merge in progress and a detached HEAD
    h.env.git(&h.vault, &["checkout", "--quiet", "--detach"]);
    write(&h.vault, "N.md", "three\n");
    assert!(matches!(snapshot(&repo, &h.opts(), &req, &TreeLock::default(), WAIT).unwrap(), Outcome::Busy(_)));
    h.env.git(&h.vault, &["checkout", "--quiet", "main"]);
    fs::write(h.vault.join(".git/MERGE_HEAD"), h.env.git(&h.vault, &["rev-parse", "HEAD"])).unwrap();
    assert!(matches!(snapshot(&repo, &h.opts(), &req, &TreeLock::default(), WAIT).unwrap(), Outcome::Busy(_)));
}

// spec: SYNC-043, SYNC-041
#[test]
fn versions_follow_renames_and_read_the_text_at_each_version() {
    let h = setup("hist-versions");
    write(&h.vault, "Old.md", "one\ntwo\n");
    let repo = h.repo();
    let v1 = committed(h.snap(&repo, Reason::Save, &["Old.md"]));
    write(&h.vault, "Old.md", "one\ntwo\nthree\n");
    let v2 = committed(h.snap(&repo, Reason::Idle, &["Old.md"]));
    fs::create_dir_all(h.vault.join("sub")).unwrap();
    fs::rename(h.vault.join("Old.md"), h.vault.join("sub/New.md")).unwrap();
    let v3 = committed(h.snap(&repo, Reason::Rename, &["Old.md", "sub/New.md"]));
    write(&h.vault, "sub/New.md", "uno\ntwo\nthree\n");
    let v4 = committed(h.snap(&repo, Reason::Close, &["sub/New.md"]));

    let list = versions(&repo, Path::new("sub/New.md"), 10).unwrap();
    assert_eq!(list.iter().map(|v| v.commit.as_str()).collect::<Vec<_>>(), [v4.as_str(), v3.as_str(), v2.as_str(), v1.as_str()]);
    assert_eq!(list.iter().map(|v| v.path.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["sub/New.md", "sub/New.md", "Old.md", "Old.md"]);
    assert_eq!(list.iter().map(|v| v.meta.reason).collect::<Vec<_>>(), [Some(Reason::Close), Some(Reason::Rename), Some(Reason::Idle), Some(Reason::Save)]);
    assert_eq!(list.iter().map(|v| v.lines).collect::<Vec<_>>(), [Some((1, 1)), Some((0, 0)), Some((1, 0)), Some((2, 0))]);
    assert!(list.iter().all(|v| !v.binary && v.meta.device.as_deref() == Some("laptop") && v.time > 0));
    assert_eq!(versions(&repo, Path::new("sub/New.md"), 2).unwrap().len(), 2);
    assert_eq!(content_at(&repo, &v2, Path::new("Old.md")).unwrap(), b"one\ntwo\nthree\n");
    assert_eq!(content_at(&repo, &v4, Path::new("sub/New.md")).unwrap(), b"uno\ntwo\nthree\n");
    assert!(content_at(&repo, &v4, Path::new("Old.md")).is_err());
    assert!(content_at(&repo, "--output=x", Path::new("Old.md")).is_err());
    // a folder rename names both folders
    fs::rename(h.vault.join("sub"), h.vault.join("moved")).unwrap();
    committed(h.snap(&repo, Reason::Rename, &["sub", "moved"]));
    let list = versions(&repo, Path::new("moved/New.md"), 10).unwrap();
    assert_eq!(list.len(), 5, "{list:?}");
    assert_eq!(list[0].path, Path::new("moved/New.md"));
    // nothing yet
    let empty = setup("hist-versions-empty");
    assert!(versions(&empty.repo(), Path::new("N.md"), 5).unwrap().is_empty());
    assert!(deleted(&empty.repo(), 5).unwrap().is_empty());
}

// spec: SYNC-042
#[test]
fn a_binary_version_has_no_line_counts() {
    let h = setup("hist-binary");
    fs::write(h.vault.join("Odd.md"), b"text\n").unwrap();
    let repo = h.repo();
    committed(h.snap(&repo, Reason::Save, &["Odd.md"]));
    fs::write(h.vault.join("Odd.md"), b"bin\0ary\0\x01\x02").unwrap();
    committed(h.snap(&repo, Reason::Save, &["Odd.md"]));
    let list = versions(&repo, Path::new("Odd.md"), 5).unwrap();
    assert_eq!((list[0].binary, list[0].lines), (true, None));
    assert_eq!((list[1].binary, list[1].lines), (false, Some((1, 0))));
}

// spec: SYNC-040, SYNC-043
#[test]
fn deleted_notes_are_listed_and_their_last_text_is_found_and_a_taken_name_gets_a_number() {
    let h = setup("hist-deleted");
    write(&h.vault, "Keep.md", "keep\n");
    write(&h.vault, "Gone.md", "gone v1\n");
    write(&h.vault, "dir/Moved.md", "moved\n");
    write(&h.vault, "Trashed.md", "trashed\n");
    write(&h.vault, "Back.md", "back\n");
    let repo = h.repo();
    committed(h.snap(&repo, Reason::Save, &["Keep.md", "dir/Moved.md", "Trashed.md", "Back.md"]));
    committed(h.snap(&repo, Reason::Save, &["Gone.md"]));
    write(&h.vault, "Gone.md", "gone v2\n");
    committed(h.snap(&repo, Reason::Save, &["Gone.md"]));
    // a deletion, a rename (no deletion), a recreated note and a move to the trash
    fs::remove_file(h.vault.join("Gone.md")).unwrap();
    committed(h.snap(&repo, Reason::Delete, &["Gone.md"]));
    fs::rename(h.vault.join("dir/Moved.md"), h.vault.join("Moved.md")).unwrap();
    committed(h.snap(&repo, Reason::Rename, &["dir/Moved.md", "Moved.md"]));
    fs::remove_file(h.vault.join("Back.md")).unwrap();
    committed(h.snap(&repo, Reason::Delete, &["Back.md"]));
    write(&h.vault, "Back.md", "again\n");
    fs::create_dir_all(h.vault.join(".trash")).unwrap();
    fs::rename(h.vault.join("Trashed.md"), h.vault.join(".trash/Trashed.md")).unwrap();
    committed(h.snap(&repo, Reason::Delete, &["Trashed.md"]));

    let list = deleted(&repo, 10).unwrap();
    let names: Vec<String> = list.iter().map(|d| d.path.to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["Trashed.md", "Gone.md"], "{list:?}");
    assert_eq!(list[1].meta.reason, Some(Reason::Delete));
    assert_eq!(deleted_content(&repo, &list[1]).unwrap(), b"gone v2\n");
    assert_eq!(deleted(&repo, 1).unwrap().len(), 1);

    assert_eq!(restored_name(Path::new("a/Gone.md"), |_| false), Path::new("a/Gone (restored).md"));
    let taken = [PathBuf::from("a/Gone (restored).md"), PathBuf::from("a/Gone (restored 2).md")];
    assert_eq!(restored_name(Path::new("a/Gone.md"), |p| taken.iter().any(|t| t == p)), Path::new("a/Gone (restored 3).md"));
    assert_eq!(restored_name(Path::new("Plan.excalidraw.md"), |_| false), Path::new("Plan (restored).excalidraw.md"));
}

// spec: SYNC-040
#[test]
fn a_note_moved_to_the_trash_by_the_sync_is_a_deletion_too() {
    let env = Env::new("hist-trash-sync");
    let remote = env.bare();
    let v = env.vault("v", &remote);
    write(&v, "T.md", "text\n");
    env.sync(&v);
    fs::create_dir_all(v.join(".trash")).unwrap();
    fs::rename(v.join("T.md"), v.join(".trash/T.md")).unwrap();
    env.sync(&v);
    let repo = locate(&env.git, &v, &env.root).unwrap();
    let list = deleted(&repo, 10).unwrap();
    assert_eq!(list.len(), 1, "{list:?}");
    assert_eq!(list[0].path, Path::new("T.md"));
    assert_eq!(list[0].meta.reason, Some(Reason::Sync));
    assert_eq!(deleted_content(&repo, &list[0]).unwrap(), b"text\n");
}

// spec: SYNC-033
#[test]
fn the_schedule_coalesces_saves_and_asks_again_after_idle_and_close() {
    let cfg = HistoryConfig { enabled: true, coalesce_minutes: 10, idle_seconds: 60, show_panel: true };
    let n = Path::new("N.md");
    let mut s = Schedule::default();
    let typing = |_: &Path| true;
    let resting = |_: &Path| false;
    assert_eq!(s.next_in(0, &cfg), None);
    // the first save is snapshotted at once
    s.saved(n, 1000);
    assert!(s.has_pending());
    assert_eq!(s.due(1000, &cfg, typing), [(n.to_path_buf(), Reason::Save)]);
    assert!(!s.has_pending());
    // saves while typing: one snapshot per 10 minutes at most
    s.saved(n, 1030);
    assert!(s.due(1030, &cfg, typing).is_empty());
    s.saved(n, 1500);
    assert!(s.due(1500, &cfg, typing).is_empty());
    assert_eq!(s.next_in(1500, &cfg), Some(60));
    assert_eq!(s.due(1599, &cfg, typing), []);
    assert_eq!(s.due(1600, &cfg, typing), [(n.to_path_buf(), Reason::Save)]);
    // idle: no change for 60 s and no unsaved buffer
    s.saved(n, 1700);
    assert!(s.due(1759, &cfg, resting).is_empty());
    assert!(s.due(1760, &cfg, typing).is_empty(), "an unsaved buffer is not idle");
    assert_eq!(s.due(1760, &cfg, resting), [(n.to_path_buf(), Reason::Idle)]);
    // closing the last tab: now
    s.saved(n, 1770);
    s.closed(n);
    assert_eq!(s.due(1771, &cfg, typing), [(n.to_path_buf(), Reason::Close)]);
    // a tab closed without pending changes does nothing
    s.closed(n);
    assert!(s.due(5000, &cfg, resting).is_empty());
    // a snapshot that was busy is asked for again, at once
    s.saved(n, 6000);
    let due = s.due(6000, &cfg, typing);
    s.again(&due.iter().map(|d| d.0.clone()).collect::<Vec<_>>(), 6000);
    assert_eq!(s.due(6001, &cfg, typing), [(n.to_path_buf(), Reason::Save)]);
    // window close: everything pending
    s.saved(n, 6002);
    s.saved(Path::new("M.md"), 6003);
    assert_eq!(s.take_all(), [PathBuf::from("M.md"), n.to_path_buf()]);
    assert!(!s.has_pending());
    s.saved(n, 7000);
    s.forget(n);
    assert!(!s.has_pending());
}

// spec: CFG-066, HIST-003
#[test]
fn the_history_settings_default_to_on_load_from_old_files_round_trip_and_stay_in_range() {
    let c = crate::config::Config::default();
    assert_eq!((c.history.enabled, c.history.coalesce_minutes, c.history.idle_seconds), (true, 10, 60));
    let (old, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"vault": "/tmp/v"}"#).unwrap();
    assert_eq!(old.history, HistoryConfig::default(), "a file from before has no history settings");
    let (c, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"history": {"coalesce_minutes": 3}}"#).unwrap();
    assert_eq!(c.history, HistoryConfig { coalesce_minutes: 3, ..HistoryConfig::default() }, "a missing field takes its default");
    let (c, _) = crate::vault_state::parse::<crate::config::Config>(br#"{"history": {"enabled": "maybe"}}"#).unwrap();
    assert_eq!(c.history, HistoryConfig::default(), "a value of the wrong shape loads as the default");
    let c = crate::config::Config { history: HistoryConfig { enabled: false, coalesce_minutes: 30, idle_seconds: 9, show_panel: false }, ..Default::default() };
    let text = crate::config::text::to_toml(&c);
    assert!(text.contains("[history]") && text.contains("enabled = false") && text.contains("coalesce_minutes = 30") && text.contains("idle_seconds = 9") && text.contains("show_panel = false"), "{text}");
    let (back, _) = crate::config::Config::parse_text(&text).unwrap();
    assert_eq!(back.history, c.history);
    // a number out of range works as its nearest limit
    let d = HistoryConfig::default();
    assert_eq!(HistoryConfig { coalesce_minutes: 0, idle_seconds: 0, ..d.clone() }.coalesce_secs(), 60);
    assert_eq!(HistoryConfig { coalesce_minutes: 0, idle_seconds: 0, ..d.clone() }.idle_secs(), 5);
    assert_eq!(HistoryConfig { coalesce_minutes: u32::MAX, idle_seconds: u32::MAX, ..d }.coalesce_secs(), 86400);
}
