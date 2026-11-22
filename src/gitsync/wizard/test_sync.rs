//! The last step of the wizard (SYNC-057, SYNC-058): what the remote holds before the first sync,
//! and the result of the test sync in plain words.

use crate::gitsync::{Git, Report, REMOTE};
use std::ffi::OsStr;
use std::path::Path;

/// What the server's repository holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remote {
    Empty,
    HasNotes,
}

/// Ask the server what is in `origin` (`git ls-remote`): also the first real test of the address and
/// the sign-in, before anything is merged. The error is git's own message.
pub fn inspect(git: &Git, vault: &Path) -> Result<Remote, String> {
    let o = git.run(vault, &[OsStr::new("ls-remote"), OsStr::new("--heads"), OsStr::new(REMOTE)])?;
    if !o.ok {
        return Err(if o.stderr.is_empty() { "git ls-remote failed".into() } else { o.stderr });
    }
    Ok(if o.text().is_empty() { Remote::Empty } else { Remote::HasNotes })
}

/// Does the vault hold any note or drawing (the first few thousand files are looked at)?
pub fn vault_has_notes(vault: &Path) -> bool {
    fn walk(dir: &Path, budget: &mut usize) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else { return false };
        for e in entries.flatten() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let Ok(t) = e.file_type() else { continue };
            if t.is_dir() {
                if walk(&e.path(), budget) {
                    return true;
                }
            } else if crate::gitsync::history::tracked(Path::new(&name)) {
                return true;
            }
        }
        false
    }
    walk(vault, &mut 5000)
}

/// What the first sync will do, in words (SYNC-057).
pub fn what_happens(remote: Remote, vault_has_notes: bool) -> &'static str {
    match (remote, vault_has_notes) {
        (Remote::Empty, true) => "The repository on the server is empty, so your notes will be uploaded to it.",
        (Remote::Empty, false) => "The repository on the server is empty and so is this vault, so there is nothing to copy yet. Notes you write from now on are uploaded.",
        (Remote::HasNotes, false) => "The repository on the server already has notes. They will be downloaded into this vault.",
        (Remote::HasNotes, true) => {
            "Both this vault and the repository on the server have notes. Shale merges them: nothing is deleted, and when the same note differs in both places both versions are kept, one as a copy marked “conflict”."
        }
    }
}

/// What the move of a local history into the vault means for the first sync (SYNC-075).
pub fn history_moved() -> &'static str {
    "The versions Shale kept of your notes on this computer have moved into the vault, so they stay in the history panel. The first sync sends them to the server together with your notes."
}

/// What went wrong, told apart so the wizard can say what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Wrong or missing user name or token (or an SSH key the server does not know: `SshKey`).
    Sign,
    NoNetwork,
    NoRepository,
    SshKey,
    HostKey,
    Certificate,
    Other(String),
}

/// Read git's error text.
pub fn classify(error: &str) -> Problem {
    let e = error.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| e.contains(w));
    if has(&["permission denied (publickey", "no matching host key type", "sign_and_send_pubkey"]) {
        Problem::SshKey
    } else if has(&["host key verification failed", "remote host identification has changed"]) {
        Problem::HostKey
    } else if has(&["ssl certificate", "certificate verify failed", "unable to get local issuer", "self-signed", "self signed", "ssl: no alternative certificate"]) && !has(&["authentication failed"]) {
        Problem::Certificate
    } else if has(&[
        "authentication failed",
        "invalid username or password",
        "invalid credentials",
        "could not read password",
        "could not read username",
        "terminal prompts disabled",
        "http basic: access denied",
        "returned error: 401",
        "returned error: 403",
        "unauthorized",
    ]) {
        Problem::Sign
    } else if has(&[
        "could not resolve host",
        "failed to connect",
        "connection refused",
        "network is unreachable",
        "timed out",
        "could not connect",
        "temporary failure in name resolution",
        "no route to host",
        "name or service not known",
        "connection reset",
        "could not resolve hostname",
    ]) {
        Problem::NoNetwork
    } else if has(&["repository not found", "not found", "does not appear to be a git repository", "returned error: 404", "does not exist"]) {
        Problem::NoRepository
    } else {
        Problem::Other(error.trim().to_string())
    }
}

impl Problem {
    /// In plain words, with what to do.
    pub fn words(&self) -> String {
        match self {
            Problem::Sign => "The server did not accept the sign-in: the user name or the token is wrong, or the token has expired or may not use repositories. Go back and enter the token again.".into(),
            Problem::NoNetwork => "Shale could not reach the server. Check your internet connection and the address of the server, then try again.".into(),
            Problem::NoRepository => "The server has no repository at this address, or your account may not use it. Go back and pick or create the repository.".into(),
            Problem::SshKey => "The server did not accept your SSH key. Add the public key to your account on the server (the page is linked in the previous step), then try again.".into(),
            Problem::HostKey => "The server's SSH key is not the one this computer remembers for it, so Shale stopped. If you did not expect a change, do not go on.".into(),
            Problem::Certificate => "The server's certificate could not be checked (it may be self-signed or expired). Shale does not go on without a valid certificate.".into(),
            Problem::Other(e) => format!("The sync did not work: {e}"),
        }
    }
}

/// The success text: what the first sync did.
pub fn worked(report: &Report) -> String {
    let mut parts = vec![];
    if report.committed > 0 {
        parts.push(format!("{} note(s) were saved to the history", report.committed));
    }
    let incoming = report.changed.len().saturating_sub(report.conflicts.len());
    if incoming > 0 {
        parts.push(format!("{incoming} file(s) were downloaded"));
    }
    if !report.conflicts.is_empty() {
        parts.push(format!("{} note(s) were different in both places, so both versions were kept", report.conflicts.len()));
    }
    if report.pushed {
        parts.push("your notes are on the server".into());
    }
    if parts.is_empty() {
        "It worked: this vault and the server are the same.".into()
    } else {
        format!("It worked: {}.", parts.join("; "))
    }
}
