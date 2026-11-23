//! Tests of moving a local-only history into the vault's new `.git` when a server is connected
//! (SYNC-072…SYNC-074), against real `git` in temporary folders.

use super::history::{self, thin, Reason, Request};
use super::movein::{local_history_of, move_in};
use super::tests::{read, write, Env};
use super::*;
use std::fs;

const WAIT: Duration = Duration::from_secs(5);

/// A vault without `.git` whose local history has three snapshots of `Note.md` and one of `Other.md`.
fn vault_with_history(env: &Env) -> (PathBuf, PathBuf) {
    let v = env.folder("vault");
    let root = env.folder("data/history");
    let repo = history::locate(&env.git, &v, &root).unwrap();
    history::ensure(&repo).unwrap();
    let tree = TreeLock::default();
    for (i, text) in ["one\n", "two\n", "three\n"].iter().enumerate() {
        write(&v, "Note.md", text);
        if i == 1 {
            write(&v, "Other.md", "other\n");
        }
        let paths = if i == 1 { vec![PathBuf::from("Note.md"), PathBuf::from("Other.md")] } else { vec![PathBuf::from("Note.md")] };
        let out = history::snapshot(&repo, &env.opts("laptop"), &Request::new(Reason::Save, paths), &tree, WAIT).unwrap();
        assert!(matches!(out, history::Outcome::Committed(_)), "{out:?}");
    }
    (v, root)
}

fn versions(env: &Env, v: &Path, root: &Path) -> Vec<history::Version> {
    let repo = history::locate(&env.git, v, root).unwrap();
    history::versions(&repo, Path::new("Note.md"), 50).unwrap()
}

fn commits(env: &Env, dir: &Path) -> Vec<String> {
    env.git(dir, &["rev-list", "main"]).lines().map(str::to_string).collect()
}

// spec: SYNC-072, SYNC-073, SYNC-074
#[test]
fn sync_072_connecting_an_empty_remote_moves_the_history_into_git_and_the_first_sync_pushes_it_as_it_is() {
    let env = Env::new("movein-empty");
    let (v, root) = vault_with_history(&env);
    let remote = env.bare();
    let before = versions(&env, &v, &root);
    assert_eq!(before.len(), 3, "{before:?}");
    let id = history::vault_id(&v).unwrap();
    let old = root.join(&id);
    let tree = TreeLock::default();

    assert!(connect_moving(&env.git, &v, remote.to_str().unwrap(), &root, &tree).unwrap(), "a history moved");
    assert!(tree.try_hold().is_some(), "the tree lock is let go");
    assert!(v.join(".git/HEAD").is_file());
    assert!(!old.exists() && !root.join(format!("{id}.moved")).exists(), "the old folder is gone");
    assert!(!v.join(".git.moving").exists() && !v.join(".git.shale-removing").exists());
    // a normal repository now
    assert_eq!(env.git(&v, &["config", "core.bare"]), "false");
    assert!(env.git.run(&v, &[OsStr::new("config"), OsStr::new("core.worktree")]).unwrap().text().is_empty());
    assert!(!v.join(".git/worktree").exists() && !v.join(".git/thinned").exists(), "what belongs to the local place stays behind");
    assert_eq!(remote_url(&env.git, &v).as_deref(), remote.to_str());
    assert_eq!(read(&v, "Note.md"), "three\n");
    // the panel finds the same versions, now from `.git`
    let repo = history::locate(&env.git, &v, &root).unwrap();
    assert!(repo.git_dir.is_none());
    assert_eq!(versions(&env, &v, &root), before);

    // the first sync pushes the snapshots as they are: same commits, no rewrite
    let local = commits(&env, &v);
    assert_eq!(local.len(), 3);
    let r = env.sync(&v);
    assert!(r.pushed, "{r:?}");
    let on_remote = commits(&env, &remote);
    assert!(local.iter().all(|c| on_remote.contains(c)), "every snapshot is on the server: {local:?} in {on_remote:?}");
    assert_eq!(env.git(&remote, &["show", "main:Note.md"]), "three");
    // the sync's own commit (the `.shale/history-id` file) sits on top; nothing was rewritten
    assert_eq!(on_remote.len(), 4);
    assert_eq!(on_remote[1..], local[..]);
}

