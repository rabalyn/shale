//! Shale's storage on network shares and cloud-synced folders (NET-*, rabalyn/shale#470).
//!
//! One set of checks ([`storage_checks`]) runs Shale's real code (the atomic save, the vault scan,
//! the save-time conflict check, the fingerprints, the lock file, the vault state) in a folder and
//! reports what that folder's file system did:
//!
//! - on a local folder, always (`net_001_…`: the baseline);
//! - on the Samba share mounted rootless through rclone FUSE (`smb_fuse_env_*`, ignored unless asked for, run by the `smb_fuse_env`
//!   suite of `.forgejo/scripts/env-tests.sh`; rclone's client, NOT the kernel CIFS client, so its observations say so);
//! - on a share or export mounted by the kernel (`smb_env_*`, `nfs_env_*`, ignored: `tests/smb-env/`, `tests/nfs-env/`; a kernel
//!   mount needs root, so the owner runs them by hand, `docs/storage-on-shares.md`);
//! - between two devices that keep a folder in sync, through the Syncthing pair of
//!   `tests/syncthing-env/` (rootless):
//!
//! ```text
//! tests/syncthing-env/syncthing-env.sh up
//! cargo test -- --ignored syncthing_env --test-threads=1
//! tests/syncthing-env/syncthing-env.sh down
//! ```
//!
//! What a check observes about the file system and cannot be a rule (a lock that does not exclude
//! on NFS, names that are folded together) is printed as `NET observation:` lines (`--nocapture`),
//! and `docs/storage-on-shares.md` quotes them.
#![cfg(test)]

use crate::fsutil;
use crate::vault::{Saved, Stamp, Vault};
use crate::vault_state::VaultData;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

fn env_dir(stack: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join(stack)
}

/// A fresh, empty folder inside `base`, removed with its content when dropped.
struct Work(PathBuf);

