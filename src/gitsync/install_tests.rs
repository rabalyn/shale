//! The managed git's install, update and removal (SYNC-025, SYNC-027, SYNC-028) against a pretend
//! GitHub on 127.0.0.1 that serves fake archives (a zip with `cmd/git.exe`, a zip or tar.gz with
//! `git-lfs`, which are shell scripts here), run on Linux for the Windows, macOS and Linux
//! recipes alike. The one real install is ignored unless asked for.

use super::install::*;
use super::managed::*;
use super::sources::*;
use crate::pdf::tex::archive::sha256_hex;
use crate::pdf::tex::archive_tests::{tar_entry, tar_gz, z, zip};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn temp(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-gitinstall-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---------------------------------------------------------------- a pretend GitHub

/// What the pretend server knows: path to body. A body marked slow is announced as long and then
/// not sent, so a download of it hangs until it is cancelled.
type Served = std::collections::HashMap<String, (Vec<u8>, bool)>;

/// A request the server got: its path and its `Accept` header.
type Asked = (String, Option<String>);

#[derive(Clone, Default)]
pub(super) struct Files {
    map: Arc<Mutex<Served>>,
    pub asked: Arc<Mutex<Vec<Asked>>>,
}

impl Files {
    pub fn put(&self, path: &str, body: Vec<u8>) {
        self.map.lock().unwrap().insert(path.to_string(), (body, false));
    }

    pub fn map_clear(&self) {
        self.map.lock().unwrap().clear();
    }

    pub fn hang(&self, path: &str) {
        self.map.lock().unwrap().insert(path.to_string(), (vec![0; 1 << 20], true));
    }

    pub fn paths(&self) -> Vec<String> {
        self.asked.lock().unwrap().iter().map(|(p, _)| p.clone()).collect()
    }
}

/// A web server on 127.0.0.1 answering from `files` (404 for the rest); it lives until the test process ends.
pub(super) fn serve(files: &Files) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
    let files = files.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let files = files.clone();
            std::thread::spawn(move || {
                let mut stream = stream;
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.lines().next().unwrap_or("").split_whitespace().nth(1).unwrap_or("").to_string();
                let accept = request.lines().find_map(|l| l.strip_prefix("accept: ").or_else(|| l.strip_prefix("Accept: "))).map(str::to_string);
                files.asked.lock().unwrap().push((path.clone(), accept));
                let found = files.map.lock().unwrap().get(&path).cloned();
                match found {
                    Some((body, true)) => {
                        let _ = stream.write_all(format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", body.len() + 1_000_000).as_bytes());
                        let _ = stream.write_all(&body[..1000]);
                        std::thread::sleep(Duration::from_secs(30));
                    }
                    Some((body, false)) => {
                        let _ = stream.write_all(format!("HTTP/1.0 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes());
                        let _ = stream.write_all(&body);
                    }
                    None => {
                        let _ = stream.write_all(b"HTTP/1.0 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    }
                }
            });
        }
    });
    base
}

// ---------------------------------------------------------------- fake archives and sources

pub(super) fn script_text(output: &str) -> Vec<u8> {
    format!("#!/bin/sh\necho '{output}'\n").into_bytes()
}

/// A fake MinGit: a zip with `cmd/git.exe`, which answers `--version`.
pub(super) fn mingit_zip(version: &str) -> Vec<u8> {
    zip(&[z("cmd/git.exe", &script_text(&format!("git version {version}.windows.1"))), z("etc/gitconfig", b"[core]\n")])
}

/// A fake git-lfs zip (Windows, macOS): the program in a top folder.
pub(super) fn lfs_zip(version: &str, program: &str) -> Vec<u8> {
    zip(&[z(&format!("git-lfs-{version}/"), b""), z(&format!("git-lfs-{version}/{program}"), &script_text(&format!("git-lfs/{version} (fake)"))), z(&format!("git-lfs-{version}/README.md"), b"readme")])
}

/// A fake git-lfs tar.gz (Linux).
pub(super) fn lfs_tgz(version: &str) -> Vec<u8> {
    tar_gz(&[tar_entry(&format!("git-lfs-{version}/"), b'5', b""), tar_entry(&format!("git-lfs-{version}/git-lfs"), b'0', &script_text(&format!("git-lfs/{version} (fake)")))])
}

fn pin(tag: &str, version: &str, bytes: &[u8]) -> Pin {
    Pin { tag: tag.into(), version: version.into(), size: bytes.len() as u64, disk: bytes.len() as u64 * 2, sha256: sha256_hex(bytes) }
}

pub(super) fn mingit_recipe(version: &str, bytes: &[u8]) -> Recipe {
    Recipe {
        kind: Kind::Git,
        name: "MinGit".into(),
        repo: "git-for-windows/git".into(),
        file: "MinGit-{v}-64-bit.zip".into(),
        into: String::new(),
        strip_root: false,
        program: "cmd/git.exe".into(),
        path: vec!["cmd".into()],
        pin: pin(&format!("v{version}.windows.1"), version, bytes),
    }
}

pub(super) fn lfs_recipe(version: &str, file: &str, program: &str, bytes: &[u8]) -> Recipe {
    Recipe {
        kind: Kind::Lfs,
        name: "git-lfs".into(),
        repo: "git-lfs/git-lfs".into(),
        file: file.into(),
        into: "lfs".into(),
        strip_root: true,
        program: format!("lfs/{program}"),
        path: vec!["lfs".into()],
        pin: pin(&format!("v{version}"), version, bytes),
    }
}

/// Put the files of a recipe at the address a pinned install asks for.
pub(super) fn publish(files: &Files, r: &Recipe, bytes: &[u8]) {
    files.put(&format!("/{}/releases/download/{}/{}", r.repo, r.pin.tag, r.file.replace("{v}", &r.pin.version)), bytes.to_vec());
}

/// A pretend world: a server, a data folder and a table of three platforms (Windows: MinGit and
/// git-lfs as zips, macOS: git-lfs as a zip, Linux: git-lfs as a tar.gz), at the versions given.
pub(super) struct World {
    pub data: PathBuf,
    pub files: Files,
    pub sources: Sources,
}

impl World {
    pub fn new(tag: &str, git: &str, lfs: &str) -> World {
        let files = Files::default();
        let base = serve(&files);
        let (g, lz, lt) = (mingit_zip(git), lfs_zip(lfs, "git-lfs.exe"), lfs_tgz(lfs));
        let mac = lfs_zip(lfs, "git-lfs");
        let (rg, rw, rm, rl) = (
            mingit_recipe(git, &g),
            lfs_recipe(lfs, "git-lfs-windows-amd64-v{v}.zip", "git-lfs.exe", &lz),
            lfs_recipe(lfs, "git-lfs-darwin-arm64-v{v}.zip", "git-lfs", &mac),
            lfs_recipe(lfs, "git-lfs-linux-amd64-v{v}.tar.gz", "git-lfs", &lt),
        );
        for (r, b) in [(&rg, &g), (&rw, &lz), (&rm, &mac), (&rl, &lt)] {
            publish(&files, r, b);
        }
        let table = vec![(Platform::new("windows", "x86_64"), vec![rg, rw]), (Platform::new("macos", "aarch64"), vec![rm]), (Platform::new("linux", "x86_64"), vec![rl])];
        World { data: temp(tag), files, sources: Sources { download_base: base.clone(), api_base: base, allow_http: true, table } }
    }

    pub fn plan(&self, os: &str, arch: &str) -> Plan {
        let recipes = self.sources.recipes(&Platform::new(os, arch)).unwrap();
        self.sources.pinned_plan(&recipes.iter().collect::<Vec<_>>())
    }

    pub fn env(&self) -> Env {
        Env { data: self.data.clone() }
    }

    /// Nothing is left but the managed folder (or nothing).
    pub fn leftovers(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(self.data.join("shale")).map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
        v.sort();
        v
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.data);
    }
}

fn quiet() {}

// spec: SYNC-025
#[test]
fn windows_installs_mingit_and_git_lfs_and_the_selected_git_runs_them() {
    let w = World::new("windows", "2.99.0", "9.9.9");
    let handle = Handle::default();
    let done = install(&w.env(), &w.plan("windows", "x86_64"), &handle, &quiet).unwrap();
    let root = managed_root(&w.data);
    assert_eq!(done.installed.versions_text(), "git 2.99.0, git-lfs 9.9.9");
    assert_eq!(Installed::read(&root), Some(done.installed.clone()), "what the manifest on disk says");
    assert!(root.join("cmd/git.exe").is_file() && root.join("etc/gitconfig").is_file() && root.join("lfs/git-lfs.exe").is_file() && root.join("lfs/README.md").is_file(), "the top folder of git-lfs is left out, MinGit is at the top");
    assert!(!root.join("lfs/git-lfs-9.9.9").exists());
    assert_eq!(w.leftovers(), ["git"], "no partial folder, no unpack folder");
    assert_eq!(handle.progress().fraction, 0.98, "the last thing it said was putting it in place");
    // it runs from there
    let git = selected_git(GitChoice::Managed, Some(&w.data), None);
    assert_eq!(git.version().as_deref(), Some("git version 2.99.0.windows.1"));
    // the server was asked for the two files and for nothing else
    assert_eq!(w.files.paths(), ["/git-for-windows/git/releases/download/v2.99.0.windows.1/MinGit-2.99.0-64-bit.zip", "/git-lfs/git-lfs/releases/download/v9.9.9/git-lfs-windows-amd64-v9.9.9.zip"]);
}

// spec: SYNC-025
#[test]
fn macos_and_linux_install_git_lfs_alone_from_a_zip_and_a_tar_gz_and_it_can_be_run() {
    for (os, arch) in [("macos", "aarch64"), ("linux", "x86_64")] {
        let w = World::new(&format!("lfs-{os}"), "2.99.0", "9.9.9");
        let done = install(&w.env(), &w.plan(os, arch), &Handle::default(), &quiet).unwrap();
        assert_eq!((done.installed.git.is_none(), done.installed.versions_text().as_str()), (true, "git-lfs 9.9.9"), "{os}");
        let program = managed_root(&w.data).join("lfs/git-lfs");
        assert!(program.is_file(), "{os}");
        // the program has its execute permission although the archive reader writes plain files
        let out = std::process::Command::new(&program).arg("version").output().unwrap();
        assert!(String::from_utf8_lossy(&out.stdout).contains("git-lfs/9.9.9"), "{os}");
        assert_eq!(w.leftovers(), ["git"], "{os}");
    }
}

// spec: SYNC-025
#[test]
fn a_download_with_another_checksum_is_refused_and_nothing_is_left() {
    let w = World::new("sum", "2.99.0", "9.9.9");
    let plan = w.plan("windows", "x86_64");
    // the server hands out other bytes for git-lfs than the ones that are pinned
    w.files.put("/git-lfs/git-lfs/releases/download/v9.9.9/git-lfs-windows-amd64-v9.9.9.zip", lfs_zip("9.9.9", "git-lfs.exe").into_iter().chain([0]).collect());
    let e = install(&w.env(), &plan, &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("git-lfs") && e.contains("SHA-256") && e.contains("refused, nothing was installed"), "{e}");
    assert!(w.leftovers().is_empty(), "no git, no git.partial: {:?}", w.leftovers());
}

// spec: SYNC-025
#[test]
fn a_download_that_fails_or_a_program_that_does_not_run_leaves_nothing() {
    // the server has no file: curl fails
    let w = World::new("missing", "2.99.0", "9.9.9");
    let plan = w.plan("linux", "x86_64");
    w.files.map_clear();
    let e = install(&w.env(), &plan, &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("failed") || e.contains("404"), "{e}");
    assert!(w.leftovers().is_empty(), "{:?}", w.leftovers());
    // a git-lfs that exits with an error
    let bad = tar_gz(&[tar_entry("git-lfs-9.9.9/", b'5', b""), tar_entry("git-lfs-9.9.9/git-lfs", b'0', b"#!/bin/sh\necho broken >&2\nexit 1\n")]);
    let w = World::new("broken", "2.99.0", "9.9.9");
    let recipe = lfs_recipe("9.9.9", "git-lfs-linux-amd64-v{v}.tar.gz", "git-lfs", &bad);
    publish(&w.files, &recipe, &bad);
    let e = install(&w.env(), &w.sources.pinned_plan(&[&recipe]), &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("lfs/git-lfs failed") && e.contains("broken"), "{e}");
    assert!(w.leftovers().is_empty(), "{:?}", w.leftovers());
    // a program that runs but is another version than the release says
    let other = tar_gz(&[tar_entry("git-lfs-9.9.9/", b'5', b""), tar_entry("git-lfs-9.9.9/git-lfs", b'0', &script_text("git-lfs/1.0.0"))]);
    let recipe = lfs_recipe("9.9.9", "git-lfs-linux-amd64-v{v}.tar.gz", "git-lfs", &other);
    publish(&w.files, &recipe, &other);
    let e = install(&w.env(), &w.sources.pinned_plan(&[&recipe]), &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("does not say it is version 9.9.9"), "{e}");
    // an archive without the program
    let empty = tar_gz(&[tar_entry("git-lfs-9.9.9/", b'5', b""), tar_entry("git-lfs-9.9.9/readme", b'0', b"x")]);
    let recipe = lfs_recipe("9.9.9", "git-lfs-linux-amd64-v{v}.tar.gz", "git-lfs", &empty);
    publish(&w.files, &recipe, &empty);
    assert!(install(&w.env(), &w.sources.pinned_plan(&[&recipe]), &Handle::default(), &quiet).is_err());
    assert!(w.leftovers().is_empty(), "{:?}", w.leftovers());
}

// spec: SYNC-025
#[test]
fn an_archive_that_climbs_out_of_the_folder_is_refused_by_the_archive_reader() {
    let w = World::new("hostile", "2.99.0", "9.9.9");
    let evil = zip(&[z("../escaped.txt", b"x"), z("cmd/git.exe", &script_text("git version 2.99.0"))]);
    let recipe = mingit_recipe("2.99.0", &evil);
    publish(&w.files, &recipe, &evil);
    let e = install(&w.env(), &w.sources.pinned_plan(&[&recipe]), &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("could not be unpacked") && e.contains("refused"), "{e}");
    assert!(!w.data.join("shale").join("escaped.txt").exists() && !w.data.join("escaped.txt").exists());
    assert!(w.leftovers().is_empty(), "{:?}", w.leftovers());
}

// spec: SYNC-025
#[test]
fn cancel_stops_a_running_download_and_leaves_nothing_behind() {
    let w = World::new("cancel", "2.99.0", "9.9.9");
    let plan = w.plan("linux", "x86_64");
    w.files.hang("/git-lfs/git-lfs/releases/download/v9.9.9/git-lfs-linux-amd64-v9.9.9.tar.gz");
    let handle = Handle::default();
    let h = handle.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        h.cancel();
    });
    let e = install(&w.env(), &plan, &handle, &quiet).unwrap_err();
    stopper.join().unwrap();
    assert_eq!(e, "cancelled");
    assert!(handle.cancelled());
    assert!(w.leftovers().is_empty(), "{:?}", w.leftovers());
}

