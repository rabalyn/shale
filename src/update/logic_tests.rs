//! The updates without a window (UPD): versions, the release, the choice of the file, the checksum,
//! the install kinds and their plans, the decisions of each mode, and the whole way from the check
//! to the replaced program against a pretend GitHub.

use super::check::*;
use super::fake_github::{FakeGithub, Sign};
use super::install::*;
use super::kind::*;
use super::*;
use crate::pdf::tex::archive_tests::{tar_entry, tar_gz, z, zip};
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex;

pub(super) fn temp(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("shale-update-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---------------------------------------------------------------- versions and the release

// spec: UPD-004
#[test]
fn upd_004_versions_compare_the_way_semver_does() {
    for (current, latest, newer) in [
        ("0.6.6", "0.6.7", true),
        ("0.6.6", "v0.6.7", true),
        ("0.6.9", "0.6.10", true),
        ("0.6.6", "0.6.6", false),
        ("0.6.6", "0.6.5", false),
        ("0.9.0", "1.0.0", true),
        ("1.0.0-rc.1", "1.0.0", true),
        ("1.0.0", "1.0.0-rc.1", false),
        ("1.0.0-rc.1", "1.0.0-rc.2", true),
        ("1.0.0-rc.2", "1.0.0-rc.10", true),
        ("1.0.0-alpha", "1.0.0-alpha.1", true),
        ("1.0.0-1", "1.0.0-alpha", true),
        ("0.6.6+build5", "0.6.6", false),
    ] {
        assert_eq!(is_newer(current, latest), newer, "{latest} against {current}");
    }
    // a text that is no version is never newer, whichever side it is on
    for bad in ["", "latest", "1.2", "1.2.3.4", "1.x.3", "v", "1..3", "1.2.3-", "01.2.x", "1.2.3-a b"] {
        assert!(!is_newer("0.6.6", bad), "{bad:?}");
        assert!(!is_newer(bad, "9.9.9"), "{bad:?}");
    }
    assert!(Version::parse("1.0.0-rc.1").unwrap().is_prerelease() && !Version::parse("1.0.0").unwrap().is_prerelease());
}

fn json(tag: &str, draft: bool, pre: bool) -> String {
    serde_json::json!({"tag_name": tag, "draft": draft, "prerelease": pre, "body": "What changed", "assets": [{"name": "a.zip", "browser_download_url": "https://github.com/x", "size": 5}]}).to_string()
}

// spec: UPD-003
#[test]
fn upd_003_the_release_is_read_and_drafts_prereleases_and_odd_tags_are_ignored() {
    let r = parse_release(&json("v0.7.0", false, false)).unwrap().unwrap();
    assert_eq!((r.version.as_str(), r.notes.as_str()), ("0.7.0", "What changed"));
    assert_eq!(r.assets, [Asset { name: "a.zip".into(), url: "https://github.com/x".into(), size: 5 }]);
    assert_eq!(parse_release(&json("v0.7.0", true, false)).unwrap(), None, "a draft");
    assert_eq!(parse_release(&json("v0.7.0", false, true)).unwrap(), None, "a pre-release");
    assert_eq!(parse_release(&json("v0.7.0-rc.1", false, false)).unwrap(), None, "a pre-release tag");
    assert_eq!(parse_release(&json("nightly", false, false)).unwrap(), None, "a tag that is no version");
    assert!(parse_release("not json").unwrap_err().contains("not a release"));
    assert!(parse_release(r#"{"message": "Not Found"}"#).is_err());
    // notes without a body, and very long notes
    assert_eq!(parse_release(r#"{"tag_name": "v1.0.0"}"#).unwrap().unwrap().notes, "");
    let long = serde_json::json!({"tag_name": "v1.0.0", "body": "x".repeat(50_000)}).to_string();
    assert_eq!(parse_release(&long).unwrap().unwrap().notes.len(), 20_000);
}

// spec: UPD-003, UPD-021
#[test]
fn upd_003_only_the_release_files_of_the_repository_are_downloaded_and_only_over_https() {
    let github = Source::github();
    assert!(github.accepts("https://github.com/rabalyn/shale/releases/download/v0.7.0/shale-0.7.0-linux-x86_64.tar.gz"));
    for bad in [
        "http://github.com/rabalyn/shale/releases/download/v1/x",
        "https://evil.example/rabalyn/shale/releases/download/v1/x",
        "https://github.com/other/shale/releases/download/v1/x",
        "https://github.com/rabalyn/shale/releases/download/v1/x y",
        "file:///etc/passwd",
    ] {
        assert!(!github.accepts(bad), "{bad}");
    }
    assert_eq!(github.latest_url(), "https://api.github.com/repos/rabalyn/shale/releases/latest");
    let pretend = Source::pretend("http://127.0.0.1:99/");
    assert!(pretend.accepts("http://127.0.0.1:99/dl/x") && !pretend.accepts("http://example.org/x"));
    assert_eq!(pretend.latest_url(), "http://127.0.0.1:99/repos/rabalyn/shale/releases/latest");
}

/// A release of a version with the files of Linux on x86_64.
pub(super) fn release_for_tests(version: &str) -> Release {
    release(version, "linux", "x86_64")
}

fn release(version: &str, os: &str, arch: &str) -> Release {
    let name = asset_name(version, os, arch).unwrap();
    let asset = |n: String| Asset { url: format!("https://github.com/rabalyn/shale/releases/download/v{version}/{n}"), name: n, size: 10 };
    Release { version: version.into(), notes: format!("Notes of {version}"), assets: vec![asset(format!("{name}.sig")), asset(format!("{name}.sha256")), asset(name)] }
}

// spec: UPD-005
#[test]
fn upd_005_the_file_is_chosen_by_system_and_processor() {
    assert_eq!(asset_name("0.7.0", "linux", "x86_64").unwrap(), "shale-0.7.0-linux-x86_64.tar.gz");
    assert_eq!(asset_name("0.7.0", "macos", "aarch64").unwrap(), "shale-0.7.0-macos-arm64.dmg");
    assert_eq!(asset_name("0.7.0", "windows", "x86_64").unwrap(), "shale-0.7.0-windows-x86_64.zip");
    for (os, arch) in [("linux", "aarch64"), ("macos", "x86_64"), ("freebsd", "x86_64"), ("windows", "aarch64")] {
        assert_eq!(asset_name("0.7.0", os, arch), None, "{os} {arch}");
    }
    for (os, arch) in [("linux", "x86_64"), ("macos", "aarch64"), ("windows", "x86_64")] {
        let r = release("0.7.0", os, arch);
        let (file, sum, sig) = asset_for(&r, os, arch).unwrap();
        assert_eq!(format!("{}.sha256", file.name), sum.name);
        assert_eq!(format!("{}.sig", file.name), sig.name);
    }
    // nothing for this system, no file in the release, no checksum file: each says so
    let r = release("0.7.0", "linux", "x86_64");
    assert!(asset_for(&r, "linux", "aarch64").unwrap_err().contains("no download of Shale 0.7.0 for linux on aarch64"));
    assert!(asset_for(&r, "windows", "x86_64").unwrap_err().contains("has no file shale-0.7.0-windows-x86_64.zip"));
    let mut no_sum = r.clone();
    no_sum.assets.retain(|a| !a.name.ends_with(".sha256"));
    assert!(asset_for(&no_sum, "linux", "x86_64").unwrap_err().contains("no checksum file"));
}

// spec: UPD-023
#[test]
fn upd_022_a_release_without_a_signature_file_is_not_downloaded_and_the_reason_says_what_to_do() {
    let mut r = release("0.7.0", "linux", "x86_64");
    r.assets.retain(|a| !a.name.ends_with(".sig"));
    let e = asset_for(&r, "linux", "x86_64").unwrap_err();
    assert!(e.contains("no signature file for shale-0.7.0-linux-x86_64.tar.gz") && e.contains("does not download or install it by itself"), "{e}");
    assert!(e.contains("https://github.com/rabalyn/shale/releases/tag/v0.7.0"), "{e}");
    assert_eq!(releases_page(), "https://github.com/rabalyn/shale/releases");
}

// spec: UPD-023
#[test]
fn upd_022_the_notice_of_an_unsigned_release_has_no_download_or_install_button_but_the_release_page() {
    let (backend, counts) = fake(Some("9.9.9"));
    counts.latest.lock().unwrap().as_mut().unwrap().assets.retain(|a| !a.name.ends_with(".sig"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert_eq!(Counts::n(&counts.fetches), 0, "an unsigned release is not even downloaded in Install mode");
    let n = st.notice(&c).unwrap();
    assert!(n.text.contains("Shale 9.9.9 is available") && n.text.contains("no signature file") && n.text.contains("releases/tag/v9.9.9"), "{}", n.text);
    assert!(!n.buttons.iter().any(|(l, _)| *l == "Download" || *l == "Install"), "{:?}", n.buttons);
    assert!(n.buttons.iter().any(|(l, _)| *l == "What's new"));
}

// spec: UPD-006
#[test]
fn upd_006_the_checksum_file_is_read_in_the_formats_of_sha256sum_and_shasum() {
    let hash = "ab".repeat(32);
    assert_eq!(parse_checksum(&format!("{hash}  shale.zip\n"), "shale.zip").unwrap(), hash);
    assert_eq!(parse_checksum(&format!("{}  *shale.zip", hash.to_uppercase()), "shale.zip").unwrap(), hash, "binary mode mark, any case");
    assert_eq!(parse_checksum(&format!("\n{hash}\n"), "shale.zip").unwrap(), hash, "a bare hash");
    assert!(parse_checksum(&format!("{hash}  other.zip"), "shale.zip").unwrap_err().contains("is for other.zip, not for shale.zip"));
    assert!(parse_checksum("abc  shale.zip", "shale.zip").unwrap_err().contains("SHA-256"));
    assert!(parse_checksum(&format!("{}zz  shale.zip", "a".repeat(62)), "shale.zip").is_err());
    assert!(parse_checksum("  \n", "shale.zip").unwrap_err().contains("empty"));
}

// spec: UPD-006
#[test]
fn upd_006_a_download_that_does_not_match_is_deleted_and_says_so() {
    let d = temp("verify");
    let (file, sum) = (d.join("shale.zip"), d.join("shale.zip.sha256"));
    std::fs::write(&file, b"the program").unwrap();
    std::fs::write(&sum, format!("{}  shale.zip\n", crate::pdf::tex::archive::sha256_hex(b"the program"))).unwrap();
    verify(&file, &sum, "shale.zip").unwrap();
    assert!(file.exists() && sum.exists(), "a good download stays");
    std::fs::write(&file, b"the program, damaged").unwrap();
    let e = verify(&file, &sum, "shale.zip").unwrap_err();
    assert!(e.contains("shale.zip is damaged") && e.contains("nothing was installed"), "{e}");
    assert!(!file.exists() && !sum.exists(), "the damaged download and its checksum are deleted");
    // an unreadable checksum, a checksum of another file and a missing download are errors too
    std::fs::write(&file, b"x").unwrap();
    std::fs::write(&sum, "garbage").unwrap();
    assert!(verify(&file, &sum, "shale.zip").is_err() && !file.exists());
    assert!(verify(&file, &sum, "shale.zip").unwrap_err().contains("Nothing was installed"));
}

// ---------------------------------------------------------------- install kinds

fn facts(os: Os) -> Facts {
    Facts { os, home: Some(PathBuf::from("/home/ada")), install_sh_next_to_it: false, desktop_file_in_prefix: false }
}

#[cfg(unix)]
#[test]
fn upd_011_the_install_kind_is_found_from_the_path_of_the_program() {
    let linux = |path: &str, f: Facts| detect(Path::new(path), &f);
    let with = |sh: bool, desktop: bool| Facts { install_sh_next_to_it: sh, desktop_file_in_prefix: desktop, ..facts(Os::Linux) };
    // spec: UPD-011
    assert_eq!(linux("/home/ada/.local/bin/shale", with(false, true)), InstallKind::LinuxInstalled { prefix: "/home/ada/.local".into(), user_default: true });
    assert_eq!(linux("/home/ada/.local/bin/shale", with(false, false)), InstallKind::LinuxInstalled { prefix: "/home/ada/.local".into(), user_default: true }, "the default prefix needs no desktop file");
    assert_eq!(linux("/usr/local/bin/shale", with(false, true)), InstallKind::LinuxInstalled { prefix: "/usr/local".into(), user_default: false });
    assert_eq!(linux("/opt/shale/bin/shale", with(false, true)), InstallKind::LinuxInstalled { prefix: "/opt/shale".into(), user_default: false });
    // spec: UPD-014
    assert_eq!(linux("/home/ada/Downloads/shale/shale", with(true, false)), InstallKind::LinuxPortable { dir: "/home/ada/Downloads/shale".into(), whole: true });
    assert_eq!(linux("/home/ada/bin/shale", with(false, false)), InstallKind::LinuxPortable { dir: "/home/ada/bin".into(), whole: false }, "a bare program");
    assert_eq!(linux("/usr/local/bin/shale", with(false, false)), InstallKind::LinuxPortable { dir: "/usr/local/bin".into(), whole: false }, "a program copied there by hand");
    // spec: UPD-012
    for p in ["/usr/bin/shale", "/usr/lib/shale/shale", "/bin/shale", "/nix/store/abc-shale/bin/shale", "/snap/shale/1/bin/shale", "/var/lib/flatpak/app/x/shale"] {
        assert_eq!(linux(p, with(false, true)), InstallKind::PackageManaged, "{p}");
    }
    // spec: UPD-013
    for p in ["/work/shale/target/debug/shale", "/work/shale/target/release/shale", "/work/shale/target/bench-release/shale", "/work/shale/target/x86_64-unknown-linux-gnu/release/shale"] {
        assert_eq!(linux(p, with(false, true)), InstallKind::Development, "{p}");
    }
    // spec: UPD-016
    assert_eq!(detect(Path::new("/Applications/Shale.app/Contents/MacOS/shale"), &facts(Os::Mac)), InstallKind::MacApp { bundle: "/Applications/Shale.app".into() });
    assert_eq!(detect(Path::new("/Users/ada/Apps/Shale.app/Contents/MacOS/shale"), &facts(Os::Mac)), InstallKind::MacApp { bundle: "/Users/ada/Apps/Shale.app".into() });
    assert_eq!(detect(Path::new("/opt/homebrew/bin/shale"), &facts(Os::Mac)), InstallKind::Development, "a bare program on macOS is never replaced");
    assert_eq!(detect(Path::new("/Applications/Other.app/Contents/Resources/shale"), &facts(Os::Mac)), InstallKind::Development);
    // spec: UPD-017
    assert_eq!(detect(Path::new("C:/Apps/shale/shale.exe"), &facts(Os::Windows)), InstallKind::Windows { dir: "C:/Apps/shale".into() });
    assert_eq!(detect(Path::new("C:/work/shale/target/release/shale.exe"), &facts(Os::Windows)), InstallKind::Development);
}

// spec: UPD-011
#[test]
fn upd_011_the_facts_are_read_from_the_disk() {
    let d = temp("facts");
    std::fs::create_dir_all(d.join("bin")).unwrap();
    std::fs::create_dir_all(d.join("share/applications")).unwrap();
    std::fs::write(d.join("bin/shale"), b"x").unwrap();
    let f = Facts::gather(&d.join("bin/shale"));
    assert!(!f.install_sh_next_to_it && !f.desktop_file_in_prefix);
    std::fs::write(d.join("share/applications/shale.desktop"), b"x").unwrap();
    std::fs::write(d.join("bin/install.sh"), b"x").unwrap();
    let f = Facts::gather(&d.join("bin/shale"));
    assert!(f.install_sh_next_to_it && f.desktop_file_in_prefix);
    assert_eq!(f.os, Os::current());
    assert!(is_writable(&d) && !is_writable(&d.join("missing")));
}

// spec: UPD-012
#[test]
fn upd_012_what_an_update_may_do_follows_the_install_kind() {
    let portable = InstallKind::LinuxPortable { dir: "/a".into(), whole: true };
    assert_eq!(capability(&portable, true), Capability::Install);
    match capability(&portable, false) {
        Capability::DownloadOnly(why) => assert!(why.contains("/a") && why.contains("cannot be written"), "{why}"),
        c => panic!("{c:?}"),
    }
    match capability(&InstallKind::PackageManaged, true) {
        Capability::NotifyOnly(why) => assert!(why.contains("yay -Syu shale-bin") && why.contains("package manager"), "{why}"),
        c => panic!("{c:?}"),
    }
    assert!(matches!(capability(&InstallKind::Development, true), Capability::NotifyOnly(_)));
    assert!(matches!(capability(&InstallKind::PackageManaged, false), Capability::NotifyOnly(_)), "being unwritable does not matter for what is never touched");
}

// spec: UPD-012
#[test]
fn upd_012_the_mode_a_found_update_gets_follows_what_the_install_allows() {
    use UpdateMode::*;
    let (install, download, notify) = (Capability::Install, Capability::DownloadOnly("no".into()), Capability::NotifyOnly("no".into()));
    for (mode, cap, file, want) in [
        (Install, &install, true, Install),
        (Download, &install, true, Download),
        (Notify, &install, true, Notify),
        (Install, &download, true, Download),
        (Download, &download, true, Download),
        (Install, &notify, true, Notify),
        (Download, &notify, true, Notify),
        (Install, &install, false, Notify),
        (Download, &install, false, Notify),
        (Off, &install, true, Off),
    ] {
        assert_eq!(act_as(mode, cap, file), want, "{mode:?} {cap:?} file={file}");
    }
}

// spec: UPD-020, UPD-015
#[test]
fn upd_020_every_kind_is_described_and_a_manual_command_is_given_where_one_helps() {
    let archive = Path::new("/cache/shale/updates/0.7.0/shale-0.7.0-linux-x86_64.tar.gz");
    let staging = Path::new("/cache/shale/updates/0.7.0");
    let installed = InstallKind::LinuxInstalled { prefix: "/usr/local".into(), user_default: false };
    let cmd = manual_command(&installed, archive, staging);
    assert!(
        cmd.starts_with("tar -xzf \"/cache/shale/updates/0.7.0/shale-0.7.0-linux-x86_64.tar.gz\" -C \"/cache/shale/updates/0.7.0\" && sudo sh \"/cache/shale/updates/0.7.0/shale/install.sh\"") && cmd.ends_with("--prefix \"/usr/local\""),
        "{cmd}"
    );
    let user = manual_command(&InstallKind::LinuxInstalled { prefix: "/home/a/.local".into(), user_default: true }, archive, staging);
    assert!(!user.contains("--prefix"), "{user}");
    assert!(!user.contains("sudo") && user.contains("&& sh \""), "a user install needs no root: {user}");
    assert!(cmd.contains("&& sudo sh "), "{cmd}");
    assert!(manual_command(&InstallKind::LinuxPortable { dir: "/x".into(), whole: true }, archive, staging).contains("--strip-components=1"));
    assert!(manual_command(&InstallKind::MacApp { bundle: "/Applications/Shale.app".into() }, archive, staging).starts_with("open "));
    assert!(manual_command(&InstallKind::Windows { dir: "C:/s".into() }, archive, staging).contains("unpack"));
    assert_eq!(manual_command(&InstallKind::PackageManaged, archive, staging), "");
    for k in [
        installed,
        InstallKind::LinuxPortable { dir: "/x".into(), whole: true },
        InstallKind::LinuxPortable { dir: "/x".into(), whole: false },
        InstallKind::PackageManaged,
        InstallKind::MacApp { bundle: "/A/Shale.app".into() },
        InstallKind::Windows { dir: "C:/s".into() },
        InstallKind::Development,
    ] {
        assert!(!describe(&k).is_empty());
    }
    assert_eq!(install_folder(&InstallKind::MacApp { bundle: "/Applications/Shale.app".into() }), PathBuf::from("/Applications"));
    assert_eq!(install_folder(&InstallKind::LinuxInstalled { prefix: "/usr/local".into(), user_default: false }), PathBuf::from("/usr/local/bin"));
    assert_eq!(install_folder(&InstallKind::PackageManaged), PathBuf::new());
}

// ---------------------------------------------------------------- plans

fn entries() -> Vec<(String, bool)> {
    vec![("README.md".into(), false), ("assets".into(), true), ("install.sh".into(), false), ("shale".into(), false), ("shale.desktop".into(), false)]
}

// spec: UPD-014
#[test]
fn upd_014_a_portable_linux_folder_gets_a_backup_and_every_file_replaced_in_place() {
    let kind = InstallKind::LinuxPortable { dir: "/p".into(), whole: true };
    let plan = plan(&kind, Path::new("/stage/shale"), &entries(), Path::new("/stage")).unwrap();
    assert_eq!(plan.ops[0], Op::CopyFile { from: "/p/shale".into(), to: "/p/shale.old".into() }, "the old program is kept first");
    assert_eq!(plan.ops[1], Op::Install { from: "/stage/shale/shale".into(), to: "/p/shale".into(), exec: true, aside: None }, "the program is renamed over the running one");
    assert!(plan.ops.contains(&Op::ReplaceDir { from: "/stage/shale/assets".into(), to: "/p/assets".into() }));
    assert!(plan.ops.contains(&Op::Install { from: "/stage/shale/install.sh".into(), to: "/p/install.sh".into(), exec: true, aside: None }));
    assert!(plan.ops.contains(&Op::Install { from: "/stage/shale/README.md".into(), to: "/p/README.md".into(), exec: false, aside: None }));
    assert_eq!(plan.ops.len(), 6);
    // a bare program: only the program
    let bare = super::install::plan(&InstallKind::LinuxPortable { dir: "/p".into(), whole: false }, Path::new("/stage/shale"), &entries(), Path::new("/stage")).unwrap();
    assert_eq!(bare.ops.len(), 2);
}

// spec: UPD-015
#[test]
fn upd_015_an_install_sh_install_runs_the_new_install_sh_with_its_prefix() {
    let with_prefix = plan(&InstallKind::LinuxInstalled { prefix: "/usr/local".into(), user_default: false }, Path::new("/stage/shale"), &entries(), Path::new("/stage")).unwrap();
    assert_eq!(with_prefix.ops[0], Op::CopyFile { from: "/usr/local/bin/shale".into(), to: "/usr/local/bin/shale.old".into() });
    assert_eq!(with_prefix.ops[1], Op::Run { program: "sh".into(), args: vec!["/stage/shale/install.sh".into(), "--prefix".into(), "/usr/local".into()] });
    let user = plan(&InstallKind::LinuxInstalled { prefix: "/home/a/.local".into(), user_default: true }, Path::new("/stage/shale"), &entries(), Path::new("/stage")).unwrap();
    assert_eq!(user.ops[1], Op::Run { program: "sh".into(), args: vec!["/stage/shale/install.sh".into()] }, "the user's default needs no --prefix");
}

// spec: UPD-016
#[test]
fn upd_016_a_mac_app_is_copied_next_to_the_old_one_and_swapped() {
    let kind = InstallKind::MacApp { bundle: "/Applications/Shale.app".into() };
    let p = plan(&kind, Path::new("/stage/shale.dmg"), &[], Path::new("/stage/mount")).unwrap();
    let run = |program: &str, args: &[&str]| Op::Run { program: program.into(), args: args.iter().map(|a| a.into()).collect() };
    assert_eq!(
        p.ops,
        [
            Op::Remove("/Applications/Shale.app.new".into()),
            Op::Remove("/Applications/Shale.app.old".into()),
            run("hdiutil", &["attach", "-readonly", "-nobrowse", "-mountpoint", "/stage/mount", "/stage/shale.dmg"]),
            run("ditto", &["/stage/mount/Shale.app", "/Applications/Shale.app.new"]),
            run("hdiutil", &["detach", "/stage/mount"]),
            Op::Swap { new: "/Applications/Shale.app.new".into(), current: "/Applications/Shale.app".into(), old: "/Applications/Shale.app.old".into(), keep_old: false },
        ]
    );
    assert_eq!(p.cleanup, [run("hdiutil", &["detach", "/stage/mount"])], "the image is detached whatever happens");
    let r = relaunch_for(&kind).unwrap();
    assert_eq!((r.program, r.args), (PathBuf::from("open"), vec!["-n".into(), "/Applications/Shale.app".into(), "--args".into(), RESTARTED.into()]));
}

// spec: UPD-017
#[test]
fn upd_017_on_windows_the_running_program_is_renamed_away_and_the_rest_replaced() {
    let kind = InstallKind::Windows { dir: "C:/Shale".into() };
    let files = vec![("README.md".into(), false), ("docs".into(), true), ("shale.exe".into(), false)];
    let p = plan(&kind, Path::new("C:/stage/shale"), &files, Path::new("C:/stage")).unwrap();
    assert_eq!(p.ops[0], Op::Install { from: "C:/stage/shale/shale.exe".into(), to: "C:/Shale/shale.exe".into(), exec: false, aside: Some("C:/Shale/shale.exe.old".into()) });
    assert_eq!(p.ops.len(), 3);
    let r = relaunch_for(&kind).unwrap();
    assert_eq!((r.program, r.args), (PathBuf::from("C:/Shale/shale.exe"), vec![RESTARTED.into()]));
}

// spec: UPD-012
#[test]
fn upd_012_a_package_managed_install_and_a_development_build_have_no_plan_and_no_restart() {
    for k in [InstallKind::PackageManaged, InstallKind::Development] {
        assert!(plan(&k, Path::new("/s"), &[], Path::new("/m")).unwrap_err().contains("never replaced"));
        assert_eq!(relaunch_for(&k), None);
        assert!(apply(&k, &Staged { version: "1".into(), dir: "/s".into(), archive: "/s/a".into() }, &AtomicBool::new(false)).is_err());
    }
}

// spec: UPD-018
#[test]
fn upd_018_the_backup_of_the_old_version_goes_after_the_new_one_ran() {
    let exe = |p: &str| PathBuf::from(p);
    assert_eq!(leftovers(&InstallKind::LinuxPortable { dir: "/p".into(), whole: true }, &exe("/p/shale")), [PathBuf::from("/p/shale.old")]);
    assert_eq!(leftovers(&InstallKind::LinuxInstalled { prefix: "/u".into(), user_default: false }, &exe("/u/bin/shale")), [PathBuf::from("/u/bin/shale.old")]);
    assert_eq!(leftovers(&InstallKind::Windows { dir: "C:/S".into() }, &exe("C:/S/shale.exe")), [PathBuf::from("C:/S/shale.exe.old")]);
    // the old program run as itself (a rollback) removes nothing; neither does anything that is never replaced
    assert!(leftovers(&InstallKind::LinuxPortable { dir: "/p".into(), whole: true }, &exe("/p/shale.old")).is_empty());
    assert!(leftovers(&InstallKind::PackageManaged, &exe("/usr/bin/shale")).is_empty());
    assert!(leftovers(&InstallKind::MacApp { bundle: "/A/Shale.app".into() }, &exe("/A/Shale.app/Contents/MacOS/shale")).is_empty());
}

// spec: UPD-018
#[test]
fn upd_018_leftovers_are_removed_after_a_moment_and_a_windows_one_at_the_start() {
    let d = temp("leftovers");
    std::fs::write(d.join("shale.old"), b"old").unwrap();
    std::fs::write(d.join("shale.exe.old"), b"old").unwrap();
    let ctx = egui::Context::default();
    let mut cfg = UpdateConfig::default();
    // Linux: not at the start
    let mut st = UpdateState::with(InstallKind::LinuxPortable { dir: d.clone(), whole: true }, d.join("shale"), fake(None).0, true);
    st.env_off = true;
    st.tick(&ctx, &mut cfg);
    assert!(d.join("shale.old").exists(), "the new version has not run for long yet");
    st.started = Instant::now() - BACKUP_KEPT - Duration::from_secs(1);
    st.tick(&ctx, &mut cfg);
    assert!(!d.join("shale.old").exists(), "removed once the new version has run");
    // Windows: at the very start
    let mut st = UpdateState::with(InstallKind::Windows { dir: d.clone() }, d.join("shale.exe"), fake(None).0, true);
    st.env_off = true;
    st.tick(&ctx, &mut cfg);
    assert!(!d.join("shale.exe.old").exists());
}

// ---------------------------------------------------------------- running plans

#[cfg(unix)]
#[test]
fn upd_014_files_are_put_in_place_over_a_running_program_and_a_failed_step_leaves_the_old_file() {
    let d = temp("ops");
    let cancel = AtomicBool::new(false);
    std::fs::write(d.join("new"), b"new").unwrap();
    std::fs::write(d.join("shale"), b"old").unwrap();
    execute(&Plan { ops: vec![Op::CopyFile { from: d.join("shale"), to: d.join("shale.old") }, Op::Install { from: d.join("new"), to: d.join("shale"), exec: true, aside: None }], cleanup: vec![] }, &cancel).unwrap();
    assert_eq!(std::fs::read(d.join("shale")).unwrap(), b"new");
    assert_eq!(std::fs::read(d.join("shale.old")).unwrap(), b"old");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(d.join("shale")).unwrap().permissions().mode() & 0o777, 0o755);
    assert!(!d.join("shale.new").exists() && !d.join("shale.old.tmp").exists(), "no half-written files stay");
    // a backup of a program that is not there is skipped
    execute(&Plan { ops: vec![Op::CopyFile { from: d.join("none"), to: d.join("none.old") }], cleanup: vec![] }, &cancel).unwrap();
    assert!(!d.join("none.old").exists());
    // a missing source: an error that names the file, the old one stays
    let e = execute(&Plan { ops: vec![Op::Install { from: d.join("missing"), to: d.join("shale"), exec: true, aside: None }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("missing"), "{e}");
    assert_eq!(std::fs::read(d.join("shale")).unwrap(), b"new");
}

#[cfg(unix)]
#[test]
fn upd_017_the_running_file_is_moved_aside_and_comes_back_when_the_new_one_cannot_be_put_in_place() {
    let d = temp("aside");
    let cancel = AtomicBool::new(false);
    std::fs::write(d.join("shale.exe"), b"old").unwrap();
    std::fs::write(d.join("new"), b"new").unwrap();
    let op = |to: &str| Op::Install { from: d.join("new"), to: d.join(to), exec: false, aside: Some(d.join("shale.exe.old")) };
    execute(&Plan { ops: vec![op("shale.exe")], cleanup: vec![] }, &cancel).unwrap();
    assert_eq!((std::fs::read(d.join("shale.exe")).unwrap(), std::fs::read(d.join("shale.exe.old")).unwrap()), (b"new".to_vec(), b"old".to_vec()));
    // the old program cannot be moved aside: an error, the program stays, no temporary file stays
    std::fs::write(d.join("new"), b"newer").unwrap();
    let e = execute(&Plan { ops: vec![Op::Install { from: d.join("new"), to: d.join("shale.exe"), exec: false, aside: Some(d.join("no-such-folder/shale.exe.old")) }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not move the running program aside"), "{e}");
    assert_eq!(std::fs::read(d.join("shale.exe")).unwrap(), b"new");
    assert!(!d.join("shale.exe.new").exists());
    // the new file cannot be put in place (a folder is in the way): the old entry is back as it was
    std::fs::create_dir(d.join("blocked")).unwrap();
    std::fs::write(d.join("blocked/inner"), b"x").unwrap();
    let e = execute(&Plan { ops: vec![Op::Install { from: d.join("new"), to: d.join("blocked"), exec: false, aside: Some(d.join("blocked")) }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not put the new file in place"), "{e}");
    assert!(d.join("blocked/inner").exists() && !d.join("blocked.new").exists(), "nothing was lost, no temporary file stays");
}

#[cfg(unix)]
#[test]
fn upd_016_a_bundle_is_swapped_and_a_failed_swap_puts_the_old_one_back() {
    let d = temp("swap");
    let cancel = AtomicBool::new(false);
    for (name, text) in [("Shale.app", "old"), ("Shale.app.new", "new")] {
        std::fs::create_dir_all(d.join(name).join("Contents")).unwrap();
        std::fs::write(d.join(name).join("Contents/v"), text).unwrap();
    }
    let swap = |new: &str| Plan { ops: vec![Op::Swap { new: d.join(new), current: d.join("Shale.app"), old: d.join("Shale.app.old"), keep_old: false }], cleanup: vec![] };
    // the new bundle is missing: the second rename fails and the old bundle comes back
    assert!(execute(&swap("nothing"), &cancel).is_err());
    assert_eq!(std::fs::read_to_string(d.join("Shale.app/Contents/v")).unwrap(), "old");
    assert!(!d.join("Shale.app.old").exists());
    execute(&swap("Shale.app.new"), &cancel).unwrap();
    assert_eq!(std::fs::read_to_string(d.join("Shale.app/Contents/v")).unwrap(), "new");
    assert!(!d.join("Shale.app.old").exists() && !d.join("Shale.app.new").exists(), "the old one is removed");
    // keep_old keeps it
    std::fs::create_dir_all(d.join("N")).unwrap();
    execute(&Plan { ops: vec![Op::Swap { new: d.join("N"), current: d.join("Shale.app"), old: d.join("Shale.app.old"), keep_old: true }], cleanup: vec![] }, &cancel).unwrap();
    assert!(d.join("Shale.app.old/Contents/v").exists());
}

#[cfg(unix)]
#[test]
fn upd_014_a_folder_replaces_the_old_one_and_a_program_that_fails_ends_the_plan_but_not_the_cleanup() {
    let d = temp("dirs");
    let cancel = AtomicBool::new(false);
    std::fs::create_dir_all(d.join("from/sub")).unwrap();
    std::fs::write(d.join("from/sub/a"), b"new").unwrap();
    std::fs::create_dir_all(d.join("to")).unwrap();
    std::fs::write(d.join("to/stale"), b"old").unwrap();
    execute(&Plan { ops: vec![Op::ReplaceDir { from: d.join("from"), to: d.join("to") }], cleanup: vec![] }, &cancel).unwrap();
    assert!(d.join("to/sub/a").exists() && !d.join("to/stale").exists() && !d.join("to.new").exists());
    let plan = Plan { ops: vec![Op::Run { program: "sh".into(), args: vec!["-c".into(), "echo it broke >&2; exit 3".into()] }, Op::Remove(d.join("to"))], cleanup: vec![Op::Remove(d.join("from"))] };
    let e = execute(&plan, &cancel).unwrap_err();
    assert!(e.contains("sh failed") && e.contains("it broke"), "{e}");
    assert!(d.join("to").exists(), "the step after the failure did not run");
    assert!(!d.join("from").exists(), "the cleanup ran");
    execute(&Plan { ops: vec![Op::Remove(d.join("nothing"))], cleanup: vec![] }, &cancel).unwrap();
    let e = execute(&Plan { ops: vec![Op::Run { program: "/nonexistent/program".into(), args: vec![] }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not run"), "{e}");
}

#[cfg(unix)]
#[test]
fn upd_014_a_new_file_that_cannot_be_put_in_place_leaves_the_old_entry_and_no_temporary_file() {
    let d = temp("install-fails");
    let cancel = AtomicBool::new(false);
    std::fs::write(d.join("new"), b"new").unwrap();
    // without a file moved aside: a folder with content is where the file should go
    std::fs::create_dir(d.join("shale")).unwrap();
    std::fs::write(d.join("shale/inner"), b"old").unwrap();
    let e = execute(&Plan { ops: vec![Op::Install { from: d.join("new"), to: d.join("shale"), exec: true, aside: None }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not put the new file in place"), "{e}");
    assert_eq!(std::fs::read(d.join("shale/inner")).unwrap(), b"old");
    assert!(!d.join("shale.new").exists(), "the temporary file is removed");
    // the place to move the program aside to is a folder with content: the program stays, no temporary file
    std::fs::write(d.join("prog"), b"old").unwrap();
    std::fs::create_dir(d.join("aside")).unwrap();
    std::fs::write(d.join("aside/x"), b"x").unwrap();
    let e = execute(&Plan { ops: vec![Op::Install { from: d.join("new"), to: d.join("prog"), exec: false, aside: Some(d.join("aside")) }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not move the running program aside"), "{e}");
    assert_eq!(std::fs::read(d.join("prog")).unwrap(), b"old");
    assert!(!d.join("prog.new").exists());
}

#[cfg(unix)]
#[test]
fn upd_014_a_folder_that_cannot_be_copied_leaves_the_old_folder_and_a_folder_in_a_locked_place_fails_in_words() {
    use std::os::unix::fs::PermissionsExt;
    let d = temp("replace-dir-fails");
    let cancel = AtomicBool::new(false);
    std::fs::create_dir_all(d.join("to")).unwrap();
    std::fs::write(d.join("to/old"), b"old").unwrap();
    // the source is missing: an error naming it, the old folder is untouched
    let e = execute(&Plan { ops: vec![Op::ReplaceDir { from: d.join("no-such-from"), to: d.join("to") }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("no-such-from"), "{e}");
    assert_eq!(std::fs::read(d.join("to/old")).unwrap(), b"old");
    // the folder that holds `to` cannot be written
    std::fs::create_dir_all(d.join("from")).unwrap();
    std::fs::write(d.join("from/new"), b"new").unwrap();
    std::fs::create_dir_all(d.join("locked/to")).unwrap();
    std::fs::write(d.join("locked/to/old"), b"old").unwrap();
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o555)).unwrap();
    let can_write = std::fs::write(d.join("locked/probe"), b"x").is_ok();
    let result = execute(&Plan { ops: vec![Op::ReplaceDir { from: d.join("from"), to: d.join("locked/to") }], cleanup: vec![] }, &cancel);
    std::fs::set_permissions(d.join("locked"), std::fs::Permissions::from_mode(0o755)).unwrap();
    if can_write {
        return; // running as root: permissions do not stop us
    }
    let e = result.unwrap_err();
    assert!(e.contains("could not make"), "{e}");
    assert_eq!(std::fs::read(d.join("locked/to/old")).unwrap(), b"old", "the old folder is still there");
}

#[cfg(unix)]
#[test]
fn upd_016_a_swap_without_a_current_version_fails_in_words_and_changes_nothing() {
    let d = temp("swap-first");
    let cancel = AtomicBool::new(false);
    std::fs::create_dir_all(d.join("Shale.app.new")).unwrap();
    std::fs::write(d.join("Shale.app.new/v"), b"new").unwrap();
    let e = execute(&Plan { ops: vec![Op::Swap { new: d.join("Shale.app.new"), current: d.join("Shale.app"), old: d.join("Shale.app.old"), keep_old: false }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not move the old version aside"), "{e}");
    assert_eq!(std::fs::read(d.join("Shale.app.new/v")).unwrap(), b"new");
    assert!(!d.join("Shale.app").exists() && !d.join("Shale.app.old").exists());
}

#[cfg(unix)]
#[test]
fn upd_016_a_failed_second_rename_of_a_swap_puts_the_old_version_back_even_when_keeping_it() {
    let d = temp("swap-second");
    let cancel = AtomicBool::new(false);
    std::fs::create_dir_all(d.join("Shale.app")).unwrap();
    std::fs::write(d.join("Shale.app/v"), b"old").unwrap();
    // the new version is missing: the second rename fails, with keep_old the backup must not stay either
    let e = execute(&Plan { ops: vec![Op::Swap { new: d.join("gone"), current: d.join("Shale.app"), old: d.join("Shale.app.old"), keep_old: true }], cleanup: vec![] }, &cancel).unwrap_err();
    assert!(e.contains("could not put the new version in place"), "{e}");
    assert_eq!(std::fs::read(d.join("Shale.app/v")).unwrap(), b"old", "the old version is back, intact");
    assert!(!d.join("Shale.app.old").exists(), "nothing is left aside");
}

// ---------------------------------------------------------------- the whole way, against a pretend GitHub

const OLD_PROGRAM: &[u8] = b"#!/bin/sh\necho old\n";
const NEW_PROGRAM: &[u8] = b"#!/bin/sh\necho new\n";

/// The Linux download of a version: the program, the real `install.sh` and some files.
fn tarball(program: &[u8]) -> Vec<u8> {
    tar_gz(&[
        tar_entry("shale/", b'5', b""),
        tar_entry("shale/shale", b'0', program),
        tar_entry("shale/install.sh", b'0', include_bytes!("../../scripts/install.sh")),
        tar_entry("shale/shale.desktop", b'0', b"[Desktop Entry]\nName=Shale\nExec=shale\n"),
        tar_entry("shale/shale.svg", b'0', b"<svg/>"),
        tar_entry("shale/README.md", b'0', b"readme of the new version"),
    ])
}

fn pretend_backend(github: &FakeGithub, staging: &Path) -> Backend {
    Backend::real_for(Source::pretend(&github.base()), staging.to_path_buf(), "linux", "x86_64")
}

/// A portable folder: the old program, `install.sh` next to it and an old README.
fn portable(tag: &str) -> (PathBuf, InstallKind) {
    let dir = temp(tag);
    std::fs::write(dir.join("shale"), OLD_PROGRAM).unwrap();
    std::fs::write(dir.join("install.sh"), b"old install.sh").unwrap();
    std::fs::write(dir.join("README.md"), b"readme of the old version").unwrap();
    (dir.clone(), InstallKind::LinuxPortable { dir, whole: true })
}

fn get_latest(github: &FakeGithub, staging: &Path) -> (Backend, Release) {
    let backend = pretend_backend(github, staging);
    let release = (backend.check)(&AtomicBool::new(false)).unwrap().expect("a release");
    (backend, release)
}

// spec: UPD-014
#[cfg(unix)]
#[test]
fn upd_003_the_check_downloads_and_installs_a_portable_folder_in_place_and_keeps_the_old_program() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let staging = temp("stage-portable");
    let (dir, kind) = portable("portable");
    let (backend, release) = get_latest(&github, &staging);
    assert_eq!(release.version, "9.9.9");
    let cancel = AtomicBool::new(false);
    let staged = (backend.fetch)(&release, &cancel).unwrap();
    assert_eq!(staged.dir, staging.join("9.9.9"));
    assert!(staged.archive.is_file(), "the download waits in the staging folder, verified");
    let installed = (backend.install)(&kind, &staged, &cancel).unwrap();
    // spec: UPD-014
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), NEW_PROGRAM, "the program was replaced");
    assert_eq!(std::fs::read(dir.join("shale.old")).unwrap(), OLD_PROGRAM, "the previous program is the backup");
    assert_eq!(std::fs::read(dir.join("README.md")).unwrap(), b"readme of the new version");
    assert_eq!(std::fs::read(dir.join("install.sh")).unwrap(), include_bytes!("../../scripts/install.sh"));
    assert!(dir.join("shale.desktop").is_file() && dir.join("shale.svg").is_file(), "the new files of the folder are there too");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(dir.join("shale")).unwrap().permissions().mode() & 0o111, 0o111, "the new program can run");
    assert_eq!(installed.relaunch.program, dir.join("shale"));
    assert_eq!(installed.version, "9.9.9");
    assert!(!staging.join("9.9.9").exists(), "the staging folder is cleaned up");
    // spec: UPD-018
    let state_exe = dir.join("shale");
    remove_leftovers(&kind, &state_exe);
    assert!(!dir.join("shale.old").exists());
    assert!(github.hits().iter().any(|h| h == "/repos/rabalyn/shale/releases/latest"));
}

// the shipped install.sh uses `install -D` (GNU): it is the Linux download's script
#[cfg(target_os = "linux")]
#[test]
fn upd_015_an_install_sh_install_is_updated_through_the_new_install_sh_with_its_backup() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let staging = temp("stage-installed");
    let prefix = temp("prefix");
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    std::fs::write(prefix.join("bin/shale"), OLD_PROGRAM).unwrap();
    let kind = InstallKind::LinuxInstalled { prefix: prefix.clone(), user_default: false };
    let (backend, release) = get_latest(&github, &staging);
    let cancel = AtomicBool::new(false);
    let staged = (backend.fetch)(&release, &cancel).unwrap();
    let installed = (backend.install)(&kind, &staged, &cancel).unwrap();
    assert_eq!(std::fs::read(prefix.join("bin/shale")).unwrap(), NEW_PROGRAM, "install.sh put the new program in place");
    assert_eq!(std::fs::read(prefix.join("bin/shale.old")).unwrap(), OLD_PROGRAM, "the old one is kept");
    assert!(prefix.join("share/applications/shale.desktop").is_file() && prefix.join("share/icons/hicolor/scalable/apps/shale.svg").is_file(), "the desktop file and the icon too");
    let desktop = std::fs::read_to_string(prefix.join("share/applications/shale.desktop")).unwrap();
    assert!(desktop.contains(&format!("Exec={}", prefix.join("bin/shale").display())), "{desktop}");
    assert_eq!(installed.relaunch.program, prefix.join("bin/shale"));
}

#[cfg(unix)]
#[test]
fn upd_006_a_wrong_checksum_installs_nothing_and_leaves_the_install_as_it_was() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], true);
    let staging = temp("stage-bad");
    let (dir, _kind) = portable("portable-bad");
    let (backend, release) = get_latest(&github, &staging);
    let e = (backend.fetch)(&release, &AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("is damaged") && e.contains("nothing was installed"), "{e}");
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), OLD_PROGRAM);
    assert!(!dir.join("shale.old").exists());
    assert_eq!(std::fs::read(dir.join("README.md")).unwrap(), b"readme of the old version");
    assert!(!staging.join("9.9.9/shale-9.9.9-linux-x86_64.tar.gz").exists(), "the damaged download was deleted");
}

#[cfg(unix)]
#[test]
fn upd_006_a_kept_download_is_checked_again_and_a_damaged_one_is_fetched_anew() {
    let github = FakeGithub::start();
    let name = "shale-9.9.9-linux-x86_64.tar.gz";
    github.publish("9.9.9", &[(name, tarball(NEW_PROGRAM))], false);
    let staging = temp("stage-again");
    let (backend, release) = get_latest(&github, &staging);
    let cancel = AtomicBool::new(false);
    let staged = (backend.fetch)(&release, &cancel).unwrap();
    let before = github.hits().iter().filter(|h| h.ends_with(".tar.gz")).count();
    (backend.fetch)(&release, &cancel).unwrap();
    assert_eq!(github.hits().iter().filter(|h| h.ends_with(".tar.gz")).count(), before, "a download that matches is not fetched again");
    std::fs::write(&staged.archive, b"cut off").unwrap();
    (backend.fetch)(&release, &cancel).unwrap();
    assert_eq!(github.hits().iter().filter(|h| h.ends_with(".tar.gz")).count(), before + 1, "a damaged one is");
    assert_eq!(std::fs::read(&staged.archive).unwrap(), tarball(NEW_PROGRAM));
    // the folder of another version is removed
    std::fs::create_dir_all(staging.join("0.0.1")).unwrap();
    (backend.fetch)(&release, &cancel).unwrap();
    assert!(!staging.join("0.0.1").exists());
}

// spec: UPD-021
#[cfg(unix)]
#[test]
fn upd_003_an_address_that_is_not_a_release_file_of_shale_is_refused_before_curl_runs() {
    let release = Release { version: "9.9.9".into(), notes: String::new(), assets: release("9.9.9", "linux", "x86_64").assets.into_iter().map(|a| Asset { url: "http://127.0.0.1:1/x".into(), ..a }).collect() };
    let e = stage(&Source::github(), &release, "linux", "x86_64", &temp("refused"), &AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("not a download of Shale: refused"), "{e}");
}

#[cfg(unix)]
#[test]
fn upd_003_a_failed_request_is_an_error_in_words() {
    let github = FakeGithub::start();
    let backend = pretend_backend(&github, &temp("stage-404"));
    let e = (backend.check)(&AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("failed") && e.contains("404"), "{e}");
    github.serve("/repos/rabalyn/shale/releases/latest", "<html>not json</html>");
    assert!(((backend.check)(&AtomicBool::new(false))).unwrap_err().contains("not a release"));
    // a server that is not there
    // (a port held closed for the whole request: a released port could be handed to a parallel test's server, #659)
    let gone = crate::calendar::dav::tests::closed_port();
    let backend = Backend::real_for(Source::pretend(&format!("http://127.0.0.1:{}", gone.port)), temp("stage-gone"), "linux", "x86_64");
    assert!((backend.check)(&AtomicBool::new(false)).is_err());
    drop(gone);
}

#[cfg(unix)]
#[test]
fn upd_017_a_windows_zip_replaces_the_program_keeps_shale_exe_old_and_replaces_the_other_files() {
    let d = temp("windows");
    std::fs::write(d.join("shale.exe"), b"old exe").unwrap();
    std::fs::write(d.join("README.md"), b"old readme").unwrap();
    let archive = zip(&[z("shale/", b""), z("shale/shale.exe", b"new exe"), z("shale/README.md", b"new readme")]);
    let staged = Staged { version: "9.9.9".into(), dir: temp("windows-stage"), archive: temp("windows-archive").join("shale-9.9.9-windows-x86_64.zip") };
    std::fs::write(&staged.archive, archive).unwrap();
    let kind = InstallKind::Windows { dir: d.clone() };
    let installed = apply(&kind, &staged, &AtomicBool::new(false)).unwrap();
    assert_eq!(std::fs::read(d.join("shale.exe")).unwrap(), b"new exe");
    assert_eq!(std::fs::read(d.join("shale.exe.old")).unwrap(), b"old exe");
    assert_eq!(std::fs::read(d.join("README.md")).unwrap(), b"new readme");
    assert_eq!(installed.relaunch.program, d.join("shale.exe"));
    // the next start removes the leftover
    remove_leftovers(&kind, &d.join("shale.exe"));
    assert!(!d.join("shale.exe.old").exists());
    // a zip without the program is refused and changes nothing
    let bad = Staged { archive: staged.archive.with_file_name("bad.zip"), dir: temp("windows-stage-bad"), ..staged };
    std::fs::write(&bad.archive, zip(&[z("shale/README.md", b"x")])).unwrap();
    assert!(apply(&kind, &bad, &AtomicBool::new(false)).unwrap_err().contains("has no shale.exe"));
    assert_eq!(std::fs::read(d.join("shale.exe")).unwrap(), b"new exe");
}

// ---------------------------------------------------------------- the settings

// spec: UPD-001
#[test]
fn upd_001_the_default_is_ask_also_for_a_settings_file_from_before_the_updates() {
    assert_eq!(UpdateConfig::default().mode, UpdateMode::Ask);
    assert_eq!(crate::config::Config::default().updates, UpdateConfig::default());
    let (old, _) = crate::config::Config::parse_text("version = 2\nreadable_width = false\n").unwrap();
    assert_eq!(old.updates.mode, UpdateMode::Ask, "an old file has no [updates]: Ask");
    assert_eq!(old.version, 3, "a new field with a default needs no migration (CFG-001)");
    let (some, _) = crate::config::Config::parse_text("version = 2\n[updates]\nmode = \"Install\"\nskipped_version = \"0.7.0\"\nlast_check = 5\n").unwrap();
    assert_eq!((some.updates.mode, some.updates.skipped_version.as_str(), some.updates.last_check), (UpdateMode::Install, "0.7.0", 5));
    assert_eq!(some.updates.install_on_start, "");
    // a mode this Shale does not know falls back to Ask instead of losing the file
    let (odd, _) = crate::config::Config::parse_text("version = 2\n[updates]\nmode = \"Nightly\"\n").unwrap();
    assert_eq!(odd.updates.mode, UpdateMode::Ask);
    // written and read again
    let text = crate::config::Config { updates: UpdateConfig { mode: UpdateMode::Download, skipped_version: "1.2.3".into(), last_check: 42, install_on_start: "1.2.4".into() }, ..Default::default() }.file_text();
    assert!(text.contains("[updates]") && text.contains("mode = \"Download\"") && text.contains("# What Shale does about new releases"), "{text}");
    assert_eq!(crate::config::Config::parse_text(&text).unwrap().0.updates.install_on_start, "1.2.4");
}

// spec: UPD-002
#[test]
fn upd_002_the_environment_can_switch_updates_off() {
    assert!(env_off(Some("off")) && env_off(Some("OFF")) && env_off(Some(" Off ")));
    assert!(!env_off(None) && !env_off(Some("")) && !env_off(Some("on")) && !env_off(Some("0")));
    assert_eq!(effective_mode(UpdateMode::Install, true), UpdateMode::Off);
    assert_eq!(effective_mode(UpdateMode::Install, false), UpdateMode::Install);
    assert_eq!(effective_mode(UpdateMode::Ask, true), UpdateMode::Off);
}

// spec: UPD-019
#[test]
fn upd_019_a_start_after_restart_now_waits_for_the_old_shale_to_let_go() {
    let dir = temp("restart-lock");
    let Some(first) = crate::instance::acquire(&dir, Duration::ZERO).unwrap() else { panic!("the first start owns the lock") };
    // a normal second start gives up after the short grace time ...
    assert!(matches!(crate::instance::claim(&dir, crate::instance::GRACE), crate::instance::Claim::AlreadyRunning));
    // ... one started by "Restart now" waits until the old one has ended
    std::thread::spawn(move || {
        std::thread::sleep(crate::instance::GRACE * 3);
        drop(first);
    });
    assert!(matches!(crate::instance::claim(&dir, crate::instance::RESTART_WAIT), crate::instance::Claim::Owner(_)));
}

// ---------------------------------------------------------------- the decisions of each mode

#[derive(Clone, Default)]
pub(super) struct Counts {
    pub(super) checks: Arc<AtomicUsize>,
    pub(super) fetches: Arc<AtomicUsize>,
    pub(super) installs: Arc<AtomicUsize>,
    /// what the next check finds
    pub(super) latest: Arc<Mutex<Option<Release>>>,
    pub(super) check_error: Arc<Mutex<Option<String>>>,
    pub(super) fetch_error: Arc<Mutex<Option<String>>>,
}

impl Counts {
    pub(super) fn n(c: &Arc<AtomicUsize>) -> usize {
        c.load(Ordering::SeqCst)
    }
}

/// A backend that counts its calls and answers what `Counts` says; `latest` is the version it finds.
pub(super) fn fake(latest: Option<&str>) -> (Backend, Counts) {
    let c = Counts::default();
    *c.latest.lock().unwrap() = latest.map(|v| release(v, "linux", "x86_64"));
    let (c1, c2, c3) = (c.clone(), c.clone(), c.clone());
    let backend = Backend {
        check: Arc::new(move |_| {
            c1.checks.fetch_add(1, Ordering::SeqCst);
            match c1.check_error.lock().unwrap().clone() {
                Some(e) => Err(e),
                None => Ok(c1.latest.lock().unwrap().clone()),
            }
        }),
        fetch: Arc::new(move |r, _| {
            c2.fetches.fetch_add(1, Ordering::SeqCst);
            match c2.fetch_error.lock().unwrap().clone() {
                Some(e) => Err(e),
                None => Ok(Staged { version: r.version.clone(), dir: "/stage".into(), archive: format!("/stage/shale-{}.tar.gz", r.version).into() }),
            }
        }),
        install: Arc::new(move |kind, staged, _| {
            c3.installs.fetch_add(1, Ordering::SeqCst);
            Ok(Installed { version: staged.version.clone(), relaunch: relaunch_for(kind).unwrap_or(Relaunch { program: "/x".into(), args: vec![] }) })
        }),
    };
    (backend, c)
}

/// The same fake with the three steps held back: the check and the download wait for the gate or
/// for Cancel (then they end with "cancelled"), the install waits for the gate only.
pub(super) fn gated(latest: Option<&str>) -> (Backend, Counts, Arc<AtomicBool>) {
    let (inner, counts) = fake(latest);
    let gate = Arc::new(AtomicBool::new(false));
    fn wait(gate: &AtomicBool, cancel: Option<&AtomicBool>) -> Result<(), String> {
        while !gate.load(Ordering::SeqCst) {
            if cancel.is_some_and(|c| c.load(Ordering::SeqCst)) {
                return Err("cancelled".into());
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }
    let (g1, g2, g3) = (gate.clone(), gate.clone(), gate.clone());
    let (check, fetch, install) = (inner.check.clone(), inner.fetch.clone(), inner.install.clone());
    let backend = Backend {
        check: Arc::new(move |cancel| {
            wait(&g1, Some(cancel))?;
            check(cancel)
        }),
        fetch: Arc::new(move |r, cancel| {
            wait(&g2, Some(cancel))?;
            fetch(r, cancel)
        }),
        install: Arc::new(move |kind, staged, cancel| {
            wait(&g3, None)?;
            install(kind, staged, cancel)
        }),
    };
    (backend, counts, gate)
}

pub(super) fn portable_kind() -> InstallKind {
    InstallKind::LinuxPortable { dir: "/p".into(), whole: true }
}

/// A state with updates on that checks at once, an install that is writable, and the platform of the fakes.
pub(super) fn state(kind: InstallKind, backend: Backend) -> UpdateState {
    let mut st = UpdateState::with(kind, "/p/shale".into(), backend, true);
    st.env_off = false;
    st.writable = true;
    st.platform = ("linux", "x86_64");
    st.current = "0.6.6".into();
    st.start_delay = Duration::ZERO;
    st
}

/// Frames until `done` (a job ends in a moment), at most 10 seconds.
fn pump(st: &mut UpdateState, cfg: &mut UpdateConfig, done: impl Fn(&UpdateState) -> bool) {
    let ctx = egui::Context::default();
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(10) {
        st.tick(&ctx, cfg);
        if done(st) && !st.busy() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("the updates did not settle: {:?} {:?}", st.phase, st.result);
}

fn cfg(mode: UpdateMode) -> UpdateConfig {
    UpdateConfig { mode, ..Default::default() }
}

// spec: UPD-002
#[test]
fn upd_002_off_makes_no_request_and_shows_nothing_also_with_a_newer_release_waiting() {
    for (mode, env) in [(UpdateMode::Off, false), (UpdateMode::Install, true), (UpdateMode::Ask, true)] {
        let (backend, counts) = fake(Some("9.9.9"));
        let mut st = state(portable_kind(), backend);
        st.env_off = env;
        let mut c = cfg(mode);
        let ctx = egui::Context::default();
        for _ in 0..20 {
            st.tick(&ctx, &mut c);
            std::thread::sleep(Duration::from_millis(2));
        }
        st.check_now();
        st.tick(&ctx, &mut c);
        assert_eq!((Counts::n(&counts.checks), Counts::n(&counts.fetches), Counts::n(&counts.installs)), (0, 0, 0), "{mode:?} env={env}: the checker was not called");
        assert!(st.notice(&c).is_none() && !st.ask && st.found.is_none() && !st.busy());
    }
    // a disabled state (tests, pictures) does not ask either
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    st.enabled = false;
    st.tick(&egui::Context::default(), &mut cfg(UpdateMode::Notify));
    assert_eq!(Counts::n(&counts.checks), 0);
}

// spec: UPD-002
#[test]
fn upd_002_switching_to_off_while_an_update_is_shown_takes_the_notice_away() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Notify);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert!(st.notice(&c).is_some());
    c.mode = UpdateMode::Off;
    st.tick(&egui::Context::default(), &mut c);
    assert!(st.notice(&c).is_none() && st.found.is_none());
    let checks = Counts::n(&counts.checks);
    std::thread::sleep(Duration::from_millis(20));
    st.tick(&egui::Context::default(), &mut c);
    assert_eq!(Counts::n(&counts.checks), checks, "no further request");
}

// spec: UPD-003
#[test]
fn upd_003_the_first_check_waits_the_delay_and_the_next_one_a_day() {
    let (backend, counts) = fake(None);
    let mut st = state(portable_kind(), backend);
    st.start_delay = Duration::from_secs(3600);
    let mut c = cfg(UpdateMode::Notify);
    let ctx = egui::Context::default();
    st.tick(&ctx, &mut c);
    assert_eq!(Counts::n(&counts.checks), 0, "not at the very start");
    st.start_delay = Duration::ZERO;
    pump(&mut st, &mut c, |s| s.result.is_some());
    assert_eq!(Counts::n(&counts.checks), 1);
    assert!(c.last_check > 0, "the time of the check is kept");
    assert_eq!(st.result, Some(CheckResult::UpToDate("0.6.6".into())));
    for _ in 0..5 {
        st.tick(&ctx, &mut c);
    }
    assert_eq!(Counts::n(&counts.checks), 1, "not again within the day");
    st.next_check = Some(Instant::now() - Duration::from_secs(1));
    pump(&mut st, &mut c, |_| Counts::n(&counts.checks) == 2);
    assert_eq!(Counts::n(&counts.checks), 2, "a day later it checks again");
    // "Check for updates now" does not wait
    st.check_now();
    pump(&mut st, &mut c, |_| Counts::n(&counts.checks) == 3);
    assert_eq!(Counts::n(&counts.checks), 3);
}

// spec: UPD-003
#[test]
fn upd_003_a_failed_check_is_a_result_in_words_and_is_tried_again_in_an_hour() {
    let (backend, counts) = fake(None);
    *counts.check_error.lock().unwrap() = Some("could not reach GitHub".into());
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Notify);
    pump(&mut st, &mut c, |s| s.result.is_some());
    assert_eq!(st.result, Some(CheckResult::Failed("could not reach GitHub".into())));
    assert_eq!(c.last_check, 0, "a failed check is not a check");
    let wait = st.next_check.unwrap().saturating_duration_since(Instant::now());
    assert!(wait > Duration::from_secs(3000) && wait <= Duration::from_secs(3600), "{wait:?}");
    assert!(st.notice(&c).is_none());
}

// spec: UPD-003, UPD-018, UPD-019
#[test]
fn upd_003_the_timings_are_the_ones_the_rules_name() {
    assert_eq!(START_DELAY, Duration::from_secs(20), "the first check, UPD-003");
    assert_eq!(CHECK_EVERY, Duration::from_secs(24 * 3600), "then every 24 hours, UPD-003");
    assert_eq!(RETRY_AFTER, Duration::from_secs(3600), "a failed check is tried again in an hour, UPD-003");
    assert_eq!(BACKUP_KEPT, Duration::from_secs(30), "the backup goes 30 seconds after the start, UPD-018");
    assert_eq!(crate::instance::RESTART_WAIT, Duration::from_secs(20), "a start after Restart now waits up to 20 seconds, UPD-019");
}

// spec: UPD-003
#[cfg(unix)]
#[test]
fn upd_003_the_check_is_one_get_of_the_latest_release_with_the_github_accept_header() {
    let github = FakeGithub::start();
    // no release is published: the check fails with a 404, the request is what counts
    let _ = (pretend_backend(&github, &temp("stage-head")).check)(&AtomicBool::new(false));
    let heads = github.heads();
    assert_eq!(heads.len(), 1, "one request: {heads:?}");
    assert!(heads[0].starts_with("GET /repos/rabalyn/shale/releases/latest "), "{}", heads[0]);
    assert!(heads[0].to_ascii_lowercase().contains("accept: application/vnd.github+json"), "{}", heads[0]);
}

// spec: UPD-002
#[test]
fn upd_002_switching_to_off_cancels_what_runs() {
    let (backend, counts, _gate) = gated(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Notify);
    let ctx = egui::Context::default();
    st.tick(&ctx, &mut c);
    assert!(st.busy(), "the check runs and waits at the gate");
    assert!(!st.cancel.load(Ordering::SeqCst));
    c.mode = UpdateMode::Off;
    st.tick(&ctx, &mut c);
    assert!(st.cancel.load(Ordering::SeqCst), "the running job is told to stop");
    assert!(!st.busy() && st.found.is_none() && st.notice(&c).is_none());
    assert_eq!(Counts::n(&counts.checks), 0, "the check never got its answer");
}

// spec: UPD-003
#[test]
fn upd_003_a_release_that_is_not_newer_is_not_offered() {
    for latest in ["0.6.6", "0.6.5", "0.5.0"] {
        let (backend, _) = fake(Some(latest));
        let mut st = state(portable_kind(), backend);
        let mut c = cfg(UpdateMode::Install);
        pump(&mut st, &mut c, |s| s.result.is_some());
        assert_eq!(st.result, Some(CheckResult::UpToDate("0.6.6".into())), "{latest}");
        assert!(st.found.is_none() && st.notice(&c).is_none() && !st.ask);
    }
}

// spec: UPD-007
#[test]
fn upd_007_ask_asks_once_and_the_answer_is_used_from_then_on() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Ask);
    pump(&mut st, &mut c, |s| s.ask);
    assert!(st.ask, "the first update found asks");
    assert!(st.notice(&c).is_none(), "the question is on the screen instead of the notice");
    assert_eq!(Counts::n(&counts.fetches), 0, "nothing is downloaded before the answer");
    st.choose(&egui::Context::default(), &mut c, UpdateMode::Download);
    assert_eq!((c.mode, st.ask), (UpdateMode::Download, false), "the answer is kept");
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Ready(_)));
    assert_eq!(Counts::n(&counts.fetches), 1, "and used for the update that was found");
    // the next check asks nothing
    st.next_check = Some(Instant::now() - Duration::from_secs(1));
    pump(&mut st, &mut c, |_| Counts::n(&counts.checks) == 2);
    assert!(!st.ask);
}

// spec: UPD-007
#[test]
fn upd_007_answering_off_ends_everything() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Ask);
    pump(&mut st, &mut c, |s| s.ask);
    st.choose(&egui::Context::default(), &mut c, UpdateMode::Off);
    assert_eq!(c.mode, UpdateMode::Off);
    assert!(st.found.is_none() && st.notice(&c).is_none() && !st.ask);
    st.tick(&egui::Context::default(), &mut c);
    assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.checks)), (0, 1));
}

// spec: UPD-008
#[test]
fn upd_008_notify_announces_without_downloading_and_skipping_hides_only_that_version() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Notify);
    pump(&mut st, &mut c, |s| s.found.is_some());
    let n = st.notice(&c).unwrap();
    assert_eq!(n.text, "Shale 9.9.9 is available.");
    assert_eq!(n.buttons.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["What's new", "Download", "Install", "Skip this version", "Later"]);
    assert_eq!(Counts::n(&counts.fetches), 0, "Notify downloads nothing by itself");
    // Skip this version: kept in the settings, gone from the screen
    assert!(st.act(&egui::Context::default(), &mut c, Action::Skip), "the settings changed");
    assert_eq!(c.skipped_version, "9.9.9");
    assert!(st.notice(&c).is_none());
    // the same version found again stays quiet ...
    st.check_now();
    pump(&mut st, &mut c, |_| Counts::n(&counts.checks) == 2);
    assert!(st.notice(&c).is_none() && !st.ask);
    // ... a newer one is announced
    *counts.latest.lock().unwrap() = Some(release("9.9.10", "linux", "x86_64"));
    st.check_now();
    pump(&mut st, &mut c, |_| Counts::n(&counts.checks) == 3);
    assert_eq!(st.notice(&c).unwrap().text, "Shale 9.9.10 is available.");
    assert_eq!(c.skipped_version, "9.9.9", "skipping 9.9.9 does not skip 9.9.10");
}

// spec: UPD-008, UPD-007
#[test]
fn upd_008_skipping_a_version_also_suppresses_the_question_of_ask() {
    let (backend, _) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = UpdateConfig { mode: UpdateMode::Ask, skipped_version: "9.9.9".into(), ..Default::default() };
    pump(&mut st, &mut c, |s| s.result.is_some());
    assert!(!st.ask && st.notice(&c).is_none());
    assert_eq!(st.result, Some(CheckResult::Available("9.9.9".into())), "the settings page still says it exists");
}

// spec: UPD-008
#[test]
fn upd_008_later_hides_the_notice_until_the_next_check() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Notify);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert!(!st.act(&egui::Context::default(), &mut c, Action::Later), "nothing is saved");
    assert!(st.notice(&c).is_none());
    st.check_now();
    pump(&mut st, &mut c, |_| Counts::n(&counts.checks) == 2);
    assert!(st.notice(&c).is_some(), "the next check announces it again");
}

// spec: UPD-009
#[test]
fn upd_009_download_fetches_in_the_background_and_offers_install_now_or_at_the_next_start() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Download);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Ready(_)));
    assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.installs)), (1, 0), "downloaded, not installed");
    let n = st.notice(&c).unwrap();
    assert!(n.text.contains("downloaded and checked, ready to install"), "{}", n.text);
    assert_eq!(n.buttons.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["Install now", "At next start", "What's new"]);
    // at the next start: kept in the settings, the notice goes
    assert!(st.act(&egui::Context::default(), &mut c, Action::InstallAtStart));
    assert_eq!(c.install_on_start, "9.9.9");
    assert!(st.notice(&c).is_none());
    // a new start: the check finds the version again and installs it without asking
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.installs)), (1, 1));
    assert_eq!(c.install_on_start, "", "the wish is used up");
}

