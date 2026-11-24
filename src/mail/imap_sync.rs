//! Read-only sync of an IMAP account into the local mail store (ADR-0007 §3, MAIL-030 to MAIL-039).
//!
//! What it does: list the folders; per folder `EXAMINE`, then fetch what the index lacks (first
//! sync: the last 3 years, newest first, in batches, resumable because "what the index lacks"
//! is recomputed each time; later: only UIDs from the stored `UIDNEXT` on), then compare flags
//! and deletions with the server. A message deleted on the server only gets `gone_on_server`;
//! its file stays. Trash and Junk are never synced here (`fetch_folder_listing` and
//! `fetch_message` are for browsing them), and Gmail's `\All` is off by default.
//!
//! Work is split like this: up to 4 connections each take whole folders from a shared queue;
//! the worker that fetched a message parses it and writes its `.eml` file (durably) before the
//! batch goes to the store's single writer thread, which commits it in one transaction.
//!
//! `UIDNEXT` of a folder is stored only when its sync completed without errors, so an
//! interrupted or partly failed sync starts again from what is already in the index.

use crate::calendar::time;
use crate::config::{MailAccount, MailAuth};
use crate::mail::db::{FolderRow, MailError, MessageSummary, Op};
use crate::mail::imap_client::{decode_mutf7, Authenticator, Body, Client, ConnectSettings, Fetched, FolderEntry, ImapError, Limits, Part, PlainAuth, SearchKey, Selected};
use crate::mail::store::MailStore;
use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

/// How a sync behaves; the defaults are what the account settings use.
#[derive(Clone, Debug)]
pub struct SyncOptions {
    /// Only mail of this many years back is fetched on the first sync.
    pub window_years: i32,
    /// Connections per account working on different folders.
    pub parallel: usize,
    /// Messages per `UID FETCH` and per index transaction.
    pub batch_size: usize,
    pub limits: Limits,
    /// "Now" in UTC seconds; None means the clock (tests set it).
    pub now: Option<i64>,
}

impl Default for SyncOptions {
    fn default() -> Self {
        SyncOptions { window_years: 3, parallel: 4, batch_size: 25, limits: Limits::default(), now: None }
    }
}

impl SyncOptions {
    fn now(&self) -> i64 {
        self.now.unwrap_or_else(time::now)
    }
}

/// What the window shows while a sync runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    FolderStarted {
        folder: String,
    },
    /// `done` of `total` messages of the folder are stored.
    Folder {
        folder: String,
        done: usize,
        total: usize,
    },
    FolderDone {
        folder: String,
        added: usize,
    },
    /// Something failed; the sync goes on with the other folders.
    Error {
        folder: Option<String>,
        message: String,
    },
    /// Always the last event.
    Finished(SyncSummary),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub folders_synced: usize,
    pub folders_failed: usize,
    pub messages_added: usize,
    pub cancelled: bool,
    pub errors: Vec<String>,
}

// ---------------------------------------------------------------- connecting

/// Makes connections for an account (so tests and the OAuth2 phase can plug in their own).
pub trait Connector: Send + Sync {
    fn connect(&self) -> Result<Client, ImapError>;
}

/// Gives a fresh authenticator for each connection (an OAuth2 one fetches a current access token).
pub trait AuthFactory: Send + Sync {
    fn authenticator(&self) -> Result<Box<dyn Authenticator>, String>;
    /// The server refused the sign-in. An OAuth2 factory forgets its access token and says yes:
    /// the connection is then tried once more with a new one (MAIL-083). Passwords say no.
    fn token_rejected(&self) -> bool {
        false
    }
}

/// Connect with an authenticator from `auth`; if the server refuses the sign-in and the factory
/// can get a new token, once more. `connect` does the actual connecting.
pub fn connect_with_retry(auth: &dyn AuthFactory, mut connect: impl FnMut(&mut dyn Authenticator) -> Result<Client, ImapError>) -> Result<Client, ImapError> {
    let mut a = auth.authenticator().map_err(ImapError::Auth)?;
    match connect(a.as_mut()) {
        Err(ImapError::Auth(_)) if auth.token_rejected() => {
            let mut a = auth.authenticator().map_err(ImapError::Auth)?;
            // a fresh token was refused too: the sign-in itself is no good any more
            connect(a.as_mut()).map_err(|e| match e {
                ImapError::Auth(t) => ImapError::Auth(format!("{t} The server refused a fresh token as well. {}", crate::mail::oauth::SIGN_IN_AGAIN)),
                other => other,
            })
        }
        r => r,
    }
}

/// User name and password.
pub struct PasswordAuth {
    pub user: String,
    pub password: crate::secret::Secret,
}

impl AuthFactory for PasswordAuth {
    fn authenticator(&self) -> Result<Box<dyn Authenticator>, String> {
        Ok(Box::new(PlainAuth::new(&self.user, self.password.expose())))
    }
}

/// The real connector: TLS with the operating system's certificate check.
pub struct ImapConnector {
    pub settings: ConnectSettings,
    pub auth: Arc<dyn AuthFactory>,
}

impl Connector for ImapConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        connect_with_retry(self.auth.as_ref(), |a| Client::connect(&self.settings, a))
    }
}

/// A connector for a configured account; the password comes from the keyring.
/// `typed_secret` is a client secret typed but not saved yet
/// (OAuth2 accounts only).
pub fn connector_for_account(account: &MailAccount, limits: Limits, typed_secret: Option<crate::secret::Secret>) -> Result<ImapConnector, String> {
    let settings = ConnectSettings { host: account.host.clone(), port: account.port, security: account.security, limits };
    let auth = auth_factory(account, typed_secret, &account.host, account.port)?;
    Ok(ImapConnector { settings, auth })
}

/// How an account signs in, for the server at `host:port` (IMAP, or SMTP for sending: the same password,
/// or the same OAuth2 sign-in, whichever server it is used with).
pub fn auth_factory(account: &MailAccount, typed_secret: Option<crate::secret::Secret>, host: &str, port: u16) -> Result<Arc<dyn AuthFactory>, String> {
    Ok(match account.auth {
        MailAuth::Password => {
            let password = crate::secrets::mail_secret(&account.id, crate::secrets::MailSecret::Password).ok_or("no password for this account in the keyring")?;
            Arc::new(PasswordAuth { user: account.user.clone(), password })
        }
        MailAuth::OAuth2 => {
            let secret = typed_secret.filter(|s| !s.is_empty()).or_else(|| crate::secrets::mail_secret(&account.id, crate::secrets::MailSecret::ClientSecret));
            let cfg = crate::mail::oauth::OAuthConfig::for_account(account, secret).map_err(|e| e.to_string())?;
            let source = crate::mail::oauth::shared_source(&account.id, cfg);
            Arc::new(crate::mail::oauth::OAuthFactory { user: account.user.clone(), host: host.to_string(), port, source })
        }
    })
}

// ---------------------------------------------------------------- which folders

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FolderKind {
    Normal,
    Trash,
    Junk,
    /// Gmail's "All Mail": every message again.
    All,
}

/// Special-use attribute first; only a folder without one is recognised by its usual name.
pub fn classify(attributes: &[String], name: &str) -> FolderKind {
    const KNOWN: [&str; 6] = ["\\sent", "\\drafts", "\\archive", "\\flagged", "\\important", "\\inbox"];
    let mut special = false;
    for a in attributes {
        match a.to_ascii_lowercase().as_str() {
            "\\trash" => return FolderKind::Trash,
            "\\junk" => return FolderKind::Junk,
            "\\all" => return FolderKind::All,
            k if KNOWN.contains(&k) => special = true,
            _ => {}
        }
    }
    if special {
        return FolderKind::Normal;
    }
    let shown = decode_mutf7(name);
    let last = shown.rsplit(['/', '.']).next().unwrap_or(&shown).trim().to_lowercase();
    match last.as_str() {
        "trash" | "deleted items" | "deleted messages" | "bin" => FolderKind::Trash,
        "junk" | "spam" | "junk e-mail" | "junk email" | "bulk mail" => FolderKind::Junk,
        _ => FolderKind::Normal,
    }
}

pub fn kind_of(f: &FolderRow) -> FolderKind {
    let attrs: Vec<String> = f.special_use.iter().flat_map(|s| s.split(' ').map(String::from)).collect();
    classify(&attrs, &f.name)
}

/// Whether a folder is synced by default (Trash, Junk and `\All` are not).
fn default_sync(kind: FolderKind) -> bool {
    kind == FolderKind::Normal
}

fn special_use_of(e: &FolderEntry) -> Option<String> {
    let v: Vec<&str> = e.attributes.iter().map(String::as_str).filter(|a| matches!(a.to_ascii_lowercase().as_str(), "\\trash" | "\\junk" | "\\all" | "\\sent" | "\\drafts" | "\\archive" | "\\flagged" | "\\important")).collect();
    if v.is_empty() {
        None
    } else {
        Some(v.join(" "))
    }
}

