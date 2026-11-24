//! File history in the app: when snapshots run, what happens to open buffers when a version is
//! restored, and the actions the history panel calls (SYNC-032…SYNC-046, ADR-0028).
//!
//! Public actions (functions of [`ShaleApp`], for the history panel of #472 and the wizard #469):
//!
//! - `history_show(&Path)`: show the panel of a note (the ⋯ menu); `open_history_changes(&Path, &Version)`:
//!   open the comparison view; `open_history_deleted()`: the list of the deleted notes.
//! - `history_restore_version(&Path, &Version)`, `history_copy_version(&Version)`,
//!   `history_view_version(&Version)`, `history_restore_deleted(&Deleted)`.
//! - `history_active() -> bool`, `history_off_reason() -> Option<String>`.
//!
//! Every git call runs in a background job: snapshots one at a time and never while the sync runs
//! (SYNC-035), reads (versions, text at a version, deleted notes) beside them.

use super::{unix_now, ShaleApp};
use crate::busy::{self, Key};
use crate::gitsync::history::thin;
use crate::gitsync::history::{self, Deleted, HistoryConfig, Outcome, Reason, Repo, Request, Schedule, Version};
use crate::gitsync::{Git, Options};
use crate::history::compare::Loaded;
use crate::history::state::{Changes, Data, DeletedList, Panel};
use crate::vault::rel_to_string;
use eframe::egui;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::Duration;

/// How long a snapshot waits for the tree lock before it is asked for again later (SYNC-035).
const TREE_WAIT: Duration = Duration::from_secs(2);
/// How long the window waits for the last snapshots when it closes or the vault is switched (SYNC-033).
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// Seconds before a snapshot that was busy or failed is tried again.
const RETRY_SECS: u64 = 5;
/// Seconds after a vault opened before the first thinning of its local history (SYNC-070).
const THIN_DELAY: u64 = 60;
/// Seconds before a thinning that found the vault busy is tried again.
const THIN_RETRY: u64 = 30;
/// How many versions and deleted notes the list asks for.
const LIST_LIMIT: usize = 300;

/// The snapshots of the file history as tasks (JOB-025): housekeeping that is over in a moment, so
/// the indicator lists one only after 2 seconds. They cannot be stopped: a snapshot waits for the
/// tree lock and then commits.
pub(crate) const HISTORY_SNAPSHOT: Key = Key::new("history-snapshot");
/// The thinning of a local-only history (SYNC-070), likewise.
pub(crate) const HISTORY_THIN: Key = Key::new("history-thin");
/// A read of the history (versions, a text at a version, deleted notes, a comparison), likewise.
pub(crate) const HISTORY_QUERY: Key = Key::new("history-query");

/// A snapshot to make and what happens after it.
struct Queued {
    req: Request,
    then: Option<Then>,
    /// It came from the schedule: when it is busy, the paths go back there.
    scheduled: bool,
}

/// What follows a snapshot.
#[derive(Clone)]
enum Then {
    /// The pre-restore snapshot is made: read the version and put it in the note (SYNC-038).
    Restore { rel: PathBuf, commit: String, path: PathBuf },
}

struct SnapJob {
    vault: PathBuf,
    queued: Queued,
    job: busy::Job<Result<Outcome, String>>,
}

/// The thinning of a local-only history (SYNC-067…SYNC-070).
struct ThinJob {
    vault: PathBuf,
    job: busy::Job<Result<thin::Outcome, String>>,
}

/// A read of the history.
#[derive(Clone)]
enum Query {
    Versions(PathBuf),
    Deleted,
    Content {
        commit: String,
        path: PathBuf,
        then: Use,
    },
    DeletedContent(Deleted),
    /// The two ends of a comparison (HIST-008): the older version, the newer one or the note as it is
    /// now (`live`: the text on the screen, or None to read the file), and the names of the sides in a patch.
    Compare(Box<Compared>),
}

/// The ends of a comparison, for a read.
#[derive(Clone)]
struct Compared {
    rel: PathBuf,
    from: Version,
    to: Option<Version>,
    live: Option<String>,
    labels: (String, String),
}

/// What is done with a text read from the history.
#[derive(Clone)]
enum Use {
    Copy,
    View(String),
    Restore { rel: PathBuf, commit: String },
    RestoreDeleted(Deleted),
}

