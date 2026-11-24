//! Publishing a vault (SITE-062…SITE-076): the export of the chosen notes is written into a git
//! repository of its own (SITE-063), made one commit with no parent (SITE-068) and force-pushed to
//! the publish branch. Nothing of the vault but that tree ever leaves the device, and the vault's
//! own repository is never touched.
//!
//! The order is the one that keeps the remote safe (SITE-065): the site is worked out and written
//! to the local repository, committed, the remote's last commit is read (SITE-074), the media are
//! uploaded to the LFS store (SITE-069) and only then the branch is pushed, in one step. Whatever
//! stops before that (an error, Cancel) leaves the remote's branch as it was. git is the one of
//! git sync (`Git`, SYNC-024) with its askpass helper for tokens (SYNC-054), so credentials,
//! host keys and the words for a sign-in problem are those of git sync (SITE-064, SITE-075).

mod message;
mod process;

#[cfg(test)]
mod env_tests;
#[cfg(test)]
mod tests;

pub use message::{local, Meta};

use super::{plan, rules, write};
use crate::busy::Cancel;
use crate::gitsync::{self, history, lfs, Git};
use crate::media::outside::Library;
use crate::vault::Vault;
use crate::vault_state::PublishSettings;
use process::{ok, run};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Short way to build git argument lists.
macro_rules! a {
    ($($x:expr),* $(,)?) => { &[$(AsRef::<OsStr>::as_ref(&$x)),*] };
}

/// The branch of a publish when the settings name none.
pub const DEFAULT_BRANCH: &str = "main";

/// Why a publish did not end with a push.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The person stopped it; the remote was not changed (SITE-065).
    Cancelled,
    /// The target's last commit is not this vault's: the publish waits for the person to confirm
    /// that this vault takes the target over (SITE-074).
    Takeover(Takeover),
    /// What went wrong, in git's words or Shale's.
    Error(String),
}

impl From<String> for Failure {
    fn from(e: String) -> Failure {
        Failure::Error(e)
    }
}

impl Failure {
    /// The message of an error; `Cancelled` and `Takeover` say it too, for a command line.
    pub fn words(&self) -> String {
        match self {
            Failure::Cancelled => "The publish was stopped; the target was not changed.".into(),
            Failure::Takeover(t) => format!("{} Publish again with --take-over to replace it.", t.words()),
            Failure::Error(e) => e.clone(),
        }
    }
}

/// What the target's last commit says when it is not this vault's (SITE-074).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Takeover {
    pub meta: Meta,
}

impl Takeover {
    /// What is there and what publishing does to it, in plain words.
    pub fn words(&self) -> String {
        let m = &self.meta;
        let by = match (&m.vault_name, &m.vault_id) {
            (Some(name), _) => format!("The vault “{name}” published there"),
            (None, Some(_)) => "Another vault published there".to_string(),
            (None, None) => return "The branch of the target was not made by a publish of Shale; publishing replaces everything on it.".into(),
        };
        let from = match (&m.device, m.time) {
            (Some(d), Some(t)) => format!(" from {d} at {}", local(t)),
            (Some(d), None) => format!(" from {d}"),
            (None, Some(t)) => format!(" at {}", local(t)),
            (None, None) => String::new(),
        };
        format!("{by}{from}; publishing replaces everything on the branch with this vault.")
    }
}

/// What a publish says when it is done (SITE-067).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    pub notes: usize,
    pub media: usize,
    pub links: usize,
    pub plain: usize,
    /// Every warning, among them each off-vault file that did not resolve on this device (SITE-036).
    pub warnings: Vec<String>,
    /// Why the site has no note, when it has none (SITE-001).
    pub empty_because: Option<String>,
}

impl Summary {
    /// The line (SITE-015): how many notes and media files, links, and links that became plain text.
    pub fn line(&self) -> String {
        super::cli::counts("Published", self.notes, self.media, self.links, self.plain)
    }
}

/// What a publish did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub summary: Summary,
    /// The snapshot commit that the branch now points to.
    pub commit: String,
    /// What the commit says about this publish (SITE-072, SITE-073).
    pub meta: Meta,
}

/// What a publish needs to know.
pub struct Request<'a> {
    pub vault: &'a Path,
    /// The media libraries of this device (MEDIA-014).
    pub libraries: &'a [Library],
    /// `<data folder>/shale/vaults` (SITE-063).
    pub base: &'a Path,
    /// This computer's name, for the commit message.
    pub device: &'a str,
    /// The person confirmed that this vault takes the target over (SITE-074).
    pub take_over: bool,
}

/// The publish repository of the vault with this ID (SITE-063).
pub fn repo_dir(base: &Path, id: &str) -> PathBuf {
    base.join(id).join("publish")
}

