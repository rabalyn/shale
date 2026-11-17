//! The managed TeX Live (TEX-002, TEX-003): the profile, the progress lines, the guard of the
//! removal, and the whole install against a pretend repository (a local web server that serves a
//! pretend `install-tl`, Unix only); and one ignored test that really installs TeX Live from the
//! default repository (`cargo test -- --ignored tex_install`, needs the network, takes minutes).

use super::engine::{managed_root, partial_root};
use super::install::*;
use std::path::{Path, PathBuf};

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-tex-install-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// spec: TEX-002
#[test]
fn the_sizes_shown_before_a_download_are_the_measured_ones() {
    // ADR-0019: 94 MB of packages and 5.5 MB for the installer, 330 MB installed
    assert_eq!((DOWNLOAD_MB, DISK_MB), (100, 330));
}

// spec: TEX-002
#[test]
fn the_profile_moves_every_path_into_the_managed_folder_and_keeps_the_rest() {
    let root = Path::new("/home/u/.local/share/shale/texlive.partial");
    let p = profile(root, "aarch64-linux");
    // a folder with spaces (macOS: ~/Library/Application Support) is written as it is: the value of a profile line runs to its end
    let spaced = profile(Path::new("/Users/u/Library/Application Support/shale/texlive.partial"), "universal-darwin");
    assert!(spaced.lines().any(|l| l == "TEXDIR /Users/u/Library/Application Support/shale/texlive.partial"), "{spaced}");
    assert!(spaced.lines().any(|l| l == "TEXMFHOME /Users/u/Library/Application Support/shale/texlive.partial/home/texmf"), "{spaced}");
    assert!(!p.contains("/opt/texlive"), "no path of the test image is left:\n{p}");
    for key in ["TEXDIR", "TEXMFLOCAL", "TEXMFSYSCONFIG", "TEXMFSYSVAR", "TEXMFCONFIG", "TEXMFHOME", "TEXMFVAR"] {
        let line = p.lines().find(|l| l.starts_with(&format!("{key} "))).unwrap_or_else(|| panic!("{key} in the profile"));
        assert!(line[key.len() + 1..].starts_with("/home/u/.local/share/shale/texlive.partial"), "{line}");
    }
    assert!(p.lines().any(|l| l == "selected_scheme scheme-infraonly"));
    assert_eq!(p.lines().filter(|l| l.starts_with("binary_")).collect::<Vec<_>>(), ["binary_aarch64-linux 1"], "the binaries of this platform only");
    for l in ["tlpdbopt_install_docfiles 0", "tlpdbopt_install_srcfiles 0", "instopt_adjustpath 0", "tlpdbopt_desktop_integration 0", "tlpdbopt_file_assocs 0"] {
        assert!(p.lines().any(|x| x == l), "{l}: no docs, no sources, and nothing added to the system");
    }
    // a Windows path is written with forward slashes
    assert!(profile(Path::new("C:\\Users\\u\\AppData\\Roaming\\shale\\texlive.partial"), "windows").contains("TEXDIR C:/Users/u/AppData/Roaming/shale/texlive.partial"));
    assert_eq!(profile(root, "x86_64-linux").lines().count(), include_str!("../../../assets/tex/texlive.profile").lines().count(), "no line is added or lost");
}

// spec: TEX-002
#[test]
fn the_package_list_is_the_file_of_assets_tex_without_comments() {
    let list = packages();
    assert!(list.contains(&"luatex") && list.contains(&"biber") && list.contains(&"koma-script"));
    assert!(list.iter().all(|p| !p.contains('#') && !p.contains(' ')));
}

// spec: TEX-002
#[test]
fn only_https_and_http_repositories_are_accepted() {
    assert_eq!(check_repository(" https://mirror.ctan.org/systems/texlive/tlnet/ "), Ok("https://mirror.ctan.org/systems/texlive/tlnet"));
    assert_eq!(check_repository("http://127.0.0.1:8080/tlnet"), Ok("http://127.0.0.1:8080/tlnet"));
    for bad in ["", "ftp://x/tlnet", "file:///tmp/tlnet", "/tmp/tlnet", "https://", "https://a b/c", "https://a\n/c", "-o https://x", "http://-x"] {
        assert!(check_repository(bad).is_err(), "{bad:?}");
    }
}

