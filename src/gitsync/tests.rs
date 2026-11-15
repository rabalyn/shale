//! Git sync against real `git` and local bare repositories in temporary folders.

use super::*;
use std::fs;

const STAMP: &str = "2026-11-14 1200";

/// A temporary folder with a hermetic git: no user or system configuration, branch `main`.
struct Env {
    root: PathBuf,
    git: Git,
}

impl Env {
    fn new(tag: &str) -> Env {
        let root = std::env::temp_dir().join(format!("shale-gitsync-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let env = [
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_COUNT", "2"),
            ("GIT_CONFIG_KEY_0", "init.defaultBranch"),
            ("GIT_CONFIG_VALUE_0", "main"),
            ("GIT_CONFIG_KEY_1", "protocol.file.allow"),
            ("GIT_CONFIG_VALUE_1", "always"),
        ];
        let git = Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() };
        Env { root, git }
    }

    /// Run git for test setup; panics on failure.
    fn git(&self, dir: &Path, a: &[&str]) -> String {
        let a: Vec<&OsStr> = a.iter().map(OsStr::new).collect();
        let o = self.git.run(dir, &a).unwrap();
        assert!(o.ok, "git {a:?} failed: {}", o.stderr);
        o.text()
    }

    fn bare(&self) -> PathBuf {
        let p = self.root.join("remote.git");
        fs::create_dir_all(&p).unwrap();
        self.git(&p, &["init", "--quiet", "--bare"]);
        p
    }

