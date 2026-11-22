//! The managed TeX Live (TEX-002, TEX-003): downloaded with `curl`, installed by the official
//! `install-tl` with the profile `assets/tex/texlive.profile`, completed with `tlmgr install` of
//! `assets/tex/packages.txt`, updated with `tlmgr update`, removed by deleting its folder. Each
//! install or update resolves the repository to one concrete mirror and uses only that (#320).
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

/// A small file every TeX Live repository has: it is asked for to learn which mirror a redirector
/// such as `mirror.ctan.org` sends to, so that one install or update uses that one mirror (#320).
pub(super) const MIRROR_PROBE: &str = "tlpkg/texlive.tlpdb.sha512";

/// How often `tlmgr` runs at most, each time on a freshly chosen mirror, while the mirror it got is
/// older than the installation (#320).
pub(super) const MIRROR_TRIES: usize = 3;

/// What `tlmgr` says when the repository is older than the installation ("… seems to be older than
/// the local installation …; please use a different mirror").
const OLDER_MIRROR: &str = "older than the local installation";

/// What `tlmgr` prints (and still exits 0) for a package it could not take from a mirror that is
/// being synced: `TLPDB::_install_data: downloading did not succeed (check_file_and_remove failed)
/// for https://…/archive/xpatch.tar.xz`, after `check_file_and_remove: checksums differ for …`. The
/// package is skipped, so the install is incomplete although `tlmgr` said nothing is wrong (#362).
const DAMAGED_DOWNLOAD: &str = "downloading did not succeed";
const DAMAGED_CHECKSUM: &str = "checksums differ";

/// The start of the error of a run whose mirror handed out damaged packages: what [`step`] gives and
/// [`tlmgr_on_mirror`] looks for, like [`OLDER_MIRROR`].
const DAMAGED_MIRROR: &str = "the mirror delivered damaged packages";

/// The package in a `tlmgr` line about a download that did not succeed (`…/archive/xpatch.tar.xz` is
/// `xpatch`), or a name for a line that only says the checksums differ.
fn damaged_package(line: &str) -> Option<String> {
    if !line.contains(DAMAGED_DOWNLOAD) && !line.contains(DAMAGED_CHECKSUM) {
        return None;
    }
    let file = line.split_whitespace().rev().find_map(|w| w.trim_end_matches(':').rsplit('/').next().filter(|f| f.contains(".tar.")))?;
    Some(file.split(".tar.").next().unwrap_or(file).to_string())
}

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
    /// The font names database (`luaotfload-tool --update`), the test document, and when it fails
    /// for a font, the font packages again from a fresh mirror (#362).
    Fonts,
    /// A template import (TEX-006): downloading the archive.
    Fetch,
    /// Reading the archive or the folder.
    Read,
    /// Generating the class files with docstrip.
    Generate,
    /// Putting the set into the vault.
    Place,
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
            Stage::Fetch => (0.1, 0.0),
            Stage::Read => (0.4, 0.0),
            Stage::Generate => (0.6, 0.0),
            Stage::Place => (0.9, 0.0),
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
            Stage::Fonts => "Building the font cache (this takes a moment)",
            Stage::Fetch => "Downloading the templates",
            Stage::Read => "Reading the templates",
            Stage::Generate => "Generating the class files with docstrip",
            Stage::Place => "Putting the set into the vault",
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
    pub(super) cancel: Arc<AtomicBool>,
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

    pub(super) fn stage(&self, stage: Stage) {
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

/// What an install or update ended with.
#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    /// The install works, but the first test compile did not (what it said, naming the mirror).
    pub warning: Option<String>,
    /// The mirror or mirrors the files came from, the first one first: the one the repository was
    /// resolved to, a fresh one when `tlmgr` found it older than the installation (#320), and the
    /// one the font packages were installed again from (#362).
    pub mirrors: Vec<String>,
}

impl Done {
    /// "from the mirror https://…" or "from the mirrors https://… and https://…".
    pub fn source_text(&self) -> String {
        match self.mirrors.as_slice() {
            [] => String::new(),
            [one] => format!("from the mirror {one}"),
            many => format!("from the mirrors {}", many.join(" and ")),
        }
    }
}

/// The TeX Live packages the fonts of the templates and the test document stand on (Latin Modern
/// for the LaTeX defaults, XCharter and Roboto for the templates); the ones installed again when a
/// font cannot be loaded (#362).
pub const FONT_PACKAGES: [&str; 4] = ["lm", "lm-math", "xcharter", "roboto"];

