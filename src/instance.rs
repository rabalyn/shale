//! Only one Shale runs per user: a second one would send a queued mail again, undo the first
//! one's "sending" state and overwrite its settings.

use std::fs::File;
use std::path::Path;

/// Take the lock in `dir`. `Ok(Some(_))`: keep the value until the process ends. `Ok(None)`:
/// another Shale is running.
pub fn acquire(dir: &Path) -> std::io::Result<Option<File>> {
    crate::fsutil::try_lock_file(&dir.join("instance.lock"))
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
    match acquire(dir) {
        Ok(Some(lock)) => Claim::Owner(lock),
        Ok(None) => Claim::AlreadyRunning,
        Err(e) => Claim::Unchecked(e),
    }
}

/// The folder of the lock: next to the mail store, under the user's data folder.
pub fn default_dir() -> std::path::PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale")
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
        let first = acquire(&dir.0).unwrap();
        assert!(first.is_some());
        assert!(acquire(&dir.0).unwrap().is_none());
        drop(first);
        assert!(acquire(&dir.0).unwrap().is_some());
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
            let lock = acquire(std::path::Path::new(&dir)).unwrap();
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

    // spec: DATA-012
    #[test]
    fn the_lock_lives_under_the_users_data_folder() {
        assert!(default_dir().ends_with("shale"));
        assert!(ALREADY_RUNNING.contains("already running"));
    }
}
