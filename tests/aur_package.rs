//! The AUR package `shale-bin` (BUILD-026): `packaging/aur/shale-bin/{PKGBUILD,.SRCINFO}` and
//! `scripts/aur-update.sh`, which the script runs against a fake `gh`, `curl` and `makepkg` on a
//! private PATH of its own. Unix only: the script is POSIX `sh`.
#![cfg(unix)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

#[path = "../src/test_exec.rs"]
mod test_exec;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");
const SUM_A: &str = "74abed4af9864825232e87943894d05b79da4d89e883b59e8d56fd0454cd8cb3";
const SUM_B: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn read(rel: &str) -> String {
    fs::read_to_string(Path::new(ROOT).join(rel)).unwrap()
}

/// The value of `key=` in the PKGBUILD (quotes, parentheses and line breaks of arrays kept as text).
fn pkgbuild_scalar(text: &str, key: &str) -> String {
    let prefix = format!("{key}=");
    text.lines().find_map(|l| l.strip_prefix(&prefix)).unwrap_or_else(|| panic!("no {key}= in PKGBUILD")).trim_matches(|c| c == '\'' || c == '"').to_string()
}

/// The quoted words of the array `key=(…)` of the PKGBUILD.
fn pkgbuild_array(text: &str, key: &str) -> Vec<String> {
    let start = text.find(&format!("\n{key}=(")).unwrap_or_else(|| panic!("no {key}=( in PKGBUILD")) + key.len() + 3;
    let body = &text[start..];
    let mut out = Vec::new();
    let mut quote: Option<(char, String)> = None;
    for c in body.chars() {
        match (&mut quote, c) {
            (None, ')') => break,
            (None, '\'' | '"') => quote = Some((c, String::new())),
            (Some((q, _)), c) if c == *q => out.push(quote.take().unwrap().1),
            (Some((_, s)), c) => s.push(c),
            (None, _) => {}
        }
    }
    out
}

/// The values of every `key = value` line of the `.SRCINFO`.
fn srcinfo(text: &str, key: &str) -> Vec<String> {
    text.lines().filter_map(|l| l.trim().strip_prefix(&format!("{key} = "))).map(str::to_string).collect()
}

/// A sortable key of a semver version: the numbers of its X.Y.Z core (the part before the first
/// `-`) and whether it has no pre-release part, so a release ranks above a pre-release of the same core.
fn version_key(v: &str) -> (Vec<u64>, bool) {
    let (core, pre) = match v.split_once('-') {
        Some((core, _)) => (core, false),
        None => (v, true),
    };
    (core.split('.').map(|p| p.parse().unwrap()).collect(), pre)
}

// spec: BUILD-026
#[test]
fn build_026_srcinfo_agrees_with_the_pkgbuild() {
    let pkgbuild = read("packaging/aur/shale-bin/PKGBUILD");
    let info = read("packaging/aur/shale-bin/.SRCINFO");
    assert_eq!(srcinfo(&info, "pkgbase"), ["shale-bin"]);
    assert_eq!(pkgbuild_scalar(&pkgbuild, "pkgname"), "shale-bin");
    assert_eq!(srcinfo(&info, "pkgver"), [pkgbuild_scalar(&pkgbuild, "pkgver")]);
    assert_eq!(srcinfo(&info, "pkgrel"), [pkgbuild_scalar(&pkgbuild, "pkgrel")]);
    assert_eq!(srcinfo(&info, "depends"), pkgbuild_array(&pkgbuild, "depends"));
    assert_eq!(srcinfo(&info, "optdepends"), pkgbuild_array(&pkgbuild, "optdepends"));
    assert_eq!(srcinfo(&info, "sha256sums"), pkgbuild_array(&pkgbuild, "sha256sums"));
    assert_eq!(srcinfo(&info, "provides"), ["shale"]);
    assert_eq!(srcinfo(&info, "conflicts"), ["shale"]);
    // the source is the renamed Linux download of the release with the version in the PKGBUILD
    let ver = pkgbuild_scalar(&pkgbuild, "pkgver");
    let source = srcinfo(&info, "source");
    assert_eq!(source, [format!("shale-bin-{ver}.tar.gz::https://github.com/rabalyn/shale/releases/download/v{ver}/shale-{ver}-linux-x86_64.tar.gz")]);
    let src_in_pkgbuild = pkgbuild_array(&pkgbuild, "source");
    assert_eq!(src_in_pkgbuild, ["${pkgname}-${pkgver}.tar.gz::https://github.com/rabalyn/shale/releases/download/v${pkgver}/shale-${pkgver}-linux-x86_64.tar.gz"]);
    assert_eq!(pkgbuild_array(&pkgbuild, "sha256sums")[0].len(), 64);
    for lib in ["alsa-lib", "libxkbcommon", "wayland", "libx11", "libxcursor", "libxrandr", "libxi", "libglvnd"] {
        assert!(srcinfo(&info, "depends").iter().any(|d| d == lib), "depends lacks {lib}");
    }
    // the files of the package
    for part in ["/usr/bin/shale", "shale.desktop", "hicolor/scalable/apps/shale.svg", "hicolor/${size}x${size}/apps/shale.png", "/usr/share/licenses/${pkgname}/LICENSE", "/usr/share/doc/shale"] {
        assert!(pkgbuild.contains(part), "PKGBUILD does not install {part}");
    }
}

