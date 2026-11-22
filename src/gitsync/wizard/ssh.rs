//! SSH keys for the wizard (SYNC-056): the keys that are there, a new one made with
//! `ssh-keygen`, and the `ssh` command git runs with it.

use crate::gitsync::Git;
use std::path::{Path, PathBuf};
use std::process::Stdio;

/// A key pair on this computer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    /// The private key file (never read by Shale).
    pub private: PathBuf,
    /// The public key, one line: what the person adds on the server.
    pub public: String,
}

impl Key {
    /// The file's name, for the list.
    pub fn name(&self) -> String {
        self.private.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// `~/.ssh`.
pub fn dir_of(home: &Path) -> PathBuf {
    home.join(".ssh")
}

fn read_public(private: &Path) -> Option<Key> {
    let mut p = private.as_os_str().to_os_string();
    p.push(".pub");
    let text = crate::fsutil::read_to_string(PathBuf::from(p)).ok()?;
    let line = text.lines().next()?.trim().to_string();
    (line.starts_with("ssh-") || line.starts_with("ecdsa-") || line.starts_with("sk-")).then(|| Key { private: private.to_path_buf(), public: line })
}

/// The key pairs of `~/.ssh` that git would use or that Shale made: `id_ed25519`, `id_ecdsa`,
/// `id_rsa` and `shale_*`, each with its `.pub` file.
pub fn existing(home: &Path) -> Vec<Key> {
    let dir = dir_of(home);
    let mut out: Vec<Key> = ["id_ed25519", "id_ecdsa", "id_rsa"].iter().filter_map(|n| read_public(&dir.join(n))).collect();
    let mut shale: Vec<PathBuf> =
        std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("shale_") && !n.to_string_lossy().ends_with(".pub"))).collect();
    shale.sort();
    out.extend(shale.iter().filter_map(|p| read_public(p)));
    out
}

/// A file name for a new key of this host: `shale_<host>`, with a number when it is taken.
pub fn new_key_path(home: &Path, host: &str) -> PathBuf {
    let host: String = host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    let dir = dir_of(home);
    let taken = |p: &Path| p.exists() || PathBuf::from(format!("{}.pub", p.display())).exists();
    let first = dir.join(format!("shale_{host}"));
    if !taken(&first) {
        return first;
    }
    (2..).map(|n| dir.join(format!("shale_{host}_{n}"))).find(|p| !taken(p)).unwrap_or(first)
}

/// Make a new ed25519 key without a passphrase (ADR-0029) and return it. Never overwrites a file.
pub fn generate(git: &Git, home: &Path, host: &str, computer: &str) -> Result<Key, String> {
    let dir = dir_of(home);
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }
    let path = new_key_path(home, host);
    let comment = format!("Shale on {computer}");
    let mut c = crate::program::command("ssh-keygen");
    c.args(["-q", "-t", "ed25519", "-N", "", "-C", &comment, "-f"]).arg(&path).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    // the selected tool set's folders (the managed git brings its own ssh-keygen on Windows)
    if let Some((_, v)) = git.env.iter().find(|(k, _)| k == "PATH") {
        c.env("PATH", v);
    }
    let o = c.output().map_err(|e| format!("could not start ssh-keygen ({e}); is OpenSSH installed?"))?;
    if !o.status.success() {
        return Err(format!("ssh-keygen failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    read_public(&path).ok_or_else(|| "ssh-keygen made no public key".into())
}

/// The `ssh` command git runs (`core.sshCommand`, SYNC-056): the chosen key only, no questions
/// asked, and for the test sync a host seen for the first time is trusted and remembered
/// (`accept-new`; a changed key is still refused). None when the path cannot be quoted safely.
pub fn ssh_command(key: Option<&Path>, accept_new: bool) -> Option<String> {
    let mut cmd = String::from("ssh");
    if let Some(k) = key {
        let text = k.to_string_lossy();
        if text.contains(['\'', '\n', '\r', '\0']) {
            return None;
        }
        cmd.push_str(&format!(" -i '{text}' -o IdentitiesOnly=yes"));
    }
    cmd.push_str(" -o BatchMode=yes");
    if accept_new {
        cmd.push_str(" -o StrictHostKeyChecking=accept-new");
    }
    Some(cmd)
}
