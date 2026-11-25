//! The thread that searches (QUERY-014…QUERY-017, QUERY-020). One [`SearchWorker`] per window
//! starts one thread, when the first query is asked and not per keystroke; the thread takes
//! [`Request`]s from a channel and sends back the [`Found`] hits. Plain threads and channels in
//! the style of `jobs`, no async runtime.
//!
//! - A request owns what the search reads: a [`Snapshot`] of the notes (pointers, QUERY-015) and
//!   the places of the mail and post indexes, which the thread opens read-only on its own
//!   (QUERY-016); it shares no connection with the window.
//! - Every query has a number. Queries go to a [`Slot`] (the picker, the sidebar, later the lanes
//!   of a board): a newer query in a slot sets the cancel flag of the older one, which the search
//!   checks per line, and a result whose number is not the latest of its slot is dropped
//!   (QUERY-017).
//! - The same thread builds the vocabulary of the lists of the filter form (QUERY-030): a request of
//!   a queue of its own that answers with the tags and properties of the snapshot, no hits.
//! - The window never waits for the thread: [`SearchWorker::take`] gives a list when it is there
//!   (QUERY-018), and [`SearchWorker::busy`] tells `Headless::settle` that a query is on its way
//!   (QUERY-020).

use super::matcher::{Budget, Run, Stop};
use super::query::Checked;
use super::{search_in, Ctx, Found, Query, Snapshot};
use crate::busy::{self, Busy, Cancel, Key, Task};
use crate::mail::db;
use crate::search_form::vocab::Vocab;
use crate::search_sources::{Prefixes, Selection};
use crate::social::post_index::{Home, PostIndex};
use eframe::egui;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// A queue of the worker: a newer query of a slot gives up the older one of the same slot, and
/// queries of other slots are not touched.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Slot(pub u64);

impl Slot {
    /// the picker window (`Space f g`)
    pub const PICKER: Slot = Slot(0);
    /// the search of the left sidebar
    pub const SIDEBAR: Slot = Slot(1);
    /// the first lane of the open board; lane `n` is `Slot(Slot::LANES + n)` (QUERY-019)
    pub const LANES: u64 = 16;

    /// The task of this queue in the busy registry (JOB-038): the picker and the sidebar have
    /// one each, the lanes of a board none (the board shows its own state).
    fn key(self) -> Option<Key> {
        match self {
            Slot::PICKER => Some(SEARCH.with("picker")),
            Slot::SIDEBAR => Some(SEARCH.with("sidebar")),
            _ => None,
        }
    }

    /// The queue of lane number `n` of the board.
    pub fn lane(n: usize) -> Slot {
        Slot(Slot::LANES + n as u64)
    }

