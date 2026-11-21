//! Putting a verified download in place (UPD-013 to UPD-019). What to do for each kind of install
//! is a [`Plan`], a list of file operations that is a pure function of the install kind and the
//! files of the download, so every kind is tested on any system; [`execute`] runs a plan. The
//! previous version is kept (`shale.old`) where that is cheap, and removed after the new one has
//! run ([`leftovers`]).

use super::check::Staged;
use super::kind::InstallKind;
use crate::pdf::tex::archive::{self, Limits};
use crate::pdf::tex::proc::{self, Ending};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// What an unpacked update may be: a program is larger than a template.
const LIMITS: Limits = Limits { total: 1 << 30, file: 512 << 20 };

/// How long `install.sh`, `hdiutil` or `ditto` may take.
const RUN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// One step of an install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Copy a file next to its place and move it over `to` (a file that does not exist is skipped).
    CopyFile { from: PathBuf, to: PathBuf },
    /// Put a file in place: copied next to `to` and moved over it, which is allowed for a program
    /// that is running on Linux. Where that is not allowed (Windows) the old file is first moved to
    /// `aside`, and moved back when the new one cannot be put in place.
    Install { from: PathBuf, to: PathBuf, exec: bool, aside: Option<PathBuf> },
    /// Put a folder in place, replacing the old one.
    ReplaceDir { from: PathBuf, to: PathBuf },
    /// Move the old `current` to `old` and `new` to `current`; `old` is removed unless `keep_old`.
    Swap { new: PathBuf, current: PathBuf, old: PathBuf, keep_old: bool },
    /// Delete a file or folder (nothing happens when it is not there).
    Remove(PathBuf),
    /// Run a program.
    Run { program: String, args: Vec<OsString> },
}

/// What an install does: the steps, and the steps that run in any case after them (to detach a disk image).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Plan {
    pub ops: Vec<Op>,
    pub cleanup: Vec<Op>,
}

/// The name of the program file of a kind of install.
fn program_name(kind: &InstallKind) -> &'static str {
    match kind {
        InstallKind::Windows { .. } => "shale.exe",
        _ => "shale",
    }
}

/// The steps for the other files of an unpacked download: files are put in place, folders replaced.
fn others(entries: &[(String, bool)], src: &Path, dir: &Path, program: &str) -> Vec<Op> {
    entries
        .iter()
        .filter(|(n, _)| n != program)
        .map(|(n, is_dir)| if *is_dir { Op::ReplaceDir { from: src.join(n), to: dir.join(n) } } else { Op::Install { from: src.join(n), to: dir.join(n), exec: n == "install.sh", aside: None } })
        .collect()
}

/// The steps for an install of `kind`. `src` is the folder of the unpacked download with the
/// top-level `entries` (name, is a folder); for macOS it is the disk image and `mount` the folder
/// to mount it on. Nothing for a kind that is never replaced.
pub fn plan(kind: &InstallKind, src: &Path, entries: &[(String, bool)], mount: &Path) -> Result<Plan, String> {
    let program = program_name(kind);
    match kind {
        InstallKind::PackageManaged | InstallKind::Development => Err("this install is never replaced by Shale".into()),
        InstallKind::LinuxPortable { dir, whole } => {
            let mut ops = vec![Op::CopyFile { from: dir.join(program), to: dir.join(format!("{program}.old")) }, Op::Install { from: src.join(program), to: dir.join(program), exec: true, aside: None }];
            if *whole {
                ops.extend(others(entries, src, dir, program));
            }
            Ok(Plan { ops, cleanup: vec![] })
        }
        InstallKind::LinuxInstalled { prefix, user_default } => {
            let bin = prefix.join("bin");
            let mut args: Vec<OsString> = vec![src.join("install.sh").into_os_string()];
            if !user_default {
                args.push("--prefix".into());
                args.push(prefix.clone().into_os_string());
            }
            Ok(Plan { ops: vec![Op::CopyFile { from: bin.join(program), to: bin.join(format!("{program}.old")) }, Op::Run { program: "sh".into(), args }], cleanup: vec![] })
        }
        InstallKind::Windows { dir } => {
            let mut ops = vec![Op::Install { from: src.join(program), to: dir.join(program), exec: false, aside: Some(dir.join(format!("{program}.old"))) }];
            ops.extend(others(entries, src, dir, program));
            Ok(Plan { ops, cleanup: vec![] })
        }
        InstallKind::MacApp { bundle } => {
            let parent = bundle.parent().ok_or("the app has no folder")?;
            let name = bundle.file_name().ok_or("the app has no name")?.to_string_lossy().into_owned();
            let (new, old) = (parent.join(format!("{name}.new")), parent.join(format!("{name}.old")));
            let detach = Op::Run { program: "hdiutil".into(), args: vec!["detach".into(), mount.into()] };
            let ops = vec![
                Op::Remove(new.clone()),
                Op::Remove(old.clone()),
                Op::Run { program: "hdiutil".into(), args: vec!["attach".into(), "-readonly".into(), "-nobrowse".into(), "-mountpoint".into(), mount.into(), src.into()] },
                Op::Run { program: "ditto".into(), args: vec![mount.join("Shale.app").into_os_string(), new.clone().into_os_string()] },
                detach.clone(),
                Op::Swap { new, current: bundle.clone(), old, keep_old: false },
            ];
            Ok(Plan { ops, cleanup: vec![detach] })
        }
    }
}

