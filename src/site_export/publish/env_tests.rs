//! The publish against a real Forgejo in a container (SITE-062…SITE-075, `tests/forgejo-env/`):
//! `cargo test -- --ignored forgejo_env` with the stack up. HTTPS goes through Shale's askpass helper
//! from a pretend keyring (SYNC-054), SSH through a key registered with the server, the media through
//! the server's LFS store.
#![cfg(target_os = "linux")]

use super::*;
use crate::calendar::dav::{self, Account};
use crate::gitsync::wizard::diagnose::{self, Facts};
use crate::gitsync::wizard::diagnose_tests::{make_key, public_of};
use crate::gitsync::wizard::forgejo_env_tests::{make_token, new_repo, register_key, server, ssh_remote, unique, Stack, PORT};
use crate::gitsync::wizard::test_sync::Problem;
use crate::secret::Secret;
use crate::site_export::test_support::Fixture;

/// A vault that publishes to `url` on the branch `site`, with these notes.
pub(super) fn vault(tag: &str, url: &str, files: &[(&str, &str)]) -> Fixture {
    let rules = format!("\"all_notes\": true, \"url\": {}, \"branch\": \"site\"", serde_json::to_string(url).unwrap());
    Fixture::with(tag, &rules, files)
}

pub(super) fn go(git: &Git, f: &Fixture, base: &Path, take_over: bool) -> Result<Published, Failure> {
    publish(git, &Request { vault: &f.dir, libraries: &[], base, device: "test device", take_over }, &Cancel::default(), &|_| {})
}

pub(super) fn with_lfs(st: &Stack) -> Git {
    let mut git = st.git.clone();
    git.env.retain(|(k, _)| k != "SHALE_TEST_NO_GIT_LFS");
    git
}

pub(super) fn run_ok(git: &Git, dir: &Path, args: &[&str]) -> String {
    let a: Vec<&OsStr> = args.iter().map(OsStr::new).collect();
    let o = git.run(dir, &a).unwrap();
    assert!(o.ok, "git {args:?}: {}", o.stderr);
    o.text()
}

/// The commit of the branch `site` on the server, asked with the same sign-in.
pub(super) fn tip(git: &Git, dir: &Path, url: &str) -> String {
    run_ok(git, dir, &["ls-remote", url, "refs/heads/site"]).split_whitespace().next().unwrap_or("").to_string()
}

/// Does the LFS store of the repository hold the object? The batch API says it has a download for it.
fn lfs_holds(token: &Secret, repo: &str, oid: &str, size: usize) -> bool {
    let account = Account { url: String::new(), username: "shale".into(), password: token.clone() };
    let body = serde_json::json!({"operation": "download", "transfers": ["basic"], "objects": [{"oid": oid, "size": size}]}).to_string();
    let url = format!("http://127.0.0.1:{PORT}/shale/{repo}.git/info/lfs/objects/batch");
    let r = dav::request_with("POST", &url, &["Content-Type: application/vnd.git-lfs+json", "Accept: application/vnd.git-lfs+json"], &body, &account).unwrap();
    assert_eq!(r.status, 200, "{}", r.body);
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    v["objects"][0]["actions"]["download"].is_object()
}

// spec: SITE-062, SITE-064, SITE-068, SITE-075, SITE-066, SITE-052
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_publish_over_https_with_a_token_makes_one_snapshot_and_the_next_replaces_it() {
    let st = Stack::new("pub-https");
    let token = make_token();
    st.keep("shale", token.expose());
    let repo = unique("site");
    crate::gitsync::wizard::api::create(&server(), &token, &repo, "made by a test").unwrap();
    let url = server().https_remote("shale", "shale", &repo);
    let f = vault("pub-https", &url, &[("Open.md", "A note about the harbour.\n"), ("Secret.md", "---\npublish: false\n---\nthe launch codes\n")]);
    let base = f.root.join("data");

    let first = go(&st.git, &f, &base, false).unwrap();
    assert_eq!(first.summary.notes, 1);
    assert_eq!(tip(&st.git, &st.root, &url), first.commit, "the branch is on the server");
    // the token is in no file of the publish repository
    let config = std::fs::read_to_string(base.join(history::vault_id(&f.dir).unwrap()).join("publish/.git/config")).unwrap();
    assert!(config.contains(&format!("url = {url}")) && !config.contains(token.expose()), "{config}");

    // the note is unpublished and a new one made: the server has one commit, with no trace of the old note
    f.file("Open.md", "---\npublish: false\n---\nA note about the harbour.\n");
    f.file("Next.md", "The lighthouse.\n");
    let second = go(&st.git, &f, &base, false).unwrap();
    assert_eq!(tip(&st.git, &st.root, &url), second.commit);
    let clone = st.root.join("clone");
    run_ok(&st.git, &st.root, &["clone", "--quiet", &url, clone.to_str().unwrap()]);
    run_ok(&st.git, &clone, &["checkout", "--quiet", "site"]);
    assert_eq!(run_ok(&st.git, &clone, &["rev-list", "--count", "--all"]), "1", "one commit on the server");
    assert!(clone.join("content/next.md").is_file() && !clone.join("content/open.md").exists() && !clone.join("content/secret.md").exists());
    let history = run_ok(&st.git, &clone, &["log", "--all", "-p", "--format=%B"]);
    assert!(history.contains("The lighthouse.") && !history.contains("harbour") && !history.contains("launch codes"), "the old note is in no commit");
    let meta = message::parse(&run_ok(&st.git, &clone, &["log", "-1", "--format=%B"]));
    assert_eq!((meta.device.as_deref(), meta.vault_id), (Some("test device"), history::vault_id(&f.dir)));

    // who published last is read from the server (SITE-072)
    let last = last_published(&st.git, &f.dir, &base, &Cancel::default()).unwrap().unwrap();
    assert!(last.this_vault && last.meta.device.as_deref() == Some("test device"));
}