    /// A queue of its own for the vocabulary of one form (QUERY-030), above the lanes: a newer
    /// request of a form gives up its older one and no other queue.
    fn vocabulary() -> Slot {
        static NEXT: AtomicU64 = AtomicU64::new(1 << 32);
        Slot(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// A queue of one owner, handed out by [`SearchWorker::lease`].
pub struct Lease {
    pub slot: Slot,
    retired: Arc<Mutex<Vec<Slot>>>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.retired.lock().unwrap_or_else(|e| e.into_inner()).push(self.slot);
    }
}

/// The task of a search in the busy registry, once per queue (JOB-038).
pub const SEARCH: Key = Key::new("search");

/// The newest query of a queue.
struct Latest {
    id: u64,
    cancel: Arc<AtomicBool>,
    /// "Searching…" in the registry while the query is on its way (JOB-038): the task of the
    /// queue, passed on to a newer query; Cancel of the registry sets `cancel`
    task: Option<Task>,
}

/// What one query is given: everything the search reads, owned, so that it can go to the thread.
pub struct Request {
    /// shared: the lanes of a board ask about the same notes (QUERY-019)
    snapshot: Arc<Snapshot>,
    /// a lane: the notes the checked query matches, as a whole each, and no hits (QUERY-004)
    lane: bool,
    /// the vocabulary of the notes for the lists of the filter form (QUERY-030), and no hits
    vocab: bool,
    text: String,
    note_lines: usize,
    limit: usize,
    prefixes: Prefixes,
    shown: Selection,
    /// the text as the view checked it, once for the text (QUERY-013)
    checked: Option<Checked>,
    /// the mail index file, while the mail store is open
    mail: Option<PathBuf>,
    posts: Home,
}

impl Request {
    /// The question `query` asks of what `ctx` reads, as it is now.
    pub fn new(ctx: &Ctx, query: &Query) -> Request {
        // (a mail connection that is not a file has nothing another connection could read)
        let mail = ctx.mail.and_then(|c| c.path()).filter(|p| !p.is_empty()).map(PathBuf::from);
        Request {
            snapshot: ctx.vault.search_snapshot(),
            lane: false,
            vocab: false,
            text: query.text.to_string(),
            note_lines: query.note_lines,
            limit: query.limit,
            prefixes: query.prefixes,
            shown: query.shown,
            checked: query.checked.cloned(),
            mail,
            posts: ctx.posts.home().clone(),
        }
    }

    /// The question one lane of a board asks (QUERY-019): the notes of `snapshot` that `checked`
    /// matches. The snapshot is shared by the lanes of one vault state.
    pub fn lane(snapshot: Arc<Snapshot>, checked: Checked) -> Request {
        Request { snapshot, lane: true, vocab: false, text: checked.text.clone(), note_lines: 0, limit: 0, prefixes: Prefixes::default(), shown: Selection::ALL, checked: Some(checked), mail: None, posts: Home::Memory("lane".into()) }
    }

    /// The question the lists of the filter form ask (QUERY-030): what the tags and the front matter
    /// of the notes of `snapshot` say.
    pub fn vocabulary(snapshot: Arc<Snapshot>) -> Request {
        Request { snapshot, lane: false, vocab: true, text: String::new(), note_lines: 0, limit: 0, prefixes: Prefixes::default(), shown: Selection::ALL, checked: None, mail: None, posts: Home::Memory("vocabulary".into()) }
    }

    fn query(&self) -> Query<'_> {
        Query { text: &self.text, note_lines: self.note_lines, limit: self.limit, prefixes: self.prefixes, shown: self.shown, checked: self.checked.as_ref() }
    }
}

/// The connections the thread keeps open, so that a query does not open them again.
#[derive(Default)]
pub struct Conns {
    mail: Option<(PathBuf, Connection)>,
    posts: Option<(Home, PostIndex)>,
}

/// The read-only connection to the mail index at `path`, opened when it is not the one kept.
fn mail_reader<'a>(kept: &'a mut Option<(PathBuf, Connection)>, path: &Path) -> Option<&'a Connection> {
    if kept.as_ref().is_none_or(|(p, _)| p != path) {
        *kept = db::open_reader(path).ok().map(|c| (path.to_path_buf(), c));
    }
    kept.as_ref().map(|(_, c)| c)
}

/// The read-only connection to the post index at `home`, opened when it is not the one kept.
fn post_reader<'a>(kept: &'a mut Option<(Home, PostIndex)>, home: &Home) -> Option<&'a PostIndex> {
    if kept.as_ref().is_none_or(|(h, _)| h != home) {
        *kept = PostIndex::open_reader(home).map(|i| (home.clone(), i));
    }
    kept.as_ref().map(|(_, i)| i)
}

/// Run one request with the connections of the thread. None when it was given up (QUERY-017).
pub(crate) fn run_search(conns: &mut Conns, req: &Request, cancel: &AtomicBool) -> Option<Found> {
    if req.lane {
        return run_lane(req, cancel);
    }
    if req.vocab {
        return Vocab::of(&req.snapshot, cancel).map(|v| Found { vocab: Some(Arc::new(v)), ..Found::default() });
    }
    let Conns { mail, posts } = conns;
    let mail = req.mail.as_deref().and_then(|p| mail_reader(mail, p));
    let posts = post_reader(posts, &req.posts);
    search_in(&req.snapshot, mail, posts, &req.query(), cancel)
}

/// Run a lane: the notes its query matches (QUERY-004), in the order of the vault. A query that did
/// not pass the check matches nothing (QUERY-012). None when it was given up (QUERY-017).
fn run_lane(req: &Request, cancel: &AtomicBool) -> Option<Found> {
    let Some(expr) = req.checked.as_ref().and_then(Checked::expr) else { return Some(Found::default()) };
    let mut run = Run::new(&req.snapshot, cancel, Budget::standard());
    let lane = run.match_notes(expr);
    match run.stopped {
        Some(Stop::Cancelled) => None,
        stopped => Some(Found { lane, stopped_early: stopped == Some(Stop::Budget), ..Found::default() }),
    }
}