// spec: UPD-009
#[test]
fn upd_009_install_now_installs_the_downloaded_version() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Download);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Ready(_)));
    st.act(&egui::Context::default(), &mut c, Action::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.installs)), (1, 1), "the download is not repeated");
}

// spec: UPD-009
#[test]
fn upd_009_a_wish_to_install_at_start_is_dropped_when_that_version_is_no_longer_newer() {
    let (backend, counts) = fake(Some("0.6.6"));
    let mut st = state(portable_kind(), backend);
    let mut c = UpdateConfig { mode: UpdateMode::Download, install_on_start: "0.6.6".into(), ..Default::default() };
    pump(&mut st, &mut c, |s| s.result.is_some());
    assert_eq!((c.install_on_start.as_str(), Counts::n(&counts.fetches)), ("", 0));
}

// spec: UPD-010
#[test]
fn upd_010_install_downloads_verifies_installs_and_offers_the_restart() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.installs)), (1, 1));
    let n = st.notice(&c).unwrap();
    assert_eq!(n.text, "Updated to Shale 9.9.9. Restart to use it.");
    assert_eq!(n.buttons.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["Restart now", "Later"]);
    // Later: the notice goes, the new version is used at the next start; nothing restarts
    st.act(&egui::Context::default(), &mut c, Action::Later);
    assert!(st.notice(&c).is_none() && st.restart.is_none());
}