fn add_mirror(mirrors: &mut Vec<String>, m: &str) {
    if !mirrors.iter().any(|x| x == m) {
        mirrors.push(m.to_string());
    }
}

/// Run `cmd`, feeding its progress lines to the handle.
fn step(handle: &Handle, notify: &dyn Fn(), stage: Stage, what: &str, cmd: Command) -> Result<(), String> {
    handle.stage(stage);
    notify();
    // every line is looked at, not only the tail kept for the error: the lines about a damaged download come early
    let mut damaged: Vec<String> = vec![];
    let run = proc::run(cmd, STEP_TIMEOUT, Some(&handle.cancel), |line| {
        if let Some(p) = damaged_package(line) {
            if !damaged.contains(&p) {
                damaged.push(p);
            }
        }
        if let Some((done, total, package)) = parse_progress(line) {
            handle.set(|p| *p = Progress { stage, done, total, package });
            notify();
        }
    })
    .map_err(|e| format!("{what} could not be started ({e})"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err(format!("{what} took more than {} minutes and was stopped", STEP_TIMEOUT.as_secs() / 60)),
        // the exit code is 0 although packages were skipped: that is no success
        Ending::Exited(Some(0)) if !damaged.is_empty() => Err(format!("{what}: {DAMAGED_MIRROR}, and these packages were skipped: {}", damaged.join(", "))),
        Ending::Exited(Some(0)) => Ok(()),
        Ending::Exited(_) if !damaged.is_empty() => Err(format!("{what} failed: {DAMAGED_MIRROR}, and these packages were skipped: {}", damaged.join(", "))),
        Ending::Exited(code) => {
            let lines = run.tail.len().saturating_sub(8);
            Err(format!("{what} failed (exit code {}):\n{}", code.map_or_else(|| "none".to_string(), |c| c.to_string()), run.tail[lines..].join("\n")))
        }
    }
}

/// Download a file with `curl` (the way the CalDAV client does its HTTP): https only, no
/// `.curlrc`, redirects only to https, a size limit.
pub(super) fn download(handle: &Handle, url: &str, to: &Path) -> Result<(), String> {
    crate::download::to_file(&handle.cancel, &crate::download::Request { url, max_bytes: 52_428_800, timeout: STEP_TIMEOUT, allow_http: url.starts_with("http://"), headers: &[] }, to)
}

/// One concrete mirror for the repository `repo` (#320). A redirector such as `mirror.ctan.org`
/// sends every request to some mirror, and mirrors are not all equally current: `install-tl` from a
/// newer mirror followed by `tlmgr` from an older one fails ("older than the local installation").
/// So the redirect is followed once, for [`MIRROR_PROBE`], and the address it ends at is used for
/// every step; a repository that does not redirect is its own mirror.
pub(super) fn resolve_mirror(handle: &Handle, repo: &str) -> Result<String, String> {
    let work = WorkDir::new().map_err(|e| format!("no temporary folder ({e})"))?;
    let probe = format!("{repo}/{MIRROR_PROBE}");
    // an https repository is only ever left for https; an http one is not made safer by refusing http
    let protos = if repo.starts_with("http://") { "=http,https" } else { "=https" };
    let mut c = crate::program::command("curl");
    c.args(["-q", "-sS", "-f", "-L", "--max-redirs", "5", "--proto", protos, "--proto-redir", protos, "--connect-timeout", "20", "--max-filesize", "1048576", "-w", "%{url_effective}\n", "-o"]).arg(work.path().join("probe")).arg(&probe);
    let run = proc::run(c, Duration::from_secs(120), Some(&handle.cancel), |_| {}).map_err(|e| format!("could not run curl ({e}) — is curl installed?"))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err(format!("the repository {repo} did not answer")),
        Ending::Exited(Some(0)) => {
            let effective = run.tail.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
            let mirror = effective.strip_suffix(MIRROR_PROBE).map(|m| m.trim_end_matches('/')).ok_or_else(|| format!("the repository {repo} sent its {MIRROR_PROBE} to {effective:?}, which is no TeX Live repository"))?;
            check_repository(mirror).map(str::to_string)
        }
        Ending::Exited(_) => Err(format!("the repository {repo} could not be reached: {}", run.tail_text())),
    }
}

