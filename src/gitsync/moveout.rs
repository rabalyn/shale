//! Moving a vault's own `.git` out of a cloud-synced or network folder into the private place of
//! the local history (SYNC-062, SYNC-063, ADR-0029).
//!
//! The move is a copy that is checked before anything is removed: the git directory is copied to
//! `<history root>/<vault id>.moving`, its refs and `HEAD` must equal the original's and `git fsck`
//! must find nothing wrong, it is renamed to `<history root>/<vault id>`, and only then is
//! `<vault>/.git` removed. Any failure before the last step removes the copy and leaves the vault
//! as it was. It runs git, so it belongs in a background job (JOB-001).

use super::history::{local_git_dir, WORKTREE_FILE};
use super::{ensure_idle, Git, TreeLock};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the move waits for a sync or a snapshot that holds the vault's tree lock.
pub const TREE_WAIT: Duration = Duration::from_secs(10);

/// A lock file inside a git directory means some git process is working in it.
pub(super) fn lock_file_in(dir: &Path) -> Option<PathBuf> {
    for e in std::fs::read_dir(dir).ok()?.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(t) = e.file_type() else { continue };
        if t.is_dir() {
            // the object store holds no lock files, and is large
            if name != "objects" {
                if let Some(l) = lock_file_in(&p) {
                    return Some(l);
                }
            }
        } else if name.ends_with(".lock") {
            return Some(p);
        }
    }
    None
}

/// Copy a git directory. A link inside it is refused (git makes none, and a link could lead out).
pub(super) fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| format!("could not make {}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("could not read {}: {e}", from.display()))? {
        let e = e.map_err(|e| e.to_string())?;
        let (src, dst) = (e.path(), to.join(e.file_name()));
        let t = e.file_type().map_err(|e| e.to_string())?;
        if t.is_symlink() {
            return Err(format!("{} is a link; Shale does not move such a repository", src.display()));
        } else if t.is_dir() {
            copy_dir(&src, &dst)?;
        } else {
            crate::fsutil::copy_new(&src, &dst).map_err(|e| format!("could not copy {}: {e}", src.display()))?;
        }
    }
    Ok(())
}

/// What a git directory says: `HEAD` and every ref, for the comparison of the original and the copy.
pub(super) fn fingerprint(git: &Git, vault: &Path, git_dir: Option<&Path>) -> Result<String, String> {
    let dir_arg = git_dir.map(|d| {
        let mut a = std::ffi::OsString::from("--git-dir=");
        a.push(d);
        a
    });
    let run = |tail: &[&str]| -> Result<String, String> {
        let mut all: Vec<&OsStr> = dir_arg.iter().map(|a| a.as_os_str()).collect();
        all.extend(tail.iter().map(OsStr::new));
        git.ok(vault, &all).map(|o| o.text())
    };
    let refs = run(&["for-each-ref", "--format=%(refname) %(objectname)"])?;
    let head = run(&["symbolic-ref", "-q", "HEAD"]).unwrap_or_default();
    Ok(format!("{head}\n{refs}"))
}

/// Why the vault's repository cannot be moved out (None: it can). Cheap: no copy is made.
pub fn refusal(git: &Git, vault: &Path) -> Option<String> {
    let dot_git = vault.join(".git");
    let meta = std::fs::symlink_metadata(&dot_git).ok()?;
    if !meta.is_dir() {
        return Some("The .git of this vault is not a folder (a link or a file that points elsewhere), so Shale leaves it alone.".into());
    }
    match git.run(vault, &[OsStr::new("remote")]) {
        Ok(o) if o.ok && o.text().is_empty() => {}
        Ok(o) if o.ok => return Some("This vault is connected to a server, so its repository stays where it is.".into()),
        Ok(o) => return Some(format!("git could not read the repository: {}", o.stderr)),
        Err(e) => return Some(e),
    }
    if let Err(e) = ensure_idle(git, vault) {
        return Some(e);
    }
    if let Some(l) = lock_file_in(&dot_git) {
        return Some(format!("Another program is working in the repository ({} exists); try again when it is done.", l.display()));
    }
    None
}

/// Move `<vault>/.git` to `<history_root>/<vault id>` (SYNC-063). Returns the new git directory.
/// `tree` is held while the repository is copied and removed; the move gives up after `wait` for it.
pub fn move_out(git: &Git, vault: &Path, history_root: &Path, tree: &TreeLock, wait: Duration) -> Result<PathBuf, String> {
    if let Some(why) = refusal(git, vault) {
        return Err(why);
    }
    let _tree = tree.hold_for(wait).ok_or("A sync or a snapshot is still running; try again in a moment")?;
    // the lock was taken: look again, something may have started before it
    if let Some(why) = refusal(git, vault) {
        return Err(why);
    }
    let target = local_git_dir(vault, history_root)?;
    if target.exists() {
        return Err(format!("This vault already has a history on this computer ({}), so nothing was moved", target.display()));
    }
    let moving = PathBuf::from(format!("{}.moving", target.display()));
    let _ = std::fs::remove_dir_all(&moving);
    if let Some(parent) = moving.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
    }
    let copied = copy_and_check(git, vault, &moving);
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&moving);
        return Err(e);
    }
    if let Err(e) = crate::fsutil::rename_noreplace(&moving, &target) {
        let _ = std::fs::remove_dir_all(&moving);
        return Err(format!("could not put the copy in place ({}): {e}", target.display()));
    }
    // the copy is in place and checked: now the vault's own repository can go. A rename first, so a
    // removal that fails halfway never leaves a vault with a broken `.git`
    let dot_git = vault.join(".git");
    let removing = vault.join(".git.shale-removing");
    let _ = std::fs::remove_dir_all(&removing);
    crate::fsutil::rename_noreplace(&dot_git, &removing).map_err(|e| format!("the history is on this computer now ({}), but the .git folder of the vault could not be removed: {e}", target.display()))?;
    std::fs::remove_dir_all(&removing).map_err(|e| format!("the history is on this computer now ({}), but the leftover folder {} of the vault could not be deleted: {e}", target.display(), removing.display()))?;
    Ok(target)
}

/// Copy `<vault>/.git` to `moving` and check the copy; write the `worktree` file.
fn copy_and_check(git: &Git, vault: &Path, moving: &Path) -> Result<(), String> {
    copy_dir(&vault.join(".git"), moving)?;
    let want = fingerprint(git, vault, None)?;
    let got = fingerprint(git, vault, Some(moving))?;
    if want != got {
        return Err("the copy of the repository does not match the original (its branches differ), so nothing was moved".into());
    }
    let dir_arg = {
        let mut a = std::ffi::OsString::from("--git-dir=");
        a.push(moving);
        a
    };
    git.ok(vault, &[dir_arg.as_os_str(), OsStr::new("fsck"), OsStr::new("--no-dangling")]).map_err(|e| format!("the copy of the repository did not pass the check ({e}), so nothing was moved"))?;
    let canon = vault.canonicalize().unwrap_or_else(|_| vault.to_path_buf());
    crate::fsutil::write_atomic(&moving.join(WORKTREE_FILE), canon.to_string_lossy().as_bytes()).map_err(|e| format!("could not write {}: {e}", moving.join(WORKTREE_FILE).display()))?;
    Ok(())
}