// spec: SYNC-025
#[test]
fn an_install_over_an_install_is_refused_and_changes_nothing() {
    let w = World::new("twice", "2.99.0", "9.9.9");
    let plan = w.plan("linux", "x86_64");
    install(&w.env(), &plan, &Handle::default(), &quiet).unwrap();
    let before = std::fs::read(managed_root(&w.data).join(MANIFEST)).unwrap();
    let e = install(&w.env(), &plan, &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("is there already: remove it first"), "{e}");
    assert_eq!(std::fs::read(managed_root(&w.data).join(MANIFEST)).unwrap(), before);
    assert_eq!(w.leftovers(), ["git"]);
    // an empty plan is an error too
    assert!(install(&Env { data: temp("empty") }, &Plan { parts: vec![], allow_http: true }, &Handle::default(), &quiet).is_err());
}

// spec: SYNC-025
#[test]
fn the_progress_shows_each_download_and_its_size() {
    let w = World::new("progress", "2.99.0", "9.9.9");
    let seen = Mutex::new(Vec::<String>::new());
    let handle = Handle::default();
    let h2 = handle.clone();
    install(&w.env(), &w.plan("windows", "x86_64"), &handle, &|| seen.lock().unwrap().push(h2.progress().text)).unwrap();
    let seen = seen.into_inner().unwrap();
    assert!(seen.iter().any(|t| t.starts_with("Downloading MinGit 2.99.0 (")) && seen.iter().any(|t| t.starts_with("Downloading git-lfs 9.9.9")), "{seen:?}");
    assert!(seen.iter().any(|t| t == "Unpacking MinGit") && seen.iter().any(|t| t == "Checking git-lfs"), "{seen:?}");
}