// spec: UPD-019
#[test]
fn upd_019_restart_now_asks_the_window_to_close_and_remembers_what_to_start() {
    let (backend, _) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    st.act(&egui::Context::default(), &mut c, Action::RestartNow);
    assert_eq!(st.restart, Some(Relaunch { program: "/p/shale".into(), args: vec![RESTARTED.into()] }));
    // without an installed update there is nothing to restart into
    let (backend, _) = fake(None);
    let mut idle = state(portable_kind(), backend);
    idle.act(&egui::Context::default(), &mut c, Action::RestartNow);
    assert_eq!(idle.restart, None);
}

// spec: UPD-019
#[cfg(unix)]
#[test]
fn upd_019_the_new_program_is_started_when_the_old_one_has_saved() {
    let d = temp("relaunch");
    let script = d.join("shale");
    crate::test_exec::write_script(&script, &format!("#!/bin/sh\necho \"$@\" > {}\n", d.join("started").display()));
    let (backend, _) = fake(None);
    let mut st = state(portable_kind(), backend);
    st.start_restart();
    assert!(!d.join("started").exists(), "nothing to start without a restart");
    st.restart = Some(Relaunch { program: script, args: vec![RESTARTED.into()] });
    st.start_restart();
    assert!(st.restart.is_none());
    let t = Instant::now();
    while !d.join("started").exists() && t.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(std::fs::read_to_string(d.join("started")).unwrap().trim(), RESTARTED);
    // a program that cannot be started is an error in words, not a crash
    assert!(Relaunch { program: d.join("missing"), args: vec![] }.spawn().unwrap_err().contains("could not start"));
}

