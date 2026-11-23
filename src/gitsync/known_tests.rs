//! The mark of a repository Shale knows (SYNC-076).

use super::tests::Env;
use super::{connect, is_repo, known};
use std::path::Path;

fn init(env: &Env, dir: &Path) {
    assert!(std::process::Command::new("git").arg("-C").arg(dir).args(["init", "--quiet"]).envs(env.git.env.iter().cloned()).status().unwrap().success());
}

// spec: SYNC-076
#[test]
fn sync_076_a_repository_is_unknown_until_marked_and_the_mark_stays_inside_git() {
    let env = Env::new("known-mark");
    let v = env.folder("vault");
    assert!(!known::is_known(&v), "no .git, nothing known");
    known::mark_known(&v).unwrap();
    assert!(!v.join(".git").exists(), "no .git is made by a mark");
    init(&env, &v);
    assert!(!known::is_known(&v));
    known::mark_known(&v).unwrap();
    assert!(known::is_known(&v));
    assert!(v.join(".git/shale-known").is_file(), "the mark is inside .git, never in the working tree");
}

// spec: SYNC-076
#[test]
fn sync_076_a_dot_git_file_points_to_the_git_directory_to_mark() {
    let env = Env::new("known-gitfile");
    let (v, real) = (env.folder("vault"), env.folder("real"));
    std::fs::write(v.join(".git"), format!("gitdir: {}\n", real.display())).unwrap();
    assert!(is_repo(&v) && !known::is_known(&v));
    known::mark_known(&v).unwrap();
    assert!(known::is_known(&v) && real.join("shale-known").is_file());
    // a relative target is read from the vault
    let rel = env.folder("rel");
    std::fs::create_dir_all(rel.join("store")).unwrap();
    std::fs::write(rel.join(".git"), "gitdir: store\n").unwrap();
    known::mark_known(&rel).unwrap();
    assert!(rel.join("store/shale-known").is_file());
    // a .git file that points nowhere is a repository nobody can mark
    let bad = env.folder("bad");
    std::fs::write(bad.join(".git"), "gitdir: nowhere\n").unwrap();
    known::mark_known(&bad).unwrap();
    assert!(!known::is_known(&bad));
}

// spec: SYNC-076, SYNC-004
#[test]
fn sync_076_connect_marks_the_repository_it_makes_and_the_one_it_finds() {
    let env = Env::new("known-connect");
    let (made, found) = (env.folder("made"), env.folder("found"));
    connect(&env.git, &made, "git@example.com:me/notes.git").unwrap();
    assert!(known::is_known(&made), "Shale made this .git (SYNC-004)");
    init(&env, &found);
    assert!(!known::is_known(&found));
    connect(&env.git, &found, "git@example.com:me/notes.git").unwrap();
    assert!(known::is_known(&found), "connecting is the person's choice too");
}
