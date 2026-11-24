//! Git sync against real `git` and local bare repositories in temporary folders.

use super::*;
use std::fs;

pub(super) const STAMP: &str = "2026-11-14 1200";

/// A temporary folder with a hermetic git: no user or system configuration, branch `main`.
pub(super) struct Env {
    pub(super) root: PathBuf,
    pub(super) git: Git,
}

impl Env {
    /// A hermetic git without Git LFS (`hide_lfs`), so the tests behave the same on machines with
    /// and without git-lfs (SYNC-016).
    pub(super) fn new(tag: &str) -> Env {
        Env::build(tag, false)
    }

    /// A hermetic git with the real git-lfs, or None (and a printed note) without it (BUILD-018).
    pub(super) fn lfs(tag: &str) -> Option<Env> {
        if Git::default().lfs_version().is_none() {
            eprintln!("skipping: git-lfs is not installed here (CI installs it)");
            return None;
        }
        Some(Env::build(tag, true))
    }

    pub(super) fn build(tag: &str, lfs: bool) -> Env {
        let root = std::env::temp_dir().join(format!("shale-gitsync-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let env = [
            ("GIT_CONFIG_GLOBAL", if cfg!(windows) { "NUL" } else { "/dev/null" }),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            ("GIT_CONFIG_COUNT", "3"),
            ("GIT_CONFIG_KEY_0", "init.defaultBranch"),
            ("GIT_CONFIG_VALUE_0", "main"),
            ("GIT_CONFIG_KEY_1", "protocol.file.allow"),
            ("GIT_CONFIG_VALUE_1", "always"),
            // Git for Windows converts line endings by default; the tests compare exact bytes
            ("GIT_CONFIG_KEY_2", "core.autocrlf"),
            ("GIT_CONFIG_VALUE_2", "false"),
        ];
        let mut git = Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() };
        if !lfs {
            hide_lfs(&mut git);
        }
        Env { root, git }
    }

    /// Run git for test setup; panics on failure.
    pub(super) fn git(&self, dir: &Path, a: &[&str]) -> String {
        let a: Vec<&OsStr> = a.iter().map(OsStr::new).collect();
        let o = self.git.run(dir, &a).unwrap();
        assert!(o.ok, "git {a:?} failed: {}", o.stderr);
        o.text()
    }

    pub(super) fn bare(&self) -> PathBuf {
        let p = self.root.join("remote.git");
        fs::create_dir_all(&p).unwrap();
        self.git(&p, &["init", "--quiet", "--bare"]);
        p
    }

    /// An empty folder (not a repository yet).
    pub(super) fn folder(&self, name: &str) -> PathBuf {
        let p = self.root.join(name);
        fs::create_dir_all(&p).unwrap();
        p
    }

    /// A vault connected to `remote` (via the same call the settings use).
    pub(super) fn vault(&self, name: &str, remote: &Path) -> PathBuf {
        let p = self.folder(name);
        connect(&self.git, &p, remote.to_str().unwrap()).unwrap();
        p
    }

    pub(super) fn opts(&self, host: &str) -> Options {
        Options { commit_prefix: String::new(), commit_suffix: String::new(), host: host.into(), stamp: STAMP.into() }
    }

    pub(super) fn sync(&self, vault: &Path) -> Report {
        let tree = TreeLock::default();
        let r = sync(&self.git, vault, &self.opts("test"), &tree).unwrap_or_else(|e| panic!("sync of {} failed: {e}", vault.display()));
        assert!(tree.try_hold().is_some(), "the sync must release the tree lock");
        r
    }