// spec: UPD-012
#[test]
fn upd_012_a_package_managed_install_only_notifies_whatever_the_mode() {
    for mode in [UpdateMode::Notify, UpdateMode::Download, UpdateMode::Install] {
        let (backend, counts) = fake(Some("9.9.9"));
        let mut st = state(InstallKind::PackageManaged, backend);
        let mut c = cfg(mode);
        pump(&mut st, &mut c, |s| s.found.is_some());
        assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.installs)), (0, 0), "{mode:?}: nothing is downloaded or touched");
        let n = st.notice(&c).unwrap();
        assert!(n.text.contains("Shale 9.9.9 is available.") && n.text.contains("yay -Syu shale-bin"), "{}", n.text);
        assert_eq!(n.buttons.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["What's new", "Skip this version", "Later"], "no Download or Install button");
    }
}

// spec: UPD-013
#[test]
fn upd_013_a_development_build_only_notifies() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(InstallKind::Development, backend);
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert_eq!(Counts::n(&counts.fetches), 0);
    assert!(st.notice(&c).unwrap().text.contains("development build"));
}

// spec: UPD-015
#[test]
fn upd_015_without_write_access_install_downloads_and_gives_the_command() {
    let (backend, counts) = fake(Some("9.9.9"));
    let kind = InstallKind::LinuxInstalled { prefix: "/usr/local".into(), user_default: false };
    let mut st = state(kind, backend);
    st.writable = false;
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Ready(_)));
    assert_eq!((Counts::n(&counts.fetches), Counts::n(&counts.installs)), (1, 0), "downloaded, never installed");
    let n = st.notice(&c).unwrap();
    assert!(n.text.contains("/usr/local/bin cannot be written") && n.text.contains("sudo sh") && n.text.contains("install.sh") && n.text.contains("--prefix \"/usr/local\""), "{}", n.text);
    assert_eq!(n.buttons.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["Copy the command", "Dismiss"]);
    match &n.buttons[0].1 {
        ui::Press::Copy(cmd) => assert!(cmd.starts_with("tar -xzf ") && cmd.contains("sudo sh"), "{cmd}"),
        p => panic!("{p:?}"),
    }
    st.act(&egui::Context::default(), &mut c, Action::Dismiss);
    assert!(st.notice(&c).is_none());
}