impl Work {
    fn new(base: &Path, tag: &str) -> Work {
        let dir = base.join(format!("net-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Work(dir)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default();
    v.sort();
    v
}

fn note(i: usize) -> String {
    format!("# Note\n\nversion {i}\n{}", "line of text\n".repeat(200))
}

fn observe(label: &str, what: &str) {
    eprintln!("NET observation [{label}]: {what}");
}

/// Wait until `f` is true (polled every 50 ms); the panic names `what`.
fn wait_until(secs: u64, what: &str, mut f: impl FnMut() -> bool) -> Duration {
    let start = Instant::now();
    while !f() {
        assert!(start.elapsed() < Duration::from_secs(secs), "timed out after {secs} s waiting for: {what}");
        std::thread::sleep(Duration::from_millis(50));
    }
    start.elapsed()
}

/// NET-001: an atomic save leaves no temporary file, and the vault scan never lists one.
fn check_atomic_save(label: &str, root: &Path) {
    let w = Work::new(root, "atomic");
    let mut vault = Vault::open(&w.0).unwrap();
    let mut base = Stamp::of("");
    fsutil::write_atomic(&w.0.join("N.md"), b"").unwrap();
    for i in 0..40 {
        let text = note(i);
        let Saved::Written(s) = vault.save_checked(Path::new("N.md"), &text, base).unwrap() else { panic!("{label}: a save of our own text is no conflict") };
        base = s;
        assert_eq!(fsutil::read_to_string(w.0.join("N.md")).unwrap(), text, "{label}: what was saved is what is read back");
    }
    let listed = names(&w.0);
    assert_eq!(listed, ["N.md"], "{label}: no temporary file is left in the folder");
    // a temporary file that a crashed save left behind is in the folder but not in the vault
    fsutil::create_new(&w.0.join(".shale-tmp-1-1-abc"), b"half a note").unwrap();
    vault.rescan();
    assert_eq!(vault.files.iter().map(|f| f.rel.to_string_lossy().into_owned()).collect::<Vec<_>>(), ["N.md"], "{label}: the scan does not list .shale-tmp-* files");
}

/// NET-002: a change made by another program, inside one timestamp tick, is seen by the next scan.
fn check_outside_change(label: &str, root: &Path) {
    let w = Work::new(root, "outside");
    let mut vault = Vault::open(&w.0).unwrap();
    let rel = Path::new("N.md");
    let Saved::Written(_) = vault.save_checked(rel, "aaaa\n", Stamp::of("")).unwrap() else { panic!() };
    assert!(vault.rescan().is_empty(), "{label}: nothing changed");
    // in place, the same length, right now: the modification time may be the same
    let mut f = OpenOptions::new().write(true).open(w.0.join("N.md")).unwrap();
    f.write_all(b"bbbb\n").unwrap();
    f.sync_all().unwrap();
    drop(f);
    assert_eq!(vault.rescan(), [rel.to_path_buf()], "{label}: an in-place change of the same length is detected");
    assert_eq!(vault.notes[rel].text, "bbbb\n");
    // another device's atomic replace: a new inode, the same length
    fsutil::write_atomic(&w.0.join("N.md"), b"cccc\n").unwrap();
    assert_eq!(vault.rescan(), [rel.to_path_buf()], "{label}: an atomic replace of the same length is detected");
    // the modification time's granularity of this file system
    let ns: Vec<u32> = (0..3)
        .map(|i| {
            let p = w.0.join(format!("t{i}.md"));
            fsutil::write_atomic(&p, b"x").unwrap();
            let m = fs::metadata(&p).unwrap().modified().unwrap();
            m.duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos()
        })
        .collect();
    let coarse = if ns.iter().all(|n| n % 2_000_000_000 == 0) && ns.iter().all(|n| *n == 0) { "whole seconds (2 s on FAT/SMB cannot be told from 1 s here)" } else { "finer than a second" };
    let skew = fs::metadata(w.0.join("t0.md")).unwrap().modified().unwrap().duration_since(std::time::SystemTime::now()).map(|d| d.as_secs_f64()).unwrap_or_else(|e| -e.duration().as_secs_f64());
    observe(label, &format!("modification time is {coarse}; subsecond parts {ns:?}; file time minus local clock {skew:.3} s"));
}

/// NET-003: a save over a file that another device changed is a conflict copy, never an overwrite.
fn check_conflict_copy(label: &str, root: &Path) {
    let w = Work::new(root, "conflict");
    let mut vault = Vault::open(&w.0).unwrap();
    let rel = Path::new("N.md");
    let Saved::Written(base) = vault.save_checked(rel, "ours 1\n", Stamp::of("")).unwrap() else { panic!() };
    fsutil::write_atomic(&w.0.join("N.md"), b"theirs\n").unwrap();
    match vault.save_checked(rel, "ours 2\n", base).unwrap() {
        Saved::Conflict { copy, theirs } => {
            assert_eq!(theirs.as_deref(), Ok("theirs\n"), "{label}");
            assert_eq!(fsutil::read_to_string(w.0.join(&copy)).unwrap(), "ours 2\n", "{label}: our text is in the copy");
        }
        other => panic!("{label}: expected a conflict copy, got {other:?}"),
    }
    assert_eq!(fsutil::read_to_string(w.0.join("N.md")).unwrap(), "theirs\n", "{label}: their version stays");
}

/// What a second process gets when it takes the lock on `path` through `fsutil::try_lock_file`:
/// runs this test binary as a child that executes [`net_004_lock_probe_child`] and reads its
/// answer from stdout. `true`: the lock was GRANTED. Needs no `flock(1)`, so it works on every
/// platform.
fn lock_from_another_process(path: &Path) -> bool {
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe).args(["--exact", "netshare_env_tests::net_004_lock_probe_child", "--ignored", "--nocapture", "--test-threads", "1"]).env(LOCK_PROBE_ENV, path).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "the lock probe child failed: {text}{}", String::from_utf8_lossy(&out.stderr));
    if text.contains("lock-probe: granted") {
        true
    } else {
        assert!(text.contains("lock-probe: refused"), "the lock probe child gave no answer: {text}");
        false
    }
}

const LOCK_PROBE_ENV: &str = "SHALE_NET_LOCK_PROBE";

/// Not a test of its own: the child side of [`lock_from_another_process`] (ignored, so a normal run
/// skips it). It takes the lock named by the environment and prints whether it was granted; without
/// the variable it fails, so running it by hand does not pass silently.
#[test]
#[ignore = "child of check_lock (NET-004); needs SHALE_NET_LOCK_PROBE"]
fn net_004_lock_probe_child() {
    let path = std::env::var_os(LOCK_PROBE_ENV).expect("SHALE_NET_LOCK_PROBE names the lock file; this test is run by check_lock");
    let got = fsutil::try_lock_file(Path::new(&path)).unwrap();
    println!("lock-probe: {}", if got.is_some() { "granted" } else { "refused" });
}

/// NET-004: the lock file (DATA-012, SYNC-013, the env lock) and what the file system makes of it.
/// Reports; asserts only that the lock can be taken at all.
fn check_lock(label: &str, root: &Path, expect_exclusive: bool) {
    let w = Work::new(root, "lock");
    let p = w.0.join("x.lock");
    let first = fsutil::try_lock_file(&p).unwrap().expect("the first lock is taken");
    let second = fsutil::try_lock_file(&p).unwrap();
    let across = lock_from_another_process(&p);
    observe(label, &format!("a second flock by the same process is {}; the lock from another process is {}", if second.is_none() { "refused" } else { "GRANTED (no exclusion)" }, if across { "GRANTED (no exclusion)" } else { "refused" }));
    if expect_exclusive {
        assert!(second.is_none(), "{label}: the lock excludes a second open");
        assert!(!across, "{label}: the lock excludes another process");
    }
    drop(first);
}

/// NET-005: names that differ in letter case or in Unicode normalisation: never a lost file.
fn check_names(label: &str, root: &Path) {
    let w = Work::new(root, "names");
    // case
    fsutil::create_new(&w.0.join("Plan.md"), b"upper").unwrap();
    let insensitive = match fsutil::create_new(&w.0.join("plan.md"), b"lower") {
        Ok(()) => {
            assert_eq!(fsutil::read_to_string(w.0.join("Plan.md")).unwrap(), "upper", "{label}: two names, two files");
            false
        }
        Err(e) => {
            assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists, "{label}");
            assert_eq!(fsutil::read_to_string(w.0.join("Plan.md")).unwrap(), "upper", "{label}: the existing file is not replaced");
            true
        }
    };
    // a case-only rename works either way (DATA-013)
    fsutil::create_new(&w.0.join("rename-me.md"), b"r").unwrap();
    fsutil::rename_noreplace(&w.0.join("rename-me.md"), &w.0.join("Rename-Me.md")).unwrap();
    assert!(names(&w.0).contains(&"Rename-Me.md".to_string()) && !names(&w.0).contains(&"rename-me.md".to_string()), "{label}: the case-only rename took effect: {:?}", names(&w.0));
    // Unicode: é as one character (NFC) and as e + combining acute (NFD)
    let (nfc, nfd) = ("Caf\u{e9}.md", "Cafe\u{301}.md");
    fsutil::create_new(&w.0.join(nfc), b"nfc").unwrap();
    let folded = match fsutil::create_new(&w.0.join(nfd), b"nfd") {
        Ok(()) => false,
        Err(e) => {
            assert_eq!(e.kind(), std::io::ErrorKind::AlreadyExists, "{label}");
            true
        }
    };
    assert_eq!(fsutil::read_to_string(w.0.join(nfc)).unwrap(), "nfc", "{label}: the first file is intact");
    let mut vault = Vault::open(&w.0).unwrap();
    vault.rescan();
    let cafes = vault.files.iter().filter(|f| f.rel.to_string_lossy().starts_with("Caf")).count();
    observe(label, &format!("letter case is {}; NFC and NFD names are {} ({cafes} note(s) with that name in the vault)", if insensitive { "IGNORED" } else { "kept" }, if folded { "the same file" } else { "two files" }));
}

