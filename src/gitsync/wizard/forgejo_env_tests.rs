//! The wizard's server path against a real Forgejo in a container (SYNC-066, `tests/forgejo-env/`):
//! `cargo test -- --ignored forgejo_env` with the stack up. Git gets the token through the real
//! askpass helper (the Shale executable, SYNC-054) from a pretend `secret-tool` (tests/askpass/), as
//! it would from a desktop's keyring.

use super::api::{self, ApiError};
use super::connect_and_inspect;
use super::provider::{Kind, Server};
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
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/askpass/secret-tool"), root.join("bin/secret-tool")).unwrap();
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

// spec: SYNC-054, SYNC-055, SYNC-057, SYNC-058
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
    let (state, has_notes) = connect_and_inspect(&st.git, &a, &remote, None).unwrap();
    assert_eq!((state, has_notes), (Remote::Empty, true));
    let rep = gitsync::sync(&st.git, &a, &options("a"), &TreeLock::default()).unwrap();
    assert!(rep.pushed, "{rep:?}");
    // the notes are on the server: another computer's vault takes them
    let b = st.vault("b");
    let (state, has_notes) = connect_and_inspect(&st.git, &b, &remote, None).unwrap();
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

// spec: SYNC-058
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
    let err = connect_and_inspect(&st.git, &v, &remote, None).unwrap_err();
    assert_eq!(err, Problem::Sign.words(), "{err}");
    // no token at all (a user the keyring knows nothing about)
    let v2 = st.vault("none");
    let err = connect_and_inspect(&st.git, &v2, &s.https_remote("nobody", "shale", &name), None).unwrap_err();
    assert_eq!(err, Problem::Sign.words(), "{err}");

    // the right token, a repository that is not there
    st.keep("shale", token.expose());
    let v3 = st.vault("missing");
    let err = connect_and_inspect(&st.git, &v3, &s.https_remote("shale", "shale", &unique("missing")), None).unwrap_err();
    assert_eq!(err, Problem::NoRepository.words(), "{err}");
    // and with it the one that is there works
    let v4 = st.vault("ok");
    assert_eq!(connect_and_inspect(&st.git, &v4, &remote, None).unwrap().0, Remote::Empty);

    // no server: a port nothing listens on
    let closed = closed_port();
    let dead = Server { kind: Kind::Forgejo, scheme: "http".into(), host: format!("127.0.0.1:{}", closed.port) };
    let v5 = st.vault("dead");
    let err = connect_and_inspect(&st.git, &v5, &dead.https_remote("shale", "shale", "x"), None).unwrap_err();
    assert_eq!(err, Problem::NoNetwork.words(), "{err}");
    // the plain-words function agrees with what git said
    let raw = test_sync::inspect(&st.git, &v5).unwrap_err();
    assert_eq!(test_sync::classify(&raw), Problem::NoNetwork, "{raw}");
}
