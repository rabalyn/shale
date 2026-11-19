//! How this copy of Shale was installed (UPD-011 to UPD-016), found from the path of the running
//! program, and what an update may do to it. Everything here is a function of a path and a few
//! facts about the disk, so every kind can be tested on any system.

use std::path::{Path, PathBuf};

/// The systems with their own way to install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// How this copy was installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallKind {
    /// `<prefix>/bin/shale`, put there by `install.sh` (`~/.local` or a `--prefix`).
    LinuxInstalled { prefix: PathBuf, user_default: bool },
    /// Run from a folder: the unpacked download (`whole`: with its `install.sh`), or a bare program.
    LinuxPortable { dir: PathBuf, whole: bool },
    /// Owned by the system or a package manager (`/usr/bin`, the AUR's `shale-bin`): never replaced.
    PackageManaged,
    /// Inside `Shale.app/Contents/MacOS`.
    MacApp { bundle: PathBuf },
    /// In the folder of the unpacked zip.
    Windows { dir: PathBuf },
    /// A cargo build (`target/debug`, `target/release`): never replaced.
    Development,
}

/// What is known about the disk around the program, for [`detect`].
#[derive(Clone, Debug)]
pub struct Facts {
    pub os: Os,
    pub home: Option<PathBuf>,
    /// `install.sh` lies next to the program: it is the unpacked download.
    pub install_sh_next_to_it: bool,
    /// `<prefix>/share/applications/shale.desktop` exists for the prefix above the program's `bin`.
    pub desktop_file_in_prefix: bool,
}

impl Facts {
    /// The facts for the program at `exe` on this computer.
    pub fn gather(exe: &Path) -> Facts {
        let dir = exe.parent().unwrap_or(Path::new("."));
        let prefix = dir.parent().unwrap_or(Path::new("."));
        Facts { os: Os::current(), home: dirs::home_dir(), install_sh_next_to_it: dir.join("install.sh").is_file(), desktop_file_in_prefix: prefix.join("share").join("applications").join("shale.desktop").is_file() }
    }
}

/// The parts of a path as text.
fn names(path: &Path) -> Vec<String> {
    path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect()
}

/// The folder `Shale.app` of a path inside `Shale.app/Contents/MacOS/`.
fn mac_bundle(exe: &Path) -> Option<PathBuf> {
    exe.ancestors()
        .skip(1)
        .find(|a| a.extension().is_some_and(|e| e == "app"))
        .filter(|app| {
            let rest = exe.strip_prefix(app).map(names).unwrap_or_default();
            rest.len() == 3 && rest[0] == "Contents" && rest[1] == "MacOS"
        })
        .map(Path::to_path_buf)
}

/// Whether the path is in a cargo `target` folder.
fn in_target(exe: &Path) -> bool {
    let n = names(exe);
    n.windows(2).any(|w| w[0] == "target" && !w[1].is_empty()) && n.iter().any(|c| matches!(c.as_str(), "debug" | "release" | "bench-release"))
}

/// Whether a Linux path belongs to the system or a package manager (`/usr/local` does not: it is
/// where `install.sh --prefix /usr/local` puts Shale).
fn system_owned(exe: &Path) -> bool {
    let p = exe.to_string_lossy();
    if p.starts_with("/usr/local/") {
        return false;
    }
    ["/usr/", "/bin/", "/sbin/", "/nix/store/", "/snap/", "/app/", "/var/lib/flatpak/", "/run/current-system/"].iter().any(|s| p.starts_with(s))
}

/// How the program at `exe` (its canonical path) was installed.
pub fn detect(exe: &Path, facts: &Facts) -> InstallKind {
    if in_target(exe) {
        return InstallKind::Development;
    }
    let dir = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
    if let Some(bundle) = mac_bundle(exe) {
        return InstallKind::MacApp { bundle };
    }
    match facts.os {
        Os::Windows => InstallKind::Windows { dir },
        Os::Mac => InstallKind::Development,
        Os::Linux => {
            if facts.install_sh_next_to_it {
                return InstallKind::LinuxPortable { dir, whole: true };
            }
            if system_owned(exe) {
                return InstallKind::PackageManaged;
            }
            let is_bin = dir.file_name().is_some_and(|n| n == "bin") && exe.file_name().is_some_and(|n| n == "shale");
            match dir.parent() {
                Some(prefix) if is_bin && (facts.desktop_file_in_prefix || facts.home.as_deref().is_some_and(|h| h.join(".local") == prefix)) => {
                    InstallKind::LinuxInstalled { prefix: prefix.to_path_buf(), user_default: facts.home.as_deref().is_some_and(|h| h.join(".local") == prefix) }
                }
                _ => InstallKind::LinuxPortable { dir, whole: false },
            }
        }
    }
}