/// The whole set, in a folder of the file system under test.
fn storage_checks(label: &str, root: &Path, expect_exclusive_lock: bool) {
    check_atomic_save(label, root);
    check_outside_change(label, root);
    check_conflict_copy(label, root);
    check_lock(label, root, expect_exclusive_lock);
    check_names(label, root);
}

// spec: NET-001, NET-002, NET-003, NET-004, NET-005
#[test]
fn net_001_the_storage_checks_pass_on_a_local_folder() {
    let base = std::env::temp_dir().join(format!("shale-net-local-{}", std::process::id()));
    fs::create_dir_all(&base).unwrap();
    storage_checks("local", &base, true);
    let _ = fs::remove_dir_all(&base);
}

/// The folder a mount helper (`smb-env.sh dir`, `smb-env.sh fuse-dir`, `nfs-env.sh dir`) prints.
fn mounted_dir(stack: &str, script: &str, command: &str) -> PathBuf {
    let out = Command::new("sh").arg(env_dir(stack).join(script)).arg(command).output().expect("cannot start the helper script");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    PathBuf::from(String::from_utf8_lossy(&out.stdout).trim())
}

// spec: NET-006, NET-011
#[test]
#[ignore = "needs a mounted Samba share: tests/smb-env/smb-env.sh up, then mount"]
fn smb_env_storage_checks() {
    crate::env_lock::hold();
    let dir = mounted_dir("smb-env", "smb-env.sh", "dir");
    storage_checks("smb", &dir, false);
}

