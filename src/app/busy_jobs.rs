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
            ("a close or switch waiting for the sync", self.pending_leave.is_some()),
            ("git sync", self.sync.running() || self.sync.lfs_job_running()),
            ("the pandoc look-up", self.word.running()),
            ("the update check", self.update.busy()),
            ("the copy of outside files", self.outside_copy.is_some()),
            ("the opening of outside files", !self.outside_opens.is_empty()),
            ("the search", self.search_worker.busy()),
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

    // spec: QUERY-020
    #[test]
    fn query_020_settle_waits_for_the_search_worker_and_a_test_sees_the_result() {
        static RELEASE: AtomicBool = AtomicBool::new(false);
        fn held(_: &mut crate::search_all::worker::Conns, _: &crate::search_all::Request, _: &AtomicBool) -> Option<crate::search_all::Found> {
            while !RELEASE.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Some(crate::search_all::Found::default())
        }
        let (mut app, d) = test_app("settle-search");
        let mut hl = Headless::new();
        hl.settle(&mut app);
        // a search that is held back is named
        app.search_worker = crate::search_all::SearchWorker::with_runner(held);
        app.side.left = crate::app::LeftPanel::Search;
        app.search.query = "markdown".into();
        std::thread::sleep(crate::search_all::DEBOUNCE + Duration::from_millis(30));
        let stuck = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            settle_within(Duration::from_millis(300), || {
                hl.frame(&mut app, vec![]);
                app.busy_jobs()
            })
        }));
        let message = *stuck.expect_err("a search that is held back does not settle").downcast::<String>().unwrap();
        assert!(message.contains("still running") && message.contains("the search"), "{message}");
        RELEASE.store(true, Ordering::SeqCst);
        hl.settle(&mut app);
        assert!(app.busy_jobs().is_empty());
        // the real worker: after settle the list of the search is there
        app.search_worker = Default::default();
        app.search.query = "vault".into();
        std::thread::sleep(crate::search_all::DEBOUNCE + Duration::from_millis(30));
        hl.settle(&mut app);
        assert!(!app.search.listed().is_empty(), "the result of the search is in the list");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: JOB-038
    #[test]
    fn job_038_the_search_of_the_window_is_a_task_of_its_registry() {
        static RELEASE: AtomicBool = AtomicBool::new(false);
        fn held(_: &mut crate::search_all::worker::Conns, _: &crate::search_all::Request, _: &AtomicBool) -> Option<crate::search_all::Found> {
            while !RELEASE.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Some(crate::search_all::Found::default())
        }
        let key = crate::search_all::worker::SEARCH.with("sidebar");
        let (mut app, d) = test_app("search-task");
        let mut hl = Headless::new();
        hl.settle(&mut app);
        // the window's own worker is wired to its registry
        app.search_worker = crate::search_all::SearchWorker::with_runner(held).with_busy(&app.busy);
        app.side.left = crate::app::LeftPanel::Search;
        app.search.query = "markdown".into();
        std::thread::sleep(crate::search_all::DEBOUNCE + Duration::from_millis(30));
        hl.frame(&mut app, vec![]);
        let task = app.busy.info(&key).expect("the query is a task");
        assert_eq!(task.label, "Searching…");
        assert!(app.busy.list_at(task.started).is_empty(), "quiet: not listed before 2 s");
        RELEASE.store(true, Ordering::SeqCst);
        hl.settle(&mut app);
        assert!(!app.busy.is_running(&key), "the task is gone when settled");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: DEV-041
    #[test]
    fn dev_041_a_window_without_a_vault_has_no_job() {
        let app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        assert!(app.busy_jobs().is_empty());
    }
}