enum Answer {
    Versions(Vec<Version>),
    Deleted(Vec<Deleted>),
    Text(Vec<u8>),
    Compare(Box<Loaded>),
}

struct QueryJob {
    vault: PathBuf,
    query: Query,
    job: busy::Job<Result<Answer, String>>,
}

/// The history's state in the app.
#[derive(Default)]
pub struct HistoryState {
    /// What is saved and not yet snapshotted (SYNC-033).
    pub sched: Schedule,
    queue: VecDeque<Queued>,
    job: Option<SnapJob>,
    /// The thinning job; no snapshot and no read runs beside it (SYNC-070).
    thin: Option<ThinJob>,
    /// No thinning before this time (unix seconds).
    thin_next: u64,
    queries: VecDeque<Query>,
    query: Option<QueryJob>,
    /// The vault the first snapshot was asked for.
    started_for: Option<PathBuf>,
    /// No snapshot job before this time (unix seconds).
    not_before: u64,
    /// The last error that was shown, so it is shown once.
    last_error: Option<String>,
    /// The list of the versions of the focused note, in the panel (HIST-004).
    pub panel: Panel,
    /// The comparison view (HIST-008).
    pub changes: Changes,
    /// The list of the deleted notes (SYNC-040).
    pub deleted: DeletedList,
    /// A screenshot run with a made-up history keeps it (HIST-018).
    pub(crate) demo: bool,
    /// Tests: the clock.
    #[cfg(test)]
    pub(crate) fake_now: Option<u64>,
    /// Tests: history runs in a window built without persistence.
    #[cfg(test)]
    pub(crate) on: bool,
}

impl HistoryState {
    pub fn now(&self) -> u64 {
        #[cfg(test)]
        if let Some(t) = self.fake_now {
            return t;
        }
        unix_now()
    }

    /// The vault is left (VAULT-037): its snapshots, queue, panel and lists are dropped. What is
    /// kept is what belongs to the window (a screenshot's made-up history, the test clock).
    pub(crate) fn drop_vault(&mut self) {
        let kept = HistoryState {
            demo: self.demo,
            #[cfg(test)]
            fake_now: self.fake_now,
            #[cfg(test)]
            on: self.on,
            ..Default::default()
        };
        *self = kept;
    }

    /// A snapshot job is running.
    pub fn busy(&self) -> bool {
        self.job.is_some() || self.thin.is_some()
    }

    /// A note or drawing was saved (SYNC-033).
    pub fn note_saved(&mut self, rel: &Path) {
        let now = self.now();
        self.sched.saved(rel, now);
    }

    #[cfg(test)]
    fn quiet(&self) -> bool {
        self.job.is_none() && self.thin.is_none() && self.queue.is_empty()
    }
}

/// "2026-11-21 15:04" in local time, for a unix time.
pub(super) fn when(ts: i64) -> String {
    let (days, sod) = crate::calendar::time::utc_to_local(ts);
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {}", crate::calendar::time::hhmm(sod))
}

impl ShaleApp {
    /// Does history run now (SYNC-032): it is on, a vault is open, there is a git and, as with the
    /// automatic sync, the window keeps its settings (a window built for a test does not).
    pub fn history_active(&mut self) -> bool {
        let persist = self.persist_config || self.history.demo;
        #[cfg(test)]
        let persist = persist || self.history.on;
        persist && self.cfg.history.enabled && self.vault.is_some() && self.git_version().is_some()
    }

    /// Why history is off, for the settings (SYNC-032); None: it is on.
    pub fn history_off_reason(&mut self) -> Option<String> {
        if !self.cfg.history.enabled {
            Some("The history is switched off. Versions already kept stay.".into())
        } else if self.git_version().is_none() {
            Some("The history is off because git is not installed. Install git, or let Shale install it, above.".into())
        } else if self.vault.is_none() {
            Some("Open a vault to keep its history.".into())
        } else {
            None
        }
    }

    /// The things a job needs: git, the vault, where the histories live and the commit options.
    pub(crate) fn history_job_data(&self) -> Option<(Git, PathBuf, PathBuf, Options)> {
        let root = self.vault.as_ref()?.root.clone();
        Some((self.sync.git.clone(), root, self.usage.places.history.clone(), self.sync_options()))
    }