// ---------------------------------------------------------------- update

fn world_with_installed(tag: &str) -> World {
    let w = World::new(tag, "2.99.0", "9.9.9");
    install(&w.env(), &w.plan("windows", "x86_64"), &Handle::default(), &quiet).unwrap();
    w
}

/// The newer fake release, published on the server of `w`: the API answers and the files.
pub(super) fn publish_newer_for_ui(w: &World, git: &str, lfs: &str) -> Plan {
    publish_newer(w, git, lfs)
}

fn publish_newer(w: &World, git: &str, lfs: &str) -> Plan {
    let (g, l) = (mingit_zip(git), lfs_zip(lfs, "git-lfs.exe"));
    let (rg, rl) = (mingit_recipe(git, &g), lfs_recipe(lfs, "git-lfs-windows-amd64-v{v}.zip", "git-lfs.exe", &l));
    publish(&w.files, &rg, &g);
    publish(&w.files, &rl, &l);
    for (r, b) in [(&rg, &g), (&rl, &l)] {
        let file = r.file.replace("{v}", &r.pin.version);
        let url = format!("{}/{}/releases/download/{}/{file}", w.sources.download_base, r.repo, r.pin.tag);
        let json = serde_json::json!({"tag_name": r.pin.tag, "assets": [{"name": file, "size": b.len(), "digest": format!("sha256:{}", sha256_hex(b)), "browser_download_url": url}]});
        w.files.put(&format!("/repos/{}/releases/latest", r.repo), json.to_string().into_bytes());
    }
    Plan { parts: vec![w.sources.pinned(&rg), w.sources.pinned(&rl)], allow_http: true }
}