/// The first day of the window: `years` before `now`, as (year, month, day).
pub fn window_start(now: i64, years: i32) -> (i32, u32, u32) {
    let (y, m, d) = time::civil_from_days(now.div_euclid(86_400));
    let y = y - years;
    (y, m, d.min(time::days_in_month(y, m)))
}

fn err_text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- the sync

/// Accounts being synced right now (a full sync or a quick check of a watched folder, MAIL-232):
/// never two at once for the same account of the same store.
static ACTIVE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Held while an account is being synced.
pub struct SyncGuard(String);

impl SyncGuard {
    /// None: another sync of this account is going on.
    pub fn try_enter(store: &MailStore, account_id: &str) -> Option<SyncGuard> {
        let key = format!("{}|{account_id}", store.index_path().display());
        let mut active = ACTIVE.lock().unwrap_or_else(|e| e.into_inner());
        if active.contains(&key) {
            return None;
        }
        active.push(key.clone());
        Some(SyncGuard(key))
    }
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        ACTIVE.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| k != &self.0);
    }
}

/// Sync all folders of an account that are switched on. Blocks until done: run it through
/// [`start`] (a background job). Progress goes to `progress`; `cancel` is checked between batches.
pub fn sync_account(store: &MailStore, account_id: &str, display: &str, connector: &dyn Connector, opts: &SyncOptions, progress: &Sender<Progress>, cancel: &AtomicBool) -> SyncSummary {
    let mut summary = SyncSummary::default();
    let _flow = tracing::info_span!("flow.mail_sync", peer = "sync").entered();
    // a quick check of a watched folder (MAIL-232) is short: wait for it rather than run beside it
    let _guard = loop {
        if let Some(g) = SyncGuard::try_enter(store, account_id) {
            break g;
        }
        if cancel.load(Ordering::SeqCst) {
            summary.cancelled = true;
            return summary;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    let first = match tracing::info_span!("flow.mail_sync.connect", peer = "imap").in_scope(|| connector.connect()) {
        Ok(c) => c,
        Err(e) => {
            summary.errors.push(err_text(&e));
            let _ = progress.send(Progress::Error { folder: None, message: err_text(&e) });
            return summary;
        }
    };
    let mut first = first;
    let queue = match folders_to_sync(store, account_id, display, &mut first) {
        Ok(q) => q,
        Err(e) => {
            summary.errors.push(e.clone());
            let _ = progress.send(Progress::Error { folder: None, message: e });
            return summary;
        }
    };
    let workers = opts.parallel.clamp(1, 8).min(queue.len().max(1));
    let queue = Mutex::new(VecDeque::from(queue));
    let first = Mutex::new(Some(first));
    let result = Mutex::new(SyncSummary::default());
    // the workers record under this sync's span, in this sync's dispatcher (DEV-026)
    let (dispatch, flow) = (tracing::dispatcher::get_default(|d| d.clone()), tracing::Span::current());
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| tracing::dispatcher::with_default(&dispatch, || flow.in_scope(|| worker(store, connector, opts, &WorkerShared { queue: &queue, first: &first, result: &result }, progress, cancel))));
        }
    });
    let mut done = result.into_inner().unwrap_or_else(|e| e.into_inner());
    done.cancelled = cancel.load(Ordering::SeqCst);
    done.errors.extend(summary.errors);
    if done.errors.is_empty() && !done.cancelled {
        // a backup only after a sync that went well (MAIL-008)
        let _backup = tracing::info_span!("flow.mail_sync.backup", peer = "store").entered();
        if let Err(e) = store.backup_if_due() {
            done.errors.push(format!("backup of the mail index failed: {e}"));
        }
    }
    done
}

/// Record the folder list of the account in the index (a new folder starts with the default
/// choice, an old one keeps the person's) and return every selectable folder with its kind.
fn record_folders(store: &MailStore, account_id: &str, display: &str, client: &mut Client) -> Result<Vec<(FolderRow, FolderKind)>, String> {
    let mut out = Vec::new();
    let listed = {
        let _list = tracing::info_span!("flow.mail_sync.list_folders", peer = "imap").entered();
        client.list_folders().map_err(err_text)?
    };
    for e in listed {
        if e.has_attr("\\Noselect") || e.has_attr("\\NonExistent") {
            continue;
        }
        let kind = classify(&e.attributes, &e.name);
        let _record = tracing::info_span!("flow.mail_sync.record_folder", peer = "store").entered();
        let row = store.ensure_folder(account_id, display, &e.name, e.delimiter.as_deref(), special_use_of(&e).as_deref(), default_sync(kind)).map_err(err_text)?;
        out.push((row, kind));
    }
    Ok(out)
}

/// Record the folder list and return the folders to sync now. Trash and Junk never sync by
/// themselves, whatever the checkbox says.
fn folders_to_sync(store: &MailStore, account_id: &str, display: &str, client: &mut Client) -> Result<Vec<FolderRow>, String> {
    Ok(record_folders(store, account_id, display, client)?.into_iter().filter(|(row, kind)| row.sync_enabled && matches!(kind, FolderKind::Normal | FolderKind::All)).map(|(row, _)| row).collect())
}

/// Connect, list the account's folders and record them, without fetching any mail (the account
/// settings' "Test connection", MAIL-042). Returns all folders of the account.
pub fn discover_folders(store: &MailStore, connector: &dyn Connector, account_id: &str, display: &str) -> Result<Vec<FolderRow>, String> {
    let mut client = connector.connect().map_err(err_text)?;
    let rows = record_folders(store, account_id, display, &mut client);
    client.logout();
    Ok(rows?.into_iter().map(|(r, _)| r).collect())
}

/// What the window shows when an account has no password to sign in with. The window looks for
/// this text to ask for the password (MAIL-059).
pub const NO_PASSWORD: &str = "no password for this account";

/// Builds an account's connector, once, in the background job.
type MakeConnector = Box<dyn FnOnce() -> Result<Arc<dyn Connector>, String> + Send>;

/// Decides how to connect only when the first connection is made, so that slow things (the
/// keyring) never run on the window's thread: the account's connector is built by `make` inside
/// the background job, once.
pub struct LazyConnector {
    make: Mutex<Option<MakeConnector>>,
    made: Mutex<Option<Result<Arc<dyn Connector>, String>>>,
}

impl LazyConnector {
    pub fn new(make: impl FnOnce() -> Result<Arc<dyn Connector>, String> + Send + 'static) -> LazyConnector {
        LazyConnector { make: Mutex::new(Some(Box::new(make))), made: Mutex::new(None) }
    }
}

impl Connector for LazyConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        let inner = {
            let mut made = self.made.lock().unwrap_or_else(|e| e.into_inner());
            if made.is_none() {
                let make = self.make.lock().unwrap_or_else(|e| e.into_inner()).take();
                *made = Some(match make {
                    Some(f) => f(),
                    None => Err("no way to connect".into()),
                });
            }
            made.clone().unwrap_or_else(|| Err("no way to connect".into()))
        };
        inner.map_err(ImapError::Auth)?.connect()
    }
}

/// Builds the connector of an account; the second argument is a password typed for this session
/// (used when the keyring has none). Tests plug in a connector for the pretend server.
pub type ConnectorFactory = Arc<dyn Fn(&MailAccount, Option<crate::secret::Secret>) -> Result<Arc<dyn Connector>, String> + Send + Sync>;

/// The real factory: the keyring's password (or the session's) and the operating system's
/// certificate check.
pub fn real_factory() -> ConnectorFactory {
    Arc::new(|account, session_password| {
        if let (Some(pw), crate::config::MailAuth::Password) = (session_password.clone(), account.auth) {
            // a password typed for this session wins over the keyring's: it is the newest
            let settings = ConnectSettings { host: account.host.clone(), port: account.port, security: account.security, limits: Limits::default() };
            return Ok(Arc::new(ImapConnector { settings, auth: Arc::new(PasswordAuth { user: account.user.clone(), password: pw }) }) as Arc<dyn Connector>);
        }
        match connector_for_account(account, Limits::default(), session_password) {
            Ok(c) => Ok(Arc::new(c) as Arc<dyn Connector>),
            Err(e) if e.contains("no password") => Err(NO_PASSWORD.into()),
            Err(e) => Err(e),
        }
    })
}

/// What the parallel workers of one sync share: the folders still to do, the connection that was
/// already open, and the summary they all add to.
struct WorkerShared<'a> {
    queue: &'a Mutex<VecDeque<FolderRow>>,
    first: &'a Mutex<Option<Client>>,
    result: &'a Mutex<SyncSummary>,
}

