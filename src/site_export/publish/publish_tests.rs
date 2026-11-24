//! The publish against real `git` and a bare repository in a temporary folder (SITE-062…SITE-076);
//! the Forgejo suite of `env_tests.rs` does the same against a real server.

use super::*;
use crate::site_export::test_support::Fixture;
use std::ffi::OsString;

/// A vault that publishes to a bare repository, a hermetic git and the folder of the publish repositories.
struct Stack {
    f: Fixture,
    git: Git,
    remote: PathBuf,
    base: PathBuf,
}

const BRANCH: &str = "site";

pub(crate) fn hermetic(lfs: bool) -> Git {
    let none = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let env = [
        ("GIT_CONFIG_GLOBAL", none),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_COUNT", "3"),
        ("GIT_CONFIG_KEY_0", "init.defaultBranch"),
        ("GIT_CONFIG_VALUE_0", "main"),
        ("GIT_CONFIG_KEY_1", "protocol.file.allow"),
        ("GIT_CONFIG_VALUE_1", "always"),
        ("GIT_CONFIG_KEY_2", "core.autocrlf"),
        ("GIT_CONFIG_VALUE_2", "false"),
    ];
    let mut git = Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() };
    if !lfs {
        crate::gitsync::hide_lfs(&mut git);
    }
    git
}

