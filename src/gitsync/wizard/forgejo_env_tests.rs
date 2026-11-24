//! The wizard's server path against a real Forgejo in a container (SYNC-066, `tests/forgejo-env/`):
//! `cargo test -- --ignored forgejo_env` with the stack up. Git gets the token through the real
//! askpass helper (the Shale executable, SYNC-054) from a pretend `secret-tool` (tests/askpass/), as
//! it would from a desktop's keyring.

use super::api::{self, ApiError};
use super::connect_and_inspect;
use super::diagnose;
use super::diagnose_tests::{make_key, public_of, TestAgent};
use super::provider::{Kind, Server};
use super::ssh;
use super::test_sync::{self, Problem, Remote};
use crate::calendar::dav::tests::closed_port;
use crate::calendar::dav::{self, Account};
use crate::gitsync::{self, credentials, Git, Options, TreeLock};
use crate::secret::Secret;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PORT: u16 = 13000;
const PASSWORD: &str = "shale-forgejo-test-1";
const PASSPHRASE: &str = "correct horse battery staple";

fn server() -> Server {
    Server { kind: Kind::Forgejo, scheme: "http".into(), host: format!("127.0.0.1:{PORT}") }
}

/// The Shale executable that cargo built for the tests: a bin target is built in a folder above the
/// test binary (`target/debug/shale`); `SHALE_ENV_TESTS_BIN` names another.
fn shale_binary() -> PathBuf {
    if let Some(p) = std::env::var_os("SHALE_ENV_TESTS_BIN") {
        return PathBuf::from(p);
    }
    let exe = std::env::current_exe().unwrap();
    let name = if cfg!(windows) { "shale.exe" } else { "shale" };
    exe.ancestors().skip(1).map(|d| d.join(name)).find(|c| c.is_file() && *c != exe).unwrap_or_else(|| panic!("the shale executable is not built above {} (cargo test builds it)", exe.display()))
}

fn unique(prefix: &str) -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{prefix}-{}-{n}", std::process::id())
}

/// A new access token of the seeded administrator, made through the API with the password.
fn make_token() -> Secret {
    let account = Account { url: String::new(), username: "shale".into(), password: Secret::new(PASSWORD) };
    let body = serde_json::json!({ "name": unique("t"), "scopes": ["all"] }).to_string();
    let r = dav::request_with("POST", &format!("http://127.0.0.1:{PORT}/api/v1/users/shale/tokens"), &["Content-Type: application/json", "Accept: application/json"], &body, &account)
        .unwrap_or_else(|e| panic!("Forgejo is not up on 127.0.0.1:{PORT} (tests/forgejo-env/forgejo-env.sh up): {e}"));
    assert_eq!(r.status, 201, "{}", r.body);
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    Secret::new(v["sha1"].as_str().expect("the token"))
}

/// A hermetic git whose askpass is the Shale executable and whose keyring is a pretend `secret-tool`.
struct Stack {
    root: PathBuf,
    git: Git,
}

impl Stack {
    fn new(tag: &str) -> Stack {
        let root = std::env::temp_dir().join(unique(&format!("shale-forgejo-{tag}")));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("secrets")).unwrap();
        let fake = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/askpass/secret-tool")).unwrap();
        crate::test_exec::write_script(&root.join("bin/secret-tool"), &fake);
        let mut git = crate::gitsync::wizard::ui_git(&root);
        let rest = std::env::var_os("PATH").unwrap_or_default();
        let path = std::env::join_paths(std::iter::once(root.join("bin")).chain(std::env::split_paths(&rest))).unwrap();
        git.env.push(("PATH".into(), path));
        git.env.push(("FAKE_SECRET_DIR".into(), root.join("secrets").into_os_string()));
        credentials::attach(&mut git, &shale_binary());
        Stack { root, git }
    }

    /// Keep `token` for `user` the way the keyring would hold it for the helper.
    fn keep(&self, user: &str, token: &str) {
        let key = credentials::server_key(&server().base()).unwrap();
        let mut c = Command::new(self.root.join("bin/secret-tool"));
        c.args(["store", "--label", "test", "application", "shale-git", "server", &key, "user", user]).env("FAKE_SECRET_DIR", self.root.join("secrets")).stdin(Stdio::piped());
        let mut child = c.spawn().unwrap();
        child.stdin.take().unwrap().write_all(token.as_bytes()).unwrap();
        assert!(child.wait().unwrap().success());
    }

    fn vault(&self, name: &str) -> PathBuf {
        let v = self.root.join(name);
        std::fs::create_dir_all(&v).unwrap();
        v
    }
}