/// What the thread runs for a request (tests put their own in).
pub(crate) type Runner = fn(&mut Conns, &Request, &AtomicBool) -> Option<Found>;

/// A request on its way to the thread.
struct Job {
    slot: Slot,
    id: u64,
    request: Request,
    cancel: Arc<AtomicBool>,
    /// the window to wake when the answer is ready
    repaint: egui::Context,
}

/// The answer to a job; no hits when the search was given up.
struct Reply {
    slot: Slot,
    id: u64,
    found: Option<Found>,
}

/// The channels to the thread, made with the first query.
struct Thread {
    jobs: Sender<Job>,
    replies: Receiver<Reply>,
}

/// The search worker of one window.
pub struct SearchWorker {
    thread: Option<Thread>,
    run: Runner,
    repaint: egui::Context,
    next: u64,
    /// the number and the cancel flag of the newest query of each slot
    latest: HashMap<Slot, Latest>,
    /// the newest queries that ended without an answer because they were given up (Cancel of the
    /// registry), by slot
    given_up: HashMap<Slot, u64>,
    busy: Busy,
    /// the answers that are the newest of their slot and not taken yet
    ready: HashMap<Slot, (u64, Found)>,
    /// queries whose reply has not been read yet
    in_flight: usize,
    /// the queues whose owner is gone, to forget with the next frame (QUERY-030)
    retired: Arc<Mutex<Vec<Slot>>>,
}

impl Default for SearchWorker {
    fn default() -> SearchWorker {
        SearchWorker::with_runner(run_search)
    }
}

impl SearchWorker {
    pub(crate) fn with_runner(run: Runner) -> SearchWorker {
        SearchWorker { thread: None, run, repaint: egui::Context::default(), next: 0, latest: HashMap::new(), given_up: HashMap::new(), busy: Busy::default(), ready: HashMap::new(), in_flight: 0, retired: Arc::default() }
    }

    /// Tests: another runner for the worker the window was built with, which keeps its registry
    /// and has not started its thread yet.
    #[cfg(test)]
    pub(crate) fn set_runner(&mut self, run: Runner) {
        assert!(self.thread.is_none(), "the thread has started");
        self.run = run;
    }

    /// The registry the queries of the picker and the sidebar are listed in (JOB-038).
    pub fn with_busy(mut self, busy: &Busy) -> SearchWorker {
        self.busy = busy.clone();
        self
    }