// spec: SYNC-072, SYNC-074
#[test]
fn sync_074_connecting_a_remote_with_notes_merges_and_keeps_our_old_versions() {
    let env = Env::new("movein-notes");
    let remote = env.bare();
    // another computer's notes: another note, and its own text of Other.md
    let b = env.vault("b", &remote);
    write(&b, "Theirs.md", "theirs\n");
    write(&b, "Other.md", "b's other\n");
    env.sync(&b);

    let (v, root) = vault_with_history(&env);
    let before = versions(&env, &v, &root);
    assert!(connect_moving(&env.git, &v, remote.to_str().unwrap(), &root, &TreeLock::default()).unwrap());
    let ours = commits(&env, &v);
    let r = env.sync(&v);
    assert!(r.pushed && r.conflicts.len() == 1, "{r:?}");
    // both sides are there, nothing of ours is lost
    assert_eq!(read(&v, "Theirs.md"), "theirs\n");
    assert_eq!(read(&v, "Note.md"), "three\n");
    assert_eq!(read(&v, "Other.md"), "b's other\n", "the server's text stays at the path");
    assert_eq!(read(&v, "Other (conflict 2026-11-14 1200).md"), "other\n", "ours is kept next to it");
    // our snapshots are still listed (the merge keeps them in the graph), and are on the server
    let after = versions(&env, &v, &root);
    for old in &before {
        assert!(after.iter().any(|n| n.commit == old.commit), "version {} is still listed", old.commit);
    }
    let on_remote = commits(&env, &remote);
    assert!(ours.iter().all(|c| on_remote.contains(c)), "our snapshots reached the server");
    // and the other computer gets our notes
    env.sync(&b);
    assert_eq!(read(&b, "Note.md"), "three\n");
}

// spec: SYNC-073
#[cfg(unix)]
#[test]
fn sync_073_a_failure_while_copying_leaves_no_git_in_the_vault_and_the_history_intact() {
    let env = Env::new("movein-fail");
    let (v, root) = vault_with_history(&env);
    let remote = env.bare();
    let id = history::vault_id(&v).unwrap();
    let before = versions(&env, &v, &root);
    // a socket cannot be opened for reading, by any user (an unreadable file would not do as root)
    let socket = crate::fsutil::bind_unix_socket(&root.join(&id).join("s"));
    let err = connect_moving(&env.git, &v, remote.to_str().unwrap(), &root, &TreeLock::default()).unwrap_err();
    assert!(err.contains("could not copy") && err.contains("/s"), "{err}");
    assert!(!v.join(".git").exists() && !v.join(".git.moving").exists(), "the vault has no .git");
    assert!(root.join(&id).join("HEAD").is_file(), "the local history stays");
    assert_eq!(versions(&env, &v, &root), before, "the history is intact");
    assert!(!root.join(format!("{id}.moved")).exists());
    // a link inside the history is refused the same way
    drop(socket);
    fs::remove_file(root.join(&id).join("s")).unwrap();
    std::os::unix::fs::symlink("/etc", root.join(&id).join("linked")).unwrap();
    let err = connect_moving(&env.git, &v, remote.to_str().unwrap(), &root, &TreeLock::default()).unwrap_err();
    assert!(err.contains("is a link"), "{err}");
    assert!(!v.join(".git").exists());
    // without the obstacle the same connection works
    fs::remove_file(root.join(&id).join("linked")).unwrap();
    assert!(connect_moving(&env.git, &v, remote.to_str().unwrap(), &root, &TreeLock::default()).unwrap());
    assert_eq!(versions(&env, &v, &root), before);
}

