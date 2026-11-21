//! The managed git (SYNC-023…SYNC-026, ADR-0027): the settings that choose it, where it lives
//! below the user's data folder, the manifest that says what is in that folder, the selection of
//! the `Git` every real use runs, and the look at what is installed. Looking runs programs
//! (`git --version`), so [`detect`] belongs in a background job and never in a frame.

use super::Git;
use crate::pdf::tex::engine::{exe_name, find_in_path, folder_bytes};
use serde::{Deserialize, Serialize};
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

/// The file in the managed folder that says what it holds.
pub const MANIFEST: &str = "shale-git.json";

/// Which git Shale runs (CFG-054, SYNC-024).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GitChoice {
    /// The git of `PATH` when there is one, else the managed git.
    #[default]
    Auto,
    /// The git of `PATH` only.
    System,
    /// The managed git.
    Managed,
}

impl GitChoice {
    pub const ALL: [GitChoice; 3] = [GitChoice::Auto, GitChoice::System, GitChoice::Managed];

    pub fn label(self) -> &'static str {
        match self {
            GitChoice::Auto => "Automatic",
            GitChoice::System => "The git of this computer",
            GitChoice::Managed => "The git that Shale installs",
        }
    }
}

/// What Shale does about a new release of the managed git (CFG-054, SYNC-027).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GitUpdates {
    /// No request is made.
    #[default]
    Off,
    /// Say that there is a new one.
    Notify,
    /// Install it.
    Install,
}

impl GitUpdates {
    pub const ALL: [GitUpdates; 3] = [GitUpdates::Off, GitUpdates::Notify, GitUpdates::Install];

    pub fn label(self) -> &'static str {
        match self {
            GitUpdates::Off => "Never look for a new version",
            GitUpdates::Notify => "Look for a new version and tell me",
            GitUpdates::Install => "Look for a new version and install it",
        }
    }
}

/// The git settings (CFG-054).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct GitConfig {
    pub choice: GitChoice,
    pub updates: GitUpdates,
}

/// Where the managed git lives below the user's data folder (`<data>/shale/git`).
pub fn managed_root(data: &Path) -> PathBuf {
    data.join("shale").join("git")
}

/// Where it is built first (`<data>/shale/git.partial`), and renamed from on success.
pub fn partial_root(data: &Path) -> PathBuf {
    data.join("shale").join("git.partial")
}

/// Where an update puts the old tree while the new one is moved in (`<data>/shale/git.old`).
pub fn old_root(data: &Path) -> PathBuf {
    data.join("shale").join("git.old")
}

/// One thing the managed folder holds, as the manifest records it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// The release it came from (`v3.8.1`, `v2.56.1.windows.1`).
    pub tag: String,
    /// Its version number (`3.8.1`).
    pub version: String,
    /// The program, relative to the managed folder.
    pub program: String,
    /// The folders, relative to the managed folder, that are added to `PATH`.
    pub path: Vec<String>,
}

/// What the managed folder holds (the manifest, SYNC-026).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct Installed {
    pub git: Option<Part>,
    pub lfs: Option<Part>,
}

/// A manifest path as a path below `root`, or none when it is not plain.
fn below(root: &Path, rel: &str) -> Option<PathBuf> {
    let p = Path::new(rel);
    (!rel.is_empty() && p.components().all(|c| matches!(c, Component::Normal(_)))).then(|| root.join(p))
}

impl Installed {
    /// What `root` holds: its manifest, without the parts whose program is not there. None when
    /// there is no manifest or nothing usable in it (a folder of something else is no managed git).
    pub fn read(root: &Path) -> Option<Installed> {
        let text = crate::fsutil::read_to_string(root.join(MANIFEST)).ok()?;
        let mut found: Installed = serde_json::from_str(&text).ok()?;
        let usable = |p: &Option<Part>| p.as_ref().is_some_and(|p| below(root, &p.program).is_some_and(|f| f.is_file()));
        if !usable(&found.git) {
            found.git = None;
        }
        if !usable(&found.lfs) {
            found.lfs = None;
        }
        (found.git.is_some() || found.lfs.is_some()).then_some(found)
    }

    /// The git program of the managed folder.
    pub fn git_program(&self, root: &Path) -> Option<PathBuf> {
        below(root, &self.git.as_ref()?.program)
    }

    /// The folders of a part that go into `PATH`.
    fn dirs(part: Option<&Part>, root: &Path) -> Vec<PathBuf> {
        part.map(|p| p.path.iter().filter_map(|d| below(root, d)).collect()).unwrap_or_default()
    }

    /// The text for the settings: "git 2.56.1, git-lfs 3.8.1".
    pub fn versions_text(&self) -> String {
        let mut v = vec![];
        if let Some(g) = &self.git {
            v.push(format!("git {}", g.version));
        }
        if let Some(l) = &self.lfs {
            v.push(format!("git-lfs {}", l.version));
        }
        v.join(", ")
    }
}

