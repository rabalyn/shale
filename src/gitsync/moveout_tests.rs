//! Tests of moving a vault's `.git` out into the local history place (SYNC-062, SYNC-063),
//! against real `git` in temporary folders.

use super::history::{self, Reason};
use super::moveout::{move_out, refusal};
use super::tests::{read, write, Env};
use super::*;
use std::fs;

const WAIT: Duration = Duration::from_secs(5);

/// A vault with a repository of two commits and no remote, and an empty history folder.
fn vault_with_repo(env: &Env) -> (PathBuf, PathBuf) {
    let v = env.folder("vault");
    env.git(&v, &["init", "--quiet"]);
    write(&v, "Note.md", "one\n");
    env.git(&v, &["add", "-A"]);
    env.git(&v, &["-c", "user.name=T", "-c", "user.email=t@x", "commit", "--quiet", "-m", "first"]);
    write(&v, "Note.md", "two\n");
    env.git(&v, &["-c", "user.name=T", "-c", "user.email=t@x", "commit", "--quiet", "-am", "second"]);
    (v, env.folder("data/history"))
}

// spec: SYNC-063, NET-017
#[test]
fn sync_063_the_repository_moves_out_checked_and_the_history_goes_on_there() {
    let env = Env::new("moveout-ok");
    let (v, root) = vault_with_repo(&env);
    let head = env.git(&v, &["rev-parse", "HEAD"]);
    let tree = TreeLock::default();
    assert_eq!(refusal(&env.git, &v), None);

    let target = move_out(&env.git, &v, &root, &tree, WAIT).unwrap();
    assert!(tree.try_hold().is_some(), "the tree lock is let go");
    assert!(!v.join(".git").exists(), "the vault has no .git any more");
    assert!(!v.join(".git.shale-removing").exists());
    assert_eq!(read(&v, "Note.md"), "two\n", "the notes are untouched");
    let id = history::vault_id(&v).expect("the vault has an ID now");
    assert_eq!(target, root.join(&id));
    assert!(!root.join(format!("{id}.moving")).exists());
    assert_eq!(read(&root.join(&id), "worktree"), v.canonicalize().unwrap().to_string_lossy());

    // the history finds the repository, and the commits are all there
    let repo = history::locate(&env.git, &v, &root).unwrap();
    assert_eq!(repo.git_dir.as_deref(), Some(target.as_path()));
    assert_eq!(repo.run_for_test(&["rev-parse", "HEAD"]).trim(), head);
    let versions = history::versions(&repo, Path::new("Note.md"), 10).unwrap();
    assert_eq!(versions.len(), 2, "{versions:?}");
    // and a snapshot goes on in it
    write(&v, "Note.md", "three\n");
    let out = history::snapshot(&repo, &env.opts("laptop"), &history::Request::new(Reason::Save, vec![PathBuf::from("Note.md")]), &tree, Duration::from_secs(5)).unwrap();
    assert!(matches!(out, history::Outcome::Committed(_)), "{out:?}");
    assert_eq!(history::versions(&repo, Path::new("Note.md"), 10).unwrap().len(), 3);
    // a second move has nothing to move
    assert_eq!(refusal(&env.git, &v), None);
    assert!(!super::is_repo(&v));
}

// spec: SYNC-063
#[test]
fn sync_063_a_repository_with_a_remote_a_link_a_lock_or_a_merge_is_never_moved() {
    let env = Env::new("moveout-refuse");
    let (v, root) = vault_with_repo(&env);
    let tree = TreeLock::default();

    // a lock file of a running git
    write(&v, ".git/index.lock", "");
    let why = refusal(&env.git, &v).unwrap();
    assert!(why.contains("index.lock"), "{why}");
    assert!(move_out(&env.git, &v, &root, &tree, WAIT).is_err());
    assert!(v.join(".git").is_dir());
    fs::remove_file(v.join(".git/index.lock")).unwrap();

    // a merge in progress
    write(&v, ".git/MERGE_HEAD", &format!("{}\n", env.git(&v, &["rev-parse", "HEAD"])));
    assert!(refusal(&env.git, &v).unwrap().contains("middle of a merge"));
    fs::remove_file(v.join(".git/MERGE_HEAD")).unwrap();

    // a remote
    let remote = env.bare();
    env.git(&v, &["remote", "add", "origin", remote.to_str().unwrap()]);
    let why = refusal(&env.git, &v).unwrap();
    assert!(why.contains("connected to a server"), "{why}");
    let err = move_out(&env.git, &v, &root, &tree, WAIT).unwrap_err();
    assert!(err.contains("connected to a server"), "{err}");
    assert!(v.join(".git").is_dir() && fs::read_dir(&root).unwrap().next().is_none(), "nothing was copied");
    env.git(&v, &["remote", "remove", "origin"]);

    // a vault without a repository has nothing to refuse
    assert_eq!(refusal(&env.git, &env.folder("plain")), None);
}