/// Run `tlmgr` (made by `cmd` for a mirror) on `mirror`. When `tlmgr` finds that mirror older than
/// the installation, or hands out damaged packages (a mirror in the middle of a sync, #362), a fresh
/// mirror of `repo` is chosen and tried, [`MIRROR_TRIES`] runs at most (#320); a second run installs
/// (or updates) only what the first one skipped. The mirror the run that worked used is given back; when
/// the last run still met damaged packages the error names every mirror tried.
fn tlmgr_on_mirror(handle: &Handle, notify: &dyn Fn(), stage: Stage, repo: &str, mut mirror: String, cmd: impl Fn(&str) -> Command) -> Result<String, String> {
    let mut runs = 1;
    let mut tried = vec![mirror.clone()];
    loop {
        match step(handle, notify, stage, "tlmgr", cmd(&mirror)) {
            Err(e) if (e.contains(OLDER_MIRROR) || e.contains(DAMAGED_MIRROR)) && runs < MIRROR_TRIES => {
                runs += 1;
                mirror = resolve_mirror(handle, repo)?;
                tried.push(mirror.clone());
            }
            Err(e) if e.contains(DAMAGED_MIRROR) => return Err(format!("{e} (mirrors tried: {})", tried.join(", "))),
            result => return result.map(|()| mirror),
        }
    }
}

/// `perl` is needed to run `install-tl` and `tlmgr` on Linux and macOS.
fn check_perl() -> Result<(), String> {
    if cfg!(windows) {
        return Ok(());
    }
    check_perl_program("perl")
}

/// What is wrong when there is no `perl` for the install, in words; none when there is one.
pub fn perl_problem() -> Option<String> {
    check_perl().err()
}

/// Whether this program runs as perl does.
pub(super) fn check_perl_program(program: &str) -> Result<(), String> {
    let mut c = crate::program::command(program);
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
    let mut mirrors = result?;
    crate::fsutil::rename_noreplace(&partial, &root).map_err(|e| {
        let _ = std::fs::remove_dir_all(&partial);
        format!("the installed TeX Live could not be moved to {} ({e})", root.display())
    })?;
    // the font names database is built here, with the paths it will have from now on
    let repo = check_repository(&env.repository).unwrap_or(&env.repository);
    let warning = fonts_after(managed_engine(&root), handle, notify, repo, &mut mirrors, "installed");
    Ok(Done { warning, mirrors })
}

/// The Fonts stage of an install or update, and the warning it ends with (none when the test
/// document compiled): [`fonts`], with the mirrors named in what it says.
fn fonts_after(engine: Result<Engine, String>, handle: &Handle, notify: &dyn Fn(), repo: &str, mirrors: &mut Vec<String>, what: &str) -> Option<String> {
    let result = engine.and_then(|e| fonts(&e, handle, notify, repo, mirrors));
    result.err().map(|e| {
        let from = Done { warning: None, mirrors: mirrors.clone() }.source_text();
        format!("TeX Live is {what}{}, but the test document did not compile: {e}", if from.is_empty() { String::new() } else { format!(" {from}") })
    })
}

/// The fonts of a TeX Live that is installed or updated (#362): `luaotfload-tool --update` builds
/// the font names database once, here, and not in the first LuaLaTeX run of every compile; then the
/// test document is compiled. When that fails because a font cannot be loaded (a package that came
/// incomplete from a mirror, a database that lacks it), the font packages are installed again from a
/// freshly resolved mirror, the database is rebuilt and the test document is tried once more. The
/// install itself is complete whatever happens here: every failure is only what the caller says.
fn fonts(engine: &Engine, handle: &Handle, notify: &dyn Fn(), repo: &str, mirrors: &mut Vec<String>) -> Result<(), String> {
    let mut update = engine.command("luaotfload-tool");
    update.arg("--update");
    // a failed update is not the end: the first LuaLaTeX run builds the database as well
    let update_failed = step(handle, notify, Stage::Fonts, "luaotfload-tool", update).err();
    let first = match compile_test_document(engine) {
        Ok(_) => return Ok(()),
        Err(e) => e,
    };
    if !first.font_problem() {
        return Err(with_note(first.to_string(), update_failed));
    }
    let mut repair = || -> Result<(), String> {
        // a fresh mirror may be older than the installation as well: tried like the packages are (#320)
        let mirror = resolve_mirror(handle, repo)?;
        let used = tlmgr_on_mirror(handle, notify, Stage::Fonts, repo, mirror, |m| {
            let mut tlmgr = engine.command(tlmgr_name());
            tlmgr.args(["--repository", m, "install", "--reinstall"]).args(FONT_PACKAGES);
            tlmgr
        })?;
        add_mirror(mirrors, &used);
        let mut rebuild = engine.command("luaotfload-tool");
        rebuild.args(["--update", "--force"]);
        step(handle, notify, Stage::Fonts, "luaotfload-tool", rebuild)
    };
    match repair() {
        Ok(()) => compile_test_document(engine).map(|_| ()).map_err(|e| e.to_string()),
        Err(why) => Err(format!("{first}\nThe fonts could not be installed again: {why}")),
    }
}