/// A branch name: git's rules for the common cases, so that a typo is said at once.
pub fn check_branch(branch: &str) -> Result<(), String> {
    let bad = branch.is_empty()
        || branch.starts_with(['-', '/', '.'])
        || branch.ends_with(['/', '.'])
        || branch.ends_with(".lock")
        || branch.contains("..")
        || branch.contains("//")
        || branch.contains("@{")
        || branch.chars().any(|c| c.is_control() || c.is_whitespace() || "~^:?*[\\".contains(c));
    if bad {
        return Err(format!("“{branch}” is not a branch name that git accepts"));
    }
    Ok(())
}

/// The publish remote and branch of the settings (SITE-064): the address by the rules of SYNC-004
/// (a password in it is refused, SEC-010), the branch `main` when none is set.
pub fn target(s: &PublishSettings) -> Result<(String, String), String> {
    let url = s.url.trim();
    if url.is_empty() {
        return Err("No address is set to publish to (Settings → Publishing).".into());
    }
    gitsync::check_url(url)?;
    let branch = if s.branch.trim().is_empty() { DEFAULT_BRANCH } else { s.branch.trim() };
    check_branch(branch)?;
    Ok((url.to_string(), branch.to_string()))
}

/// An address without what does not make it another one: letter case, a trailing slash, `.git`.
fn same_address(a: &str, b: &str) -> bool {
    let plain = |s: &str| s.trim().trim_end_matches('/').trim_end_matches(".git").trim_end_matches('/').to_ascii_lowercase();
    plain(a) == plain(b)
}

/// The publish branch is never the branch the vault syncs with: a force-push there would replace
/// the history of the notes.
fn refuse_the_sync_remote(git: &Git, vault: &Path, url: &str) -> Result<(), String> {
    match gitsync::remote_url(git, vault) {
        Some(origin) if same_address(&origin, url) => Err("This is the address the vault syncs with. A publish replaces everything on its branch, so it needs a repository of its own.".into()),
        _ => Ok(()),
    }
}

/// The repository of the publish, made when it is not there, with the remote and the branch (SITE-063).
fn prepare_repo(git: &Git, dir: &Path, url: &str, branch: &str, cancel: &Cancel) -> Result<(), Failure> {
    std::fs::create_dir_all(dir).map_err(|e| format!("could not make {}: {e}", dir.display()))?;
    if !dir.join(".git").exists() {
        ok(git, dir, a!["init", "--quiet"], cancel)?;
    }
    ok(git, dir, a!["symbolic-ref", "HEAD", format!("refs/heads/{branch}")], cancel)?;
    let has_origin = run(git, dir, a!["remote", "get-url", gitsync::REMOTE], cancel)?.ok;
    ok(git, dir, a!["remote", if has_origin { "set-url" } else { "add" }, gitsync::REMOTE, url], cancel)?;
    // the manifest of the export (SITE-014) is a file of the local tree, not of the site
    let exclude = dir.join(".git").join("info").join("exclude");
    if let Some(parent) = exclude.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("could not make {}: {e}", parent.display()))?;
    }
    crate::fsutil::write_atomic(&exclude, format!("/{}\n", write::MANIFEST).as_bytes()).map_err(|e| format!("could not write {}: {e}", exclude.display()))?;
    Ok(())
}

/// The work tree holds exactly the site: what an earlier publish left is removed, then the export is written.
fn fill_tree(export: &plan::Export, dir: &Path) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|e| format!("could not read {}: {e}", dir.display()))?;
    for e in entries.flatten() {
        if e.file_name() == ".git" {
            continue;
        }
        let path = e.path();
        let removed = if e.file_type().is_ok_and(|t| t.is_dir()) { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        removed.map_err(|e| format!("could not remove {}: {e}", path.display()))?;
    }
    write::write(export, dir)
}

/// Does a file of the tree go through LFS (the extensions of SYNC-017)?
fn routed_to_lfs(path: &str) -> bool {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    crate::media::media_extensions().any(|m| m == ext)
}

/// What the branch of the target holds now.
struct Tip {
    /// The commit; none when the branch does not exist.
    sha: Option<String>,
    /// What its message says (empty for a commit that no publish made).
    meta: Meta,
}

/// Read the target's last commit (SITE-074, SITE-072): `ls-remote` for the branch, then the one
/// commit (no files) for its message.
fn remote_tip(git: &Git, dir: &Path, branch: &str, cancel: &Cancel) -> Result<Tip, Failure> {
    let spec = format!("refs/heads/{branch}");
    let listing = ok(git, dir, a!["ls-remote", "--heads", gitsync::REMOTE, spec], cancel)?;
    let Some(sha) = listing.text().split_whitespace().next().map(str::to_string) else { return Ok(Tip { sha: None, meta: Meta::default() }) };
    ok(git, dir, a!["fetch", "--quiet", "--no-tags", "--depth=1", "--filter=blob:none", gitsync::REMOTE, spec], cancel)?;
    let message = ok(git, dir, a!["log", "-1", "--format=%B", "FETCH_HEAD"], cancel)?;
    Ok(Tip { sha: Some(sha), meta: message::parse(&String::from_utf8_lossy(&message.stdout)) })
}

/// What the target's branch was last published with (SITE-072): `None` when the branch does not exist.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Last {
    pub meta: Meta,
    /// The commit is a publish of this vault.
    pub this_vault: bool,
}