// spec: UPD-005
#[test]
fn upd_005_a_release_without_a_file_for_this_system_only_notifies_and_says_so() {
    let (backend, counts) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    st.platform = ("freebsd", "x86_64");
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert_eq!(Counts::n(&counts.fetches), 0);
    let n = st.notice(&c).unwrap();
    assert!(n.text.contains("cannot be downloaded from here") && n.text.contains("no download of Shale 9.9.9 for freebsd on x86_64"), "{}", n.text);
    assert!(!n.buttons.iter().any(|(l, _)| *l == "Download" || *l == "Install"));
}

// spec: UPD-010
#[test]
fn upd_010_a_failed_download_or_install_is_shown_in_words_and_can_be_tried_again() {
    let (backend, counts) = fake(Some("9.9.9"));
    *counts.fetch_error.lock().unwrap() = Some("the download took too long and was stopped".into());
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Failed(_)));
    let n = st.notice(&c).unwrap();
    assert!(n.error && n.text.contains("The update to Shale 9.9.9 failed: the download took too long"), "{}", n.text);
    assert_eq!(n.buttons.iter().map(|(l, _)| *l).collect::<Vec<_>>(), ["Try again", "Dismiss"]);
    *counts.fetch_error.lock().unwrap() = None;
    st.act(&egui::Context::default(), &mut c, Action::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    assert_eq!(Counts::n(&counts.installs), 1);
    // a cancelled job is not an error
    let (backend, counts) = fake(Some("9.9.9"));
    *counts.fetch_error.lock().unwrap() = Some("cancelled".into());
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Download);
    pump(&mut st, &mut c, |_| Counts::n(&counts.fetches) == 1);
    assert_eq!(st.phase, Phase::Idle);
}