// spec: SYNC-073
#[cfg(unix)]
#[test]
fn sync_073_a_copy_that_does_not_pass_the_check_is_not_used() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new("movein-fsck");
    let (v, root) = vault_with_history(&env);
    let gd = root.join(history::vault_id(&v).unwrap());
    let before = versions(&env, &v, &root);
    // a damaged object makes `git fsck` fail on the copy
    let dir = fs::read_dir(gd.join("objects")).unwrap().flatten().find(|e| e.file_name().len() == 2).unwrap().path();
    let file = fs::read_dir(&dir).unwrap().next().unwrap().unwrap().path();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).unwrap();
    fs::write(&file, b"not a git object").unwrap();
    let err = move_in(&env.git, &v, &root, &TreeLock::default(), WAIT).unwrap_err();
    assert!(err.contains("did not pass the check"), "{err}");
    assert!(!v.join(".git").exists() && !v.join(".git.moving").exists(), "the vault has no .git");
    assert!(gd.join("HEAD").is_file() && gd.join("refs").is_dir(), "the local history stays");
    assert!(before.len() == 3);
}

// spec: SYNC-072
#[test]
fn sync_072_a_vault_without_a_local_history_connects_as_before_and_one_with_git_is_left_alone() {
    let env = Env::new("movein-none");
    let root = env.folder("data/history");
    let remote = env.bare();
    let v = env.folder("plain");
    write(&v, "Note.md", "hi\n");
    assert_eq!(local_history_of(&v, &root), None);
    assert!(!connect_moving(&env.git, &v, remote.to_str().unwrap(), &root, &TreeLock::default()).unwrap());
    assert!(v.join(".git").is_dir() && remote_url(&env.git, &v).is_some());
    assert!(!v.join(".shale/history-id").exists(), "looking for a history gives the vault no ID");
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    // a vault with its own `.git` is a repository already: an old local history is not touched
    let env2 = Env::new("movein-git");
    let (w, root2) = vault_with_history(&env2);
    env2.git(&w, &["init", "--quiet"]);
    assert_eq!(local_history_of(&w, &root2), None);
    assert_eq!(move_in(&env2.git, &w, &root2, &TreeLock::default(), WAIT).unwrap(), None);
    assert!(fs::read_dir(&root2).unwrap().next().is_some(), "the old history is still there");
}

// spec: SYNC-072, SYNC-069
#[test]
fn sync_072_after_the_move_the_history_is_the_vault_s_own_and_is_never_thinned() {
    let env = Env::new("movein-thin");
    let (v, root) = vault_with_history(&env);
    // before: due for thinning
    let repo = history::locate(&env.git, &v, &root).unwrap();
    assert!(!matches!(thin::run_if_due(&repo, &TreeLock::default(), WAIT, 1).unwrap(), thin::Outcome::Skipped));
    connect_moving(&env.git, &v, env.bare().to_str().unwrap(), &root, &TreeLock::default()).unwrap();
    let repo = history::locate(&env.git, &v, &root).unwrap();
    assert!(repo.git_dir.is_none());
    assert_eq!(thin::run_if_due(&repo, &TreeLock::default(), WAIT, 1_000_000_000_000).unwrap(), thin::Outcome::Skipped);
    // and a snapshot goes on in `.git`
    write(&v, "Note.md", "four\n");
    let out = history::snapshot(&repo, &env.opts("laptop"), &Request::new(Reason::Save, vec![PathBuf::from("Note.md")]), &TreeLock::default(), WAIT).unwrap();
    assert!(matches!(out, history::Outcome::Committed(_)), "{out:?}");
    assert_eq!(versions(&env, &v, &root).len(), 4);
}

// spec: SYNC-072
#[test]
fn sync_072_the_move_waits_for_the_tree_lock_and_gives_up_leaving_everything() {
    let env = Env::new("movein-lock");
    let (v, root) = vault_with_history(&env);
    let id = history::vault_id(&v).unwrap();
    let tree = TreeLock::default();
    let held = tree.hold();
    let t = std::time::Instant::now();
    let err = move_in(&env.git, &v, &root, &tree, Duration::from_millis(200)).unwrap_err();
    assert!(err.contains("still running") && t.elapsed() >= Duration::from_millis(190), "{err}");
    assert!(!v.join(".git").exists() && root.join(&id).join("HEAD").is_file());
    // a running git in the history is waited for as well
    drop(held);
    write(&root.join(&id), "index.lock", "");
    let err = move_in(&env.git, &v, &root, &tree, WAIT).unwrap_err();
    assert!(err.contains("index.lock"), "{err}");
    assert!(!v.join(".git").exists());
}