    /// An empty folder (not a repository yet).
    fn folder(&self, name: &str) -> PathBuf {
        let p = self.root.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// A vault connected to `remote` (via the same call the settings use).
    fn vault(&self, name: &str, remote: &Path) -> PathBuf {
        let p = self.folder(name);
        connect(&self.git, &p, remote.to_str().unwrap()).unwrap();
        p
    }

    fn opts(&self, host: &str) -> Options {
        Options { commit_prefix: String::new(), commit_suffix: String::new(), host: host.into(), stamp: STAMP.into() }
    }

    fn sync(&self, vault: &Path) -> Report {
        let tree = TreeLock::default();
        let r = sync(&self.git, vault, &self.opts("test"), &tree).unwrap_or_else(|e| panic!("sync of {} failed: {e}", vault.display()));
        assert!(tree.try_hold().is_some(), "the sync must release the tree lock");
        r
    }

    fn try_sync(&self, vault: &Path) -> Result<Report, String> {
        sync(&self.git, vault, &self.opts("test"), &TreeLock::default())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn write(vault: &Path, rel: &str, text: &str) {
    let p = vault.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    crate::fsutil::write_atomic(&p, text.as_bytes()).unwrap();
}

fn read(vault: &Path, rel: &str) -> String {
    crate::fsutil::read_to_string(vault.join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

/// No file in the working tree (outside .git) contains conflict markers.
fn assert_no_markers(dir: &Path) {
    for e in fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        if e.file_name() == ".git" {
            continue;
        }
        if e.file_type().unwrap().is_dir() {
            assert_no_markers(&e.path());
        } else if let Ok(t) = crate::fsutil::read_to_string(e.path()) {
            assert!(!t.contains("<<<<<<<") && !t.contains(">>>>>>>"), "conflict markers in {}", e.path().display());
        }
    }
}

// spec: SYNC-006, SYNC-002
#[test]
fn commits_everything_with_prefix_and_suffix_but_not_temp_files() {
    let env = Env::new("commit");
    let v = env.folder("vault");
    env.git(&v, &["init", "--quiet"]);
    write(&v, "Note.md", "hello\n");
    write(&v, ".shale/settings.json", "{}\n");
    write(&v, "sub/.shale-tmp-1-2-3", "half written");
    write(&v, ".shale-tmp-4-5-6", "half written");
    let opts = Options { commit_prefix: "[notes] ".into(), commit_suffix: " #auto".into(), host: "laptop".into(), stamp: STAMP.into() };
    let r = sync(&env.git, &v, &opts, &TreeLock::default()).unwrap();
    assert_eq!(r.committed, 2);
    assert!(!r.remote);
    assert_eq!(env.git(&v, &["log", "-1", "--format=%s"]), "[notes] Sync from laptop: 2 file(s) #auto");
    assert_eq!(env.git(&v, &["log", "-1", "--format=%an <%ae>"]), "Shale <shale@localhost>");
    let files = env.git(&v, &["ls-files"]);
    assert_eq!(files.lines().collect::<Vec<_>>(), vec![".shale/settings.json", "Note.md"]);
    // nothing changed: no new commit
    let r = env.sync(&v);
    assert_eq!(r.committed, 0);
    assert_eq!(env.git(&v, &["rev-list", "--count", "HEAD"]), "1");
    assert!(r.summary().contains("no remote"));
}

// spec: SYNC-007
#[test]
fn two_vaults_sync_through_a_bare_remote() {
    let env = Env::new("two");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, "Note.md", "from a\n");
    write(&a, ".shale/settings.json", "{\"people_dir\": \"Leute\"}\n");
    let r = env.sync(&a);
    assert!(r.pushed && r.remote);
    assert_eq!(env.git(&remote, &["rev-parse", "main"]), env.git(&a, &["rev-parse", "HEAD"]));

    // a second machine starts from an empty folder
    let b = env.vault("b", &remote);
    let r = env.sync(&b);
    assert_eq!(read(&b, "Note.md"), "from a\n");
    assert_eq!(read(&b, ".shale/settings.json"), "{\"people_dir\": \"Leute\"}\n");
    assert!(r.changed.contains(&PathBuf::from("Note.md")));
    assert!(r.changed.contains(&PathBuf::from(".shale/settings.json")));

    // b edits, a fast-forwards
    write(&b, "Note.md", "from b\n");
    write(&b, "New.md", "new\n");
    env.sync(&b);
    let r = env.sync(&a);
    assert_eq!(read(&a, "Note.md"), "from b\n");
    assert_eq!(read(&a, "New.md"), "new\n");
    let mut changed = r.changed.clone();
    changed.sort();
    assert_eq!(changed, vec![PathBuf::from("New.md"), PathBuf::from("Note.md")]);
    assert_eq!(env.git(&a, &["rev-parse", "HEAD"]), env.git(&b, &["rev-parse", "HEAD"]));
}

// spec: SYNC-008
#[test]
fn edits_on_both_sides_become_a_conflict_copy() {
    let env = Env::new("conflict");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, "Plans/Note.md", "base\n");
    write(&a, "Plan.excalidraw.md", "base drawing\n");
    env.sync(&a);
    let b = env.vault("b", &remote);
    env.sync(&b);

    write(&a, "Plans/Note.md", "a's version\n");
    write(&a, "Plan.excalidraw.md", "a's drawing\n");
    write(&a, "Only a.md", "a\n");
    env.sync(&a);
    write(&b, "Plans/Note.md", "b's version\n");
    write(&b, "Plan.excalidraw.md", "b's drawing\n");
    write(&b, "Only b.md", "b\n");
    let r = env.sync(&b);

    // the remote version stays, the local one is next to it
    assert_eq!(read(&b, "Plans/Note.md"), "a's version\n");
    assert_eq!(read(&b, "Plans/Note (conflict 2026-11-14 1200).md"), "b's version\n");
    assert_eq!(read(&b, "Plan.excalidraw.md"), "a's drawing\n");
    assert_eq!(read(&b, "Plan (conflict 2026-11-14 1200).excalidraw.md"), "b's drawing\n");
    assert_eq!(read(&b, "Only a.md"), "a\n");
    assert_eq!(read(&b, "Only b.md"), "b\n");
    assert_eq!(r.conflicts.len(), 2, "{r:?}");
    assert!(r.pushed);
    assert_no_markers(&b);
    assert_eq!(env.git(&b, &["status", "--porcelain"]), "");
    // both histories are in the remote
    assert_eq!(env.git(&remote, &["rev-parse", "main"]), env.git(&b, &["rev-parse", "HEAD"]));

    // the first machine gets both versions
    let r = env.sync(&a);
    assert_eq!(read(&a, "Plans/Note (conflict 2026-11-14 1200).md"), "b's version\n");
    assert_eq!(read(&a, "Plans/Note.md"), "a's version\n");
    assert!(r.conflicts.is_empty());
    assert_no_markers(&a);
}

// spec: SYNC-009
#[test]
fn a_file_deleted_on_one_side_and_changed_on_the_other_is_kept() {
    let env = Env::new("delete");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, "Gone there.md", "base\n");
    write(&a, "Gone here.md", "base\n");
    env.sync(&a);
    let b = env.vault("b", &remote);
    env.sync(&b);

    fs::remove_file(a.join("Gone there.md")).unwrap();
    write(&a, "Gone here.md", "changed by a\n");
    env.sync(&a);
    write(&b, "Gone there.md", "changed by b\n");
    fs::remove_file(b.join("Gone here.md")).unwrap();
    env.sync(&b);

    assert_eq!(read(&b, "Gone there.md"), "changed by b\n");
    assert_eq!(read(&b, "Gone here.md"), "changed by a\n");
    env.sync(&a);
    assert_eq!(read(&a, "Gone there.md"), "changed by b\n");
    assert_eq!(read(&a, "Gone here.md"), "changed by a\n");
    assert_no_markers(&a);
}

// spec: SYNC-010
#[test]
fn a_state_conflict_keeps_the_local_state_without_a_copy() {
    let env = Env::new("state");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, STATE_PATH, "{\"open_files\": []}\n");
    env.sync(&a);
    let b = env.vault("b", &remote);
    env.sync(&b);
    write(&a, STATE_PATH, "{\"open_files\": [\"A.md\"]}\n");
    env.sync(&a);
    write(&b, STATE_PATH, "{\"open_files\": [\"B.md\"]}\n");
    let r = env.sync(&b);
    assert_eq!(read(&b, STATE_PATH), "{\"open_files\": [\"B.md\"]}\n");
    assert!(r.conflicts.is_empty());
    let names: Vec<_> = fs::read_dir(b.join(".shale")).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(names.len(), 1, "{names:?}");
}

