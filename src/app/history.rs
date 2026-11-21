//! File history in the app: when snapshots run, what happens to open buffers when a version is
//! restored, and the actions the history panel calls (SYNC-032…SYNC-046, ADR-0028).
//!
//! Public actions (functions of [`ShaleApp`], for the history panel #472 and the wizard #469):
//!
//! - `open_history_versions(&Path)`, `open_history_deleted()`: open the list dialog (until #472).
//! - `history_restore_version(&Path, &Version)`, `history_copy_version(&Path, &Version)`,
//!   `history_view_version(&Path, &Version)`, `history_restore_deleted(&Deleted)`.
//! - `history_active() -> bool`, `history_off_reason() -> Option<String>`.
//!
//! Every git call runs in a background job: snapshots one at a time and never while the sync runs
//! (SYNC-035), reads (versions, text at a version, deleted notes) beside them.

use super::{unix_now, ShaleApp};
use crate::gitsync::history::{self, Deleted, HistoryConfig, Outcome, Reason, Repo, Request, Schedule, Version};
use crate::gitsync::{Git, Options};
use crate::vault::rel_to_string;
use eframe::egui;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

/// How long a snapshot waits for the tree lock before it is asked for again later (SYNC-035).
const TREE_WAIT: Duration = Duration::from_secs(2);
/// How long the window waits for the last snapshots when it closes or the vault is switched (SYNC-033).
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// Seconds before a snapshot that was busy or failed is tried again.
const RETRY_SECS: u64 = 5;
/// How many versions and deleted notes the list asks for.
const LIST_LIMIT: usize = 300;

/// What a job sends back: its result, or why it died.
type Sent<T> = Result<Result<T, String>, String>;

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
    rx: Receiver<Sent<Outcome>>,
}

/// A read of the history.
#[derive(Clone)]
enum Query {
    Versions(PathBuf),
    Deleted,
    Content { commit: String, path: PathBuf, then: Use },
    DeletedContent(Deleted),
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
}

struct QueryJob {
    vault: PathBuf,
    query: Query,
    rx: Receiver<Sent<Answer>>,
}

/// Which list the dialog shows.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Tab {
    /// The versions of this note.
    Note(PathBuf),
    Deleted,
}

/// The list dialog (SYNC-045), until the panel of #472.
#[derive(Default)]
pub struct HistoryDialog {
    pub tab: Option<Tab>,
    /// The note the dialog was opened for: its tab stays when "Deleted notes" is shown.
    pub note: Option<PathBuf>,
    pub versions: Vec<Version>,
    pub deleted: Vec<Deleted>,
    pub loading: bool,
    pub error: Option<String>,
    /// An old text shown read-only: its title and the text (SYNC-039).
    pub view: Option<(String, String)>,
    /// Where the buttons were drawn, for UI tests.
    pub(crate) rects: Vec<(String, egui::Rect)>,
}

/// The history's state in the app.
#[derive(Default)]
pub struct HistoryState {
    /// What is saved and not yet snapshotted (SYNC-033).
    pub sched: Schedule,
    queue: VecDeque<Queued>,
    job: Option<SnapJob>,
    queries: VecDeque<Query>,
    query: Option<QueryJob>,
    /// The vault the first snapshot was asked for.
    started_for: Option<PathBuf>,
    /// No snapshot job before this time (unix seconds).
    not_before: u64,
    /// The last error that was shown, so it is shown once.
    last_error: Option<String>,
    pub dialog: HistoryDialog,
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

    /// A snapshot job is running.
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// A note or drawing was saved (SYNC-033).
    pub fn note_saved(&mut self, rel: &Path) {
        let now = self.now();
        self.sched.saved(rel, now);
    }

    #[cfg(test)]
    fn quiet(&self) -> bool {
        self.job.is_none() && self.queue.is_empty()
    }
}

/// "2026-11-21 15:04" in local time, for a unix time.
pub fn when(ts: i64) -> String {
    let (days, sod) = crate::calendar::time::utc_to_local(ts);
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {}", crate::calendar::time::hhmm(sod))
}

/// What a reason is called in the list.
pub fn reason_label(r: Option<Reason>) -> &'static str {
    match r {
        Some(Reason::Save) => "Saved",
        Some(Reason::Idle) => "Saved (idle)",
        Some(Reason::Close) => "Closed",
        Some(Reason::Initial) => "Start of the history",
        Some(Reason::Sync) => "Synced",
        Some(Reason::Merge) => "Merged",
        Some(Reason::Restore) => "Restored",
        Some(Reason::Rename) => "Renamed",
        Some(Reason::Delete) => "Deleted",
        None => "Changed by another program",
    }
}