// spec: UPD-010
#[test]
fn upd_010_a_failed_install_is_shown_and_the_notice_can_be_dismissed() {
    let (mut backend, _) = fake(Some("9.9.9"));
    backend.install = Arc::new(|_, _, _| Err("could not put the new file in place: /p/shale (Permission denied)".into()));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Failed(_)));
    assert!(st.notice(&c).unwrap().text.contains("Permission denied"));
    st.act(&egui::Context::default(), &mut c, Action::Dismiss);
    assert!(st.notice(&c).is_none() && st.phase == Phase::Idle);
}

// spec: UPD-020
#[test]
fn upd_020_the_release_notes_open_and_close() {
    let (backend, _) = fake(Some("9.9.9"));
    let mut st = state(portable_kind(), backend);
    let mut c = cfg(UpdateMode::Notify);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert!(!st.notes_open);
    st.act(&egui::Context::default(), &mut c, Action::ShowNotes);
    assert!(st.notes_open);
    assert_eq!(st.found.as_ref().unwrap().release.notes, "Notes of 9.9.9");
}

// spec: UPD-003
#[test]
fn upd_003_when_a_time_is_shown_it_is_in_local_time_as_year_month_day_and_clock() {
    let text = when(1_800_000_000);
    assert_eq!(text.len(), "2027-01-15 08:00".len(), "{text}");
    assert!(text.starts_with("2027-01-1"), "{text}");
}