fn worker(store: &MailStore, connector: &dyn Connector, opts: &SyncOptions, shared: &WorkerShared, progress: &Sender<Progress>, cancel: &AtomicBool) {
    let WorkerShared { queue, first, result } = *shared;
    let mut client = first.lock().unwrap_or_else(|e| e.into_inner()).take();
    loop {
        if cancel.load(Ordering::SeqCst) {
            return;
        }
        let Some(folder) = queue.lock().unwrap_or_else(|e| e.into_inner()).pop_front() else { return };
        let shown = decode_mutf7(&folder.name);
        let c = match client.take() {
            Some(c) => c,
            None => match connector.connect() {
                Ok(c) => c,
                Err(e) => {
                    fail(result, progress, &shown, err_text(e));
                    continue;
                }
            },
        };
        let _ = progress.send(Progress::FolderStarted { folder: shown.clone() });
        let mut c = c;
        let outcome = tracing::info_span!("flow.mail_sync.folder", peer = "sync").in_scope(|| crate::jobs::catch(|| sync_folder(store, &mut c, &folder, opts, progress, cancel)).unwrap_or_else(Err));
        match outcome {
            Ok((added, _)) => {
                client = Some(c); // the connection is still good: next folder
                let mut r = result.lock().unwrap_or_else(|e| e.into_inner());
                r.folders_synced += 1;
                r.messages_added += added;
                let _ = progress.send(Progress::FolderDone { folder: shown, added });
            }
            Err(e) => fail(result, progress, &shown, e), // the connection may be out of step: drop it
        }
    }
}

fn fail(result: &Mutex<SyncSummary>, progress: &Sender<Progress>, folder: &str, message: String) {
    let mut r = result.lock().unwrap_or_else(|e| e.into_inner());
    r.folders_failed += 1;
    r.errors.push(format!("{folder}: {message}"));
    let _ = progress.send(Progress::Error { folder: Some(folder.to_string()), message });
}

/// Make the folder row say what the server says about UIDVALIDITY; a change starts it afresh
/// (old files and rows stay, under the old number).
fn adopt_validity(store: &MailStore, row: &FolderRow, sel: &Selected, now: i64) -> Result<FolderRow, String> {
    let mut row = row.clone();
    if sel.uidvalidity == crate::mail::db::LOCAL_UIDVALIDITY {
        // RFC 3501 does not allow 0; 0 is where Shale keeps its own copies of sent mail apart from the server's
        return Err("the server reports UIDVALIDITY 0, which is not allowed".into());
    }
    if row.uidvalidity != Some(sel.uidvalidity) {
        row.uidvalidity = Some(sel.uidvalidity);
        row.uidnext = None;
        row.highestmodseq = None;
        store.commit(vec![Op::FolderState { folder: row.id, uidvalidity: row.uidvalidity, uidnext: None, highestmodseq: None, last_sync: now }]).map_err(err_text)?;
    }
    Ok(row)
}

/// Fetch whole messages by UID, write their files, commit in batches. Returns how many were stored.
fn fetch_full(store: &MailStore, client: &mut Client, row: &FolderRow, uv: u32, uids: &[u32], opts: &SyncOptions, mut after_batch: impl FnMut(usize) -> bool) -> Result<usize, String> {
    let mut stored = 0;
    for chunk in uids.chunks(opts.batch_size.max(1)) {
        let mut ops: Vec<Op> = Vec::new();
        let mut failure: Option<String> = None;
        client
            .uid_fetch_messages(chunk, Part::Full, |f: Fetched| {
                let flags = f.flags.clone().unwrap_or_default();
                match &f.body {
                    Some(Body::Data(d)) if d.len() as u64 <= opts.limits.max_message => match store.save_message(row, uv, f.uid, d, &flags, f.internal_date) {
                        Ok(m) => ops.push(Op::message(m)),
                        Err(e) => {
                            failure = Some(format!("could not save message {}: {e}", f.uid));
                            return Err(ImapError::Protocol("stopped".into()));
                        }
                    },
                    Some(Body::Data(d)) => ops.push(Op::message(store.too_large(row, uv, f.uid, &flags, f.internal_date, d.len() as u64))),
                    Some(Body::TooLarge(n)) => ops.push(Op::message(store.too_large(row, uv, f.uid, &flags, f.internal_date, *n))),
                    None => {} // the message vanished between the search and the fetch
                }
                Ok(())
            })
            .map_err(|e| failure.clone().unwrap_or_else(|| err_text(e)))?;
        stored += ops.len();
        {
            let _span = tracing::info_span!("flow.mail_sync.store_messages", peer = "store").entered();
            store.commit(ops).map_err(err_text)?;
        }
        if !after_batch(stored) {
            break;
        }
    }
    Ok(stored)
}

/// Sync one folder (see the module comment). Returns the number of new messages and the number
/// of messages whose flags or presence on the server changed.
fn sync_folder(store: &MailStore, client: &mut Client, folder: &FolderRow, opts: &SyncOptions, progress: &Sender<Progress>, cancel: &AtomicBool) -> Result<(usize, usize), String> {
    let shown = decode_mutf7(&folder.name);
    let now = opts.now();
    let sel = {
        let _span = tracing::info_span!("flow.mail_sync.select_folder", peer = "imap").entered();
        client.examine(&folder.name).map_err(err_text)?
    };
    let row = {
        let _span = tracing::info_span!("flow.mail_sync.adopt_validity", peer = "store").entered();
        adopt_validity(store, folder, &sel, now)?
    };
    let uv = sel.uidvalidity;
    let reader = store.reader().map_err(err_text)?;
    let had_rows = !reader.flags_of(row.id, uv).map_err(err_text)?.is_empty();

    // what to fetch: the window on a first sync, everything from UIDNEXT on afterwards
    let search = tracing::info_span!("flow.mail_sync.search_new", peer = "imap").entered();
    let candidates = match row.uidnext {
        Some(next) => client.uid_search(SearchKey::UidFrom(next)).map_err(err_text)?.into_iter().filter(|&u| u >= next).collect::<Vec<_>>(),
        None => {
            let (y, m, d) = window_start(now, opts.window_years);
            client.uid_search(SearchKey::Since(y, m, d)).map_err(err_text)?
        }
    };
    drop(search);
    let present = reader.present_uids(row.id, uv).map_err(err_text)?;
    let mut wanted: Vec<u32> = candidates.into_iter().filter(|u| !present.contains(u)).collect();
    wanted.sort_unstable_by(|a, b| b.cmp(a)); // newest first
    let total = wanted.len();
    let mut error: Option<String> = None;
    let fetch = tracing::info_span!("flow.mail_sync.fetch", peer = "imap").entered();
    let stored = fetch_full(store, client, &row, uv, &wanted, opts, |done| {
        let _ = progress.send(Progress::Folder { folder: shown.clone(), done, total });
        !cancel.load(Ordering::SeqCst)
    })
    .unwrap_or_else(|e| {
        error = Some(e);
        0
    });
    drop(fetch);
    if let Some(e) = error {
        return Err(e);
    }
    let cancelled = cancel.load(Ordering::SeqCst);
    let complete = stored == total && !cancelled;

    let mut changed = 0;
    if complete && had_rows {
        let _compare = tracing::info_span!("flow.mail_sync.compare_with_server", peer = "imap").entered();
        changed = compare_with_server(store, client, &row, &sel, opts)?;
    }
    if complete {
        crate::mail::ops::drop_local_copies_seen_on_server(store, row.id, uv)?;
    }
    let keep = row.uidvalidity == Some(uv);
    let _state = tracing::info_span!("flow.mail_sync.store_folder_state", peer = "store").entered();
    store
        .commit(vec![Op::FolderState {
            folder: row.id,
            uidvalidity: Some(uv),
            uidnext: if complete {
                Some(sel.uidnext)
            } else if keep {
                row.uidnext
            } else {
                None
            },
            highestmodseq: if complete { sel.highestmodseq } else { row.highestmodseq },
            last_sync: now,
        }])
        .map_err(err_text)?;
    Ok((stored, changed))
}

/// Flags and deletions: what changed on the server since last time. Files are never touched.
/// Returns how many messages changed (flags, or gone from / back on the server).
fn compare_with_server(store: &MailStore, client: &mut Client, row: &FolderRow, sel: &Selected, opts: &SyncOptions) -> Result<usize, String> {
    let uv = sel.uidvalidity;
    let reader = store.reader().map_err(err_text)?;
    let known = reader.flags_of(row.id, uv).map_err(err_text)?;
    let on_server: HashSet<u32> = client.uid_search(SearchKey::All).map_err(err_text)?.into_iter().collect();
    // a message with an action of the person that the server has not confirmed yet is left alone:
    // the server wins, unless the person's own change is still on its way (MAIL-108)
    let waiting = reader.pending_uids(row.id, uv).map_err(err_text)?;
    let mut ops = Vec::new();
    for (uid, (_, gone)) in &known {
        if waiting.contains(uid) {
            continue;
        }
        let there = on_server.contains(uid);
        if *gone == there {
            ops.push(Op::Gone { folder: row.id, uidvalidity: uv, uid: *uid, gone: !there });
        }
    }
    // CONDSTORE: only what changed since the stored HIGHESTMODSEQ; otherwise every flag is compared
    let since = if row.highestmodseq.is_some() && sel.highestmodseq.is_some() { row.highestmodseq } else { None };
    client
        .uid_fetch_flags(since, |f| {
            if let (Some(flags), Some((old, _))) = (&f.flags, known.get(&f.uid)) {
                if flags != old && !waiting.contains(&f.uid) {
                    ops.push(Op::Flags { folder: row.id, uidvalidity: uv, uid: f.uid, flags: flags.clone() });
                }
            }
            Ok(())
        })
        .map_err(err_text)?;
    let changes = ops.len();
    store.commit(ops).map_err(err_text)?;
    // files that went missing locally and still exist on the server are fetched again
    let refetch: Vec<u32> = reader.missing_uids(row.id, uv).map_err(err_text)?.into_iter().filter(|u| on_server.contains(u)).collect();
    fetch_full(store, client, row, uv, &refetch, opts, |_| true)?;
    Ok(changes)
}

