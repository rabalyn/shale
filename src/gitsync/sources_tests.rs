//! Where the managed git comes from (SYNC-029, SYNC-027, ADR-0027): the pinned table, what is offered
//! on each platform, the install commands of the distributions (`os-release`), and the release
//! of a project read from GitHub's API answer.

use super::sources::*;

fn have(git: bool, lfs: bool) -> Have {
    Have { git, lfs }
}

// spec: SYNC-029
#[test]
fn the_real_table_pins_every_download_with_its_sha256_and_size() {
    let s = Sources::real();
    assert_eq!((s.download_base.as_str(), s.api_base.as_str(), s.allow_http), ("https://github.com", "https://api.github.com", false));
    assert_eq!(s.table.len(), 6, "windows, macos and linux, each x86_64 and aarch64");
    for (platform, recipes) in &s.table {
        for r in recipes {
            assert_eq!(r.pin.sha256.len(), 64, "{platform:?} {}", r.name);
            assert!(r.pin.sha256.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{platform:?} {}", r.name);
            assert!(r.pin.size > 1_000_000 && r.pin.disk >= r.pin.size, "{platform:?} {}", r.name);
            assert!(r.file.contains("{v}") && r.program.starts_with(if r.kind == Kind::Git { "cmd/" } else { "lfs/" }), "{platform:?} {}", r.name);
        }
        let git = recipes.iter().filter(|r| r.kind == Kind::Git).count();
        assert_eq!(git, usize::from(platform.os == "windows"), "only Windows downloads git: {platform:?}");
        assert_eq!(recipes.iter().filter(|r| r.kind == Kind::Lfs).count(), 1);
    }
    // the first file of Windows x86-64 is MinGit at the pinned address
    let win = s.recipes(&Platform::new("windows", "x86_64")).unwrap();
    let p = s.pinned(&win[0]);
    assert_eq!(p.url, "https://github.com/git-for-windows/git/releases/download/v2.56.1.windows.1/MinGit-2.56.1-64-bit.zip");
    assert_eq!((p.version.as_str(), p.sha256.as_str()), ("2.56.1", "4a92850b6e917da4fd010669b614b55dccf5b0ec333a534cc4be4700dfefdafd"));
    let lfs = s.pinned(&s.recipes(&Platform::new("linux", "x86_64")).unwrap()[0]);
    assert_eq!(lfs.url, "https://github.com/git-lfs/git-lfs/releases/download/v3.8.1/git-lfs-linux-amd64-v3.8.1.tar.gz");
    assert!(s.recipes(&Platform::new("freebsd", "x86_64")).is_none());
    assert!(s.recipes(&Platform::new("linux", "riscv64")).is_none());
}

// spec: SYNC-029
#[test]
fn windows_downloads_mingit_with_git_lfs_and_only_git_lfs_when_that_is_missing() {
    let s = Sources::real();
    let win = Platform::new("windows", "aarch64");
    let o = s.offer(&win, have(false, false), None);
    let plan = o.download.unwrap();
    assert_eq!((plan.what(), plan.parts.len(), o.help.is_none()), ("git and git-lfs", 2, true));
    assert!(plan.parts[0].url.ends_with("MinGit-2.56.1-arm64.zip") && plan.parts[1].url.ends_with("git-lfs-windows-arm64-v3.8.1.zip"), "{:?}", plan.parts.iter().map(|p| &p.url).collect::<Vec<_>>());
    assert!(plan.download_bytes() > 40_000_000 && plan.disk_bytes() > 100_000_000);
    assert_eq!(plan.versions_text(), "git 2.56.1, git-lfs 3.8.1");
    let only_lfs = s.offer(&win, have(true, false), None).download.unwrap();
    assert_eq!((only_lfs.what(), only_lfs.parts.len()), ("git-lfs", 1));
    assert!(s.offer(&win, have(true, true), None).download.is_none(), "nothing is offered when both are there");
}

// spec: SYNC-029
#[test]
fn macos_opens_apples_installer_for_git_and_downloads_git_lfs() {
    let s = Sources::real();
    let mac = Platform::new("macos", "aarch64");
    let o = s.offer(&mac, have(false, false), None);
    assert_eq!((o.help, o.download.is_none()), (Some(GitHelp::Xcode), true), "no git: Apple's dialog, nothing is downloaded");
    let o = s.offer(&mac, have(true, false), None);
    assert!(o.help.is_none());
    assert_eq!(o.download.unwrap().parts[0].url, "https://github.com/git-lfs/git-lfs/releases/download/v3.8.1/git-lfs-darwin-arm64-v3.8.1.zip");
    assert!(s.offer(&mac, have(true, true), None).download.is_none());
    assert_eq!(super::install::xcode_select_command().get_args().collect::<Vec<_>>(), ["--install"]);
}

// spec: SYNC-029
#[test]
fn linux_shows_the_distributions_command_without_git_and_downloads_git_lfs_with_it() {
    let s = Sources::real();
    let linux = Platform::new("linux", "x86_64");
    let o = s.offer(&linux, have(false, false), Some("ID=debian\nNAME=\"Debian GNU/Linux\"\n"));
    assert!(o.download.is_none());
    assert_eq!(o.help, Some(GitHelp::Package(PackageHelp { distro: "Debian GNU/Linux".into(), command: "sudo apt install git git-lfs".into() })));
    assert_eq!(s.offer(&linux, have(false, false), Some("ID=plan9\n")).help, Some(GitHelp::Unknown));
    assert_eq!(s.offer(&linux, have(false, false), None).help, Some(GitHelp::Unknown));
    let o = s.offer(&linux, have(true, false), None);
    assert_eq!(o.download.unwrap().what(), "git-lfs");
    // a system without a pin says so and offers nothing
    let o = s.offer(&Platform::new("linux", "riscv64"), have(false, false), None);
    assert!(o.download.is_none() && o.help.is_none() && o.unsupported.unwrap().contains("riscv64"));
}

// spec: SYNC-029
#[test]
fn the_os_release_of_each_family_gives_its_package_manager() {
    let cmd = |text: &str| package_help(text).map(|p| p.command);
    assert_eq!(cmd("ID=ubuntu\nID_LIKE=debian\n").as_deref(), Some("sudo apt install git git-lfs"));
    assert_eq!(cmd("ID=linuxmint\nID_LIKE=\"ubuntu debian\"\n").as_deref(), Some("sudo apt install git git-lfs"), "by ID_LIKE when the ID is a derivative");
    assert_eq!(cmd("ID=fedora\n").as_deref(), Some("sudo dnf install git git-lfs"));
    assert_eq!(cmd("ID=\"rocky\"\nID_LIKE=\"rhel centos fedora\"\n").as_deref(), Some("sudo dnf install git git-lfs"));
    assert_eq!(cmd("ID=cachyos\nID_LIKE=arch\n").as_deref(), Some("sudo pacman -S git git-lfs"));
    assert_eq!(cmd("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\"\n").as_deref(), Some("sudo zypper install git git-lfs"));
    assert_eq!(cmd("ID=alpine\n").as_deref(), Some("sudo apk add git git-lfs"));
    assert_eq!(cmd("ID=nixos\n"), None, "a distribution that is not known has no command");
    assert_eq!(cmd(""), None);
    assert_eq!(package_help("ID=arch\nNAME='Arch Linux'\n").unwrap().distro, "Arch Linux");
    assert_eq!(package_help("ID=arch\n").unwrap().distro, "arch", "the ID when there is no NAME");
}

// spec: SYNC-027
#[test]
fn versions_and_tags_compare_by_their_numbers() {
    assert_eq!(version_of("v2.56.1.windows.1").as_deref(), Some("2.56.1"));
    assert_eq!(version_of("v3.8.1").as_deref(), Some("3.8.1"));
    assert_eq!(version_of("3.8").as_deref(), Some("3.8"));
    assert_eq!(version_of("nightly"), None);
    assert!(is_newer("v3.8.1", "v3.9.0") && is_newer("v3.8.1", "v3.8.2") && is_newer("v3.8.1", "v4.0.0"));
    assert!(is_newer("v2.56.1.windows.1", "v2.56.1.windows.2"), "a rebuild of the same version is newer");
    assert!(is_newer("v2.56.1.windows.1", "v2.56.2.windows.1"));
    assert!(!is_newer("v3.8.1", "v3.8.1") && !is_newer("v3.9.0", "v3.8.1"));
    assert!(is_newer("v3.9.9", "v3.10.0"), "3.10 is newer than 3.9.9, by numbers and not as text");
    assert!(!is_newer("v2.56.1.windows.2", "v2.56.1.windows.1"));
}

fn release(tag: &str, assets: &[(&str, u64, Option<&str>, &str)]) -> String {
    let a: Vec<serde_json::Value> = assets.iter().map(|(n, size, digest, url)| serde_json::json!({"name": n, "size": size, "digest": digest, "browser_download_url": url})).collect();
    serde_json::json!({"tag_name": tag, "assets": a}).to_string()
}

// spec: SYNC-027
#[test]
fn the_newest_release_is_read_from_the_api_answer_with_its_listed_checksum() {
    let s = Sources::real();
    let r = &s.recipes(&Platform::new("linux", "aarch64")).unwrap()[0];
    let sha = "ab".repeat(32);
    let url = "https://github.com/git-lfs/git-lfs/releases/download/v3.9.0/git-lfs-linux-arm64-v3.9.0.tar.gz";
    let ok = release("v3.9.0", &[("git-lfs-linux-amd64-v3.9.0.tar.gz", 1, Some("sha256:00"), "x"), ("git-lfs-linux-arm64-v3.9.0.tar.gz", 5_000_000, Some(&format!("sha256:{}", sha.to_uppercase())), url)]);
    let p = s.latest(r, &ok).unwrap();
    assert_eq!((p.tag.as_str(), p.version.as_str(), p.size, p.sha256.as_str(), p.url.as_str()), ("v3.9.0", "3.9.0", 5_000_000, sha.as_str(), url));
    // refused: no checksum, a malformed one, an address elsewhere, a missing file, no release
    let no_digest = release("v3.9.0", &[("git-lfs-linux-arm64-v3.9.0.tar.gz", 1, None, url)]);
    assert!(s.latest(r, &no_digest).unwrap_err().contains("lists no SHA-256"));
    let short = release("v3.9.0", &[("git-lfs-linux-arm64-v3.9.0.tar.gz", 1, Some("sha256:abcd"), url)]);
    assert!(s.latest(r, &short).unwrap_err().contains("lists no SHA-256"));
    let elsewhere = release("v3.9.0", &[("git-lfs-linux-arm64-v3.9.0.tar.gz", 1, Some(&format!("sha256:{sha}")), "https://evil.example/git-lfs.tar.gz")]);
    assert!(s.latest(r, &elsewhere).unwrap_err().contains("not a release download of git-lfs/git-lfs"));
    let other_project = release("v3.9.0", &[("git-lfs-linux-arm64-v3.9.0.tar.gz", 1, Some(&format!("sha256:{sha}")), "https://github.com/someone/else/releases/download/v3.9.0/git-lfs-linux-arm64-v3.9.0.tar.gz")]);
    assert!(s.latest(r, &other_project).is_err());
    let missing = release("v3.9.0", &[("git-lfs-linux-amd64-v3.9.0.tar.gz", 1, Some(&format!("sha256:{sha}")), url)]);
    assert!(s.latest(r, &missing).unwrap_err().contains("has no git-lfs-linux-arm64-v3.9.0.tar.gz"));
    assert!(s.latest(r, "{}").unwrap_err().contains("not a release"));
    assert!(s.latest(r, &release("nightly", &[])).unwrap_err().contains("no version"));
}
