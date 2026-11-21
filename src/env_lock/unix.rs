//! The Unix side of the lock (see the parent module): `File::lock` is an advisory `flock`, so the
//! holder can write its token into the locked file and a waiting process can read it.
// Already test-only through the parent; stated here too because the DATA-010 scan in fsutil.rs reads this file alone.
#![cfg(test)]

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