/// `path_var` (a `PATH`, or the one of this process) with `front` before and `back` after it.
fn path_around(front: &[PathBuf], back: &[PathBuf], path_var: Option<&OsStr>) -> OsString {
    let old = path_var.map(OsStr::to_os_string).or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    let all = front.iter().cloned().chain(std::env::split_paths(&old)).chain(back.iter().cloned());
    std::env::join_paths(all).unwrap_or(old)
}

/// The `Git` to run for this choice (SYNC-024): the program and its `PATH`. Every real use of git
/// (the sync, git-lfs, the settings) runs this one. Only looks at files; no program is started.
///
/// - `System`: the git of `PATH`, nothing added.
/// - `Managed`: the git of the managed folder, with its folders (and git-lfs') in front of `PATH`; where
///   only git-lfs is managed (macOS, Linux, SYNC-029) the git of `PATH` with the managed git-lfs in front.
/// - `Auto`: the git of `PATH` when there is one (a git-lfs of `PATH` wins, the managed one is the
///   fallback behind it), else the managed git, else plain `git`, which is then found missing.
pub fn selected_git(choice: GitChoice, data: Option<&Path>, path_var: Option<&OsStr>) -> Git {
    let root = data.map(managed_root);
    let installed = root.as_deref().and_then(Installed::read);
    let managed_program = installed.as_ref().zip(root.as_deref()).and_then(|(i, r)| i.git_program(r));
    let system = find_in_path("git", path_var).map(|d| d.join(exe_name("git")));
    let all_dirs = || -> Vec<PathBuf> {
        match (&installed, &root) {
            (Some(i), Some(r)) => [Installed::dirs(i.git.as_ref(), r), Installed::dirs(i.lfs.as_ref(), r)].concat(),
            _ => vec![],
        }
    };
    let lfs_dirs = || match (&installed, &root) {
        (Some(i), Some(r)) => Installed::dirs(i.lfs.as_ref(), r),
        _ => vec![],
    };
    let (program, front, back) = match choice {
        GitChoice::System => (system, vec![], vec![]),
        GitChoice::Managed => (managed_program.or(system), all_dirs(), vec![]),
        GitChoice::Auto => match (system, managed_program) {
            (Some(s), _) => (Some(s), vec![], lfs_dirs()),
            (None, Some(m)) => (Some(m), all_dirs(), vec![]),
            (None, None) => (None, vec![], lfs_dirs()),
        },
    };
    let mut git = Git { program: program.unwrap_or_else(|| PathBuf::from("git")), env: vec![] };
    if !front.is_empty() || !back.is_empty() {
        git.env.push(("PATH".into(), path_around(&front, &back, path_var)));
    }
    git
}

/// Where a program that is in use comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    System,
    Managed,
}

/// What the look at this computer found (SYNC-023).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    /// `git --version` of the git in `PATH`.
    pub system_git: Option<String>,
    /// `git lfs version` of the git in `PATH`.
    pub system_lfs: Option<String>,
    /// The managed folder, when it holds a git or a git-lfs.
    pub managed: Option<ManagedInfo>,
    /// The git in use for the choice (version line) and where it is from.
    pub git_in_use: Option<(String, Origin)>,
    /// The git-lfs in use for the choice.
    pub lfs_in_use: Option<(String, Origin)>,
}

/// The managed folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagedInfo {
    pub installed: Installed,
    /// What it takes on disk.
    pub bytes: u64,
}

/// Look at this computer: the git of `PATH`, the managed folder, and the git the choice gives.
/// Runs programs; call it from a job.
pub fn detect(choice: GitChoice, data: Option<&Path>, path_var: Option<&OsStr>) -> Detection {
    let root = data.map(managed_root);
    let managed = root.as_deref().and_then(|r| Some(ManagedInfo { installed: Installed::read(r)?, bytes: folder_bytes(r) }));
    let system = selected_git(GitChoice::System, data, path_var);
    let has_system = find_in_path("git", path_var).is_some();
    let (system_git, system_lfs) = if has_system { (system.version(), system.lfs_version()) } else { (None, None) };
    let used = selected_git(choice, data, path_var);
    let git_origin = if root.as_deref().is_some_and(|r| used.program.starts_with(r)) { Origin::Managed } else { Origin::System };
    let managed_lfs = managed.as_ref().is_some_and(|m| m.installed.lfs.is_some());
    let lfs_origin = match choice {
        GitChoice::System => Origin::System,
        GitChoice::Managed if managed_lfs => Origin::Managed,
        GitChoice::Managed => Origin::System,
        GitChoice::Auto if system_lfs.is_none() && managed_lfs => Origin::Managed,
        GitChoice::Auto => Origin::System,
    };
    Detection { system_git, system_lfs, managed, git_in_use: used.version().map(|v| (v, git_origin)), lfs_in_use: used.lfs_version().map(|v| (v, lfs_origin)) }
}