// ---------------------------------------------------------------- the whole way through the state

// spec: UPD-006
#[cfg(unix)]
#[test]
fn upd_010_the_state_installs_a_portable_folder_from_the_pretend_github_in_install_mode() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let staging = temp("stage-state");
    let (dir, kind) = portable("portable-state");
    let mut st = UpdateState::with(kind, dir.join("shale"), pretend_backend(&github, &staging), true);
    st.env_off = false;
    st.platform = ("linux", "x86_64");
    st.start_delay = Duration::ZERO;
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), NEW_PROGRAM);
    assert_eq!(std::fs::read(dir.join("shale.old")).unwrap(), OLD_PROGRAM);
    assert!(c.last_check > 0);
    // a checksum that does not match: nothing is changed and the notice says why
    let github = FakeGithub::start();
    github.publish("9.9.10", &[("shale-9.9.10-linux-x86_64.tar.gz", tarball(b"#!/bin/sh\necho evil\n"))], true);
    let (dir, kind) = portable("portable-state-bad");
    let mut st = UpdateState::with(kind, dir.join("shale"), pretend_backend(&github, &temp("stage-state-bad")), true);
    st.env_off = false;
    st.platform = ("linux", "x86_64");
    st.start_delay = Duration::ZERO;
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Failed(_)));
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), OLD_PROGRAM);
    assert!(st.notice(&c).unwrap().text.contains("is damaged"), "{}", st.notice(&c).unwrap().text);
}