/// The label of the rootless stand-in: its observations are rclone's SMB client and VFS layer, not the kernel CIFS client's.
const SMB_FUSE_LABEL: &str = "smb, rclone FUSE, not the kernel CIFS client";

// spec: NET-006, NET-011, NET-018
#[test]
#[ignore = "needs the Samba share mounted by rclone: tests/smb-env/smb-env.sh up, then fuse-mount (rootless)"]
fn smb_fuse_env_storage_checks() {
    crate::env_lock::hold();
    let dir = mounted_dir("smb-env", "smb-env.sh", "fuse-dir");
    // The lock EXCLUDES here, unlike the kernel CIFS client's (smb_env_storage_checks, which only reports): a FUSE file system
    // that does not implement `flock` gets the kernel's local lock emulation, so two processes of this machine exclude each
    // other and rclone takes no lock on the server, which no other client would see. Expected per client, not a finding about SMB.
    storage_checks(SMB_FUSE_LABEL, &dir, true);
}

// spec: NET-006, NET-012
#[test]
#[ignore = "kernel mount by hand, no rootless stand-in (rclone has no NFS client, no sound FUSE NFS client exists, and the kernel nfsd server needs root too): sudo tests/nfs-env/nfs-env.sh up, then mount (#477)"]
fn nfs_env_storage_checks() {
    crate::env_lock::hold();
    let dir = mounted_dir("nfs-env", "nfs-env.sh", "dir");
    storage_checks("nfs", &dir, false);
}

/// The detection of SYNC-059 on a mounted share: a network file system, by the mount table.
fn detect_network_on(dir: &Path, fs_names: &[&str]) {
    let found = crate::gitsync::detect::detect_real(dir);
    let net: Vec<_> = found.iter().filter(|f| f.is_network()).collect();
    assert!(net.iter().any(|f| matches!(&f.kind, crate::gitsync::detect::Kind::Network { fs, sure: true } if fs_names.contains(&fs.as_str()))), "no network file system among {found:?} (wanted one of {fs_names:?})");
    observe("detect", &format!("{} is {}", dir.display(), net[0].what()));
}

// spec: SYNC-059, NET-011
#[test]
#[ignore = "needs a mounted Samba share: tests/smb-env/smb-env.sh up, then mount"]
fn smb_env_detect_sees_the_mounted_share_as_a_network_folder() {
    crate::env_lock::hold();
    detect_network_on(&mounted_dir("smb-env", "smb-env.sh", "dir"), &["cifs", "smb3", "smb2", "smbfs"]);
}

// spec: SYNC-059, NET-018
#[test]
#[ignore = "needs the Samba share mounted by rclone: tests/smb-env/smb-env.sh up, then fuse-mount (rootless)"]
fn smb_fuse_env_detect_sees_the_fuse_mounted_share_as_a_network_folder() {
    crate::env_lock::hold();
    detect_network_on(&mounted_dir("smb-env", "smb-env.sh", "fuse-dir"), &["fuse.rclone"]);
}

// spec: SYNC-059, NET-012
#[test]
#[ignore = "needs a mounted NFS export: tests/nfs-env/nfs-env.sh up, then mount"]
fn nfs_env_detect_sees_the_mounted_export_as_a_network_folder() {
    crate::env_lock::hold();
    detect_network_on(&mounted_dir("nfs-env", "nfs-env.sh", "dir"), &["nfs", "nfs4"]);
}

// ---- two devices through Syncthing ----

/// The vault folders of device A and device B of the running pair.
fn pair() -> (PathBuf, PathBuf) {
    let out = Command::new("sh").arg(env_dir("syncthing-env").join("syncthing-env.sh")).arg("dirs").output().expect("cannot start syncthing-env.sh");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut lines = text.lines();
    (PathBuf::from(lines.next().unwrap()), PathBuf::from(lines.next().unwrap()))
}

