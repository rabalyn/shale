//! Moving a local-only history into the vault's new `.git` when a server is connected (SYNC-072,
//! SYNC-073, ADR-0028 amendment of 2026-11-22). The reverse of [`super::moveout`].
//!
//! The git directory `<history root>/<vault id>` is copied to `<vault>/.git.moving`, its refs and
//! `HEAD` must equal the original's and `git fsck` must find nothing wrong, it is renamed to
//! `<vault>/.git`, made a normal repository (`core.bare=false`, no `core.worktree`) in which
//! `git status` must work, and only then is the old git directory removed. Any failure before that
//! removes the copy and leaves the vault without `.git` and the local history as it was. It runs
//! git, so it belongs in a background job (JOB-001).

use super::history::{local_git_dir, vault_id, WORKTREE_FILE};
use super::moveout::{copy_dir, fingerprint, lock_file_in};
use super::{Git, TreeLock};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The git directory of the vault's local-only history, if there is one (it has a `HEAD`) and the
/// vault has no `.git` yet. Writes nothing into the vault.
pub fn local_history_of(vault: &Path, history_root: &Path) -> Option<PathBuf> {
    if super::is_repo(vault) {
        return None;
    }
    // a vault without an ID has no history; looking must not give it one
    vault_id(vault)?;
    let dir = local_git_dir(vault, history_root).ok()?;
    dir.join("HEAD").is_file().then_some(dir)
}

/// Remove a folder in two steps, so that a removal that fails halfway never leaves `path` itself
/// half there: rename it aside, then delete.
fn remove_dir(path: &Path, aside: &Path) -> Result<(), String> {
    let _ = std::fs::remove_dir_all(aside);
    crate::fsutil::rename_noreplace(path, aside).map_err(|e| format!("could not remove {}: {e}", path.display()))?;
    std::fs::remove_dir_all(aside).map_err(|e| format!("could not delete {}: {e}", aside.display()))
}

/// Move the local history of `vault` into `<vault>/.git` (SYNC-072). `Ok(None)`: there was nothing
/// to move (no local history, or the vault has `.git`). `Ok(Some(old))`: it moved; `old` is the
/// git directory that was removed. `tree` is held for the move, which gives up after `wait`.
pub fn move_in(git: &Git, vault: &Path, history_root: &Path, tree: &TreeLock, wait: Duration) -> Result<Option<PathBuf>, String> {
    if local_history_of(vault, history_root).is_none() {
        return Ok(None);
    }
    let _tree = tree.hold_for(wait).ok_or("A sync or a snapshot is still running; try again in a moment")?;
    // the lock was taken: look again, something may have changed before it
    let Some(source) = local_history_of(vault, history_root) else { return Ok(None) };
    if let Some(l) = lock_file_in(&source) {
        return Err(format!("Another program is working in the history ({} exists); try again when it is done.", l.display()));
    }
    let (dot_git, moving) = (vault.join(".git"), vault.join(".git.moving"));
    let _ = std::fs::remove_dir_all(&moving);
    if let Err(e) = copy_check_and_place(git, vault, &source, &moving) {
        // the vault keeps no half repository: neither the copy nor a `.git` that did not pass
        let _ = std::fs::remove_dir_all(&moving);
        if dot_git.exists() {
            let _ = remove_dir(&dot_git, &vault.join(".git.shale-removing"));
        }
        return Err(e);
    }
    // the copy is the vault's repository now: the old folder can go. A failure to delete it only
    // leaves a folder in the data folder behind; the versions are in `.git`
    let _ = remove_dir(&source, &PathBuf::from(format!("{}.moved", source.display())));
    Ok(Some(source))
}

fn copy_check_and_place(git: &Git, vault: &Path, source: &Path, moving: &Path) -> Result<(), String> {
    copy_dir(source, moving)?;
    // what belongs to the local place only
    let _ = std::fs::remove_file(moving.join(WORKTREE_FILE));
    let _ = std::fs::remove_file(moving.join(super::history::thin::STAMP_FILE));
    let want = fingerprint(git, vault, Some(source))?;
    let got = fingerprint(git, vault, Some(moving))?;
    if want != got {
        return Err("the copy of the history does not match the original (its branches differ), so nothing was moved".into());
    }
    let mut dir_arg = OsString::from("--git-dir=");
    dir_arg.push(moving);
    git.ok(vault, &[dir_arg.as_os_str(), OsStr::new("fsck"), OsStr::new("--no-dangling")]).map_err(|e| format!("the copy of the history did not pass the check ({e}), so nothing was moved"))?;
    crate::fsutil::rename_noreplace(moving, &vault.join(".git")).map_err(|e| format!("could not put the history into the vault ({}): {e}", vault.join(".git").display()))?;
    // now it is the vault's own repository: git finds the work tree by itself
    git.ok(vault, &[OsStr::new("config"), OsStr::new("core.bare"), OsStr::new("false")])?;
    let unset = git.run(vault, &[OsStr::new("config"), OsStr::new("--unset"), OsStr::new("core.worktree")])?;
    // exit status 5: the key was not there
    if !unset.ok && !unset.stderr.is_empty() {
        return Err(format!("git config --unset core.worktree: {}", unset.stderr));
    }
    git.ok(vault, &[OsStr::new("status"), OsStr::new("--porcelain")]).map_err(|e| format!("the history in the vault did not work ({e}), so nothing was moved"))?;
    Ok(())
}