    /// The window the thread wakes when an answer is ready; read the replies that are in.
    /// Called at the start of every frame.
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.repaint = ctx.clone();
        self.pump();
    }

    /// Read the replies: keep the newest of each slot, drop the others (QUERY-017).
    fn pump(&mut self) {
        let retired = std::mem::take(&mut *self.retired.lock().unwrap_or_else(|e| e.into_inner()));
        for slot in retired {
            self.forget(slot);
        }
        let Some(thread) = &self.thread else { return };
        while let Ok(r) = thread.replies.try_recv() {
            self.in_flight -= 1;
            let Some(latest) = self.latest.get_mut(&r.slot).filter(|l| l.id == r.id) else { continue };
            // (the task ends before the answer can be taken)
            latest.task = None;
            match r.found {
                Some(found) => {
                    self.ready.insert(r.slot, (r.id, found));
                }
                None => {
                    self.given_up.insert(r.slot, r.id);
                }
            }
        }
    }

    /// Ask for a search. The older query of the slot is given up. The number comes back to
    /// [`SearchWorker::take`] the answer with.
    pub fn submit(&mut self, slot: Slot, request: Request) -> u64 {
        let id = self.next;
        self.next += 1;
        let cancel = Arc::new(AtomicBool::new(false));
        let mut task = None;
        if let Some(old) = self.latest.remove(&slot) {
            old.cancel.store(true, Ordering::Relaxed);
            task = old.task;
        }
        // one task per queue: a newer query takes it over, and Cancel stops the newer query
        let task = task.or_else(|| slot.key().and_then(|k| self.busy.begin_listed_after(k, "Searching…", busy::LISTED_AFTER)));
        let task = task.map(|mut t| {
            t.cancellable_with(Cancel::from(cancel.clone()));
            t
        });
        self.latest.insert(slot, Latest { id, cancel: cancel.clone(), task });
        self.ready.remove(&slot);
        self.given_up.remove(&slot);
        let run = self.run;
        let thread = self.thread.get_or_insert_with(|| spawn(run));
        self.in_flight += 1;
        // (the thread ends only when this side is dropped: a send does not fail)
        let _ = thread.jobs.send(Job { slot, id, request, cancel, repaint: self.repaint.clone() });
        id
    }

    /// A queue of its own, for as long as the lease lives (QUERY-030): when it is dropped the
    /// worker forgets the queue with its next frame, and gives up its query on its way.
    pub fn lease(&self) -> Lease {
        Lease { slot: Slot::vocabulary(), retired: self.retired.clone() }
    }

    /// Give up the query of the slot and drop everything the worker keeps for it.
    fn forget(&mut self, slot: Slot) {
        self.cancel(slot);
    }

    /// How many queues the worker keeps something for (tests).
    #[cfg(test)]
    pub fn queues(&self) -> usize {
        self.latest.len() + self.ready.len() + self.given_up.len()
    }

    /// Give up the query of the slot that is on its way, if any: its answer is dropped.
    pub fn cancel(&mut self, slot: Slot) {
        if let Some(latest) = self.latest.remove(&slot) {
            latest.cancel.store(true, Ordering::Relaxed);
        }
        self.ready.remove(&slot);
        self.given_up.remove(&slot);
    }

    /// Did query `id` of `slot`, still the newest, end without an answer because it was given up
    /// (Cancel in the indicator)? The view then keeps its last list and does not ask again for
    /// the same text.
    pub fn gave_up(&mut self, slot: Slot, id: u64) -> bool {
        self.pump();
        self.given_up.get(&slot) == Some(&id)
    }

    /// The answer to query `id` of `slot`, once, if it is in and still the newest.
    pub fn take(&mut self, slot: Slot, id: u64) -> Option<Found> {
        self.pump();
        match self.ready.get(&slot) {
            Some((got, _)) if *got == id => self.ready.remove(&slot).map(|(_, f)| f),
            _ => None,
        }
    }

    /// Is a query on its way (QUERY-020)? `Headless::settle` waits while it is (tests).
    #[cfg(test)]
    pub fn busy(&self) -> bool {
        self.in_flight > 0
    }
}

impl Drop for SearchWorker {
    /// A window that closes gives up the queries on their way, so that the thread ends with the
    /// channel.
    fn drop(&mut self) {
        for latest in self.latest.values() {
            latest.cancel.store(true, Ordering::Relaxed);
        }
    }
}