fn st(cmd: &str) {
    let out = Command::new("sh").arg(env_dir("syncthing-env").join("syncthing-env.sh")).arg(cmd).output().expect("cannot start syncthing-env.sh");
    assert!(out.status.success(), "syncthing-env.sh {cmd}: {}", String::from_utf8_lossy(&out.stderr));
}

fn has_text(p: PathBuf, text: &'static str) -> impl FnMut() -> bool {
    move || fs::read_to_string(&p).map(|t| t == text).unwrap_or(false)
}

/// A synced pair of folders `t-<tag>-<pid>` (made on A, waited for on B), removed on both sides at the end.
struct Pair {
    a: PathBuf,
    b: PathBuf,
}

impl Pair {
    fn new(tag: &str) -> Pair {
        let (da, db) = pair();
        // a name of its own per run: a delete of an earlier run that is still on its way must not meet a new folder of the same name
        let name = format!("t-{tag}-{}", std::process::id());
        let (a, b) = (da.join(&name), db.join(&name));
        fs::create_dir_all(&a).unwrap();
        wait_until(60, "the folder to reach B", || b.is_dir());
        Pair { a, b }
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        let _ = Command::new("sh").arg(env_dir("syncthing-env").join("syncthing-env.sh")).arg("resume").output();
        let _ = fs::remove_dir_all(&self.a);
        let _ = fs::remove_dir_all(&self.b);
    }
}

// spec: NET-001, NET-002, NET-010
#[test]
#[ignore = "needs the Syncthing pair: tests/syncthing-env/syncthing-env.sh up"]
fn syncthing_env_saves_arrive_on_the_other_device_and_no_temp_file_leaks() {
    crate::env_lock::hold();
    let p = Pair::new("atomic");
    let rel = Path::new("N.md");
    let mut va = Vault::open(&p.a).unwrap();
    let mut vb = Vault::open(&p.b).unwrap();
    // B's poller: every name that appears in B's folder, 100 times a second
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = std::sync::Arc::new(std::sync::Mutex::new(std::collections::BTreeSet::<String>::new()));
    let poller = {
        let (stop, seen, dir) = (stop.clone(), seen.clone(), p.b.clone());
        std::thread::spawn(move || {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                seen.lock().unwrap().extend(names(&dir));
                std::thread::sleep(Duration::from_millis(10));
            }
        })
    };
    let mut base = Stamp::of("");
    let mut last = String::new();
    for i in 0..30 {
        last = note(i);
        let Saved::Written(s) = va.save_checked(rel, &last, base).unwrap() else { panic!("no conflict on A") };
        base = s;
        std::thread::sleep(Duration::from_millis(150));
    }
    let after = last.clone();
    let took = wait_until(60, "the last version to reach B", || fs::read_to_string(p.b.join(rel)).map(|t| t == after).unwrap_or(false));
    // B's Shale finds the change by its own scan (DATA-002)
    let found = wait_until(10, "B's scan to report the note", || vb.rescan().contains(&rel.to_path_buf()) || vb.notes.get(rel).is_some_and(|n| n.text == after));
    assert_eq!(vb.notes[rel].text, last);
    std::thread::sleep(Duration::from_secs(5));
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    poller.join().unwrap();
    let seen: Vec<String> = seen.lock().unwrap().iter().cloned().collect();
    observe("syncthing", &format!("last save reached B {:.1} s later; B's scan found it {:.1} s after that; every name B ever listed: {seen:?}", took.as_secs_f64(), found.as_secs_f64()));
    assert!(seen.iter().all(|n| !n.contains("shale-tmp")), "a Shale temporary file reached the other device: {seen:?}");
    assert_eq!(names(&p.b), ["N.md"], "nothing but the note is in B's folder");
}