    pub(super) fn try_sync(&self, vault: &Path) -> Result<Report, String> {
        sync(&self.git, vault, &self.opts("test"), &TreeLock::default())
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

pub(super) fn write(vault: &Path, rel: &str, text: &str) {
    let p = vault.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    crate::fsutil::write_atomic(&p, text.as_bytes()).unwrap();
}

pub(super) fn read(vault: &Path, rel: &str) -> String {
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
    // the undo histories stay on the device (EDIT-022)
    write(&v, ".shale/undo/0123456789abcdef.json", "{}\n");
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

// spec: MEDIA-023, SYNC-007
#[test]
fn a_vault_with_outside_links_syncs_only_its_own_files_and_leaves_the_outside_files_alone() {
    let env = Env::new("outside-media");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    // the library folder and a loose file sit next to the vault, never inside it
    let videos = env.folder("Videos");
    write(&videos, "2024/clip.mp4", "outside video");
    let loose = env.folder("loose");
    write(&loose, "pic.png", "outside picture");
    let file_link = crate::media::testdata::file_address(&loose.join("pic.png"));
    let note = format!("# Trip\n\n![[@Videos/2024/clip.mp4]]\n\n![]({file_link})\n");
    write(&a, "Trip.md", &note);
    write(&a, "z_media/2026/11/261122_inside.png", "a copy in the vault");
    let outside_before = (read(&videos, "2024/clip.mp4"), read(&loose, "pic.png"), fs::metadata(videos.join("2024/clip.mp4")).unwrap().modified().unwrap());
    let r = env.sync(&a);
    assert!(r.pushed && r.remote);
    let tracked: Vec<String> = env.git(&a, &["ls-files"]).lines().map(String::from).collect();
    assert_eq!(tracked, ["Trip.md", "z_media/2026/11/261122_inside.png"], "the links are text; the files they name are not in the history");
    assert!(env.git(&remote, &["ls-tree", "-r", "--name-only", "main"]).lines().all(|f| !f.contains("clip.mp4") && !f.contains("pic.png")), "nor on the server");
    // another device gets the note with its links, and not the files
    let b = env.vault("b", &remote);
    env.sync(&b);
    assert_eq!(read(&b, "Trip.md"), note);
    assert!(!b.join("2024").exists() && !b.join("Videos").exists() && !b.join("pic.png").exists() && !b.join("clip.mp4").exists());
    // a change of the note on the second device comes back; the outside files were never touched
    write(&b, "Trip.md", &format!("{note}\nmore\n"));
    env.sync(&b);
    env.sync(&a);
    assert!(read(&a, "Trip.md").ends_with("more\n"));
    assert_eq!(outside_before, (read(&videos, "2024/clip.mp4"), read(&loose, "pic.png"), fs::metadata(videos.join("2024/clip.mp4")).unwrap().modified().unwrap()), "read-only: content and time are as they were");
    assert!(!videos.join(".git").exists() && !loose.join(".git").exists());
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
fn refuses_to_start_during_a_rebase_a_cherry_pick_or_a_revert() {
    let env = Env::new("refuse-ops");
    let v = env.folder("vault");
    env.git(&v, &["init", "--quiet"]);
    write(&v, "Note.md", "base\n");
    env.sync(&v);
    let who = ["-c", "user.name=t", "-c", "user.email=t@t"];
    let commit = |text: &str| {
        write(&v, "Note.md", text);
        env.git(&v, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--quiet", "-am", text]);
    };
    env.git(&v, &["checkout", "--quiet", "-b", "side"]);
    commit("side\n");
    env.git(&v, &["checkout", "--quiet", "main"]);
    commit("main one\n");
    let first = env.git(&v, &["rev-parse", "HEAD"]);
    commit("main two\n");
    let head = env.git(&v, &["rev-parse", "HEAD"]);
    for (start, what) in [(vec!["cherry-pick", "side"], "cherry-pick"), (vec!["revert", "--no-edit", first.as_str()], "revert"), (vec!["rebase", "side"], "rebase")] {
        let mut args = who.to_vec();
        args.extend(start);
        let args: Vec<&std::ffi::OsStr> = args.into_iter().map(std::ffi::OsStr::new).collect();
        let o = env.git.run(&v, &args).unwrap();
        assert!(!o.ok, "{what} should stop on a conflict");
        let before = read(&v, "Note.md");
        let e = env.try_sync(&v).unwrap_err();
        assert!(e.contains(what), "{e}");
        assert_eq!(read(&v, "Note.md"), before, "{what}: the sync changed a file");
        env.git(&v, &[what, "--abort"]);
        assert_eq!(env.git(&v, &["rev-parse", "HEAD"]), head, "{what} aborted: back where it was");
    }
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
    let env = Env::new("reject");
    let remote = env.bare();
    let a = env.vault("a", &remote);
    write(&a, "Note.md", "base\n");
    env.sync(&a);
    let remote_head = env.git(&remote, &["rev-parse", "main"]);
    let hook = remote.join("hooks").join("pre-receive");
    crate::test_exec::write_script(&hook, "#!/bin/sh\necho no pushes today >&2\nexit 1\n");
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
    // an absolute folder path (on Windows that needs a drive)
    let abs = if cfg!(windows) { r"C:\srv\notes.git" } else { "/srv/notes.git" };
    connect(&env.git, &v, abs).unwrap();
    assert!(is_repo(&v));
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some(abs));
    connect(&env.git, &v, " https://example.org/me/notes.git ").unwrap();
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some("https://example.org/me/notes.git"));
    assert!(connect(&env.git, &v, "ext::sh -c touch% /tmp/pwned").is_err());
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some("https://example.org/me/notes.git"));
}

// spec: SYNC-004
#[test]
fn only_plain_remote_urls_are_accepted() {
    for ok in [
        "https://github.com/me/notes.git",
        "https://me@host/x",
        "http://localhost/x",
        "http://127.0.0.1:8080/x",
        "git://[::1]/x",
        "ssh://git@host:22/x.git",
        "file:///srv/x.git",
        "git@github.com:me/notes.git",
        "host:notes",
        if cfg!(windows) { r"C:\srv\git\notes.git" } else { "/srv/git/notes.git" },
    ] {
        assert!(check_url(ok).is_ok(), "{ok}");
    }
    for bad in ["", "-uhelp", "--upload-pack=touch /tmp/x", "ext::sh -c id", "fd::17", "notes", "relative/path", "https://host/a b", "https://host/\nx", "ftp:x/../::y"] {
        assert!(check_url(bad).is_err(), "{bad:?}");
    }
}

// spec: SYNC-004
#[test]
fn a_folder_path_may_have_spaces_an_address_may_not() {
    for ok in [
        // a Unix path is absolute on Unix only; the Windows forms below are checked on every system
        if cfg!(windows) { r"C:\Users\Ada\Dropbox\My notes.git" } else { "/Users/ada/Library/CloudStorage/Dropbox/My notes.git" },
        if cfg!(windows) { r"  C:\srv\my notes\x.git  " } else { "  /srv/my notes/x.git  " },
        r"C:\Users\Ada\OneDrive - Company\notes.git",
        "C:/Users/Ada/OneDrive - Company/notes.git",
        r"\\server\my share\notes.git",
        "file:///srv/my%20notes/x.git",
    ] {
        assert!(check_url(ok).is_ok(), "{ok}");
    }
    for bad in [
        "https://host/my notes.git",
        "https://host /x.git",
        "ssh://git@host/my notes.git",
        "git@host:my notes.git",
        "host:my notes",
        "file:///srv/my notes/x.git",
        "ext::sh -c id /srv/a b",
        "/srv/my\nnotes.git",
        "/srv/my\tnotes.git",
        "/srv/a\0b",
        "C:\\a\u{7}b",
        "- /srv/x y",
        "-C /srv/x y",
        "--upload-pack=x /srv/a b",
    ] {
        assert!(check_url(bad).is_err(), "{bad:?}");
    }
    assert!(check_url("https://host/my notes.git").unwrap_err().contains("spaces"));
}

// spec: SYNC-004
#[test]
fn connect_hands_a_folder_with_a_space_to_git_as_one_argument() {
    let env = Env::new("connect-space");
    let v = env.folder("v");
    let spaced = env.root.join("my notes; touch pwned").join("remote.git");
    std::fs::create_dir_all(&spaced).unwrap();
    let spaced = spaced.to_string_lossy().into_owned();
    connect(&env.git, &v, &spaced).unwrap();
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some(spaced.as_str()));
    assert!(!env.root.join("pwned").exists() && !v.join("pwned").exists());
    // a Windows path is kept as typed
    let win = r"C:\Users\Ada\OneDrive - Company\notes.git";
    connect(&env.git, &v, win).unwrap();
    assert_eq!(remote_url(&env.git, &v).as_deref(), Some(win));
}

// spec: SYNC-004
#[test]
fn a_vault_syncs_with_a_remote_in_a_folder_with_a_space() {
    let env = Env::new("sync-space");
    let remote = env.root.join("my notes").join("remote.git");
    std::fs::create_dir_all(&remote).unwrap();
    assert!(env.git.ok(&remote, args!["init", "--quiet", "--bare"]).is_ok());
    let v = env.folder("v");
    connect(&env.git, &v, remote.to_str().unwrap()).unwrap();
    write(&v, "First.md", "x\n");
    assert!(env.sync(&v).pushed);
}

// spec: SEC-010
#[test]
fn an_address_with_a_password_or_without_encryption_is_refused() {
    for bad in ["https://user:token@host/x.git", "ssh://git:secret@host/x", "https://:token@host/x", "user:secret@host:notes.git"] {
        let e = check_url(bad).unwrap_err();
        assert!(e.contains("password") && e.contains("credential helper"), "{bad}: {e}");
    }
    for bad in ["http://host/x", "git://example.com/x", "HTTP://host/x", "http://localhost.evil.com/x", "http://localhost@evil.com/x"] {
        let e = check_url(bad).unwrap_err();
        assert!(e.contains("unencrypted"), "{bad}: {e}");
    }
    // a user name alone is fine
    assert!(check_url("ssh://git@host/x.git").is_ok());
    assert!(check_url("https://user@host/x.git").is_ok());
}

// spec: SEC-010
#[test]
fn a_password_in_an_existing_remote_is_hidden_when_shown() {
    assert_eq!(hide_passwords("https://user:tok3n@host/x.git"), "https://user:***@host/x.git");
    assert_eq!(hide_passwords("fatal: unable to access 'https://u:p@h:8443/a/b.git/': Could not resolve host"), "fatal: unable to access 'https://u:***@h:8443/a/b.git/': Could not resolve host");
    assert_eq!(hide_passwords("https://user@host/x.git and /srv/x.git"), "https://user@host/x.git and /srv/x.git");
    assert!(!hide_passwords("a https://x:y@h/ b ssh://q:r@s/").contains(":y@"));
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
    // stdin is closed: a git that reads it gets nothing (the hash of the empty blob)
    let o = Git::default().run(Path::new("."), args!["hash-object", "--stdin"]).unwrap();
    assert_eq!(o.text(), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391", "{}", o.stderr);
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

// spec: BUILD-018
#[test]
fn the_computer_has_a_name_for_commit_messages() {
    let h = host_name();
    assert!(!h.is_empty() && !h.contains('\n') && h == h.trim(), "{h:?}");
}

/// 70 bytes that are no text and no pointer.
fn media(seed: u8) -> Vec<u8> {
    (0..70u8).map(|i| i.wrapping_mul(7).wrapping_add(seed) | 0x80).collect()
}

/// The LFS object file in a bare remote's store.
fn remote_object(remote: &Path, oid: &str) -> PathBuf {
    remote.join("lfs").join("objects").join(&oid[..2]).join(&oid[2..4]).join(oid)
}

/// The pointer stored in a commit for a path, None if the blob is not a pointer.
fn stored_pointer(env: &Env, vault: &Path, rev_path: &str) -> Option<lfs::Pointer> {
    let o = env.git.run(vault, args!["cat-file", "blob", rev_path]).unwrap();
    assert!(o.ok, "{rev_path}: {}", o.stderr);
    lfs::parse_pointer(&o.stdout)
}

// spec: SYNC-017
#[test]
fn shales_block_keeps_the_users_lines_and_is_written_the_same_every_time() {
    let block = lfs::with_block("").unwrap();
    assert!(block.starts_with("# shale-media-begin\n") && block.ends_with("# shale-media-end\n"));
    assert_eq!(block.lines().count(), 2 + 2 * crate::media::media_extensions().count());
    for e in crate::media::media_extensions() {
        assert!(block.contains(&format!("\n*.{e} filter=lfs diff=lfs merge=lfs -text\n")), "{e}");
        assert!(block.contains(&format!("\n*.{} filter=lfs diff=lfs merge=lfs -text\n", e.to_ascii_uppercase())), "{e}");
    }
    let mine = "# mine\n*.md text eol=lf\n";
    let once = lfs::with_block(mine).unwrap();
    assert!(once.starts_with(mine) && once.ends_with(&block));
    assert_eq!(lfs::with_block(&once).unwrap(), once, "writing it again changes nothing");
    // lines before and after the block, and a last line without a newline, are kept
    let both = format!("first\n{block}last");
    assert_eq!(lfs::with_block(&both).unwrap(), both);
    assert_eq!(lfs::with_block("no newline").unwrap(), format!("no newline\n\n{block}"));
    // a block without its end is not guessed at
    assert!(lfs::with_block("# shale-media-begin\n*.mp3 filter=lfs\n").unwrap_err().contains("shale-media-end"));
    assert!(lfs::has_block(&once) && !lfs::has_block(mine));
    assert!(lfs::uses_lfs(&once) && !lfs::uses_lfs(mine) && !lfs::uses_lfs("# *.mp3 filter=lfs\n"));
}

// spec: SYNC-017, MEDIA-001
#[test]
fn the_lfs_patterns_cover_exactly_the_extensions_that_have_a_player() {
    // one list (#206): every pattern is a media file that gets a player, in lower and upper case ...
    let patterns = lfs::patterns();
    for p in &patterns {
        let name = p.replace('*', "file");
        assert!(crate::media::is_media(Path::new(&name)), "{p} has no player");
    }
    // ... and every extension with a player is routed to LFS, in both cases
    let mut want: Vec<String> = Vec::new();
    for e in ["mp3", "ogg", "oga", "opus", "flac", "m4a", "aac", "wav", "mp4", "m4v", "mov", "webm", "mkv", "avi", "ogv"] {
        assert!(crate::media::is_media(Path::new(&format!("x.{e}"))), "{e}");
        want.push(format!("*.{e}"));
        want.push(format!("*.{}", e.to_ascii_uppercase()));
    }
    let mut got = patterns;
    got.sort();
    want.sort();
    assert_eq!(got, want);
}

// spec: SYNC-022
#[test]
fn pointer_files_are_recognised_and_other_files_are_not() {
    let oid = "4d7a214614ab2935c943f9e0ff69d22eadbb8f32b1258daaa5e2ca24d17e2393";
    let text = format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 12345\n");
    assert_eq!(lfs::parse_pointer(text.as_bytes()), Some(lfs::Pointer { oid: oid.into(), size: 12345 }));
    assert_eq!(lfs::parse_pointer(&media(1)), None);
    assert_eq!(lfs::parse_pointer(b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 1\n"), None);
    assert_eq!(lfs::parse_pointer(b"just a note\n"), None);
    assert_eq!(lfs::parse_pointer(&vec![b'x'; 2000]), None);
}

// spec: SYNC-016
#[test]
fn a_missing_git_lfs_is_reported_and_nothing_changes() {
    let env = Env::new("lfs-missing");
    assert_eq!(env.git.lfs_version(), None);
    let v = env.folder("v");
    env.git(&v, &["init", "--quiet"]);
    assert_eq!(lfs::status(&env.git, &v), lfs::Status::default());
    write(&v, "Song.mp3", "raw");
    let e = lfs::setup(&env.git, &v, &env.opts("x"), &TreeLock::default()).unwrap_err();
    assert!(e.contains("not installed") && e.contains("git-lfs.com"), "{e}");
    assert!(!v.join(".gitattributes").exists(), "nothing was written");
    // a new repository made by connect stays as it was without git-lfs
    let remote = env.bare();
    let w = env.vault("w", &remote);
    assert!(!w.join(".gitattributes").exists());
    assert!(!lfs::configured(&env.git, &w));
}

// spec: SYNC-019
#[test]
fn without_git_lfs_a_repository_that_uses_it_commits_nothing() {
    let env = Env::new("lfs-guard");
    let v = env.folder("v");
    env.git(&v, &["init", "--quiet"]);
    write(&v, "Note.md", "a note\n");
    env.sync(&v);
    let head = env.git(&v, &["rev-parse", "HEAD"]);
    // another machine set LFS up: its lines are here, this computer has no git-lfs
    write(&v, ".gitattributes", "# mine\n*.mp3 filter=lfs diff=lfs merge=lfs -text\n");
    write(&v, "Song.mp3", "raw media");
    let e = env.try_sync(&v).unwrap_err();
    assert!(e.contains("Git LFS is not installed") && e.contains("Nothing was committed"), "{e}");
    assert_eq!(env.git(&v, &["rev-parse", "HEAD"]), head, "no commit");
    assert_eq!(env.git(&v, &["diff", "--cached", "--name-only"]), "", "nothing staged");
    assert!(env.git(&v, &["ls-files"]).lines().all(|f| f != "Song.mp3"));
    // without LFS lines everything is as it always was
    fs::remove_file(v.join(".gitattributes")).unwrap();
    let r = env.sync(&v);
    assert_eq!(r.committed, 1, "Song.mp3 is an ordinary file in a repository that does not use LFS");
}

// spec: SYNC-017, SYNC-018, SYNC-005
#[test]
fn storing_media_with_lfs_converts_what_is_tracked_in_one_commit_and_rewrites_nothing() {
    let Some(env) = Env::lfs("lfs-setup") else { return };
    let v = env.folder("v");
    env.git(&v, &["init", "--quiet"]);
    let song = media(3);
    write(&v, "Note.md", "a note\n");
    write(&v, ".gitattributes", "# mine\n*.md text eol=lf\n");
    fs::create_dir_all(v.join("z_media")).unwrap();
    crate::fsutil::write_atomic(&v.join("z_media/Song.mp3"), &song).unwrap();
    crate::fsutil::write_atomic(&v.join("Clip.MOV"), &media(4)).unwrap();
    env.sync(&v);
    let before = env.git(&v, &["rev-parse", "HEAD"]);
    assert!(stored_pointer(&env, &v, "HEAD:z_media/Song.mp3").is_none(), "tracked as a plain file so far");

    let opts = Options { commit_prefix: "[n] ".into(), commit_suffix: " #a".into(), ..env.opts("t") };
    let tree = TreeLock::default();
    // SYNC-005: not while a merge runs
    write(&v, ".git/MERGE_HEAD", &format!("{before}\n"));
    let e = lfs::setup(&env.git, &v, &opts, &tree).unwrap_err();
    assert!(e.contains("merge"), "{e}");
    assert!(!lfs::configured(&env.git, &v), "nothing set up");
    fs::remove_file(v.join(".git/MERGE_HEAD")).unwrap();

    assert_eq!(lfs::setup(&env.git, &v, &opts, &tree).unwrap(), 3, ".gitattributes and the two media files");
    assert!(tree.try_hold().is_some(), "the tree lock is released");
    assert_eq!(env.git(&v, &["log", "-1", "--format=%s"]), "[n] Store media files with Git LFS #a");
    assert_eq!(env.git(&v, &["rev-list", "--count", "HEAD"]), "2", "history is not rewritten");
    assert_eq!(env.git(&v, &["rev-parse", "HEAD^"]), before);
    assert_eq!(stored_pointer(&env, &v, "HEAD:z_media/Song.mp3").unwrap().size, 70);
    assert!(stored_pointer(&env, &v, "HEAD:Clip.MOV").is_some());
    assert_eq!(crate::fsutil::read(v.join("z_media/Song.mp3")).unwrap(), song, "the work tree keeps the real file");
    let attrs = read(&v, ".gitattributes");
    assert!(attrs.starts_with("# mine\n*.md text eol=lf\n") && attrs.contains("# shale-media-begin\n"), "{attrs}");
    assert!(env.git(&v, &["config", "--local", "--get", "filter.lfs.smudge"]).contains("--skip"), "pointers stay pointers on checkout");
    assert!(lfs::status(&env.git, &v).stores_media);
    assert!(env.git.run(&v, args!["config", "--global", "--list"]).unwrap().text().is_empty(), "the global config is never touched");

    // again: nothing to do, no commit
    assert_eq!(lfs::setup(&env.git, &v, &opts, &tree).unwrap(), 0);
    assert_eq!(env.git(&v, &["rev-list", "--count", "HEAD"]), "2");
    assert_eq!(read(&v, ".gitattributes"), attrs);
}

// spec: SYNC-017
#[test]
fn connect_sets_a_new_repository_up_for_lfs_and_leaves_an_existing_one_alone() {
    let Some(env) = Env::lfs("lfs-connect") else { return };
    let remote = env.bare();
    let v = env.vault("new", &remote);
    assert!(lfs::status(&env.git, &v).stores_media);
    assert!(read(&v, ".gitattributes").contains("*.mp3 filter=lfs"));
    assert!(!env.git.run(&v, args!["rev-parse", "--verify", "--quiet", "HEAD"]).unwrap().ok, "nothing is committed by connect");
    let old = env.folder("old");
    env.git(&old, &["init", "--quiet"]);
    connect(&env.git, &old, remote.to_str().unwrap()).unwrap();
    assert!(!old.join(".gitattributes").exists() && !lfs::configured(&env.git, &old));
}

// spec: SYNC-021, SYNC-017
#[test]
fn the_sync_uploads_the_lfs_files_and_another_computer_downloads_them_on_demand() {
    let Some(env) = Env::lfs("lfs-push") else { return };
    let remote = env.bare();
    let a = env.vault("a", &remote);
    let song = media(5);
    crate::fsutil::write_atomic(&a.join("Song.mp3"), &song).unwrap();
    write(&a, "Note.md", "a note\n");
    let r = env.sync(&a);
    assert!(r.pushed);
    let p = stored_pointer(&env, &a, "HEAD:Song.mp3").expect("committed as a pointer");
    assert!(remote_object(&remote, &p.oid).is_file(), "the file reached the remote's LFS store");
    assert!(stored_pointer(&env, &remote, "main:Song.mp3").is_some(), "the history holds only the pointer");

    // another computer: pointer files (no smudge), the real file when asked for
    let b = env.vault("b", &remote);
    env.sync(&b);
    assert_eq!(lfs::parse_pointer(&crate::fsutil::read(b.join("Song.mp3")).unwrap()), Some(p));
    assert_eq!(read(&b, "Note.md"), "a note\n");
    env.git(&b, &["lfs", "pull", "--include=Song.mp3", "--exclude="]);
    assert_eq!(crate::fsutil::read(b.join("Song.mp3")).unwrap(), song);
    // the pulled file is no change for the next sync
    assert_eq!(env.sync(&b).committed, 0);
}

// spec: SYNC-019
#[test]
fn a_merge_that_brings_in_lfs_lines_sets_this_repository_up() {
    let Some(env) = Env::lfs("lfs-merge") else { return };
    let remote = env.bare();
    let a = env.vault("a", &remote);
    crate::fsutil::write_atomic(&a.join("Song.mp3"), &media(6)).unwrap();
    env.sync(&a);
    // an older repository without LFS, with notes of its own
    let b = env.folder("b");
    env.git(&b, &["init", "--quiet"]);
    connect(&env.git, &b, remote.to_str().unwrap()).unwrap();
    write(&b, "Mine.md", "mine\n");
    assert!(!lfs::configured(&env.git, &b));
    let r = env.sync(&b);
    assert!(r.pushed);
    assert!(lfs::configured(&env.git, &b), "set up after the merge");
    assert!(env.git(&b, &["config", "--local", "--get", "filter.lfs.smudge"]).contains("--skip"));
    assert!(lfs::parse_pointer(&crate::fsutil::read(b.join("Song.mp3")).unwrap()).is_some());
    assert_eq!(read(&b, "Mine.md"), "mine\n");
    // media added here from now on goes to LFS
    crate::fsutil::write_atomic(&b.join("Two.ogg"), &media(7)).unwrap();
    env.sync(&b);
    assert!(stored_pointer(&env, &b, "HEAD:Two.ogg").is_some());
}

// spec: SYNC-022, SYNC-008
#[test]
fn the_conflict_copy_of_an_lfs_file_is_stored_in_lfs_too() {
    let Some(env) = Env::lfs("lfs-copy") else { return };
    let remote = env.bare();
    let (a, b) = (env.vault("a", &remote), env.vault("b", &remote));
    crate::fsutil::write_atomic(&a.join("Song.mp3"), &media(1)).unwrap();
    env.sync(&a);
    env.sync(&b);
    // both change the file
    let (theirs, ours) = (media(2), media(3));
    crate::fsutil::write_atomic(&a.join("Song.mp3"), &theirs).unwrap();
    env.sync(&a);
    crate::fsutil::write_atomic(&b.join("Song.mp3"), &ours).unwrap();
    let r = env.sync(&b);
    assert_eq!(r.conflicts.len(), 1, "{r:?}");
    let copy = r.conflicts[0].1.clone();
    assert_eq!(copy, PathBuf::from("Song (conflict 2026-11-14 1200).mp3"));
    // the remote's version stays at the path (a pointer here, its file was never downloaded);
    // the local version, whose file is here, is a real file again under the copy's name
    assert!(lfs::parse_pointer(&crate::fsutil::read(b.join("Song.mp3")).unwrap()).is_some());
    assert_eq!(crate::fsutil::read(b.join(&copy)).unwrap(), ours);
    // both are in LFS: pointers in the history, files in the remote's store
    let copy_name = copy.to_string_lossy().into_owned();
    let p = stored_pointer(&env, &b, &format!("HEAD:{copy_name}")).expect("the copy is a pointer in the history");
    assert!(stored_pointer(&env, &b, "HEAD:Song.mp3").is_some());
    assert!(r.pushed && remote_object(&remote, &p.oid).is_file());
    assert_eq!(env.git(&b, &["status", "--porcelain"]), "", "nothing is left over");
    // the other computer gets the copy and can download it
    env.sync(&a);
    assert!(lfs::parse_pointer(&crate::fsutil::read(a.join(&copy)).unwrap()).is_some());
    env.git(&a, &["lfs", "pull", &format!("--include={copy_name}"), "--exclude="]);
    assert_eq!(crate::fsutil::read(a.join(&copy)).unwrap(), ours);
}

// spec: SYNC-020, VAULT-018
#[test]
fn the_download_asks_for_one_file_and_the_path_stays_in_the_vault() {
    let args = |p: &str| lfs::pull_args(Path::new(p)).map(|a| a.iter().map(|s| s.to_string_lossy().into_owned()).collect::<Vec<_>>());
    assert_eq!(args("Music/Song.mp3").unwrap(), ["lfs", "pull", "--include=Music/Song.mp3", "--exclude="]);
    assert_eq!(args("./a/./b.ogg").unwrap()[2], "--include=a/b.ogg");
    // glob characters match themselves, they do not widen the include
    assert_eq!(args("Live [2024] *.mp3").unwrap()[2], "--include=Live ?2024? ?.mp3");
    for bad in ["../Outside.mp3", "a/../../x.mp3", "/etc/passwd", "", "."] {
        assert!(args(bad).is_err(), "{bad}");
    }
    assert!(args("../x").unwrap_err().contains("not a file in the vault"));
    // `always`: only when the vault says so and the repository uses LFS
    let dir = std::env::temp_dir().join(format!("shale-pull-after-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    use crate::vault_state::LfsDownload::{Always, OnDemand};
    assert!(!lfs::pull_after_sync(&Always, &dir), "no LFS lines, nothing to pull");
    write(&dir, ".gitattributes", "*.mp3 filter=lfs diff=lfs merge=lfs -text\n");
    assert!(lfs::pull_after_sync(&Always, &dir));
    assert!(!lfs::pull_after_sync(&OnDemand, &dir), "on demand is the default");
    let _ = fs::remove_dir_all(&dir);
}

// spec: SYNC-020
#[test]
fn a_download_without_git_lfs_or_outside_the_vault_says_why_and_changes_nothing() {
    let env = Env::new("lfs-dl-missing");
    let remote = env.bare();
    let v = env.vault("v", &remote);
    let pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc\nsize 5\n";
    write(&v, "Song.mp3", pointer);
    let e = lfs::download(&env.git, &v, Path::new("Song.mp3"), &TreeLock::default()).unwrap_err();
    assert!(e.contains("not installed") && e.contains("git-lfs.com"), "{e}");
    assert_eq!(read(&v, "Song.mp3"), pointer);
    let e = lfs::download(&env.git, &v, Path::new("../x.mp3"), &TreeLock::default()).unwrap_err();
    assert!(e.contains("not a file in the vault"), "{e}");
    let plain = env.folder("plain");
    assert!(lfs::download(&env.git, &plain, Path::new("Song.mp3"), &TreeLock::default()).unwrap_err().contains("not a git repository"));
    assert!(lfs::pull_all(&env.git, &v, &TreeLock::default()).unwrap_err().contains("not installed"));
}

// spec: SYNC-020
#[test]
fn a_pointer_is_downloaded_on_demand_from_the_remote_and_only_that_file() {
    let Some(env) = Env::lfs("lfs-download") else { return };
    let remote = env.bare();
    let a = env.vault("a", &remote);
    let (one, two) = (media(11), media(12));
    crate::fsutil::write_atomic(&a.join("One.mp3"), &one).unwrap();
    crate::fsutil::write_atomic(&a.join("Two.mp3"), &two).unwrap();
    assert!(env.sync(&a).pushed);
    // another computer: pointers only
    let b = env.vault("b", &remote);
    env.sync(&b);
    assert!(lfs::parse_pointer(&crate::fsutil::read(b.join("One.mp3")).unwrap()).is_some());
    let tree = TreeLock::default();
    lfs::download(&env.git, &b, Path::new("One.mp3"), &tree).unwrap();
    assert_eq!(crate::fsutil::read(b.join("One.mp3")).unwrap(), one);
    assert!(lfs::parse_pointer(&crate::fsutil::read(b.join("Two.mp3")).unwrap()).is_some(), "only the file asked for");
    assert!(tree.try_hold().is_some(), "the tree lock is free again");
    assert_eq!(env.sync(&b).committed, 0, "the downloaded file is no change");
    // the whole repository, as the setting `always` does after a sync
    lfs::pull_all(&env.git, &b, &tree).unwrap();
    assert_eq!(crate::fsutil::read(b.join("Two.mp3")).unwrap(), two);
    // a file whose upload never happened is an error in words, not a silent pointer
    let ghost = "version https://git-lfs.github.com/spec/v1\noid sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd\nsize 5\n";
    write(&b, "Ghost.mp3", ghost);
    let e = lfs::download(&env.git, &b, Path::new("Ghost.mp3"), &tree).unwrap_err();
    assert!(!e.is_empty(), "{e}");
}

// spec: DEV-026, DEV-029
#[test]
fn dev_029_dataflow_of_a_first_sync_a_pull_and_an_incremental_sync_with_lfs() {
    let Some(env) = Env::lfs("dataflow-sync") else { return };
    let remote = env.bare();
    let a = env.vault("a", &remote);
    let b = env.vault("b", &remote);
    crate::fsutil::write_atomic(&a.join("Song.mp3"), &media(21)).unwrap();
    write(&a, "Note.md", "from a\n");
    let flow = crate::dataflow::record(|| {
        // a's first sync commits and pushes, b's first sync fast-forwards from the remote
        assert!(env.sync(&a).pushed);
        env.sync(&b);
        // b edits and pushes, a pulls it, and a sync with nothing new changes nothing
        write(&b, "Note.md", "from b\n");
        env.sync(&b);
        env.sync(&a);
        assert_eq!(env.sync(&a).committed, 0);
    });
    crate::dataflow::verify(&Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/git-sync.md"), "git_sync", &crate::dataflow::render_mermaid(&flow));
}

// spec: DEV-026, DEV-029
#[test]
fn dev_029_dataflow_of_an_lfs_download_on_demand() {
    let Some(env) = Env::lfs("dataflow-lfs-dl") else { return };
    let remote = env.bare();
    let a = env.vault("a", &remote);
    crate::fsutil::write_atomic(&a.join("One.mp3"), &media(22)).unwrap();
    assert!(env.sync(&a).pushed);
    let b = env.vault("b", &remote);
    env.sync(&b);
    let tree = TreeLock::default();
    let flow = crate::dataflow::record(|| lfs::download(&env.git, &b, Path::new("One.mp3"), &tree).unwrap());
    crate::dataflow::verify(&Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/git-lfs-download.md"), "git_lfs_download", &crate::dataflow::render_mermaid(&flow));
}