/// `text`, and `note` (what an earlier step said) after it.
fn with_note(text: String, note: Option<String>) -> String {
    match note {
        Some(n) => format!("{text}\n{n}"),
        None => text,
    }
}

/// Everything up to a complete tree in `partial`.
fn install_into(env: &Env, partial: &Path, handle: &Handle, notify: &dyn Fn()) -> Result<Vec<String>, String> {
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
    // one mirror for the installer, install-tl and tlmgr (#320)
    let mirror = resolve_mirror(handle, repo)?;
    let mut mirrors = vec![mirror.clone()];
    download(handle, &format!("{mirror}/{}", installer_archive()), &archive)?;
    // the system's tar: bsdtar on Windows 10 and later reads the zip as well
    let mut tar = crate::program::command("tar");
    tar.arg("-xf").arg(&archive).arg("-C").arg(&inst).arg("--strip-components=1");
    step(handle, notify, Stage::Unpack, "tar", tar).map_err(|e| format!("the installer could not be unpacked: {e}"))?;
    let profile_file = work.path().join("texlive.profile");
    crate::fsutil::create_new(&profile_file, profile(partial, platform).as_bytes()).map_err(|e| format!("the install profile could not be written ({e})"))?;
    let mut install_tl = if cfg!(windows) {
        let mut c = crate::program::command(inst.join("install-tl-windows.bat"));
        c.arg("-no-gui");
        c
    } else {
        let mut c = crate::program::command("perl");
        c.arg(inst.join("install-tl"));
        c
    };
    install_tl.args(["-no-interaction", "-profile"]).arg(&profile_file).args(["-repository", &mirror]).current_dir(&inst);
    step(handle, notify, Stage::Base, "install-tl", install_tl)?;
    let bin = bin_dir_with(partial, tlmgr_name()).ok_or("install-tl made no tlmgr")?;
    let used = tlmgr_on_mirror(handle, notify, Stage::Packages, repo, mirror, |m| {
        let mut tlmgr = crate::program::command(bin.join(tlmgr_name()));
        tlmgr.env("PATH", engine::path_with(&bin)).args(["--repository", m, "install"]).args(packages());
        tlmgr
    })?;
    add_mirror(&mut mirrors, &used);
    if !bin.join(exe_name("lualatex")).is_file() || !bin.join(exe_name("biber")).is_file() {
        return Err("the install finished but lualatex or biber is missing".into());
    }
    Ok(mirrors)
}

/// Update the managed TeX Live (TEX-003): `tlmgr update --self --all`, from one mirror of the
/// repository, and from a fresh one while that mirror is older than the installation (#320).
pub fn update(env: &Env, handle: &Handle, notify: &dyn Fn()) -> Result<Done, String> {
    let repo = check_repository(&env.repository)?;
    let root = managed_root(&env.data);
    let engine = managed_engine(&root)?;
    check_perl()?;
    handle.stage(Stage::Update);
    notify();
    let mirror = resolve_mirror(handle, repo)?;
    let mut mirrors = vec![mirror.clone()];
    let used = tlmgr_on_mirror(handle, notify, Stage::Update, repo, mirror, |m| {
        let mut tlmgr = engine.command(tlmgr_name());
        tlmgr.args(["--repository", m, "update", "--self", "--all"]);
        tlmgr
    })?;
    add_mirror(&mut mirrors, &used);
    // a new TeX Live may have new fonts and a new kernel: the database again, and the test document
    let warning = fonts_after(Ok(engine), handle, notify, repo, &mut mirrors, "updated");
    Ok(Done { warning, mirrors })
}

// ---------------------------------------------------------------- remove

/// May this folder be removed as the managed TeX Live? Only the exact folder `<data>/shale/texlive`
/// (or its `.partial` twin), which is a real folder: a link is never followed, and neither is a
/// link in the place of `<data>/shale`.
pub fn removable(data: &Path, folder: &Path) -> Result<(), String> {
    crate::fsutil::removable_folder(data, folder, &[managed_root(data), partial_root(data)], "the managed TeX Live")
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