// spec: SYNC-063
#[cfg(unix)]
#[test]
fn sync_063_a_failure_while_copying_leaves_the_vault_s_git_and_no_copy_behind() {
    let env = Env::new("moveout-fail");
    let (v, root) = vault_with_repo(&env);
    // a link inside .git is refused
    std::os::unix::fs::symlink("/etc", v.join(".git/linked")).unwrap();
    let err = move_out(&env.git, &v, &root, &TreeLock::default(), WAIT).unwrap_err();
    assert!(err.contains("is a link"), "{err}");
    fs::remove_file(v.join(".git/linked")).unwrap();
    // a socket inside .git cannot be opened for reading, by any user (an unreadable file would
    // not do: the root user reads everything), so the copy stops half way
    let socket = crate::fsutil::bind_unix_socket(&v.join(".git/s"));
    let err = move_out(&env.git, &v, &root, &TreeLock::default(), WAIT).unwrap_err();
    assert!(err.contains("could not copy") && err.contains(".git/s"), "{err}");
    assert!(v.join(".git").is_dir(), "the vault keeps its repository");
    let left: Vec<_> = fs::read_dir(&root).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert!(left.is_empty(), "no half copy is left: {left:?}");
    assert_eq!(env.git(&v, &["log", "--format=%s"]).lines().count(), 2, "the history is intact");
    // without the socket the same move works
    drop(socket);
    fs::remove_file(v.join(".git/s")).unwrap();
    move_out(&env.git, &v, &root, &TreeLock::default(), WAIT).unwrap();
    assert!(!v.join(".git").exists());
}

// spec: SYNC-063
#[test]
fn sync_063_a_vault_that_already_has_a_history_here_is_not_moved_over_it() {
    let env = Env::new("moveout-exists");
    let (v, root) = vault_with_repo(&env);
    let id = "0123456789abcdef0123456789abcdef";
    write(&v, ".shale/history-id", &format!("{id}\n"));
    fs::create_dir_all(root.join(id)).unwrap();
    let err = move_out(&env.git, &v, &root, &TreeLock::default(), WAIT).unwrap_err();
    assert!(err.contains("already has a history"), "{err}");
    assert!(v.join(".git").is_dir());
    assert!(fs::read_dir(root.join(id)).unwrap().next().is_none(), "the existing folder is untouched");
}

// spec: SYNC-063
#[test]
fn sync_063_the_move_waits_for_the_tree_lock_and_gives_up() {
    let env = Env::new("moveout-lock");
    let (v, root) = vault_with_repo(&env);
    let tree = TreeLock::default();
    let held = tree.hold();
    let t = std::time::Instant::now();
    let err = move_out(&env.git, &v, &root, &tree, Duration::from_millis(200)).unwrap_err();
    assert!(err.contains("still running") && t.elapsed() >= Duration::from_millis(190), "{err}");
    assert!(v.join(".git").is_dir());
    drop(held);
}

// spec: SYNC-064
#[test]
fn sync_064_a_moved_vault_keeps_its_history_through_its_id() {
    let env = Env::new("moveout-moved");
    let (v, root) = vault_with_repo(&env);
    move_out(&env.git, &v, &root, &TreeLock::default(), WAIT).unwrap();
    let id = history::vault_id(&v).unwrap();
    let new_place = env.root.join("elsewhere");
    fs::rename(&v, &new_place).unwrap();
    let repo = history::locate(&env.git, &new_place, &root).unwrap();
    assert_eq!(repo.git_dir.as_deref(), Some(root.join(&id).as_path()), "the same history");
    assert_eq!(history::versions(&repo, Path::new("Note.md"), 10).unwrap().len(), 2);
}
