//! The managed TeX Live (TEX-002, TEX-003): downloaded with `curl`, installed by the official
//! `install-tl` with the profile `assets/tex/texlive.profile`, completed with `tlmgr install` of
//! `assets/tex/packages.txt`, updated with `tlmgr update`, removed by deleting its folder.
//! It is installed into `<data>/shale/texlive.partial` and renamed to `texlive` only when it is
//! complete, so a half-finished install is never used. Everything here runs in a job and
//! reports progress through a [`Handle`].

use super::compile::{compile_test_document, WorkDir};
use super::engine::{self, bin_dir_with, exe_name, managed_bin_dir, managed_root, partial_root, Engine, Kind};
use super::proc::{self, Ending};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The download of an install in megabytes, and what it takes on disk. Measured on 2026-11-17
/// with the TL2026 repository: 326 MB installed, about 100 MB downloaded (ADR-0020; the download is
/// 94 MB of packages and 5.5 MB for the installer in ADR-0019's measurement).
pub const DOWNLOAD_MB: u64 = 100;
pub const DISK_MB: u64 = 330;

/// The TeX Live packages installed after `scheme-infraonly`, the one list of `assets/tex/packages.txt`.
const PACKAGES: &str = include_str!("../../../assets/tex/packages.txt");

/// The install profile; its paths are `/opt/texlive/…`, the folder of the test image.
const PROFILE: &str = include_str!("../../../assets/tex/texlive.profile");
const PROFILE_ROOT: &str = "/opt/texlive";

/// How long one step (the download, `install-tl`, `tlmgr`) may take.
const STEP_TIMEOUT: Duration = Duration::from_secs(45 * 60);

/// The packages of `assets/tex/packages.txt`, one name each.
pub fn packages() -> Vec<&'static str> {
    PACKAGES.lines().map(|l| l.split('#').next().unwrap_or("").trim()).filter(|l| !l.is_empty()).collect()
}

/// The platform folder name TeX Live uses for this computer (`binary_<name>` in the profile), or
/// none when TeX Live has no installer binaries for it.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("x86_64-linux"),
        ("linux", "aarch64") => Some("aarch64-linux"),
        ("macos", _) => Some("universal-darwin"),
        ("windows", _) => Some("windows"),
        _ => None,
    }
}