// spec: SYNC-027
#[test]
fn the_check_finds_a_newer_release_by_the_api_with_its_listed_checksum_and_nothing_when_up_to_date() {
    let w = world_with_installed("check");
    let platform = Platform::new("windows", "x86_64");
    let installed = Installed::read(&managed_root(&w.data)).unwrap();
    let fetch = |url: &str| fetch_api(url, true, &std::sync::atomic::AtomicBool::new(false));
    // no release is published: the API has nothing, the check fails in words
    assert!(check_latest(&w.sources, &platform, &installed, &fetch).is_err());
    // the same release as installed: up to date
    let same = publish_newer(&w, "2.99.0", "9.9.9");
    assert_eq!(same.parts.len(), 2);
    assert!(check_latest(&w.sources, &platform, &installed, &fetch).unwrap().is_none());
    // a newer git-lfs only: both are in the plan (the tree is rebuilt whole), with the listed checksums
    publish_newer(&w, "2.99.0", "9.10.0");
    let plan = check_latest(&w.sources, &platform, &installed, &fetch).unwrap().unwrap();
    assert_eq!(plan.versions_text(), "git 2.99.0, git-lfs 9.10.0");
    assert_eq!(plan.parts[1].sha256, sha256_hex(&lfs_zip("9.10.0", "git-lfs.exe")));
    assert!(plan.parts[1].url.starts_with(&format!("{}/git-lfs/git-lfs/releases/download/v9.10.0/", w.sources.download_base)));
    // the API was asked with the JSON Accept header, once per project
    let asked = w.files.asked.lock().unwrap().clone();
    assert!(asked.iter().filter(|(p, _)| p.starts_with("/repos/")).all(|(_, a)| a.as_deref() == Some("application/vnd.github+json")), "{asked:?}");
    // a part that is not managed (the git of the system) is not looked up
    let lfs_only = Installed { git: None, ..installed.clone() };
    let before = w.files.paths().len();
    check_latest(&w.sources, &platform, &lfs_only, &fetch).unwrap();
    assert!(w.files.paths()[before..].iter().all(|p| p.contains("git-lfs/git-lfs")), "{:?}", &w.files.paths()[before..]);
    // a platform without sources
    assert!(check_latest(&w.sources, &Platform::new("plan9", "mips"), &installed, &fetch).is_err());
}