// spec: SYNC-005
#[test]
fn refuses_to_start_during_a_merge_or_on_a_detached_head() {
    let env = Env::new("refuse");
    let v = env.folder("vault");
    env.git(&v, &["init", "--quiet"]);
    write(&v, "Note.md", "base\n");
    env.sync(&v);
    env.git(&v, &["checkout", "--quiet", "-b", "side"]);
    write(&v, "Note.md", "side\n");
    env.git(&v, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "-am", "side"]);
    env.git(&v, &["checkout", "--quiet", "main"]);
    write(&v, "Note.md", "main\n");
    env.git(&v, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "-am", "main"]);
    let o = env.git.run(&v, args!["-c", "user.name=t", "-c", "user.email=t@t", "merge", "side"]).unwrap();
    assert!(!o.ok);
    let before = read(&v, "Note.md");
    assert!(before.contains("<<<<<<<"));
    let head = env.git(&v, &["rev-parse", "HEAD"]);
    let e = env.try_sync(&v).unwrap_err();
    assert!(e.contains("merge"), "{e}");
    assert_eq!(read(&v, "Note.md"), before);
    assert_eq!(env.git(&v, &["rev-parse", "HEAD"]), head);
    env.git(&v, &["merge", "--abort"]);

    env.git(&v, &["checkout", "--quiet", "--detach"]);
    write(&v, "Note.md", "edited while detached\n");
    let e = env.try_sync(&v).unwrap_err();
    assert!(e.contains("detached"), "{e}");
    assert_eq!(env.git(&v, &["rev-parse", "HEAD"]), head);
    assert_eq!(read(&v, "Note.md"), "edited while detached\n");
}

