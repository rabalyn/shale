//! Installing, updating and removing the managed git (SYNC-025, SYNC-027, SYNC-028, ADR-0027), on the
//! model of the managed TeX Live (TEX-002, TEX-003): every file is downloaded with the shared
//! `curl` download, checked against its SHA-256, unpacked by the template archive reader into
//! `<data>/shale/git.partial`, its programs are run from there, and only then is the folder renamed to
//! `git`. A cancelled or failed install leaves nothing. Everything here runs in a job and reports
//! through a [`Handle`].

use super::managed::{self, Installed, MANIFEST};
use super::sources::{self, Kind, Part, Plan, Platform, Sources};
use crate::download::{self, Request};
use crate::pdf::tex::archive::{extract_with, sha256_hex, Limits};
use crate::pdf::tex::compile::WorkDir;
use crate::pdf::tex::proc::{self, Ending};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long one download may take.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// How long a program of an install may take to answer `--version`.
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

/// What an archive may unpack to (MinGit is 96 MB in 378 files).
const LIMITS: Limits = Limits { total: 512 << 20, file: 128 << 20 };

/// What a running job tells the window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub text: String,
    /// 0 to 1.
    pub fraction: f32,
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

    /// Stop the job: its download is stopped and its partial folder is removed.
    #[cfg(all(test, unix))]
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// The token of the registry's task for this job: Cancel there sets the flag the job looks at (JOB-028).
    pub fn token(&self) -> crate::busy::Cancel {
        self.cancel.clone().into()
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    fn set(&self, text: impl Into<String>, fraction: f32) {
        if let Ok(mut p) = self.progress.lock() {
            *p = Progress { text: text.into(), fraction: fraction.clamp(0.0, 1.0) };
        }
    }

    fn check(&self) -> Result<(), String> {
        if self.cancelled() {
            Err("cancelled".into())
        } else {
            Ok(())
        }
    }
}

/// Where the managed git is kept.
#[derive(Clone, Debug)]
pub struct Env {
    /// The user's data folder (`<data>/shale/git` is the managed git).
    pub data: PathBuf,
}

/// What an install or update ended with.
#[derive(Debug, PartialEq, Eq)]
pub struct Done {
    pub installed: Installed,
}

/// Download `part` to `to`, with the progress of the file's growth between `from` and `from + share`
/// of the whole.
pub(super) fn download_part(part: &Part, allow_http: bool, to: &Path, handle: &Handle, label: &str, from: f32, share: f32) -> Result<(), String> {
    let done = AtomicBool::new(false);
    let size = part.size.max(1) as f32;
    std::thread::scope(|s| {
        s.spawn(|| {
            while !done.load(Ordering::SeqCst) {
                let got = std::fs::metadata(to).map_or(0, |m| m.len()) as f32;
                handle.set(label.to_string(), from + share * (got / size).min(1.0));
                std::thread::sleep(Duration::from_millis(150));
            }
        });
        let r = download::to_file(&handle.cancel, &Request { url: &part.url, max_bytes: part.size + (10 << 20), timeout: DOWNLOAD_TIMEOUT, allow_http: allow_http && part.url.starts_with("http://"), headers: &[] }, to);
        done.store(true, Ordering::SeqCst);
        r
    })
}

/// Unpack `bytes` as the recipe says: into `<partial>/<into>`, without the top folder when the
/// archive has one. The archive reader refuses any path out of the folder and any link.
pub(super) fn unpack(part: &Part, bytes: &[u8], partial: &Path, index: usize) -> Result<(), String> {
    let r = &part.recipe;
    let tmp = partial.join(format!(".unpack-{index}"));
    std::fs::create_dir_all(&tmp).map_err(|e| format!("cannot make {} ({e})", tmp.display()))?;
    extract_with(bytes, &tmp, LIMITS).map_err(|e| format!("{} could not be unpacked: {e}", r.name))?;
    let src = if r.strip_root {
        let mut it = std::fs::read_dir(&tmp).map_err(|e| e.to_string())?.flatten();
        match (it.next(), it.next()) {
            (Some(only), None) if only.file_type().is_ok_and(|t| t.is_dir()) => only.path(),
            _ => return Err(format!("{} has no single top folder as expected: refused", r.name)),
        }
    } else {
        tmp.clone()
    };
    let dest = if r.into.is_empty() { partial.to_path_buf() } else { partial.join(&r.into) };
    if r.into.is_empty() {
        for e in std::fs::read_dir(&src).map_err(|e| e.to_string())?.flatten() {
            crate::fsutil::rename_noreplace(&e.path(), &dest.join(e.file_name())).map_err(|x| format!("{} could not be placed ({x})", e.path().display()))?;
        }
    } else {
        crate::fsutil::rename_noreplace(&src, &dest).map_err(|x| format!("{} could not be placed ({x})", dest.display()))?;
    }
    std::fs::remove_dir_all(&tmp).map_err(|e| format!("cannot remove {} ({e})", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let program = partial.join(&r.program);
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).map_err(|e| format!("{} is not in the archive ({e})", r.program))?;
    }
    Ok(())
}

