//! Busy states (JOB-003…JOB-010, #397): what runs in the background right now, so that every
//! view can say what is happening, a button or key cannot start the same action twice, and the
//! corner indicator can list the work and offer Cancel.
//!
//! A [`Busy`] is a cheap handle on one shared registry (the window owns it; clone it into anything
//! that starts work, also before the window exists). An action starts a [`Task`] with a [`Key`]
//! (its identity: "git sync", "vault check") and a text ("Syncing with git…"). While the task
//! lives, the registry refuses a second one with the same key, the views of the action ask
//! [`Busy::info`] for its text, and the indicator lists it. A task ends when its value is dropped:
//! [`Busy::spawn`] and [`Task::run`] start the job through [`crate::jobs`] and drop the task
//! before the result can be received (also after a panic), so a result that has arrived always
//! finds the action idle.

use crate::jobs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

mod demo;
pub mod ui;

#[cfg(test)]
mod tests;

/// What a task is: the action that runs. Two running tasks never share a key.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    kind: &'static str,
    quiet: bool,
}

/// How long a quiet task runs before the indicator lists it (JOB-030).
pub const QUIET_FOR: Duration = Duration::from_secs(2);

impl Key {
    /// The key of an action, as a constant of the module that owns the action.
    pub const fn new(kind: &'static str) -> Key {
        Key { kind, quiet: false }
    }

    /// The key of housekeeping that is usually over in a moment (a snapshot of the file history):
    /// the views of the action see the task at once ([`Busy::info`]), but the indicator lists it
    /// only when it has run for [`QUIET_FOR`], so it does not flicker (JOB-030).
    pub const fn quiet(kind: &'static str) -> Key {
        Key { kind, quiet: true }
    }
}

/// The request to stop, shared by the window, the indicator and the job, which looks at it between
/// two steps of its work.
#[derive(Clone, Debug, Default)]
pub struct Cancel(Arc<AtomicBool>);

impl Cancel {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// What the registry knows of a running task.
#[derive(Clone, Debug)]
pub struct TaskInfo {
    pub id: u64,
    /// What is happening, as a short phrase that ends in "…".
    pub label: String,
    pub started: Instant,
    /// Cancel can stop the work.
    pub cancellable: bool,
    /// Cancel was asked for and the work has not ended yet.
    pub cancelling: bool,
}

struct Entry {
    id: u64,
    key: Key,
    label: String,
    started: Instant,
    cancel: Option<Cancel>,
}

#[derive(Default)]
struct Tasks {
    next: u64,
    list: Vec<Entry>,
}

/// The registry of running tasks (a handle: clones share it).
#[derive(Clone, Default)]
pub struct Busy(Arc<Mutex<Tasks>>);

impl Busy {
    fn tasks(&self) -> MutexGuard<'_, Tasks> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Register a task. `None`: a task with this key already runs, and the action must not start
    /// a second time. Whoever holds the [`Task`] keeps it listed.
    pub fn begin(&self, key: Key, label: impl Into<String>) -> Option<Task> {
        let mut tasks = self.tasks();
        if tasks.list.iter().any(|e| e.key == key) {
            return None;
        }
        let id = tasks.next;
        tasks.next += 1;
        tasks.list.push(Entry { id, key, label: label.into(), started: Instant::now(), cancel: None });
        Some(Task { busy: self.clone(), id, cancel: None })
    }

    /// Run `f` as a task that cannot be stopped. `None`: the action already runs. `notify` is
    /// called when the job has ended (to repaint the window).
    pub fn spawn<T: Send + 'static>(&self, key: Key, label: impl Into<String>, f: impl FnOnce() -> T + Send + 'static, notify: impl FnOnce() + Send + 'static) -> Option<Job<T>> {
        Some(self.begin(key, label)?.run(f, notify))
    }