// spec: SYNC-011
#[cfg(unix)]
#[test]
fn a_conflict_that_cannot_be_resolved_is_undone_and_nothing_is_lost() {
    let env = Env::new("abort");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    std::os::unix::fs::symlink("one", a.join("link")).unwrap();
    write(&a, "Note.md", "base\n");
    env.sync(&a);
    let b = env.vault("b", &remote);
    env.sync(&b);
    fs::remove_file(a.join("link")).unwrap();
    std::os::unix::fs::symlink("two", a.join("link")).unwrap();
    env.sync(&a);
    fs::remove_file(b.join("link")).unwrap();
    std::os::unix::fs::symlink("three", b.join("link")).unwrap();
    write(&b, "Note.md", "b's edit\n");
    let e = env.try_sync(&b).unwrap_err();
    assert!(e.contains("undone"), "{e}");
    assert!(!b.join(env.git(&b, &["rev-parse", "--git-path", "MERGE_HEAD"])).exists());
    assert_eq!(read(&b, "Note.md"), "b's edit\n");
    assert_eq!(fs::read_link(b.join("link")).unwrap(), PathBuf::from("three"));
    assert_eq!(env.git(&b, &["status", "--porcelain"]), "");
    assert_eq!(env.git(&b, &["show", "HEAD:Note.md"]), "b's edit");
}

// spec: SYNC-012
#[cfg(unix)]
#[test]
fn a_rejected_push_is_reported_and_both_histories_stay() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new("reject");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, "Note.md", "base\n");
    env.sync(&a);
    let remote_head = env.git(&remote, &["rev-parse", "main"]);
    let hook = remote.join("hooks").join("pre-receive");
    crate::fsutil::write_atomic(&hook, b"#!/bin/sh\necho no pushes today >&2\nexit 1\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    write(&a, "Note.md", "local change\n");
    let e = env.try_sync(&a).unwrap_err();
    assert!(e.contains("push"), "{e}");
    assert_eq!(env.git(&remote, &["rev-parse", "main"]), remote_head);
    assert_eq!(env.git(&a, &["show", "HEAD:Note.md"]), "local change");
    assert_eq!(env.git(&a, &["rev-parse", "HEAD~1"]), remote_head);
    // the code never asks git to force anything
    let src = include_str!("../gitsync.rs");
    assert!(!src.contains("--force") && !src.contains("\"-f\"") && !src.contains("+refs"));
}

// spec: SYNC-007
#[test]
fn a_vault_with_notes_joins_a_remote_with_notes_and_another_branch_name() {
    let env = Env::new("unrelated");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, "From a.md", "a\n");
    write(&a, "Same.md", "a's\n");
    env.sync(&a);

    // an existing vault, `git init` by hand with branch master, then connected in the settings
    let c = env.folder("c");
    env.git(&c, &["init", "--quiet", "-b", "master"]);
    write(&c, "From c.md", "c\n");
    write(&c, "Same.md", "c's\n");
    connect(&env.git, &c, remote.to_str().unwrap()).unwrap();
    let r = env.sync(&c);
    assert_eq!(env.git(&c, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert_eq!(read(&c, "From a.md"), "a\n");
    assert_eq!(read(&c, "From c.md"), "c\n");
    assert_eq!(read(&c, "Same.md"), "a's\n");
    assert_eq!(read(&c, "Same (conflict 2026-11-14 1200).md"), "c's\n");
    assert_eq!(r.conflicts.len(), 1);
    assert!(env.git(&remote, &["branch", "--list"]).lines().all(|l| l.trim_start_matches(['*', ' ']) == "main"));
    env.sync(&a);
    assert_eq!(read(&a, "From c.md"), "c\n");
}

// spec: SYNC-007
#[test]
fn an_empty_vault_and_an_empty_remote_sync_without_errors() {
    let env = Env::new("empty");
    let remote = env.bare();
    let v = env.vault("v", &remote);
    let r = env.sync(&v);
    assert_eq!(r, Report { remote: true, ..Report::default() });
    write(&v, "First.md", "x\n");
    assert!(env.sync(&v).pushed);
}

// spec: SYNC-004
#[test]
fn connect_creates_the_repository_and_updates_the_url() {
    let env = Env::new("connect");
    let v = env.folder("v");
    assert!(!is_repo(&v));
    connect(&env.git, &v, "/srv/notes.git").unwrap();
    assert!(is_repo(&v));
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some("/srv/notes.git"));
    connect(&env.git, &v, " https://example.org/me/notes.git ").unwrap();
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some("https://example.org/me/notes.git"));
    assert!(connect(&env.git, &v, "ext::sh -c touch% /tmp/pwned").is_err());
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some("https://example.org/me/notes.git"));
}