// spec: NET-007, NET-015
#[test]
#[ignore = "needs the Syncthing pair: tests/syncthing-env/syncthing-env.sh up"]
fn syncthing_env_an_edit_on_both_devices_gives_a_sync_conflict_file_that_shale_lists() {
    crate::env_lock::hold();
    let p = Pair::new("conflict");
    let rel = Path::new("N.md");
    fsutil::create_new(&p.a.join(rel), b"base\n").unwrap();
    wait_until(60, "the note to reach B", has_text(p.b.join(rel), "base\n"));
    st("pause");
    std::thread::sleep(Duration::from_secs(2));
    fsutil::write_atomic(&p.a.join(rel), b"from A\n").unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    fsutil::write_atomic(&p.b.join(rel), b"from B\n").unwrap();
    std::thread::sleep(Duration::from_secs(2));
    st("resume");
    let conflicted = |d: &Path| names(d).into_iter().find(|n| n.contains(".sync-conflict-"));
    wait_until(90, "a .sync-conflict- file on both devices", || conflicted(&p.a).is_some() && conflicted(&p.b).is_some());
    wait_until(60, "both devices to hold the same two files", || names(&p.a) == names(&p.b));
    let name = conflicted(&p.a).unwrap();
    observe("syncthing", &format!("files after the conflict: {:?}", names(&p.a)));
    // Name.sync-conflict-YYYYMMDD-HHMMSS-DEVICEID.md: the extension stays last, so it is a note
    let stem = name.strip_suffix(".md").expect("the extension is kept");
    let (_, tail) = stem.split_once(".sync-conflict-").unwrap();
    let parts: Vec<&str> = tail.split('-').collect();
    assert!(parts.len() == 3 && parts[0].len() == 8 && parts[1].len() == 6 && !parts[2].is_empty(), "the name pattern is Name.sync-conflict-DATE-TIME-DEVICE.md, not {name}");
    for dir in [&p.a, &p.b] {
        let v = Vault::open(dir).unwrap();
        let mut texts: Vec<String> = v.files.iter().map(|f| fsutil::read_to_string(dir.join(&f.rel)).unwrap()).collect();
        texts.sort();
        assert_eq!(texts, ["from A\n", "from B\n"], "both versions survive and are notes of the vault: {:?}", v.files.iter().map(|f| &f.rel).collect::<Vec<_>>());
        assert!(v.notes.contains_key(Path::new(&name)), "the conflict file is a note (the vault lists it)");
        // NET-015: and Shale knows it is Syncthing's copy of N.md, and leaves it out of the links
        let c = v.conflict_of(Path::new(&name)).expect("the real Syncthing name is recognised");
        assert_eq!((c.client, c.original.as_deref()), (crate::conflict::Client::Syncthing, Some(Path::new("N.md"))));
        assert!(v.is_shadow_copy(Path::new(&name)) && v.conflict_of(Path::new("N.md")).is_none());
        assert_eq!(v.resolve("N", None), Some(PathBuf::from("N.md")));
    }
}

// spec: NET-008
#[test]
#[ignore = "needs the Syncthing pair: tests/syncthing-env/syncthing-env.sh up"]
fn syncthing_env_a_case_only_rename_and_unicode_names_arrive_as_they_are() {
    crate::env_lock::hold();
    let p = Pair::new("names");
    fsutil::create_new(&p.a.join("plan.md"), b"p").unwrap();
    let nfd = "Cafe\u{301}.md";
    fsutil::create_new(&p.a.join(nfd), b"nfd").unwrap();
    wait_until(60, "both notes to reach B", || names(&p.b).len() == 2);
    fsutil::rename_noreplace(&p.a.join("plan.md"), &p.a.join("Plan.md")).unwrap();
    wait_until(60, "the new spelling to reach B", || names(&p.b).contains(&"Plan.md".to_string()) && !names(&p.b).contains(&"plan.md".to_string()));
    // Syncthing sends names as NFC: the file made as NFD (what macOS makes) arrives under the NFC spelling, as the same single note
    let cafes: Vec<String> = names(&p.b).into_iter().filter(|n| n.starts_with("Caf")).collect();
    assert_eq!(cafes.len(), 1, "one note, not two: {cafes:?}");
    assert_eq!(fs::read_to_string(p.b.join(&cafes[0])).unwrap(), "nfd");
    observe("syncthing", &format!("a note made as NFD on A is on B as {}", if cafes[0] == nfd { "NFD (byte for byte)" } else { "NFC (the name was normalised)" }));
}

