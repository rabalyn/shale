//! Changes made in another client show up within seconds (MAIL-230 to MAIL-234, raba/shale#227).
//!
//! While the mail view is open, the folder that is open and each account's Inbox are watched, each on
//! a connection of its own that only reads (`EXAMINE`, never `SELECT`; ADR-0009). The connection
//! sits in `IDLE` (RFC 2177) and the server tells it when something in the folder changed; then
//! the folder gets a quick check with the same code as the sync of that folder
//! (`imap_sync::check_folder`: new mail, changed flags, mail gone from the server), and the
//! connection waits again. A server without `IDLE` is asked every minute instead.
//!
//! The quick check never runs beside a full sync of the same account (`imap_sync::SyncGuard`):
//! it waits a moment and tries again, and the full sync waits for a check that is already running.
//! A connection that breaks is opened again after a pause that grows to five minutes; nothing is
//! said to the person for that. A refused sign-in is not tried again until the account changes or
//! the mail view is left and opened again (a wrong password is not tried over and over).

use crate::config::MailAccount;
use crate::mail::imap_client::{IdleEnd, ImapError};
use crate::mail::imap_sync::{check_folder, Connector, SyncOptions};
use crate::mail::store::MailStore;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

/// A folder to keep fresh: its account (as configured), the folder's name as the server spells
/// it, and whether the person typed a password for this session (a changed one needs a new connection).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub account: MailAccount,
    pub folder: String,
    pub session_password: bool,
}

/// How a watched folder behaves; the defaults are what the app uses, tests shorten them.
#[derive(Clone, Debug)]
pub struct Timing {
    /// Longest time in one `IDLE` (servers drop it after 30 minutes).
    pub idle_max: Duration,
    /// Time between two looks at a folder of a server without `IDLE`.
    pub poll: Duration,
    /// Pause before the first new try after an error, and the longest pause.
    pub backoff_first: Duration,
    pub backoff_max: Duration,
    /// Pause before a quick check that had to give way to a full sync is tried again.
    pub busy_retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Timing { idle_max: Duration::from_secs(25 * 60), poll: Duration::from_secs(60), backoff_first: Duration::from_secs(5), backoff_max: Duration::from_secs(300), busy_retry: Duration::from_secs(2) }
    }
}

/// What a watched folder reports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A quick check ran: new messages, and messages whose flags or presence changed.
    Checked { account: String, folder: String, added: usize, changed: usize },
    /// The connection or the check failed (it is tried again unless the sign-in was refused).
    Failed { account: String, folder: String, message: String },
}

struct Running {
    wanted: Wanted,
    stop: Arc<AtomicBool>,
}

/// The watched folders of the mail view.
pub struct Watch {
    /// False: nothing is watched (tests that count the connections of the server switch it off).
    pub enabled: bool,
    pub timing: Timing,
    running: HashMap<(String, String), Running>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    /// Quick checks going on right now.
    checking: Arc<AtomicUsize>,
    /// The last thing that went wrong per account and folder, until a check works again.
    pub errors: HashMap<(String, String), String>,
}

impl Default for Watch {
    fn default() -> Self {
        let (tx, rx) = channel();
        Watch { enabled: true, timing: Timing::default(), running: HashMap::new(), tx, rx, checking: Arc::new(AtomicUsize::new(0)), errors: HashMap::new() }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop_all();
    }
}

impl Watch {
    /// Watch exactly `wanted`: connections of folders no longer wanted (or of a changed account) end,
    /// new ones start. `connector_for` makes the connector of an account.
    pub fn update(&mut self, wanted: &[Wanted], store: &Arc<MailStore>, connector_for: &dyn Fn(&MailAccount) -> Arc<dyn Connector>, opts: &SyncOptions, ctx: Option<&eframe::egui::Context>) {
        let key = |w: &Wanted| (w.account.id.clone(), w.folder.clone());
        let keep: Vec<(String, String)> = wanted.iter().map(key).collect();
        self.running.retain(|k, r| {
            let same = wanted.iter().any(|w| key(w) == *k && *w == r.wanted);
            if !same {
                r.stop.store(true, Ordering::SeqCst);
            }
            same
        });
        self.errors.retain(|k, _| keep.contains(k));
        for w in wanted {
            if self.running.contains_key(&key(w)) {
                continue;
            }
            let stop = Arc::new(AtomicBool::new(false));
            let job = Job {
                store: store.clone(),
                connector: connector_for(&w.account),
                account: w.account.id.clone(),
                folder: w.folder.clone(),
                opts: opts.clone(),
                timing: self.timing.clone(),
                stop: stop.clone(),
                tx: self.tx.clone(),
                checking: self.checking.clone(),
                ctx: ctx.cloned(),
            };
            if std::thread::Builder::new().name("mail-watch".into()).spawn(move || run(job)).is_ok() {
                self.running.insert(key(w), Running { wanted: w.clone(), stop });
            }
        }
    }

