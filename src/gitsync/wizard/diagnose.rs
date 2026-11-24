//! Why a sign-in failed (SYNC-077…SYNC-083): git's own error text, read together with the facts of a
//! few cheap checks that git does not tell (does the SSH key have a passphrase, does an
//! ssh-agent run, does it hold the key). The classifier is a pure function of both; the checks
//! are small programs run as argument lists, never through a shell, and never ask for a passphrase
//! (Shale neither asks for nor keeps one, like MAIL-171 for GnuPG).

use super::test_sync::{classify, Problem};
use crate::gitsync::Git;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// Does an ssh-agent answer?
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Agent {
    /// Not looked at.
    #[default]
    Unknown,
    /// No agent runs, or it cannot be reached.
    Unreachable,
    Running,
}

/// What the cheap checks found about the SSH sign-in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// A private key that ssh would offer has a passphrase.
    pub key_encrypted: bool,
    pub agent: Agent,
    /// The agent holds every key that has a passphrase (false without such a key).
    pub agent_has_key: bool,
}

/// What git's text and the facts say (SYNC-077…SYNC-081).
pub fn diagnose(error: &str, facts: &Facts) -> Problem {
    match classify(error) {
        Problem::SshKey if facts.key_encrypted && !facts.agent_has_key => {
            if facts.agent == Agent::Running {
                Problem::KeyLocked
            } else {
                Problem::NoAgent
            }
        }
        p => p,
    }
}

/// Does this private key have a passphrase? `ssh-keygen -y -P ""` prints the public key of a key
/// that has none and fails with "incorrect passphrase" for one that has; any other failure (no such
/// file, not a key) is None.
pub fn key_encrypted(git: &Git, key: &Path) -> Option<bool> {
    let mut c = crate::program::command("ssh-keygen");
    c.args(["-y", "-P", "", "-f"]).arg(key).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    path_of(git, &mut c);
    let o = c.output().ok()?;
    if o.status.success() {
        return Some(false);
    }
    String::from_utf8_lossy(&o.stderr).to_ascii_lowercase().contains("passphrase").then_some(true)
}

/// The agent's keys, one public key per line (`ssh-add -L`), with the environment git runs with
/// (`SSH_AUTH_SOCK`). None: no agent, or it cannot be reached. An agent with no keys is `Some("")`.
pub fn agent_keys(git: &Git) -> Option<String> {
    let mut c = crate::program::command("ssh-add");
    c.arg("-L").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    path_of(git, &mut c);
    if let Some((_, v)) = git.env.iter().find(|(k, _)| k == "SSH_AUTH_SOCK") {
        c.env("SSH_AUTH_SOCK", v);
    }
    let o = c.output().ok()?;
    match o.status.code() {
        Some(0) => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        Some(1) => Some(String::new()), // "The agent has no identities."
        _ => None,
    }
}

/// The selected tool set's folders (the managed git brings its own ssh tools on Windows).
fn path_of(git: &Git, c: &mut std::process::Command) {
    if let Some((_, v)) = git.env.iter().find(|(k, _)| k == "PATH") {
        c.env("PATH", v);
    }
}

/// Does the agent's list hold this public key (`ssh-ed25519 AAAA… comment`)? Type and key count, the comment does not.
pub fn agent_holds(list: &str, public: &str) -> bool {
    let key = |l: &str| {
        let mut f = l.split_whitespace();
        Some((f.next()?.to_string(), f.next()?.to_string()))
    };
    key(public).is_some_and(|want| list.lines().filter_map(key).any(|k| k == want))
}

/// The private key named by `-i` in an `ssh` command (`core.sshCommand`, `GIT_SSH_COMMAND`), quoted or not.
pub fn key_of_command(cmd: &str) -> Option<PathBuf> {
    let rest = &cmd[cmd.find("-i ")? + 3..];
    let rest = rest.trim_start();
    let path = match rest.strip_prefix('\'') {
        Some(quoted) => quoted.split('\'').next()?,
        None => rest.split_whitespace().next()?,
    };
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// The private keys ssh would offer for this vault: the one of the ssh command, or else the
/// default keys of `~/.ssh` that exist.
fn keys_offered(git: &Git, vault: &Path, home: &Path) -> Vec<PathBuf> {
    let env_cmd = git.env.iter().find(|(k, _)| k == "GIT_SSH_COMMAND").map(|(_, v)| v.to_string_lossy().into_owned());
    let cmd = env_cmd.or_else(|| git.run(vault, &[std::ffi::OsStr::new("config"), std::ffi::OsStr::new("core.sshCommand")]).ok().filter(|o| o.ok).map(|o| o.text()));
    if let Some(k) = cmd.as_deref().and_then(key_of_command) {
        return vec![k];
    }
    let dir = super::ssh::dir_of(home);
    ["id_ed25519", "id_ecdsa", "id_rsa"].iter().map(|n| dir.join(n)).filter(|p| p.is_file()).collect()
}

/// The facts for a failed sync of `vault`: only when git's text says the server did not accept the SSH key (the
/// other cases need no check), so a sync that fails for any other reason costs nothing.
pub fn gather(git: &Git, vault: &Path, home: &Path, error: &str) -> Facts {
    if classify(error) != Problem::SshKey {
        return Facts::default();
    }
    let mut facts = Facts::default();
    let encrypted: Vec<PathBuf> = keys_offered(git, vault, home).into_iter().filter(|k| key_encrypted(git, k) == Some(true)).collect();
    if encrypted.is_empty() {
        return facts;
    }
    facts.key_encrypted = true;
    let list = agent_keys(git);
    facts.agent = if list.is_some() { Agent::Running } else { Agent::Unreachable };
    facts.agent_has_key = list.is_some_and(|l| {
        encrypted.iter().all(|k| {
            let mut p = k.as_os_str().to_os_string();
            p.push(".pub");
            crate::fsutil::read_to_string(PathBuf::from(p)).is_ok_and(|public| agent_holds(&l, public.lines().next().unwrap_or("")))
        })
    });
    facts
}