/// A quick look at one folder of an account that is kept open: new mail, changed flags, mail gone
/// from the server, the same as the sync of that folder does (MAIL-232). `client` is a connection
/// of the account (it may sit in a folder; the folder is examined again). None: another sync of
/// the account is going on, nothing was done. Else the number of new messages and of changed ones.
pub fn check_folder(store: &MailStore, client: &mut Client, account_id: &str, folder_name: &str, opts: &SyncOptions) -> Result<Option<(usize, usize)>, String> {
    let Some(_guard) = SyncGuard::try_enter(store, account_id) else { return Ok(None) };
    let row = folder_row(store, account_id, folder_name)?;
    let (tx, _rx) = channel();
    let never = AtomicBool::new(false);
    crate::jobs::catch(|| sync_folder(store, client, &row, opts, &tx, &never)).unwrap_or_else(Err).map(Some)
}

// ---------------------------------------------------------------- browsing Trash and Junk

fn folder_row(store: &MailStore, account_id: &str, name: &str) -> Result<FolderRow, String> {
    let reader = store.reader().map_err(err_text)?;
    if let Some(r) = crate::mail::db::folder_by_name(reader.conn(), account_id, name).map_err(err_text)? {
        return Ok(r);
    }
    store.ensure_folder(account_id, account_id, name, None, None, false).map_err(err_text)
}

/// The list of a folder that is not synced (Trash, Junk): headers of the newest `limit`
/// messages of the window are fetched and indexed, without bodies. Returns how many were new.
pub fn fetch_folder_listing(store: &MailStore, connector: &dyn Connector, account_id: &str, folder_name: &str, limit: usize, opts: &SyncOptions) -> Result<usize, String> {
    let mut client = connector.connect().map_err(err_text)?;
    let sel = client.examine(folder_name).map_err(err_text)?;
    let row = adopt_validity(store, &folder_row(store, account_id, folder_name)?, &sel, opts.now())?;
    let uv = sel.uidvalidity;
    let (y, m, d) = window_start(opts.now(), opts.window_years);
    let mut uids = client.uid_search(SearchKey::Since(y, m, d)).map_err(err_text)?;
    let known = store.reader().map_err(err_text)?.flags_of(row.id, uv).map_err(err_text)?;
    uids.retain(|u| !known.contains_key(u));
    uids.sort_unstable_by(|a, b| b.cmp(a));
    uids.truncate(limit);
    let mut added = 0;
    for chunk in uids.chunks(opts.batch_size.max(1)) {
        let mut ops = Vec::new();
        client
            .uid_fetch_messages(chunk, Part::Headers, |f| {
                if let Some(Body::Data(h)) = &f.body {
                    ops.push(Op::message(store.header_only(&row, uv, f.uid, h, crate::mail::store::ServerMeta { flags: &f.flags.clone().unwrap_or_default(), internal_date: f.internal_date, size: f.size.unwrap_or(0) })));
                }
                Ok(())
            })
            .map_err(err_text)?;
        added += ops.len();
        store.commit(ops).map_err(err_text)?;
    }
    client.logout();
    Ok(added)
}

/// Download one message (for opening it in a folder that is not synced) and return its row.
pub fn fetch_message(store: &MailStore, connector: &dyn Connector, account_id: &str, folder_name: &str, uid: u32, opts: &SyncOptions) -> Result<MessageSummary, String> {
    let mut client = connector.connect().map_err(err_text)?;
    let sel = client.examine(folder_name).map_err(err_text)?;
    let row = adopt_validity(store, &folder_row(store, account_id, folder_name)?, &sel, opts.now())?;
    fetch_full(store, &mut client, &row, sel.uidvalidity, &[uid], opts, |_| true)?;
    client.logout();
    let found = store.reader().map_err(err_text)?.message(row.id, uid).map_err(err_text)?;
    found.ok_or_else(|| "the server has no such message".to_string())
}

// ---------------------------------------------------------------- as a background job

/// Run a sync as a background job (panics become an error in the summary). The receiver gets
/// the progress events and, last, `Progress::Finished`. The busy task of the sync (JOB-021) is dropped
/// just before that, so a view that has seen the end finds the account idle.
pub fn start(store: Arc<MailStore>, account_id: String, display: String, connector: Arc<dyn Connector>, opts: SyncOptions, cancel: Arc<AtomicBool>, task: crate::busy::Task) -> Receiver<Progress> {
    let (tx, rx) = channel();
    let tx2 = tx.clone();
    crate::jobs::run(
        move || sync_account(&store, &account_id, &display, connector.as_ref(), &opts, &tx, &cancel),
        move |r| {
            drop(task);
            let summary = r.unwrap_or_else(|e| SyncSummary { errors: vec![e], ..Default::default() });
            let _ = tx2.send(Progress::Finished(summary));
        },
    );
    rx
}