    /// Run `f` as a task that Cancel can stop: `f` looks at the token between two steps and ends early.
    pub fn spawn_cancellable<T: Send + 'static>(&self, key: Key, label: impl Into<String>, f: impl FnOnce(&Cancel) -> T + Send + 'static, notify: impl FnOnce() + Send + 'static) -> Option<Job<T>> {
        let mut task = self.begin(key, label)?;
        let cancel = task.cancellable();
        Some(task.run(move || f(&cancel), notify))
    }

    /// The task of this action, if it runs.
    pub fn info(&self, key: &Key) -> Option<TaskInfo> {
        self.tasks().list.iter().find(|e| e.key == *key).map(Entry::info)
    }

    pub fn is_running(&self, key: &Key) -> bool {
        self.tasks().list.iter().any(|e| e.key == *key)
    }

    /// Every running task, the oldest first, except a quiet one that has not run for [`QUIET_FOR`] yet.
    pub fn list(&self) -> Vec<TaskInfo> {
        self.tasks().list.iter().filter(|e| !e.key.quiet || e.started.elapsed() >= QUIET_FOR).map(Entry::info).collect()
    }

    /// Ask the task to stop; true when it can be stopped. It stays listed until its job has ended.
    pub fn cancel(&self, id: u64) -> bool {
        let tasks = self.tasks();
        let Some(cancel) = tasks.list.iter().find(|e| e.id == id).and_then(|e| e.cancel.as_ref()) else { return false };
        cancel.cancel();
        true
    }
}

impl Entry {
    fn info(&self) -> TaskInfo {
        TaskInfo { id: self.id, label: self.label.clone(), started: self.started, cancellable: self.cancel.is_some(), cancelling: self.cancel.as_ref().is_some_and(Cancel::is_cancelled) }
    }
}

/// A registered task: listed as long as this value lives.
#[must_use = "a task is listed only while it is held"]
pub struct Task {
    busy: Busy,
    id: u64,
    cancel: Option<Cancel>,
}

impl Task {
    /// Make the task one that Cancel can stop, and hand out the token its work looks at.
    pub fn cancellable(&mut self) -> Cancel {
        let cancel = self.cancel.get_or_insert_with(Cancel::default).clone();
        if let Some(e) = self.busy.tasks().list.iter_mut().find(|e| e.id == self.id) {
            e.cancel = Some(cancel.clone());
        }
        cancel
    }

    /// Run `f` on a new thread under this task: the task ends before the job's result can be
    /// received, also when `f` panics (the result is then an error message). `notify` is called
    /// after the result is sent.
    pub fn run<T: Send + 'static>(self, f: impl FnOnce() -> T + Send + 'static, notify: impl FnOnce() + Send + 'static) -> Job<T> {
        let (tx, rx) = channel();
        let cancel = self.cancel.clone();
        jobs::run(f, move |r| {
            drop(self);
            let _ = tx.send(r);
            notify();
        });
        Job { rx, cancel }
    }
}

impl Drop for Task {
    fn drop(&mut self) {
        self.busy.tasks().list.retain(|e| e.id != self.id);
    }
}

/// A job that runs under a task: where its result arrives, and how to stop it.
pub struct Job<T> {
    rx: Receiver<Result<T, String>>,
    cancel: Option<Cancel>,
}

impl<T> Job<T> {
    /// The result if the job has ended: its value, or the message of a panic. `Empty` while it runs.
    pub fn try_recv(&self) -> Result<Result<T, String>, TryRecvError> {
        self.rx.try_recv()
    }

    /// Wait for the result for at most `limit` (the window that closes waits for the last job a moment).
    pub fn recv_timeout(&self, limit: Duration) -> Result<Result<T, String>, RecvTimeoutError> {
        self.rx.recv_timeout(limit)
    }

    /// Ask the job to stop (nothing happens for a task that cannot be stopped).
    pub fn cancel(&self) {
        if let Some(c) = &self.cancel {
            c.cancel();
        }
    }
}
