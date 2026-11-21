//! The lock of the podman test stacks (DEV-034), against lock files of their own: the real one is
//! never touched.

use crate::env_lock::{acquire, held_by_parent};
use std::fs::{File, OpenOptions, TryLockError};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn lock_file(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-env-lock-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    let _ = std::fs::remove_file(&path);
    path
}

fn try_lock_by_a_second_open(path: &Path) -> Result<File, TryLockError> {
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).unwrap();
    file.try_lock().map(|()| file)
}

// spec: DEV-034
#[test]
fn dev_034_the_holder_takes_the_lock_and_a_second_open_cannot_while_it_is_held() {
    let path = lock_file("held");
    let (file, token) = acquire(&path, None).expect("nobody held it: taken");
    assert!(matches!(try_lock_by_a_second_open(&path), Err(TryLockError::WouldBlock)), "a second open must not get the lock");
    assert_eq!(std::fs::read_to_string(&path).unwrap().trim_end(), token, "the token is in the file");
    drop(file);
    assert!(try_lock_by_a_second_open(&path).is_ok(), "released when the holder ends");
}

// spec: DEV-034
#[test]
fn dev_034_a_process_started_by_the_holder_with_its_token_does_not_wait_for_it() {
    let path = lock_file("nested");
    let (_holder, token) = acquire(&path, None).unwrap();
    assert!(held_by_parent(&path, Some(&token)));
    // would block for ever if it waited for the holder, which is this very thread
    assert!(acquire(&path, Some(&token)).is_none(), "the parent's token skips the lock");
}

// spec: DEV-034
#[test]
fn dev_034_a_stale_or_foreign_token_never_skips_the_lock() {
    let path = lock_file("stale");
    let (holder, token) = acquire(&path, None).unwrap();
    assert!(!held_by_parent(&path, Some("somebody-else")), "another holder's token");
    assert!(!held_by_parent(&path, Some("")) && !held_by_parent(&path, None), "no token");
    drop(holder);
    // the holder ended: its token is still in the file, but nobody holds the lock
    assert!(!held_by_parent(&path, Some(&token)), "a stale token");
    let (_second, fresh) = acquire(&path, Some(&token)).expect("the stale token did not skip the lock: it was taken");
    assert_ne!(fresh, token);
}

// spec: DEV-034
#[test]
fn dev_034_a_second_taker_waits_until_the_first_one_lets_go() {
    let path = lock_file("wait");
    let (holder, _) = acquire(&path, None).unwrap();
    let got = Arc::new(AtomicBool::new(false));
    let waiter = {
        let (path, got) = (path.clone(), got.clone());
        std::thread::spawn(move || {
            let taken = acquire(&path, None);
            got.store(true, Ordering::SeqCst);
            taken
        })
    };
    std::thread::sleep(Duration::from_millis(400));
    assert!(!got.load(Ordering::SeqCst), "it must still wait while the first holds the lock");
    drop(holder);
    let taken = waiter.join().unwrap();
    assert!(taken.is_some() && got.load(Ordering::SeqCst), "it got the lock once it was free");
}

// spec: DEV-034
#[test]
fn dev_034_every_ignored_env_test_takes_the_lock_in_its_first_lines() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|x| x == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut checked = 0;
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, _) in lines.iter().enumerate().filter(|(_, l)| l.trim_start().starts_with("#[ignore") && ["mail-env.sh", "caldav-env.sh", "tex-env.sh"].iter().any(|h| l.contains(h))) {
            let body: Vec<&str> = lines[i + 1..].iter().skip_while(|l| !l.contains("fn ")).skip(1).take(3).copied().collect();
            assert!(body.iter().any(|l| l.contains("env_lock::hold()") || l.contains("= turn()") || l.contains("Env::start()")), "{}:{}: an ignored env test takes the lock in its first lines, not {body:?}", file.display(), i + 1);
            checked += 1;
        }
    }
    assert!(checked >= 30, "only {checked} ignored env tests found");
}