/// The profile for an install into `root`: the one of `assets/tex/`, with its paths moved into
/// `root` and the binaries of this platform.
pub fn profile(root: &Path, platform: &str) -> String {
    let root_text = root.to_string_lossy().replace('\\', "/");
    PROFILE
        .lines()
        .map(|l| {
            if l.starts_with("TEXDIR") || l.starts_with("TEXMF") {
                l.replace(PROFILE_ROOT, &root_text)
            } else if l.starts_with("binary_") {
                format!("binary_{platform} 1")
            } else {
                l.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The archive of the installer in a repository.
pub fn installer_archive() -> &'static str {
    if cfg!(windows) {
        "install-tl.zip"
    } else {
        "install-tl-unx.tar.gz"
    }
}

/// A repository address that may be used: `https://` or `http://`, with a host and nothing that
/// could change a command line.
pub fn check_repository(url: &str) -> Result<&str, String> {
    let url = url.trim().trim_end_matches('/');
    let host = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"));
    match host {
        Some(h) if !h.is_empty() && !url.chars().any(|c| c.is_whitespace() || c.is_control()) && !h.starts_with('-') => Ok(url),
        _ => Err(format!("{url:?} is not a repository address (https://…)")),
    }
}

// ---------------------------------------------------------------- progress

/// Where an install or update is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stage {
    #[default]
    Starting,
    Download,
    Unpack,
    /// `install-tl`: the infrastructure.
    Base,
    /// `tlmgr install`: the packages of the list.
    Packages,
    /// `tlmgr update`.
    Update,
    /// The first LuaLaTeX run, which builds the font names database.
    Fonts,
}

/// What an install or update has done, for the window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub stage: Stage,
    pub done: u32,
    pub total: u32,
    /// The package being installed.
    pub package: String,
}

impl Progress {
    /// How far along, 0 to 1: the stages have a share of the whole each (the packages are the
    /// bulk of the download).
    pub fn fraction(&self) -> f32 {
        let within = if self.total == 0 { 0.0 } else { (self.done as f32 / self.total as f32).min(1.0) };
        let (from, share) = match self.stage {
            Stage::Starting => (0.0, 0.0),
            Stage::Download => (0.0, 0.05),
            Stage::Unpack => (0.05, 0.01),
            Stage::Base => (0.06, 0.44),
            Stage::Packages => (0.5, 0.44),
            Stage::Fonts => (0.94, 0.06),
            Stage::Update => (0.0, 1.0),
        };
        from + share * within
    }

    /// "Installing the packages: koma-script (31 of 52)".
    pub fn text(&self) -> String {
        let step = match self.stage {
            Stage::Starting => "Starting",
            Stage::Download => "Downloading the installer",
            Stage::Unpack => "Unpacking the installer",
            Stage::Base => "Installing the base of TeX Live",
            Stage::Packages => "Installing the packages",
            Stage::Update => "Updating",
            Stage::Fonts => "Building the font cache (the first LuaLaTeX run, this takes a moment)",
        };
        match (self.total, self.package.is_empty()) {
            (0, _) => step.to_string(),
            (t, true) => format!("{step} ({} of {t})", self.done),
            (t, false) => format!("{step}: {} ({} of {t})", self.package, self.done),
        }
    }
}

/// `[ 12/134, 00:03/00:20] install: koma-script [1234k]` (tlmgr) and `Installing [3/3, time/total:
/// 00:01/00:04]: texlive.infra [143k]` (install-tl) as (12, 134, "koma-script"): the progress lines
/// of the two programs.
pub fn parse_progress(line: &str) -> Option<(u32, u32, String)> {
    let line = line.trim_start();
    let rest = line.strip_prefix('[').or_else(|| line.strip_prefix("Installing ["))?;
    let (counts, after) = rest.split_once(']')?;
    let (done, total) = counts.split(',').next()?.split_once('/')?;
    let (done, total): (u32, u32) = (done.trim().parse().ok()?, total.trim().parse().ok()?);
    let mut words = after.trim_start_matches(':').split_whitespace();
    let first = words.next()?;
    // "install: name", "update: name", or just "name"
    let name = if first.ends_with(':') { words.next().unwrap_or("") } else { first };
    Some((done, total, name.to_string()))
}

/// What a running job shares with the window: how far it is, and the wish to stop.
#[derive(Clone, Default)]
pub struct Handle {
    progress: Arc<Mutex<Progress>>,
    cancel: Arc<AtomicBool>,
}

impl Handle {
    pub fn progress(&self) -> Progress {
        self.progress.lock().map(|p| p.clone()).unwrap_or_default()
    }

    /// Tests: whether the job was asked to stop.
    #[cfg(test)]
    pub(super) fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// Stop the job: its programs are killed and its partial folder is removed.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub(super) fn set(&self, f: impl FnOnce(&mut Progress)) {
        if let Ok(mut p) = self.progress.lock() {
            f(&mut p);
        }
    }

    fn stage(&self, stage: Stage) {
        self.set(|p| *p = Progress { stage, ..Progress::default() });
    }
}

// ---------------------------------------------------------------- the install

/// What an install or update needs to know.
#[derive(Clone, Debug)]
pub struct Env {
    /// The user's data folder (`<data>/shale/texlive` is the managed TeX Live).
    pub data: PathBuf,
    /// The TeX Live repository to download from.
    pub repository: String,
}

/// What an install ended with.
#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    /// The install works, but the first test compile did not (what it said).
    pub warning: Option<String>,
}