/// Run the program of a part from the partial folder and check what it says (`git --version`,
/// `git-lfs version`), so a folder that does not run is never renamed into place.
fn check_program(part: &Part, partial: &Path, handle: &Handle) -> Result<(), String> {
    let r = &part.recipe;
    let program = partial.join(&r.program);
    let mut c = crate::program::command(&program);
    c.arg(if r.kind == Kind::Git { "--version" } else { "version" }).current_dir(partial);
    if let Some(first) = r.path.first() {
        c.env("PATH", crate::pdf::tex::engine::path_with(&partial.join(first)));
    }
    let run = proc::run(c, CHECK_TIMEOUT, Some(&handle.cancel), |_| {}).map_err(|e| format!("{} could not be started ({e})", r.program))?;
    match run.ending {
        Ending::Cancelled => Err("cancelled".into()),
        Ending::TimedOut => Err(format!("{} did not answer", r.program)),
        Ending::Exited(Some(0)) if run.tail_text().contains(&part.version) => Ok(()),
        Ending::Exited(Some(0)) => Err(format!("{} does not say it is version {}: {}", r.program, part.version, run.tail_text())),
        Ending::Exited(_) => Err(format!("{} failed: {}", r.program, run.tail_text())),
    }
}

/// The manifest of what a plan installs.
fn manifest(plan: &Plan) -> Installed {
    let part_of = |kind: Kind| plan.parts.iter().find(|p| p.recipe.kind == kind).map(|p| managed::Part { tag: p.tag.clone(), version: p.version.clone(), program: p.recipe.program.clone(), path: p.recipe.path.clone() });
    Installed { git: part_of(Kind::Git), lfs: part_of(Kind::Lfs) }
}

/// Everything up to a complete tree in `partial`.
fn build(plan: &Plan, partial: &Path, handle: &Handle, notify: &dyn Fn()) -> Result<Installed, String> {
    if plan.parts.is_empty() {
        return Err("there is nothing to install".into());
    }
    let _ = std::fs::remove_dir_all(partial);
    std::fs::create_dir_all(partial).map_err(|e| format!("cannot make the folder {} ({e})", partial.display()))?;
    let work = WorkDir::new().map_err(|e| format!("no temporary folder ({e})"))?;
    let total = plan.download_bytes().max(1) as f32;
    // the downloads are 90 % of the work, the checks the rest
    let mut done = 0u64;
    for (i, part) in plan.parts.iter().enumerate() {
        handle.check()?;
        let label = format!("Downloading {} {} ({} MB)", part.recipe.name, part.version, part.size.div_ceil(1 << 20));
        handle.set(label.clone(), done as f32 / total * 0.9);
        notify();
        let file = work.path().join(format!("part-{i}"));
        download_part(part, plan.allow_http, &file, handle, &label, done as f32 / total * 0.9, part.size as f32 / total * 0.9)?;
        done += part.size;
        let bytes = crate::fsutil::read(&file).map_err(|e| format!("the download of {} could not be read ({e})", part.recipe.name))?;
        let _ = std::fs::remove_file(&file);
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(&part.sha256) {
            return Err(format!("the download of {} has the SHA-256 {got}, not the expected {}: refused, nothing was installed", part.recipe.name, part.sha256));
        }
        handle.check()?;
        handle.set(format!("Unpacking {}", part.recipe.name), done as f32 / total * 0.9);
        notify();
        unpack(part, &bytes, partial, i)?;
    }
    for part in &plan.parts {
        handle.check()?;
        handle.set(format!("Checking {}", part.recipe.name), 0.95);
        notify();
        check_program(part, partial, handle)?;
    }
    let installed = manifest(plan);
    let text = serde_json::to_vec_pretty(&installed).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(&partial.join(MANIFEST), &text).map_err(|e| format!("the manifest could not be written ({e})"))?;
    handle.check()?;
    Ok(installed)
}

/// The install or update of `plan`: built in the partial folder, then put in place. `replace`: a
/// managed folder is there and is replaced by two renames (the old one is put back when the second
/// fails); otherwise there must be none.
fn put_in_place(env: &Env, plan: &Plan, replace: bool, handle: &Handle, notify: &dyn Fn()) -> Result<Done, String> {
    let (root, partial, old) = (managed::managed_root(&env.data), managed::partial_root(&env.data), managed::old_root(&env.data));
    match (root.exists(), replace) {
        (true, false) => return Err(format!("{} is there already: remove it first", root.display())),
        (false, true) => return Err(format!("{} is not there: nothing to update", root.display())),
        _ => {}
    }
    if let Some(parent) = partial.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot make the data folder ({e})"))?;
    }
    let installed = match build(plan, &partial, handle, notify) {
        Ok(i) => i,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&partial);
            return Err(e);
        }
    };
    handle.set("Putting it in place", 0.98);
    notify();
    if replace {
        let _ = std::fs::remove_dir_all(&old);
        crate::fsutil::rename_noreplace(&root, &old).map_err(|e| {
            let _ = std::fs::remove_dir_all(&partial);
            format!("the old git could not be moved aside ({e}); it stays as it was")
        })?;
        if let Err(e) = crate::fsutil::rename_noreplace(&partial, &root) {
            let back = crate::fsutil::rename_noreplace(&old, &root);
            let _ = std::fs::remove_dir_all(&partial);
            let note = if back.is_ok() { "the old one stays as it was".to_string() } else { format!("the old one is in {}", old.display()) };
            return Err(format!("the new git could not be moved to {} ({e}); {note}", root.display()));
        }
        let _ = std::fs::remove_dir_all(&old);
    } else {
        crate::fsutil::rename_noreplace(&partial, &root).map_err(|e| {
            let _ = std::fs::remove_dir_all(&partial);
            format!("the installed git could not be moved to {} ({e})", root.display())
        })?;
    }
    Ok(Done { installed })
}

