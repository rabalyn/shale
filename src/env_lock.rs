//! The machine-wide lock of the podman test stacks (DEV-034, #420), the Rust side of
//! `tests/env-lock.sh`. The stacks (`shale-mail-env`, `shale-caldav-env`, the image `shale-tex-env`)
//! have fixed names per machine, so two checkouts must not use them at the same time. Every
//! ignored env test calls [`hold`] first; it takes the one lock file for the life of the test
//! process (a `static`, released by the OS at exit), so `cargo test -- --ignored mail_env_ …` in
//! one worktree waits while `scripts/land.sh` or another worktree runs its suites.
//!
//! The protocol is the shell's: the holder writes a random token into the lock file and exports it
//! as `SHALE_ENV_LOCK_TOKEN`; a process started by a holder trusts it only when the file holds
//! exactly that token and the lock is held right now, so it does not wait for its own parent (and a
//! stale or inherited token never skips the lock). A holder here exports the token too, so the
//! `mail-env.sh` and `tex-env.sh` the tests start do not wait for the test process.

use std::fs::{File, OpenOptions, TryLockError};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub const TOKEN_VAR: &str = "SHALE_ENV_LOCK_TOKEN";
/// Overrides the lock file (the tests of the lock use their own).
pub const FILE_VAR: &str = "SHALE_ENV_LOCK_FILE";

/// One file per user and machine, outside every checkout: `$XDG_RUNTIME_DIR` is per user and goes
/// at logout, the `/tmp` fallback carries the user's name (as `tests/env-lock.sh` does).
pub fn lock_path() -> PathBuf {
    if let Some(p) = std::env::var_os(FILE_VAR).filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    let dir = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    let user = std::env::var("USER").ok().filter(|u| !u.is_empty()).or_else(|| std::env::var("LOGNAME").ok().filter(|u| !u.is_empty())).unwrap_or_else(|| "user".into());
    dir.join(format!("shale-env-stacks-{user}.lock"))
}

/// Open without truncating: the token of a holder we may be waiting for stays readable.
fn open(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

/// Whether `token` (the value a parent exported) is the token of a holder that holds the lock now.
pub fn held_by_parent(path: &Path, token: Option<&str>) -> bool {
    let Some(token) = token.filter(|t| !t.is_empty()) else { return false };
    let Ok(text) = std::fs::read_to_string(path) else { return false };
    if text.trim_end() != token {
        return false;
    }
    match open(path) {
        // free: the holder is gone, the token is stale (the lock is released again when `file` drops)
        Ok(file) => matches!(file.try_lock(), Err(TryLockError::WouldBlock)),
        Err(_) => false,
    }
}

/// Take the lock (waiting without a timeout, and saying so once), write a fresh token into the
/// file and return both; `None` when a parent holds it already.
pub fn acquire(path: &Path, parent_token: Option<&str>) -> Option<(File, String)> {
    if held_by_parent(path, parent_token) {
        return None;
    }
    let mut file = open(path).unwrap_or_else(|e| panic!("cannot open the lock file {}: {e}", path.display()));
    match file.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            eprintln!("waiting for the podman test stacks, used by another checkout…");
            file.lock().unwrap_or_else(|e| panic!("cannot lock {}: {e}", path.display()));
        }
        Err(TryLockError::Error(e)) => panic!("cannot lock {}: {e}", path.display()),
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let token = format!("{}-{nanos:x}", std::process::id());
    file.set_len(0).and_then(|()| writeln!(file, "{token}")).unwrap_or_else(|e| panic!("cannot write {}: {e}", path.display()));
    Some((file, token))
}

static HELD: OnceLock<Option<File>> = OnceLock::new();

/// Every ignored env test calls this first. The first call takes the lock for the whole test
/// process and exports the token for the scripts the tests start; later calls (other tests, other
/// threads) wait for that and return.
pub fn hold() {
    HELD.get_or_init(|| {
        let parent = std::env::var(TOKEN_VAR).ok();
        acquire(&lock_path(), parent.as_deref()).map(|(file, token)| {
            // before any test starts a child; every env test is behind this `get_or_init`
            std::env::set_var(TOKEN_VAR, token);
            file
        })
    });
}