// ---------------------------------------------------------------- running a plan

fn io_err(what: &str, path: &Path, e: std::io::Error) -> String {
    format!("{what} {} ({e})", path.display())
}

/// `<name>.new` next to `path`.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).map_err(|e| io_err("could not make it a program:", path, e))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), String> {
    Ok(())
}

/// Copy `from` to `tmp` (replacing a leftover of an earlier try).
fn copy_to(from: &Path, tmp: &Path) -> Result<(), String> {
    let _ = std::fs::remove_file(tmp);
    crate::fsutil::copy_new(from, tmp).map_err(|e| format!("{} could not be copied to {} ({e})", from.display(), tmp.display()))
}

fn remove_any(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_err("could not look at", path, e)),
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path).map_err(|e| io_err("could not remove", path, e)),
        Ok(_) => std::fs::remove_file(path).map_err(|e| io_err("could not remove", path, e)),
    }
}

fn run_op(op: &Op, cancel: &AtomicBool) -> Result<(), String> {
    match op {
        Op::CopyFile { from, to } => {
            if !from.is_file() {
                return Ok(());
            }
            let tmp = sibling(to, ".tmp");
            copy_to(from, &tmp)?;
            crate::fsutil::rename_replace(&tmp, to).map_err(|e| io_err("could not put the copy in place:", to, e))
        }
        Op::Install { from, to, exec, aside } => {
            let tmp = sibling(to, ".new");
            copy_to(from, &tmp)?;
            if *exec {
                make_executable(&tmp)?;
            }
            let moved = match aside {
                Some(aside) if to.exists() => {
                    if let Err(e) = crate::fsutil::rename_replace(to, aside) {
                        let _ = std::fs::remove_file(&tmp);
                        return Err(io_err("could not move the running program aside:", to, e));
                    }
                    true
                }
                _ => false,
            };
            if let Err(e) = crate::fsutil::rename_replace(&tmp, to) {
                if let (true, Some(aside)) = (moved, aside) {
                    let _ = crate::fsutil::rename_replace(aside, to);
                }
                let _ = std::fs::remove_file(&tmp);
                return Err(io_err("could not put the new file in place:", to, e));
            }
            Ok(())
        }
        Op::ReplaceDir { from, to } => {
            let tmp = sibling(to, ".new");
            remove_any(&tmp)?;
            std::fs::create_dir(&tmp).map_err(|e| io_err("could not make", &tmp, e))?;
            archive::copy_folder(from, &tmp)?;
            remove_any(to)?;
            crate::fsutil::rename_replace(&tmp, to).map_err(|e| io_err("could not put the new folder in place:", to, e))
        }
        Op::Swap { new, current, old, keep_old } => {
            crate::fsutil::rename_replace(current, old).map_err(|e| io_err("could not move the old version aside:", current, e))?;
            if let Err(e) = crate::fsutil::rename_replace(new, current) {
                let _ = crate::fsutil::rename_replace(old, current);
                return Err(io_err("could not put the new version in place:", current, e));
            }
            if *keep_old {
                Ok(())
            } else {
                remove_any(old)
            }
        }
        Op::Remove(path) => remove_any(path),
        Op::Run { program, args } => {
            let mut c = Command::new(program);
            c.args(args);
            let run = proc::run(c, RUN_TIMEOUT, Some(cancel), |_| {}).map_err(|e| format!("could not run {program} ({e})"))?;
            match run.ending {
                Ending::Cancelled => Err("cancelled".into()),
                Ending::TimedOut => Err(format!("{program} took too long and was stopped")),
                Ending::Exited(Some(0)) => Ok(()),
                Ending::Exited(_) => Err(format!("{program} failed: {}", run.tail_text())),
            }
        }
    }
}