/// Run `cmd`, feeding its progress lines to the handle.
fn step(handle: &Handle, notify: &dyn Fn(), stage: Stage, what: &str, cmd: Command) -> Result<(), String> {
    handle.stage(stage);
    notify();
    let run = proc::run(cmd, STEP_TIMEOUT, Some(&handle.cancel), |line| {
        if let Some((done, total, package)) = parse_progress(line) {
            handle.set(|p| *p = Progress { stage, done, total, package });
            notify();
        }
    })
    .map_err(|e| format!("{what} could not be started ({e})"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err(format!("{what} took more than {} minutes and was stopped", STEP_TIMEOUT.as_secs() / 60)),
        Ending::Exited(Some(0)) => Ok(()),
        Ending::Exited(code) => {
            let lines = run.tail.len().saturating_sub(8);
            Err(format!("{what} failed (exit code {}):\n{}", code.map_or_else(|| "none".to_string(), |c| c.to_string()), run.tail[lines..].join("\n")))
        }
    }
}

/// Download a file with `curl` (the way the CalDAV client does its HTTP): https only, no
/// `.curlrc`, redirects only to https, a size limit.
fn download(handle: &Handle, url: &str, to: &Path) -> Result<(), String> {
    let protos = if url.starts_with("http://") { "=http,https" } else { "=https" };
    let mut c = Command::new("curl");
    c.args(["-q", "-sS", "-f", "-L", "--max-redirs", "5", "--proto", protos, "--proto-redir", "=https", "--connect-timeout", "20", "--max-filesize", "52428800", "-o"]).arg(to).arg(url);
    let run = proc::run(c, STEP_TIMEOUT, Some(&handle.cancel), |_| {}).map_err(|e| format!("could not run curl ({e}) — is curl installed?"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err("the download took too long and was stopped".into()),
        Ending::Exited(Some(0)) => Ok(()),
        Ending::Exited(_) => Err(format!("the download of {url} failed: {}", run.tail_text())),
    }
}

/// `perl` is needed to run `install-tl` and `tlmgr` on Linux and macOS.
fn check_perl() -> Result<(), String> {
    if cfg!(windows) {
        return Ok(());
    }
    check_perl_program("perl")
}

/// Whether this program runs as perl does.
pub(super) fn check_perl_program(program: &str) -> Result<(), String> {
    let mut c = Command::new(program);
    c.args(["-e", "1"]);
    match proc::run(c, Duration::from_secs(20), None, |_| {}) {
        Ok(r) if r.success() => Ok(()),
        _ => Err("TeX Live is installed with perl, and there is none on this computer: install perl first (for example `sudo apt install perl`, or Xcode's command line tools on a Mac), then try again.".into()),
    }
}

/// The managed TeX Live as an engine: the programs of `root`.
fn managed_engine(root: &Path) -> Result<Engine, String> {
    let bin = managed_bin_dir(root).ok_or_else(|| "no TeX Live programs were found in the installed folder".to_string())?;
    Ok(Engine::new(Kind::Managed, bin))
}

fn tlmgr_name() -> &'static str {
    if cfg!(windows) {
        "tlmgr.bat"
    } else {
        "tlmgr"
    }
}

/// Install the managed TeX Live (TEX-002). A cancelled or failed install leaves nothing behind
/// but a message; the folder it worked in is removed.
pub fn install(env: &Env, handle: &Handle, notify: &dyn Fn()) -> Result<Done, String> {
    let root = managed_root(&env.data);
    let partial = partial_root(&env.data);
    if root.exists() {
        return Err(format!("{} is there already: remove it first", root.display()));
    }
    let result = install_into(env, &partial, handle, notify);
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&partial);
    }
    result?;
    crate::fsutil::rename_noreplace(&partial, &root).map_err(|e| {
        let _ = std::fs::remove_dir_all(&partial);
        format!("the installed TeX Live could not be moved to {} ({e})", root.display())
    })?;
    // the first run builds the font names database: here, with the paths it will have from now on
    handle.stage(Stage::Fonts);
    notify();
    let warning = match managed_engine(&root).and_then(|e| compile_test_document(&e).map(|_| ()).map_err(|e| e.to_string())) {
        Ok(()) => None,
        Err(e) => Some(format!("TeX Live is installed, but the test document did not compile: {e}")),
    };
    Ok(Done { warning })
}