// spec: SYNC-027
#[test]
fn an_update_replaces_the_installed_git_and_removes_the_old_one() {
    let w = world_with_installed("update");
    let plan = publish_newer(&w, "2.100.0", "9.10.0");
    let done = update(&w.env(), &plan, &Handle::default(), &quiet).unwrap();
    assert_eq!(done.installed.versions_text(), "git 2.100.0, git-lfs 9.10.0");
    assert_eq!(Installed::read(&managed_root(&w.data)).unwrap().versions_text(), "git 2.100.0, git-lfs 9.10.0");
    assert_eq!(w.leftovers(), ["git"], "no git.old, no git.partial");
    assert_eq!(selected_git(GitChoice::Managed, Some(&w.data), None).version().as_deref(), Some("git version 2.100.0.windows.1"));
}

// spec: SYNC-027
#[test]
fn a_failed_or_cancelled_update_leaves_the_working_install_as_it_was() {
    let w = world_with_installed("update-fails");
    let plan = publish_newer(&w, "2.100.0", "9.10.0");
    let before = std::fs::read(managed_root(&w.data).join(MANIFEST)).unwrap();
    let intact = |w: &World| {
        assert_eq!(std::fs::read(managed_root(&w.data).join(MANIFEST)).unwrap(), before);
        assert_eq!(selected_git(GitChoice::Managed, Some(&w.data), None).version().as_deref(), Some("git version 2.99.0.windows.1"));
        assert_eq!(w.leftovers(), ["git"]);
    };
    // a checksum that is not the file's
    let mut bad = plan.clone();
    bad.parts[1].sha256 = "00".repeat(32);
    assert!(update(&w.env(), &bad, &Handle::default(), &quiet).unwrap_err().contains("SHA-256"));
    intact(&w);
    // a cancel before it starts
    let handle = Handle::default();
    handle.cancel();
    assert_eq!(update(&w.env(), &plan, &handle, &quiet).unwrap_err(), "cancelled");
    intact(&w);
    // a download that hangs, then is cancelled
    w.files.hang("/git-for-windows/git/releases/download/v2.100.0.windows.1/MinGit-2.100.0-64-bit.zip");
    let handle = Handle::default();
    let h = handle.clone();
    let stopper = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(600));
        h.cancel();
    });
    assert_eq!(update(&w.env(), &plan, &handle, &quiet).unwrap_err(), "cancelled");
    stopper.join().unwrap();
    intact(&w);
    // there is nothing to update
    let e = update(&Env { data: temp("nothing") }, &plan, &Handle::default(), &quiet).unwrap_err();
    assert!(e.contains("nothing to update"), "{e}");
}