impl Drop for Stack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn options(host: &str) -> Options {
    Options { host: host.into(), stamp: "2026-11-21 1200".into(), ..Default::default() }
}

// spec: SYNC-054, SYNC-055, SYNC-057, SYNC-058, SYNC-066
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_a_token_creates_a_repository_and_git_syncs_with_it_through_the_askpass_helper() {
    let st = Stack::new("sync");
    let token = make_token();
    let s = server();
    // the API: who, create, list, and the clash of a second create
    assert_eq!(api::whoami(&s, &token).unwrap(), "shale");
    let name = unique("notes");
    let made = api::create(&s, &token, &name, "made by a test").unwrap();
    assert_eq!(made.full_name(), format!("shale/{name}"));
    assert!(made.private);
    assert_eq!(api::create(&s, &token, &name, ""), Err(ApiError::Exists));
    assert!(api::list(&s, &token).unwrap().iter().any(|r| r.name == name), "the new repository is listed");
    assert_eq!(api::whoami(&s, &Secret::new("not-a-token")), Err(ApiError::Auth));

    // git signs in through the helper: the address has the user name, the token is only in the keyring
    st.keep("shale", token.expose());
    let remote = s.https_remote("shale", "shale", &name);
    assert!(!remote.contains(token.expose()));
    let a = st.vault("a");
    crate::fsutil::write_atomic(&a.join("Note.md"), b"from a\n").unwrap();
    let (state, has_notes, _) = connect_and_inspect(&st.git, &a, &remote, None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap();
    assert_eq!((state, has_notes), (Remote::Empty, true));
    let rep = gitsync::sync(&st.git, &a, &options("a"), &TreeLock::default()).unwrap();
    assert!(rep.pushed, "{rep:?}");
    // the notes are on the server: another computer's vault takes them
    let b = st.vault("b");
    let (state, has_notes, _) = connect_and_inspect(&st.git, &b, &remote, None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap();
    assert_eq!((state, has_notes), (Remote::HasNotes, false));
    gitsync::sync(&st.git, &b, &options("b"), &TreeLock::default()).unwrap();
    assert_eq!(crate::fsutil::read_to_string(b.join("Note.md")).unwrap(), "from a\n");
    // and the token is in no file of either repository
    for v in [&a, &b] {
        let config = std::fs::read_to_string(v.join(".git/config")).unwrap();
        assert!(!config.contains(token.expose()), "{config}");
        assert!(config.contains(&format!("url = {remote}")), "{config}");
    }
}

// spec: SYNC-058, SYNC-066
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_a_wrong_token_a_missing_repository_and_no_server_are_told_in_plain_words() {
    let st = Stack::new("words");
    let token = make_token();
    let s = server();
    let name = unique("exists");
    api::create(&s, &token, &name, "").unwrap();
    let remote = s.https_remote("shale", "shale", &name);

    // a wrong token
    st.keep("shale", "this-is-not-the-token");
    let v = st.vault("wrong");
    let err = connect_and_inspect(&st.git, &v, &remote, None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap_err();
    assert_eq!(err, Problem::Sign.words(), "{err}");
    // no token at all (a user the keyring knows nothing about)
    let v2 = st.vault("none");
    let err = connect_and_inspect(&st.git, &v2, &s.https_remote("nobody", "shale", &name), None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap_err();
    assert_eq!(err, Problem::Sign.words(), "{err}");

    // the right token, a repository that is not there
    st.keep("shale", token.expose());
    let v3 = st.vault("missing");
    let err = connect_and_inspect(&st.git, &v3, &s.https_remote("shale", "shale", &unique("missing")), None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap_err();
    assert_eq!(err, Problem::NoRepository.words(), "{err}");
    // and with it the one that is there works
    let v4 = st.vault("ok");
    assert_eq!(connect_and_inspect(&st.git, &v4, &remote, None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap().0, Remote::Empty);

    // no server: a port nothing listens on
    let closed = closed_port();
    let dead = Server { kind: Kind::Forgejo, scheme: "http".into(), host: format!("127.0.0.1:{}", closed.port) };
    let v5 = st.vault("dead");
    let err = connect_and_inspect(&st.git, &v5, &dead.https_remote("shale", "shale", "x"), None, &st.root.join("history"), &TreeLock::default(), &crate::busy::Cancel::default()).unwrap_err();
    assert_eq!(err, Problem::NoNetwork.words(), "{err}");
    // the plain-words function agrees with what git said
    let raw = test_sync::inspect(&st.git, &v5).unwrap_err();
    assert_eq!(test_sync::classify(&raw), Problem::NoNetwork, "{raw}");
}

// ---------------------------------------------------------------- SSH (SYNC-081, SYNC-083)

const SSH_PORT: u16 = 13022;

/// Register a public key with the seeded administrator through the API (SYNC-081).
fn register_key(public: &str) {
    let account = Account { url: String::new(), username: "shale".into(), password: Secret::new(PASSWORD) };
    let body = serde_json::json!({ "title": unique("k"), "key": public }).to_string();
    let r = dav::request_with("POST", &format!("http://127.0.0.1:{PORT}/api/v1/user/keys"), &["Content-Type: application/json", "Accept: application/json"], &body, &account)
        .unwrap_or_else(|e| panic!("Forgejo is not up on 127.0.0.1:{PORT} (tests/forgejo-env/forgejo-env.sh up): {e}"));
    assert_eq!(r.status, 201, "{}", r.body);
}

fn ssh_remote(repo: &str) -> String {
    format!("ssh://git@127.0.0.1:{SSH_PORT}/shale/{repo}.git")
}

impl Stack {
    /// A git that signs in over SSH with `key` (the command of the wizard, SYNC-056) and this agent
    /// socket; a `known_hosts` file of the test keeps the real one of the person untouched.
    fn ssh_git(&self, key: &Path, sock: &Path) -> Git {
        let mut git = self.git.clone();
        let hosts = self.root.join("known_hosts");
        let cmd = format!("{} -o 'UserKnownHostsFile={}'", ssh::ssh_command(Some(key), true).unwrap(), hosts.display());
        git.env.push(("GIT_SSH_COMMAND".into(), cmd.into()));
        git.env.push(("SSH_AUTH_SOCK".into(), sock.as_os_str().to_os_string()));
        git
    }

    /// A vault whose `origin` is `url`.
    fn connected(&self, git: &Git, name: &str, url: &str) -> PathBuf {
        let v = self.vault(name);
        gitsync::connect_moving(git, &v, url, &self.root.join("history"), &TreeLock::default()).unwrap();
        v
    }
}

/// A new private repository of the administrator.
fn new_repo(prefix: &str) -> String {
    let name = unique(prefix);
    api::create(&server(), &make_token(), &name, "made by a test").unwrap();
    name
}

// spec: SYNC-081, SYNC-083
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_ssh_a_key_without_a_passphrase_creates_a_repository_and_git_syncs_with_it() {
    let st = Stack::new("ssh-plain");
    let key = make_key(&st.root, "plain", "");
    register_key(&public_of(&key));
    let git = st.ssh_git(&key, &st.root.join("no-agent.sock"));
    let url = ssh_remote(&new_repo("ssh"));
    let a = st.connected(&git, "a", &url);
    crate::fsutil::write_atomic(&a.join("Note.md"), b"over ssh\n").unwrap();
    assert_eq!(test_sync::inspect(&git, &a).unwrap(), Remote::Empty);
    assert!(gitsync::sync(&git, &a, &options("a"), &TreeLock::default()).unwrap().pushed);
    // another computer's vault takes the note
    let b = st.connected(&git, "b", &url);
    assert_eq!(test_sync::inspect(&git, &b).unwrap(), Remote::HasNotes);
    gitsync::sync(&git, &b, &options("b"), &TreeLock::default()).unwrap();
    assert_eq!(crate::fsutil::read_to_string(b.join("Note.md")).unwrap(), "over ssh\n");
    // a key the server does not know is "not accepted", and a key without a passphrase is never "locked"
    let stranger = make_key(&st.root, "stranger", "");
    let git2 = st.ssh_git(&stranger, &st.root.join("no-agent.sock"));
    let raw = test_sync::inspect(&git2, &b).unwrap_err();
    assert_eq!(diagnose::diagnose(&raw, &diagnose::gather(&git2, &b, &st.root, &raw)), Problem::SshKey, "{raw}");
}

// spec: SYNC-077, SYNC-083
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_ssh_a_key_with_a_passphrase_syncs_through_an_agent_that_holds_it() {
    let st = Stack::new("ssh-agent");
    let key = make_key(&st.root, "secret", PASSPHRASE);
    register_key(&public_of(&key));
    let agent = TestAgent::start(&st.root);
    agent.add(&key, PASSPHRASE);
    // Shale is given the agent's socket and nothing else: no passphrase anywhere
    let git = st.ssh_git(&key, &agent.sock);
    assert!(git.env.iter().all(|(_, v)| !v.to_string_lossy().contains(PASSPHRASE)));
    let url = ssh_remote(&new_repo("agent"));
    let a = st.connected(&git, "a", &url);
    crate::fsutil::write_atomic(&a.join("Note.md"), b"through the agent\n").unwrap();
    assert_eq!(test_sync::inspect(&git, &a).unwrap(), Remote::Empty);
    assert!(gitsync::sync(&git, &a, &options("a"), &TreeLock::default()).unwrap().pushed);
    let b = st.connected(&git, "b", &url);
    gitsync::sync(&git, &b, &options("b"), &TreeLock::default()).unwrap();
    assert_eq!(crate::fsutil::read_to_string(b.join("Note.md")).unwrap(), "through the agent\n");
}

// spec: SYNC-079, SYNC-080, SYNC-083
#[test]
#[ignore = "needs the Forgejo test stack: tests/forgejo-env/forgejo-env.sh up"]
fn forgejo_env_ssh_a_key_with_a_passphrase_that_the_agent_does_not_hold_is_told_as_locked() {
    let st = Stack::new("ssh-locked");
    let key = make_key(&st.root, "secret", PASSPHRASE);
    register_key(&public_of(&key));
    let url = ssh_remote(&new_repo("locked"));
    let agent = TestAgent::start(&st.root);
    let told = |git: &Git, v: &Path| {
        let raw = test_sync::inspect(git, v).unwrap_err();
        assert!(raw.contains("Permission denied (publickey"), "{raw}");
        diagnose::diagnose(&raw, &diagnose::gather(git, v, &st.root, &raw))
    };
    // an agent that runs and does not hold the key: locked, with the fix
    let git = st.ssh_git(&key, &agent.sock);
    let v = st.connected(&git, "v", &url);
    let p = told(&git, &v);
    assert_eq!(p, Problem::KeyLocked);
    assert!(p.words().contains("ssh-add") && p.guide() == Some("The key is locked: add it to the ssh-agent"), "{}", p.words());
    // the normal sync fails with the same text, and the facts the sync job gathers give the same diagnosis
    let raw = gitsync::sync(&git, &v, &options("v"), &TreeLock::default()).unwrap_err();
    assert_eq!(diagnose::diagnose(&raw, &diagnose::gather(&git, &v, &st.root, &raw)), Problem::KeyLocked, "{raw}");
    // no agent at all
    let dead = st.ssh_git(&key, &st.root.join("nobody.sock"));
    assert_eq!(told(&dead, &v), Problem::NoAgent);
    // the agent holds a key with a passphrase that the server does not know: not accepted, not locked
    let stranger = make_key(&st.root, "stranger", PASSPHRASE);
    agent.add(&stranger, PASSPHRASE);
    assert_eq!(told(&st.ssh_git(&stranger, &agent.sock), &v), Problem::SshKey);
    // and once the agent holds the right key it works
    agent.add(&key, PASSPHRASE);
    assert_eq!(test_sync::inspect(&git, &v).unwrap(), Remote::Empty);
}