impl Stack {
    /// `lfs`: the real git-lfs, or None (with a printed note) where it is not installed (BUILD-018).
    fn new(tag: &str, lfs: bool, files: &[(&str, &str)]) -> Option<Stack> {
        if lfs && Git::default().lfs_version().is_none() {
            eprintln!("skipping: git-lfs is not installed here (CI installs it)");
            return None;
        }
        let f = Fixture::new(tag);
        let git = hermetic(lfs);
        let remote = f.root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        run(&git, &remote, &["init", "--quiet", "--bare"]);
        let base = f.root.join("data");
        let stack = Stack { f, git, remote, base };
        stack.rules(&format!("\"all_notes\": true, \"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&stack.url()).unwrap()));
        for (path, text) in files {
            stack.f.file(path, text);
        }
        Some(stack)
    }

    fn url(&self) -> String {
        self.remote.to_string_lossy().into_owned()
    }

    fn rules(&self, rules: &str) {
        self.f.publish(rules);
    }

    fn request<'a>(&'a self, device: &'a str, take_over: bool) -> Request<'a> {
        Request { vault: &self.f.dir, libraries: &[], base: &self.base, device, take_over }
    }

    fn publish_as(&self, device: &str, take_over: bool) -> Result<Published, Failure> {
        publish(&self.git, &self.request(device, take_over), &Cancel::default(), &|_| {})
    }

    fn publish(&self) -> Result<Published, Failure> {
        self.publish_as("laptop", false)
    }

    fn remote(&self, args: &[&str]) -> String {
        run(&self.git, &self.remote, args)
    }

    /// The files of the branch on the remote.
    fn remote_files(&self) -> Vec<String> {
        self.remote(&["ls-tree", "-r", "--name-only", BRANCH]).lines().map(String::from).collect()
    }

    fn remote_tip(&self) -> String {
        self.remote(&["rev-parse", &format!("refs/heads/{BRANCH}")])
    }

    fn id(&self) -> String {
        history::vault_id(&self.f.dir).expect("the publish gave the vault an ID")
    }
}

fn run(git: &Git, dir: &Path, args: &[&str]) -> String {
    let a: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let o = git.run(dir, &a).unwrap();
    assert!(o.ok, "git {args:?} failed: {}", o.stderr);
    o.text()
}

// spec: SITE-073, SITE-072
#[test]
fn site_073_the_message_names_the_device_the_time_the_vault_id_and_its_name_and_reads_back() {
    let meta = Meta { device: Some("laptop".into()), time: Some(1_795_000_000), vault_id: Some("ab".repeat(16)), vault_name: Some("My notes".into()) };
    let text = message::compose(3, &meta);
    assert_eq!(text.lines().next(), Some("Publish My notes: 3 notes"));
    assert!(text.contains("\nShale-Device: laptop\n") && text.contains(&format!("\nShale-Vault: {}\n", "ab".repeat(16))) && text.contains("\nShale-Vault-Name: My notes\n"), "{text}");
    assert!(text.contains(&format!("\nShale-Time: {}\n", message::iso(1_795_000_000))));
    assert_eq!(message::parse(&text), meta);
    // one note, and a name or a device that tries to add a line of its own
    assert_eq!(message::compose(1, &Meta { vault_name: Some("A\nShale-Vault: evil".into()), ..Default::default() }).lines().next(), Some("Publish AShale-Vault: evil: 1 note"));
    let sneaky = message::compose(1, &Meta { device: Some("pc\nShale-Vault: ffff".into()), vault_id: Some("1".repeat(32)), ..Default::default() });
    assert_eq!(message::parse(&sneaky).vault_id, Some("1".repeat(32)));
    // a message that no publish made says nothing
    assert_eq!(message::parse("Fix the typo\n\nBody text: with a colon\n"), Meta::default());
    assert_eq!(message::iso(0), "1970-01-01T00:00:00Z");
    assert_eq!(message::parse_iso("2026-11-24T14:05:09Z").map(message::iso), Some("2026-11-24T14:05:09Z".to_string()));
    assert_eq!(message::parse_iso("yesterday"), None);
    assert_eq!(message::parse_iso("2026-11-24T25:00:00Z"), None);
}

// spec: SITE-064
#[test]
fn site_064_the_address_follows_the_rules_of_git_sync_and_a_password_in_it_is_refused() {
    let s = |url: &str, branch: &str| PublishSettings { url: url.into(), branch: branch.into(), ..Default::default() };
    assert_eq!(target(&s("https://git.example.org/me/site.git", "")).unwrap(), ("https://git.example.org/me/site.git".into(), "main".into()));
    assert_eq!(target(&s("git@example.org:me/site.git", " pages ")).unwrap().1, "pages");
    assert_eq!(target(&s("/srv/git/site.git", "main")).unwrap().0, "/srv/git/site.git");
    assert!(target(&s("", "")).unwrap_err().contains("No address"));
    let pw = target(&s("https://me:secret@example.org/site.git", "")).unwrap_err();
    assert!(pw.contains("password") && !pw.contains("secret"), "{pw}");
    assert!(target(&s("http://example.org/site.git", "")).unwrap_err().contains("unencrypted"));
    assert!(target(&s("ext::sh -c touch /tmp/x", "")).is_err());
    assert!(target(&s("-oProxyCommand=x", "")).is_err());
    for bad in ["-x", "a b", "a..b", "a:b", "x/", "x.lock", "a~b", "/x", ".x", ""] {
        assert!(check_branch(bad).is_err(), "{bad:?}");
    }
    assert!(check_branch("release/site-1").is_ok());
}

// spec: SITE-062, SITE-063, SITE-068, SITE-073, SITE-072
#[test]
fn site_068_a_publish_is_one_commit_with_no_parent_and_the_next_one_replaces_it_with_the_notes_gone() {
    let st = Stack::new("snap", false, &[("Open.md", "Open note #pub\n"), ("Secret.md", "---\npublish: false\n---\nthe launch codes\n")]).unwrap();
    let before = crate::site_export::test_support::snapshot(&st.f.dir);
    let first = st.publish().unwrap();
    assert_eq!((first.summary.notes, first.summary.media), (1, 0));
    assert_eq!(first.summary.line(), "Published 1 note, 0 media files and 0 links; 0 links became plain text.");
    // the branch is one commit without a parent, and it is what was made
    assert_eq!(st.remote_tip(), first.commit);
    assert_eq!(st.remote(&["rev-list", "--count", BRANCH]), "1");
    assert_eq!(st.remote(&["rev-list", "--parents", "-1", BRANCH]), first.commit, "no parent");
    let files = st.remote_files();
    assert!(files.contains(&"content/open.md".to_string()) && files.contains(&"hugo.toml".to_string()) && files.iter().any(|f| f.starts_with("themes/shale/")), "{files:?}");
    assert!(!files.iter().any(|f| f.contains("secret") || f.starts_with(".shale") || f == ".shale-export.json"), "{files:?}");
    // the message names the vault, the device and the time (SITE-073, SITE-072)
    let meta = message::parse(&st.remote(&["log", "-1", "--format=%B", BRANCH]));
    assert_eq!((meta.device.as_deref(), meta.vault_id.as_deref(), meta.vault_name.as_deref()), (Some("laptop"), Some(st.id().as_str()), Some("vault")));
    assert!(meta.time.is_some_and(|t| (crate::calendar::time::now() - t).abs() < 600));
    // the repository is of its own below the data folder (SITE-063) and the vault has none
    assert!(st.base.join(st.id()).join("publish/.git").is_dir());
    assert!(!st.f.dir.join(".git").exists(), "the vault is no repository");
    // the vault changed in one file: its ID
    let after = crate::site_export::test_support::snapshot(&st.f.dir);
    let changed: Vec<_> = after.iter().filter(|a| !before.iter().any(|b| b.0 == a.0 && b.1 == a.1)).map(|a| a.0.strip_prefix(&st.f.dir).unwrap().to_string_lossy().replace('\\', "/")).collect();
    assert_eq!(changed, [".shale/history-id"]);

    // a note is unpublished and another is new: the next snapshot has no trace of the first, in the tree or in the history
    st.f.file("Open.md", "---\npublish: false\n---\nOpen note\n");
    st.f.file("Next.md", "The next note\n");
    let second = st.publish_as("desktop", false).unwrap();
    assert_ne!(second.commit, first.commit);
    assert_eq!(st.remote(&["rev-list", "--count", BRANCH]), "1", "still one commit");
    assert!(st.remote_files().contains(&"content/next.md".to_string()) && !st.remote_files().contains(&"content/open.md".to_string()));
    assert!(st.remote(&["log", "--all", "-p", "--format=%H"]).lines().all(|l| !l.contains("Open note")), "the old note is in no commit of the remote");
    assert!(!st.remote(&["rev-list", "--all"]).contains(&first.commit), "the first snapshot is in no branch of the remote");
    assert_eq!(message::parse(&st.remote(&["log", "-1", "--format=%B", BRANCH])).device.as_deref(), Some("desktop"), "the last publish wins (SITE-072)");
}

fn run_fails(st: &Stack, args: &[&str]) -> bool {
    let a: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    !st.git.run(&st.remote, &a).unwrap().ok
}

// spec: SITE-072, SITE-074
#[test]
fn site_074_a_target_of_another_vault_or_of_no_vault_asks_and_a_new_branch_or_its_own_does_not() {
    let st = Stack::new("takeover", false, &[("A.md", "a note\n")]).unwrap();
    // a branch that does not exist yet is taken without asking
    assert!(run_fails(&st, &["rev-parse", "--verify", &format!("refs/heads/{BRANCH}")]));
    let first = st.publish().unwrap();
    // the same vault again: no question
    let again = st.publish().unwrap();
    assert_eq!(st.remote_tip(), again.commit);
    assert_eq!(st.remote(&["rev-list", "--count", BRANCH]), "1");
    let last = last_published(&st.git, &st.f.dir, &st.base, &Cancel::default()).unwrap().unwrap();
    assert!(last.this_vault && last.meta.device.as_deref() == Some("laptop"));

    // another vault with its own ID publishes to the same target
    let other = Stack::new("takeover-other", false, &[("B.md", "another vault\n")]).unwrap();
    let url = st.url();
    other.rules(&format!("\"all_notes\": true, \"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&url).unwrap()));
    other.f.file(".shale/history-id", &format!("{}\n", "cd".repeat(16)));
    let refused = other.publish_as("tablet", false).unwrap_err();
    let Failure::Takeover(t) = &refused else { panic!("{refused:?}") };
    assert_eq!((t.meta.vault_id.as_deref(), t.meta.vault_name.as_deref(), t.meta.device.as_deref()), (Some(st.id().as_str()), Some("vault"), Some("laptop")));
    assert!(t.words().contains("“vault”") && t.words().contains("from laptop at "), "{}", t.words());
    assert!(refused.words().contains("--take-over"));
    assert_eq!(st.remote_tip(), again.commit, "nothing was pushed");
    let seen = last_published(&other.git, &other.f.dir, &other.base, &Cancel::default()).unwrap().unwrap();
    assert!(!seen.this_vault, "the settings say whose it is");
    // confirmed: it is taken over, and the old vault is asked from then on
    let taken = other.publish_as("tablet", true).unwrap();
    assert_eq!(st.remote_tip(), taken.commit);
    assert!(st.remote_files().contains(&"content/b.md".to_string()) && !st.remote_files().contains(&"content/a.md".to_string()));
    assert!(matches!(st.publish(), Err(Failure::Takeover(_))));
    let _ = first;

    // a branch whose last commit no publish made asks as well, and says so
    let foreign = Stack::new("takeover-foreign", false, &[("C.md", "c\n")]).unwrap();
    let work = foreign.f.root.join("work");
    std::fs::create_dir_all(&work).unwrap();
    run(&foreign.git, &work, &["init", "--quiet"]);
    std::fs::write(work.join("README.md"), "somebody's repository\n").unwrap();
    run(&foreign.git, &work, &["add", "-A"]);
    run(&foreign.git, &work, &["-c", "user.name=x", "-c", "user.email=x@x", "commit", "--quiet", "-m", "Add a readme"]);
    run(&foreign.git, &work, &["push", "--quiet", &foreign.url(), &format!("HEAD:refs/heads/{BRANCH}")]);
    let tip = foreign.remote_tip();
    let Failure::Takeover(t) = foreign.publish().unwrap_err() else { panic!("a foreign branch asks") };
    assert_eq!(t.meta, Meta::default());
    assert!(t.words().contains("not made by a publish of Shale"), "{}", t.words());
    assert_eq!(foreign.remote_tip(), tip);
    foreign.publish_as("laptop", true).unwrap();
    assert!(!foreign.remote_files().contains(&"README.md".to_string()));
}

// spec: SITE-065
#[test]
fn site_065_a_cancelled_or_failed_publish_leaves_the_remote_as_it_was() {
    let st = Stack::new("fail", false, &[("A.md", "a note\n")]).unwrap();
    let first = st.publish().unwrap();
    st.f.file("A.md", "a changed note\n");
    // cancelled before it starts, and cancelled when it comes to the push
    let cancel = Cancel::default();
    cancel.cancel();
    assert_eq!(publish(&st.git, &st.request("laptop", false), &cancel, &|_| {}).unwrap_err(), Failure::Cancelled);
    let late = Cancel::default();
    let stop = late.clone();
    let r = publish(&st.git, &st.request("laptop", false), &late, &|step| {
        if step == "Publishing…" {
            stop.cancel();
        }
    });
    assert_eq!(r.unwrap_err(), Failure::Cancelled);
    assert_eq!(st.remote_tip(), first.commit, "the remote was not changed");
    // a remote that refuses the push (a hook that fails): the error says why and the branch is as it was
    #[cfg(unix)]
    {
        crate::test_exec::write_script(&st.remote.join("hooks/pre-receive"), "#!/bin/sh\necho 'no pushes today' >&2\nexit 1\n");
        let e = st.publish().unwrap_err();
        assert!(matches!(&e, Failure::Error(m) if m.contains("no pushes today")), "{e:?}");
        assert_eq!(st.remote_tip(), first.commit);
    }
    // an address that is nowhere
    st.rules(&format!("\"all_notes\": true, \"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&st.f.root.join("nowhere.git").to_string_lossy()).unwrap()));
    assert!(matches!(st.publish(), Err(Failure::Error(_))));
    // a stopped git is stopped: the long step is killed, not waited for
    let slow = Cancel::default();
    let stopper = slow.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        stopper.cancel();
    });
    #[cfg(unix)]
    {
        let mut git = st.git.clone();
        let dir = st.f.root.join("gate");
        std::fs::create_dir_all(&dir).unwrap();
        let _gate = crate::gitsync::hold_git(&mut git, &dir);
        let started = std::time::Instant::now();
        assert_eq!(process::run(&git, &st.f.root, &[OsStr::new("status")], &slow).unwrap_err(), Failure::Cancelled);
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }
    t.join().unwrap();
}

// spec: SITE-051, SITE-064, SITE-066, SITE-063
#[test]
fn site_064_a_publish_needs_the_switch_an_address_and_a_repository_of_its_own() {
    let st = Stack::new("needs", false, &[("A.md", "a note\n")]).unwrap();
    st.f.file(".shale/settings.json", "{\"version\": 1}");
    assert!(matches!(st.publish(), Err(Failure::Error(m)) if m.contains("off for this vault")));
    st.f.file(".shale/settings.json", "{\"version\": 1, \"publish\": {\"enabled\": true, \"all_notes\": true}}");
    assert!(matches!(st.publish(), Err(Failure::Error(m)) if m.contains("No address")));
    // the address the vault syncs with is never a target: a force-push there would replace the notes
    st.rules(&format!("\"all_notes\": true, \"url\": {}", serde_json::to_string(&st.url()).unwrap()));
    run(&st.git, &st.f.dir, &["init", "--quiet"]);
    run(&st.git, &st.f.dir, &["remote", "add", "origin", &format!("{}/", st.url())]);
    let before = st.f.dir.join(".git/config");
    let config = std::fs::read(&before).unwrap();
    assert!(matches!(st.publish(), Err(Failure::Error(m)) if m.contains("syncs with")));
    assert_eq!(std::fs::read(&before).unwrap(), config, "the vault's repository is never touched");
    assert!(st.remote(&["for-each-ref"]).is_empty(), "nothing was pushed");
}

// spec: SITE-036, SITE-067
#[test]
fn site_067_the_summary_counts_and_lists_every_warning_among_them_the_off_vault_files_that_did_not_resolve() {
    let st = Stack::new("summary", false, &[("A.md", "![[@Gone/x.png]] and [[B]] and [[Missing]] ![[pic.png]]\n"), ("B.md", "b\n")]).unwrap();
    st.f.bytes("pic.png", b"png");
    let done = st.publish().unwrap();
    let s = &done.summary;
    assert_eq!((s.notes, s.media), (2, 1));
    assert_eq!(s.line(), "Published 2 notes, 1 media file and 1 link; 2 links became plain text.");
    assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
    assert!(s.warnings[0].contains("@Gone/x.png"), "{:?}", s.warnings);
    assert!(s.empty_because.is_none());
    // nothing chosen: the site is empty and says why
    st.rules(&format!("\"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&st.url()).unwrap()));
    let empty = st.publish().unwrap();
    assert_eq!(empty.summary.notes, 0);
    assert!(empty.summary.empty_because.is_some_and(|w| w.contains("No note is chosen")));
}

// spec: SITE-001
#[test]
fn site_001_an_empty_publish_pushes_the_hello_world_page() {
    let st = Stack::new("hello", false, &[("A.md", "a note\n")]).unwrap();
    st.rules(&format!("\"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&st.url()).unwrap()));
    let done = st.publish().unwrap();
    assert_eq!(done.summary.notes, 0);
    assert_eq!(st.remote_files().iter().filter(|f| f.starts_with("content/")).collect::<Vec<_>>(), ["content/_index.md"]);
    let page = st.remote(&["show", &format!("{BRANCH}:content/_index.md")]);
    assert!(page.contains("No notes are chosen for publishing yet.") && !page.contains("laptop") && !page.contains(&st.id()), "{page}");
    // a note chosen later replaces the page
    st.f.file("A.md", "#pub a note\n");
    st.rules(&format!("\"include_tags\": [\"pub\"], \"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&st.url()).unwrap()));
    st.publish().unwrap();
    assert!(!st.remote(&["show", &format!("{BRANCH}:content/_index.md")]).contains("Hello from Shale"));
}

// spec: SITE-069, SITE-071, SITE-036
#[test]
fn site_069_media_go_to_lfs_are_uploaded_once_and_without_git_lfs_the_publish_says_so() {
    let Some(st) = Stack::new("lfs", true, &[("A.md", "listen ![[song.mp3]] and ![[pic.png]]\n")]) else { return };
    st.f.bytes("song.mp3", b"not really an mp3, but bytes");
    st.f.bytes("pic.png", b"png");
    let first = st.publish().unwrap();
    assert_eq!(first.summary.media, 2);
    // on the remote the song is a pointer, the picture is a plain file, and the song is in the LFS store
    let pointer = st.remote(&["show", &format!("{BRANCH}:static/media/files/song.mp3")]);
    assert!(pointer.starts_with("version https://git-lfs.github.com/spec/v1"), "{pointer}");
    assert_eq!(st.remote(&["show", &format!("{BRANCH}:static/media/files/pic.png")]), "png");
    let oid = pointer.lines().find_map(|l| l.strip_prefix("oid sha256:")).unwrap().to_string();
    let object = st.remote.join("lfs/objects").join(&oid[..2]).join(&oid[2..4]).join(&oid);
    assert_eq!(std::fs::read(&object).unwrap(), b"not really an mp3, but bytes");
    assert!(st.remote(&["show", &format!("{BRANCH}:.gitattributes")]).contains("*.mp3 filter=lfs"));
    // a second publish with the same file does not send it again
    let stamp = std::fs::metadata(&object).unwrap().modified().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    st.f.file("A.md", "listen again ![[song.mp3]]\n");
    st.publish().unwrap();
    assert_eq!(std::fs::metadata(&object).unwrap().modified().unwrap(), stamp, "the object was not uploaded again");
    // a song that is no longer published stays in the store until the server collects it (SITE-071)
    st.f.file("A.md", "no song\n");
    st.publish().unwrap();
    assert!(object.is_file());
    assert!(!st.remote_files().iter().any(|f| f.ends_with("song.mp3")));

    // without git-lfs the publish stops before it touches the remote, and says what to do
    let Some(plain) = Stack::new("lfs-missing", false, &[("A.md", "listen ![[song.mp3]]\n")]) else { return };
    plain.f.bytes("song.mp3", b"bytes");
    let e = plain.publish().unwrap_err();
    assert!(matches!(&e, Failure::Error(m) if m.contains("Git LFS is not installed") && m.contains("git-lfs.com")), "{e:?}");
    assert!(plain.remote(&["for-each-ref"]).is_empty());
    // and a site with no media needs none
    plain.f.file("A.md", "no media\n");
    std::fs::remove_file(plain.f.dir.join("song.mp3")).unwrap();
    plain.publish().unwrap();
}

// spec: SITE-066, SITE-076
#[test]
fn site_066_the_command_line_names_one_vault_and_the_confirmation_of_a_takeover() {
    use crate::site_export::cli::{parse_command, Command, PublishArgs};
    let words = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    assert_eq!(parse_command(words(&["--publish", "v"])).unwrap(), Command::Publish(PublishArgs { vault: "v".into(), take_over: false }));
    assert_eq!(parse_command(words(&["v", "--take-over", "--publish"])).unwrap(), Command::Publish(PublishArgs { vault: "v".into(), take_over: true }));
    assert!(matches!(parse_command(words(&["v", "o", "--dry-run"])).unwrap(), Command::Export(_)));
    // one vault, never several and never an output folder: there is no publishing of all vaults at once (SITE-076)
    assert!(parse_command(words(&["--publish", "a", "b"])).unwrap_err().contains("takes one vault folder, not 2"));
    assert!(parse_command(words(&["--publish"])).unwrap_err().contains("not 0"));
    assert!(parse_command(words(&["--publish", "v", "--dry-run"])).unwrap_err().contains("unknown option --dry-run"));
    assert!(parse_command(words(&["v", "o", "--take-over"])).unwrap_err().contains("unknown option --take-over"));
}

// spec: SITE-066, SITE-074
#[test]
fn site_066_the_command_publishes_prints_the_summary_and_stops_at_a_target_of_another_vault_unless_told() {
    use crate::site_export::cli::{publish as run_publish, PublishArgs};
    let st = Stack::new("cli", false, &[("A.md", "a note ![[@Gone/x.png]]\n")]).unwrap();
    let go = |st: &Stack, take_over: bool| {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let r = run_publish(&PublishArgs { vault: st.f.dir.clone(), take_over }, &st.git, &[], &st.base, &mut out, &mut err);
        (r, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    };
    let (r, out, err) = go(&st, false);
    r.unwrap();
    assert_eq!(out, "Published 1 note, 0 media files and 0 links; 1 link became plain text.\n");
    assert!(err.contains("shale: Writing the site…\n") && err.contains("shale: warning: A.md: @Gone/x.png"), "{err}");
    assert_eq!(st.remote(&["rev-list", "--count", BRANCH]), "1");
    // another vault's target
    let other = Stack::new("cli-other", false, &[("B.md", "b\n")]).unwrap();
    other.rules(&format!("\"all_notes\": true, \"url\": {}, \"branch\": \"{BRANCH}\"", serde_json::to_string(&st.url()).unwrap()));
    other.f.file(".shale/history-id", &format!("{}\n", "ef".repeat(16)));
    let tip = st.remote_tip();
    let (r, out, _) = go(&other, false);
    assert!(r.unwrap_err().contains("published there") && out.is_empty());
    assert_eq!(st.remote_tip(), tip);
    let (r, out, _) = go(&other, true);
    r.unwrap();
    assert!(out.starts_with("Published 1 note"));
    assert_ne!(st.remote_tip(), tip);
    // a folder that is no vault: the reason, and nothing is pushed
    let (mut o, mut e) = (Vec::new(), Vec::new());
    let args = PublishArgs { vault: st.f.root.join("nothing"), take_over: false };
    assert!(run_publish(&args, &st.git, &[], &st.base, &mut o, &mut e).unwrap_err().contains("is not a folder"));
}

// spec: SITE-075, SITE-064, SITE-052
#[test]
fn site_075_targets_on_one_server_and_user_share_one_keyring_entry_and_the_command_uses_the_askpass_helper() {
    use crate::gitsync::credentials::{parse_prompt, server_key, user_of, ASKPASS_ENV};
    let a = "https://shale@Git.Example.org/me/site.git";
    let b = "https://shale@git.example.org:443/other/place.git";
    assert_eq!((server_key(a), user_of(a)), (server_key(b), user_of(b)));
    assert_eq!(server_key(a).as_deref(), Some("https://git.example.org"));
    // another host, or another user on the same host, has an entry of its own
    assert_ne!(server_key(a), server_key("https://shale@git.other.org/me/site.git"));
    assert_ne!(user_of(a), user_of("https://ada@git.example.org/me/site.git"));
    // the prompt of git for either target names the same entry
    let (pa, pb) = (parse_prompt(&format!("Password for '{a}': ")).unwrap(), parse_prompt(&format!("Password for '{b}': ")).unwrap());
    assert_eq!(pa, pb);
    // the program of the command line asks Shale for passwords, like the one of the window
    let program = crate::site_export::cli::device_git(crate::gitsync::managed::GitChoice::Auto);
    assert!(program.env.iter().any(|(k, v)| k == "GIT_ASKPASS" && !v.is_empty()) && program.env.iter().any(|(k, v)| k == ASKPASS_ENV && v == "1"));
}