// ---------------------------------------------------------------- remove

// spec: SYNC-028
#[test]
fn only_the_exact_managed_folders_may_be_removed() {
    let data = temp("removable");
    std::fs::create_dir_all(managed_root(&data)).unwrap();
    assert!(removable(&data, &managed_root(&data)).is_ok());
    for other in [data.join("shale"), data.join("shale").join("texlive"), data.join("shale").join("git").join("lfs"), data.join("git"), data.clone()] {
        let e = removable(&data, &other).unwrap_err();
        assert!(e.contains("is not the folder of the managed git"), "{other:?}: {e}");
    }
    assert!(removable(&data, &partial_root(&data)).is_err(), "a twin that is not there is not a folder");
    let _ = std::fs::remove_dir_all(&data);
}

// spec: SYNC-028
#[test]
fn a_link_is_never_followed_out_of_the_data_folder() {
    use std::os::unix::fs::symlink;
    let data = temp("links");
    let outside = temp("links-outside");
    std::fs::write(outside.join("precious.txt"), "keep me").unwrap();
    std::fs::create_dir_all(data.join("shale")).unwrap();
    symlink(&outside, managed_root(&data)).unwrap();
    assert!(remove(&data).unwrap_err().contains("not a plain folder"));
    assert!(outside.join("precious.txt").is_file());
    std::fs::remove_file(managed_root(&data)).unwrap();
    // a link in the place of <data>/shale
    std::fs::remove_dir_all(data.join("shale")).unwrap();
    std::fs::create_dir_all(outside.join("git")).unwrap();
    symlink(&outside, data.join("shale")).unwrap();
    assert!(remove(&data).is_err());
    assert!(outside.join("git").is_dir(), "the link was not followed");
    // a link inside the tree is removed as a link, its target stays
    std::fs::remove_file(data.join("shale")).unwrap();
    std::fs::create_dir_all(managed_root(&data).join("lfs")).unwrap();
    symlink(&outside, managed_root(&data).join("lfs").join("linked")).unwrap();
    remove(&data).unwrap();
    assert!(outside.join("precious.txt").is_file(), "the target of a link inside the tree stays");
    let _ = std::fs::remove_dir_all(&data);
    let _ = std::fs::remove_dir_all(&outside);
}

