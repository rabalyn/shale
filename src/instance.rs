//! Only one Shale runs per user: a second one would send a queued mail again, undo the first
//! one's "sending" state and overwrite its settings. A second start does not just fail: it hands
//! its folder or file to the running Shale (DATA-015) over a socket on the loopback interface.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

/// How long a start that finds the lock taken keeps trying before it says "already running". A
/// program started just before (git, gpg, the opener) holds a copy of the locked file from its
/// creation until it starts, a few milliseconds (5 ms at most in a test that started programs
/// without pause), so the lock can outlive the Shale that took it by that long.
pub const GRACE: Duration = Duration::from_millis(200);

/// Take the lock in `dir`, trying again for up to `grace` while it is taken. `Ok(Some(_))`: keep
/// the value until the process ends. `Ok(None)`: another Shale is running.
pub fn acquire(dir: &Path, grace: Duration) -> std::io::Result<Option<File>> {
    let path = dir.join("instance.lock");
    let start = Instant::now();
    loop {
        if let Some(lock) = crate::fsutil::try_lock_file(&path)? {
            return Ok(Some(lock));
        }
        if start.elapsed() >= grace {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// What a start finds out about other running Shales.
pub enum Claim {
    /// this one is the only one: keep the lock until the process ends
    Owner(File),
    /// another Shale holds the lock
    AlreadyRunning,
    /// the lock could not be checked (broken data folder): start anyway, say why
    Unchecked(std::io::Error),
}

/// `acquire` as `main` uses it: every outcome is a way to go on.
pub fn claim(dir: &Path) -> Claim {
    match acquire(dir, GRACE) {
        Ok(Some(lock)) => Claim::Owner(lock),
        Ok(None) => Claim::AlreadyRunning,
        Err(e) => Claim::Unchecked(e),
    }
}

/// The folder of the lock: next to the mail store, under the user's data folder.
pub fn default_dir() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale")
}

/// Where the running Shale says how to reach it: its port and a secret, owner-only (DATA-014).
const HANDOVER_FILE: &str = "instance.port";
/// What a message may be at most: a handful of paths.
const MAX_MESSAGE: u64 = 64 * 1024;
/// How long a second start keeps trying to reach the first (which may just be starting up).
const REACH: Duration = Duration::from_secs(2);

/// What the running Shale listens to: the paths that later starts handed over, one list per start.
pub struct Handover {
    rx: Receiver<Vec<PathBuf>>,
    /// set by the window on its first frame, so a message can wake it
    wake: Arc<OnceLock<eframe::egui::Context>>,
}

impl Handover {
    /// The lists of paths that arrived since the last call (an empty list: just "come forward").
    pub fn take(&self) -> Vec<Vec<PathBuf>> {
        self.rx.try_iter().collect()
    }

    /// Give the listener the window to wake.
    pub fn attach(&self, ctx: &eframe::egui::Context) {
        let _ = self.wake.set(ctx.clone());
    }
}

/// A secret for this run: eight random words from the system-seeded hasher, no dependency needed.
fn secret() -> String {
    use std::hash::{BuildHasher, Hasher};
    (0..4).map(|_| format!("{:016x}", std::collections::hash_map::RandomState::new().build_hasher().finish())).collect()
}

/// Start listening in `dir` (the owner calls this once, after it took the lock).
pub fn listen(dir: &Path) -> std::io::Result<Handover> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let port = listener.local_addr()?.port();
    let token = secret();
    crate::fsutil::write_atomic_private(&dir.join(HANDOVER_FILE), format!("{port}\n{token}\n").as_bytes())?;
    let (tx, rx) = channel();
    let wake: Arc<OnceLock<eframe::egui::Context>> = Arc::new(OnceLock::new());
    let wake_in = wake.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            if let Some(paths) = read_message(stream, &token) {
                if tx.send(paths).is_err() {
                    return;
                }
                if let Some(ctx) = wake_in.get() {
                    ctx.request_repaint();
                }
            }
        }
    });
    Ok(Handover { rx, wake })
}

/// One message: a line of JSON `{"token": …, "paths": […]}`. Anything else (wrong secret, too
/// long, too slow, not JSON) is dropped without an answer.
fn read_message(mut stream: TcpStream, token: &str) -> Option<Vec<PathBuf>> {
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    let mut line = String::new();
    BufReader::new((&stream).take(MAX_MESSAGE)).read_line(&mut line).ok()?;
    let v: serde_json::Value = serde_json::from_str(&line).ok()?;
    if v.get("token")?.as_str()? != token {
        return None;
    }
    let paths = v.get("paths")?.as_array()?.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect();
    stream.write_all(b"ok\n").ok()?;
    Some(paths)
}

