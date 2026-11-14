//! The mail view's state and the work behind it (ADR-0007, phase 1b).
//!
//! `MailView` owns what the window shows: the opened store, the folder list with its counts, the
//! page cache of the message list, the open message, the search, the syncs that run and the
//! jobs behind the buttons. Everything slow (opening the store, reading and parsing a message,
//! searching, talking to a server) runs as a job through `jobs` and reports back over one
//! channel, so a frame never waits for it. The drawing is in `mail_view.rs`, the account
//! settings in `mail_dialog.rs`.
//!
//! Nothing here changes a server: the only server commands are the read-only ones of `imap_sync`.

use crate::config::{Config, MailAccount};
use crate::imap_client::decode_mutf7;
use crate::imap_sync::{self, ConnectorFactory, FolderKind, LazyConnector, Progress, SyncOptions, SyncSummary, NO_PASSWORD};
use crate::mail::Mail;
use crate::mail_db::{FolderRow, MessageSummary, STATE_HEADER, STATE_TOO_LARGE};
use crate::mail_store::{MailStore, Reader};
use eframe::egui;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Messages read from the index at a time (MAIL-052).
pub const PAGE: usize = 200;
/// How many headers of the newest mail a listing of Trash or Junk fetches.
pub const LISTING_LIMIT: usize = 1000;
/// The most hits one search shows.
pub const SEARCH_LIMIT: usize = 2000;
/// Typing pauses this long before a search starts (MAIL-060).
pub const DEBOUNCE: Duration = Duration::from_millis(300);
/// A message text longer than this many characters is cut in the reading pane (MAIL-053).
pub const TEXT_CAP: usize = 200_000;
/// While a sync runs the counts and the list are read again at most this often.
const REFRESH_EVERY: Duration = Duration::from_millis(800);

/// What the store is doing.
pub enum StoreState {
    Closed,
    Opening,
    Ready(Arc<MailStore>),
    Failed(String),
}

/// A message read and parsed for the reading pane.
pub struct Loaded {
    pub mail: Mail,
    /// The text as shown (cut at `TEXT_CAP`).
    pub shown: String,
    pub cut: bool,
}

/// What the reading pane has for the open message.
pub enum Body {
    Loading,
    Ready(Arc<Loaded>),
    Failed(String),
    /// Headers only (Trash, Junk): "Load this message".
    NeedsDownload,
    /// Over the size limit: "Load this message".
    TooLarge,
    /// The server is being asked for it.
    Fetching,
}

pub struct OpenMail {
    pub msg: MessageSummary,
    pub body: Body,
}

/// The state of a listing of a folder that is not synced.
pub enum ListingState {
    Loading,
    Done(usize),
    Failed(String),
}

/// What "Test connection" found.
pub enum TestState {
    Running,
    Ok(String),
    Failed(String),
}

/// One sync of one account, running or finished.
pub struct SyncRun {
    rx: Receiver<Progress>,
    pub cancel: Arc<AtomicBool>,
    pub folder: String,
    pub done: usize,
    pub total: usize,
    pub added: usize,
    pub errors: Vec<String>,
    pub finished: Option<SyncSummary>,
}

impl SyncRun {
    pub fn running(&self) -> bool {
        self.finished.is_none()
    }
}

/// Asked when an account has no password to sign in with (MAIL-059).
pub struct PasswordPrompt {
    pub account: String,
    pub text: String,
    pub retry: Retry,
}

/// What to do again once the password is known.
#[derive(Clone)]
pub enum Retry {
    Sync,
    Listing(i64),
    Message { folder: i64, uid: u32 },
}

/// What the window must do for the app, returned by `show`.
pub enum MailAction {
    OpenAccounts,
    /// Hand these messages to the Import dialog (MAIL-070).
    Import(Vec<MessageSummary>),
    /// Save attachment `index` of the open message into the vault.
    SaveAttachment {
        loaded: Arc<Loaded>,
        index: usize,
    },
    Toast(String),
}

enum Event {
    Store(Result<Arc<MailStore>, String>),
    Search { gen: u64, result: Result<Vec<MessageSummary>, String> },
    Message { id: i64, result: Result<Arc<Loaded>, String> },
    Fetched { key: (i64, u32), retry: Retry, result: Result<Box<MessageSummary>, String> },
    Listing { folder: i64, result: Result<usize, String> },
    Tested { account: String, result: Result<usize, String> },
    Secret { account: String, stored: bool },
}