/// What an update may do on this install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Capability {
    /// Download, verify and install.
    Install,
    /// Download and verify; the person installs by hand, because the program's folder cannot be
    /// written (the reason says which).
    DownloadOnly(String),
    /// Only say that there is a new version, with the way to get it (the reason).
    NotifyOnly(String),
}

/// What an update may do on an install; `writable` says whether the folders it would change can be written.
pub fn capability(kind: &InstallKind, writable: bool) -> Capability {
    match kind {
        InstallKind::PackageManaged => Capability::NotifyOnly("Shale is managed by your package manager, which owns its files. Update Shale with your package manager (for example `yay -Syu shale-bin`).".into()),
        InstallKind::Development => Capability::NotifyOnly("This is a development build (a cargo target folder); it is never replaced. Build the new version from the source.".into()),
        _ if writable => Capability::Install,
        k => Capability::DownloadOnly(format!("The folder {} cannot be written by you, so Shale cannot replace itself there.", install_folder(k).display())),
    }
}

/// The folder an update writes into (the one that has to be writable).
pub fn install_folder(kind: &InstallKind) -> PathBuf {
    match kind {
        InstallKind::LinuxInstalled { prefix, .. } => prefix.join("bin"),
        InstallKind::LinuxPortable { dir, .. } | InstallKind::Windows { dir } => dir.clone(),
        InstallKind::MacApp { bundle } => bundle.parent().unwrap_or(Path::new("/")).to_path_buf(),
        InstallKind::PackageManaged | InstallKind::Development => PathBuf::new(),
    }
}

/// Whether the person can write into the folder an update changes: a file is made there and removed.
pub fn is_writable(folder: &Path) -> bool {
    let probe = folder.join(format!(".shale-update-probe-{}", std::process::id()));
    let ok = crate::fsutil::create_new(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// A line for the settings: how this copy was installed.
pub fn describe(kind: &InstallKind) -> String {
    match kind {
        InstallKind::LinuxInstalled { prefix, .. } => format!("Linux, installed with install.sh into {}", prefix.display()),
        InstallKind::LinuxPortable { dir, whole: true } => format!("Linux, run from the unpacked download in {}", dir.display()),
        InstallKind::LinuxPortable { dir, whole: false } => format!("Linux, a program file in {}", dir.display()),
        InstallKind::PackageManaged => "Linux, installed by a package manager".into(),
        InstallKind::MacApp { bundle } => format!("macOS, the app {}", bundle.display()),
        InstallKind::Windows { dir } => format!("Windows, run from the unpacked download in {}", dir.display()),
        InstallKind::Development => "a development build".into(),
    }
}

/// What the person can run to install the downloaded `archive` themselves (the folder cannot be written).
pub fn manual_command(kind: &InstallKind, archive: &Path, staging: &Path) -> String {
    let q = |p: &Path| format!("\"{}\"", p.display());
    match kind {
        InstallKind::LinuxInstalled { prefix, user_default } => {
            let prefix_arg = if *user_default { String::new() } else { format!(" --prefix {}", q(prefix)) };
            format!("tar -xzf {} -C {} && sudo sh {}{prefix_arg}", q(archive), q(staging), q(&staging.join("shale").join("install.sh")))
        }
        InstallKind::LinuxPortable { dir, .. } => format!("tar -xzf {} -C {} --strip-components=1", q(archive), q(dir)),
        InstallKind::MacApp { .. } => format!("open {}   (then drag Shale.app to the Applications folder)", q(archive)),
        InstallKind::Windows { dir } => format!("unpack {} into {} (after closing Shale)", q(archive), q(dir)),
        InstallKind::PackageManaged | InstallKind::Development => String::new(),
    }
}