#[cfg(unix)]
#[test]
fn upd_006_the_downloads_are_in_the_cache_folder_of_shale() {
    let root = staging_root();
    assert!(root.ends_with("shale/updates"), "{}", root.display());
}

// ---------------------------------------------------------------- the dataflow diagrams (DEV-032)

fn update_doc() -> PathBuf {
    crate::dataflow::doc("update.md")
}

// spec: DEV-026, DEV-032
#[cfg(unix)]
#[test]
fn dev_032_dataflow_of_the_check_for_a_release() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let backend = pretend_backend(&github, &temp("flow-check"));
    let flow = crate::dataflow::record(|| assert!((backend.check)(&AtomicBool::new(false)).unwrap().is_some()));
    crate::dataflow::verify_flow(&flow, "update_check", &update_doc());
}

// spec: DEV-026, DEV-032
#[cfg(unix)]
#[test]
fn dev_032_dataflow_of_a_download_with_its_checksum() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let (backend, release) = get_latest(&github, &temp("flow-download"));
    let flow = crate::dataflow::record(|| {
        (backend.fetch)(&release, &AtomicBool::new(false)).unwrap();
    });
    crate::dataflow::verify_flow(&flow, "update_download", &update_doc());
}

// spec: DEV-026, DEV-032
#[cfg(unix)]
#[test]
fn dev_032_dataflow_of_an_install_into_a_portable_folder() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let (dir, kind) = portable("flow-portable");
    let (backend, release) = get_latest(&github, &temp("flow-stage-portable"));
    let cancel = AtomicBool::new(false);
    let staged = (backend.fetch)(&release, &cancel).unwrap();
    let flow = crate::dataflow::record(|| {
        (backend.install)(&kind, &staged, &cancel).unwrap();
    });
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), NEW_PROGRAM);
    crate::dataflow::verify_flow(&flow, "update_install_folder", &update_doc());
}

// the shipped install.sh uses `install -D` (GNU): it is the Linux download's script
// spec: DEV-026, DEV-032
#[cfg(target_os = "linux")]
#[test]
fn dev_032_dataflow_of_an_install_through_install_sh() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[("shale-9.9.9-linux-x86_64.tar.gz", tarball(NEW_PROGRAM))], false);
    let prefix = temp("flow-prefix");
    std::fs::create_dir_all(prefix.join("bin")).unwrap();
    std::fs::write(prefix.join("bin/shale"), OLD_PROGRAM).unwrap();
    let kind = InstallKind::LinuxInstalled { prefix: prefix.clone(), user_default: false };
    let (backend, release) = get_latest(&github, &temp("flow-stage-installed"));
    let cancel = AtomicBool::new(false);
    let staged = (backend.fetch)(&release, &cancel).unwrap();
    let flow = crate::dataflow::record(|| {
        (backend.install)(&kind, &staged, &cancel).unwrap();
    });
    assert_eq!(std::fs::read(prefix.join("bin/shale")).unwrap(), NEW_PROGRAM);
    crate::dataflow::verify_flow(&flow, "update_install_script", &update_doc());
}

// ---------------------------------------------------------------- signed, unsigned and tampered releases (UPD-022)

#[cfg(unix)]
const SIGNED_FILE: &str = "shale-9.9.9-linux-x86_64.tar.gz";

/// What `fetch` makes of a release signed as `sign` (with the checksums right).
#[cfg(unix)]
fn fetch_signed(sign: Sign, tag: &str) -> (Result<Staged, String>, PathBuf, FakeGithub) {
    let github = FakeGithub::start();
    github.publish_signed("9.9.9", &[(SIGNED_FILE, tarball(NEW_PROGRAM))], false, sign);
    let staging = temp(tag);
    let (backend, release) = get_latest(&github, &staging);
    let got = (backend.fetch)(&release, &AtomicBool::new(false));
    (got, staging, github)
}

// spec: UPD-024
#[cfg(unix)]
#[test]
fn upd_022_a_signed_release_is_downloaded_with_its_signature_and_checked_against_it() {
    let (got, staging, github) = fetch_signed(Sign::Valid, "stage-signed");
    let staged = got.unwrap();
    assert!(staged.archive.is_file());
    assert!(staging.join("9.9.9").join(format!("{SIGNED_FILE}.sig")).is_file(), "the signature is kept next to the download");
    assert!(github.hits().iter().any(|h| h == &format!("/dl/{SIGNED_FILE}.sig")));
}

// spec: UPD-023
#[cfg(unix)]
#[test]
fn upd_022_an_unsigned_release_downloads_nothing_and_tells_where_to_get_it() {
    let (got, staging, github) = fetch_signed(Sign::Unsigned, "stage-unsigned");
    let e = got.unwrap_err();
    assert!(e.contains("no signature file") && e.contains("releases/tag/v9.9.9"), "{e}");
    assert!(!github.hits().iter().any(|h| h.ends_with(".tar.gz")), "the archive was not even fetched");
    assert!(!staging.join("9.9.9").join(SIGNED_FILE).exists());
}

// spec: UPD-024
#[cfg(unix)]
#[test]
fn upd_022_a_file_changed_after_signing_a_foreign_key_and_a_foreign_name_are_refused_and_deleted() {
    for (sign, tag, why) in [(Sign::Tampered, "stage-tampered", "does not match the file"), (Sign::OtherKey, "stage-other-key", "not signed with the release key"), (Sign::OtherName, "stage-other-name", "signed for another file")] {
        let (got, staging, _github) = fetch_signed(sign, tag);
        let e = got.unwrap_err();
        assert!(e.contains("is not signed by the owner of Shale's release key") && e.contains(why) && e.contains("nothing was installed"), "{sign:?}: {e}");
        assert!(e.contains("https://github.com/rabalyn/shale/releases"), "{e}");
        let dir = staging.join("9.9.9");
        assert!(!dir.join(SIGNED_FILE).exists() && !dir.join(format!("{SIGNED_FILE}.sig")).exists(), "{sign:?}: deleted");
    }
}

// spec: UPD-024
#[cfg(unix)]
#[test]
fn upd_022_a_damaged_checksum_is_reported_before_the_signature_and_a_signature_file_that_is_no_signature_is_refused() {
    let github = FakeGithub::start();
    github.publish_signed("9.9.9", &[(SIGNED_FILE, tarball(NEW_PROGRAM))], true, Sign::Valid);
    let (backend, release) = get_latest(&github, &temp("stage-sum-first"));
    assert!(((backend.fetch)(&release, &AtomicBool::new(false))).unwrap_err().contains("is damaged"));
    github.serve(&format!("/dl/{SIGNED_FILE}.sig"), "not a signature");
    github.publish_signed("9.9.9", &[(SIGNED_FILE, tarball(NEW_PROGRAM))], false, Sign::Valid);
    github.serve(&format!("/dl/{SIGNED_FILE}.sig"), "not a signature");
    let (backend, release) = get_latest(&github, &temp("stage-junk-sig"));
    let e = (backend.fetch)(&release, &AtomicBool::new(false)).unwrap_err();
    assert!(e.contains("not signed by the owner") && e.contains("no BEGIN and END"), "{e}");
}

// spec: UPD-024
#[cfg(unix)]
#[test]
fn upd_022_a_kept_download_is_checked_against_the_signature_again_and_a_swapped_one_is_fetched_anew() {
    let github = FakeGithub::start();
    github.publish("9.9.9", &[(SIGNED_FILE, tarball(NEW_PROGRAM))], false);
    let staging = temp("stage-kept-signed");
    let (backend, release) = get_latest(&github, &staging);
    let cancel = AtomicBool::new(false);
    let staged = (backend.fetch)(&release, &cancel).unwrap();
    let archives = |g: &FakeGithub| g.hits().iter().filter(|h| h.ends_with(".tar.gz")).count();
    (backend.fetch)(&release, &cancel).unwrap();
    assert_eq!(archives(&github), 1, "a kept download that is signed is not fetched again");
    // the account is taken over: another file, with a checksum that fits, and the old signature
    let evil = tarball(b"#!/bin/sh\necho evil\n");
    github.serve(&format!("/dl/{SIGNED_FILE}"), evil.clone());
    github.serve(&format!("/dl/{SIGNED_FILE}.sha256"), format!("{}  {SIGNED_FILE}\n", crate::pdf::tex::archive::sha256_hex(&evil)));
    std::fs::write(&staged.archive, &evil).unwrap();
    let e = (backend.fetch)(&release, &cancel).unwrap_err();
    assert!(e.contains("does not match the file"), "{e}");
    assert_eq!(archives(&github), 2, "the kept file was not trusted: it was fetched anew and refused");
    assert!(!staged.archive.exists());
}

// spec: UPD-024
#[cfg(unix)]
#[test]
fn upd_022_a_tampered_release_installs_nothing_through_the_state() {
    let github = FakeGithub::start();
    github.publish_signed("9.9.9", &[(SIGNED_FILE, tarball(b"#!/bin/sh\necho evil\n"))], false, Sign::Tampered);
    let (dir, kind) = portable("portable-tampered");
    let mut st = UpdateState::with(kind, dir.join("shale"), pretend_backend(&github, &temp("stage-state-tampered")), true);
    st.env_off = false;
    st.platform = ("linux", "x86_64");
    st.start_delay = Duration::ZERO;
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Failed(_)));
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), OLD_PROGRAM);
    let n = st.notice(&c).unwrap();
    assert!(n.error && n.text.contains("not signed by the owner of Shale's release key"), "{}", n.text);
}

// spec: UPD-023
#[cfg(unix)]
#[test]
fn upd_022_an_unsigned_release_in_install_mode_is_announced_not_installed_through_the_state() {
    let github = FakeGithub::start();
    github.publish_signed("9.9.9", &[(SIGNED_FILE, tarball(NEW_PROGRAM))], false, Sign::Unsigned);
    let (dir, kind) = portable("portable-unsigned");
    let mut st = UpdateState::with(kind, dir.join("shale"), pretend_backend(&github, &temp("stage-state-unsigned")), true);
    st.env_off = false;
    st.platform = ("linux", "x86_64");
    st.start_delay = Duration::ZERO;
    let mut c = cfg(UpdateMode::Install);
    pump(&mut st, &mut c, |s| s.found.is_some());
    assert_eq!(st.phase, Phase::Idle, "nothing is downloaded or installed");
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), OLD_PROGRAM);
    assert!(!github.hits().iter().any(|h| h.ends_with(".tar.gz")));
    let n = st.notice(&c).unwrap();
    assert!(n.text.contains("no signature file") && !n.error, "{}", n.text);
    // the owner signs it later: the next check offers the installation
    github.publish("9.9.9", &[(SIGNED_FILE, tarball(NEW_PROGRAM))], false);
    st.check_now();
    pump(&mut st, &mut c, |s| matches!(s.phase, Phase::Installed(_)));
    assert_eq!(std::fs::read(dir.join("shale")).unwrap(), NEW_PROGRAM);
}