// spec: SYNC-004
#[test]
fn only_plain_remote_urls_are_accepted() {
    for ok in ["https://github.com/me/notes.git", "http://host/x", "ssh://git@host:22/x.git", "git://host/x", "file:///srv/x.git", "git@github.com:me/notes.git", "host:notes", "/srv/git/notes.git"] {
        assert!(check_url(ok).is_ok(), "{ok}");
    }
    for bad in ["", "-uhelp", "--upload-pack=touch /tmp/x", "ext::sh -c id", "fd::17", "notes", "relative/path", "https://host/a b", "https://host/\nx", "ftp:x/../::y"] {
        assert!(check_url(bad).is_err(), "{bad:?}");
    }
}

// spec: SYNC-003
#[test]
fn without_git_the_sync_reports_it() {
    let env = Env::new("nogit");
    let v = env.folder("v");
    fs::create_dir_all(v.join(".git")).unwrap();
    let missing = Git { program: env.root.join("no-such-git"), env: vec![] };
    assert_eq!(missing.version(), None);
    let e = sync(&missing, &v, &env.opts("x"), &TreeLock::default()).unwrap_err();
    assert!(e.contains("could not be started"), "{e}");
    assert!(env.git.version().unwrap().starts_with("git version"));
}

// spec: SYNC-002
#[test]
fn a_repository_in_a_parent_folder_is_not_used() {
    let env = Env::new("parent");
    let outer = env.folder("outer");
    env.git(&outer, &["init", "--quiet"]);
    let v = env.folder("outer/vault");
    write(&v, "Note.md", "x\n");
    assert!(!is_repo(&v));
    assert!(env.try_sync(&v).is_err());
    assert_eq!(env.git(&outer, &["rev-list", "--all", "--count"]), "0");
}

// spec: SYNC-001
#[test]
fn git_runs_without_a_shell_prompts_or_inherited_repository_variables() {
    let c = Git::default().command(Path::new("/v"), args!["status"]);
    assert_eq!(c.get_program(), "git");
    assert_eq!(c.get_args().collect::<Vec<_>>(), vec!["-C", "/v", "status"]);
    let envs: Vec<_> = c.get_envs().collect();
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] {
        assert!(envs.contains(&(OsStr::new(v), None)), "{v} not removed");
    }
    assert!(envs.contains(&(OsStr::new("GIT_TERMINAL_PROMPT"), Some(OsStr::new("0")))));
}

// spec: SYNC-008
#[test]
fn conflict_copy_names() {
    let none = |_: &Path| false;
    assert_eq!(conflict_name(Path::new("a/Note.md"), STAMP, none), PathBuf::from("a/Note (conflict 2026-11-14 1200).md"));
    assert_eq!(conflict_name(Path::new("Plan.excalidraw.md"), STAMP, none), PathBuf::from("Plan (conflict 2026-11-14 1200).excalidraw.md"));
    assert_eq!(conflict_name(Path::new("v1.2 notes.md"), STAMP, none), PathBuf::from("v1.2 notes (conflict 2026-11-14 1200).md"));
    assert_eq!(conflict_name(Path::new(".gitignore"), STAMP, none), PathBuf::from(".gitignore (conflict 2026-11-14 1200)"));
    let first = PathBuf::from("Note (conflict 2026-11-14 1200).md");
    assert_eq!(conflict_name(Path::new("Note.md"), STAMP, |p| p == first), PathBuf::from("Note (conflict 2026-11-14 1200 2).md"));
}