pub struct MailView {
    /// Where the store is (the default, or a test folder).
    pub root: PathBuf,
    pub store: StoreState,
    pub factory: ConnectorFactory,
    pub opts: SyncOptions,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    ctx: Option<egui::Context>,
    reader: Option<Reader>,
    // data read from the index when something changed
    pub folders: Vec<FolderRow>,
    pub counts: HashMap<i64, (i64, i64)>,
    dirty: bool,
    last_refresh: Option<Instant>,
    /// Test aid: how many times the list or counts were read from the index.
    pub queries: u64,
    // the list
    pub folder: Option<i64>,
    pub total: usize,
    pages: HashMap<usize, Vec<MessageSummary>>,
    pub cursor: Option<usize>,
    pub marked: BTreeMap<i64, MessageSummary>,
    pub scroll_to_cursor: bool,
    /// Rows drawn last frame and the height of the list, for keeping the cursor in view.
    pub visible: std::ops::Range<usize>,
    pub list_h: f32,
    // search
    pub query: String,
    pub search_all: bool,
    pub results: Option<Vec<MessageSummary>>,
    pub searching: bool,
    pub search_error: Option<String>,
    search_gen: u64,
    search_edited: Option<Instant>,
    pub focus_search: bool,
    /// Enter in the search field: put the cursor on the first hit when the results arrive.
    pub to_results: bool,
    // the open message
    pub open: Option<OpenMail>,
    fetching: HashSet<(i64, u32)>,
    pub listings: HashMap<i64, ListingState>,
    // sync
    pub syncs: HashMap<String, SyncRun>,
    last_auto: Option<Instant>,
    pub session_pw: HashMap<String, String>,
    pub prompt: Option<PasswordPrompt>,
    pub tests: HashMap<String, TestState>,
    /// Controls drawn in the last frame, by name (for the UI tests).
    pub rects: Vec<(String, egui::Rect)>,
    /// Things to tell the person (the app shows them as messages).
    pub notices: Vec<String>,
}

/// Opening the store and the jobs report errors as text; a panic in a job is one too.
fn flatten<T>(r: Result<Result<T, String>, String>) -> Result<T, String> {
    r.and_then(|x| x)
}

impl MailView {
    pub fn new(root: PathBuf) -> MailView {
        let (tx, rx) = channel();
        MailView {
            root,
            store: StoreState::Closed,
            factory: imap_sync::real_factory(),
            opts: SyncOptions::default(),
            tx,
            rx,
            ctx: None,
            reader: None,
            folders: vec![],
            counts: HashMap::new(),
            dirty: false,
            last_refresh: None,
            queries: 0,
            folder: None,
            total: 0,
            pages: HashMap::new(),
            cursor: None,
            marked: BTreeMap::new(),
            scroll_to_cursor: false,
            visible: 0..0,
            list_h: 400.0,
            query: String::new(),
            search_all: true,
            results: None,
            searching: false,
            search_error: None,
            search_gen: 0,
            search_edited: None,
            focus_search: false,
            to_results: false,
            open: None,
            fetching: HashSet::new(),
            listings: HashMap::new(),
            syncs: HashMap::new(),
            last_auto: None,
            session_pw: HashMap::new(),
            prompt: None,
            tests: HashMap::new(),
            rects: vec![],
            notices: vec![],
        }
    }

    #[cfg(test)]
    pub fn reader_for_tests(&self) -> Option<&Reader> {
        self.reader.as_ref()
    }

    pub fn set_context(&mut self, ctx: &egui::Context) {
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
    }

    pub fn ready_store(&self) -> Option<Arc<MailStore>> {
        match &self.store {
            StoreState::Ready(s) => Some(s.clone()),
            _ => None,
        }
    }