    pub fn stop_all(&mut self) {
        for (_, r) in self.running.drain() {
            r.stop.store(true, Ordering::SeqCst);
        }
        self.errors.clear();
    }

    /// An account was removed or changed: its connections end.
    pub fn stop_account(&mut self, account: &str) {
        self.running.retain(|k, r| {
            let keep = k.0 != account;
            if !keep {
                r.stop.store(true, Ordering::SeqCst);
            }
            keep
        });
        self.errors.retain(|k, _| k.0 != account);
    }

    /// Is a quick check writing to the index right now?
    pub fn checking(&self) -> bool {
        self.checking.load(Ordering::SeqCst) > 0
    }

    /// The folders being watched: (account, folder).
    #[cfg(test)]
    pub fn watching(&self) -> Vec<(String, String)> {
        let mut v: Vec<_> = self.running.keys().cloned().collect();
        v.sort();
        v
    }

    /// Take in what the watchers reported. True when mail or flags changed (the lists must be read again).
    pub fn drain(&mut self) -> bool {
        let mut changed = false;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Checked { account, folder, added, changed: c } => {
                    self.errors.remove(&(account, folder));
                    changed |= added + c > 0;
                }
                Event::Failed { account, folder, message } => {
                    if self.running.contains_key(&(account.clone(), folder.clone())) {
                        self.errors.insert((account, folder), message);
                    }
                }
            }
        }
        changed
    }
}

// ---------------------------------------------------------------- the thread of one folder

struct Job {
    store: Arc<MailStore>,
    connector: Arc<dyn Connector>,
    account: String,
    folder: String,
    opts: SyncOptions,
    timing: Timing,
    stop: Arc<AtomicBool>,
    tx: Sender<Event>,
    checking: Arc<AtomicUsize>,
    ctx: Option<eframe::egui::Context>,
}

impl Job {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn tell(&self, ev: Event) {
        if self.stopped() {
            return;
        }
        let _ = self.tx.send(ev);
        if let Some(c) = &self.ctx {
            c.request_repaint();
        }
    }

    /// Wait, looking at the stop flag several times a second.
    fn nap(&self, d: Duration) {
        let end = std::time::Instant::now() + d;
        while !self.stopped() {
            let left = end.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return;
            }
            std::thread::sleep(left.min(Duration::from_millis(50)));
        }
    }
}

/// How a session of one connection ended.
enum Ended {
    Stopped,
    /// Try again after a pause.
    Broken(String),
    /// Do not try again: the sign-in was refused.
    SignIn(String),
}

fn run(job: Job) {
    let mut pause = job.timing.backoff_first;
    while !job.stopped() {
        let ended = crate::jobs::catch(|| session(&job, &mut pause)).unwrap_or_else(Ended::Broken);
        match ended {
            Ended::Stopped => return,
            Ended::SignIn(message) => {
                job.tell(Event::Failed { account: job.account.clone(), folder: job.folder.clone(), message });
                return;
            }
            Ended::Broken(message) => {
                job.tell(Event::Failed { account: job.account.clone(), folder: job.folder.clone(), message });
                job.nap(pause);
                pause = (pause * 2).min(job.timing.backoff_max);
            }
        }
    }
}

/// One connection: check the folder, wait for a change (or the time to look again), check again.
fn session(job: &Job, pause: &mut Duration) -> Ended {
    let mut client = match job.connector.connect() {
        Ok(c) => c,
        Err(ImapError::Auth(m)) => return Ended::SignIn(m),
        Err(e) => return Ended::Broken(e.to_string()),
    };
    let mut idle = client.has_idle();
    let mut check = true;
    let ended = loop {
        if job.stopped() {
            break Ended::Stopped;
        }
        if check {
            job.checking.fetch_add(1, Ordering::SeqCst);
            let checked = check_folder(&job.store, &mut client, &job.account, &job.folder, &job.opts);
            job.checking.fetch_sub(1, Ordering::SeqCst);
            match checked {
                Ok(Some((added, changed))) => {
                    *pause = job.timing.backoff_first;
                    job.tell(Event::Checked { account: job.account.clone(), folder: job.folder.clone(), added, changed });
                }
                Ok(None) => {
                    job.nap(job.timing.busy_retry); // a full sync of the account is going on
                    continue;
                }
                Err(e) => break Ended::Broken(e),
            }
        }
        check = true;
        if idle {
            match client.idle(job.timing.idle_max, &|| job.stopped()) {
                Ok(IdleEnd::Changed) => {}
                Ok(IdleEnd::Elapsed) => check = false,
                Ok(IdleEnd::Stopped) => break Ended::Stopped,
                // this server does not want an IDLE here: ask it from time to time instead
                Err(ImapError::Server(_)) => idle = false,
                Err(e) => break Ended::Broken(e.to_string()),
            }
        } else {
            job.nap(job.timing.poll);
        }
    };
    client.logout();
    ended
}