// spec: TEX-002
#[test]
fn progress_lines_of_install_tl_and_tlmgr_give_a_count_and_a_name() {
    assert_eq!(parse_progress("[ 12/134, 00:03/00:20] install: koma-script [1234k]"), Some((12, 134, "koma-script".into())));
    assert_eq!(parse_progress("[1/52, ??:??/??:??] install: luatex.x86_64-linux [4096k]"), Some((1, 52, "luatex.x86_64-linux".into())));
    assert_eq!(parse_progress("[003/134] update: biber"), Some((3, 134, "biber".into())));
    // install-tl's own lines
    assert_eq!(parse_progress("Installing [3/3, time/total: 00:01/00:04]: texlive.infra.x86_64-linux [143k]"), Some((3, 3, "texlive.infra.x86_64-linux".into())));
    assert_eq!(parse_progress("  [ 2/9] 12many"), Some((2, 9, "12many".into())));
    assert_eq!(parse_progress("[ 2/9]"), None, "no name: nothing to show");
    for no in ["", "install: koma-script", "Installing: texlive.infra", "Time used for installing the packages: 00:02", "[x/y] install: a", "[12] install: a", "Installing: [1/2] a", "[ ??/?? ] install: a"] {
        assert_eq!(parse_progress(no), None, "{no:?}");
    }
}

// spec: TEX-002
#[test]
fn the_progress_bar_runs_from_nothing_to_all_through_the_stages() {
    let at = |stage, done, total| Progress { stage, done, total, package: String::new() }.fraction();
    let marks = [
        at(Stage::Starting, 0, 0),
        at(Stage::Download, 0, 0),
        at(Stage::Unpack, 0, 0),
        at(Stage::Base, 0, 80),
        at(Stage::Base, 40, 80),
        at(Stage::Base, 80, 80),
        at(Stage::Packages, 1, 50),
        at(Stage::Packages, 25, 50),
        at(Stage::Packages, 50, 50),
        at(Stage::Fonts, 0, 0),
        at(Stage::Fonts, 1, 1),
    ];
    assert!(marks.windows(2).all(|w| w[0] <= w[1]), "never goes back: {marks:?}");
    assert_eq!((marks[0], *marks.last().unwrap()), (0.0, 1.0));
    assert_eq!(at(Stage::Update, 5, 10), 0.5);
    assert_eq!(at(Stage::Packages, 99, 50), at(Stage::Packages, 50, 50), "a count past the total does not overshoot");
    let p = Progress { stage: Stage::Packages, done: 31, total: 52, package: "koma-script".into() };
    assert_eq!(p.text(), "Installing the packages: koma-script (31 of 52)");
    assert_eq!(Progress { stage: Stage::Download, ..Progress::default() }.text(), "Downloading the installer");
    assert!(Progress { stage: Stage::Fonts, ..Progress::default() }.text().contains("font cache"));
    assert_eq!(Progress { stage: Stage::Base, done: 2, total: 9, package: String::new() }.text(), "Installing the base of TeX Live (2 of 9)");
}

// spec: TEX-002
#[test]
fn a_missing_perl_is_said_clearly() {
    let e = check_perl_program("shale-no-such-perl").unwrap_err();
    assert!(e.contains("install perl first"), "{e}");
}

// spec: TEX-003
#[test]
fn only_the_exact_managed_folder_may_be_removed() {
    let data = temp("guard");
    let root = managed_root(&data);
    std::fs::create_dir_all(root.join("bin")).unwrap();
    assert_eq!(removable(&data, &root), Ok(()));
    assert!(removable(&data, &partial_root(&data)).is_err(), "not there");
    for other in [data.clone(), data.join("shale"), data.join("shale").join("mail"), root.join("bin"), data.join("shale").join("texlive2"), PathBuf::from("/"), PathBuf::from("/home"), root.join("..")] {
        let why = removable(&data, &other).unwrap_err();
        assert!(why.contains("not the folder of the managed TeX Live"), "{other:?}: {why}");
    }
    // a file in its place
    let other_data = temp("guard-file");
    std::fs::create_dir_all(other_data.join("shale")).unwrap();
    std::fs::write(managed_root(&other_data), "x").unwrap();
    assert!(removable(&other_data, &managed_root(&other_data)).unwrap_err().contains("not a plain folder"));
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&other_data);
}