/// Run the steps of a plan; the first that fails ends it (the steps of `cleanup` still run).
pub fn execute(plan: &Plan, cancel: &AtomicBool) -> Result<(), String> {
    let result = plan.ops.iter().try_for_each(|op| run_op(op, cancel));
    for op in &plan.cleanup {
        let _ = run_op(op, cancel);
    }
    result
}

// ---------------------------------------------------------------- the whole install

/// How the new version is started after an update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Relaunch {
    pub program: PathBuf,
    pub args: Vec<OsString>,
}

/// The option the new program gets: it waits for the old one to let go of the single instance lock.
pub const RESTARTED: &str = "--restarted";

impl Relaunch {
    /// Start it, detached from this process.
    pub fn spawn(&self) -> Result<(), String> {
        let mut c = Command::new(&self.program);
        c.args(&self.args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        crate::program::spawn(&mut c).map(|_| ()).map_err(|e| format!("could not start {} ({e})", self.program.display()))
    }
}

/// How to start the program of an install again.
pub fn relaunch_for(kind: &InstallKind) -> Option<Relaunch> {
    let plain = |program: PathBuf| Some(Relaunch { program, args: vec![RESTARTED.into()] });
    match kind {
        InstallKind::LinuxInstalled { prefix, .. } => plain(prefix.join("bin").join(program_name(kind))),
        InstallKind::LinuxPortable { dir, .. } | InstallKind::Windows { dir } => plain(dir.join(program_name(kind))),
        InstallKind::MacApp { bundle } => Some(Relaunch { program: "open".into(), args: vec!["-n".into(), bundle.clone().into_os_string(), "--args".into(), RESTARTED.into()] }),
        InstallKind::PackageManaged | InstallKind::Development => None,
    }
}

/// An update that was put in place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    pub version: String,
    pub relaunch: Relaunch,
}

/// The entries of a folder, folders and files, in name order.
fn list(dir: &Path) -> Result<Vec<(String, bool)>, String> {
    let mut out: Vec<(String, bool)> = std::fs::read_dir(dir).map_err(|e| io_err("could not read", dir, e))?.flatten().map(|e| (e.file_name().to_string_lossy().into_owned(), e.path().is_dir())).collect();
    out.sort();
    Ok(out)
}

/// Unpack the download of a Linux or Windows install and give its folder (the one with the program).
fn unpack(kind: &InstallKind, staged: &Staged) -> Result<PathBuf, String> {
    let dest = staged.dir.join("unpacked");
    remove_any(&dest)?;
    std::fs::create_dir_all(&dest).map_err(|e| io_err("could not make", &dest, e))?;
    let bytes = crate::fsutil::read(&staged.archive).map_err(|e| io_err("could not read", &staged.archive, e))?;
    archive::extract_with(&bytes, &dest, LIMITS)?;
    let root = archive::single_root(&dest);
    if !root.join(program_name(kind)).is_file() {
        return Err(format!("the download has no {}, so it is not installed", program_name(kind)));
    }
    Ok(root)
}

// The dataflow spans of an install (DEV-032): one flow for each kind the tests run on Linux. The
// other kinds have none yet, so their spans are disabled.
fn flow_span(kind: &InstallKind) -> tracing::Span {
    match kind {
        InstallKind::LinuxPortable { .. } => tracing::info_span!("flow.update_install_folder", peer = "app"),
        InstallKind::LinuxInstalled { .. } => tracing::info_span!("flow.update_install_script", peer = "app"),
        _ => tracing::Span::none(),
    }
}

fn unpack_span(kind: &InstallKind) -> tracing::Span {
    match kind {
        InstallKind::LinuxPortable { .. } => tracing::info_span!("flow.update_install_folder.unpack", peer = "disk"),
        InstallKind::LinuxInstalled { .. } => tracing::info_span!("flow.update_install_script.unpack", peer = "disk"),
        _ => tracing::Span::none(),
    }
}

