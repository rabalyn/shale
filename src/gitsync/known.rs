//! Which repositories Shale already knows (SYNC-076, rabalyn/shale#585): one it made itself
//! (SYNC-004) and one the person has been asked about. The mark is a small file inside the git
//! directory, so it is never committed or synced, and a vault that is copied without its `.git`
//! asks again.

use std::path::{Path, PathBuf};

/// The name of the mark in the git directory.
const MARK: &str = "shale-known";

/// The git directory of a vault: `<vault>/.git`, or the folder a `.git` file points to (a linked
/// worktree or a submodule). None: the vault has no `.git`.
fn git_dir(vault: &Path) -> Option<PathBuf> {
    let dot = vault.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = crate::fsutil::read_to_string(&dot).ok()?;
    let target = Path::new(text.trim().strip_prefix("gitdir:")?.trim());
    let dir = if target.is_absolute() { target.to_path_buf() } else { vault.join(target) };
    dir.is_dir().then_some(dir)
}

/// Shale made this repository or has asked about it: no question when the vault opens.
pub fn is_known(vault: &Path) -> bool {
    git_dir(vault).is_some_and(|d| d.join(MARK).exists())
}

/// Remember that Shale knows this repository. Without a git directory it writes nothing.
pub fn mark_known(vault: &Path) -> std::io::Result<()> {
    match git_dir(vault) {
        Some(d) => crate::fsutil::write_atomic(&d.join(MARK), b"Shale has made this repository or asked about it\n"),
        None => Ok(()),
    }
}