/// Hand `paths` (made absolute here, the other Shale has another working folder) to the Shale
/// that runs. `true` when it took them. Tries for a moment: the first Shale may not have opened
/// its socket yet, and a file left by a crash names a port nobody listens on.
pub fn hand_over(dir: &Path, paths: &[PathBuf]) -> bool {
    let paths: Vec<String> = paths.iter().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()).to_string_lossy().into_owned()).collect();
    let start = Instant::now();
    loop {
        if try_hand_over(dir, &paths) {
            return true;
        }
        if start.elapsed() >= REACH {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn try_hand_over(dir: &Path, paths: &[String]) -> bool {
    let Ok(text) = crate::fsutil::read_to_string(dir.join(HANDOVER_FILE)) else { return false };
    let mut lines = text.lines();
    let (Some(Ok(port)), Some(token)) = (lines.next().map(str::parse::<u16>), lines.next()) else { return false };
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, Duration::from_secs(1)) else { return false };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let msg = serde_json::json!({ "token": token, "paths": paths }).to_string();
    if stream.write_all(format!("{msg}\n").as_bytes()).is_err() {
        return false;
    }
    let mut answer = String::new();
    BufReader::new(&stream).read_line(&mut answer).is_ok() && answer.trim() == "ok"
}

pub const ALREADY_RUNNING: &str = "Shale is already running. Only one Shale can run at a time, so that mail is never sent twice and settings are not overwritten. Switch to the window that is open.";

/// A tiny window that tells the person, so a double click on the icon is not silent.
pub fn show_already_running() {
    eprintln!("{ALREADY_RUNNING}");
    let options = eframe::NativeOptions { viewport: eframe::egui::ViewportBuilder::default().with_title("Shale").with_app_id("shale").with_inner_size([460.0, 170.0]).with_resizable(false), ..Default::default() };
    // a failure to open the window is not worth more than the message on stderr
    let _ = eframe::run_ui_native("Shale", options, |ui, _frame| {
        ui.add_space(8.0);
        ui.label(ALREADY_RUNNING);
        ui.add_space(12.0);
        if ui.button("Quit").clicked() {
            ui.ctx().send_viewport_cmd(eframe::egui::ViewportCommand::Close);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// A data folder of its own for one test, removed at the end.
    struct Dir(std::path::PathBuf);

    impl Dir {
        fn new() -> Dir {
            Dir(std::env::temp_dir().join(format!("shale-instance-test-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))))
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    // spec: DATA-012
    #[test]
    fn a_second_instance_is_refused_until_the_first_has_ended() {
        let dir = Dir::new();
        let first = acquire(&dir.0, Duration::ZERO).unwrap();
        assert!(first.is_some());
        assert!(acquire(&dir.0, Duration::ZERO).unwrap().is_none());
        drop(first);
        // a program that another test started at this moment may still hold a copy for a few ms
        assert!(acquire(&dir.0, GRACE).unwrap().is_some());
    }

    // spec: DATA-012
    #[test]
    #[cfg(unix)]
    fn the_first_start_owns_and_the_second_is_told_another_is_running() {
        let dir = Dir::new();
        let Claim::Owner(first) = claim(&dir.0) else { panic!("the first start must be the owner") };
        assert!(matches!(claim(&dir.0), Claim::AlreadyRunning), "a second start while the first runs");
        assert!(matches!(claim(&dir.0), Claim::AlreadyRunning), "and a third");
        // two folders are two users: they do not disturb each other
        let other = Dir::new();
        assert!(matches!(claim(&other.0), Claim::Owner(_)));
        drop(first);
        assert!(matches!(claim(&dir.0), Claim::Owner(_)), "the lock is free when the owner has ended");
    }

    // spec: DATA-012
    #[test]
    fn a_lock_that_is_still_held_after_the_grace_time_means_already_running() {
        let dir = Dir::new();
        let Claim::Owner(_first) = claim(&dir.0) else { panic!("the first start must be the owner") };
        let t = Instant::now();
        assert!(matches!(claim(&dir.0), Claim::AlreadyRunning));
        assert!(t.elapsed() >= GRACE, "the second start tried for the whole grace time");
    }

    // spec: DATA-012
    #[test]
    fn a_lock_given_back_within_the_grace_time_is_taken() {
        let dir = Dir::new();
        let Claim::Owner(first) = claim(&dir.0) else { panic!("the first start must be the owner") };
        let ender = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(40));
            drop(first);
        });
        assert!(matches!(claim(&dir.0), Claim::Owner(_)), "the second start waited for the end of the first");
        ender.join().unwrap();
    }

    // spec: DATA-012
    #[test]
    #[cfg(unix)]
    fn a_lock_file_left_by_a_crashed_instance_is_taken_over() {
        let dir = Dir::new();
        std::fs::create_dir_all(&dir.0).unwrap();
        // what a crash leaves: the file, with a process number in it, and nobody holding the lock
        std::fs::write(dir.0.join("instance.lock"), "4194303\n").unwrap();
        assert!(matches!(claim(&dir.0), Claim::Owner(_)), "a stale file does not keep anyone out");
        // and a crash of a real owner: its process ends without cleaning up
        let exe = std::env::current_exe().unwrap();
        let crashed = std::process::Command::new(&exe).args(["--exact", "instance::tests::helper_that_takes_the_lock_and_dies", "--nocapture"]).env("SHALE_TEST_LOCK_DIR", &dir.0).status().unwrap();
        assert!(crashed.success(), "the helper process ran");
        assert!(matches!(claim(&dir.0), Claim::Owner(_)), "the lock went away with the process");
    }

    /// Not a test by itself: the helper process of the test above takes the lock and ends without
    /// giving it back (when run on its own, it does nothing).
    #[test]
    fn helper_that_takes_the_lock_and_dies() {
        if let Some(dir) = std::env::var_os("SHALE_TEST_LOCK_DIR") {
            let lock = acquire(std::path::Path::new(&dir), Duration::ZERO).unwrap();
            assert!(lock.is_some());
            std::mem::forget(lock);
        }
    }

    // spec: DATA-012
    #[test]
    fn a_data_folder_that_cannot_be_used_does_not_keep_shale_from_starting() {
        let dir = Dir::new();
        std::fs::write(&dir.0, "a file where the folder should be").unwrap();
        let Claim::Unchecked(e) = claim(&dir.0) else { panic!("the lock cannot be checked in a file") };
        assert!(!e.to_string().is_empty());
        // the same when the folder is below a file
        assert!(matches!(claim(&dir.0.join("deeper")), Claim::Unchecked(_)));
    }

    /// A message from a second start, as the running one gets it.
    fn deliver(dir: &Dir, paths: &[&str]) -> bool {
        hand_over(&dir.0, &paths.iter().map(PathBuf::from).collect::<Vec<_>>())
    }

    fn wait_for(h: &Handover) -> Vec<Vec<PathBuf>> {
        let t = Instant::now();
        loop {
            let got = h.take();
            if !got.is_empty() || t.elapsed() > Duration::from_secs(3) {
                return got;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    // spec: DATA-015
    #[test]
    fn a_second_start_hands_its_paths_to_the_running_one_in_order() {
        let dir = Dir::new();
        let Claim::Owner(_lock) = claim(&dir.0) else { panic!("owner") };
        let h = listen(&dir.0).unwrap();
        assert!(deliver(&dir, &["/vault/a", "/vault/a/Note ü.md"]));
        assert!(deliver(&dir, &[]));
        let mut got = wait_for(&h);
        if got.len() < 2 {
            got.extend(wait_for(&h));
        }
        assert_eq!(got, vec![vec![PathBuf::from("/vault/a"), PathBuf::from("/vault/a/Note ü.md")], vec![]]);
    }

    // spec: DATA-015
    #[test]
    fn a_relative_path_is_made_absolute_by_the_sender() {
        let dir = Dir::new();
        let _h = listen(&dir.0).unwrap_or_else(|_| {
            std::fs::create_dir_all(&dir.0).unwrap();
            listen(&dir.0).unwrap()
        });
        assert!(hand_over(&dir.0, &[PathBuf::from("notes/x.md")]));
        let got = wait_for(&_h);
        assert!(got[0][0].is_absolute() && got[0][0].ends_with("notes/x.md"), "{got:?}");
    }

    // spec: DATA-015
    #[test]
    fn a_message_with_the_wrong_secret_or_garbage_is_dropped() {
        let dir = Dir::new();
        std::fs::create_dir_all(&dir.0).unwrap();
        let h = listen(&dir.0).unwrap();
        let port: u16 = std::fs::read_to_string(dir.0.join(HANDOVER_FILE)).unwrap().lines().next().unwrap().parse().unwrap();
        for msg in ["{\"token\":\"nope\",\"paths\":[\"/x\"]}\n", "not json\n", "{\"paths\":[\"/x\"]}\n"] {
            let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            s.write_all(msg.as_bytes()).unwrap();
            let mut answer = String::new();
            BufReader::new(&s).read_line(&mut answer).unwrap();
            assert_eq!(answer, "", "no answer for {msg:?}");
        }
        assert!(h.take().is_empty());
    }

    // spec: DATA-015
    #[test]
    fn a_port_file_nobody_listens_to_means_the_hand_over_fails() {
        let dir = Dir::new();
        std::fs::create_dir_all(&dir.0).unwrap();
        // nothing at all
        assert!(!try_hand_over(&dir.0, &[]));
        // a crashed Shale's file: a port that is free now
        let free = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap().local_addr().unwrap().port();
        std::fs::write(dir.0.join(HANDOVER_FILE), format!("{free}\nsecret\n")).unwrap();
        assert!(!try_hand_over(&dir.0, &[]));
        std::fs::write(dir.0.join(HANDOVER_FILE), "garbage").unwrap();
        assert!(!try_hand_over(&dir.0, &[]));
    }

    // spec: DATA-012
    #[test]
    fn the_lock_lives_under_the_users_data_folder() {
        assert!(default_dir().ends_with("shale"));
        assert!(ALREADY_RUNNING.contains("already running"));
    }
}