// spec: SITE-064, SITE-065, SITE-075
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_publish_a_wrong_token_or_a_missing_repository_fails_in_plain_words_and_changes_nothing() {
    let st = Stack::new("pub-fail");
    let token = make_token();
    st.keep("shale", token.expose());
    let repo = unique("site");
    crate::gitsync::wizard::api::create(&server(), &token, &repo, "").unwrap();
    let url = server().https_remote("shale", "shale", &repo);
    let f = vault("pub-fail", &url, &[("Open.md", "A note.\n")]);
    let base = f.root.join("data");
    let first = go(&st.git, &f, &base, false).unwrap();

    // the token is wrong now: git says so and the publish fails before it pushes anything
    st.keep("shale", "this-is-not-the-token");
    f.file("Open.md", "A changed note.\n");
    let Failure::Error(e) = go(&st.git, &f, &base, false).unwrap_err() else { panic!("an error") };
    assert_eq!(diagnose::diagnose(&e, &Facts::default()), Problem::Sign, "{e}");
    assert!(!e.contains("this-is-not-the-token"));
    st.keep("shale", token.expose());
    assert_eq!(tip(&st.git, &st.root, &url), first.commit, "the server is as it was");
    // a repository that is not there
    let gone = vault("pub-gone", &server().https_remote("shale", "shale", &unique("missing")), &[("Open.md", "A note.\n")]);
    let Failure::Error(e) = go(&st.git, &gone, &gone.root.join("data"), false).unwrap_err() else { panic!("an error") };
    assert_eq!(diagnose::diagnose(&e, &Facts::default()), Problem::NoRepository, "{e}");
    // no token for the user the address names
    let nobody = vault("pub-nobody", &server().https_remote("nobody", "shale", &repo), &[("Open.md", "A note.\n")]);
    let Failure::Error(e) = go(&st.git, &nobody, &nobody.root.join("data"), false).unwrap_err() else { panic!("an error") };
    assert_eq!(diagnose::diagnose(&e, &Facts::default()), Problem::Sign, "{e}");
}

// spec: SITE-074, SITE-072
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_publish_a_target_of_another_vault_asks_and_is_taken_over_only_when_confirmed() {
    let st = Stack::new("pub-takeover");
    let token = make_token();
    st.keep("shale", token.expose());
    let repo = unique("site");
    crate::gitsync::wizard::api::create(&server(), &token, &repo, "").unwrap();
    let url = server().https_remote("shale", "shale", &repo);
    let a = vault("pub-a", &url, &[("A.md", "from vault a\n")]);
    let b = vault("pub-b", &url, &[("B.md", "from vault b\n")]);
    b.file(".shale/history-id", &format!("{}\n", "12".repeat(16)));
    let first = go(&st.git, &a, &a.root.join("data"), false).unwrap();
    let Failure::Takeover(t) = go(&st.git, &b, &b.root.join("data"), false).unwrap_err() else { panic!("another vault's target asks") };
    assert_eq!((t.meta.vault_name.as_deref(), t.meta.device.as_deref()), (Some("vault"), Some("test device")));
    assert_eq!(tip(&st.git, &st.root, &url), first.commit, "nothing was pushed");
    let taken = go(&st.git, &b, &b.root.join("data"), true).unwrap();
    assert_eq!(tip(&st.git, &st.root, &url), taken.commit);
    // the first vault is the stranger now
    assert!(matches!(go(&st.git, &a, &a.root.join("data"), false), Err(Failure::Takeover(_))));
    // a branch that does not exist in a new repository is taken without asking
    let fresh = unique("fresh");
    crate::gitsync::wizard::api::create(&server(), &token, &fresh, "").unwrap();
    let c = vault("pub-c", &server().https_remote("shale", "shale", &fresh), &[("C.md", "c\n")]);
    go(&st.git, &c, &c.root.join("data"), false).unwrap();
}