// spec: BUILD-026
#[test]
fn build_026_the_packaged_version_is_a_release_not_newer_than_cargo_toml() {
    let pkgbuild = read("packaging/aur/shale-bin/PKGBUILD");
    let ver = pkgbuild_scalar(&pkgbuild, "pkgver");
    assert!(ver.chars().all(|c| c.is_ascii_digit() || c == '.') && ver.split('.').count() == 3, "pkgver {ver} is not X.Y.Z");
    let cargo = read("Cargo.toml");
    let cargo_ver = cargo.lines().find_map(|l| l.strip_prefix("version = ")).unwrap().trim_matches('"');
    assert!(version_key(&ver) <= version_key(cargo_ver), "pkgver {ver} is newer than Cargo.toml {cargo_ver}");
}

// spec: BUILD-026
#[test]
fn build_026_a_pre_release_cargo_version_is_compared_by_semver() {
    assert!(version_key("0.6.9") <= version_key("0.6.10-rc.1"));
    assert!(version_key("0.6.10") > version_key("0.6.10-rc.1"));
    assert!(version_key("0.6.10-rc.1") <= version_key("0.6.10"));
    assert!(version_key("0.6.10") <= version_key("0.6.10"));
}

// spec: BUILD-026
#[test]
fn build_026_srcinfo_is_what_makepkg_prints() {
    if Command::new("makepkg").arg("--version").output().is_err() {
        eprintln!("note: makepkg is not installed, so .SRCINFO is not compared with `makepkg --printsrcinfo`");
        return;
    }
    let out = Command::new("makepkg").arg("--printsrcinfo").current_dir(Path::new(ROOT).join("packaging/aur/shale-bin")).output().unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8(out.stdout).unwrap(), read("packaging/aur/shale-bin/.SRCINFO"));
}

// spec: BUILD-026
#[test]
fn build_026_update_script_uses_no_gnu_only_sed_in_place() {
    // BSD sed (macOS) reads the word after `-i` as a backup suffix, so `sed -i -e` fails there
    let script = read("scripts/aur-update.sh");
    for l in script.lines().filter(|l| !l.trim_start().starts_with('#')) {
        assert!(!l.contains("sed -i") && !l.contains("sed --in-place"), "GNU-only sed in scripts/aur-update.sh: {l}");
    }
}

/// A copy of the script, Cargo.toml and PKGBUILD in a temp tree, and a PATH with fake tools.
struct Env {
    base: PathBuf,
}

