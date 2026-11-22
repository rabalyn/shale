//! Screenshots of the indicator (JOB-014): a run with `SHALE_DEMO_BUSY=1` starts two made-up tasks
//! that run for the whole run, so the guide's picture shows the list without a real sync or export.

use super::{Busy, Key};
use std::time::{Duration, Instant};

/// How long the made-up tasks have run, so that the list says "1 min" in every run of a picture.
const AGE: Duration = Duration::from_secs(75);
/// A made-up task ends by itself after this long, if the run has not ended before.
const LIFETIME: Duration = Duration::from_secs(600);

impl Busy {
    /// Start the two made-up tasks: a sync that cannot be stopped and a PDF export that can.
    pub fn start_demo(&self) {
        let notify = || {};
        let _ = self.spawn(Key::new("demo-sync"), "Syncing with git…", || std::thread::sleep(LIFETIME), notify);
        let _ = self.spawn_cancellable(
            Key::new("demo-export"),
            "Exporting “Handbook” to PDF…",
            |cancel| {
                let end = Instant::now() + LIFETIME;
                while !cancel.is_cancelled() && Instant::now() < end {
                    std::thread::sleep(Duration::from_millis(100));
                }
            },
            notify,
        );
        for e in &mut self.tasks().list {
            e.started = Instant::now().checked_sub(AGE).unwrap_or(e.started);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: JOB-014
    #[test]
    fn job_014_the_demo_starts_a_task_that_cannot_stop_and_one_that_can_and_both_look_a_minute_old() {
        let busy = Busy::default();
        busy.start_demo();
        let tasks = busy.list();
        assert_eq!(tasks.iter().map(|t| (t.label.as_str(), t.cancellable)).collect::<Vec<_>>(), [("Syncing with git…", false), ("Exporting “Handbook” to PDF…", true)]);
        assert!(tasks.iter().all(|t| (60..120).contains(&t.started.elapsed().as_secs())));
        let export = tasks.iter().find(|t| t.cancellable).unwrap();
        assert!(busy.cancel(export.id));
        let start = Instant::now();
        while busy.info(&Key::new("demo-export")).is_some() {
            assert!(start.elapsed() < Duration::from_secs(10), "the made-up export stops when it is cancelled");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