// spec: NET-009
#[test]
#[ignore = "needs the Syncthing pair: tests/syncthing-env/syncthing-env.sh up"]
fn syncthing_env_vault_state_written_on_both_devices_at_once_leaves_a_conflict_file_in_shale_folder() {
    crate::env_lock::hold();
    let p = Pair::new("state");
    let mut da = VaultData::load(&p.a).0;
    da.state.recent_tags = vec!["a".into()];
    da.save().unwrap();
    wait_until(60, "state.json to reach B", || p.b.join(".shale").join("state.json").is_file());
    st("pause");
    std::thread::sleep(Duration::from_secs(2));
    da.state.recent_tags = vec!["from-a".into()];
    da.save().unwrap();
    std::thread::sleep(Duration::from_millis(1500));
    let mut db = VaultData::load(&p.b).0;
    db.state.recent_tags = vec!["from-b".into()];
    db.save().unwrap();
    std::thread::sleep(Duration::from_secs(2));
    st("resume");
    let shale = |d: &Path| names(&d.join(".shale"));
    wait_until(90, "a conflict file in .shale/ on both devices", || [&p.a, &p.b].iter().all(|d| shale(d).iter().any(|n| n.contains("sync-conflict"))));
    observe("syncthing", &format!(".shale/ after both devices saved the state: {:?}", shale(&p.a)));
    // Shale loads what is left at state.json without a problem, and the conflict file is not in the vault
    for d in [&p.a, &p.b] {
        let (data, problems) = VaultData::load(d);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(data.state.recent_tags.len(), 1);
        assert!(Vault::open(d).unwrap().files.is_empty(), "the files in .shale/ are not notes");
    }
}

// spec: NET-016
#[test]
#[ignore = "needs the Syncthing pair: tests/syncthing-env/syncthing-env.sh up"]
fn syncthing_env_device_local_state_of_a_synced_vault_leaves_no_conflict_file() {
    crate::env_lock::hold();
    let p = Pair::new("local");
    let bases = (std::env::temp_dir().join(format!("shale-net016-a-{}", std::process::id())), std::env::temp_dir().join(format!("shale-net016-b-{}", std::process::id())));
    // the ID file of A reaches B, so both devices name the same vault; each keeps its own folder
    assert_eq!(crate::device_local::place(&p.a, true, &bases.0), None);
    wait_until(60, "the vault ID to reach B", || p.b.join(crate::gitsync::history::ID_FILE).is_file());
    assert_eq!(crate::device_local::place(&p.b, true, &bases.1), None);
    st("pause");
    let mut da = VaultData::load(&p.a).0;
    da.state.recent_tags = vec!["from-a".into()];
    da.save().unwrap();
    let mut db = VaultData::load(&p.b).0;
    db.state.recent_tags = vec!["from-b".into()];
    db.save().unwrap();
    st("resume");
    std::thread::sleep(Duration::from_secs(15));
    for d in [&p.a, &p.b] {
        let shale = names(&d.join(".shale"));
        observe("syncthing", &format!(".shale/ after both devices saved the state: {shale:?}"));
        assert!(shale.iter().all(|n| !n.starts_with("state")), "no state file travels: {shale:?}");
    }
    assert_eq!(VaultData::load(&p.a).0.state.recent_tags, ["from-a"]);
    assert_eq!(VaultData::load(&p.b).0.state.recent_tags, ["from-b"]);
    let _ = fs::remove_dir_all(&bases.0);
    let _ = fs::remove_dir_all(&bases.1);
}

// spec: SYNC-060, NET-010
#[test]
#[ignore = "needs the Syncthing pair: tests/syncthing-env/syncthing-env.sh up"]
fn syncthing_env_detect_sees_a_synced_folder_by_its_marker_and_warns_that_a_repository_does_not_belong_there() {
    use crate::gitsync::detect::{detect_real, repo_warning, Cloud, Kind};
    crate::env_lock::hold();
    let (a, b) = pair();
    for dir in [&a, &b] {
        let found = detect_real(dir);
        assert!(found.iter().any(|f| f.kind == Kind::Cloud(Cloud::Syncthing)), "{found:?}");
        assert!(repo_warning(&found, false).unwrap().contains("a Syncthing folder"));
    }
    // a vault folder inside it is found through its parent
    let p = Pair::new("detect");
    let found = detect_real(&p.a);
    assert!(found.iter().any(|f| f.kind == Kind::Cloud(Cloud::Syncthing)), "{found:?}");
}