    /// Run `f` as a job; its outcome comes back as an event and wakes the window.
    fn spawn<T: Send + 'static>(&self, f: impl FnOnce() -> T + Send + 'static, wrap: impl FnOnce(Result<T, String>) -> Event + Send + 'static) {
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        crate::jobs::run(f, move |r| {
            let _ = tx.send(wrap(r));
            if let Some(c) = ctx {
                c.request_repaint();
            }
        });
    }

    /// Open the store in the background, once (MAIL-050).
    pub fn open_store(&mut self) {
        if !matches!(self.store, StoreState::Closed) {
            return;
        }
        self.store = StoreState::Opening;
        let root = self.root.clone();
        self.spawn(move || MailStore::open(&root).map(Arc::new).map_err(|e| e.0), |r| Event::Store(flatten(r)));
    }

    // ------------------------------------------------------------ polling

    /// Take in what the jobs and syncs reported, start the timers' work. Called every frame by
    /// the app; returns true while something is going on (the window keeps repainting).
    pub fn tick(&mut self, cfg: &Config, wanted: bool) -> bool {
        if wanted || !cfg.mail_accounts.is_empty() {
            self.open_store();
        }
        while let Ok(ev) = self.rx.try_recv() {
            self.handle(ev, cfg);
        }
        self.poll_syncs();
        // the automatic sync (MAIL-058)
        if let (Some(_), true, minutes) = (self.ready_store(), !cfg.mail_accounts.is_empty(), cfg.mail_sync_minutes) {
            if minutes > 0 && self.last_auto.is_none_or(|t| t.elapsed() >= Duration::from_secs(u64::from(minutes) * 60)) {
                self.last_auto = Some(Instant::now());
                self.start_sync_all(cfg);
            }
        }
        if let Some(t) = self.search_edited {
            if t.elapsed() >= DEBOUNCE {
                self.search_edited = None;
                self.run_search();
            }
        }
        if self.dirty && self.last_refresh.is_none_or(|t| t.elapsed() >= REFRESH_EVERY || !self.any_sync_running()) {
            self.refresh();
        }
        // the first folder (the Inbox) opens by itself once there is one
        if self.folder.is_none() && !self.folders.is_empty() && self.reader.is_some() {
            self.select_first_folder(cfg);
        }
        self.busy()
    }

    /// Is a job, a sync or a debounce going on?
    pub fn busy(&self) -> bool {
        matches!(self.store, StoreState::Opening)
            || self.any_sync_running()
            || self.searching
            || self.search_edited.is_some()
            || self.dirty
            || self.fetching.len() + self.listings.values().filter(|l| matches!(l, ListingState::Loading)).count() > 0
            || self.tests.values().any(|t| matches!(t, TestState::Running))
            || matches!(self.open.as_ref().map(|o| &o.body), Some(Body::Loading))
    }

    fn handle(&mut self, ev: Event, cfg: &Config) {
        match ev {
            Event::Store(Ok(s)) => {
                let o = &s.opened;
                if let Some(q) = &o.quarantined {
                    let how = if o.restored_from.is_some() { "restored from a backup" } else { "rebuilt from the message files" };
                    self.notices.push(format!("The mail index was damaged: it was {how}. The damaged file is kept as {}.", q.display()));
                }
                self.reader = s.reader().ok();
                self.store = StoreState::Ready(s);
                self.refresh();
                if self.folder.is_none() {
                    self.select_first_folder(cfg);
                }
            }
            Event::Store(Err(e)) => self.store = StoreState::Failed(e),
            Event::Search { gen, result } if gen == self.search_gen => {
                self.searching = false;
                match result {
                    Ok(v) => {
                        self.cursor = if std::mem::take(&mut self.to_results) && !v.is_empty() { Some(0) } else { None };
                        self.results = Some(v);
                        self.search_error = None;
                    }
                    Err(e) => {
                        self.results = Some(vec![]);
                        self.search_error = Some(e);
                    }
                }
            }
            Event::Search { .. } => {}
            Event::Message { id, result } => {
                if let Some(o) = self.open.as_mut().filter(|o| o.msg.id == id) {
                    o.body = match result {
                        Ok(l) => Body::Ready(l),
                        Err(e) => Body::Failed(e),
                    };
                }
            }
            Event::Fetched { key, retry, result } => {
                self.fetching.remove(&key);
                match result {
                    Ok(m) => {
                        self.dirty = true;
                        self.refresh();
                        if self.open.as_ref().is_some_and(|o| (o.msg.folder_id, o.msg.uid) == key) {
                            self.open_message(*m);
                        }
                    }
                    Err(e) => {
                        if e.contains(NO_PASSWORD) {
                            self.ask_password(&key_account(&self.folders, key.0), retry);
                        }
                        if let Some(o) = self.open.as_mut().filter(|o| (o.msg.folder_id, o.msg.uid) == key) {
                            o.body = Body::Failed(e);
                        }
                    }
                }
            }
            Event::Listing { folder, result } => {
                match result {
                    Ok(n) => {
                        self.listings.insert(folder, ListingState::Done(n));
                    }
                    Err(e) => {
                        if e.contains(NO_PASSWORD) {
                            self.ask_password(&key_account(&self.folders, folder), Retry::Listing(folder));
                        }
                        self.listings.insert(folder, ListingState::Failed(e));
                    }
                }
                self.dirty = true;
            }
            Event::Secret { account, stored } => {
                if stored {
                    // the keyring has it now; from here on it is the one source
                    self.session_pw.remove(&account);
                } else {
                    self.notices.push("No keyring could keep the password: Shale remembers it until you quit and asks again next time.".into());
                }
            }
            Event::Tested { account, result } => {
                self.tests.insert(
                    account,
                    match result {
                        Ok(n) => TestState::Ok(format!("Connected. The account has {n} folder{}.", if n == 1 { "" } else { "s" })),
                        Err(e) => TestState::Failed(e),
                    },
                );
                self.dirty = true;
            }
        }
    }

    fn poll_syncs(&mut self) {
        let mut ended: Vec<String> = vec![];
        let mut changed = false;
        for (id, run) in self.syncs.iter_mut() {
            if run.finished.is_some() {
                continue;
            }
            loop {
                match run.rx.try_recv() {
                    Ok(Progress::FolderStarted { folder }) => {
                        run.folder = folder;
                        run.done = 0;
                        run.total = 0;
                    }
                    Ok(Progress::Folder { folder, done, total }) => {
                        run.folder = folder;
                        run.done = done;
                        run.total = total;
                        changed = true;
                    }
                    Ok(Progress::FolderDone { added, .. }) => {
                        run.added += added;
                        changed = true;
                    }
                    Ok(Progress::Error { folder, message }) => {
                        run.errors.push(match folder {
                            Some(f) => format!("{}: {message}", decode_mutf7(&f)),
                            None => message,
                        });
                    }
                    Ok(Progress::Finished(s)) => {
                        run.finished = Some(s);
                        changed = true;
                        ended.push(id.clone());
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        // the job always ends with Finished; if it did not, say so rather than spin
                        run.finished = Some(SyncSummary { errors: vec!["the sync stopped without a report".into()], ..Default::default() });
                        changed = true;
                        ended.push(id.clone());
                        break;
                    }
                }
            }
        }
        if changed {
            self.dirty = true;
        }
        for id in ended {
            let no_pw = self.syncs.get(&id).is_some_and(|r| r.errors.iter().any(|e| e.contains(NO_PASSWORD)) || r.finished.as_ref().is_some_and(|s| s.errors.iter().any(|e| e.contains(NO_PASSWORD))));
            if no_pw {
                self.ask_password(&id, Retry::Sync);
            }
            self.refresh();
        }
    }

    pub fn any_sync_running(&self) -> bool {
        self.syncs.values().any(|r| r.running())
    }

    /// The status bar's words while mail work is going on (MAIL-058).
    pub fn status(&self, cfg: &Config) -> Option<String> {
        let name = |id: &str| cfg.mail_accounts.iter().find(|a| a.id == id).map(|a| if a.name.is_empty() { a.host.clone() } else { a.name.clone() }).unwrap_or_else(|| id.to_string());
        let mut parts = vec![];
        for (id, r) in self.syncs.iter().filter(|(_, r)| r.running()) {
            let f = decode_mutf7(&r.folder);
            parts.push(if r.total > 0 { format!("Mail {}: {f} {}/{}", name(id), r.done, r.total) } else { format!("Mail {}: {}", name(id), if f.is_empty() { "connecting…".to_string() } else { f }) });
        }
        if parts.is_empty() {
            return None;
        }
        Some(parts.join(" · "))
    }

    // ------------------------------------------------------------ data from the index

    /// Read folders and counts again, and the list's size; the page cache starts over.
    pub fn refresh(&mut self) {
        self.dirty = false;
        self.last_refresh = Some(Instant::now());
        let Some(r) = self.reader.as_ref() else { return };
        self.queries += 1;
        if let Ok(f) = r.folders(None) {
            self.folders = f;
        }
        if let Ok(c) = r.folder_counts() {
            self.counts = c;
        }
        if let Some(folder) = self.folder {
            // keep the cursor on the same message when mail arrives above it
            let anchor = self.cursor.and_then(|c| self.row(c)).map(|m| (m.date, m.uid));
            self.total = r.count_folder(folder).unwrap_or(0).max(0) as usize;
            self.pages.clear();
            if let (Some((date, uid)), None) = (anchor, &self.results) {
                if let Ok(p) = r.position_in_list(folder, date, uid) {
                    self.cursor = Some(p.max(0) as usize);
                }
            }
        }
        if self.results.is_some() && !self.query.trim().is_empty() {
            self.run_search();
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn select_first_folder(&mut self, cfg: &Config) {
        let first = cfg.mail_accounts.first().and_then(|a| self.account_folders(&a.id).into_iter().next());
        if let Some(f) = first {
            self.folder = Some(f.id);
            self.total = self.reader.as_ref().and_then(|r| r.count_folder(f.id).ok()).unwrap_or(0).max(0) as usize;
            self.pages.clear();
        }
    }

    /// The folders of an account in the order they are shown: Inbox first, special folders,
    /// the rest by name, Junk and Trash last; sub-folders right after their parent.
    pub fn account_folders(&self, account: &str) -> Vec<FolderRow> {
        let mut v: Vec<FolderRow> = self.folders.iter().filter(|f| f.account == account).cloned().collect();
        let rank = |f: &FolderRow| -> u8 {
            let su = f.special_use.as_deref().unwrap_or("").to_ascii_lowercase();
            let shown = decode_mutf7(&f.name);
            if shown.eq_ignore_ascii_case("inbox") {
                0
            } else if su.contains("\\drafts") {
                1
            } else if su.contains("\\sent") {
                2
            } else if su.contains("\\archive") {
                3
            } else {
                match imap_sync::kind_of(f) {
                    FolderKind::Junk => 6,
                    FolderKind::Trash => 7,
                    FolderKind::All => 5,
                    FolderKind::Normal => 4,
                }
            }
        };
        let top = |f: &FolderRow| -> String {
            let shown = decode_mutf7(&f.name);
            let d = f.delimiter.clone().unwrap_or_else(|| "/".into());
            shown.split(d.as_str()).next().unwrap_or("").to_lowercase()
        };
        let ranks: HashMap<String, u8> = v.iter().filter(|f| folder_depth(f) == 0).map(|f| (top(f), rank(f))).collect();
        v.sort_by_key(|f| (ranks.get(&top(f)).copied().unwrap_or(4), decode_mutf7(&f.name).to_lowercase()));
        v
    }

    /// Where a folder sits in its account's tree: how deep, and what it is called there. A
    /// folder whose parent is not a folder of its own (the server lists `Projects/Hall B`
    /// without `Projects`) shows its whole path under the nearest folder that exists.
    pub fn tree_entry(&self, f: &FolderRow) -> (usize, String) {
        let shown = decode_mutf7(&f.name);
        let d = f.delimiter.clone().unwrap_or_else(|| "/".into());
        let parts: Vec<&str> = shown.split(d.as_str()).collect();
        for cut in (1..parts.len()).rev() {
            let prefix = parts[..cut].join(&d);
            if self.folders.iter().any(|o| o.account == f.account && decode_mutf7(&o.name) == prefix) {
                return (cut, parts[cut..].join(&d));
            }
        }
        (0, shown)
    }

    pub fn folder_row(&self, id: i64) -> Option<&FolderRow> {
        self.folders.iter().find(|f| f.id == id)
    }

    // ------------------------------------------------------------ the list

    /// A message of the shown list (the search results or the folder's pages read so far).
    pub fn row(&self, i: usize) -> Option<&MessageSummary> {
        match &self.results {
            Some(r) => r.get(i),
            None => self.pages.get(&(i / PAGE)).and_then(|p| p.get(i % PAGE)),
        }
    }

    pub fn list_len(&self) -> usize {
        self.results.as_ref().map_or(self.total, |r| r.len())
    }

    /// Read the pages that hold rows `range` (a steady frame needs none).
    pub fn ensure_rows(&mut self, range: std::ops::Range<usize>) {
        if self.results.is_some() || range.is_empty() {
            return;
        }
        let (Some(folder), Some(r)) = (self.folder, self.reader.as_ref()) else { return };
        for page in range.start / PAGE..=(range.end - 1) / PAGE {
            if !self.pages.contains_key(&page) {
                self.queries += 1;
                let rows = r.list(folder, PAGE, page * PAGE).unwrap_or_default();
                self.pages.insert(page, rows);
            }
        }
    }

    /// Show the folder `id` (MAIL-056: a folder that is not synced is listed on demand).
    pub fn select_folder(&mut self, cfg: &Config, id: i64) {
        self.clear_search();
        self.folder = Some(id);
        self.cursor = None;
        self.marked.clear();
        self.pages.clear();
        self.total = self.reader.as_ref().and_then(|r| r.count_folder(id).ok()).unwrap_or(0).max(0) as usize;
        self.queries += 1;
        self.list_on_demand(cfg, id, false);
    }

    /// Ctrl+J / Ctrl+K: the next or previous folder of the account of the open folder.
    pub fn step_folder(&mut self, cfg: &Config, delta: isize) {
        let Some(cur) = self.folder.and_then(|id| self.folder_row(id)).cloned() else { return };
        let list = self.account_folders(&cur.account);
        if let Some(i) = list.iter().position(|f| f.id == cur.id) {
            let n = (i as isize + delta).clamp(0, list.len() as isize - 1) as usize;
            if list[n].id != cur.id {
                self.select_folder(cfg, list[n].id);
            }
        }
    }

    /// Fetch the headers of a folder the sync leaves alone (Trash, Junk, All Mail unless it is
    /// ticked), once per run unless `force`.
    pub fn list_on_demand(&mut self, cfg: &Config, id: i64, force: bool) {
        let Some(row) = self.folder_row(id).cloned() else { return };
        let kind = imap_sync::kind_of(&row);
        let on_demand = matches!(kind, FolderKind::Trash | FolderKind::Junk) || (kind == FolderKind::All && !row.sync_enabled);
        if !on_demand || matches!(self.listings.get(&id), Some(ListingState::Loading)) || (!force && self.listings.contains_key(&id)) {
            return;
        }
        let (Some(store), Some(account)) = (self.ready_store(), cfg.mail_accounts.iter().find(|a| a.id == row.account).cloned()) else { return };
        self.listings.insert(id, ListingState::Loading);
        let connector = self.lazy_connector(&account, None);
        let opts = self.opts.clone();
        self.spawn(move || imap_sync::fetch_folder_listing(&store, &connector, &row.account, &row.name, LISTING_LIMIT, &opts), move |r| Event::Listing { folder: id, result: flatten(r) });
    }

    fn lazy_connector(&self, account: &MailAccount, typed: Option<String>) -> LazyConnector {
        let factory = self.factory.clone();
        let acct = account.clone();
        let session = typed.or_else(|| self.session_pw.get(&account.id).cloned());
        LazyConnector::new(move || factory(&acct, session))
    }

    pub fn move_cursor(&mut self, delta: isize) {
        let n = self.list_len();
        if n == 0 {
            return;
        }
        let at = match self.cursor {
            Some(c) => (c as isize + delta).clamp(0, n as isize - 1) as usize,
            None if delta < 0 => n - 1,
            None => 0,
        };
        self.cursor = Some(at);
        self.scroll_to_cursor = true;
    }

    /// The message under the cursor (its page is read if need be).
    pub fn cursor_message(&mut self) -> Option<MessageSummary> {
        let c = self.cursor?;
        self.ensure_rows(c..c + 1);
        self.row(c).cloned()
    }

    pub fn toggle_mark(&mut self, m: &MessageSummary) {
        if self.marked.remove(&m.id).is_none() {
            self.marked.insert(m.id, m.clone());
        }
    }

    // ------------------------------------------------------------ the open message

    /// Show a message in the reading pane; the reading and parsing happen in a job (MAIL-053).
    pub fn open_message(&mut self, msg: MessageSummary) {
        let body = if msg.state == STATE_HEADER {
            Body::NeedsDownload
        } else if msg.state == STATE_TOO_LARGE {
            Body::TooLarge
        } else if msg.missing_file {
            Body::Failed("The file of this message is missing; the next sync fetches it again.".into())
        } else {
            Body::Loading
        };
        let id = msg.id;
        let loading = matches!(body, Body::Loading);
        self.open = Some(OpenMail { msg: msg.clone(), body });
        if loading {
            if let Some(store) = self.ready_store() {
                self.spawn(
                    move || {
                        let mail = store.read_mail(&msg).map_err(|e| e.0)?;
                        let (shown, cut) = cut_text(&mail.text);
                        Ok::<_, String>(Arc::new(Loaded { mail, shown, cut }))
                    },
                    move |r| Event::Message { id, result: flatten(r) },
                );
            }
        }
    }

    pub fn close_message(&mut self) {
        self.open = None;
    }

    /// "Load this message": fetch the whole message from the server (MAIL-055).
    pub fn load_message(&mut self, cfg: &Config, msg: &MessageSummary) {
        let key = (msg.folder_id, msg.uid);
        if self.fetching.contains(&key) {
            return;
        }
        let (Some(store), Some(account)) = (self.ready_store(), cfg.mail_accounts.iter().find(|a| a.id == msg.account).cloned()) else { return };
        self.fetching.insert(key);
        if let Some(o) = self.open.as_mut().filter(|o| o.msg.id == msg.id) {
            o.body = Body::Fetching;
        }
        let connector = self.lazy_connector(&account, None);
        let opts = self.opts.clone();
        let m = msg.clone();
        let retry = Retry::Message { folder: msg.folder_id, uid: msg.uid };
        self.spawn(move || imap_sync::fetch_message(&store, &connector, &m.account, &m.folder, m.uid, &opts), move |r| Event::Fetched { key, retry, result: flatten(r).map(Box::new) });
    }

    // ------------------------------------------------------------ search

    /// The search text changed: search after a pause (MAIL-060).
    pub fn query_edited(&mut self) {
        self.search_edited = Some(Instant::now());
        if self.query.trim().is_empty() {
            self.clear_search();
        }
    }

    /// Search now (Enter in the field).
    pub fn search_now(&mut self) {
        self.search_edited = None;
        self.run_search();
    }

    fn run_search(&mut self) {
        let q = self.query.trim().to_string();
        if q.is_empty() {
            self.clear_search();
            return;
        }
        let Some(store) = self.ready_store() else { return };
        self.search_gen += 1;
        let gen = self.search_gen;
        self.searching = true;
        let folder = if self.search_all { None } else { self.folder };
        self.spawn(
            move || {
                let r = store.reader().map_err(|e| e.0)?;
                r.search(&q, folder, SEARCH_LIMIT, 0).map_err(|e| e.0)
            },
            move |r| Event::Search { gen, result: flatten(r) },
        );
    }

    pub fn clear_search(&mut self) {
        self.search_gen += 1;
        self.searching = false;
        self.search_edited = None;
        if self.results.take().is_some() {
            self.cursor = None;
        }
        self.query.clear();
        self.search_error = None;
    }

    // ------------------------------------------------------------ sync and test

    pub fn start_sync_all(&mut self, cfg: &Config) {
        for a in &cfg.mail_accounts {
            self.start_sync(cfg, &a.id);
        }
    }

    /// Sync one account as a job; never two at once for the same account (MAIL-058).
    pub fn start_sync(&mut self, cfg: &Config, id: &str) {
        if self.syncs.get(id).is_some_and(|r| r.running()) {
            return;
        }
        let (Some(store), Some(account)) = (self.ready_store(), cfg.mail_accounts.iter().find(|a| a.id == id).cloned()) else { return };
        let cancel = Arc::new(AtomicBool::new(false));
        let display = if account.name.is_empty() { account.host.clone() } else { account.name.clone() };
        let connector: Arc<dyn imap_sync::Connector> = Arc::new(self.lazy_connector(&account, None));
        let rx = imap_sync::start(store, account.id.clone(), display, connector, self.opts.clone(), cancel.clone());
        self.syncs.insert(id.to_string(), SyncRun { rx, cancel, folder: String::new(), done: 0, total: 0, added: 0, errors: vec![], finished: None });
    }

    pub fn cancel_syncs(&mut self) {
        for r in self.syncs.values().filter(|r| r.running()) {
            r.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// The dialog's "Test connection": connect, list and record the folders, fetch nothing.
    pub fn test_connection(&mut self, account: &MailAccount, typed: Option<String>) {
        let Some(store) = self.ready_store() else {
            self.tests.insert(account.id.clone(), TestState::Failed("The local mail copy is still being opened; try again in a moment.".into()));
            return;
        };
        self.tests.insert(account.id.clone(), TestState::Running);
        let connector = self.lazy_connector(account, typed);
        let id = account.id.clone();
        let display = if account.name.is_empty() { account.host.clone() } else { account.name.clone() };
        let id2 = id.clone();
        self.spawn(move || imap_sync::discover_folders(&store, &connector, &id, &display).map(|f| f.len()), move |r| Event::Tested { account: id2, result: flatten(r) });
    }

    /// A folder's checkbox: sync it or not (MAIL-042).
    pub fn set_folder_sync(&mut self, folder: i64, enabled: bool) -> Result<(), String> {
        let store = self.ready_store().ok_or("the local mail copy is not open yet")?;
        store.commit(vec![crate::mail_db::Op::SyncEnabled { folder, enabled }]).map_err(|e| e.0)?;
        self.dirty = true;
        self.refresh();
        Ok(())
    }

    /// Save a typed password: into the keyring if there is one, and meanwhile (or instead) in
    /// memory for this session. Never into the settings file (MAIL-041).
    pub fn save_password(&mut self, account: &str, password: String) {
        self.session_pw.insert(account.to_string(), password.clone());
        let id = account.to_string();
        let id2 = id.clone();
        self.spawn(move || crate::secrets::set_mail_secret(&id, crate::secrets::MailSecret::Password, &password), move |r| Event::Secret { account: id2, stored: r.unwrap_or(false) });
    }

    /// An account was removed: its secrets go, its syncs stop; its mail files stay (MAIL-043).
    pub fn forget_account(&mut self, account: &str) {
        self.session_pw.remove(account);
        if let Some(r) = self.syncs.remove(account) {
            r.cancel.store(true, Ordering::SeqCst);
        }
        if self.folder.and_then(|f| self.folder_row(f)).is_some_and(|f| f.account == account) {
            self.folder = None;
            self.total = 0;
            self.pages.clear();
            self.cursor = None;
            self.open = None;
        }
        let id = account.to_string();
        crate::jobs::run(
            move || {
                crate::secrets::clear_mail_secret(&id, crate::secrets::MailSecret::Password);
                crate::secrets::clear_mail_secret(&id, crate::secrets::MailSecret::RefreshToken);
            },
            |_| {},
        );
    }

    /// Ask for the password of an account (once at a time).
    pub fn ask_password(&mut self, account: &str, retry: Retry) {
        if self.prompt.is_none() && !account.is_empty() {
            self.prompt = Some(PasswordPrompt { account: account.to_string(), text: String::new(), retry });
        }
    }

    /// The typed password is kept for this session and the failed work is tried again.
    pub fn give_password(&mut self, cfg: &Config) {
        let Some(p) = self.prompt.take() else { return };
        if p.text.is_empty() {
            return;
        }
        self.session_pw.insert(p.account.clone(), p.text);
        match p.retry {
            Retry::Sync => self.start_sync(cfg, &p.account),
            Retry::Listing(f) => self.list_on_demand(cfg, f, true),
            Retry::Message { folder, uid } => {
                if let Some(msg) = self.reader.as_ref().and_then(|r| r.message(folder, uid).ok().flatten()) {
                    self.load_message(cfg, &msg);
                }
            }
        }
    }
}

fn key_account(folders: &[FolderRow], folder: i64) -> String {
    folders.iter().find(|f| f.id == folder).map(|f| f.account.clone()).unwrap_or_default()
}

/// How deep a folder is in its account's tree (`INBOX/Work/2026` is 2).
pub fn folder_depth(f: &FolderRow) -> usize {
    let shown = decode_mutf7(&f.name);
    let d = f.delimiter.clone().unwrap_or_else(|| "/".into());
    shown.matches(d.as_str()).count()
}

/// The folder's name as shown: the last part of its path, readable (not modified UTF-7).
pub fn folder_label(f: &FolderRow) -> String {
    let shown = decode_mutf7(&f.name);
    let d = f.delimiter.clone().unwrap_or_else(|| "/".into());
    shown.rsplit(d.as_str()).next().unwrap_or(&shown).to_string()
}

/// The last part of a folder name as the server sent it, readable.
pub fn folder_label_of(name: &str) -> String {
    let shown = decode_mutf7(name);
    shown.rsplit(['/', '.']).next().unwrap_or(&shown).to_string()
}

/// A text cut at `TEXT_CAP` characters (at a character boundary), and whether it was cut.
pub fn cut_text(t: &str) -> (String, bool) {
    match t.char_indices().nth(TEXT_CAP) {
        Some((i, _)) => (t[..i].to_string(), true),
        None => (t.to_string(), false),
    }
}

/// "14:05" today, "13 Nov" this year, "2024-03-02" before (local time).
pub fn list_date(ts: Option<i64>, now: i64) -> String {
    use crate::cal_time as t;
    let Some(ts) = ts else { return String::new() };
    let (day, secs) = t::utc_to_local(ts);
    let (today, _) = t::utc_to_local(now);
    let (y, m, d) = t::civil_from_days(day);
    let (ty, _, _) = t::civil_from_days(today);
    if day == today {
        t::hhmm(secs)
    } else if y == ty {
        const MON: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
        format!("{d} {}", MON[(m as usize - 1) % 12])
    } else {
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// "2026-11-13 10:22" (local time).
pub fn full_date(ts: i64) -> String {
    use crate::cal_time as t;
    let (day, secs) = t::utc_to_local(ts);
    let (y, m, d) = t::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02} {}", t::hhmm(secs))
}

/// Bytes as "12 KB" / "3.4 MB".
pub fn size_text(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", n.div_ceil(1024).max(1))
    }
}