/// Who published the target's branch last and when (SITE-072), for the settings. Reads the remote only.
pub fn last_published(git: &Git, vault: &Path, base: &Path, cancel: &Cancel) -> Result<Option<Last>, Failure> {
    let settings = rules::read_settings(vault)?;
    let (url, branch) = target(&settings.publish)?;
    let id = history::ensure_vault_id(vault)?;
    let dir = repo_dir(base, &id);
    prepare_repo(git, &dir, &url, &branch, cancel)?;
    let tip = remote_tip(git, &dir, &branch, cancel)?;
    Ok(tip.sha.map(|_| Last { this_vault: tip.meta.vault_id.as_deref() == Some(id.as_str()), meta: tip.meta }))
}

/// Publish the vault (SITE-062): the export, one snapshot commit, the media to LFS, the force-push.
/// `step` is told what happens now (the progress, SITE-065); `cancel` stops it, and a stopped or
/// failed publish leaves the remote as it was.
pub fn publish(git: &Git, req: &Request, cancel: &Cancel, step: &dyn Fn(&str)) -> Result<Published, Failure> {
    let settings = rules::read_settings(req.vault)?;
    if !settings.publish.enabled {
        return Err(Failure::Error("Publishing is off for this vault (Settings → Publishing).".into()));
    }
    let (url, branch) = target(&settings.publish)?;
    refuse_the_sync_remote(git, req.vault, &url)?;
    let id = history::ensure_vault_id(req.vault)?;

    step("Working out the site…");
    let mut vault = Vault::open(req.vault).map_err(|e| format!("could not open {}: {e}", req.vault.display()))?;
    vault.set_libraries(req.libraries);
    let export = plan::build(&vault)?;
    let name = plan::vault_name(&vault);
    if cancel.is_cancelled() {
        return Err(Failure::Cancelled);
    }

    step("Writing the site…");
    let dir = repo_dir(req.base, &id);
    prepare_repo(git, &dir, &url, &branch, cancel)?;
    fill_tree(&export, &dir)?;
    let lfs_files = export.media.iter().filter(|m| routed_to_lfs(m)).count();
    let lfs_installed = git.lfs_version().is_some();
    if lfs_files > 0 && !lfs_installed {
        return Err(Failure::Error(format!("The site has media files that go to Git LFS, but Git LFS is not installed on this computer. {}", lfs::INSTALL_HINT)));
    }
    if lfs_installed {
        lfs::prepare_new(git, &dir)?;
    }

    step("Making the snapshot…");
    let meta = Meta { device: Some(req.device.to_string()), time: Some(crate::calendar::time::now()), vault_id: Some(id.clone()), vault_name: Some(name) };
    let commit = snapshot(git, &dir, &branch, &message::compose(export.notes.len(), &meta), cancel)?;

    step("Looking at the target…");
    let tip = remote_tip(git, &dir, &branch, cancel)?;
    let ours = tip.meta.vault_id.as_deref() == Some(id.as_str());
    if tip.sha.is_some() && !ours && !req.take_over {
        return Err(Failure::Takeover(Takeover { meta: tip.meta }));
    }

    if lfs_files > 0 {
        step("Uploading the media files…");
        ok(git, &dir, a!["lfs", "push", gitsync::REMOTE, format!("refs/heads/{branch}")], cancel)?;
    }

    step("Publishing…");
    // the lease: the branch is replaced only if it is still what was just read, so a publish of
    // another vault in between is not overwritten unseen (SITE-074)
    let lease = format!("--force-with-lease=refs/heads/{branch}:{}", tip.sha.as_deref().unwrap_or(""));
    let spec = format!("refs/heads/{branch}:refs/heads/{branch}");
    ok(git, &dir, a!["push", "--quiet", lease, gitsync::REMOTE, spec], cancel)?;

    let summary = Summary { notes: export.notes.len(), media: export.media.len(), links: export.links, plain: export.plain, warnings: export.warnings, empty_because: export.empty_because };
    Ok(Published { summary, commit, meta })
}

/// Everything of the work tree as one commit with no parent, the branch pointing to it (SITE-068).
fn snapshot(git: &Git, dir: &Path, branch: &str, message: &str, cancel: &Cancel) -> Result<String, Failure> {
    ok(git, dir, a!["add", "-A"], cancel)?;
    let tree = ok(git, dir, a!["write-tree"], cancel)?.text();
    let mut args: Vec<std::ffi::OsString> = ["-c", "commit.gpgsign=false"].map(Into::into).to_vec();
    args.extend(gitsync::identity(git, dir)?.into_iter().map(Into::into));
    args.extend(["commit-tree", tree.as_str(), "-m", message].map(Into::into));
    let list: Vec<&OsStr> = args.iter().map(|s| s.as_os_str()).collect();
    let commit = ok(git, dir, &list, cancel)?.text();
    ok(git, dir, a!["update-ref", format!("refs/heads/{branch}"), commit], cancel)?;
    Ok(commit)
}