#[cfg(unix)]
mod fake {
    use super::*;
    use crate::pdf::tex::compile_tests::fake::{script, Fake};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // spec: TEX-003
    #[test]
    fn a_link_is_never_followed_out_of_the_data_folder() {
        let data = temp("links");
        let outside = temp("links-outside");
        std::fs::write(outside.join("precious.txt"), "keep me").unwrap();
        // the folder itself is a link
        std::fs::create_dir_all(data.join("shale")).unwrap();
        symlink(&outside, managed_root(&data)).unwrap();
        let why = remove(&data).unwrap_err();
        assert!(why.contains("not a plain folder"), "{why}");
        assert!(outside.join("precious.txt").is_file(), "nothing outside was touched");
        std::fs::remove_file(managed_root(&data)).unwrap();
        // a link in the place of <data>/shale
        std::fs::remove_dir_all(data.join("shale")).unwrap();
        std::fs::create_dir_all(outside.join("texlive")).unwrap();
        symlink(&outside, data.join("shale")).unwrap();
        assert!(remove(&data).is_err());
        assert!(outside.join("texlive").is_dir(), "the link to the folder was not followed");
        // a link inside the tree: removed as a link, its target stays
        std::fs::remove_file(data.join("shale")).unwrap();
        std::fs::create_dir_all(managed_root(&data).join("bin")).unwrap();
        symlink(&outside, managed_root(&data).join("bin").join("linked")).unwrap();
        remove(&data).unwrap();
        assert!(outside.join("precious.txt").is_file(), "the target of a link inside the tree stays");
        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&outside);
    }

    // spec: TEX-003
    #[test]
    fn remove_deletes_the_managed_folder_and_an_old_partial_one_and_nothing_else() {
        let data = temp("remove");
        std::fs::create_dir_all(managed_root(&data).join("texmf-dist")).unwrap();
        std::fs::write(managed_root(&data).join("texmf-dist").join("a.sty"), "x").unwrap();
        std::fs::create_dir_all(partial_root(&data)).unwrap();
        std::fs::create_dir_all(data.join("shale").join("mail")).unwrap();
        std::fs::write(data.join("shale").join("mail").join("keep.txt"), "mail").unwrap();
        std::fs::write(data.join("shale").join("config.json"), "{}").unwrap();
        remove(&data).unwrap();
        assert!(!managed_root(&data).exists() && !partial_root(&data).exists());
        assert!(data.join("shale").join("mail").join("keep.txt").is_file() && data.join("shale").join("config.json").is_file(), "everything else stays");
        assert!(remove(&data).unwrap_err().contains("not a plain folder"), "nothing to remove is said, not hidden");
        let _ = std::fs::remove_dir_all(&data);
    }

    // ---------------------------------------------------------------- a pretend repository

    /// A web server on 127.0.0.1 that serves `body` at `/tlnet/<archive>` and says 404 for the rest;
    /// the paths asked for are recorded. It lives until the test process ends.
    fn serve(body: Vec<u8>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}/tlnet", listener.local_addr().unwrap().port());
        let asked = Arc::new(Mutex::new(vec![]));
        let log = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let first = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
                let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
                log.lock().unwrap().push(path.clone());
                let ok = path == format!("/tlnet/{}", installer_archive());
                let head = if ok { format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()) } else { "HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string() };
                let _ = stream.write_all(head.as_bytes());
                if ok {
                    let _ = stream.write_all(&body);
                }
            }
        });
        (url, asked)
    }

    /// The archive of a pretend installer: a folder with the perl script `install-tl`.
    fn installer(tag: &str, perl: &str) -> Vec<u8> {
        let dir = temp(&format!("archive-{tag}"));
        let inner = dir.join("install-tl-20260101");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("install-tl"), perl).unwrap();
        let out = dir.join("a.tar.gz");
        let ok = std::process::Command::new("tar").arg("czf").arg(&out).arg("-C").arg(&dir).arg("install-tl-20260101").status().unwrap().success();
        assert!(ok, "tar made the archive");
        let bytes = std::fs::read(&out).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        bytes
    }

    /// The perl of a pretend `install-tl`: reads TEXDIR and the platform from the profile, prints progress
    /// lines, makes `bin/<platform>/` with a `tlmgr` (records its arguments, prints progress) and a
    /// `lualatex` and `biber` that work. `middle` is perl run after the first progress line.
    fn install_tl(middle: &str, tlmgr: &str) -> String {
        format!(
            r##"use strict; use File::Path qw(make_path);
my ($profile, $repo, $dir, $plat);
for (my $i = 0; $i < @ARGV; $i++) {{ $profile = $ARGV[$i+1] if $ARGV[$i] eq '-profile'; $repo = $ARGV[$i+1] if $ARGV[$i] eq '-repository'; }}
open(my $p, '<', $profile) or die "no profile";
while (<$p>) {{ chomp; $dir = $1 if /^TEXDIR (.*)$/; $plat = $1 if /^binary_(\S+) 1/; }}
$| = 1;
print "[ 1/2, ??:??/??:??] install: amsfonts [10k]\n";
{middle}
print "[ 2/2, ??:??/??:??] install: babel [20k]\n";
sub wr {{ my ($f, $t) = @_; open(my $h, '>', $f) or die; print $h $t; close $h; chmod 0755, $f; }}
make_path("$dir/bin/$plat");
open(my $a, '>', "$dir/install.args") or die; print $a "@ARGV\n"; close $a;
wr("$dir/bin/$plat/tlmgr", "#!/bin/sh\necho \"\$@\" > '$dir/tlmgr.args'\n" . q~{tlmgr}~ . "\n");
wr("$dir/bin/$plat/lualatex", "#!/bin/sh\nfor a; do last=\$a; done\nstem=\${{last%.tex}}\necho ok > \$stem.log\necho '%PDF-1.5 pretend' > \$stem.pdf\n");
wr("$dir/bin/$plat/biber", "#!/bin/sh\nexit 0\n");
"##
        )
    }

    const GOOD_TLMGR: &str = "echo '[1/2, ??:??/??:??] install: koma-script [1k]'\necho '[2/2, ??:??/??:??] install: biber [1k]'";

    fn env(data: &Path, repository: &str) -> Env {
        Env { data: data.to_path_buf(), repository: repository.to_string() }
    }

    // spec: TEX-002
    #[test]
    fn an_install_downloads_unpacks_installs_the_list_moves_the_folder_into_place_and_tries_it() {
        let data = temp("full");
        let (repo, asked) = serve(installer("full", &install_tl("", GOOD_TLMGR)));
        let handle = Handle::default();
        let notified = AtomicU32::new(0);
        let done = install(&env(&data, &repo), &handle, &|| {
            notified.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        assert_eq!(done, Done { warning: None }, "the pretend TeX compiles the test document");
        assert_eq!(asked.lock().unwrap().as_slice(), [format!("/tlnet/{}", installer_archive())], "the installer was downloaded from the repository, nothing else");
        let root = managed_root(&data);
        assert!(root.join("bin").is_dir() && !partial_root(&data).exists(), "moved into place, no partial folder left");
        let args = std::fs::read_to_string(root.join("install.args")).unwrap();
        assert!(args.contains("-no-interaction") && args.contains(&format!("-repository {repo}")) && args.contains("-profile"), "install-tl got the profile and the repository: {args}");
        let tlmgr = std::fs::read_to_string(root.join("tlmgr.args")).unwrap();
        let expected: Vec<&str> = ["--repository", repo.as_str(), "install"].into_iter().chain(packages()).collect();
        assert_eq!(tlmgr.split_whitespace().collect::<Vec<_>>(), expected, "tlmgr installs the packages of the list");
        assert_eq!(handle.progress().stage, Stage::Fonts, "the last stage is the first LuaLaTeX run");
        assert!(notified.load(Ordering::SeqCst) >= 4, "the window was told about the progress");
        assert!(root.read_dir().unwrap().all(|e| e.unwrap().file_name() != "texlive.profile"), "the profile stays in the temporary folder");
        // a second install refuses: remove it first
        assert!(install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap_err().contains("is there already"));
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn progress_lines_reach_the_handle_while_the_installer_runs() {
        let data = temp("progress");
        let (repo, _) = serve(installer("progress", &install_tl("print \"[ 1/2, ??:??/??:??] install: amsfonts [10k]\\n\"; sleep 2;", GOOD_TLMGR)));
        let handle = Handle::default();
        let h = handle.clone();
        let env = env(&data, &repo);
        let job = std::thread::spawn(move || install(&env, &h, &|| {}));
        let mut seen = None;
        for _ in 0..200 {
            let p = handle.progress();
            if p.stage == Stage::Base && p.done == 1 {
                seen = Some(p);
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(seen, Some(Progress { stage: Stage::Base, done: 1, total: 2, package: "amsfonts".into() }));
        assert!(job.join().unwrap().is_ok());
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn cancel_kills_the_installer_and_removes_the_partial_folder() {
        let data = temp("cancel");
        let (repo, _) = serve(installer("cancel", &install_tl("sleep 60;", GOOD_TLMGR)));
        let handle = Handle::default();
        let h = handle.clone();
        let env = env(&data, &repo);
        let started = std::time::Instant::now();
        let job = std::thread::spawn(move || install(&env, &h, &|| {}));
        for _ in 0..200 {
            if handle.progress().stage == Stage::Base && handle.progress().done == 1 {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(handle.progress().stage, Stage::Base, "the installer runs");
        handle.cancel();
        assert_eq!(job.join().unwrap().unwrap_err(), "cancelled");
        assert!(started.elapsed() < Duration::from_secs(30), "the sleeping installer was killed, not waited for");
        assert!(!partial_root(&data).exists() && !managed_root(&data).exists(), "nothing of the install is left");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn a_failed_installer_or_tlmgr_leaves_nothing_and_says_what_it_printed() {
        let data = temp("fail");
        let (repo, _) = serve(installer("fail", &install_tl("print STDERR \"disk is full\\n\"; exit 3;", GOOD_TLMGR)));
        let e = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("install-tl failed (exit code 3)") && e.contains("disk is full"), "{e}");
        assert!(!partial_root(&data).exists() && !managed_root(&data).exists());
        let (repo, _) = serve(installer("fail2", &install_tl("", "echo 'tlmgr: package repository is down' >&2\nexit 1")));
        let e = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("tlmgr failed") && e.contains("repository is down"), "{e}");
        assert!(!partial_root(&data).exists() && !managed_root(&data).exists(), "a failed tlmgr leaves no half-installed folder");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn a_failed_download_or_a_bad_address_stops_before_anything_is_installed() {
        let data = temp("download");
        let (repo, asked) = serve(vec![]);
        let e = install(&env(&data, &format!("{repo}x")), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("the download of") && e.contains("failed"), "{e}");
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert!(!partial_root(&data).exists());
        let e = install(&env(&data, "not a url"), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("is not a repository address"), "{e}");
        // an archive that is no archive
        let (repo, _) = serve(b"not a tar file".to_vec());
        let e = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("could not be unpacked"), "{e}");
        assert!(!partial_root(&data).exists() && !managed_root(&data).exists());
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn an_install_whose_test_document_fails_is_installed_with_a_warning() {
        let data = temp("warning");
        let broken = install_tl("", GOOD_TLMGR).replace("echo ok > \\$stem.log\\necho '%PDF-1.5 pretend' > \\$stem.pdf\\n", "exit 1\\n");
        let (repo, _) = serve(installer("warning", &broken));
        let done = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap();
        let w = done.warning.expect("a warning");
        assert!(w.contains("installed, but the test document did not compile"), "{w}");
        assert!(managed_root(&data).join("bin").is_dir(), "the TeX Live is there");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-003
    #[test]
    fn update_runs_tlmgr_update_self_all_with_progress_and_tries_the_result() {
        let f = Fake::new("update");
        let data = f.dir.join("data");
        let bin = managed_root(&data).join("bin").join("x86_64-linux");
        script(&bin, "lualatex", "for a; do last=$a; done\nstem=${last%.tex}\necho ok > $stem.log\necho '%PDF' > $stem.pdf");
        script(&bin, "biber", "exit 0");
        script(&bin, "tlmgr", &format!("echo \"$@\" > '{}/tlmgr.args'\necho '[1/3, ??:??/??:??] update: pdftex [5k]'\necho '[2/3, ??:??/??:??] update: biber [5k]'", f.dir.display()));
        let handle = Handle::default();
        let done = update(&env(&data, "https://example.org/tlnet/"), &handle, &|| {}).unwrap();
        assert_eq!(done.warning, None);
        assert_eq!(std::fs::read_to_string(f.dir.join("tlmgr.args")).unwrap().trim(), "--repository https://example.org/tlnet update --self --all");
        assert_eq!(handle.progress().stage, Stage::Fonts);
        // not installed: nothing to update
        assert!(update(&env(&f.dir.join("empty"), "https://example.org/tlnet"), &Handle::default(), &|| {}).unwrap_err().contains("no TeX Live programs"));
        // a failing tlmgr
        script(&bin, "tlmgr", "echo 'tlmgr: remote repository is newer than local' >&2\nexit 1");
        let e = update(&env(&data, "https://example.org/tlnet"), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("tlmgr failed") && e.contains("is newer than local"), "{e}");
    }
}

// ---------------------------------------------------------------- the real install

// spec: TEX-002, TEX-003, TEX-004
#[test]
#[ignore = "needs the network and takes minutes: installs the current TeX Live from the default repository"]
fn tex_install_the_managed_tex_live_installs_is_found_compiles_and_is_removed() {
    // a space in the path: the default on macOS (~/Library/Application Support)
    let data = temp("real tex test");
    // the default repository, what a person gets; SHALE_TEX_INSTALL_REPOSITORY tries another one
    let repository = std::env::var("SHALE_TEX_INSTALL_REPOSITORY").unwrap_or_else(|_| super::engine::DEFAULT_REPOSITORY.to_string());
    let env = Env { data: data.clone(), repository };
    let handle = Handle::default();
    let started = std::time::Instant::now();
    let h = handle.clone();
    let done = install(&env, &h, &|| {}).unwrap_or_else(|e| panic!("the install failed: {e}"));
    eprintln!("tex_install: installed in {:.0} s", started.elapsed().as_secs_f32());
    assert_eq!(done.warning, None, "the test document compiled right after the install");
    let root = managed_root(&data);
    let size: u64 = {
        fn walk(d: &Path) -> u64 {
            std::fs::read_dir(d)
                .unwrap()
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&e.path()),
                    Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
                    _ => 0,
                })
                .sum()
        }
        walk(&root)
    };
    eprintln!("tex_install: {} MB on disk", size / 1_000_000);
    assert!(!partial_root(&data).exists());
    // found as the managed TeX, with every package of the list
    let d = super::engine::detect(Some(&data), Some("/nonexistent".as_ref()));
    let e = d.managed.as_ref().unwrap_or_else(|e| panic!("detection: {e}"));
    assert!(e.missing.is_empty(), "missing: {:?}", e.missing);
    assert!(d.selected(super::engine::TexChoice::Auto).is_ok());
    // a small document compiles (the install already ran the test document once; this is the fast second run)
    let out = super::compile::compile_test_document(e).unwrap_or_else(|e| panic!("{e}\n{}", e.log_tail));
    assert!(out.pdf.starts_with(b"%PDF") && out.runs >= 2);
    assert!(!out.font_cache_built, "the font cache was built by the install, not by this run");
    // and it is removed
    remove(&data).unwrap();
    assert!(!root.exists());
    let _ = std::fs::remove_dir_all(&data);
}
