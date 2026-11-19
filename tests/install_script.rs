//! `scripts/install.sh` (BUILD-025), the installer inside the Linux download, run against a fake
//! package folder (a dummy `shale`, the icons, the desktop file) with HOME and XDG_DATA_HOME in a
//! temp folder. Unix only: the script is POSIX `sh` and uses coreutils `install`.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const SIZES: [u32; 8] = [16, 32, 48, 64, 128, 256, 512, 1024];

/// A fresh temp folder with a package (`pkg/`), a `home/` and a `data/` folder.
struct Env {
    base: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let base = std::env::temp_dir().join(format!("shale-install-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let pkg = base.join("pkg");
        fs::create_dir_all(pkg.join("assets/icon")).unwrap();
        fs::create_dir_all(base.join("home")).unwrap();
        fs::write(pkg.join("shale"), "#!/bin/sh\necho dummy shale v1\n").unwrap();
        fs::write(pkg.join("shale.svg"), "<svg/>").unwrap();
        fs::copy(Path::new(ROOT).join("assets/icon/shale.desktop"), pkg.join("shale.desktop")).unwrap();
        for n in SIZES {
            fs::write(pkg.join(format!("assets/icon/shale-{n}.png")), format!("png{n}")).unwrap();
        }
        fs::copy(Path::new(ROOT).join("scripts/install.sh"), pkg.join("install.sh")).unwrap();
        fs::set_permissions(pkg.join("install.sh"), fs::Permissions::from_mode(0o755)).unwrap();
        // a PATH of its own with only the tools the script needs, so the host's cache tools
        // (update-desktop-database, gtk-update-icon-cache) do not write into the test folders
        let tools = base.join("tools");
        fs::create_dir_all(&tools).unwrap();
        for t in ["sh", "install", "mkdir", "rm", "sed", "chmod", "mv", "dirname", "cat", "ldd"] {
            if let Some(p) = ["/usr/bin", "/bin"].iter().map(|d| Path::new(d).join(t)).find(|p| p.exists()) {
                std::os::unix::fs::symlink(p, tools.join(t)).unwrap();
            }
        }
        Env { base }
    }
    fn tools(&self) -> PathBuf {
        self.base.join("tools")
    }
    fn pkg(&self) -> PathBuf {
        self.base.join("pkg")
    }
    fn home(&self) -> PathBuf {
        self.base.join("home")
    }
    fn data(&self) -> PathBuf {
        self.base.join("data")
    }
    /// Runs the script from another working directory, with HOME and XDG_DATA_HOME set.
    fn run(&self, args: &[&str]) -> Output {
        Command::new("sh").arg(self.pkg().join("install.sh")).args(args).current_dir(&self.base).env("HOME", self.home()).env("XDG_DATA_HOME", self.data()).env("PATH", self.tools()).output().unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn mode(p: &Path) -> u32 {
    fs::metadata(p).unwrap().permissions().mode() & 0o777
}

/// Every file below `dir`, relative, sorted.
fn files(dir: &Path) -> Vec<String> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
        let Ok(rd) = fs::read_dir(dir) else { return };
        for e in rd {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(base, &p, out);
            } else {
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

fn check_installed(bin: &Path, share: &Path) {
    assert_eq!(mode(bin), 0o755, "program mode");
    assert!(fs::read_to_string(bin).unwrap().starts_with("#!/bin/sh"), "program content");
    let svg = share.join("icons/hicolor/scalable/apps/shale.svg");
    assert_eq!((fs::read_to_string(&svg).unwrap(), mode(&svg)), ("<svg/>".to_string(), 0o644));
    for n in SIZES {
        let png = share.join(format!("icons/hicolor/{n}x{n}/apps/shale.png"));
        assert_eq!(fs::read_to_string(&png).unwrap(), format!("png{n}"));
        assert_eq!(mode(&png), 0o644);
    }
    let desktop = share.join("applications/shale.desktop");
    assert_eq!(mode(&desktop), 0o644);
    let text = fs::read_to_string(&desktop).unwrap();
    let exec: Vec<&str> = text.lines().filter(|l| l.starts_with("Exec=")).collect();
    let want = if bin.to_str().unwrap().contains(' ') { format!("Exec=\"{}\"", bin.display()) } else { format!("Exec={}", bin.display()) };
    assert_eq!(exec, [want], "Exec is the absolute path");
    for keep in ["[Desktop Entry]", "Name=Shale", "Icon=shale", "Categories=Office;", "StartupWMClass=shale"] {
        assert!(text.lines().any(|l| l == keep), "desktop file keeps {keep}");
    }
}

// spec: BUILD-025
#[test]
fn build_025_per_user_install_puts_every_file_in_place() {
    let e = Env::new("user");
    let out = e.run(&[]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    check_installed(&e.home().join(".local/bin/shale"), &e.data());
    // nothing else was written
    assert_eq!(files(&e.home()), [".local/bin/shale"]);
    assert_eq!(files(&e.data()).len(), SIZES.len() + 2);
    // the bin dir is not on PATH: a warning, not a failure
    assert!(String::from_utf8_lossy(&out.stderr).contains("not on your PATH"));
}

// spec: BUILD-025
#[test]
fn build_025_without_xdg_data_home_the_data_goes_to_local_share() {
    let e = Env::new("noxdg");
    let out = Command::new("sh").arg(e.pkg().join("install.sh")).env("HOME", e.home()).env_remove("XDG_DATA_HOME").env("PATH", format!("{}:{}", e.tools().display(), e.home().join(".local/bin").display())).output().unwrap();
    assert!(out.status.success());
    check_installed(&e.home().join(".local/bin/shale"), &e.home().join(".local/share"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("not on your PATH"), "bin dir on PATH: no warning");
}

// spec: BUILD-025
#[test]
fn build_025_prefix_installs_below_the_prefix_and_uninstall_removes_exactly_those_files() {
    let e = Env::new("prefix");
    let prefix = e.base.join("usr local");
    let p = prefix.to_str().unwrap();
    let out = e.run(&["--prefix", p]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    check_installed(&prefix.join("bin/shale"), &prefix.join("share"));
    assert!(fs::read_to_string(prefix.join("share/applications/shale.desktop")).unwrap().contains(&format!("Exec=\"{}/bin/shale\"", p)), "a path with a space is quoted");
    assert!(files(&e.home()).is_empty() && !e.data().exists(), "the user's folders are untouched");

    // unrelated files, also in the same folders, survive the uninstall
    fs::write(prefix.join("bin/other"), "x").unwrap();
    fs::write(prefix.join("share/applications/other.desktop"), "x").unwrap();
    fs::write(prefix.join("share/icons/hicolor/48x48/apps/other.png"), "x").unwrap();
    let out = e.run(&["--uninstall", "--prefix", p]);
    assert!(out.status.success());
    assert_eq!(files(&prefix), ["bin/other", "share/applications/other.desktop", "share/icons/hicolor/48x48/apps/other.png"]);
}

// spec: BUILD-025
#[test]
fn build_025_uninstall_per_user_leaves_unrelated_files_and_reinstall_upgrades_in_place() {
    let e = Env::new("reinstall");
    assert!(e.run(&[]).status.success());
    let bin = e.home().join(".local/bin/shale");
    fs::write(e.home().join(".local/bin/mine"), "x").unwrap();
    fs::write(e.data().join("applications/mine.desktop"), "x").unwrap();

    // a newer package: the same command replaces the program
    fs::write(e.pkg().join("shale"), "#!/bin/sh\necho dummy shale v2\n").unwrap();
    let out = e.run(&[]);
    assert!(out.status.success());
    assert!(fs::read_to_string(&bin).unwrap().contains("v2"));
    check_installed(&bin, &e.data());

    let out = e.run(&["--uninstall"]);
    assert!(out.status.success());
    assert_eq!(files(&e.home()), [".local/bin/mine"]);
    assert_eq!(files(&e.data()), ["applications/mine.desktop"]);
    // uninstalling again is not an error
    assert!(e.run(&["--uninstall"]).status.success());
}

// spec: BUILD-025
#[test]
fn build_025_unknown_option_prints_usage_and_exits_2_and_help_exits_0() {
    let e = Env::new("options");
    let out = e.run(&["--frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Usage: install.sh"));
    let out = e.run(&["--prefix"]);
    assert_eq!(out.status.code(), Some(2));
    let out = e.run(&["--help"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stdout).contains("--uninstall"));
    assert!(files(&e.home()).is_empty() && !e.data().exists(), "nothing was installed");
}

// spec: BUILD-025
#[test]
fn build_025_a_missing_program_fails_and_installs_nothing() {
    let e = Env::new("missing");
    fs::remove_file(e.pkg().join("shale")).unwrap();
    let out = e.run(&[]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("'shale' is not next to this script"));
    assert!(files(&e.home()).is_empty() && !e.data().exists());
}

// spec: BUILD-025
#[test]
fn build_025_missing_libraries_are_named_with_a_hint() {
    let e = Env::new("ldd");
    // a fake `ldd` first on PATH reports a missing library
    let tools = e.tools();
    let _ = fs::remove_file(tools.join("ldd"));
    // and a cache tool that fails: ignored
    fs::write(tools.join("gtk-update-icon-cache"), "#!/bin/sh\nexit 1\n").unwrap();
    fs::set_permissions(tools.join("gtk-update-icon-cache"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(tools.join("ldd"), "#!/bin/sh\necho '\tlibasound.so.2 => not found'\necho '\tlibc.so.6 => /lib/libc.so.6 (0x1)'\n").unwrap();
    fs::set_permissions(tools.join("ldd"), fs::Permissions::from_mode(0o755)).unwrap();
    let out = Command::new("sh").arg(e.pkg().join("install.sh")).env("HOME", e.home()).env("XDG_DATA_HOME", e.data()).env("PATH", &tools).output().unwrap();
    assert!(out.status.success(), "a missing library is a warning");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("libasound.so.2") && err.contains("pacman -S alsa-lib") && err.contains("apt install libasound2"), "{err}");
    assert!(!err.contains("libc.so.6"));
}

// spec: BUILD-025
#[test]
fn build_025_both_workflows_pack_the_installer_and_the_icons() {
    for wf in [".github/workflows/build.yml", ".forgejo/workflows/ci.yml"] {
        let text = fs::read_to_string(Path::new(ROOT).join(wf)).unwrap();
        assert!(text.contains("scripts/install.sh dist/shale/"), "{wf}: install.sh is not packed");
        assert!(text.contains("assets/icon/shale-*.png dist/shale/assets/icon/"), "{wf}: the PNG icons are not packed");
    }
    let mode = fs::metadata(Path::new(ROOT).join("scripts/install.sh")).unwrap().permissions().mode();
    assert_ne!(mode & 0o100, 0, "scripts/install.sh is executable");
}