/// Everything up to a complete tree in `partial`.
fn install_into(env: &Env, partial: &Path, handle: &Handle, notify: &dyn Fn()) -> Result<(), String> {
    let repo = check_repository(&env.repository)?;
    let platform = platform().ok_or_else(|| format!("TeX Live has no installer for {} {}: install a TeX yourself and choose it in this page", std::env::consts::OS, std::env::consts::ARCH))?;
    check_perl()?;
    let _ = std::fs::remove_dir_all(partial);
    std::fs::create_dir_all(partial.parent().unwrap_or(partial)).map_err(|e| format!("cannot make the data folder ({e})"))?;
    let work = WorkDir::new().map_err(|e| format!("no temporary folder ({e})"))?;
    let archive = work.path().join(installer_archive());
    let inst = work.path().join("installer");
    std::fs::create_dir(&inst).map_err(|e| e.to_string())?;
    handle.stage(Stage::Download);
    notify();
    download(handle, &format!("{repo}/{}", installer_archive()), &archive)?;
    // the system's tar: bsdtar on Windows 10 and later reads the zip as well
    let mut tar = Command::new("tar");
    tar.arg("-xf").arg(&archive).arg("-C").arg(&inst).arg("--strip-components=1");
    step(handle, notify, Stage::Unpack, "tar", tar).map_err(|e| format!("the installer could not be unpacked: {e}"))?;
    let profile_file = work.path().join("texlive.profile");
    crate::fsutil::create_new(&profile_file, profile(partial, platform).as_bytes()).map_err(|e| format!("the install profile could not be written ({e})"))?;
    let mut install_tl = if cfg!(windows) {
        let mut c = Command::new(inst.join("install-tl-windows.bat"));
        c.arg("-no-gui");
        c
    } else {
        let mut c = Command::new("perl");
        c.arg(inst.join("install-tl"));
        c
    };
    install_tl.args(["-no-interaction", "-profile"]).arg(&profile_file).args(["-repository", repo]).current_dir(&inst);
    step(handle, notify, Stage::Base, "install-tl", install_tl)?;
    let bin = bin_dir_with(partial, tlmgr_name()).ok_or("install-tl made no tlmgr")?;
    let mut tlmgr = Command::new(bin.join(tlmgr_name()));
    tlmgr.env("PATH", engine::path_with(&bin)).args(["--repository", repo, "install"]).args(packages());
    step(handle, notify, Stage::Packages, "tlmgr", tlmgr)?;
    if !bin.join(exe_name("lualatex")).is_file() || !bin.join(exe_name("biber")).is_file() {
        return Err("the install finished but lualatex or biber is missing".into());
    }
    Ok(())
}

/// Update the managed TeX Live (TEX-003): `tlmgr update --self --all`.
pub fn update(env: &Env, handle: &Handle, notify: &dyn Fn()) -> Result<Done, String> {
    let repo = check_repository(&env.repository)?;
    let root = managed_root(&env.data);
    let engine = managed_engine(&root)?;
    check_perl()?;
    let mut tlmgr = engine.command(tlmgr_name());
    tlmgr.args(["--repository", repo, "update", "--self", "--all"]);
    step(handle, notify, Stage::Update, "tlmgr", tlmgr)?;
    // a new TeX Live may have new fonts and a new kernel: try it
    handle.stage(Stage::Fonts);
    notify();
    let warning = compile_test_document(&engine).err().map(|e| format!("TeX Live is updated, but the test document did not compile: {e}"));
    Ok(Done { warning })
}

// ---------------------------------------------------------------- remove

/// May this folder be removed as the managed TeX Live? Only the exact folder `<data>/shale/texlive`
/// (or its `.partial` twin), which is a real folder: a link is never followed, and neither is a
/// link in the place of `<data>/shale`.
pub fn removable(data: &Path, folder: &Path) -> Result<(), String> {
    if folder != managed_root(data) && folder != partial_root(data) {
        return Err(format!("{} is not the folder of the managed TeX Live", folder.display()));
    }
    let real_dir = |p: &Path| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink());
    if !real_dir(&data.join("shale")) {
        return Err(format!("{} is not a plain folder", data.join("shale").display()));
    }
    if !real_dir(folder) {
        return Err(format!("{} is not a plain folder (a link, or a file): nothing was removed", folder.display()));
    }
    Ok(())
}

/// Remove the managed TeX Live (TEX-003): its folder and nothing else.
pub fn remove(data: &Path) -> Result<(), String> {
    let root = managed_root(data);
    removable(data, &root)?;
    std::fs::remove_dir_all(&root).map_err(|e| format!("{} could not be removed ({e})", root.display()))?;
    // an interrupted install of an earlier run
    let partial = partial_root(data);
    if removable(data, &partial).is_ok() {
        let _ = std::fs::remove_dir_all(&partial);
    }
    Ok(())
}