impl Env {
    fn new(name: &str) -> Env {
        let base = std::env::temp_dir().join(format!("shale-aur-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let pkg = base.join("repo/packaging/aur/shale-bin");
        fs::create_dir_all(&pkg).unwrap();
        fs::create_dir_all(base.join("repo/scripts")).unwrap();
        fs::create_dir_all(base.join("tools")).unwrap();
        fs::copy(Path::new(ROOT).join("scripts/aur-update.sh"), base.join("repo/scripts/aur-update.sh")).unwrap();
        fs::write(base.join("repo/Cargo.toml"), "[package]\nname = \"shale\"\nversion = \"0.7.0\"\n").unwrap();
        let pkgbuild = read("packaging/aur/shale-bin/PKGBUILD").replace("pkgrel=1", "pkgrel=3");
        fs::write(pkg.join("PKGBUILD"), pkgbuild).unwrap();
        for t in ["sh", "sed", "awk", "mktemp", "rm", "dirname", "cat", "head", "tr"] {
            if let Some(p) = ["/usr/bin", "/bin"].iter().map(|d| Path::new(d).join(t)).find(|p| p.exists()) {
                std::os::unix::fs::symlink(p, base.join("tools").join(t)).unwrap();
            }
        }
        Env { base }
    }
    fn fake(&self, name: &str, body: &str) {
        let p = self.base.join("tools").join(name);
        test_exec::write_script(&p, &format!("#!/bin/sh\n{body}\n"));
    }
    /// A `gh` that logs its arguments and writes the checksum file for `-p` into the `-D` folder
    /// when `sum` is given, and fails (no such release) otherwise.
    fn fake_gh(&self, sum: Option<&str>) {
        let write = match sum {
            Some(s) => format!("printf '%s  %s\\n' {s} \"$pat\" > \"$dir/$pat\""),
            None => "exit 1".to_string(),
        };
        self.fake("gh", &format!("echo \"gh $*\" >> \"{}/calls\"\nwhile [ $# -gt 0 ]; do case $1 in -p) pat=$2;; -D) dir=$2;; esac; shift; done\n{write}", self.base.display()));
    }
    fn fake_curl(&self, sum: Option<&str>) {
        let body = match sum {
            Some(s) => format!("echo \"curl $*\" >> \"{}/calls\"\nwhile [ $# -gt 0 ]; do case $1 in -o) out=$2;; esac; shift; done\nprintf '%s  file\\n' {s} > \"$out\"", self.base.display()),
            None => "exit 22".to_string(),
        };
        self.fake("curl", &body);
    }
    fn fake_makepkg(&self) {
        self.fake("makepkg", "echo \"printed by fake makepkg: $(sed -n 's/^pkgver=//p' PKGBUILD) $(sed -n 's/^pkgrel=//p' PKGBUILD)\"");
    }
    fn pkgbuild(&self) -> String {
        fs::read_to_string(self.base.join("repo/packaging/aur/shale-bin/PKGBUILD")).unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new("sh").arg(self.base.join("repo/scripts/aur-update.sh")).args(args).env("PATH", self.base.join("tools")).output().unwrap()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

// spec: BUILD-026
#[test]
fn build_026_update_script_writes_version_release_checksum_and_srcinfo() {
    let e = Env::new("update");
    e.fake_gh(Some(SUM_B));
    e.fake_curl(None);
    e.fake_makepkg();
    let out = e.run(&["0.6.0"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    let pkgbuild = e.pkgbuild();
    assert_eq!(pkgbuild_scalar(&pkgbuild, "pkgver"), "0.6.0");
    assert_eq!(pkgbuild_scalar(&pkgbuild, "pkgrel"), "1", "pkgrel is reset");
    assert_eq!(pkgbuild_array(&pkgbuild, "sha256sums"), [SUM_B]);
    assert!(!pkgbuild.contains(SUM_A));
    let info = fs::read_to_string(e.base.join("repo/packaging/aur/shale-bin/.SRCINFO")).unwrap();
    assert_eq!(info.trim(), "printed by fake makepkg: 0.6.0 1");
    let calls = fs::read_to_string(e.base.join("calls")).unwrap();
    assert!(calls.contains("release download v0.6.0 -R rabalyn/shale -p shale-0.6.0-linux-x86_64.tar.gz.sha256"), "{calls}");
    let stdout = text(&out.stdout);
    assert!(stdout.contains("ssh://aur@aur.archlinux.org/shale-bin.git") && stdout.contains("git push"), "{stdout}");
    // the script only prints the AUR steps: nothing but gh was asked to do anything
    assert!(!calls.contains("git "));
}

// spec: BUILD-026
#[test]
fn build_026_update_script_defaults_to_the_version_in_cargo_toml() {
    let e = Env::new("default");
    e.fake_gh(Some(SUM_B));
    e.fake_makepkg();
    let out = e.run(&[]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(pkgbuild_scalar(&e.pkgbuild(), "pkgver"), "0.7.0");
}

// spec: BUILD-026
#[test]
fn build_026_update_script_falls_back_to_curl_when_gh_is_missing() {
    let e = Env::new("curl");
    e.fake_curl(Some(SUM_B));
    e.fake_makepkg();
    let out = e.run(&["0.6.0"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(pkgbuild_array(&e.pkgbuild(), "sha256sums"), [SUM_B]);
    let calls = fs::read_to_string(e.base.join("calls")).unwrap();
    assert!(calls.contains("https://github.com/rabalyn/shale/releases/download/v0.6.0/shale-0.6.0-linux-x86_64.tar.gz.sha256"), "{calls}");
}

// spec: BUILD-026
#[test]
fn build_026_update_script_fails_clearly_without_the_release_or_its_checksum() {
    let e = Env::new("missing");
    e.fake_gh(None);
    e.fake_curl(None);
    e.fake_makepkg();
    let before = e.pkgbuild();
    let out = e.run(&["9.9.9"]);
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.contains("release v9.9.9") && err.contains("shale-9.9.9-linux-x86_64.tar.gz.sha256") && err.contains("not found"), "{err}");
    assert_eq!(e.pkgbuild(), before, "nothing is written");
    assert!(!e.base.join("repo/packaging/aur/shale-bin/.SRCINFO").exists());
}

// spec: BUILD-026
#[test]
fn build_026_update_script_rejects_a_checksum_file_without_a_checksum_and_bad_versions() {
    let e = Env::new("badsum");
    e.fake_gh(Some("not-a-checksum"));
    e.fake_makepkg();
    let before = e.pkgbuild();
    let out = e.run(&["0.6.0"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("does not start with a sha256 checksum"));
    assert_eq!(e.pkgbuild(), before);
    for bad in ["0.6", "v0.6.0", "0.6.0-rc.1"] {
        let out = e.run(&[bad]);
        assert_eq!(out.status.code(), Some(2), "{bad}");
    }
}

// spec: BUILD-026
#[test]
fn build_026_update_script_says_so_when_makepkg_is_missing() {
    let e = Env::new("nomakepkg");
    e.fake_gh(Some(SUM_B));
    let before = e.pkgbuild();
    let out = e.run(&["0.6.0"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("makepkg is needed"), "{}", text(&out.stderr));
    assert_eq!(e.pkgbuild(), before);
}
