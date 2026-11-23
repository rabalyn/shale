//! Tests: which background jobs of the window are still running (DEV-041). A job that ends after
//! the test looked (the tag words written to `.shale/tag-words.json`, a rescan that applies its
//! result, a vault check) changes a file or a state in the middle of the test, and only on a
//! loaded machine. `Headless::settle` (and `ui_test_support::settle`) frame until this list is
//! empty.

use super::ShaleApp;

impl ShaleApp {
    /// What runs in the background (or is about to start with the next frame), by name.
    pub(crate) fn busy_jobs(&self) -> Vec<&'static str> {
        let jobs = [
            ("the tag learner", self.tag_learner.pending(self.vault.as_ref())),
            ("the loading of a vault", self.vault_load.is_loading()),
            ("the look-up of the vault folders", self.presence.pending()),
            ("the rescan of the vault", self.rescan.in_flight()),
            ("the vault check", self.vault_check.running()),
            ("the data usage jobs", self.usage.any_running()),
            ("the file history", self.history.busy()),
            ("git sync", self.sync.running() || self.sync.lfs_job_running()),
            ("the pandoc look-up", self.word.running()),
            ("the update check", self.update.busy()),
            ("the copy of outside files", self.outside_copy.is_some()),
            ("the opening of outside files", !self.outside_opens.is_empty()),
        ];
        jobs.into_iter().filter(|(_, busy)| *busy).map(|(name, _)| name).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{tests::test_app, Options};
    use crate::ui_test_support::{settle_within, Headless};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    // spec: DEV-041
    #[test]
    fn dev_041_settle_waits_until_the_tag_words_are_written() {
        let (mut app, d) = test_app("settle-tag-words");
        assert_eq!(app.busy_jobs(), ["the tag learner"], "learning is due with the first frame");
        let mut hl = Headless::new();
        hl.settle(&mut app);
        assert!(app.busy_jobs().is_empty());
        assert!(crate::import::tagwords::file_in(&d).exists(), "the words are in the vault when the window has settled");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DEV-041
    #[test]
    fn dev_041_settle_waits_for_a_rescan_and_names_a_job_that_does_not_end() {
        let (mut app, d) = test_app("settle-rescan");
        let mut hl = Headless::new();
        hl.settle(&mut app);
        let release = Arc::new(AtomicBool::new(false));
        let gate = release.clone();
        app.rescan.with_runner(move |input, probes| {
            while !gate.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            crate::app::rescan::scan(input, probes)
        });
        app.rescan.due_now();
        let stuck = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            settle_within(Duration::from_millis(300), || {
                hl.frame(&mut app, vec![]);
                app.busy_jobs()
            })
        }));
        let message = *stuck.expect_err("a scan that is held back does not settle").downcast::<String>().unwrap();
        assert!(message.contains("still running") && message.contains("the rescan of the vault"), "{message}");
        release.store(true, Ordering::SeqCst);
        hl.settle(&mut app);
        assert!(!app.rescan.in_flight());
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DEV-041
    #[test]
    fn dev_041_a_window_without_a_vault_has_no_job() {
        let app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        assert!(app.busy_jobs().is_empty());
    }
}
