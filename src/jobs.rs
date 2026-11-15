//! Background jobs that always end: with a result, or with an error message if the job panicked.
//!
//! A bare `thread::spawn` that panics never reports back, so its spinner would run forever
//! (a malformed PDF can make the renderer panic, for example). Every slow job in Shale starts
//! through this module instead.

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::mpsc::{channel, Receiver};

/// Run `f` on a new thread and hand its outcome to `deliver`: `Ok(value)`, or `Err(message)`
/// if `f` panicked. `deliver` is called exactly once, also after a panic.
pub fn run<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static, deliver: impl FnOnce(Result<T, String>) + Send + 'static) {
    std::thread::spawn(move || deliver(catch(f)));
}

/// Run `f` on a new thread; the receiver gets `Ok(value)`, or `Err(message)` if it panicked.
/// `notify` is called after the result is sent (for example to repaint the window).
pub fn spawn<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static, notify: impl FnOnce() + Send + 'static) -> Receiver<Result<T, String>> {
    let (tx, rx) = channel();
    run(f, move |r| {
        let _ = tx.send(r);
        notify();
    });
    rx
}

/// Run `f` here, turning a panic into an error message.
pub fn catch<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|p| format!("the background job crashed: {}", panic_text(&*p)))
}

/// What a panic said.
fn panic_text(p: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown error".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // spec: JOB-002
    #[test]
    fn a_panicking_job_arrives_as_an_error() {
        let rx = spawn(|| -> u32 { panic!("bad pdf {}", 7) }, || {});
        let r = rx.recv_timeout(Duration::from_secs(5)).expect("the job must report back");
        let e = r.unwrap_err();
        assert!(e.contains("bad pdf 7"), "{e}");
        let rx = spawn(|| -> u32 { std::panic::panic_any(42u8) }, || {});
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().unwrap_err().contains("unknown error"));
    }

    // spec: JOB-002
    #[test]
    fn a_normal_job_returns_its_value_and_notifies() {
        let (ntx, nrx) = channel();
        let rx = spawn(|| 40 + 2, move || ntx.send(()).unwrap());
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), Ok(42));
        nrx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    // spec: JOB-002
    #[test]
    fn deliver_is_called_once_after_a_panic() {
        let (tx, rx) = channel();
        run(|| -> () { panic!("boom") }, move |r| tx.send(r).unwrap());
        assert!(rx.recv_timeout(Duration::from_secs(5)).unwrap().is_err());
        assert!(rx.recv_timeout(Duration::from_millis(100)).is_err(), "only one result");
    }
}