// spec: SITE-069, SITE-071
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_publish_media_go_to_the_lfs_store_once_and_the_branch_holds_pointers() {
    let st = Stack::new("pub-lfs");
    if st.git.lfs_version().is_none() && with_lfs(&st).lfs_version().is_none() {
        eprintln!("skipping: git-lfs is not installed here (CI installs it)");
        return;
    }
    let git = {
        let mut g = with_lfs(&st);
        g.env.push(("GIT_LFS_PROGRESS".into(), st.root.join("lfs-progress").into_os_string()));
        g
    };
    let token = make_token();
    st.keep("shale", token.expose());
    let repo = unique("site");
    crate::gitsync::wizard::api::create(&server(), &token, &repo, "").unwrap();
    let url = server().https_remote("shale", "shale", &repo);
    let f = vault("pub-lfs", &url, &[("Songs.md", "Listen ![[tune.mp3]]\n")]);
    // new bytes every run: the store of a server keeps an object once for every repository
    let song = format!("not an mp3 but some bytes of one, {}", unique("song")).into_bytes();
    f.bytes("tune.mp3", &song);
    let base = f.root.join("data");
    let progress = || std::fs::read_to_string(st.root.join("lfs-progress")).unwrap_or_default();

    let first = go(&git, &f, &base, false).unwrap();
    assert_eq!(first.summary.media, 1);
    let oid = {
        use std::fmt::Write;
        let d = ring::digest::digest(&ring::digest::SHA256, &song);
        d.as_ref().iter().fold(String::new(), |mut s, b| {
            let _ = write!(s, "{b:02x}");
            s
        })
    };
    assert!(lfs_holds(&token, &repo, &oid, song.len()), "the LFS store of the server holds the song");
    let clone = st.root.join("clone");
    run_ok(&git, &st.root, &["clone", "--quiet", &url, clone.to_str().unwrap()]);
    run_ok(&git, &clone, &["checkout", "--quiet", "site"]);
    let pointer = std::fs::read_to_string(clone.join("static/media/files/tune.mp3")).unwrap();
    assert!(pointer.contains(&format!("oid sha256:{oid}")), "the branch holds a pointer: {pointer}");
    let sent = progress().lines().filter(|l| l.starts_with("upload")).count();
    assert_eq!(sent, 1, "the song was sent: {:?}", progress());

    // the same song in the next snapshot is not sent again
    f.file("Songs.md", "Listen again ![[tune.mp3]]\n");
    let second = go(&git, &f, &base, false).unwrap();
    assert_ne!(first.commit, second.commit);
    assert_eq!(progress().lines().filter(|l| l.starts_with("upload")).count(), sent, "not uploaded again: {:?}", progress());
    // unpublished, the song stays in the store until the server collects it (SITE-071)
    f.file("Songs.md", "No song now\n");
    go(&git, &f, &base, false).unwrap();
    assert!(lfs_holds(&token, &repo, &oid, song.len()));
}

// spec: SITE-064, SITE-074, SITE-075
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_publish_over_ssh_with_a_key_and_a_stranger_key_is_not_accepted() {
    let st = Stack::new("pub-ssh");
    let key = make_key(&st.root, "plain", "");
    register_key(&public_of(&key));
    let git = st.ssh_git(&key, &st.root.join("no-agent.sock"));
    let url = ssh_remote(&new_repo("pubssh"));
    let f = vault("pub-ssh", &url, &[("Open.md", "Over ssh.\n")]);
    let base = f.root.join("data");
    let done = go(&git, &f, &base, false).unwrap();
    assert_eq!(tip(&git, &st.root, &url), done.commit);
    // and again: the snapshot replaces the first
    f.file("Open.md", "Over ssh, changed.\n");
    let again = go(&git, &f, &base, false).unwrap();
    assert_eq!(tip(&git, &st.root, &url), again.commit);
    assert_eq!(last_published(&git, &f.dir, &base, &Cancel::default()).unwrap().map(|l| l.this_vault), Some(true));
    // a key the server does not know
    let stranger = make_key(&st.root, "stranger", "");
    let other = st.ssh_git(&stranger, &st.root.join("no-agent.sock"));
    let Failure::Error(e) = go(&other, &f, &base, false).unwrap_err() else { panic!("an error") };
    assert_eq!(diagnose::diagnose(&e, &diagnose::gather(&other, &base, &st.root, &e)), Problem::SshKey, "{e}");
    assert_eq!(tip(&git, &st.root, &url), again.commit, "the server is as it was");
}
