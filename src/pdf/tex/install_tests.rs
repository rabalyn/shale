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

    /// What the pretend web server answers to a path.
    enum Reply {
        Body(Vec<u8>),
        Redirect(String),
        NotFound,
    }

    /// A web server on 127.0.0.1 that answers each request with `answer(path)`; the paths asked for
    /// are recorded. It lives until the test process ends. Gives `http://127.0.0.1:<port>/tlnet`.
    fn serve_with(answer: impl Fn(&str) -> Reply + Send + 'static) -> (String, Arc<Mutex<Vec<String>>>) {
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
                let (head, body) = match answer(&path) {
                    Reply::Body(b) => (format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", b.len()), b),
                    Reply::Redirect(to) => (format!("HTTP/1.0 302 Found\r\nLocation: {to}\r\nContent-Length: 0\r\n\r\n"), vec![]),
                    Reply::NotFound => ("HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string(), vec![]),
                };
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        (url, asked)
    }

    /// A pretend repository (a mirror): `body` at `/tlnet/<archive>`, a checksum file at
    /// `/tlnet/<MIRROR_PROBE>`, 404 for the rest.
    fn serve(body: Vec<u8>) -> (String, Arc<Mutex<Vec<String>>>) {
        serve_with(move |path| {
            if path == format!("/tlnet/{}", installer_archive()) {
                Reply::Body(body.clone())
            } else if path == format!("/tlnet/{MIRROR_PROBE}") {
                Reply::Body(b"0123abcd  texlive.tlpdb\n".to_vec())
            } else {
                Reply::NotFound
            }
        })
    }

    /// A pretend `mirror.ctan.org`: each request is sent on to the next of `mirrors` (in turn,
    /// starting again after the last), with the path below `/tlnet` kept.
    fn redirector(mirrors: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let next = AtomicU32::new(0);
        serve_with(move |path| {
            let i = next.fetch_add(1, Ordering::SeqCst) as usize % mirrors.len();
            Reply::Redirect(format!("{}{}", mirrors[i], path.strip_prefix("/tlnet").unwrap_or(path)))
        })
    }

    /// The paths a server was asked for.
    fn paths(asked: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
        asked.lock().unwrap().clone()
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

    /// The start of every pretend program: `R` is the root of the TeX tree it lives in (`bin/<platform>/..`), whatever the
    /// folder is called now (the partial folder while `install-tl` runs, `texlive` afterwards).
    const ROOT_SH: &str = "R=$(cd \"$(dirname \"$0\")/../..\" && pwd)\n";
    const ROOT: &str = "#!/bin/sh\nR=$(cd \"$(dirname \"$0\")/../..\" && pwd)\n";

    /// A pretend `lualatex`: records its run in `order`, makes a log and a PDF; fails with the error a font that cannot be
    /// loaded gives (#362) while the number in `fontfail` is above 0 (counting it down on every run).
    const LUALATEX_OK: &str = "echo lualatex >> $R/order\nfor a; do last=$a; done\nstem=${last%.tex}\nif [ -f $R/fontfail ] && [ $(cat $R/fontfail) -gt 0 ]; then\n  echo $(( $(cat $R/fontfail) - 1 )) > $R/fontfail\n  echo '! Font \\TU/lmr/m/n/10 not loadable: Metric (TFM) file or installed font not found.' > $stem.log\n  exit 1\nfi\necho ok > $stem.log\necho '%PDF-1.5 pretend' > $stem.pdf\n";

    /// A pretend `lualatex` that fails with an error that is no font problem.
    const LUALATEX_BROKEN: &str = "echo lualatex >> $R/order\nfor a; do last=$a; done\nstem=${last%.tex}\necho '! Undefined control sequence.' > $stem.log\nexit 1\n";

    /// A pretend `luaotfload-tool`: records its arguments in `order` and `luaotfload.calls`.
    const LUAOTFLOAD_OK: &str = "echo \"luaotfload-tool $*\" >> $R/order\necho \"$*\" >> $R/luaotfload.calls\n";

    /// A pretend `luaotfload-tool` that fails.
    const LUAOTFLOAD_BROKEN: &str = "echo \"luaotfload-tool $*\" >> $R/order\necho 'luaotfload | db : the database is broken' >&2\nexit 1\n";

    /// The perl of a pretend `install-tl`: reads TEXDIR and the platform from the profile, prints progress
    /// lines, makes `bin/<platform>/` with a `tlmgr` (records its arguments in `tlmgr.args` and every run in
    /// `tlmgr.calls`, prints progress), a `luaotfload-tool`, a `lualatex` and a `biber`. `middle` is perl run
    /// after the first progress line.
    fn install_tl(middle: &str, tlmgr: &str) -> String {
        install_tl_with(middle, tlmgr, LUALATEX_OK, LUAOTFLOAD_OK)
    }

    fn install_tl_with(middle: &str, tlmgr: &str, lualatex: &str, luaotfload: &str) -> String {
        format!(
            r##"use strict; use File::Path qw(make_path);
my ($profile, $repo, $dir, $plat);
for (my $i = 0; $i < @ARGV; $i++) {{ $profile = $ARGV[$i+1] if $ARGV[$i] eq '-profile'; $repo = $ARGV[$i+1] if $ARGV[$i] eq '-repository'; }}
open(my $p, '<', $profile) or die "no profile";
while (<$p>) {{ chomp; $dir = $1 if /^TEXDIR (.*)$/; $plat = $1 if /^binary_(\S+) 1/; }}
$| = 1;
print "[ 1/2, ??:??/??:??] install: amsfonts [10k]\n";
make_path($dir);
{middle}
print "[ 2/2, ??:??/??:??] install: babel [20k]\n";
sub wr {{ my ($f, $t) = @_; open(my $h, '>', $f) or die; print $h $t; close $h; chmod 0755, $f; }}
make_path("$dir/bin/$plat");
open(my $a, '>', "$dir/install.args") or die; print $a "@ARGV\n"; close $a;
wr("$dir/bin/$plat/tlmgr", q~{ROOT}echo "$@" > $R/tlmgr.args
echo "$@" >> $R/tlmgr.calls
{tlmgr}
~);
wr("$dir/bin/$plat/luaotfload-tool", q~{ROOT}{luaotfload}~);
wr("$dir/bin/$plat/lualatex", q~{ROOT}{lualatex}~);
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
        assert_eq!(done, Done { warning: None, mirrors: vec![repo.clone()] }, "the pretend TeX compiles the test document, and the one mirror is named");
        assert_eq!(paths(&asked), [format!("/tlnet/{MIRROR_PROBE}"), format!("/tlnet/{}", installer_archive())], "the mirror was resolved and the installer downloaded from it, nothing else");
        let root = managed_root(&data);
        assert!(root.join("bin").is_dir() && !partial_root(&data).exists(), "moved into place, no partial folder left");
        let args = std::fs::read_to_string(root.join("install.args")).unwrap();
        assert!(args.contains("-no-interaction") && args.contains(&format!("-repository {repo}")) && args.contains("-profile"), "install-tl got the profile and the repository: {args}");
        let tlmgr = std::fs::read_to_string(root.join("tlmgr.args")).unwrap();
        let expected: Vec<&str> = ["--repository", repo.as_str(), "install"].into_iter().chain(packages()).collect();
        assert_eq!(tlmgr.split_whitespace().collect::<Vec<_>>(), expected, "tlmgr installs the packages of the list");
        assert_eq!(handle.progress().stage, Stage::Fonts, "the last stage is the font names database and the test document");
        assert_eq!(order(&root), ["luaotfload-tool --update", "lualatex"], "the font names database is built before the test document is compiled");
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
        // no repository there: the mirror cannot be resolved
        let e = install(&env(&data, &format!("{repo}x")), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("could not be reached"), "{e}");
        assert_eq!(asked.lock().unwrap().len(), 1);
        assert!(!partial_root(&data).exists());
        // a repository without the installer
        let (repo, asked) = serve_with(|path| if path.ends_with(MIRROR_PROBE) { Reply::Body(b"x".to_vec()) } else { Reply::NotFound });
        let e = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("the download of") && e.contains("failed"), "{e}");
        assert_eq!(asked.lock().unwrap().len(), 2);
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

    /// What the pretend programs of the tree `root` ran, in order: `lualatex` and `luaotfload-tool <arguments>`.
    fn order(root: &Path) -> Vec<String> {
        std::fs::read_to_string(root.join("order")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// The runs of the pretend `tlmgr` (its arguments, one line each).
    fn tlmgr_calls(root: &Path) -> Vec<String> {
        std::fs::read_to_string(root.join("tlmgr.calls")).unwrap_or_default().lines().map(str::to_string).collect()
    }

    /// The command line of the repair of the fonts on `mirror` (#362).
    fn reinstall_line(mirror: &str) -> String {
        format!("--repository {mirror} install --reinstall {}", FONT_PACKAGES.join(" "))
    }

    /// `middle` of a pretend `install-tl` that makes the first `n` runs of `lualatex` fail with a font error.
    fn font_fails(n: u32) -> String {
        format!("open(my $f, '>', \"$dir/fontfail\") or die; print $f \"{n}\\n\"; close $f;")
    }

    // spec: TEX-002
    #[test]
    fn an_install_whose_test_document_fails_is_installed_with_a_warning_that_names_the_mirror() {
        let data = temp("warning");
        let (repo, _) = serve(installer("warning", &install_tl_with("", GOOD_TLMGR, LUALATEX_BROKEN, LUAOTFLOAD_OK)));
        let done = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap();
        let w = done.warning.expect("a warning");
        assert!(w.contains(&format!("installed from the mirror {repo}, but the test document did not compile")) && w.contains("Undefined control sequence"), "{w}");
        assert_eq!(done.mirrors, std::slice::from_ref(&repo));
        assert!(managed_root(&data).join("bin").is_dir(), "the TeX Live is there");
        assert_eq!(order(&managed_root(&data)), ["luaotfload-tool --update", "lualatex"], "a failure that is no font problem is not repaired: no more runs");
        assert_eq!(tlmgr_calls(&managed_root(&data)).len(), 1, "nothing was installed again");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn a_font_that_cannot_be_loaded_is_repaired_from_a_fresh_mirror_and_then_the_test_document_compiles() {
        let data = temp("font-repair");
        let body = installer("font-repair", &install_tl_with(&font_fails(1), GOOD_TLMGR, LUALATEX_OK, LUAOTFLOAD_OK));
        let (a, _) = serve(body.clone());
        let (b, asked_b) = serve(body);
        // the redirector sends the install to the first mirror and the repair to the second
        let (redirect, asked) = redirector(vec![a.clone(), b.clone()]);
        let done = install(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        assert_eq!(done, Done { warning: None, mirrors: vec![a.clone(), b.clone()] }, "no warning, and both mirrors are known");
        assert_eq!(done.source_text(), format!("from the mirrors {a} and {b}"));
        let root = managed_root(&data);
        assert_eq!(order(&root), ["luaotfload-tool --update", "lualatex", "luaotfload-tool --update --force", "lualatex"], "the database, the failed test document, the database again from scratch, the test document");
        assert_eq!(tlmgr_calls(&root).last().unwrap(), &reinstall_line(&b), "the font packages again, from the fresh mirror");
        assert_eq!(tlmgr_calls(&root).len(), 2);
        assert_eq!(paths(&asked).len(), 2, "the repository was resolved twice: for the install and for the repair");
        assert_eq!(paths(&asked_b), [format!("/tlnet/{MIRROR_PROBE}")], "the second mirror was only asked for the probe");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn the_font_repair_leaves_a_mirror_older_than_the_install_for_a_fresh_one_like_the_packages_do() {
        let data = temp("font-older");
        let (old, _) = serve(vec![]);
        let (new, _) = serve(vec![]);
        // tlmgr refuses to install anything again from the old mirror
        let tlmgr = format!("case \"$*\" in *{old}*--reinstall*) {OLDER} ;; esac\n{GOOD_TLMGR}");
        let (first, _) = serve(installer("font-older", &install_tl_with(&font_fails(1), &tlmgr, LUALATEX_OK, LUAOTFLOAD_OK)));
        let (redirect, _) = redirector(vec![first.clone(), old.clone(), new.clone()]);
        let done = install(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        assert_eq!(done, Done { warning: None, mirrors: vec![first, new.clone()] }, "the old mirror was left, and is not named: no file came from it");
        let calls = tlmgr_calls(&managed_root(&data));
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!((calls[1].clone(), calls[2].clone()), (reinstall_line(&old), reinstall_line(&new)));
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn a_font_that_stays_unloadable_after_the_repair_is_a_warning_naming_both_mirrors_and_the_install_stays() {
        let data = temp("font-stays");
        let body = installer("font-stays", &install_tl_with(&font_fails(99), GOOD_TLMGR, LUALATEX_OK, LUAOTFLOAD_OK));
        let (a, _) = serve(body.clone());
        let (b, _) = serve(body);
        let (redirect, _) = redirector(vec![a.clone(), b.clone()]);
        let done = install(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        let w = done.warning.expect("a warning");
        assert!(w.contains(&format!("installed from the mirrors {a} and {b}, but the test document did not compile")) && w.contains("not loadable") && w.contains("font names database"), "{w}");
        assert_eq!(order(&managed_root(&data)).iter().filter(|l| *l == "lualatex").count(), 2, "repaired once, not again and again");
        assert!(managed_root(&data).join("bin").is_dir() && !partial_root(&data).exists(), "the install is complete, only its test failed");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn a_repair_that_fails_and_a_luaotfload_tool_that_fails_are_warnings_never_an_install_failure() {
        let data = temp("repair-fails");
        // tlmgr installs the packages and refuses to install them again
        let tlmgr = format!("case \"$*\" in *--reinstall*) echo 'tlmgr: no network' >&2; exit 1 ;; esac\n{GOOD_TLMGR}");
        let (repo, _) = serve(installer("repair-fails", &install_tl_with(&font_fails(99), &tlmgr, LUALATEX_OK, LUAOTFLOAD_OK)));
        let done = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap();
        let w = done.warning.expect("a warning");
        assert!(w.contains("not loadable") && w.contains("could not be installed again") && w.contains("no network"), "{w}");
        assert_eq!(order(&managed_root(&data)), ["luaotfload-tool --update", "lualatex"], "no database rebuild after a failed reinstall");
        remove(&data).unwrap();
        // a luaotfload-tool that fails: the first LuaLaTeX run builds the database as well, so a test document that compiles is no warning
        let (repo, _) = serve(installer("tool-fails", &install_tl_with("", GOOD_TLMGR, LUALATEX_OK, LUAOTFLOAD_BROKEN)));
        let done = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap();
        assert_eq!(done.warning, None);
        assert_eq!(order(&managed_root(&data)), ["luaotfload-tool --update", "lualatex"]);
        remove(&data).unwrap();
        // … and when the test document fails too, what the tool said is part of the warning
        let (repo, _) = serve(installer("tool-fails2", &install_tl_with("", GOOD_TLMGR, LUALATEX_BROKEN, LUAOTFLOAD_BROKEN)));
        let w = install(&env(&data, &repo), &Handle::default(), &|| {}).unwrap().warning.expect("a warning");
        assert!(w.contains("Undefined control sequence") && w.contains("luaotfload-tool failed") && w.contains("the database is broken"), "{w}");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn the_font_packages_of_the_repair_are_in_the_package_list_and_in_the_script_of_the_test_image() {
        let list = packages();
        assert!(FONT_PACKAGES.iter().all(|p| list.contains(p)), "{FONT_PACKAGES:?}");
        let script = include_str!("../../../tests/tex-env/install-texlive.sh");
        assert!(script.contains(&format!("font_packages=\"{}\"", FONT_PACKAGES.join(" "))), "the image repairs the same packages");
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
        let (repo, _) = serve(vec![]);
        let done = update(&env(&data, &format!("{repo}/")), &handle, &|| {}).unwrap();
        assert_eq!(done.warning, None, "a TeX Live without luaotfload-tool (it could not be started) still tries the test document");
        assert_eq!(done.mirrors, std::slice::from_ref(&repo));
        assert_eq!(std::fs::read_to_string(f.dir.join("tlmgr.args")).unwrap().trim(), format!("--repository {repo} update --self --all"));
        assert_eq!(handle.progress().stage, Stage::Fonts);
        // not installed: nothing to update
        assert!(update(&env(&f.dir.join("empty"), "https://example.org/tlnet"), &Handle::default(), &|| {}).unwrap_err().contains("no TeX Live programs"));
        // a failing tlmgr
        script(&bin, "tlmgr", "echo 'tlmgr: remote repository is newer than local' >&2\nexit 1");
        let e = update(&env(&data, &repo), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("tlmgr failed") && e.contains("is newer than local"), "{e}");
    }

    // spec: TEX-003
    #[test]
    fn an_update_builds_the_font_database_first_and_repairs_a_font_that_cannot_be_loaded_like_an_install() {
        let f = Fake::new("update-fonts");
        let data = f.dir.join("data");
        let root = managed_root(&data);
        let bin = root.join("bin").join("x86_64-linux");
        let tools = |lualatex: &str| {
            script(&bin, "lualatex", &format!("{ROOT_SH}{lualatex}"));
            script(&bin, "luaotfload-tool", &format!("{ROOT_SH}{LUAOTFLOAD_OK}"));
            script(&bin, "biber", "exit 0");
            script(&bin, "tlmgr", &format!("{ROOT_SH}echo \"$@\" >> $R/tlmgr.calls\n{GOOD_TLMGR}"));
        };
        // the root of the pretend tree is `$R`: script() puts its own shebang first, so the prelude has no shebang here
        tools(LUALATEX_OK);
        let (a, _) = serve(vec![]);
        let (b, _) = serve(vec![]);
        let (redirect, _) = redirector(vec![a.clone(), b.clone()]);
        let done = update(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        assert_eq!(done, Done { warning: None, mirrors: vec![a.clone()] });
        assert_eq!(order(&root), ["luaotfload-tool --update", "lualatex"], "the database before the test document");
        // a font that cannot be loaded: repaired from the next mirror
        std::fs::remove_file(root.join("order")).unwrap();
        std::fs::write(root.join("fontfail"), "1\n").unwrap();
        let done = update(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        assert_eq!(done.warning, None);
        assert_eq!(done.mirrors.len(), 2, "{:?}", done.mirrors);
        assert_eq!(order(&root), ["luaotfload-tool --update", "lualatex", "luaotfload-tool --update --force", "lualatex"]);
        assert_eq!(tlmgr_calls(&root).last().unwrap(), &reinstall_line(done.mirrors.last().unwrap()));
        // a font that stays unloadable: a warning that says "updated" and names the mirrors
        std::fs::write(root.join("fontfail"), "99\n").unwrap();
        let w = update(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap().warning.expect("a warning");
        assert!(w.starts_with("TeX Live is updated from the mirror") && w.contains("but the test document did not compile") && w.contains("not loadable"), "{w}");
        // a failure that is no font problem is not repaired
        tools(LUALATEX_BROKEN);
        std::fs::remove_file(root.join("order")).unwrap();
        let before = tlmgr_calls(&root).len();
        let w = update(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap().warning.expect("a warning");
        assert!(w.contains("Undefined control sequence"), "{w}");
        assert_eq!(tlmgr_calls(&root).len(), before + 1, "only the update itself");
    }

    /// What tlmgr says on a mirror older than the installation (Forgejo run #361, #320).
    const OLDER: &str = "echo 'tlmgr: Remote database at the mirror' >&2\necho '(revision 81147 of the texlive-scripts package)' >&2\necho 'seems to be older than the local installation' >&2\necho '(revision 81161 of texlive-scripts);' >&2\necho 'please use a different mirror and/or wait a day or two.' >&2\nexit 1";

    // spec: TEX-002
    #[test]
    fn a_redirector_is_resolved_to_one_mirror_that_every_step_of_the_install_uses() {
        // mirror.ctan.org sends each request somewhere else: install-tl and tlmgr must not get two
        let data = temp("one-mirror");
        let body = installer("one-mirror", &install_tl("", GOOD_TLMGR));
        let (a, asked_a) = serve(body.clone());
        let (b, asked_b) = serve(body);
        let (redirect, asked_redirect) = redirector(vec![a.clone(), b]);
        assert_eq!(resolve_mirror(&Handle::default(), &a), Ok(a.clone()), "a repository that does not redirect is its own mirror");
        asked_a.lock().unwrap().clear();
        install(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        assert_eq!(paths(&asked_redirect), [format!("/tlnet/{MIRROR_PROBE}")], "the redirector is asked once");
        assert_eq!(paths(&asked_a), [format!("/tlnet/{MIRROR_PROBE}"), format!("/tlnet/{}", installer_archive())], "the installer comes from the mirror it chose");
        assert!(paths(&asked_b).is_empty(), "the other mirror is never used");
        let root = managed_root(&data);
        assert!(std::fs::read_to_string(root.join("install.args")).unwrap().trim_end().ends_with(&format!("-repository {a}")));
        assert!(std::fs::read_to_string(root.join("tlmgr.args")).unwrap().starts_with(&format!("--repository {a} install ")));
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-002
    #[test]
    fn a_mirror_older_than_the_install_is_left_for_a_fresh_one() {
        let data = temp("older-install");
        let (new, _) = serve(vec![]);
        let (old, _) = serve(vec![]);
        // tlmgr finds every mirror but the new one older than the installation
        let tlmgr = format!("case \"$*\" in *{new}*) ;; *) {OLDER} ;; esac\n{GOOD_TLMGR}");
        let (first, _) = serve(installer("older-install", &install_tl("", &tlmgr)));
        // the install gets the first mirror, tlmgr's retries the old one and then the new one
        let (redirect, asked) = redirector(vec![first.clone(), old, new.clone()]);
        install(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap();
        assert_eq!(paths(&asked).len(), MIRROR_TRIES, "the first mirror, then a fresh one for each refused tlmgr run");
        let root = managed_root(&data);
        assert!(std::fs::read_to_string(root.join("install.args")).unwrap().trim_end().ends_with(&format!("-repository {first}")), "install-tl ran on the first mirror");
        assert!(std::fs::read_to_string(root.join("tlmgr.args")).unwrap().starts_with(&format!("--repository {new} install ")), "tlmgr installed from the new mirror");
        let _ = std::fs::remove_dir_all(&data);
    }

    // spec: TEX-003
    #[test]
    fn an_update_on_an_older_mirror_tries_a_fresh_one_a_few_times_and_then_says_why() {
        let f = Fake::new("older-update");
        let data = f.dir.join("data");
        let bin = managed_root(&data).join("bin").join("x86_64-linux");
        script(&bin, "lualatex", "for a; do last=$a; done\nstem=${last%.tex}\necho ok > $stem.log\necho '%PDF' > $stem.pdf");
        script(&bin, "biber", "exit 0");
        let (new, _) = serve(vec![]);
        let (old, _) = serve(vec![]);
        let runs = f.dir.join("runs");
        script(&bin, "tlmgr", &format!("echo run >> '{}'\ncase \"$*\" in *{old}*) {OLDER} ;; esac\necho \"$@\" > '{}/tlmgr.args'", runs.display(), f.dir.display()));
        // the redirector sends the first request to the old mirror, the next to the new one
        let (redirect, asked) = redirector(vec![old.clone(), new.clone()]);
        assert_eq!(update(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap().warning, None);
        assert_eq!(paths(&asked).len(), 2);
        assert_eq!(std::fs::read_to_string(f.dir.join("tlmgr.args")).unwrap().trim(), format!("--repository {new} update --self --all"));
        assert_eq!(std::fs::read_to_string(&runs).unwrap().lines().count(), 2);
        // only old mirrors: MIRROR_TRIES runs, then tlmgr's words
        std::fs::remove_file(&runs).unwrap();
        let (redirect, asked) = redirector(vec![old.clone()]);
        let e = update(&env(&data, &redirect), &Handle::default(), &|| {}).unwrap_err();
        assert!(e.contains("tlmgr failed") && e.contains("older than the local installation") && e.contains("different mirror"), "{e}");
        assert_eq!(std::fs::read_to_string(&runs).unwrap().lines().count(), MIRROR_TRIES);
        assert_eq!(paths(&asked).len(), MIRROR_TRIES);
        // any other failure is not tried again
        std::fs::remove_file(&runs).unwrap();
        script(&bin, "tlmgr", &format!("echo run >> '{}'\necho 'tlmgr: no space left' >&2\nexit 1", runs.display()));
        assert!(update(&env(&data, &new), &Handle::default(), &|| {}).unwrap_err().contains("no space left"));
        assert_eq!(std::fs::read_to_string(&runs).unwrap().lines().count(), 1);
    }

    // spec: TEX-002
    #[test]
    fn a_redirect_that_does_not_end_at_a_repository_is_refused() {
        let (elsewhere, _) = serve_with(|_| Reply::Body(b"<html>".to_vec()));
        let (redirect, _) = serve_with(move |_| Reply::Redirect(format!("{elsewhere}/index.html")));
        let e = resolve_mirror(&Handle::default(), &redirect).unwrap_err();
        assert!(e.contains("which is no TeX Live repository"), "{e}");
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
    let mirrors = done.source_text();
    eprintln!("tex_install: installed in {:.0} s, {mirrors}", started.elapsed().as_secs_f32());
    assert_eq!(done.warning, None, "the test document compiled right after the install ({mirrors})");
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
    let out = super::compile::compile_test_document(e).unwrap_or_else(|e| panic!("{e}\n{}\n({mirrors})", e.log_tail));
    assert!(out.pdf.starts_with(b"%PDF") && out.runs >= 2, "{mirrors}");
    assert!(!out.font_cache_built, "the font cache was built by the install, not by this run ({mirrors})");
    // and it is removed
    remove(&data).unwrap();
    assert!(!root.exists());
    let _ = std::fs::remove_dir_all(&data);
}