    /// A tab closed: when no other tab shows the note, its saved changes are due (SYNC-033).
    pub(crate) fn history_tab_closed(&mut self, rel: &Path) {
        self.history.sched.closed(rel);
    }

    /// Every frame: collect finished jobs, start the next snapshot or read.
    pub(crate) fn history_tick(&mut self, ctx: &egui::Context) {
        let now = self.history.now();
        self.history_collect(ctx);
        if !self.history_active() {
            self.history.sched.take_all();
            self.history.queue.clear();
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        if self.history.started_for.as_deref() != Some(root.as_path()) {
            self.history.started_for = Some(root.clone());
            self.history.sched = Schedule::default();
            self.history.queue.clear();
            self.history.queries.clear();
            self.history.thin_next = now + THIN_DELAY;
            // a new local history starts with the vault as it is (SYNC-044)
            self.history.queue.push_back(Queued { req: Request::new(Reason::Initial, vec![]), then: None, scheduled: false });
        }
        self.history_panel_tick();
        self.history_start_query(ctx);
        // a connection may move the local history into the vault's `.git`: nothing starts beside it (SYNC-072)
        let sync_busy = self.sync.running() || self.sync.lfs_job_running() || self.sync.setup.joining.active();
        if self.history.job.is_none() && self.history.thin.is_none() && !sync_busy && now >= self.history.not_before {
            let cfg: HistoryConfig = self.cfg.history.clone();
            let (notes, drawings) = (&self.notes, &self.drawings);
            let editing = |p: &Path| notes.get(p).is_some_and(|d| d.dirty) || drawings.get(p).is_some_and(|d| d.dirty);
            let mut due: Vec<(Reason, Vec<PathBuf>)> = vec![];
            for (path, reason) in self.history.sched.due(now, &cfg, editing) {
                match due.iter_mut().find(|(r, _)| *r == reason) {
                    Some((_, paths)) => paths.push(path),
                    None => due.push((reason, vec![path])),
                }
            }
            for (reason, paths) in due {
                self.history.queue.push_back(Queued { req: Request::new(reason, paths), then: None, scheduled: true });
            }
            if let Some(q) = self.history.queue.pop_front() {
                self.history_start_snapshot(ctx, root.clone(), q);
            }
        }
        if self.history.job.is_none() && self.history.thin.is_none() && self.history.queue.is_empty() && self.history.query.is_none() && !sync_busy && now >= self.history.thin_next {
            self.history_start_thin(ctx, root, now);
        }
        if let Some(secs) = self.history.sched.next_in(now, &self.cfg.history) {
            ctx.request_repaint_after(Duration::from_secs(secs.clamp(1, 30)));
        }
    }

    fn history_start_snapshot(&mut self, ctx: &egui::Context, vault: PathBuf, queued: Queued) {
        let Some((git, root, hist, opts)) = self.history_job_data() else { return };
        let (tree, req, repaint) = (self.sync.tree.clone(), queued.req.clone(), ctx.clone());
        let job = self.busy.spawn_quiet(
            HISTORY_SNAPSHOT,
            "Saving a version of the changed notes…",
            move || {
                let repo = history::locate(&git, &root, &hist)?;
                history::snapshot(&repo, &opts, &req, &tree, TREE_WAIT)
            },
            move || repaint.request_repaint(),
        );
        match job {
            Some(job) => self.history.job = Some(SnapJob { vault, queued, job }),
            None => self.history.queue.push_front(queued), // the task is still listed: the next frame tries again
        }
    }

    /// Thin the local history when a day has passed since the last time (SYNC-070). A vault with its
    /// own `.git` is skipped by the job (SYNC-069).
    fn history_start_thin(&mut self, ctx: &egui::Context, vault: PathBuf, now: u64) {
        let Some((git, root, hist, _)) = self.history_job_data() else { return };
        let (tree, repaint) = (self.sync.tree.clone(), ctx.clone());
        let job = self.busy.spawn_quiet(
            HISTORY_THIN,
            "Thinning out the old versions…",
            move || {
                let repo = history::locate(&git, &root, &hist)?;
                thin::run_if_due(&repo, &tree, TREE_WAIT, now)
            },
            move || repaint.request_repaint(),
        );
        self.history.thin = job.map(|job| ThinJob { vault, job });
    }

    fn history_start_query(&mut self, ctx: &egui::Context) {
        if self.history.query.is_some() || self.history.thin.is_some() || self.sync.setup.joining.active() {
            return;
        }
        let Some(query) = self.history.queries.pop_front() else { return };
        let Some((git, root, hist, _)) = self.history_job_data() else { return };
        let (job_query, repaint, job_root) = (query.clone(), ctx.clone(), root.clone());
        let job = self.busy.spawn_quiet(
            HISTORY_QUERY,
            "Reading the history…",
            move || {
                let repo = history::locate(&git, &job_root, &hist)?;
                run_query(&repo, &job_query)
            },
            move || repaint.request_repaint(),
        );
        match job {
            Some(job) => self.history.query = Some(QueryJob { vault: root, query, job }),
            None => self.history.queries.push_front(query), // the task is still listed: the next frame tries again
        }
    }

    /// Take what the jobs finished.
    fn history_collect(&mut self, ctx: &egui::Context) {
        let now = self.history.now();
        if let Some(job) = &self.history.job {
            let r = match job.job.try_recv() {
                Ok(r) => Some(r.unwrap_or_else(Err)),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    None
                }
                Err(TryRecvError::Disconnected) => Some(Err("the snapshot stopped unexpectedly".into())),
            };
            if let Some(r) = r {
                if let Some(job) = self.history.job.take() {
                    self.history_snapshot_done(ctx, job, r, now);
                }
            }
        }
        if let Some(job) = &self.history.thin {
            let r = match job.job.try_recv() {
                Ok(r) => Some(r.unwrap_or_else(Err)),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    None
                }
                Err(TryRecvError::Disconnected) => Some(Err("the thinning stopped unexpectedly".into())),
            };
            if let Some(r) = r {
                if let Some(job) = self.history.thin.take() {
                    self.history_thin_done(job, r, now);
                }
            }
        }
        if let Some(job) = &self.history.query {
            let r = match job.job.try_recv() {
                Ok(r) => Some(r.unwrap_or_else(Err)),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    None
                }
                Err(TryRecvError::Disconnected) => Some(Err("the history could not be read".into())),
            };
            if let Some(r) = r {
                if let Some(job) = self.history.query.take() {
                    if self.vault.as_ref().map(|v| v.root.as_path()) == Some(job.vault.as_path()) {
                        self.history_query_done(job.query, r);
                    }
                    ctx.request_repaint();
                }
            }
        }
    }

    /// A thinning ended (SYNC-070): the next one is a day away, a busy one is tried again soon; the
    /// versions the panel shows may have other commit ids now.
    fn history_thin_done(&mut self, job: ThinJob, r: Result<thin::Outcome, String>, now: u64) {
        let same_vault = self.vault.as_ref().map(|v| v.root.as_path()) == Some(job.vault.as_path());
        if !same_vault {
            return;
        }
        self.history.thin_next = now + thin::INTERVAL_SECS;
        match r {
            Ok(thin::Outcome::Busy(why)) => {
                tracing::info!("thinning later: {why}");
                self.history.thin_next = now + THIN_RETRY;
            }
            Ok(thin::Outcome::Thinned { kept, dropped }) => {
                tracing::info!("the history was thinned: {kept} versions kept, {dropped} dropped");
                self.history_refresh();
            }
            Ok(_) => {}
            Err(e) => self.toast(format!("Could not thin the history: {e}")),
        }
    }

    fn history_snapshot_done(&mut self, ctx: &egui::Context, job: SnapJob, r: Result<Outcome, String>, now: u64) {
        let SnapJob { vault, queued, .. } = job;
        let same_vault = self.vault.as_ref().map(|v| v.root.as_path()) == Some(vault.as_path());
        match r {
            Ok(Outcome::Busy(why)) => {
                tracing::info!("snapshot later: {why}");
                self.history.not_before = now + RETRY_SECS;
                if queued.scheduled {
                    self.history.sched.again(&queued.req.paths, now);
                } else {
                    self.history.queue.push_front(queued);
                }
            }
            Ok(outcome) => {
                self.history.last_error = None;
                if let (Some(Then::Restore { rel, commit, path }), true) = (queued.then, same_vault) {
                    self.history.queries.push_back(Query::Content { commit: commit.clone(), path, then: Use::Restore { rel, commit } });
                    self.history_start_query(ctx);
                }
                if matches!(outcome, Outcome::Committed(_)) {
                    self.history_refresh();
                }
            }
            Err(e) => {
                self.history.not_before = now + 30;
                if self.history.last_error.as_deref() != Some(e.as_str()) {
                    self.history.last_error = Some(e.clone());
                    self.toast(format!("Could not keep the history: {e}"));
                }
            }
        }
    }

    /// The versions may have changed (a snapshot was made, a sync ended): the panel reads its list
    /// again when it is shown, and the lists that are open now.
    pub(crate) fn history_refresh(&mut self) {
        self.history.panel.stale = true;
        if matches!(self.modal, Some(super::Modal::History)) {
            self.history.deleted.loading = true;
            self.history.queries.push_back(Query::Deleted);
        }
    }

    /// Every frame, in `history_tick`: the panel asks for the list of the note it shows, when it has
    /// none or an old one (HIST-007).
    fn history_panel_tick(&mut self) {
        let Some(want) = self.history.panel.want.take() else { return };
        let panel = &mut self.history.panel;
        if panel.loading || (panel.note.as_ref() == Some(&want) && !panel.stale) {
            return;
        }
        panel.loading = true;
        panel.stale = false;
        panel.error = None;
        self.history.queries.push_back(Query::Versions(want));
    }

    fn history_query_done(&mut self, query: Query, r: Result<Answer, String>) {
        let answer = match r {
            Ok(a) => a,
            Err(e) => {
                match query {
                    Query::Versions(_) => {
                        self.history.panel.loading = false;
                        self.history.panel.error = Some(e.clone());
                    }
                    Query::Deleted => {
                        self.history.deleted.loading = false;
                        self.history.deleted.error = Some(e.clone());
                    }
                    Query::Compare(_) => {
                        self.history.changes.loading = false;
                        self.history.changes.error = Some(e.clone());
                    }
                    _ => {}
                }
                self.toast(format!("Could not read the history: {e}"));
                return;
            }
        };
        match (query, answer) {
            (Query::Versions(rel), Answer::Versions(list)) => {
                let panel = &mut self.history.panel;
                panel.note = Some(rel);
                panel.versions = list;
                panel.loading = false;
                panel.error = None;
            }
            (Query::Deleted, Answer::Deleted(list)) => {
                self.history.deleted.list = list;
                self.history.deleted.loading = false;
                self.history.deleted.error = None;
            }
            (Query::Compare(c), Answer::Compare(loaded)) => {
                let Compared { rel, from, to, .. } = *c;
                let ch = &mut self.history.changes;
                // an answer for ends the view has moved away from is dropped
                if ch.rel == rel && ch.from == from.commit && ch.to == to.map(|t| t.commit) {
                    ch.data = Some(Data::of(&rel, *loaded));
                    ch.loading = false;
                    ch.error = None;
                    ch.current = 0;
                    ch.jump = true;
                    ch.open_folds.clear();
                }
            }
            (Query::Content { then, .. }, Answer::Text(bytes)) => self.history_use_text(then, bytes),
            (Query::DeletedContent(d), Answer::Text(bytes)) => self.history_use_text(Use::RestoreDeleted(d), bytes),
            _ => {}
        }
    }

    /// A text read from the history arrived: copy it, show it or put it back.
    fn history_use_text(&mut self, what: Use, bytes: Vec<u8>) {
        let Ok(text) = String::from_utf8(bytes) else {
            self.toast("That version is not text, so it can't be shown or restored");
            return;
        };
        match what {
            Use::Copy => match self.ctx.clone() {
                Some(ctx) => {
                    ctx.copy_text(text);
                    self.toast("Copied the text of that version");
                }
                None => self.toast("Could not copy: there is no window"),
            },
            Use::View(title) => self.history.changes.alone = Some((title, text)),
            Use::Restore { rel, commit } => self.history_apply_restore(&rel, &commit, text),
            Use::RestoreDeleted(d) => self.history_apply_restore_deleted(&d, text),
        }
    }

    /// Why nothing can be read: history is off, or this window keeps none.
    pub(crate) fn history_unavailable(&mut self) -> Option<String> {
        if self.history_active() {
            None
        } else {
            Some(self.history_off_reason().unwrap_or_else(|| "The history is not kept in this window.".into()))
        }
    }

    /// Show the history of a note (HIST-003): the column in the margin when the window has room for
    /// it, else the panel of the right sidebar.
    pub fn history_show(&mut self, _rel: &Path) {
        self.cfg.history.show_panel = true;
        self.save_config();
        if self.history.panel.place != Some(crate::history::place::Place::Margin) {
            self.show_history_in_sidebar();
        }
    }

    /// Open the comparison of a version with the note as it is now (HIST-008).
    pub fn open_history_changes(&mut self, rel: &Path, v: &Version) {
        self.history.changes = Changes { rel: rel.to_path_buf(), from: v.commit.clone(), ..Default::default() };
        self.modal = Some(super::Modal::Changes);
        self.history_compare();
    }

    /// Read the two ends of the comparison again, after they changed (HIST-014).
    pub(crate) fn history_compare(&mut self) {
        let ch = &self.history.changes;
        let versions = &self.history.panel.versions;
        let Some(from) = versions.iter().find(|v| v.commit == ch.from).cloned() else { return };
        let to = ch.to.as_ref().and_then(|c| versions.iter().find(|v| &v.commit == c).cloned());
        let now = crate::calendar::time::now();
        let rel = ch.rel.clone();
        let name = rel_to_string(&rel);
        let labels = (format!("{name} ({})", crate::history::labels::day_time(from.time, now)), format!("{name} ({})", to.as_ref().map_or("now".to_string(), |t| crate::history::labels::day_time(t.time, now))));
        // the open buffer lacks the `created:` and `modified:` lines a save adds (VAULT-045), so "now" is the buffer as a save would write it
        let on_disk = self.vault.as_ref().and_then(|v| v.notes.get(&rel)).map(|n| n.text.as_str());
        let live = if to.is_none() { self.notes.get(&rel).filter(|d| d.unreadable.is_none()).map(|d| crate::note_props::stamp(&d.text, on_disk, now).unwrap_or_else(|| d.text.clone())) } else { None };
        self.history.changes.loading = true;
        self.history.changes.data = None;
        self.history.queries.push_back(Query::Compare(Box::new(Compared { rel, from, to, live, labels })));
    }

    /// Open the list of the deleted notes (SYNC-040).
    pub fn open_history_deleted(&mut self) {
        self.history.deleted = DeletedList::default();
        match self.history_unavailable() {
            Some(why) => self.history.deleted.error = Some(why),
            None => {
                self.history.deleted.loading = true;
                self.history.queries.push_back(Query::Deleted);
            }
        }
        self.modal = Some(super::Modal::History);
    }

    /// "Restore this version" (SYNC-038): the saved text is snapshotted first, then the old text becomes
    /// the note's content as one new change (see [`ShaleApp::history_apply_restore`]).
    pub fn history_restore_version(&mut self, rel: &Path, v: &Version) {
        if !self.history_active() {
            self.toast("The history is off, so a version can't be restored");
            return;
        }
        // what is on the screen is saved first, so the pre-restore snapshot holds it
        self.flush_all();
        let then = Then::Restore { rel: rel.to_path_buf(), commit: v.commit.clone(), path: v.path.clone() };
        self.history.queue.push_front(Queued { req: Request::new(Reason::Save, vec![rel.to_path_buf()]), then: Some(then), scheduled: false });
        self.history.not_before = 0;
    }

    /// "Copy text from this version" (SYNC-039).
    pub fn history_copy_version(&mut self, v: &Version) {
        self.history.queries.push_back(Query::Content { commit: v.commit.clone(), path: v.path.clone(), then: Use::Copy });
    }

    /// "Open this version read-only" (SYNC-039).
    pub fn history_view_version(&mut self, v: &Version) {
        let title = format!("{} as of {}", rel_to_string(&v.path), when(v.time));
        self.history.queries.push_back(Query::Content { commit: v.commit.clone(), path: v.path.clone(), then: Use::View(title) });
    }

    /// "Restore" a deleted note (SYNC-040).
    pub fn history_restore_deleted(&mut self, d: &Deleted) {
        if !self.history_active() {
            self.toast("The history is off, so a note can't be restored");
            return;
        }
        self.history.queries.push_back(Query::DeletedContent(d.clone()));
    }

    /// Put the text of an old version in the note as a new change (SYNC-038): in the open buffer as
    /// one undo step (EDIT-021), which is saved at once, or written like any save (DATA-003) when the
    /// note is not open; then a snapshot with reason `restore` names the version.
    fn history_apply_restore(&mut self, rel: &Path, commit: &str, text: String) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        let name = rel_to_string(rel);
        let saved = if self.notes.contains_key(rel) {
            if let Some(doc) = self.notes.get_mut(rel) {
                if doc.unreadable.is_some() {
                    self.toast(format!("“{name}” could not be read, so it is not changed"));
                    return;
                }
                self.undo.of(rel).checkpoint(&doc.text);
                doc.text = text;
                self.undo.of(rel).checkpoint(&doc.text);
                doc.dirty = true;
                doc.last_edit = std::time::Instant::now();
            }
            self.save_note(rel);
            self.notes.get(rel).is_some_and(|d| !d.dirty)
        } else if self.drawings.contains_key(rel) {
            let Some(old) = self.drawings.get(rel) else { return };
            let mut doc = crate::drawing::DrawingDoc::new(rel.to_path_buf(), &text);
            doc.base = old.base;
            doc.dirty = true;
            self.scenes.put(rel, doc.scene.clone());
            self.drawings.insert(rel.to_path_buf(), doc);
            self.save_drawing(rel);
            self.drawings.get(rel).is_some_and(|d| !d.dirty)
        } else {
            let Some(v) = self.vault.as_mut() else { return };
            let base = crate::fsutil::read(v.abs(rel)).map(|b| crate::vault::Stamp::of_bytes(&b)).unwrap_or_default();
            match v.save_checked(rel, &text, base) {
                Ok(crate::vault::Saved::Written(_)) => true,
                Ok(crate::vault::Saved::Conflict { .. }) => false,
                Err(e) => {
                    self.toast(format!("Could not restore “{name}”: {e}"));
                    return;
                }
            }
        };
        if !saved {
            self.toast(format!("“{name}” could not be saved yet; the old text stays in it until it is"));
            return;
        }
        let at = self.history.panel.versions.iter().find(|v| v.commit == commit).map(|v| format!(" of {}", when(v.time))).unwrap_or_default();
        self.toast(format!("Restored “{name}” to the version{at}"));
        self.history.sched.forget(rel);
        let mut req = Request::new(Reason::Restore, vec![rel.to_path_buf()]);
        req.restored = Some(commit.to_string());
        self.history.queue.push_back(Queued { req, then: None, scheduled: false });
    }

    /// Write a deleted note back (SYNC-040): at its path, or at a free name when that is taken.
    fn history_apply_restore_deleted(&mut self, d: &Deleted, text: String) {
        let Some(_tree) = self.hold_sync_tree() else { return };
        let Some(v) = self.vault.as_mut() else { return };
        let target = if v.abs(&d.path).symlink_metadata().is_ok() { history::restored_name(&d.path, |p| v.abs(p).symlink_metadata().is_ok()) } else { d.path.clone() };
        let abs = v.abs(&target);
        let written = abs.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| crate::fsutil::create_new(&abs, text.as_bytes()));
        if let Err(e) = written {
            self.toast(format!("Could not restore “{}”: {e}", rel_to_string(&d.path)));
            return;
        }
        v.rescan();
        self.toast(format!("Restored “{}”", rel_to_string(&target)));
        let mut req = Request::new(Reason::Restore, vec![target.clone()]);
        req.restored = Some(d.commit.clone());
        self.history.queue.push_back(Queued { req, then: None, scheduled: false });
        self.open(&target, true);
        self.history_refresh();
    }

    /// A note or folder was renamed or moved (SYNC-041): one snapshot of the old and the new path and
    /// of the notes whose links were rewritten.
    pub(crate) fn history_renamed(&mut self, from: &Path, to: &Path, changed: &[PathBuf]) {
        self.history.sched.take_under(from);
        let mut paths = vec![from.to_path_buf(), to.to_path_buf()];
        paths.extend(changed.iter().cloned());
        self.history.queue.push_back(Queued { req: Request::new(Reason::Rename, paths), then: None, scheduled: false });
    }

    /// An import wrote notes and rewrote the `previous` / `next` line of neighbours in a thread: one
    /// snapshot of all of them (IMP-072, ADR-0028).
    pub(crate) fn history_imported(&mut self, made: &[PathBuf], touched: &[PathBuf]) {
        let paths: Vec<PathBuf> = made.iter().chain(touched).cloned().collect();
        if !paths.is_empty() {
            self.history.queue.push_back(Queued { req: Request::new(Reason::Import, paths), then: None, scheduled: false });
        }
    }

    /// Before a note or folder goes to the trash, the caller holding the tree lock (SYNC-046): the saved
    /// text that no snapshot has yet is snapshotted, now.
    pub(crate) fn history_before_trash(&mut self, rel: &Path) {
        let pending = self.history.sched.take_under(rel);
        if pending.is_empty() || !self.history_active() {
            return;
        }
        let Some((git, root, hist, opts)) = self.history_job_data() else { return };
        let result = history::locate(&git, &root, &hist).and_then(|repo| history::snapshot_held(&repo, &opts, &Request::new(Reason::Save, pending)));
        if let Err(e) = result {
            self.toast(format!("Could not keep the history: {e}"));
        }
    }

    /// A note or folder went to the trash: record the deletion (SYNC-046).
    pub(crate) fn history_trashed(&mut self, rel: &Path) {
        self.history.queue.push_back(Queued { req: Request::new(Reason::Delete, vec![rel.to_path_buf()]), then: None, scheduled: false });
    }

    /// At window close and before another vault opens (SYNC-033): wait a moment for the running
    /// snapshot, then snapshot what is still waiting. Best effort: a busy repository is left to the
    /// next start.
    pub(crate) fn history_flush_now(&mut self) {
        if !self.history_active() {
            return;
        }
        if let Some(job) = self.history.job.take() {
            let _ = job.job.recv_timeout(EXIT_WAIT);
        }
        if let Some(job) = self.history.thin.take() {
            let _ = job.job.recv_timeout(EXIT_WAIT);
        }
        let mut reqs: Vec<Request> = self.history.queue.drain(..).filter(|q| q.then.is_none() && q.req.reason != Reason::Initial).map(|q| q.req).collect();
        let paths = self.history.sched.take_all();
        if !paths.is_empty() {
            reqs.push(Request::new(Reason::Close, paths));
        }
        if reqs.is_empty() {
            return;
        }
        let Some((git, root, hist, opts)) = self.history_job_data() else { return };
        let Ok(repo) = history::locate(&git, &root, &hist) else { return };
        for req in reqs {
            match history::snapshot(&repo, &opts, &req, &self.sync.tree, EXIT_WAIT) {
                Ok(Outcome::Busy(_)) => return,
                Ok(_) => {}
                Err(e) => eprintln!("could not keep the history: {e}"),
            }
        }
    }

    /// Nothing is waiting or running (tests).
    #[cfg(test)]
    pub(crate) fn history_quiet(&self) -> bool {
        self.history.quiet() && self.history.query.is_none() && self.history.queries.is_empty()
    }
}

