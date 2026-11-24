//! Tests of the sign-in diagnosis (SYNC-077…SYNC-079, SYNC-082): the classifier with the texts git
//! and ssh print, and the cheap checks with the real `ssh-keygen`, `ssh-agent` and `ssh-add`. A
//! missing tool fails the test (these are part of OpenSSH, which the sync needs).

use super::diagnose::{self, agent_holds, key_of_command, Agent, Facts};
use super::test_sync::{Problem, GUIDE_NOTE};
use super::{ssh, ui_git};
use crate::gitsync::Git;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const PASSPHRASE: &str = "correct horse battery staple";

fn unique(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() % 1_000_000;
    // short: the path of an agent's socket may have 107 bytes at most
    let d = std::env::temp_dir().join(format!("shale-{tag}-{}-{n}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A key pair made by the real `ssh-keygen`, with this passphrase (empty: none).
pub(super) fn make_key(dir: &Path, name: &str, passphrase: &str) -> PathBuf {
    let key = dir.join(name);
    let o = Command::new("ssh-keygen").args(["-q", "-t", "ed25519", "-N", passphrase, "-C", "shale test"]).arg("-f").arg(&key).stdin(Stdio::null()).output().expect("ssh-keygen is part of OpenSSH");
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    key
}

pub(super) fn public_of(key: &Path) -> String {
    let mut p = key.as_os_str().to_os_string();
    p.push(".pub");
    std::fs::read_to_string(PathBuf::from(p)).unwrap().trim().to_string()
}

/// An `ssh-agent` of the test's own, on a socket of its own, stopped when dropped.
pub(super) struct TestAgent {
    pub sock: PathBuf,
    pid: String,
    dir: PathBuf,
}

impl TestAgent {
    pub(super) fn start(dir: &Path) -> TestAgent {
        let sock = dir.join("agent.sock");
        let o = Command::new("ssh-agent").arg("-s").arg("-a").arg(&sock).stdin(Stdio::null()).output().expect("ssh-agent is part of OpenSSH");
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        let text = String::from_utf8_lossy(&o.stdout).into_owned();
        let pid = text.split("SSH_AGENT_PID=").nth(1).and_then(|r| r.split(';').next()).unwrap_or_else(|| panic!("no pid in {text}")).to_string();
        TestAgent { sock, pid, dir: dir.to_path_buf() }
    }

    /// `ssh-add <key>` with its passphrase given by `SSH_ASKPASS` (a script of the test; this is the only place that
    /// passes a passphrase to anything: Shale itself never does).
    pub(super) fn add(&self, key: &Path, passphrase: &str) {
        let askpass = self.dir.join("askpass.sh");
        crate::test_exec::write_script(&askpass, &format!("#!/bin/sh\necho '{passphrase}'\n"));
        let o = Command::new("ssh-add").arg(key).env("SSH_AUTH_SOCK", &self.sock).env("SSH_ASKPASS", &askpass).env("SSH_ASKPASS_REQUIRE", "force").env("DISPLAY", ":0").stdin(Stdio::null()).output().expect("ssh-add is part of OpenSSH");
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }
}

impl Drop for TestAgent {
    fn drop(&mut self) {
        let _ = Command::new("ssh-agent").arg("-k").env("SSH_AGENT_PID", &self.pid).env("SSH_AUTH_SOCK", &self.sock).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// A hermetic git that gets this `SSH_AUTH_SOCK` and this ssh command.
pub(super) fn git_with(sock: &Path, cmd: Option<String>) -> Git {
    let mut git = ui_git(Path::new("."));
    git.env.push(("SSH_AUTH_SOCK".into(), sock.as_os_str().to_os_string()));
    if let Some(c) = cmd {
        git.env.push(("GIT_SSH_COMMAND".into(), OsString::from(c)));
    }
    git
}

const DENIED: &str = "git@127.0.0.1: Permission denied (publickey).\nfatal: Could not read from remote repository.\n\nPlease make sure you have the correct access rights\nand the repository exists.";

fn locked(agent: Agent, held: bool) -> Facts {
    Facts { key_encrypted: true, agent, agent_has_key: held }
}

// spec: SYNC-079
#[test]
fn sync_079_each_case_is_told_from_the_text_of_git_and_the_facts_of_the_checks() {
    let none = Facts::default();
    let changed = "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\nIT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!\nOffending ED25519 key in /home/ada/.ssh/known_hosts:3\nHost key verification failed.\nfatal: Could not read from remote repository.";
    let cases: Vec<(&str, Facts, Problem)> = vec![
        // the key: unknown to the server, locked, no agent, or held by the agent and still unknown
        (DENIED, none, Problem::SshKey),
        (DENIED, locked(Agent::Running, false), Problem::KeyLocked),
        (DENIED, locked(Agent::Unreachable, false), Problem::NoAgent),
        (DENIED, locked(Agent::Unknown, false), Problem::NoAgent),
        (DENIED, locked(Agent::Running, true), Problem::SshKey),
        // facts about a key do not turn another failure into a lock
        ("fatal: unable to access 'https://x/o/r.git/': Could not resolve host: x", locked(Agent::Running, false), Problem::NoNetwork),
        // a password where a token is needed (GitHub, GitLab) and a plain wrong token (Forgejo)
        ("remote: Support for password authentication was removed on August 13, 2021.\nremote: Please see https://docs.github.com/get-started/git-basics/about-remote-repositories#cloning-with-https-urls for information on currently recommended modes of authentication.\nfatal: Authentication failed for 'https://github.com/ada/notes.git/'", none, Problem::TokenNeeded),
        ("remote: HTTP Basic: Access denied. The provided password or token is incorrect or your account has 2FA enabled and you must use a personal access token instead of a password. See https://gitlab.example.org/help/topics/git/troubleshooting_git#error-on-git-fetch-http-basic-access-denied\nfatal: Authentication failed for 'https://gitlab.example.org/ada/notes.git/'", none, Problem::TokenNeeded),
        ("remote: Credentials are incorrect or have expired. Retry your command or see https://codeberg.org/forgejo/forgejo/issues/2809 for more information\nfatal: Authentication failed for 'http://127.0.0.1:13000/shale/exp1.git/'", none, Problem::Sign),
        ("fatal: could not read Password for 'https://ada@git.example.org': terminal prompts disabled", none, Problem::Sign),
        // a login page of a proxy where git was expected
        ("fatal: unable to update url base from redirection:\n  asked for: http://127.0.0.1:13555/a/b.git/info/refs?service=git-upload-pack\n   redirect: http://127.0.0.1:13555/login", none, Problem::LoginPage),
        ("error: Cannot access URL http://127.0.0.1:13555/a/b.git/, return code 22\nfatal: git-http-push failed\nerror: failed to push some refs to 'http://127.0.0.1:13555/a/b.git'", none, Problem::LoginPage),
        ("fatal: unexpected content-type text/html; charset=utf-8", none, Problem::LoginPage),
        ("<!DOCTYPE html><html><head><title>Sign in</title>", none, Problem::LoginPage),
        // the host key, the others, and git's own message for anything else
        (changed, none, Problem::HostKey),
        ("No ED25519 host key is known for [127.0.0.1]:13022 and you have requested strict checking.\nHost key verification failed.", none, Problem::HostKey),
        ("fatal: unable to access 'https://x/': SSL certificate problem: self-signed certificate", none, Problem::Certificate),
        ("remote: Repository not found.\nfatal: repository 'https://x/o/r.git/' not found", none, Problem::NoRepository),
        ("ssh: connect to host x port 22: Network is unreachable", none, Problem::NoNetwork),
        ("fatal: something nobody expected", none, Problem::Other("fatal: something nobody expected".into())),
    ];
    for (text, facts, want) in cases {
        assert_eq!(diagnose::diagnose(text, &facts), want, "{text}");
    }
}

// spec: SYNC-079, SYNC-082
#[test]
fn sync_079_every_case_has_words_a_fix_and_a_guide_section_that_exists() {
    let note = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/userguide").join(format!("{GUIDE_NOTE}.md"))).unwrap();
    let headings: Vec<&str> = note.lines().filter_map(|l| l.strip_prefix("# ")).collect();
    let no_guide = [Problem::NoNetwork, Problem::NoRepository, Problem::Certificate];
    for p in super::test_sync::FIXED {
        assert!(p.what().len() > 20, "{p:?}");
        assert_eq!(p.fix().is_some(), p != Problem::Certificate, "{p:?}");
        assert_eq!(p.words(), p.fix().map_or(p.what(), |f| format!("{} {f}", p.what())));
        assert_eq!(Problem::from_words(&p.words()), Some(p.clone()), "the wizard hands the words on as text");
        match p.guide() {
            Some(h) => assert!(headings.contains(&h), "the note has no section “{h}” for {p:?}: {headings:?}"),
            None => assert!(no_guide.contains(&p), "{p:?} has no guide section"),
        }
    }
    let other = Problem::Other("boom".into());
    assert!(other.words().contains("boom") && other.guide().is_none() && other.fix().is_none() && Problem::from_words(&other.words()).is_none());
}

// spec: SYNC-078
#[test]
fn sync_078_the_key_of_an_ssh_command_is_read_quoted_or_not() {
    assert_eq!(key_of_command("ssh -i '/home/ada/.ssh/shale_x y' -o IdentitiesOnly=yes -o BatchMode=yes"), Some(PathBuf::from("/home/ada/.ssh/shale_x y")));
    assert_eq!(key_of_command("ssh -i /home/ada/.ssh/id_rsa -o BatchMode=yes"), Some(PathBuf::from("/home/ada/.ssh/id_rsa")));
    assert_eq!(key_of_command("ssh -o BatchMode=yes"), None);
    assert_eq!(key_of_command("ssh -i"), None);
    // what the wizard writes is read back
    let cmd = ssh::ssh_command(Some(Path::new("/k/shale_host")), true).unwrap();
    assert_eq!(key_of_command(&cmd), Some(PathBuf::from("/k/shale_host")));
}

// spec: SYNC-078
#[test]
fn sync_078_the_agent_s_list_holds_a_public_key_by_type_and_key_not_by_comment() {
    let list = "ssh-ed25519 AAAAC3Nza1 ada@laptop\nssh-rsa AAAAB3Nza2 work\n";
    assert!(agent_holds(list, "ssh-ed25519 AAAAC3Nza1 Shale on laptop"));
    assert!(agent_holds(list, "ssh-rsa AAAAB3Nza2"));
    assert!(!agent_holds(list, "ssh-ed25519 AAAAC3Nza2 ada@laptop"), "the same key text with another type is another key");
    assert!(!agent_holds(list, "ssh-ed25519 AAAAC3Nza9 x"));
    assert!(!agent_holds("", "ssh-ed25519 AAAAC3Nza1 x") && !agent_holds(list, "") && !agent_holds(list, "ssh-ed25519"));
}

// spec: SYNC-078
#[test]
fn sync_078_a_key_with_a_passphrase_is_told_from_one_without_by_the_real_ssh_keygen() {
    let d = unique("keys");
    let git = ui_git(&d);
    let plain = make_key(&d, "plain", "");
    let secret = make_key(&d, "secret", PASSPHRASE);
    assert_eq!(diagnose::key_encrypted(&git, &plain), Some(false));
    assert_eq!(diagnose::key_encrypted(&git, &secret), Some(true));
    assert_eq!(diagnose::key_encrypted(&git, &d.join("missing")), None);
    std::fs::write(d.join("junk"), "not a key\n").unwrap();
    assert_eq!(diagnose::key_encrypted(&git, &d.join("junk")), None);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: SYNC-078
#[test]
fn sync_078_the_agent_is_asked_with_the_real_ssh_add_and_its_list_is_compared_with_the_public_key() {
    let d = unique("agent");
    let key = make_key(&d, "secret", PASSPHRASE);
    let agent = TestAgent::start(&d);
    let git = git_with(&agent.sock, None);
    assert_eq!(diagnose::agent_keys(&git).as_deref(), Some(""), "an agent without keys answers with an empty list");
    agent.add(&key, PASSPHRASE);
    let list = diagnose::agent_keys(&git).unwrap();
    assert!(agent_holds(&list, &public_of(&key)), "{list}");
    let other = make_key(&d, "other", "");
    assert!(!agent_holds(&list, &public_of(&other)));
    // no agent at that socket
    assert_eq!(diagnose::agent_keys(&git_with(&d.join("nobody.sock"), None)), None);
    drop(agent);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: SYNC-078, SYNC-079
#[test]
fn sync_078_gather_looks_only_at_an_ssh_key_failure_and_finds_the_locked_key() {
    let d = unique("gather");
    let vault = d.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    let plain = make_key(&d, "plain", "");
    let secret = make_key(&d, "secret", PASSPHRASE);
    let agent = TestAgent::start(&d);
    let cmd = |k: &Path| ssh::ssh_command(Some(k), false);
    let facts = |git: &Git, home: &Path, text: &str| diagnose::gather(git, &vault, home, text);

    // a failure that is no key problem is not looked at, whatever the keys are
    let git = git_with(&agent.sock, cmd(&secret));
    assert_eq!(facts(&git, &d, "Could not resolve host: x"), Facts::default());
    // a key without a passphrase: nothing to find
    assert_eq!(facts(&git_with(&agent.sock, cmd(&plain)), &d, DENIED), Facts::default());
    // a key with a passphrase and an agent that does not hold it: locked
    let found = facts(&git, &d, DENIED);
    assert_eq!(found, Facts { key_encrypted: true, agent: Agent::Running, agent_has_key: false });
    assert_eq!(diagnose::diagnose(DENIED, &found), Problem::KeyLocked);
    // once the agent holds it, the server simply does not know the key
    agent.add(&secret, PASSPHRASE);
    let found = facts(&git, &d, DENIED);
    assert_eq!(found, Facts { key_encrypted: true, agent: Agent::Running, agent_has_key: true });
    assert_eq!(diagnose::diagnose(DENIED, &found), Problem::SshKey);
    // no agent that answers
    let found = facts(&git_with(&d.join("nobody.sock"), cmd(&secret)), &d, DENIED);
    assert_eq!(found, Facts { key_encrypted: true, agent: Agent::Unreachable, agent_has_key: false });
    assert_eq!(diagnose::diagnose(DENIED, &found), Problem::NoAgent);

    // the key of the repository's own ssh command, and the default keys of ~/.ssh
    let git = git_with(&agent.sock, None);
    let inner = vault.join("repo");
    std::fs::create_dir_all(&inner).unwrap();
    git.ok(&inner, &[OsStr::new("init"), OsStr::new("-q")]).unwrap();
    let c = cmd(&secret).unwrap();
    git.ok(&inner, &[OsStr::new("config"), OsStr::new("core.sshCommand"), OsStr::new(&c)]).unwrap();
    let found = diagnose::gather(&git_with(&d.join("nobody.sock"), None), &inner, &d, DENIED);
    assert!(found.key_encrypted && found.agent == Agent::Unreachable, "{found:?}");
    let home = d.join("home");
    std::fs::create_dir_all(home.join(".ssh")).unwrap();
    assert_eq!(facts(&git, &home, DENIED), Facts::default(), "no key at all");
    let default = make_key(&home.join(".ssh"), "id_ed25519", PASSPHRASE);
    assert!(facts(&git, &home, DENIED).key_encrypted && !facts(&git, &home, DENIED).agent_has_key);
    agent.add(&default, PASSPHRASE);
    assert!(facts(&git, &home, DENIED).agent_has_key);
    drop(agent);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: SYNC-077
#[test]
fn sync_077_git_passes_the_agent_socket_on_to_ssh_and_ssh_never_asks() {
    let d = unique("sock");
    let out = d.join("seen.txt");
    let fake = d.join("fake-ssh.sh");
    crate::test_exec::write_script(&fake, &format!("#!/bin/sh\necho \"$SSH_AUTH_SOCK\" > '{}'\nexit 255\n", out.display()));
    let git = git_with(Path::new("/run/some-agent.sock"), Some(fake.display().to_string()));
    let vault = d.join("vault");
    std::fs::create_dir_all(&vault).unwrap();
    git.ok(&vault, &[OsStr::new("init"), OsStr::new("-q")]).unwrap();
    // git runs the ssh command with the environment of the Git, which has the agent's socket
    let _ = git.run(&vault, &[OsStr::new("ls-remote"), OsStr::new("ssh://git@127.0.0.1:1/o/r.git")]).unwrap();
    assert_eq!(std::fs::read_to_string(&out).unwrap().trim(), "/run/some-agent.sock");
    // the command of a chosen key never prompts for a passphrase or a host
    let cmd = ssh::ssh_command(Some(Path::new("/k/id")), false).unwrap();
    assert!(cmd.contains("BatchMode=yes") && !cmd.contains("passphrase"), "{cmd}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A web server that answers every request like a proxy that wants a browser sign-in: a redirect
/// to `/login` (and `/login` itself), or the login page for every path. It runs until the test ends.
fn login_proxy(redirect: bool) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut head = [0u8; 2048];
            let n = stream.read(&mut head).unwrap_or(0);
            let first = String::from_utf8_lossy(&head[..n]).lines().next().unwrap_or("").to_string();
            let page = "<html><body>Sign in to continue</body></html>";
            let reply = if redirect && !first.contains(" /login") {
                "HTTP/1.1 302 Found\r\nLocation: /login\r\nContent-Type: text/html\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
            } else {
                format!("HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len())
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}/ada/notes.git")
}

// spec: SYNC-079
#[test]
fn sync_079_a_login_page_instead_of_git_is_told_from_the_real_output_of_git() {
    let d = unique("login");
    let git = ui_git(&d);
    let v = d.join("vault");
    std::fs::create_dir_all(&v).unwrap();
    git.ok(&v, &[OsStr::new("init"), OsStr::new("-q")]).unwrap();
    git.ok(&v, &[OsStr::new("-c"), OsStr::new("user.name=a"), OsStr::new("-c"), OsStr::new("user.email=a@b"), OsStr::new("commit"), OsStr::new("-q"), OsStr::new("--allow-empty"), OsStr::new("-m"), OsStr::new("x")]).unwrap();
    // a redirect to a sign-in page stops the first request
    git.ok(&v, &[OsStr::new("remote"), OsStr::new("add"), OsStr::new("origin"), OsStr::new(&login_proxy(true))]).unwrap();
    let raw = super::test_sync::inspect(&git, &v).unwrap_err();
    assert_eq!(diagnose::diagnose(&raw, &Facts::default()), Problem::LoginPage, "{raw}");
    // an HTML page for every path looks like an empty repository to a fetch, and the push says it
    git.ok(&v, &[OsStr::new("remote"), OsStr::new("set-url"), OsStr::new("origin"), OsStr::new(&login_proxy(false))]).unwrap();
    let pushed = git.run(&v, &[OsStr::new("push"), OsStr::new("origin"), OsStr::new("HEAD:refs/heads/main")]).unwrap();
    assert!(!pushed.ok);
    assert_eq!(diagnose::diagnose(&pushed.stderr, &Facts::default()), Problem::LoginPage, "{}", pushed.stderr);
    let _ = std::fs::remove_dir_all(&d);
}