fn run_span(kind: &InstallKind) -> tracing::Span {
    match kind {
        InstallKind::LinuxPortable { .. } => tracing::info_span!("flow.update_install_folder.replace_files", peer = "disk"),
        InstallKind::LinuxInstalled { .. } => tracing::info_span!("flow.update_install_script.run_install_sh", peer = "installer"),
        _ => tracing::Span::none(),
    }
}

/// Put the verified download `staged` in place for an install of `kind`. On an error the old
/// version is still in place.
pub fn apply(kind: &InstallKind, staged: &Staged, cancel: &AtomicBool) -> Result<Installed, String> {
    let relaunch = relaunch_for(kind).ok_or("this install is never replaced by Shale")?;
    let _flow = flow_span(kind).entered();
    let plan = if let InstallKind::MacApp { .. } = kind {
        let mount = staged.dir.join("mount");
        std::fs::create_dir_all(&mount).map_err(|e| io_err("could not make", &mount, e))?;
        plan(kind, &staged.archive, &[], &mount)?
    } else {
        let root = unpack_span(kind).in_scope(|| unpack(kind, staged))?;
        plan(kind, &root, &list(&root)?, &staged.dir)?
    };
    run_span(kind).in_scope(|| execute(&plan, cancel))?;
    let _ = std::fs::remove_dir_all(&staged.dir);
    Ok(Installed { version: staged.version.clone(), relaunch })
}

/// The backups an earlier update left, which are removed once the new version has run: the
/// `shale.old` next to a program called `shale` (not when the old program itself is what runs),
/// the `shale.exe.old` of Windows.
pub fn leftovers(kind: &InstallKind, exe: &Path) -> Vec<PathBuf> {
    let running = exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match kind {
        InstallKind::LinuxInstalled { prefix, .. } if running == "shale" => vec![prefix.join("bin").join("shale.old")],
        InstallKind::LinuxPortable { dir, .. } if running == "shale" => vec![dir.join("shale.old")],
        InstallKind::Windows { dir } if running == "shale.exe" => vec![dir.join("shale.exe.old")],
        _ => vec![],
    }
}

/// Remove the leftover backups (what cannot be removed stays for the next start).
pub fn remove_leftovers(kind: &InstallKind, exe: &Path) {
    for p in leftovers(kind, exe) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(tag: &str) -> PathBuf {
        super::super::logic_tests::temp(tag)
    }

    // spec: UPD-014
    #[test]
    fn upd_014_remove_any_accepts_a_missing_path_and_removes_a_file_a_folder_and_only_the_link_of_a_link() {
        let d = temp("remove-any");
        remove_any(&d.join("nothing")).unwrap();
        std::fs::write(d.join("file"), b"x").unwrap();
        remove_any(&d.join("file")).unwrap();
        assert!(!d.join("file").exists());
        std::fs::create_dir_all(d.join("dir/sub/deeper")).unwrap();
        std::fs::write(d.join("dir/sub/deeper/f"), b"x").unwrap();
        remove_any(&d.join("dir")).unwrap();
        assert!(!d.join("dir").exists(), "the folder goes with everything in it");
        std::fs::create_dir(d.join("target")).unwrap();
        std::fs::write(d.join("target/keep"), b"x").unwrap();
        std::os::unix::fs::symlink(d.join("target"), d.join("link")).unwrap();
        remove_any(&d.join("link")).unwrap();
        assert!(std::fs::symlink_metadata(d.join("link")).is_err());
        assert!(d.join("target/keep").exists(), "what the link pointed to stays");
    }

    // spec: UPD-014
    #[test]
    fn upd_014_remove_any_says_in_words_when_a_path_cannot_be_removed() {
        let d = temp("remove-any-fail");
        std::fs::create_dir(d.join("locked")).unwrap();
        std::fs::write(d.join("locked/f"), b"x").unwrap();
        std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
        let can_write = std::fs::write(d.join("locked/probe"), b"x").is_ok();
        let result = remove_any(&d.join("locked/f"));
        std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
        if can_write {
            return; // running as root: permissions do not stop us
        }
        let e = result.unwrap_err();
        assert!(e.contains("could not remove") && e.contains("locked/f"), "{e}");
        assert!(d.join("locked/f").exists());
    }
}