// spec: SYNC-028
#[test]
fn remove_deletes_the_managed_folder_and_its_leftover_twins_and_nothing_else() {
    let w = world_with_installed("remove");
    std::fs::create_dir_all(partial_root(&w.data)).unwrap();
    std::fs::create_dir_all(old_root(&w.data)).unwrap();
    std::fs::create_dir_all(w.data.join("shale").join("texlive")).unwrap();
    std::fs::write(w.data.join("shale").join("config.json"), "{}").unwrap();
    remove(&w.data).unwrap();
    assert_eq!(w.leftovers(), ["config.json", "texlive"], "only git and its twins are gone");
    assert!(remove(&w.data).unwrap_err().contains("not a plain folder"), "nothing to remove is said, not hidden");
}

// ---------------------------------------------------------------- the real thing

/// The real git-lfs of this platform, from github.com into a temporary data folder (SYNC-025,
/// SYNC-029). Needs a network. `cargo test --locked real_ -- --ignored`
// spec: SYNC-029
#[test]
#[ignore = "downloads git-lfs from github.com"]
fn real_install_of_git_lfs_from_github_and_it_runs() {
    let sources = Sources::real();
    let platform = Platform::current();
    let Some(recipes) = sources.recipes(&platform) else { return };
    let lfs: Vec<&Recipe> = recipes.iter().filter(|r| r.kind == Kind::Lfs).collect();
    let data = temp("real-lfs");
    let handle = Handle::default();
    let done = install(&Env { data: data.clone() }, &sources.pinned_plan(&lfs), &handle, &quiet).unwrap();
    assert_eq!(done.installed.lfs.unwrap().version, "3.8.1");
    let program = managed_root(&data).join(&lfs[0].program);
    let out = std::process::Command::new(&program).arg("version").output().unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("git-lfs/3.8.1"), "{out:?}");
    // with a git on this machine, `git lfs version` finds it through the managed PATH
    let git = selected_git(GitChoice::Managed, Some(&data), None);
    if git.version().is_some() {
        assert!(git.lfs_version().is_some_and(|v| v.starts_with("git-lfs/3.8.1")), "{:?}", git.lfs_version());
    }
    remove(&data).unwrap();
    assert!(!managed_root(&data).exists());
    let _ = std::fs::remove_dir_all(&data);
}

/// The real MinGit of Windows x86-64 downloaded, checked against its pinned SHA-256 and unpacked by the
/// archive reader (it is not run here: it is a Windows program).
// spec: SYNC-029
#[test]
#[ignore = "downloads MinGit (40 MB) from github.com"]
fn real_mingit_zip_has_the_pinned_checksum_and_unpacks_with_its_program() {
    let sources = Sources::real();
    let recipes = sources.recipes(&Platform::new("windows", "x86_64")).unwrap();
    let part = sources.pinned(&recipes[0]);
    let dir = temp("real-mingit");
    let file = dir.join("mingit.zip");
    download_part(&part, false, &file, &Handle::default(), "MinGit", 0.0, 1.0).unwrap();
    let bytes = std::fs::read(&file).unwrap();
    assert_eq!((bytes.len() as u64, sha256_hex(&bytes)), (part.size, part.sha256.clone()));
    let partial = dir.join("partial");
    std::fs::create_dir_all(&partial).unwrap();
    unpack(&part, &bytes, &partial, 0).unwrap();
    assert!(partial.join("cmd/git.exe").is_file() && partial.join("etc/gitconfig").is_file(), "{:?}", std::fs::read_dir(&partial).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>());
    assert!(crate::pdf::tex::engine::folder_bytes(&partial) > 90_000_000, "about {} bytes on disk", part.disk);
    let _ = std::fs::remove_dir_all(&dir);
}