/// Run a read of the history (in a job).
fn run_query(repo: &Repo, q: &Query) -> Result<Answer, String> {
    Ok(match q {
        Query::Versions(rel) => Answer::Versions(history::versions(repo, rel, LIST_LIMIT)?),
        Query::Deleted => Answer::Deleted(history::deleted(repo, LIST_LIMIT)?),
        Query::Content { commit, path, .. } => Answer::Text(history::content_at(repo, commit, path)?),
        Query::DeletedContent(d) => Answer::Text(history::deleted_content(repo, d)?),
        Query::Compare(c) => {
            let Compared { rel, from, to, live, labels } = &**c;
            let old = history::content_at(repo, &from.commit, &from.path)?;
            let (new, binary) = match (to, live) {
                (Some(t), _) => (history::content_at(repo, &t.commit, &t.path)?, from.binary || t.binary),
                (None, Some(text)) => (text.clone().into_bytes(), from.binary),
                (None, None) => (crate::fsutil::read(repo.vault.join(rel)).map_err(|e| e.to_string())?, from.binary),
            };
            Answer::Compare(Box::new(Loaded::of(rel, binary, old, new, (&labels.0, &labels.1))))
        }
    })
}

#[cfg(test)]
mod history_tests;
#[cfg(test)]
mod history_ui_tests;