impl From<MailError> for String {
    fn from(e: MailError) -> String {
        e.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::MailSecurity;
    use crate::mail::db::{STATE_HEADER, STATE_TOO_LARGE};
    use crate::mail::imap_fake_tests::{message, server_cert, FakeServer};
    use crate::mail::ui::MAIL_SYNC;
    use std::sync::atomic::AtomicBool;

    /// 2026-11-14 12:00 UTC.
    const NOW: i64 = 1_794_657_600;
    const DAY: i64 = 86_400;

    struct TestConnector {
        settings: ConnectSettings,
        user: String,
        password: String,
    }

    impl Connector for TestConnector {
        fn connect(&self) -> Result<Client, ImapError> {
            let mut a = PlainAuth::new(&self.user, &self.password);
            Client::connect_test(&self.settings, server_cert(), &mut a)
        }
    }

    fn connector(server: &FakeServer) -> TestConnector {
        let s = server.state();
        TestConnector { settings: server.settings(), user: s.user.clone(), password: s.password.clone() }
    }

    /// An auth factory that counts what is asked of it.
    struct CountingAuth {
        rejects: bool,
        fail_on_call: Option<usize>,
        asked: std::sync::atomic::AtomicUsize,
        rejected: std::sync::atomic::AtomicUsize,
    }

    impl CountingAuth {
        fn new(rejects: bool, fail_on_call: Option<usize>) -> CountingAuth {
            CountingAuth { rejects, fail_on_call, asked: Default::default(), rejected: Default::default() }
        }
        fn asked(&self) -> usize {
            self.asked.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl AuthFactory for CountingAuth {
        fn authenticator(&self) -> Result<Box<dyn Authenticator>, String> {
            let n = self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if self.fail_on_call == Some(n) {
                return Err(format!("no token (call {n})"));
            }
            Ok(Box::new(PlainAuth::new("ada", "pw")))
        }
        fn token_rejected(&self) -> bool {
            self.rejected.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.rejects
        }
    }

    // spec: MAIL-222
    #[test]
    fn connect_with_retry_asks_again_once_only_for_a_refused_sign_in_of_a_factory_that_can_get_a_new_token() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let auth_err = |t: &str| Err::<Client, _>(ImapError::Auth(t.into()));
        // a password: one try, the refusal as it is
        let (f, n) = (CountingAuth::new(false, None), AtomicUsize::new(0));
        let e = connect_with_retry(&f, |_| {
            n.fetch_add(1, Ordering::SeqCst);
            auth_err("wrong password")
        })
        .err()
        .unwrap();
        assert_eq!((e, n.load(Ordering::SeqCst), f.asked()), (ImapError::Auth("wrong password".into()), 1, 1));
        // a token that is refused: a second try with a second authenticator, and a second refusal says to sign in again
        let (f, n) = (CountingAuth::new(true, None), AtomicUsize::new(0));
        let e = connect_with_retry(&f, |_| {
            n.fetch_add(1, Ordering::SeqCst);
            auth_err("token expired")
        })
        .err()
        .unwrap();
        assert_eq!((n.load(Ordering::SeqCst), f.asked()), (2, 2));
        assert!(matches!(&e, ImapError::Auth(t) if t.starts_with("token expired") && t.contains("refused a fresh token as well") && t.contains(crate::mail::oauth::SIGN_IN_AGAIN)), "{e}");
        // a failure that is not a refusal is not retried, and one on the second try is passed on as it is
        let (f, n) = (CountingAuth::new(true, None), AtomicUsize::new(0));
        let e = connect_with_retry(&f, |_| {
            n.fetch_add(1, Ordering::SeqCst);
            Err::<Client, _>(ImapError::Connection("down".into()))
        })
        .err()
        .unwrap();
        assert_eq!((e, n.load(Ordering::SeqCst), f.asked()), (ImapError::Connection("down".into()), 1, 1));
        let (f, n) = (CountingAuth::new(true, None), AtomicUsize::new(0));
        let e = connect_with_retry(&f, |_| match n.fetch_add(1, Ordering::SeqCst) {
            0 => auth_err("expired"),
            _ => Err(ImapError::Tls("bad certificate".into())),
        })
        .err()
        .unwrap();
        assert_eq!((e, n.load(Ordering::SeqCst)), (ImapError::Tls("bad certificate".into()), 2));
        // no authenticator at all: an auth error, nothing is connected; none on the second ask: one connection was tried
        let (f, n) = (CountingAuth::new(true, Some(1)), AtomicUsize::new(0));
        let e = connect_with_retry(&f, |_| {
            n.fetch_add(1, Ordering::SeqCst);
            auth_err("x")
        })
        .err()
        .unwrap();
        assert_eq!((e, n.load(Ordering::SeqCst)), (ImapError::Auth("no token (call 1)".into()), 0));
        let (f, n) = (CountingAuth::new(true, Some(2)), AtomicUsize::new(0));
        let e = connect_with_retry(&f, |_| {
            n.fetch_add(1, Ordering::SeqCst);
            auth_err("x")
        })
        .err()
        .unwrap();
        assert_eq!((e, n.load(Ordering::SeqCst)), (ImapError::Auth("no token (call 2)".into()), 1));
        // the second try works: a signed-in client, two authenticators asked for
        let server = server();
        let (f, n) = (CountingAuth::new(true, None), AtomicUsize::new(0));
        let c = connect_with_retry(&f, |_| match n.fetch_add(1, Ordering::SeqCst) {
            0 => auth_err("expired"),
            _ => {
                let (user, password) = {
                    let s = server.state();
                    (s.user.clone(), s.password.clone())
                };
                Client::connect_test(&server.settings(), server_cert(), &mut PlainAuth::new(&user, &password))
            }
        });
        c.unwrap().logout();
        assert_eq!((n.load(Ordering::SeqCst), f.asked()), (2, 2));
    }

    // spec: MAIL-222
    #[test]
    fn the_production_connector_says_a_refused_connection_and_an_untrusted_certificate_as_they_are_and_does_not_retry() {
        let settings = |port: u16| ConnectSettings {
            host: "localhost".into(),
            port,
            security: MailSecurity::ImplicitTls,
            limits: Limits { connect_timeout: std::time::Duration::from_secs(5), io_timeout: std::time::Duration::from_secs(5), ..Limits::default() },
        };
        // nothing listens: a connection error, the factory asked once even though it would give a new token
        let auth = Arc::new(CountingAuth::new(true, None));
        let down = crate::mail::imap_fake_tests::closed_port();
        let c = ImapConnector { settings: settings(down.port), auth: auth.clone() };
        assert!(matches!(c.connect(), Err(ImapError::Connection(_))));
        assert_eq!(auth.asked(), 1);
        // the pretend server's certificate is not in the system's store: refused with a TLS error, not a retry
        let server = server();
        let auth = Arc::new(CountingAuth::new(true, None));
        let c = ImapConnector { settings: settings(server.port), auth: auth.clone() };
        assert!(matches!(c.connect(), Err(ImapError::Tls(_))));
        assert_eq!(auth.asked(), 1);
        // the real factory with a password typed for this session builds the connector from the account without the keyring
        let account = MailAccount { id: "t-real".into(), host: "localhost".into(), port: server.port, user: "ada".into(), ..MailAccount::default() };
        let conn = real_factory()(&account, Some("pw".into())).unwrap();
        assert!(matches!(conn.connect(), Err(ImapError::Tls(_))));
        let account = MailAccount { port: down.port, ..account };
        assert!(matches!(real_factory()(&account, Some("pw".into())).unwrap().connect(), Err(ImapError::Connection(_))));
        // the lazy connector builds once, and a factory that fails is the sign-in error of every connect
        let made = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let m = made.clone();
        let lazy = LazyConnector::new(move || {
            m.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Err("no password for this account".into())
        });
        for _ in 0..3 {
            assert!(matches!(lazy.connect(), Err(ImapError::Auth(t)) if t == NO_PASSWORD));
        }
        assert_eq!(made.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    // spec: MAIL-222
    #[test]
    fn the_connector_of_an_oauth_account_takes_its_settings_from_the_account_and_a_broken_oauth_setup_is_said_before_any_connection() {
        let account = MailAccount {
            id: "t-imap-oauth-connector".into(),
            host: "imap.example.org".into(),
            port: 1993,
            user: "ada@example.org".into(),
            auth: crate::config::MailAuth::OAuth2,
            security: MailSecurity::StartTls,
            oauth_provider: crate::config::OAuthProvider::Custom,
            oauth_client_id: "shale".into(),
            oauth_auth_url: "http://127.0.0.1:1/authorize".into(),
            oauth_token_url: "http://127.0.0.1:1/token".into(),
            oauth_scope: "mail".into(),
            ..MailAccount::default()
        };
        let limits = Limits { max_message: 5, ..Limits::default() };
        let c = connector_for_account(&account, limits, Some("typed-secret".into())).unwrap();
        assert_eq!((c.settings.host.as_str(), c.settings.port, c.settings.security, c.settings.limits.max_message), ("imap.example.org", 1993, MailSecurity::StartTls, 5));
        // the real factory: a typed secret is no password for an OAuth2 account
        assert!(real_factory()(&account, Some("typed-secret".into())).is_ok());
        // no client ID, a scope that is empty, an endpoint that is not an address: the sentence for the settings dialog
        for (what, broken) in [
            ("client ID", MailAccount { oauth_client_id: " ".into(), ..account.clone() }),
            ("scope", MailAccount { oauth_scope: "".into(), ..account.clone() }),
            ("Token address", MailAccount { oauth_token_url: "nonsense".into(), ..account.clone() }),
        ] {
            let e = connector_for_account(&broken, Limits::default(), Some("s".into())).err().unwrap();
            assert!(e.contains(what), "{what}: {e}");
            assert_eq!(real_factory()(&broken, Some("s".into())).err().unwrap(), e);
        }
    }

    // spec: MAIL-059
    #[test]
    fn a_password_account_without_a_stored_password_is_told_so_and_the_real_factory_maps_it_to_no_password() {
        // The pretend keyring is shared by all tests and keyed by account id: ids of its own that nothing
        // ever stores a password for (and no global switch such as `pretend_no_keyring`) keep this parallel-safe.
        let account = MailAccount { id: "t-imap-no-password".into(), host: "localhost".into(), user: "ada".into(), ..MailAccount::default() };
        assert!(crate::secrets::mail_secret(&account.id, crate::secrets::MailSecret::Password).is_none());
        assert_eq!(auth_factory(&account, None, "localhost", 993).err().unwrap(), "no password for this account in the keyring");
        // a client secret typed for this session is no password
        assert_eq!(auth_factory(&account, Some("typed".into()), "localhost", 993).err().unwrap(), "no password for this account in the keyring");
        // through the production factory: the window looks for exactly NO_PASSWORD
        assert_eq!(real_factory()(&account, None).err().unwrap(), NO_PASSWORD);
        // a lazy connector over it gives that sign-in error on every connect
        let factory = real_factory();
        let acct = account.clone();
        let lazy = LazyConnector::new(move || factory(&acct, None));
        assert!(matches!(lazy.connect(), Err(ImapError::Auth(t)) if t == NO_PASSWORD));
        // a password in the keyring (an id of its own again) is found
        let stored = MailAccount { id: "t-imap-stored-password".into(), ..account };
        assert!(crate::secrets::set_mail_secret(&stored.id, crate::secrets::MailSecret::Password, "pw"));
        assert!(auth_factory(&stored, None, "localhost", 993).is_ok());
        assert!(real_factory()(&stored, None).is_ok());
    }

    fn opts() -> SyncOptions {
        SyncOptions { now: Some(NOW), batch_size: 2, ..SyncOptions::default() }
    }

    fn root(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("shale-sync-{tag}-{}-{}", std::process::id(), NOW));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn server() -> FakeServer {
        let server = FakeServer::start(MailSecurity::ImplicitTls);
        {
            let mut s = server.state();
            s.add_folder("INBOX", &[]);
            s.add_folder("Sent", &["\\Sent"]);
            s.add_folder("Trash", &["\\Trash"]);
            s.add_folder("Spam", &[]); // no special-use: recognised by its name
            s.add_folder("[Gmail]/All Mail", &["\\All"]);
            for i in 0..5 {
                s.add_message("INBOX", message(&format!("inbox {i}"), "Bob <bob@x.de>", &format!("hello number {i}")), NOW - (10 - i) * DAY);
            }
            s.add_message("Sent", message("sent one", "ada@example.com", "outgoing"), NOW - 2 * DAY);
            s.add_message("Trash", message("trashed", "x@x.de", "in the bin"), NOW - DAY);
            s.add_message("Spam", message("spammy", "x@x.de", "buy now"), NOW - DAY);
            s.add_message("[Gmail]/All Mail", message("dup", "x@x.de", "everything"), NOW - DAY);
        }
        server
    }

    fn run(store: &MailStore, server: &FakeServer, o: &SyncOptions) -> (SyncSummary, Vec<Progress>) {
        let (tx, rx) = channel();
        let cancel = AtomicBool::new(false);
        let summary = sync_account(store, "acct1", "Test account", &connector(server), o, &tx, &cancel);
        drop(tx);
        (summary, rx.try_iter().collect())
    }

    fn inbox_uids(store: &MailStore) -> Vec<u32> {
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        r.list(f.id, 100, 0).unwrap().iter().map(|m| m.uid).collect()
    }

    // spec: MAIL-030, MAIL-031, MAIL-032, MAIL-037
    #[test]
    fn the_first_sync_stores_files_and_rows_skips_trash_junk_and_all_and_reports_progress() {
        let (dir, server) = (root("first"), server());
        let store = MailStore::open(&dir).unwrap();
        let (sum, events) = run(&store, &server, &opts());
        assert_eq!(sum.errors, Vec::<String>::new());
        assert_eq!((sum.folders_synced, sum.messages_added), (2, 6));
        assert_eq!(inbox_uids(&store), vec![5, 4, 3, 2, 1]);
        // files and index agree
        assert!(dir.join("acct1/INBOX/1000/1.eml").is_file());
        let r = store.reader().unwrap();
        assert_eq!(r.search("outgoing", None, 5, 0).unwrap().len(), 1);
        assert_eq!(r.search("number", None, 10, 0).unwrap().len(), 5);
        // Trash, Spam and All Mail were listed but never opened
        let examined: Vec<String> = server.state().verbs().into_iter().filter(|v| v.starts_with("EXAMINE")).collect();
        assert!(examined.iter().all(|v| v.contains("INBOX") || v.contains("Sent")), "{examined:?}");
        let folders = r.folders(Some("acct1")).unwrap();
        assert_eq!(folders.len(), 5);
        assert!(folders.iter().filter(|f| !f.sync_enabled).count() == 3);
        // progress: started, counted, done, and the last event is Finished
        assert!(events.iter().any(|e| matches!(e, Progress::FolderStarted { folder } if folder == "INBOX")));
        assert!(events.iter().any(|e| matches!(e, Progress::Folder { folder, done: 5, total: 5 } if folder == "INBOX")));
        assert!(events.iter().any(|e| matches!(e, Progress::FolderDone { folder, added: 5 } if folder == "INBOX")));
        // the daily backup was made after the good sync
        assert!(std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_str().unwrap().starts_with("index.backup-")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-032
    #[test]
    fn the_first_sync_fetches_newest_first_and_only_the_last_three_years() {
        let (dir, server) = (root("window"), server());
        server.state().add_message("INBOX", message("ancient", "old@x.de", "from long ago"), NOW - 4 * 365 * DAY);
        let store = MailStore::open(&dir).unwrap();
        // one connection: with several, the order in which their commands reach the server is up to the scheduler
        run(&store, &server, &SyncOptions { parallel: 1, ..opts() });
        let r = store.reader().unwrap();
        assert!(r.search("ancient", None, 5, 0).unwrap().is_empty(), "outside the window");
        let fetches: Vec<String> = server.state().verbs().into_iter().filter(|v| v.starts_with("UID FETCH") && v.contains("BODY.PEEK[]")).collect();
        let at = |p: &str| fetches.iter().position(|f| f.starts_with(p)).unwrap_or_else(|| panic!("no {p} in {fetches:?}"));
        assert!(at("UID FETCH 4:5 ") < at("UID FETCH 2:3 ") && at("UID FETCH 2:3 ") < at("UID FETCH 1 "), "newest first: {fetches:?}");
        let searches: Vec<String> = server.state().verbs().into_iter().filter(|v| v.starts_with("UID SEARCH SINCE")).collect();
        assert!(searches[0].ends_with("14-Nov-2023"), "{searches:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-033, MAIL-035, MAIL-009
    #[test]
    fn a_later_sync_fetches_only_new_mail_updates_flags_and_a_deleted_mail_keeps_its_file() {
        for condstore in [true, false] {
            let (dir, server) = (root(&format!("incr{condstore}")), server());
            server.state().condstore = condstore;
            let store = MailStore::open(&dir).unwrap();
            run(&store, &server, &opts());
            let before = server.state().commands.len();
            {
                let mut s = server.state();
                s.add_message("INBOX", message("brand new", "n@x.de", "fresh"), NOW);
                s.set_flags("INBOX", 2, &["\\Seen", "\\Flagged"]);
                s.delete_message("INBOX", 1);
            }
            let (sum, _) = run(&store, &server, &opts());
            assert_eq!(sum.errors, Vec::<String>::new());
            assert_eq!(sum.messages_added, 1);
            let verbs: Vec<String> = server.state().verbs()[before..].to_vec();
            let fetched: Vec<&String> = verbs.iter().filter(|v| v.contains("BODY.PEEK[]")).collect();
            assert_eq!(fetched.len(), 1, "{verbs:?}");
            assert!(fetched[0].starts_with("UID FETCH 6 "), "only the new UID: {}", fetched[0]);
            let r = store.reader().unwrap();
            let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
            let msgs = r.list(f.id, 100, 0).unwrap();
            let by = |uid: u32| msgs.iter().find(|m| m.uid == uid).unwrap().clone();
            assert_eq!(by(2).flags, "\\Flagged \\Seen");
            assert!(by(1).gone_on_server);
            assert!(!by(3).gone_on_server);
            assert!(dir.join("acct1/INBOX/1000/1.eml").is_file(), "the file of a message deleted on the server stays");
            assert_eq!(r.search("fresh", None, 5, 0).unwrap().len(), 1);
            if condstore {
                assert!(verbs.iter().any(|v| v.contains("CHANGEDSINCE")), "{verbs:?}");
            } else {
                assert!(!verbs.iter().any(|v| v.contains("CHANGEDSINCE")));
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    // spec: MAIL-034
    #[test]
    fn a_changed_uidvalidity_fetches_the_folder_again_into_a_new_directory_and_keeps_the_old_files() {
        let (dir, server) = (root("uv"), server());
        let store = MailStore::open(&dir).unwrap();
        run(&store, &server, &opts());
        server.state().renumber("INBOX", 2000);
        let (sum, _) = run(&store, &server, &opts());
        assert_eq!(sum.errors, Vec::<String>::new());
        assert!(dir.join("acct1/INBOX/1000/3.eml").is_file(), "old files stay");
        assert!(dir.join("acct1/INBOX/2000/3.eml").is_file());
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        assert_eq!(f.uidvalidity, Some(2000));
        assert_eq!(r.list(f.id, 100, 0).unwrap().len(), 5, "listed once, not twice");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-204
    #[test]
    fn after_a_renumbering_mail_the_server_no_longer_has_stays_as_a_local_copy_without_actions() {
        let (dir, server) = (root("uv-keep"), server());
        let store = MailStore::open(&dir).unwrap();
        run(&store, &server, &opts());
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        let gone_subject = r.list(f.id, 100, 0).unwrap().into_iter().find(|m| m.uid == 2).unwrap().subject;
        {
            let mut s = server.state();
            s.renumber("INBOX", 2000);
            s.delete_message("INBOX", 2); // the server forgot this one in the renumbering
        }
        let (sum, _) = run(&store, &server, &opts());
        assert_eq!(sum.errors, Vec::<String>::new());
        let r = store.reader().unwrap();
        let list = r.list(f.id, 100, 0).unwrap();
        assert_eq!(list.len(), 5, "4 from the server once each, plus the one only kept locally");
        let kept: Vec<_> = list.iter().filter(|m| m.uidvalidity == 1000).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].subject, gone_subject);
        assert!(kept[0].gone_on_server, "shown as not on the server");
        assert!(list.iter().filter(|m| m.uidvalidity == 2000).all(|m| !m.gone_on_server));
        assert_eq!(r.search(&gone_subject, None, 5, 0).unwrap().len(), 1, "found by search");
        assert_eq!(r.count_folder(f.id).unwrap(), 5);
        assert_eq!(r.folder_counts().unwrap()[&f.id].0, 5);
        assert_eq!(r.position_in_list(f.id, kept[0].date, kept[0].uid).unwrap() as usize, list.iter().position(|m| m.id == kept[0].id).unwrap());
        // a second sync does not bring it back to life or hide it
        run(&store, &server, &opts());
        assert_eq!(store.reader().unwrap().count_folder(f.id).unwrap(), 5);
        let before = server.state().commands.len();
        for action in [crate::mail::ops::Action::Delete, crate::mail::ops::Action::MoveTo { folder: "Sent".into() }, crate::mail::ops::Action::SetFlags { add: vec![crate::mail::imap_client::MailFlag::Seen], remove: vec![] }] {
            let out = crate::mail::ops::enqueue(&store, std::slice::from_ref(kept[0]), &action, NOW).unwrap();
            assert!(!out.notes.is_empty(), "refused with a note");
        }
        assert!(store.reader().unwrap().pending_ops(None).unwrap().is_empty());
        assert_eq!(server.state().commands.len(), before, "nothing went to the server");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-032, MAIL-037
    #[test]
    fn a_cancelled_sync_resumes_with_what_is_missing() {
        let (dir, server) = (root("resume"), server());
        server.state().fetch_delay = std::time::Duration::from_millis(40);
        let store = MailStore::open(&dir).unwrap();
        let (tx, rx) = channel();
        let cancel = AtomicBool::new(false);
        let o = SyncOptions { parallel: 1, batch_size: 1, ..opts() };
        let cancel_ref = &cancel;
        let summary = std::thread::scope(|s| {
            s.spawn(move || {
                // cancel as soon as the first batch is reported
                while let Ok(e) = rx.recv() {
                    if matches!(e, Progress::Folder { .. }) {
                        cancel_ref.store(true, Ordering::SeqCst);
                    }
                    if matches!(e, Progress::FolderDone { .. } | Progress::Error { .. }) {
                        break;
                    }
                }
            });
            sync_account(&store, "acct1", "T", &connector(&server), &o, &tx, &cancel)
        });
        assert!(summary.cancelled);
        let partial = store.reader().unwrap().count_messages().unwrap();
        assert!((1..6).contains(&partial), "partial: {partial}");
        let r = store.reader().unwrap();
        let inbox = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        assert_eq!(inbox.uidnext, None, "an unfinished folder is not marked done");
        // second run: nothing already stored is fetched again
        let before = server.state().commands.len();
        server.state().fetch_delay = std::time::Duration::ZERO;
        let (sum, _) = run(&store, &server, &opts());
        assert_eq!(sum.errors, Vec::<String>::new());
        assert_eq!(store.reader().unwrap().count_messages().unwrap(), 6);
        let stored_before: Vec<u32> = (1..=5).collect();
        let refetched = server.state().verbs()[before..].iter().filter(|v| v.contains("BODY.PEEK[]")).count();
        assert!(refetched <= stored_before.len() + 1 - partial as usize + 1, "only the missing ones: {refetched}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-023, MAIL-100
    #[test]
    fn a_whole_sync_sends_only_read_only_commands() {
        let (dir, server) = (root("readonly"), server());
        let store = MailStore::open(&dir).unwrap();
        run(&store, &server, &opts());
        server.state().add_message("INBOX", message("again", "n@x.de", "x"), NOW);
        run(&store, &server, &opts());
        let cmds = server.state().commands.clone();
        assert!(cmds.len() > 20);
        for c in &cmds {
            let up = c.to_ascii_uppercase();
            let verb = up.split_whitespace().nth(1).unwrap_or("");
            assert!(["CAPABILITY", "AUTHENTICATE", "LIST", "EXAMINE", "UID", "LOGOUT"].contains(&verb), "{c}");
            for banned in ["SELECT", "STORE", "EXPUNGE", "COPY", "MOVE", "APPEND", "DELETE"] {
                assert!(!up.split_whitespace().skip(1).any(|w| w == banned), "{c}");
            }
            let stripped = up.replace("BODY.PEEK[", "").replace("RFC822.SIZE", "");
            assert!(!stripped.contains("BODY[") && !stripped.contains("RFC822"), "{c}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-036
    #[test]
    fn several_connections_work_on_different_folders_at_once_but_never_more_than_four() {
        let (dir, server) = (root("parallel"), server());
        {
            let mut s = server.state();
            for n in 0..6 {
                let name = format!("Project {n}");
                s.add_folder(&name, &[]);
                for i in 0..2 {
                    s.add_message(&name, message(&format!("p{n}-{i}"), "a@x.de", "text"), NOW - DAY);
                }
            }
            s.fetch_delay = std::time::Duration::from_millis(120);
        }
        let store = MailStore::open(&dir).unwrap();
        let (sum, _) = run(&store, &server, &opts());
        assert_eq!(sum.errors, Vec::<String>::new());
        assert_eq!(sum.folders_synced, 8);
        let max = server.state().max_open_connections;
        assert!((2..=4).contains(&max), "connections at once: {max}");
        assert_eq!(store.reader().unwrap().count_messages().unwrap(), 5 + 1 + 12);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-037, MAIL-039
    #[test]
    fn an_error_in_one_folder_does_not_stop_the_others() {
        let (dir, server) = (root("onefails"), server());
        server.state().refuse_examine.push("Sent".into());
        let store = MailStore::open(&dir).unwrap();
        let (sum, events) = run(&store, &server, &opts());
        assert_eq!((sum.folders_synced, sum.folders_failed), (1, 1));
        assert!(sum.errors[0].starts_with("Sent:"), "{:?}", sum.errors);
        assert!(events.iter().any(|e| matches!(e, Progress::Error { folder: Some(f), .. } if f == "Sent")));
        assert_eq!(inbox_uids(&store).len(), 5);
        // no backup after a sync with errors
        assert!(!std::fs::read_dir(&dir).unwrap().flatten().any(|e| e.file_name().to_str().unwrap().starts_with("index.backup-")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-038
    #[test]
    fn a_message_over_the_limit_is_not_stored_and_not_fetched_again() {
        let (dir, server) = (root("big"), server());
        server.state().pad_message = Some((3, 2_000_000));
        let store = MailStore::open(&dir).unwrap();
        let mut o = opts();
        o.limits.max_message = 500_000;
        let (sum, _) = run(&store, &server, &o);
        assert_eq!(sum.errors, Vec::<String>::new());
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        let big = r.message(f.id, 3).unwrap().unwrap();
        assert_eq!(big.state, STATE_TOO_LARGE);
        assert!(big.size > 2_000_000 && big.path.is_empty());
        assert!(!dir.join("acct1/INBOX/1000/3.eml").exists());
        assert_eq!(r.list(f.id, 10, 0).unwrap().len(), 5, "the others came through");
        server.state().add_message("INBOX", message("later", "a@x.de", "x"), NOW);
        let before = server.state().commands.len();
        run(&store, &server, &o);
        assert!(!server.state().verbs()[before..].iter().any(|v| v.starts_with("UID FETCH 3")), "not downloaded again");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-014
    #[test]
    fn a_garbled_message_from_the_server_is_stored_raw_and_the_sync_goes_on() {
        let (dir, server) = (root("garbled"), server());
        let junk: Vec<u8> = (0..2000u32).map(|i| (i.wrapping_mul(2654435761) >> 11) as u8).collect();
        let uid = server.state().add_message("INBOX", junk.clone(), NOW - DAY);
        let store = MailStore::open(&dir).unwrap();
        let (sum, _) = run(&store, &server, &opts());
        assert_eq!(sum.errors, Vec::<String>::new());
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        let m = r.message(f.id, uid).unwrap().unwrap();
        assert!(!m.parsed);
        assert_eq!(std::fs::read(dir.join(&m.path)).unwrap(), junk);
        assert_eq!(r.list(f.id, 20, 0).unwrap().len(), 6);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-031
    #[test]
    fn trash_and_junk_are_browsed_on_request_and_a_message_is_downloaded_only_when_opened() {
        let (dir, server) = (root("trash"), server());
        let store = MailStore::open(&dir).unwrap();
        run(&store, &server, &opts());
        let c = connector(&server);
        assert_eq!(fetch_folder_listing(&store, &c, "acct1", "Trash", 50, &opts()).unwrap(), 1);
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "Trash").unwrap();
        let listed = r.list(f.id, 10, 0).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!((listed[0].state, listed[0].subject.as_str()), (STATE_HEADER, "trashed"));
        assert!(!dir.join("acct1/Trash").exists(), "no body downloaded yet");
        assert!(!f.sync_enabled);
        let opened = fetch_message(&store, &c, "acct1", "Trash", listed[0].uid, &opts()).unwrap();
        assert_eq!(opened.state, 0);
        assert!(dir.join(&opened.path).is_file());
        assert_eq!(store.read_mail(&opened).unwrap().text, "in the bin");
        // listing again adds nothing, and Spam can be browsed the same way
        assert_eq!(fetch_folder_listing(&store, &c, "acct1", "Trash", 50, &opts()).unwrap(), 0);
        assert_eq!(fetch_folder_listing(&store, &c, "acct1", "Spam", 50, &opts()).unwrap(), 1);
        // a later full sync still does not touch them
        let before = server.state().commands.len();
        run(&store, &server, &opts());
        assert!(!server.state().verbs()[before..].iter().any(|v| v.contains("Trash") || v.contains("Spam")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-007
    #[test]
    fn a_file_that_went_missing_is_fetched_again_by_the_next_sync() {
        let (dir, server) = (root("refetch"), server());
        {
            let store = MailStore::open(&dir).unwrap();
            run(&store, &server, &opts());
        }
        std::fs::remove_file(dir.join("acct1/INBOX/1000/2.eml")).unwrap();
        let store = MailStore::open(&dir).unwrap();
        assert_eq!(store.opened.missing_files, 1);
        run(&store, &server, &opts());
        assert!(dir.join("acct1/INBOX/1000/2.eml").is_file());
        assert!(store.reader().unwrap().missing_uids(1, 1000).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-039
    #[test]
    fn the_background_job_always_ends_with_a_summary() {
        let (dir, server) = (root("job"), server());
        let store = Arc::new(MailStore::open(&dir).unwrap());
        let c: Arc<dyn Connector> = Arc::new(connector(&server));
        let busy = crate::busy::Busy::default();
        let rx = start(store.clone(), "acct1".into(), "T".into(), c, opts(), Arc::new(AtomicBool::new(false)), busy.begin(MAIL_SYNC.with("acct1"), "Syncing T…").unwrap());
        let last = std::iter::from_fn(|| rx.recv_timeout(std::time::Duration::from_secs(20)).ok()).last().unwrap();
        assert!(matches!(&last, Progress::Finished(s) if s.messages_added == 6), "{last:?}");
        // a connector that panics still ends with a Finished event
        struct Boom;
        impl Connector for Boom {
            fn connect(&self) -> Result<Client, ImapError> {
                panic!("boom")
            }
        }
        let rx = start(store, "acct1".into(), "T".into(), Arc::new(Boom), opts(), Arc::new(AtomicBool::new(false)), busy.begin(MAIL_SYNC.with("acct1"), "Syncing T…").expect("the first task ended with its job"));
        let last = rx.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
        assert!(matches!(&last, Progress::Finished(s) if !s.errors.is_empty()), "{last:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A connector that waits for a signal before it connects (the gate hook of the JOB-021 tests).
    struct Gated {
        inner: TestConnector,
        gate: Mutex<Option<Receiver<()>>>,
    }

    impl Connector for Gated {
        fn connect(&self) -> Result<Client, ImapError> {
            // only the first connection waits; the workers of the folders connect again afterwards
            if let Some(gate) = self.gate.lock().unwrap().take() {
                let _ = gate.recv_timeout(std::time::Duration::from_secs(20));
            }
            self.inner.connect()
        }
    }

    fn gated(server: &FakeServer) -> (Arc<dyn Connector>, Sender<()>) {
        let (tx, rx) = channel();
        (Arc::new(Gated { inner: connector(server), gate: Mutex::new(Some(rx)) }), tx)
    }

    fn finished(rx: &Receiver<Progress>) -> SyncSummary {
        loop {
            if let Progress::Finished(s) = rx.recv_timeout(std::time::Duration::from_secs(30)).expect("the sync ends") {
                return s;
            }
        }
    }

    /// Start the sync of an account as its task, the way the mail view does.
    fn start_task(busy: &crate::busy::Busy, store: &Arc<MailStore>, id: &str, display: &str, c: Arc<dyn Connector>, o: SyncOptions) -> Receiver<Progress> {
        let mut task = busy.begin(MAIL_SYNC.with(id), format!("Syncing {display}…")).expect("the account is idle");
        let cancel = task.cancellable().flag();
        start(store.clone(), id.into(), display.into(), c, o, cancel, task)
    }

    // spec: JOB-021
    #[test]
    fn job_021_a_sync_is_a_task_with_the_account_name_that_cancel_stops_between_batches_and_a_later_sync_completes() {
        let (dir, server) = (root("job021"), server());
        // the server serves a batch only when the test sends a token: the first one, then none until Cancel
        let (serve, tokens) = channel();
        let mut serve = Some(serve);
        server.state().fetch_gate = Some(Arc::new(Mutex::new(tokens)));
        let store = Arc::new(MailStore::open(&dir).unwrap());
        let busy = crate::busy::Busy::default();
        let (c, gate) = gated(&server);
        let o = SyncOptions { parallel: 1, batch_size: 1, ..opts() };
        let rx = start_task(&busy, &store, "acct1", "Work mail", c, o.clone());
        // while it runs (held at the gate) it is listed with the account name, and a second one is refused
        let info = busy.info(&MAIL_SYNC.with("acct1")).expect("listed while it runs");
        assert_eq!(info.label, "Syncing Work mail…");
        assert!(info.cancellable && !info.cancelling);
        assert!(busy.begin(MAIL_SYNC.with("acct1"), "again").is_none(), "never two syncs of one account");
        gate.send(()).unwrap();
        serve.as_ref().unwrap().send(()).unwrap();
        // Cancel after the first batch has been stored (the next fetch waits for a token): the sync stops between batches
        let mut summary = None;
        let mut cancelled = false;
        while let Ok(e) = rx.recv_timeout(std::time::Duration::from_secs(30)) {
            match e {
                Progress::Folder { .. } if !cancelled => {
                    cancelled = true;
                    assert!(busy.cancel(info.id), "the registry's Cancel reaches the sync");
                    assert!(busy.info(&MAIL_SYNC.with("acct1")).is_some_and(|t| t.cancelling));
                    // Cancel is set: closing the gate lets the waiting fetch through, the sync stops at its next check
                    serve.take();
                }
                Progress::Finished(s) => {
                    summary = Some(s);
                    break;
                }
                _ => {}
            }
        }
        let summary = summary.expect("the sync ends");
        assert!(summary.cancelled && summary.errors.is_empty(), "{summary:?}");
        assert!(busy.list().is_empty(), "the task ended before the last report");
        let partial = store.reader().unwrap().count_messages().unwrap();
        assert!((1..6).contains(&partial), "some messages are whole, not all: {partial}");
        // no message file is half written, the index is sound (MAIL-006, MAIL-007)
        crate::mail::db::quick_check(&store.index_path()).expect("the index passes its check after a stop");
        let mut stray = vec![];
        for e in walk(&dir) {
            if e.extension().is_some_and(|x| x == "tmp" || x == "part") {
                stray.push(e);
            }
        }
        assert!(stray.is_empty(), "no half-written file stays: {stray:?}");
        // the next sync continues normally with a fresh token
        let (c, gate) = gated(&server);
        gate.send(()).unwrap();
        let rx = start_task(&busy, &store, "acct1", "Work mail", c, opts());
        let again = finished(&rx);
        assert!(!again.cancelled && again.errors.is_empty(), "{again:?}");
        assert_eq!(store.reader().unwrap().count_messages().unwrap(), 6);
        assert!(busy.list().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
        let mut out = vec![];
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push(p);
            }
        }
        out
    }

    // spec: JOB-021
    #[test]
    fn job_021_two_accounts_sync_as_two_tasks_and_cancelling_one_leaves_the_other_running() {
        let (dir, s1, s2) = (root("job021-two"), server(), server());
        let store = Arc::new(MailStore::open(&dir).unwrap());
        let busy = crate::busy::Busy::default();
        let (c1, g1) = gated(&s1);
        let (c2, g2) = gated(&s2);
        let rx1 = start_task(&busy, &store, "acct1", "Work", c1, opts());
        let rx2 = start_task(&busy, &store, "acct2", "Home", c2, opts());
        let labels: Vec<String> = busy.list().into_iter().map(|t| t.label).collect();
        assert_eq!(labels, ["Syncing Work…", "Syncing Home…"], "two tasks, one per account");
        // Cancel the first while both wait: only it stops
        assert!(busy.cancel(busy.info(&MAIL_SYNC.with("acct1")).unwrap().id));
        assert!(busy.info(&MAIL_SYNC.with("acct2")).is_some_and(|t| !t.cancelling));
        g1.send(()).unwrap();
        let one = finished(&rx1);
        assert!(one.cancelled, "{one:?}");
        assert_eq!(busy.list().iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Syncing Home…"], "the other still runs");
        g2.send(()).unwrap();
        let two = finished(&rx2);
        assert!(!two.cancelled && two.errors.is_empty() && two.messages_added == 6, "{two:?}");
        assert!(busy.list().is_empty());
        crate::mail::db::quick_check(&store.index_path()).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-030, MAIL-031
    #[test]
    fn folders_are_classified_by_special_use_then_by_name() {
        let a = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(classify(&a(&["\\Trash"]), "Anything"), FolderKind::Trash);
        assert_eq!(classify(&a(&["\\Junk"]), "Anything"), FolderKind::Junk);
        assert_eq!(classify(&a(&["\\All"]), "[Gmail]/All Mail"), FolderKind::All);
        assert_eq!(classify(&a(&[]), "Deleted Items"), FolderKind::Trash);
        assert_eq!(classify(&a(&[]), "INBOX.Spam"), FolderKind::Junk);
        assert_eq!(classify(&a(&[]), "Junk E-mail"), FolderKind::Junk);
        assert_eq!(classify(&a(&["\\Sent"]), "Trash"), FolderKind::Normal, "special-use wins over the name");
        assert_eq!(classify(&a(&[]), "Trash/Project"), FolderKind::Normal);
        assert_eq!(classify(&a(&[]), "INBOX"), FolderKind::Normal);
    }

    #[test]
    fn the_window_starts_three_years_back() {
        assert_eq!(window_start(NOW, 3), (2023, 11, 14));
        assert_eq!(window_start(1_709_208_000, 4), (2020, 2, 29));
        assert_eq!(window_start(1_709_208_000, 3), (2021, 2, 28));
    }
}