/// Start the thread: it works through the jobs in order and ends when the worker is dropped. A
/// job that was given up before its turn is not started; a search that panics answers with
/// nothing found and the thread goes on (`jobs::catch`).
fn spawn(run: Runner) -> Thread {
    let (jobs, job_rx) = channel::<Job>();
    let (reply_tx, replies) = channel::<Reply>();
    std::thread::Builder::new()
        .name("search".into())
        .spawn(move || {
            let mut conns = Conns::default();
            while let Ok(job) = job_rx.recv() {
                let found = if job.cancel.load(Ordering::Relaxed) {
                    None
                } else {
                    crate::jobs::catch(|| run(&mut conns, &job.request, &job.cancel)).unwrap_or_else(|e| {
                        tracing::warn!("search: {e}");
                        Some(Found::default())
                    })
                };
                let _ = reply_tx.send(Reply { slot: job.slot, id: job.id, found });
                job.repaint.request_repaint();
            }
        })
        .expect("the search thread starts");
    Thread { jobs, replies }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mail::db::{IndexFields, NewMessage, Op, STATE_FULL};
    use crate::picker::Hit;
    use crate::social::tests::post;
    use crate::vault::Vault;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    /// A request that searches nothing: the runners of the tests only read its text.
    fn bare(text: &str) -> Request {
        Request { snapshot: Arc::default(), lane: false, vocab: false, text: text.into(), note_lines: 3, limit: 20, prefixes: Prefixes::default(), shown: Selection::ALL, checked: None, mail: None, posts: Home::Memory("none".into()) }
    }

    fn titles(f: &Found) -> Vec<String> {
        f.hits.iter().map(|h| h.title.clone()).collect()
    }

    /// Wait until no query is on its way any more.
    fn drain(w: &mut SearchWorker) {
        let end = Instant::now() + Duration::from_secs(10);
        while w.busy() {
            assert!(Instant::now() < end, "the worker never answered");
            std::thread::sleep(Duration::from_millis(1));
            w.pump();
        }
    }

    /// Runs on the thread of the worker and answers with the name of that thread and the text.
    fn echo(_: &mut Conns, req: &Request, _: &AtomicBool) -> Option<Found> {
        let who = std::thread::current();
        Some(Found { hits: vec![Hit::new("", format!("{}:{:?}", req.text, who.id()), who.name().unwrap_or("").into(), crate::app::Action::FocusEditor)], counts: None, ..Default::default() })
    }

    /// Runs until it is given up.
    fn hold(_: &mut Conns, _: &Request, cancel: &AtomicBool) -> Option<Found> {
        let end = Instant::now() + Duration::from_secs(10);
        while !cancel.load(Ordering::Relaxed) && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(1));
        }
        None
    }

    /// "hold" runs until it is given up, anything else is answered at once, a given-up search
    /// included (a search that was done before it looked at its flag).
    fn hold_or_echo(c: &mut Conns, req: &Request, cancel: &AtomicBool) -> Option<Found> {
        if req.text == "hold" {
            hold(c, req, cancel)
        } else {
            echo(c, req, cancel)
        }
    }

    // spec: QUERY-014
    #[test]
    fn one_thread_is_started_with_the_first_query_and_answers_all_of_them() {
        let mut w = SearchWorker::with_runner(echo);
        assert!(w.thread.is_none(), "no thread until a query is asked");
        let main = std::thread::current().id();
        let mut seen = vec![];
        for text in ["a", "b", "c"] {
            let id = w.submit(Slot::PICKER, bare(text));
            drain(&mut w);
            let found = w.take(Slot::PICKER, id).expect("the answer");
            let title = titles(&found).remove(0);
            assert!(title.starts_with(&format!("{text}:")), "{title}");
            assert!(!title.contains(&format!("{main:?}")), "not the thread of the window: {title}");
            assert_eq!(found.hits[0].detail, "search");
            seen.push(title.split_once(':').unwrap().1.to_string());
        }
        assert!(seen.windows(2).all(|p| p[0] == p[1]), "the same thread for every query: {seen:?}");
        assert!(w.take(Slot::PICKER, 99).is_none() && !w.busy());
    }

    // spec: QUERY-014
    #[test]
    fn a_search_that_panics_answers_with_nothing_and_the_thread_goes_on() {
        fn boom(c: &mut Conns, req: &Request, cancel: &AtomicBool) -> Option<Found> {
            assert!(req.text != "boom", "the search crashed");
            echo(c, req, cancel)
        }
        let mut w = SearchWorker::with_runner(boom);
        let bad = w.submit(Slot::PICKER, bare("boom"));
        drain(&mut w);
        assert!(w.take(Slot::PICKER, bad).expect("an answer").hits.is_empty());
        let good = w.submit(Slot::PICKER, bare("fine"));
        drain(&mut w);
        assert_eq!(titles(&w.take(Slot::PICKER, good).unwrap()).len(), 1, "the thread is still there");
    }

    // spec: QUERY-017
    #[test]
    fn a_newer_query_of_a_slot_gives_up_the_older_one_and_leaves_the_other_slots() {
        let mut w = SearchWorker::with_runner(hold_or_echo);
        let first = w.submit(Slot::PICKER, bare("hold"));
        assert!(!w.latest[&Slot::PICKER].cancel.load(Ordering::Relaxed));
        let second = w.submit(Slot::PICKER, bare("hold"));
        assert_ne!(first, second, "every query has a number");
        assert!(second > first);
        assert!(!w.latest[&Slot::PICKER].cancel.load(Ordering::Relaxed), "the flag of the newest query is not set");
        // the older ones are given up: the thread (there is one) comes free for the next jobs
        let third = w.submit(Slot::PICKER, bare("last"));
        let side = w.submit(Slot::SIDEBAR, bare("hold"));
        let end = Instant::now() + Duration::from_secs(10);
        while w.ready.get(&Slot::PICKER).is_none_or(|(id, _)| *id != third) {
            assert!(Instant::now() < end, "the newest query was never answered");
            std::thread::sleep(Duration::from_millis(1));
            w.pump();
        }
        assert!(w.take(Slot::PICKER, first).is_none() && w.take(Slot::PICKER, second).is_none(), "an older query has no answer");
        assert!(titles(&w.take(Slot::PICKER, third).unwrap())[0].starts_with("last:"));
        // the query of the sidebar was not touched by those of the picker: it is given up on its own
        assert!(w.busy() && !w.latest[&Slot::SIDEBAR].cancel.load(Ordering::Relaxed));
        w.cancel(Slot::SIDEBAR);
        drain(&mut w);
        assert!(w.take(Slot::SIDEBAR, side).is_none(), "a query that was cancelled is dropped");
    }

    /// Wait until the answer of `slot` is in.
    fn drain_slot(w: &mut SearchWorker, slot: Slot) {
        let end = Instant::now() + Duration::from_secs(10);
        while !w.ready.contains_key(&slot) {
            assert!(Instant::now() < end, "never answered");
            std::thread::sleep(Duration::from_millis(1));
            w.pump();
        }
    }

    fn search_key(slot: &str) -> Key {
        SEARCH.with(slot)
    }

    // spec: JOB-038
    #[test]
    fn job_038_a_query_is_a_task_that_is_listed_after_the_wait_and_gone_with_its_answer() {
        let busy = Busy::default();
        let mut w = SearchWorker::with_runner(hold_or_echo).with_busy(&busy);
        let key = search_key("sidebar");
        assert!(!busy.is_running(&key));
        let held = w.submit(Slot::SIDEBAR, bare("hold"));
        let info = busy.info(&key).expect("a task while the query runs");
        assert_eq!(info.label, "Searching…");
        assert!(info.cancellable && !info.cancelling);
        let t = info.started;
        assert!(busy.list_at(t).is_empty(), "quiet: not listed at first");
        assert!(busy.list_at(t + busy::LISTED_AFTER - Duration::from_millis(1)).is_empty());
        assert_eq!(busy.list_at(t + busy::LISTED_AFTER).len(), 1, "listed after the wait");
        // the picker is another queue with its own task; a lane of a board has none
        w.submit(Slot::PICKER, bare("hold"));
        assert!(busy.is_running(&search_key("picker")) && busy.is_running(&key));
        w.cancel(Slot::PICKER);
        assert!(!busy.is_running(&search_key("picker")) && busy.is_running(&key));
        assert!(Slot::lane(0).key().is_none());
        w.cancel(Slot::SIDEBAR);
        assert!(!busy.is_running(&key), "the task ends with the query that is given up");
        assert!(w.take(Slot::SIDEBAR, held).is_none());
        // an answered query ends its task before the answer can be taken
        let id = w.submit(Slot::SIDEBAR, bare("quick"));
        drain(&mut w);
        assert!(!busy.is_running(&key));
        assert!(w.take(Slot::SIDEBAR, id).is_some());
    }

    // spec: JOB-038
    #[test]
    fn job_038_a_newer_query_keeps_the_one_task() {
        let busy = Busy::default();
        let mut w = SearchWorker::with_runner(hold_or_echo).with_busy(&busy);
        let key = search_key("sidebar");
        let first = w.submit(Slot::SIDEBAR, bare("hold"));
        let id = busy.info(&key).unwrap().id;
        let second = w.submit(Slot::SIDEBAR, bare("hold"));
        assert_eq!(busy.info(&key).map(|t| t.id), Some(id), "the same task");
        assert_eq!(busy.list_at(Instant::now() + busy::LISTED_AFTER).len(), 1, "one task, not two");
        // the newer query is the one that Cancel stops
        assert!(!w.latest[&Slot::SIDEBAR].cancel.load(Ordering::Relaxed));
        let third = w.submit(Slot::SIDEBAR, bare("last"));
        drain_slot(&mut w, Slot::SIDEBAR);
        assert!(!busy.is_running(&key), "the task ended with the newest answer");
        assert!(w.take(Slot::SIDEBAR, first).is_none() && w.take(Slot::SIDEBAR, second).is_none(), "the older results are dropped");
        assert!(w.take(Slot::SIDEBAR, third).is_some());
    }

    // spec: JOB-038
    #[test]
    fn job_038_cancel_of_the_registry_gives_the_query_up_and_the_last_list_stays() {
        let busy = Busy::default();
        let mut w = SearchWorker::with_runner(hold_or_echo).with_busy(&busy);
        let key = search_key("picker");
        let id = w.submit(Slot::PICKER, bare("hold"));
        let task = busy.info(&key).unwrap();
        assert!(busy.cancel(task.id), "the registry offers Cancel");
        assert!(w.latest[&Slot::PICKER].cancel.load(Ordering::Relaxed), "it is the flag of the query");
        let end = Instant::now() + Duration::from_secs(10);
        while !w.gave_up(Slot::PICKER, id) {
            assert!(Instant::now() < end, "the query was not stopped");
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(!busy.is_running(&key) && !w.busy());
        assert!(w.take(Slot::PICKER, id).is_none(), "no answer: the view keeps its last list");
    }

    // spec: QUERY-017
    #[test]
    fn an_answer_that_is_not_the_latest_of_its_slot_is_dropped() {
        // (a search that does not look at its flag answers although it was given up)
        let mut w = SearchWorker::with_runner(echo);
        let old = w.submit(Slot::PICKER, bare("old"));
        let new = w.submit(Slot::PICKER, bare("new"));
        drain(&mut w);
        assert!(w.take(Slot::PICKER, old).is_none(), "stale");
        assert!(titles(&w.take(Slot::PICKER, new).unwrap())[0].starts_with("new:"));
        assert!(w.take(Slot::PICKER, new).is_none(), "an answer is taken once");
        // an answer that came before the next query is not kept for it
        let a = w.submit(Slot::SIDEBAR, bare("a"));
        drain(&mut w);
        let b = w.submit(Slot::SIDEBAR, bare("b"));
        assert!(w.take(Slot::SIDEBAR, a).is_none(), "the answer of an older query went with the newer one");
        drain(&mut w);
        assert!(w.take(Slot::SIDEBAR, b).is_some());
    }

    // spec: QUERY-017
    #[test]
    fn a_search_stops_at_the_next_line_once_its_flag_is_set() {
        let d = std::env::temp_dir().join(format!("shale-worker-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("A.md"), "banana\n".repeat(10)).unwrap();
        let v = Vault::open(&d).unwrap();
        let snap = Snapshot::of(&v);
        let q = Query::picker("banana");
        let go = AtomicBool::new(false);
        let checked = super::super::query::Checked::search("banana", Prefixes::default());
        let scan = |flag: &AtomicBool| {
            let mut run = super::super::Run::new(&snap, flag, super::super::Budget::standard());
            (super::super::scan_notes(&snap, &q, checked.expr().unwrap(), &mut run), run.stopped)
        };
        let (hits, stopped) = scan(&go);
        assert_eq!((hits.len(), stopped), (3, None), "not given up: the lines of the note");
        let stop = AtomicBool::new(true);
        let (hits, stopped) = scan(&stop);
        assert_eq!((hits.len(), stopped), (0, Some(super::super::Stop::Cancelled)), "given up: not a line is read");
        assert!(search_in(&snap, None, None, &q, &stop).is_none(), "and the search has no answer");
        assert!(search_in(&snap, None, None, &q, &go).is_some_and(|f| f.hits.len() == 3));
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-014, QUERY-017
    #[test]
    fn a_window_that_closes_gives_up_its_queries_and_the_thread_ends() {
        static STARTED: AtomicUsize = AtomicUsize::new(0);
        static ENDED: AtomicUsize = AtomicUsize::new(0);
        fn hold_and_note(c: &mut Conns, req: &Request, cancel: &AtomicBool) -> Option<Found> {
            STARTED.fetch_add(1, Ordering::SeqCst);
            let r = hold(c, req, cancel);
            ENDED.fetch_add(1, Ordering::SeqCst);
            r
        }
        let mut w = SearchWorker::with_runner(hold_and_note);
        w.submit(Slot::PICKER, bare("x"));
        let end = Instant::now() + Duration::from_secs(5);
        while STARTED.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < end, "the search never started");
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(w);
        while ENDED.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < end, "the search was not given up");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn mail_file(dir: &Path) -> PathBuf {
        let path = dir.join("mail-index.db");
        let mut c = Connection::open(&path).unwrap();
        db::migrate(&c).unwrap();
        let f = db::upsert_folder(&c, "a", "A", "INBOX", Some("/"), None, true).unwrap();
        c.execute("UPDATE folders SET uidvalidity = 1 WHERE id = ?1", [f.id]).unwrap();
        let fields = IndexFields { date: Some(10), from: "ann@example.org".into(), to: "me@example.org".into(), subject: "Banana order".into(), ..Default::default() };
        let m = NewMessage { folder: f.id, uidvalidity: 1, uid: 1, path: "a/f/1/0.eml".into(), state: STATE_FULL, flags: String::new(), size: 10, parsed: true, fields };
        db::apply_batch(&mut c, &[Op::message(m)], None).unwrap();
        path
    }

    // spec: QUERY-014, QUERY-016
    #[test]
    fn the_worker_reads_the_mail_and_post_indexes_through_connections_of_its_own() {
        let d = std::env::temp_dir().join(format!("shale-worker-conns-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Fruit.md"), "banana bread\n").unwrap();
        let d = crate::known_vaults::canonical(&d); // macOS: /var is /private/var, which SQLite reports
        let v = Vault::open(&d).unwrap();
        let mail = Connection::open(mail_file(&d)).unwrap();
        let mut posts = PostIndex::open(&d.join("posts.db")).unwrap();
        let mut p = post("p1", "ada", crate::calendar::time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let ctx = Ctx { vault: &v, mail: Some(&mail), posts: &posts };
        let req = Request::new(&ctx, &Query::picker("banana"));
        assert_eq!(req.mail.as_deref(), Some(d.join("mail-index.db").as_path()), "the mail connection says where its file is");
        assert_eq!(req.posts, Home::File(d.join("posts.db")));
        let mut w = SearchWorker::default();
        let id = w.submit(Slot::PICKER, req);
        drain(&mut w);
        let found = w.take(Slot::PICKER, id).unwrap();
        assert_eq!(found.hits.iter().map(|h| h.group).collect::<Vec<_>>(), ["Notes", "Mail", "Posts"], "the three sources, read on the worker");
        assert!(found.counts.is_some());
        // the connections the thread keeps cannot write
        let mut conns = Conns::default();
        let req = Request::new(&ctx, &Query::picker("banana"));
        assert!(run_search(&mut conns, &req, &AtomicBool::new(false)).is_some());
        let (path, mail_conn) = conns.mail.as_ref().expect("kept for the next query");
        assert_eq!(path, &d.join("mail-index.db"));
        let err = mail_conn.execute("CREATE TABLE intruder (x)", []).unwrap_err().to_string();
        assert!(err.contains("readonly"), "{err}");
        let (home, reader) = conns.posts.as_mut().expect("kept for the next query");
        assert_eq!(home, &Home::File(d.join("posts.db")));
        reader.add("acct", "home", &[post("p2", "bob", crate::calendar::time::now() - 60, None)]);
        assert_eq!(posts.count(None), 1, "a write through the reader of the worker changed nothing");
        // a query for another index file opens that one
        let again = run_search(&mut conns, &Request { mail: None, ..Request::new(&ctx, &Query::picker("banana")) }, &AtomicBool::new(false)).unwrap();
        assert_eq!(again.counts.unwrap().get(crate::search_sources::Kind::Mail), 0, "no mail store: no mail searched");
        let _ = std::fs::remove_dir_all(&d);
    }

    // spec: QUERY-016
    #[test]
    fn an_index_that_cannot_be_opened_is_searched_as_empty_and_tried_again() {
        let d = std::env::temp_dir().join(format!("shale-worker-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let mut conns = Conns::default();
        let none = Request { mail: Some(d.join("no-such-mail.db")), posts: Home::File(d.join("no-such-posts.db")), ..bare("banana") };
        let found = run_search(&mut conns, &none, &AtomicBool::new(false)).unwrap();
        assert!(found.hits.is_empty());
        assert!(conns.mail.is_none() && conns.posts.is_none(), "nothing is kept for a file that is not there");
        // an in-memory index is read through its shared name, and one that is gone has nothing
        let mut posts = PostIndex::in_memory();
        let mut p = post("p1", "ada", crate::calendar::time::now() - 3600, None);
        p.text_markdown = "banana republic".into();
        posts.add("acct", "home", &[p]);
        let live = Request { posts: posts.home().clone(), ..bare("banana") };
        assert_eq!(run_search(&mut conns, &live, &AtomicBool::new(false)).unwrap().hits.len(), 1);
        // (the database lives as long as a connection to it: the writer and the reader of the thread)
        drop((posts, conns));
        let gone = Request { posts: live.posts.clone(), ..bare("banana") };
        let mut fresh = Conns::default();
        assert!(run_search(&mut fresh, &gone, &AtomicBool::new(false)).unwrap().hits.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