impl ShaleApp {
    /// Does history run now (SYNC-032): it is on, a vault is open, there is a git and, as with the
    /// automatic sync, the window keeps its settings (a window built for a test does not).
    pub fn history_active(&mut self) -> bool {
        #[cfg(test)]
        let persist = self.persist_config || self.history.on;
        #[cfg(not(test))]
        let persist = self.persist_config;
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
    fn history_job_data(&self) -> Option<(Git, PathBuf, PathBuf, Options)> {
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
            // a new local history starts with the vault as it is (SYNC-044)
            self.history.queue.push_back(Queued { req: Request::new(Reason::Initial, vec![]), then: None, scheduled: false });
        }
        self.history_start_query(ctx);
        let sync_busy = self.sync.running() || self.sync.lfs_job_running();
        if self.history.job.is_none() && !sync_busy && now >= self.history.not_before {
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
                self.history_start_snapshot(ctx, root, q);
            }
        }
        if let Some(secs) = self.history.sched.next_in(now, &self.cfg.history) {
            ctx.request_repaint_after(Duration::from_secs(secs.clamp(1, 30)));
        }
    }

    fn history_start_snapshot(&mut self, ctx: &egui::Context, vault: PathBuf, queued: Queued) {
        let Some((git, root, hist, opts)) = self.history_job_data() else { return };
        let (tree, req, repaint) = (self.sync.tree.clone(), queued.req.clone(), ctx.clone());
        let rx = crate::jobs::spawn(
            move || {
                let repo = history::locate(&git, &root, &hist)?;
                history::snapshot(&repo, &opts, &req, &tree, TREE_WAIT)
            },
            move || repaint.request_repaint(),
        );
        self.history.job = Some(SnapJob { vault, queued, rx });
    }

    fn history_start_query(&mut self, ctx: &egui::Context) {
        if self.history.query.is_some() {
            return;
        }
        let Some(query) = self.history.queries.pop_front() else { return };
        let Some((git, root, hist, _)) = self.history_job_data() else { return };
        let (job_query, repaint, job_root) = (query.clone(), ctx.clone(), root.clone());
        let rx = crate::jobs::spawn(
            move || {
                let repo = history::locate(&git, &job_root, &hist)?;
                run_query(&repo, &job_query)
            },
            move || repaint.request_repaint(),
        );
        self.history.query = Some(QueryJob { vault: root, query, rx });
    }

    /// Take what the jobs finished.
    fn history_collect(&mut self, ctx: &egui::Context) {
        let now = self.history.now();
        if let Some(job) = &self.history.job {
            let r = match job.rx.try_recv() {
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
        if let Some(job) = &self.history.query {
            let r = match job.rx.try_recv() {
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
                    self.history_refresh_dialog();
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

    /// Ask for the list the dialog shows again, after something changed it.
    fn history_refresh_dialog(&mut self) {
        let query = match (&self.modal, &self.history.dialog.tab) {
            (Some(super::Modal::History), Some(Tab::Note(rel))) => Query::Versions(rel.clone()),
            (Some(super::Modal::History), Some(Tab::Deleted)) => Query::Deleted,
            _ => return,
        };
        self.history.dialog.loading = true;
        self.history.queries.push_back(query);
    }

    fn history_query_done(&mut self, query: Query, r: Result<Answer, String>) {
        let answer = match r {
            Ok(a) => a,
            Err(e) => {
                self.history.dialog.loading = false;
                self.history.dialog.error = Some(e.clone());
                self.toast(format!("Could not read the history: {e}"));
                return;
            }
        };
        match (query, answer) {
            (Query::Versions(rel), Answer::Versions(list)) => {
                if self.history.dialog.tab == Some(Tab::Note(rel)) {
                    self.history.dialog.versions = list;
                    self.history.dialog.loading = false;
                    self.history.dialog.error = None;
                }
            }
            (Query::Deleted, Answer::Deleted(list)) => {
                self.history.dialog.deleted = list;
                self.history.dialog.loading = false;
                self.history.dialog.error = None;
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
            Use::View(title) => self.history.dialog.view = Some((title, text)),
            Use::Restore { rel, commit } => self.history_apply_restore(&rel, &commit, text),
            Use::RestoreDeleted(d) => self.history_apply_restore_deleted(&d, text),
        }
    }

    /// The dialog asks for a list; with the history off it says why instead.
    fn history_ask(&mut self, dialog: &mut HistoryDialog, query: Query) {
        match self.history_off_reason().filter(|_| !self.history_active()) {
            Some(why) => dialog.error = Some(why),
            None if !self.history_active() => dialog.error = Some("The history is not kept in this window.".into()),
            None => {
                dialog.loading = true;
                self.history.queries.push_back(query);
            }
        }
    }

    /// Open the list of the versions of a note (SYNC-045).
    pub fn open_history_versions(&mut self, rel: &Path) {
        let mut dialog = HistoryDialog { tab: Some(Tab::Note(rel.to_path_buf())), note: Some(rel.to_path_buf()), ..Default::default() };
        self.history_ask(&mut dialog, Query::Versions(rel.to_path_buf()));
        self.history.dialog = dialog;
        self.modal = Some(super::Modal::History);
    }

    /// Open the list of the deleted notes (SYNC-040, SYNC-045).
    pub fn open_history_deleted(&mut self) {
        let note = match (&self.modal, &self.history.dialog.note) {
            (Some(super::Modal::History), n) => n.clone(),
            _ => self.current_path().filter(|p| self.notes.contains_key(p) || self.drawings.contains_key(p)),
        };
        let mut dialog = HistoryDialog { tab: Some(Tab::Deleted), note, ..Default::default() };
        self.history_ask(&mut dialog, Query::Deleted);
        self.history.dialog = dialog;
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
        let at = self.history.dialog.versions.iter().find(|v| v.commit == commit).map(|v| format!(" of {}", when(v.time))).unwrap_or_default();
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
        self.history_refresh_dialog();
    }

    /// A note or folder was renamed or moved (SYNC-041): one snapshot of the old and the new path and
    /// of the notes whose links were rewritten.
    pub(crate) fn history_renamed(&mut self, from: &Path, to: &Path, changed: &[PathBuf]) {
        self.history.sched.take_under(from);
        let mut paths = vec![from.to_path_buf(), to.to_path_buf()];
        paths.extend(changed.iter().cloned());
        self.history.queue.push_back(Queued { req: Request::new(Reason::Rename, paths), then: None, scheduled: false });
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
            let _ = job.rx.recv_timeout(EXIT_WAIT);
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
    })
}

#[cfg(test)]
mod history_tests;