/// Install the managed git (SYNC-025). A cancelled or failed install leaves nothing behind but a message.
pub fn install(env: &Env, plan: &Plan, handle: &Handle, notify: &dyn Fn()) -> Result<Done, String> {
    put_in_place(env, plan, false, handle, notify)
}

/// Replace the managed git by the newer one of `plan` (SYNC-027); the old one stays until the new one runs.
pub fn update(env: &Env, plan: &Plan, handle: &Handle, notify: &dyn Fn()) -> Result<Done, String> {
    put_in_place(env, plan, true, handle, notify)
}

// ---------------------------------------------------------------- the update check

/// The newest releases of what is installed on `platform`, when any is newer than what is installed
/// (SYNC-027). `fetch` gets the text of an API address (the real one asks GitHub with `curl`).
pub fn check_latest(sources: &Sources, platform: &Platform, installed: &Installed, fetch: &dyn Fn(&str) -> Result<String, String>) -> Result<Option<Plan>, String> {
    let recipes = sources.recipes(platform).ok_or_else(|| format!("no sources for {} {}", platform.os, platform.arch))?;
    let mut parts = vec![];
    let mut newer = false;
    for r in recipes {
        let have = match r.kind {
            Kind::Git => installed.git.as_ref(),
            Kind::Lfs => installed.lfs.as_ref(),
        };
        // a part that is not installed (git of the system, SYNC-029) is not updated
        let Some(have) = have else { continue };
        let part = sources.latest(r, &fetch(&sources.latest_url(&r.repo))?)?;
        newer |= sources::is_newer(&have.tag, &part.tag);
        parts.push(part);
    }
    Ok((newer && !parts.is_empty()).then_some(Plan { parts, allow_http: sources.allow_http }))
}

/// The real `fetch`: GitHub's API with `curl` (https only, no `.curlrc`, a size limit), cancellable.
pub fn fetch_api(url: &str, allow_http: bool, cancel: &AtomicBool) -> Result<String, String> {
    let work = WorkDir::new().map_err(|e| format!("no temporary folder ({e})"))?;
    let file = work.path().join("api.json");
    download::to_file(cancel, &Request { url, max_bytes: 4 << 20, timeout: Duration::from_secs(60), allow_http: allow_http && url.starts_with("http://"), headers: &["Accept: application/vnd.github+json"] }, &file)?;
    crate::fsutil::read_to_string(&file).map_err(|e| format!("the answer could not be read ({e})"))
}

// ---------------------------------------------------------------- remove

/// May this folder be removed as the managed git? Only the exact folders `<data>/shale/git`, its
/// `.partial` and `.old` twins, as real folders: a link is never followed (SYNC-028).
pub fn removable(data: &Path, folder: &Path) -> Result<(), String> {
    crate::fsutil::removable_folder(data, folder, &[managed::managed_root(data), managed::partial_root(data), managed::old_root(data)], "the managed git")
}

/// Remove the managed git (SYNC-028): its folder and nothing else.
pub fn remove(data: &Path) -> Result<(), String> {
    let root = managed::managed_root(data);
    removable(data, &root)?;
    std::fs::remove_dir_all(&root).map_err(|e| format!("{} could not be removed ({e})", root.display()))?;
    // an interrupted install or update of an earlier run
    for twin in [managed::partial_root(data), managed::old_root(data)] {
        if removable(data, &twin).is_ok() {
            let _ = std::fs::remove_dir_all(&twin);
        }
    }
    Ok(())
}

/// The command that starts Apple's installer of the command line tools (SYNC-029); the dialog is
/// Apple's, Shale does not wait for it.
pub fn xcode_select_command() -> Command {
    let mut c = crate::program::command("xcode-select");
    c.arg("--install");
    c
}

/// Start Apple's installer of the command line tools.
pub fn start_xcode_select() -> Result<(), String> {
    let out = xcode_select_command().stdin(std::process::Stdio::null()).output().map_err(|e| format!("xcode-select could not be started ({e})"))?;
    // "xcode-select: error: command line tools are already installed" also exits 1
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}
