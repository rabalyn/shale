//! The mail view's state and the work behind it (ADR-0007, phase 1b).
//!
//! `MailView` owns what the window shows: the opened store, the folder list with its counts, the
//! page cache of the message list, the open message, the search, the syncs that run and the
//! jobs behind the buttons. Everything slow (opening the store, reading and parsing a message,
//! searching, talking to a server) runs as a job through `jobs` and reports back over one
//! channel, so a frame never waits for it. The drawing is in `mail/view.rs`, the account
//! settings in `mail/dialog.rs`.
//!
//! Syncing never changes a server: it uses only the read-only commands of `imap_sync`. What the
//! person does (mark read, flag, move, delete) is recorded and sent by `mail_ops` (ADR-0009): the
//! view enqueues, starts a job per account, shows the optimistic state and the waiting marker, and
//! tells the person when the server refused something.

use crate::busy::{Busy, Key};
use crate::config::{Config, MailAccount};
use crate::mail::compose::{self, Kind};
use crate::mail::compose_ui::{Ask, Compose, ComposeOut, Field, LocalOut, LocalView, VaultInfo};
use crate::mail::crypto::{self, Encryption, Security, Structure};
use crate::mail::db::{FolderRow, MessageSummary, STATE_FULL, STATE_HEADER, STATE_TOO_LARGE};
use crate::mail::gpg::Gnupg;
use crate::mail::imap_client::decode_mutf7;
use crate::mail::imap_client::MailFlag;
use crate::mail::imap_sync::{self, ConnectorFactory, FolderKind, LazyConnector, Progress, SyncOptions, SyncSummary, NO_PASSWORD};
use crate::mail::ops::{self, Action, Confirmed, OpsReport};
use crate::mail::outbox::{DraftInfo, Item, Outbox, Recovery, State};
use crate::mail::send::{self, SendReport, SmtpConnector};
use crate::mail::store::{MailStore, Reader};
use crate::mail::watch::{Wanted, Watch};
use crate::mail::Mail;
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

/// The check of the mail index when the store opens (MAIL-006, JOB-015): started at app start, shown by the mail view through this key.
pub const MAIL_INDEX: Key = Key::new("mail-index");

/// The rebuild of the mail index (USE-012, JOB-016): a task that Cancel can stop between two folders.
pub const MAIL_REBUILD: Key = Key::new("mail-rebuild");

/// The sync of one mail account (JOB-021): one task per account, `MAIL_SYNC.with(account id)`, that Cancel
/// stops between two batches of messages or two folders. The idle wait of the watch (IDLE) is not a task.
pub const MAIL_SYNC: Key = Key::new("mail-sync");

/// What the store is doing.
pub enum StoreState {
    Closed,
    Opening,
    Ready(Arc<MailStore>),
    /// The index is being built again (USE-012): nothing syncs, sends or changes it meanwhile.
    Rebuilding(Arc<MailStore>),
    Failed(String),
}

/// A message read and parsed for the reading pane.
pub struct Loaded {
    pub mail: Mail,
    /// The text as shown (cut at `TEXT_CAP`).
    pub shown: String,
    pub cut: bool,
    /// What GnuPG says about signatures and encryption (ADR-0011); empty for a plain mail.
    pub security: Security,
}

/// What GnuPG is and has on this computer (MAIL-191): the versions and the secret keys, for the settings.
#[derive(Clone, Debug, Default)]
pub struct GnupgInfo {
    /// Per standard: the version line, or why the program cannot be used.
    pub pgp: Option<Result<String, String>>,
    pub smime: Option<Result<String, String>>,
    pub pgp_keys: Vec<crate::mail::gpg::KeyInfo>,
    pub smime_keys: Vec<crate::mail::gpg::KeyInfo>,
}

impl GnupgInfo {
    pub fn version(&self, s: crate::config::CryptoStandard) -> Option<&Result<String, String>> {
        match s {
            crate::config::CryptoStandard::OpenPgp => self.pgp.as_ref(),
            crate::config::CryptoStandard::Smime => self.smime.as_ref(),
        }
    }
    pub fn keys(&self, s: crate::config::CryptoStandard) -> &[crate::mail::gpg::KeyInfo] {
        match s {
            crate::config::CryptoStandard::OpenPgp => &self.pgp_keys,
            crate::config::CryptoStandard::Smime => &self.smime_keys,
        }
    }
}

/// Read what GnuPG has: used by the settings and by the tests.
pub fn probe_gnupg(g: &Gnupg) -> GnupgInfo {
    use crate::config::CryptoStandard::{OpenPgp, Smime};
    let cancel = AtomicBool::new(false);
    let one = |s| match g.version(s) {
        Ok(v) => (Some(Ok(v)), g.list_keys(s, true, None, &cancel).unwrap_or_default()),
        Err(e) => (Some(Err(e.to_string())), vec![]),
    };
    let (pgp, pgp_keys) = one(OpenPgp);
    let (smime, smime_keys) = one(Smime);
    GnupgInfo { pgp, smime, pgp_keys, smime_keys }
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

/// The browser sign-in of an OAuth2 account (MAIL-086).
pub enum SignInState {
    /// Waiting for the person in the browser; `url` is the sign-in address (to copy when no
    /// browser opened) once it is known.
    Waiting {
        cancel: Arc<AtomicBool>,
        url: Option<String>,
    },
    /// Signed in; `user` when the provider said who; `kept`: the keyring has the refresh token.
    Done {
        user: Option<String>,
        kept: bool,
    },
    Failed(String),
}

/// One sync of one account, running or finished.
pub struct SyncRun {
    pub(crate) rx: Receiver<Progress>,
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
    pub text: crate::secret::Secret,
    /// The field takes the keyboard when it first appears (MAIL-065).
    pub focus: bool,
    pub retry: Retry,
}

/// What to do again once the password is known.
#[derive(Clone)]
pub enum Retry {
    Sync,
    Listing(i64),
    Message {
        folder: i64,
        uid: u32,
    },
    /// Send the actions that wait for the server.
    Ops,
}

/// "Move to…": the messages and what has been typed to find the folder (MAIL-110).
pub struct MovePicker {
    pub msgs: Vec<MessageSummary>,
    pub query: String,
    pub cursor: usize,
    pub focus: bool,
}

/// "Delete forever?": waits for the person's confirmation (MAIL-105).
pub struct ForeverAsk {
    pub msgs: Vec<MessageSummary>,
}

/// "Delete the local copy?": waits for the person's confirmation (MAIL-238).
pub struct LocalDeleteAsk {
    pub msgs: Vec<MessageSummary>,
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
    Search {
        gen: u64,
        result: Result<Vec<MessageSummary>, String>,
    },
    Message {
        id: i64,
        result: Result<Arc<Loaded>, String>,
    },
    Fetched {
        key: (i64, u32),
        retry: Retry,
        result: Result<Box<MessageSummary>, String>,
    },
    Listing {
        folder: i64,
        result: Result<usize, String>,
    },
    Tested {
        account: String,
        /// The folders found, and how signing in to SMTP went (None: the account has no SMTP server set).
        result: Result<(usize, Option<Result<(), String>>), String>,
    },
    Secret {
        account: String,
        stored: bool,
    },
    SignInUrl {
        account: String,
        url: String,
    },
    SignedIn {
        account: String,
        result: Result<crate::mail::oauth::SignedIn, String>,
    },
    /// Does the account have a refresh token? (read from the keyring in a job)
    Probed {
        account: String,
        signed_in: bool,
    },
    /// A run of `ops::run_pending` ended.
    Ops {
        account: String,
        result: Result<OpsReport, String>,
    },
    /// The start-up check of drafts and outbox is done (MAIL-130, MAIL-133).
    Recovered(Result<Recovery, String>),
    /// A run of `send::send_due` ended.
    Sent {
        account: String,
        result: Result<SendReport, String>,
    },
    /// GnuPG has been asked about this message (signatures, decryption).
    Crypto {
        id: i64,
        result: Result<Arc<Loaded>, String>,
    },
    /// What GnuPG is and has on this computer.
    Gnupg(Result<GnupgInfo, String>),
    /// The agent's cache times were read (MAIL-235).
    Agent(Result<crate::mail::agent_cache::AgentInfo, String>),
    /// The cache times were changed (or not) and read again (MAIL-236).
    AgentWritten {
        result: Result<(), String>,
        info: Result<crate::mail::agent_cache::AgentInfo, String>,
    },
    /// The keys found for the recipients of the mail being written (MAIL-192).
    Keys {
        sig: String,
        result: Result<Vec<crypto::KeyLookup>, String>,
    },
    /// A mail that is signed or encrypted has been put into the outbox, or could not be (MAIL-195).
    Queued {
        draft_id: String,
        result: Result<Box<Item>, String>,
    },
    /// The mail to answer or forward has been read.
    ReplyMail {
        kind: Kind,
        msg: Box<MessageSummary>,
        result: Result<Box<Mail>, String>,
    },
}

/// Makes the SMTP connector of an account (tests plug in the pretend server).
pub type SmtpFactory = Arc<dyn Fn(&MailAccount, Option<crate::secret::Secret>) -> Result<Arc<dyn SmtpConnector>, String> + Send + Sync>;

pub fn real_smtp_factory() -> SmtpFactory {
    Arc::new(send::connector_for_account)
}

/// Builds its connector when the first connection is made (inside the job: the keyring may be slow).
struct LazySmtp {
    factory: SmtpFactory,
    account: MailAccount,
    session: Option<crate::secret::Secret>,
}

impl SmtpConnector for LazySmtp {
    fn connect(&self) -> Result<crate::mail::smtp_client::Smtp, crate::mail::smtp_client::SmtpError> {
        let c = (self.factory)(&self.account, self.session.clone()).map_err(crate::mail::smtp_client::SmtpError::Auth)?;
        c.connect()
    }
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
    /// The registry of running work (JOB-003): the app hands over its own, so the check of the index is listed in the indicator (JOB-015).
    pub busy: Busy,
    reader: Option<Reader>,
    // data read from the index when something changed
    pub folders: Vec<FolderRow>,
    pub counts: HashMap<i64, (i64, i64)>,
    dirty: bool,
    last_refresh: Option<Instant>,
    /// Test aid: how many times the list or counts were read from the index.
    pub queries: u64,
    /// Where the view was left when the vault was last closed, until the folders are read and the
    /// place is found (or known to be gone) (NAV-045).
    pub restore: Option<crate::vault_state::SavedMail>,
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
    /// The open folder and the Inboxes, kept fresh while the mail view is open (MAIL-230).
    pub watch: Watch,
    pub session_pw: HashMap<String, crate::secret::Secret>,
    pub prompt: Option<PasswordPrompt>,
    pub tests: HashMap<String, TestState>,
    /// The browser sign-ins of OAuth2 accounts.
    pub signins: HashMap<String, SignInState>,
    /// OAuth2 accounts known to have (true) or not have (false) a refresh token.
    pub signed_in: HashMap<String, bool>,
    /// Controls drawn in the last frame, by name (for the UI tests).
    pub rects: Vec<(String, egui::Rect)>,
    /// Things to tell the person (the app shows them as messages).
    pub notices: Vec<String>,
    // phase 2: flags, move, delete (ADR-0009)
    ops_running: HashSet<String>,
    /// Actions waiting for the server, and the accounts they belong to (read with the counts).
    pub pending_total: i64,
    pending_accounts: HashSet<String>,
    /// Waiting actions per account id, configured or not (MAIL-207).
    pending_by_account: HashMap<String, i64>,
    last_ops: Option<Instant>,
    /// How long to wait before sending waiting actions again (tests shorten it).
    pub ops_retry: Duration,
    /// Why the server could not be reached, per account (shown while actions wait).
    pub offline: HashMap<String, String>,
    /// The open message is marked read when this has run (message id, when it was opened).
    read_timer: Option<(i64, Instant)>,
    pub mover: Option<MovePicker>,
    pub forever: Option<ForeverAsk>,
    pub local_delete: Option<LocalDeleteAsk>,
    /// Actions the server refused or that could not be done, until the person dismisses them.
    pub problems: Vec<String>,
    // phase 3: writing and sending (ADR-0010)
    pub smtp_factory: SmtpFactory,
    pub compose: Option<Compose>,
    pub outbox: Option<Arc<Outbox>>,
    /// The start-up check is done: nothing is sent before.
    recovered: bool,
    pub local_drafts: Vec<DraftInfo>,
    pub local_items: Vec<Item>,
    local_dirty: bool,
    last_local: Option<Instant>,
    pub local_view: Option<LocalView>,
    pub outbox_sel: Option<(String, String)>,
    send_running: HashSet<String>,
    /// Account ids and their names, for the lists (set every frame by the app).
    pub account_names: HashMap<String, String>,
    // phase 4: signing and encrypting (ADR-0011)
    /// How GnuPG is called (tests give it a temp home).
    pub gnupg: Gnupg,
    /// The person has typed the consent to decrypted local storage (set from the settings every frame).
    pub consent: bool,
    crypto_cancel: Option<Arc<AtomicBool>>,
    /// The message GnuPG is being asked about (a result for another message is not the running one's).
    crypto_id: Option<i64>,
    crypto_running: bool,
    /// What GnuPG is and has (read when the settings open).
    pub gnupg_info: Option<GnupgInfo>,
    gnupg_probing: bool,
    /// How long the agent keeps a passphrase, and the write that changes it (MAIL-235, MAIL-236).
    pub agent: crate::mail::agent_cache::AgentState,
    pub vault: Option<VaultInfo>,
    /// Drafts and outbox entries were announced once at start.
    announced_local: bool,
    /// Local sent copies that the server does not have and nothing uploads (MAIL-210).
    pub unsent: Vec<crate::mail::db::UnsentCopy>,
    pub unsent_open: bool,
    /// The keyboard of the drafts and outbox lists, the menu of a row and the unsent window (MAIL-067…069).
    pub(crate) kbd: crate::mail::keys::Kbd,
    // the keyboard (MAIL-062…065)
    /// The pane that has the keyboard.
    pub pane: Pane,
    /// The keyboard is in the left sidebar: the mail keys wait and no pane is marked.
    pub away: bool,
    /// A dialog of the app is open (set by `show`).
    pub(crate) dialog: bool,
    /// Pending scroll of the message pane in points (j / k there).
    pub(crate) read_scroll: f32,
    /// Test aid: how far the message pane is scrolled.
    pub read_offset: f32,
    /// The selected attachment of the open message, and a key asked to open or save it.
    pub att_sel: usize,
    pub(crate) att_open: bool,
    pub(crate) att_save: bool,
    /// The app's one player (MEDIA-007), which plays the audio attachments of the open message (MAIL-240); the app hands over its own.
    pub media: crate::media::MediaPlayer,
    /// The private temp copies of the audio attachments of the open message, which the player plays from (MAIL-240).
    pub(crate) att_audio: Option<AttAudio>,
}

/// The audio attachments of one open message as private temp files (SEC-006): the player reads files, a mail's
/// attachment is in memory. The files are removed when another message opens or the view goes (MAIL-240).
pub(crate) struct AttAudio {
    /// The message and the parsed mail the files were made from.
    pub(crate) msg: i64,
    pub(crate) mail: Arc<Loaded>,
    /// Per attachment of the mail: the file of an audio attachment, None for any other.
    pub(crate) files: Vec<Option<PathBuf>>,
}

impl AttAudio {
    /// The temp copies of the audio attachments of `l`; one that cannot be written has no player (the Open button stays).
    pub(crate) fn make(msg: i64, l: &Arc<Loaded>) -> AttAudio {
        let files = l
            .mail
            .attachments
            .iter()
            .map(|a| {
                let ext = crate::media::attachment_audio_ext(&a.name, &a.mime)?;
                // the player knows a sound by its extension: a name without a sound's one gets it
                let name = if a.name.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext)) { a.name.clone() } else { format!("{}.{ext}", if a.name.trim().is_empty() { "attachment" } else { a.name.as_str() }) };
                crate::opener::save_temp_copy(&name, &a.data).ok()
            })
            .collect();
        AttAudio { msg, mail: l.clone(), files }
    }
}

impl Drop for AttAudio {
    fn drop(&mut self) {
        for f in self.files.iter().flatten() {
            crate::opener::discard_temp_copy(f);
        }
    }
}

/// What the folder pane lists.
#[derive(Clone, Debug, PartialEq, Eq)]
enum FolderEntry {
    Folder(i64),
    Local(LocalView),
}

/// The three panes of the mail view, left to right.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pane {
    Folders,
    List,
    Message,
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
            busy: Busy::default(),
            reader: None,
            folders: vec![],
            counts: HashMap::new(),
            dirty: false,
            last_refresh: None,
            queries: 0,
            restore: None,
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
            watch: Watch::default(),
            session_pw: HashMap::new(),
            prompt: None,
            tests: HashMap::new(),
            signins: HashMap::new(),
            signed_in: HashMap::new(),
            rects: vec![],
            notices: vec![],
            ops_running: HashSet::new(),
            pending_total: 0,
            pending_accounts: HashSet::new(),
            pending_by_account: HashMap::new(),
            last_ops: None,
            ops_retry: Duration::from_secs(30),
            offline: HashMap::new(),
            read_timer: None,
            mover: None,
            forever: None,
            local_delete: None,
            problems: vec![],
            smtp_factory: real_smtp_factory(),
            compose: None,
            outbox: None,
            recovered: false,
            local_drafts: vec![],
            local_items: vec![],
            local_dirty: false,
            last_local: None,
            local_view: None,
            outbox_sel: None,
            send_running: HashSet::new(),
            account_names: HashMap::new(),
            gnupg: Gnupg::default(),
            consent: false,
            crypto_cancel: None,
            crypto_id: None,
            crypto_running: false,
            gnupg_info: None,
            agent: Default::default(),
            gnupg_probing: false,
            vault: None,
            announced_local: false,
            unsent: vec![],
            unsent_open: false,
            kbd: Default::default(),
            pane: Pane::List,
            away: false,
            dialog: false,
            read_scroll: 0.0,
            read_offset: 0.0,
            att_sel: 0,
            att_open: false,
            att_save: false,
            media: crate::media::MediaPlayer::default(),
            att_audio: None,
        }
    }

    #[cfg(test)]
    pub fn reader_for_tests(&self) -> Option<&Reader> {
        self.reader.as_ref()
    }

    /// The index for the picker's search over everything (NAV-022); None until the store is open.
    pub fn index(&self) -> Option<&rusqlite::Connection> {
        self.reader.as_ref().map(Reader::conn)
    }

    /// Show a message the picker found (NAV-025): its folder is listed with the cursor on it and
    /// the message is open in the reading pane.
    pub fn show_hit(&mut self, cfg: &Config, msg: MessageSummary) {
        self.select_folder(cfg, msg.folder_id);
        if let Some(p) = self.reader.as_ref().and_then(|r| r.position_in_list(msg.folder_id, msg.date, msg.uid).ok()) {
            self.cursor = Some(p.max(0) as usize);
            self.scroll_to_cursor = true;
        }
        self.pane = Pane::List;
        self.open_message(msg);
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

    /// Why the mail index cannot be rebuilt now (USE-012): a sync, an action or a send runs,
    /// or the store is not open.
    pub fn rebuild_blocked(&self) -> Option<String> {
        if !matches!(self.store, StoreState::Ready(_)) {
            return Some("the mail store is not open".into());
        }
        if self.any_sync_running() {
            return Some("mail is syncing; try again when it is done".into());
        }
        if !self.ops_running.is_empty() || !self.send_running.is_empty() {
            return Some("changes or mail are being sent to the server; try again when that is done".into());
        }
        if self.watch.checking() {
            return Some("a folder is being checked for changes; try again in a moment".into());
        }
        None
    }

    /// Hand the store over for a rebuild: until [`MailView::end_rebuild`] nothing starts that
    /// writes the index (every such job starts from `ready_store`).
    pub fn begin_rebuild(&mut self) -> Result<Arc<MailStore>, String> {
        if let Some(why) = self.rebuild_blocked() {
            return Err(why);
        }
        let StoreState::Ready(s) = std::mem::replace(&mut self.store, StoreState::Closed) else { unreachable!("checked above") };
        self.close_message();
        self.store = StoreState::Rebuilding(s.clone());
        Ok(s)
    }

    /// The rebuild ended (well or not): the index is read again from the start.
    pub fn end_rebuild(&mut self, cfg: &Config) {
        let StoreState::Rebuilding(s) = &self.store else { return };
        let s = s.clone();
        self.reader = s.reader().ok();
        self.store = StoreState::Ready(s);
        // the folders keep their rows (and ids), so the folder shown stays
        self.refresh();
        if self.folder.is_none() {
            self.select_first_folder(cfg);
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
        // the task ends with the job, before its result is taken in (JOB-003), so the view never finds it stale
        let Some(task) = self.busy.begin(MAIL_INDEX, "Checking the mail index…") else { return };
        self.store = StoreState::Opening;
        let root = self.root.clone();
        self.spawn(
            move || {
                let _task = task;
                MailStore::open(&root).map(Arc::new).map_err(|e| e.0)
            },
            |r| Event::Store(flatten(r)),
        );
    }

    // ------------------------------------------------------------ polling

    /// Take in what the jobs and syncs reported, start the timers' work. Called every frame by
    /// the app; returns true while something is going on (the window keeps repainting).
    pub fn tick(&mut self, cfg: &Config, wanted: bool) -> bool {
        if wanted || !cfg.mail_accounts.is_empty() {
            self.open_store();
        }
        self.consent = cfg.mail_encryption_consent.is_some();
        while let Ok(ev) = self.rx.try_recv() {
            self.handle(ev, cfg);
        }
        self.poll_syncs();
        for id in crate::mail::oauth::take_unsaved_notices() {
            let name = cfg.mail_accounts.iter().find(|a| a.id == id).map_or(id.as_str(), |a| a.name.as_str());
            self.notices.push(format!("The sign-in of “{name}” was renewed but could not be saved in the keyring; you may have to sign in again after a restart."));
        }
        self.tick_compose_keys();
        self.tick_ops(cfg);
        self.tick_local(cfg);
        self.tick_watch(cfg, wanted);
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
        // the place the view was left at (NAV-045), else the first folder (the Inbox) opens by itself once there is one
        if !self.folders.is_empty() && self.reader.is_some() {
            self.restore_place(cfg);
            if self.folder.is_none() {
                self.select_first_folder(cfg);
            }
        }
        self.busy()
    }

    /// Keep the open folder and the Inboxes fresh while the mail view is shown (MAIL-230, MAIL-233):
    /// only folders that a sync has finished once, and not while the store is being rebuilt.
    fn tick_watch(&mut self, cfg: &Config, wanted: bool) {
        if self.watch.drain() {
            self.dirty = true;
        }
        let Some(store) = self.ready_store().filter(|_| wanted && self.watch.enabled && !cfg.mail_accounts.is_empty()) else {
            self.watch.stop_all();
            return;
        };
        let open = self.folder.filter(|_| self.local_view.is_none()).and_then(|id| self.folder_row(id)).map(|f| (f.account.clone(), f.name.clone()));
        let mut targets: Vec<Wanted> = vec![];
        for a in &cfg.mail_accounts {
            let session_password = self.session_pw.contains_key(&a.id);
            for f in self.folders.iter().filter(|f| f.account == a.id && f.sync_enabled && f.uidnext.is_some()) {
                let is_open = open.as_ref().is_some_and(|(acct, name)| *acct == f.account && *name == f.name);
                if is_open || f.name.eq_ignore_ascii_case("INBOX") {
                    targets.push(Wanted { account: a.clone(), folder: f.name.clone(), session_password });
                }
            }
        }
        let mut watch = std::mem::take(&mut self.watch);
        watch.update(&targets, &store, &|a| Arc::new(self.lazy_connector(a, None)), &self.opts, self.ctx.as_ref());
        self.watch = watch;
    }

    /// Is a job, a sync or a debounce going on?
    pub fn busy(&self) -> bool {
        matches!(self.store, StoreState::Opening | StoreState::Rebuilding(_))
            || self.any_sync_running()
            || self.searching
            || self.search_edited.is_some()
            || self.dirty
            || self.fetching.len() + self.listings.values().filter(|l| matches!(l, ListingState::Loading)).count() > 0
            || self.tests.values().any(|t| matches!(t, TestState::Running))
            || self.signins.values().any(|t| matches!(t, SignInState::Waiting { .. }))
            || matches!(self.open.as_ref().map(|o| &o.body), Some(Body::Loading))
            || self.crypto_running
            || self.gnupg_probing
            || self.read_timer.is_some()
            || !self.ops_running.is_empty()
            || self.pending_total > 0
            || self.compose.is_some()
            || !self.send_running.is_empty()
            || self.local_items.iter().any(|i| matches!(i.state, State::Queued | State::Sending))
    }

    fn handle(&mut self, ev: Event, cfg: &Config) {
        match ev {
            Event::Store(Ok(s)) => {
                let o = &s.opened;
                if let Some(q) = &o.quarantined {
                    let how = if o.restored_from.is_some() { "restored from a backup" } else { "rebuilt from the message files" };
                    let queued = if o.dropped_queued > 0 { format!(" {} queued change(s) from the backup were not sent again; check them.", o.dropped_queued) } else { String::new() };
                    self.notices.push(format!("The mail index was damaged: it was {how}. The damaged file is kept as {}.{queued}", q.display()));
                }
                self.reader = s.reader().ok();
                self.store = StoreState::Ready(s);
                self.start_recovery();
                self.refresh();
                self.restore_place(cfg);
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
                let mut check = None;
                if let Some(o) = self.open.as_mut().filter(|o| o.msg.id == id) {
                    o.body = match result {
                        Ok(l) => {
                            if l.security.working.is_some() {
                                check = Some(o.msg.clone());
                            }
                            Body::Ready(l)
                        }
                        Err(e) => Body::Failed(e),
                    };
                    // the message is on screen: it counts as read after the delay (MAIL-109)
                    if matches!(o.body, Body::Ready(_)) && !o.msg.is_seen() && !o.msg.gone_on_server {
                        self.read_timer = Some((id, Instant::now()));
                    }
                }
                if let Some(msg) = check {
                    self.start_crypto(msg);
                }
            }
            Event::Crypto { id, result } => {
                if self.crypto_id == Some(id) {
                    self.crypto_running = false;
                    self.crypto_id = None;
                    self.crypto_cancel = None;
                }
                if self.open.as_ref().is_some_and(|o| o.msg.id == id) {
                    if let Some(o) = self.open.as_mut() {
                        o.body = match result {
                            Ok(l) => Body::Ready(l),
                            Err(e) => Body::Failed(e),
                        };
                    }
                    // the decrypted copy changed what the index knows: the list and the counts are read again
                    self.dirty = true;
                }
            }
            Event::Keys { sig, result } => {
                if let Some(c) = self.compose.as_mut() {
                    c.keys_running = false;
                    match result {
                        Ok(v) => {
                            c.keys = v;
                            c.keys_for = Some(sig);
                        }
                        Err(e) => {
                            c.keys = vec![];
                            c.keys_for = Some(sig);
                            c.error = Some(format!("The keys could not be looked up: {e}"));
                        }
                    }
                }
            }
            Event::Queued { draft_id, result } => {
                if self.compose.as_ref().is_some_and(|c| c.draft.id == draft_id) {
                    if let Some(c) = self.compose.as_mut() {
                        c.crypting = None;
                    }
                    match result {
                        Ok(item) => self.finish_queued(*item),
                        Err(e) => {
                            if let Some(c) = self.compose.as_mut() {
                                c.error = Some(e);
                            }
                        }
                    }
                }
            }
            Event::Gnupg(r) => {
                self.gnupg_probing = false;
                if let Ok(info) = r {
                    self.gnupg_info = Some(info);
                }
            }
            Event::Agent(r) => {
                self.agent.reading = false;
                self.agent.info = Some(r);
            }
            Event::AgentWritten { result, info } => {
                self.agent.writing = false;
                self.agent.written = Some(result);
                self.agent.info = Some(info);
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
            Event::SignInUrl { account, url } => {
                if let Some(SignInState::Waiting { url: u, .. }) = self.signins.get_mut(&account) {
                    *u = Some(url);
                }
            }
            Event::SignedIn { account, result } => {
                match result {
                    Ok(r) => {
                        self.signed_in.insert(account.clone(), true);
                        if !r.kept {
                            self.notices.push("No keyring could keep the sign-in: Shale remembers it until you quit and asks you to sign in again next time.".into());
                        }
                        self.signins.insert(account.clone(), SignInState::Done { user: r.user, kept: r.kept });
                        // an old failure of this account's test is out of date now
                        self.tests.remove(&account);
                    }
                    Err(e) => {
                        self.signins.insert(account, SignInState::Failed(e));
                    }
                }
                self.dirty = true;
            }
            Event::Probed { account, signed_in } => {
                self.signed_in.insert(account, signed_in);
            }
            Event::Ops { account, result } => {
                self.ops_running.remove(&account);
                match result {
                    Ok(r) => {
                        for f in &r.failed {
                            self.problems.push(f.clone());
                            self.notices.push(f.clone());
                        }
                        self.notices.extend(r.notes.iter().cloned());
                        match &r.offline {
                            Some(o) if o.contains(NO_PASSWORD) => {
                                self.offline.insert(account.clone(), o.clone());
                                self.ask_password(&account, Retry::Ops);
                            }
                            Some(o) => {
                                if crate::mail::oauth::needs_sign_in(o) {
                                    self.signed_in.insert(account.clone(), false);
                                    self.notices.push("A mail account needs you to sign in again: open the account settings.".into());
                                }
                                self.offline.insert(account.clone(), o.clone());
                            }
                            None => {
                                self.offline.remove(&account);
                                // more came in while this ran: send it too
                                if r.remaining > 0 && r.done + r.gone + r.failed.len() > 0 {
                                    self.last_ops = None;
                                }
                            }
                        }
                    }
                    Err(e) => self.problems.push(format!("Mail changes could not be sent: {e}")),
                }
                self.dirty = true;
            }
            Event::Recovered(r) => {
                self.recovered = true;
                match r {
                    Ok(rec) => {
                        if !rec.unsure.is_empty() {
                            self.notices.push(format!("{} mail{} may have been sent when Shale was closed: look in the Outbox before sending again.", rec.unsure.len(), if rec.unsure.len() == 1 { "" } else { "s" }));
                        }
                    }
                    Err(e) => self.problems.push(format!("The check of drafts and outbox failed: {e}")),
                }
                self.local_dirty = true;
                self.refresh_local();
                if !std::mem::replace(&mut self.announced_local, true) && !self.local_drafts.is_empty() {
                    let n = self.local_drafts.len();
                    self.notices.push(format!("{n} unsent draft{} found: open Drafts to continue writing.", if n == 1 { "" } else { "s" }));
                }
            }
            Event::Sent { account, result } => {
                self.send_running.remove(&account);
                self.local_dirty = true;
                match result {
                    Ok(r) => {
                        if !r.sent.is_empty() {
                            self.notices.push(format!("{} mail{} sent.", r.sent.len(), if r.sent.len() == 1 { "" } else { "s" }));
                        }
                        for (_, e) in &r.failed {
                            self.problems.push(format!("A mail was not sent: {e} (see the Outbox)"));
                        }
                        if !r.unsure.is_empty() {
                            self.problems.push("A mail may have been sent: the server did not answer. Look in your Sent folder, then use the Outbox to send it again or discard it.".to_string());
                        }
                        self.notices.extend(r.notes);
                        // the copy for the Sent folder goes to the server like the other waiting actions
                        self.mark_dirty();
                        self.start_ops(cfg, &account);
                    }
                    Err(e) => self.problems.push(format!("Sending failed inside Shale: {e}")),
                }
            }
            Event::ReplyMail { kind, msg, result } => match result {
                Ok(mail) => self.open_answer(cfg, kind, &msg, &mail),
                Err(e) => self.problems.push(format!("The mail could not be read for the answer: {e}")),
            },
            Event::Tested { account, result } => {
                self.tests.insert(
                    account,
                    match result {
                        Ok((n, None)) => TestState::Ok(format!("Connected. The account has {n} folder{}.", if n == 1 { "" } else { "s" })),
                        Ok((n, Some(Ok(())))) => TestState::Ok(format!("Connected. The account has {n} folder{}. Sending works: signed in to the SMTP server.", if n == 1 { "" } else { "s" })),
                        Ok((n, Some(Err(e)))) => TestState::Failed(format!("Reading works ({n} folder{}), but the SMTP server (sending) said: {e}", if n == 1 { "" } else { "s" })),
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
            let signin_needed = self.syncs.get(&id).is_some_and(|r| r.errors.iter().any(|e| crate::mail::oauth::needs_sign_in(e)) || r.finished.as_ref().is_some_and(|s| s.errors.iter().any(|e| crate::mail::oauth::needs_sign_in(e))));
            if signin_needed {
                self.signed_in.insert(id.clone(), false);
                self.notices.push("A mail account needs you to sign in again: open the account settings.".into());
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
        let waiting = r.pending_ops(None).unwrap_or_default();
        self.pending_total = waiting.len() as i64;
        self.pending_accounts = waiting.iter().map(|p| p.account.clone()).collect();
        self.pending_by_account.clear();
        for p in &waiting {
            *self.pending_by_account.entry(p.account.clone()).or_default() += 1;
        }
        self.unsent = r.unsent_copies(false).unwrap_or_default();
        self.unsent_open &= !self.unsent.is_empty();
        // the marked mails and the open one as the index has them now (flags change, mail moves away)
        let ids: Vec<i64> = self.marked.keys().copied().collect();
        for id in ids {
            match r.message_by_id(id) {
                Ok(Some(m)) => {
                    self.marked.insert(id, m);
                }
                Ok(None) => {
                    self.marked.remove(&id);
                }
                Err(_) => {}
            }
        }
        if let Some(o) = self.open.as_mut() {
            match r.message_by_id(o.msg.id) {
                Ok(Some(m)) => o.msg = m,
                Ok(None) => self.open = None,
                Err(_) => {}
            }
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
            if self.results.is_none() {
                // mail that left the folder (moved, deleted) can shorten the list under the cursor
                self.cursor = self.cursor.and_then(|c| if self.total == 0 { None } else { Some(c.min(self.total - 1)) });
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

    /// What is saved of the view for the next start (NAV-045): the folder and the open message;
    /// a place not found yet stays as it is; nothing for the Drafts and the Outbox.
    pub fn saved_place(&self) -> Option<crate::vault_state::SavedMail> {
        if let Some(p) = &self.restore {
            return Some(p.clone());
        }
        let f = self.folder.filter(|_| self.local_view.is_none()).and_then(|id| self.folder_row(id))?;
        let uid = self.open.as_ref().filter(|o| o.msg.folder_id == f.id).map(|o| o.msg.uid);
        Some(crate::vault_state::SavedMail { account: f.account.clone(), folder: f.name.clone(), uid, ..Default::default() })
    }

    /// Go to the place saved by the last run (NAV-045), once the folders are read: its folder, the
    /// message when it is still there. The place is dropped when the account or the folder is gone;
    /// it waits while the account has no folder yet (its first sync is still to come).
    fn restore_place(&mut self, cfg: &Config) {
        let Some(p) = self.restore.clone() else { return };
        if !cfg.mail_accounts.iter().any(|a| a.id == p.account) {
            self.restore = None;
            return;
        }
        if !self.folders.iter().any(|f| f.account == p.account) {
            return;
        }
        self.restore = None;
        let Some(id) = self.folders.iter().find(|f| f.account == p.account && f.name == p.folder).map(|f| f.id) else { return };
        self.select_folder(cfg, id);
        let msg = p.uid.and_then(|uid| self.reader.as_ref().and_then(|r| r.message(id, uid).ok().flatten()));
        if let Some(msg) = msg {
            if let Some(pos) = self.reader.as_ref().and_then(|r| r.position_in_list(id, msg.date, msg.uid).ok()) {
                self.cursor = Some(pos.max(0) as usize);
                self.scroll_to_cursor = true;
            }
            self.open_message(msg);
        }
    }

    /// Show the folder `id` (MAIL-056: a folder that is not synced is listed on demand).
    pub fn select_folder(&mut self, cfg: &Config, id: i64) {
        self.restore = None;
        self.clear_search();
        self.local_view = None;
        self.folder = Some(id);
        self.cursor = None;
        self.marked.clear();
        self.pages.clear();
        self.total = self.reader.as_ref().and_then(|r| r.count_folder(id).ok()).unwrap_or(0).max(0) as usize;
        self.queries += 1;
        self.list_on_demand(cfg, id, false);
    }

    /// The entries of the folder pane in the order they are drawn: per account its folders, then
    /// Drafts and Outbox.
    fn folder_entries(&self, cfg: &Config) -> Vec<FolderEntry> {
        let mut v = Vec::new();
        for a in &cfg.mail_accounts {
            v.extend(self.account_folders(&a.id).into_iter().map(|f| FolderEntry::Folder(f.id)));
            v.push(FolderEntry::Local(LocalView::Drafts(a.id.clone())));
            v.push(FolderEntry::Local(LocalView::Outbox(a.id.clone())));
        }
        v
    }

    /// j / k in the folder pane: select the next or previous entry and show its list.
    pub(crate) fn step_entry(&mut self, cfg: &Config, delta: isize) {
        let entries = self.folder_entries(cfg);
        if entries.is_empty() {
            return;
        }
        let cur = entries.iter().position(|e| match e {
            FolderEntry::Folder(id) => self.local_view.is_none() && self.folder == Some(*id),
            FolderEntry::Local(v) => self.local_view.as_ref() == Some(v),
        });
        let at = match cur {
            Some(c) => (c as isize + delta).clamp(0, entries.len() as isize - 1) as usize,
            None if delta < 0 => entries.len() - 1,
            None => 0,
        };
        if Some(at) == cur {
            return;
        }
        match entries[at].clone() {
            FolderEntry::Folder(id) => self.select_folder(cfg, id),
            FolderEntry::Local(v) => self.select_local(v),
        }
    }

    /// Ctrl+H / Ctrl+L: the keyboard one pane to the left (-1) or right (+1). False when there
    /// is nowhere to go: left of the folder pane (the caller goes on into the sidebar), or the
    /// view does not show its panes (MAIL-063). `sidebar`: the keyboard comes from the left
    /// sidebar, so it enters at the folder pane. The search field of the view is left first.
    pub fn step_pane(&mut self, ctx: &egui::Context, cfg: &Config, delta: isize, sidebar: bool) -> bool {
        if self.compose.is_some() || !matches!(self.store, StoreState::Ready(_)) || cfg.mail_accounts.is_empty() {
            return false;
        }
        let search_id = egui::Id::unique("mail-search");
        let foreign = ctx.memory(|m| m.focused()).is_some_and(|f| f != search_id);
        if let Some(f) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(f));
        }
        let to = match (self.pane, delta) {
            _ if sidebar || foreign => {
                if delta < 0 {
                    return false;
                }
                Pane::Folders
            }
            (Pane::Folders, d) if d < 0 => return false,
            (Pane::Folders, _) => Pane::List,
            (Pane::List, d) if d < 0 => Pane::Folders,
            (Pane::List, _) | (Pane::Message, 1..) => Pane::Message,
            (Pane::Message, _) => Pane::List,
        };
        self.pane = to;
        true
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

    fn lazy_connector(&self, account: &MailAccount, typed: Option<crate::secret::Secret>) -> LazyConnector {
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
        self.cancel_crypto();
        self.att_sel = 0;
        self.read_scroll = 0.0;
        self.open = Some(OpenMail { msg: msg.clone(), body });
        if loading {
            if let Some(store) = self.ready_store() {
                let consent = self.consent;
                self.spawn(move || read_for_pane(&store, &msg, consent).map(Arc::new), move |r| Event::Message { id, result: flatten(r) });
            }
        }
    }

    pub fn close_message(&mut self) {
        self.cancel_crypto();
        self.open = None;
        self.release_att_audio();
    }

    /// The temp files of the audio attachments of the open message, made when it is first shown (MAIL-240): a
    /// message that has changed (another one, or the same one read again with GnuPG's answer) gets new ones.
    pub(crate) fn att_audio_files(&mut self) -> Vec<Option<PathBuf>> {
        let ready = match self.open.as_ref() {
            Some(OpenMail { msg, body: Body::Ready(l) }) => Some((msg.id, l.clone())),
            _ => None,
        };
        let same = matches!((&self.att_audio, &ready), (Some(a), Some((id, l))) if a.msg == *id && Arc::ptr_eq(&a.mail, l));
        if !same {
            self.release_att_audio();
            if let Some((id, l)) = ready.filter(|(_, l)| l.mail.attachments.iter().any(|a| crate::media::attachment_audio_ext(&a.name, &a.mime).is_some())) {
                self.att_audio = Some(AttAudio::make(id, &l));
            }
        }
        self.att_audio.as_ref().map(|a| a.files.clone()).unwrap_or_default()
    }

    /// Stops a sound of the open message that plays and removes the temp files (MAIL-240).
    pub(crate) fn release_att_audio(&mut self) {
        if let Some(a) = self.att_audio.take() {
            // each attachment's player is owned by its own file
            for f in a.files.iter().flatten() {
                self.media.stop_for_note(f);
            }
        }
    }

    /// Ask GnuPG about the open message in a job (MAIL-171, MAIL-173, MAIL-182): signatures, and decryption when the
    /// consent is given. The window never waits for it; a passphrase dialog may open meanwhile.
    fn start_crypto(&mut self, msg: MessageSummary) {
        let Some(store) = self.ready_store() else { return };
        self.cancel_crypto();
        let cancel = Arc::new(AtomicBool::new(false));
        self.crypto_cancel = Some(cancel.clone());
        self.crypto_id = Some(msg.id);
        self.crypto_running = true;
        let (g, consent, id) = (self.gnupg.clone(), self.consent, msg.id);
        self.spawn(move || crypto_job(&store, &g, &msg, consent, &cancel).map(Arc::new), move |r| Event::Crypto { id, result: flatten(r) });
    }

    /// The Cancel button of the status strip, and leaving the message: stop what GnuPG is doing (the child is killed).
    pub fn cancel_crypto(&mut self) {
        if let Some(c) = self.crypto_cancel.take() {
            c.store(true, Ordering::SeqCst);
        }
    }

    /// Find out what GnuPG is and has (a job): the settings show it (MAIL-191).
    pub fn detect_gnupg(&mut self) {
        if self.gnupg_probing {
            return;
        }
        self.gnupg_probing = true;
        let g = self.gnupg.clone();
        self.spawn(move || Ok::<_, String>(probe_gnupg(&g)), |r| Event::Gnupg(flatten(r)));
        self.read_agent();
    }

    /// Read how long the agent keeps a passphrase (a job; MAIL-235).
    fn read_agent(&mut self) {
        if self.agent.reading {
            return;
        }
        self.agent.reading = true;
        let g = self.gnupg.clone();
        self.spawn(move || crate::mail::agent_cache::read(&g, &AtomicBool::new(false)), |r| Event::Agent(flatten(r)));
    }

    /// Write the cache times with gpgconf and read them again (a job; MAIL-236). Only the second click calls this.
    pub fn write_agent(&mut self, choice: crate::mail::agent_cache::Choice) {
        use crate::mail::agent_cache::{apply, read, Plan};
        let Some(Ok(info)) = self.agent.info.as_ref() else { return };
        if self.agent.writing {
            return;
        }
        self.agent.writing = true;
        self.agent.written = None;
        let (g, plan) = (self.gnupg.clone(), Plan::new(&info.homedir, choice));
        self.spawn(
            move || {
                let cancel = AtomicBool::new(false);
                let result = apply(&g, &plan, &cancel);
                (result, read(&g, &cancel))
            },
            |r| match r {
                Ok((result, info)) => Event::AgentWritten { result, info },
                Err(e) => Event::AgentWritten { result: Err(e.clone()), info: Err(e) },
            },
        );
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
        let display = if account.name.is_empty() { account.host.clone() } else { account.name.clone() };
        // the task ends with the job, before its last report is taken in (JOB-021)
        let Some(mut task) = self.busy.begin(MAIL_SYNC.with(id), format!("Syncing {display}…")) else { return };
        let cancel = task.cancellable().flag();
        let connector: Arc<dyn imap_sync::Connector> = Arc::new(self.lazy_connector(&account, None));
        let rx = imap_sync::start(store, account.id.clone(), display, connector, self.opts.clone(), cancel.clone(), task);
        self.syncs.insert(id.to_string(), SyncRun { rx, cancel, folder: String::new(), done: 0, total: 0, added: 0, errors: vec![], finished: None });
    }

    pub fn cancel_syncs(&mut self) {
        for r in self.syncs.values().filter(|r| r.running()) {
            r.cancel.store(true, Ordering::SeqCst);
        }
    }

    /// The dialog's "Test connection": connect, list and record the folders, fetch nothing.
    pub fn test_connection(&mut self, account: &MailAccount, typed: Option<crate::secret::Secret>) {
        let Some(store) = self.ready_store() else {
            self.tests.insert(account.id.clone(), TestState::Failed("The local mail copy is still being opened; try again in a moment.".into()));
            return;
        };
        self.tests.insert(account.id.clone(), TestState::Running);
        // the password (or client secret) typed in the dialog, not saved yet, is tried for sending too (MAIL-121)
        let smtp_session = typed.clone().or_else(|| self.session_pw.get(&account.id).cloned());
        let connector = self.lazy_connector(account, typed);
        let id = account.id.clone();
        let display = if account.name.is_empty() { account.host.clone() } else { account.name.clone() };
        let id2 = id.clone();
        let smtp = (!account.smtp_host.trim().is_empty()).then(|| LazySmtp { factory: self.smtp_factory.clone(), account: account.clone(), session: smtp_session });
        self.spawn(
            move || -> Result<(usize, Option<Result<(), String>>), String> {
                let n = imap_sync::discover_folders(&store, &connector, &id, &display).map(|f| f.len())?;
                // sending: connect and sign in, never send (MAIL-121)
                let s = smtp.map(|c| send::test_connection(&c).map(|_| ()));
                Ok((n, s))
            },
            move |r| Event::Tested { account: id2, result: flatten(r) },
        );
    }

    /// "Sign in in the browser": the whole OAuth2 flow as a job (MAIL-086). `secret` is the
    /// client secret typed in the dialog, if any (else the keyring's is used).
    pub fn start_sign_in(&mut self, account: &MailAccount, secret: Option<crate::secret::Secret>) {
        if matches!(self.signins.get(&account.id), Some(SignInState::Waiting { .. })) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.signins.insert(account.id.clone(), SignInState::Waiting { cancel: cancel.clone(), url: None });
        let (acct, id, id2) = (account.clone(), account.id.clone(), account.id.clone());
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        self.spawn(
            move || {
                let secret = secret.filter(|s| !s.is_empty()).or_else(|| crate::secrets::mail_secret(&acct.id, crate::secrets::MailSecret::ClientSecret));
                let shown = |url: &str| {
                    let _ = tx.send(Event::SignInUrl { account: id.clone(), url: url.to_string() });
                    if let Some(c) = &ctx {
                        c.request_repaint();
                    }
                };
                crate::mail::oauth::sign_in(&acct, secret, &cancel, crate::mail::oauth::SIGN_IN_TIMEOUT, &shown).map_err(|e| e.to_string())
            },
            move |r| Event::SignedIn { account: id2, result: flatten(r) },
        );
    }

    /// Stop waiting for the browser.
    pub fn cancel_sign_in(&mut self, account: &str) {
        if let Some(SignInState::Waiting { cancel, .. }) = self.signins.get(account) {
            cancel.store(true, Ordering::SeqCst);
        }
    }

    /// Find out (in a job: it reads the keyring) whether an OAuth2 account is signed in.
    pub fn probe_sign_in(&mut self, account: &MailAccount) {
        if account.auth != crate::config::MailAuth::OAuth2 || self.signed_in.contains_key(&account.id) {
            return;
        }
        let acct = account.clone();
        let id = account.id.clone();
        self.spawn(move || crate::mail::oauth::is_signed_in(&acct), move |r| Event::Probed { account: id, signed_in: r.unwrap_or(false) });
    }

    /// Keep a typed client secret in the keyring (never in the settings file).
    pub fn save_client_secret(&mut self, account: &str, secret: crate::secret::Secret) {
        let id = account.to_string();
        let id2 = id.clone();
        self.spawn(move || crate::secrets::set_mail_secret(&id, crate::secrets::MailSecret::ClientSecret, secret.expose()), move |r| Event::Secret { account: id2, stored: r.unwrap_or(false) });
    }

    /// A folder's checkbox: sync it or not (MAIL-042).
    pub fn set_folder_sync(&mut self, folder: i64, enabled: bool) -> Result<(), String> {
        let store = self.ready_store().ok_or("the local mail copy is not open yet")?;
        store.commit(vec![crate::mail::db::Op::SyncEnabled { folder, enabled }]).map_err(|e| e.0)?;
        self.dirty = true;
        self.refresh();
        Ok(())
    }

    /// Save a typed password: into the keyring if there is one, and meanwhile (or instead) in
    /// memory for this session. Never into the settings file (MAIL-041).
    pub fn save_password(&mut self, account: &str, password: crate::secret::Secret) {
        self.session_pw.insert(account.to_string(), password.clone());
        let id = account.to_string();
        let id2 = id.clone();
        self.spawn(move || crate::secrets::set_mail_secret(&id, crate::secrets::MailSecret::Password, password.expose()), move |r| Event::Secret { account: id2, stored: r.unwrap_or(false) });
    }

    /// An account was removed: its secrets go, its syncs stop; its mail files stay (MAIL-043).
    pub fn forget_account(&mut self, account: &str) {
        self.session_pw.remove(account);
        self.signins.remove(account);
        self.signed_in.remove(account);
        crate::mail::oauth::forget_source(account);
        if let Some(r) = self.syncs.remove(account) {
            r.cancel.store(true, Ordering::SeqCst);
        }
        self.watch.stop_account(account);
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
                crate::secrets::clear_mail_secret(&id, crate::secrets::MailSecret::ClientSecret);
            },
            |_| {},
        );
    }

    /// Ask for the password of an account (once at a time).
    pub fn ask_password(&mut self, account: &str, retry: Retry) {
        if self.prompt.is_none() && !account.is_empty() {
            self.prompt = Some(PasswordPrompt { account: account.to_string(), text: Default::default(), focus: true, retry });
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
            Retry::Ops => self.start_ops(cfg, &p.account),
        }
    }

    // ------------------------------------------------------------ flags, move, delete (ADR-0009)

    /// The messages an action works on: the marked ones, else the open one, else the one under
    /// the cursor (the same rule as "Import to vault").
    pub fn targets(&mut self) -> Vec<MessageSummary> {
        if !self.marked.is_empty() {
            return self.marked.values().cloned().collect();
        }
        if let Some(o) = &self.open {
            return vec![o.msg.clone()];
        }
        self.cursor_message().into_iter().collect()
    }

    /// Record an action for these messages and send it (MAIL-107). The change shows at once.
    /// Returns how many were recorded.
    pub fn apply(&mut self, cfg: &Config, msgs: Vec<MessageSummary>, action: Action) -> usize {
        let Some(store) = self.ready_store() else { return 0 };
        match ops::enqueue(&store, &msgs, &action, crate::calendar::time::now()) {
            Ok(q) => {
                self.notices.extend(q.notes);
                self.dirty = true;
                self.refresh();
                let mut accounts: Vec<String> = msgs.iter().map(|m| m.account.clone()).collect();
                accounts.sort();
                accounts.dedup();
                if q.queued > 0 {
                    for a in accounts {
                        self.start_ops(cfg, &a);
                    }
                }
                q.queued
            }
            Err(e) => {
                self.problems.push(e.clone());
                self.notices.push(e);
                0
            }
        }
    }

    /// `u`: mark the messages read, or, when they all are, unread.
    pub fn toggle_read(&mut self, cfg: &Config) {
        let t = self.targets();
        self.toggle_read_on(cfg, t);
    }

    pub fn toggle_read_on(&mut self, cfg: &Config, t: Vec<MessageSummary>) {
        if t.is_empty() {
            return;
        }
        let all_read = t.iter().all(|m| m.is_seen());
        let a = if all_read { Action::SetFlags { add: vec![], remove: vec![MailFlag::Seen] } } else { Action::SetFlags { add: vec![MailFlag::Seen], remove: vec![] } };
        self.read_timer = None; // a person who decides has no need for the timer
        self.apply(cfg, t, a);
    }

    /// `s` / `f`: flag the messages, or, when they all are, unflag.
    pub fn toggle_flag(&mut self, cfg: &Config) {
        let t = self.targets();
        self.toggle_flag_on(cfg, t);
    }

    pub fn toggle_flag_on(&mut self, cfg: &Config, t: Vec<MessageSummary>) {
        if t.is_empty() {
            return;
        }
        let all = t.iter().all(|m| m.is_flagged());
        let a = if all { Action::SetFlags { add: vec![], remove: vec![MailFlag::Flagged] } } else { Action::SetFlags { add: vec![MailFlag::Flagged], remove: vec![] } };
        self.apply(cfg, t, a);
    }

    pub fn in_trash(&self, m: &MessageSummary) -> bool {
        self.folder_row(m.folder_id).is_some_and(|f| imap_sync::kind_of(f) == FolderKind::Trash)
    }

    /// `d` / Delete: move to Trash; in Trash, ask whether to delete forever.
    pub fn delete_selected(&mut self, cfg: &Config) {
        let t = self.targets();
        self.delete_on(cfg, t);
    }

    pub fn delete_on(&mut self, cfg: &Config, t: Vec<MessageSummary>) {
        if t.is_empty() {
            return;
        }
        let (bin, rest): (Vec<_>, Vec<_>) = t.into_iter().partition(|m| self.in_trash(m));
        if rest.is_empty() {
            self.forever = Some(ForeverAsk { msgs: bin });
            return;
        }
        if self.apply(cfg, rest, Action::Delete) > 0 {
            self.after_leaving();
        }
    }

    /// The confirmed "Delete forever" (the dialog's button).
    pub fn confirm_forever(&mut self, cfg: &Config) {
        let Some(ask) = self.forever.take() else { return };
        let n = ask.msgs.len();
        if self.apply(cfg, ask.msgs, Action::DeleteForever(Confirmed::after_dialog(n))) > 0 {
            self.after_leaving();
        }
    }

    /// Shift+D: ask whether to delete the local copy of the mails that are deleted on the server (MAIL-238).
    pub fn delete_local_selected(&mut self) {
        let t = self.targets();
        self.delete_local_on(t);
    }

    pub fn delete_local_on(&mut self, t: Vec<MessageSummary>) {
        let (ok, rest): (Vec<_>, Vec<_>) = t.into_iter().partition(ops::local_deletable);
        if ok.is_empty() {
            if !rest.is_empty() {
                self.notices.push("Only the copy of a mail that was deleted on the server can be deleted here; a mail that is still on the server would come back with the next sync.".into());
            }
            return;
        }
        if !rest.is_empty() {
            self.notices.push(format!("{} selected mail{} still on the server: only the others are offered.", rest.len(), if rest.len() == 1 { " is" } else { "s are" }));
        }
        self.local_delete = Some(LocalDeleteAsk { msgs: ok });
    }

    /// The confirmed "Delete local copy" (the dialog's button): no server is involved.
    pub fn confirm_local_delete(&mut self) {
        let Some(ask) = self.local_delete.take() else { return };
        let Some(store) = self.ready_store() else { return };
        let n = ask.msgs.len();
        match ops::delete_local_copies(&store, &ask.msgs, Confirmed::after_dialog(n), &crate::mail::store::today_string()) {
            Ok(done) => {
                self.notices.extend(done.notes);
                if done.removed > 0 {
                    self.notices.push(format!("The local copy of {} mail{} was deleted (the files are in the local trash).", done.removed, if done.removed == 1 { "" } else { "s" }));
                }
                self.dirty = true;
                self.refresh();
                self.marked.clear();
                if self.open.as_ref().is_some_and(|o| ask.msgs.iter().any(|m| m.id == o.msg.id)) {
                    self.close_message();
                }
            }
            Err(e) => {
                self.problems.push(e.clone());
                self.notices.push(e);
            }
        }
    }

    /// After mail was sent away (moved, deleted): nothing is marked any more, the reading pane
    /// does not show mail that is leaving, and the cursor goes on to the next one.
    fn after_leaving(&mut self) {
        self.marked.clear();
        if self.open.as_ref().is_some_and(|o| o.msg.pending.is_some()) {
            self.open = None;
        }
        if self.cursor.is_some() && self.results.is_none() {
            self.move_cursor(1);
        }
    }

    /// `m`: the folder picker for the messages.
    pub fn open_mover(&mut self) {
        let t = self.targets();
        self.open_mover_on(t);
    }

    pub fn open_mover_on(&mut self, t: Vec<MessageSummary>) {
        if t.is_empty() {
            return;
        }
        let account = t[0].account.clone();
        if t.iter().any(|m| m.account != account) {
            self.problems.push("Choose mail of one account to move it: folders belong to an account.".into());
            return;
        }
        self.mover = Some(MovePicker { msgs: t, query: String::new(), cursor: 0, focus: true });
    }

    /// The folders the picker offers for what has been typed: the account's folders in the order of
    /// the folder list, without the folder all the messages are in already.
    pub fn mover_candidates(&self) -> Vec<FolderRow> {
        let Some(p) = &self.mover else { return vec![] };
        let account = p.msgs[0].account.clone();
        let same = p.msgs.iter().all(|m| m.folder_id == p.msgs[0].folder_id);
        let q = p.query.trim().to_lowercase();
        self.account_folders(&account).into_iter().filter(|f| !(same && f.id == p.msgs[0].folder_id)).filter(|f| q.is_empty() || decode_mutf7(&f.name).to_lowercase().contains(&q)).collect()
    }

    /// Move the picker's messages to this folder.
    pub fn move_to(&mut self, cfg: &Config, folder: &FolderRow) {
        let Some(p) = self.mover.take() else { return };
        if self.apply(cfg, p.msgs, Action::MoveTo { folder: folder.name.clone() }) > 0 {
            self.after_leaving();
        }
    }

    /// Send what waits for the server, as a job (one at a time per account).
    pub fn start_ops(&mut self, cfg: &Config, account: &str) {
        if self.ops_running.contains(account) {
            return;
        }
        let (Some(store), Some(acct)) = (self.ready_store(), cfg.mail_accounts.iter().find(|a| a.id == account).cloned()) else { return };
        self.ops_running.insert(account.to_string());
        let connector = self.lazy_connector(&acct, None);
        let id = account.to_string();
        let id2 = id.clone();
        self.spawn(move || ops::run_pending(&store, &connector, &id), move |r| Event::Ops { account: id2, result: r });
    }

    /// Every frame: the open message becomes read after the delay, and what waits for the server
    /// is sent again from time to time (the network may be back).
    fn tick_ops(&mut self, cfg: &Config) {
        if let Some((id, t0)) = self.read_timer {
            let msg = self.open.as_ref().filter(|o| o.msg.id == id && !o.msg.is_seen() && !o.msg.gone_on_server && o.msg.pending.is_none()).map(|o| o.msg.clone());
            match msg {
                Some(m) if cfg.mail_mark_read => {
                    if t0.elapsed() >= Duration::from_secs(u64::from(cfg.mail_mark_read_secs)) {
                        self.read_timer = None;
                        self.apply(cfg, vec![m], Action::SetFlags { add: vec![MailFlag::Seen], remove: vec![] });
                    }
                }
                _ => self.read_timer = None,
            }
        }
        if self.pending_total > 0 && self.ready_store().is_some() && self.last_ops.is_none_or(|t| t.elapsed() >= self.ops_retry) {
            self.last_ops = Some(Instant::now());
            for a in self.pending_accounts.clone() {
                self.start_ops(cfg, &a);
            }
        }
    }

    /// Is a job sending actions to the server?
    #[cfg(test)]
    pub fn busy_with_ops(&self) -> bool {
        !self.ops_running.is_empty()
    }

    /// Waiting actions of configured accounts, and of account ids that are not configured (any more); the second
    /// kind is never sent (MAIL-207).
    pub fn pending_split(&self, cfg: &Config) -> (i64, i64) {
        let (mut live, mut orphaned) = (0, 0);
        for (a, n) in &self.pending_by_account {
            if cfg.mail_accounts.iter().any(|x| &x.id == a) {
                live += n;
            } else {
                orphaned += n;
            }
        }
        (live, orphaned)
    }

    /// "3 changes are waiting for the server" (and why), for the bars; only actions that will be sent.
    pub fn pending_text(&self, cfg: &Config) -> Option<String> {
        let n = self.pending_split(cfg).0;
        if n == 0 {
            return None;
        }
        Some(format!("{n} change{} waiting for the server", if n == 1 { " is" } else { "s are" }))
    }

    /// "2 queued changes belong to removed accounts (not sent)" and the hover text that says why (MAIL-208).
    pub fn orphaned_text(&self, cfg: &Config) -> Option<(String, &'static str)> {
        let n = self.pending_split(cfg).1;
        if n == 0 {
            return None;
        }
        let t = format!("{n} queued change{} to removed accounts (not sent)", if n == 1 { " belongs" } else { "s belong" });
        Some((t, "These changes were queued for an account that is no longer set up (removed, or its server or user was changed, which makes it a new account). They are kept, never sent, and not lost."))
    }

    // ------------------------------------------------------------ phase 3: writing and sending (ADR-0010)

    /// The check of drafts and outbox at start-up: a mail found in `Sending` becomes `Unsure`, a cut-off undo or
    /// queuing is finished. Nothing is sent before it is done.
    fn start_recovery(&mut self) {
        let ob = Arc::new(Outbox::new(&self.root));
        self.outbox = Some(ob.clone());
        self.recovered = false;
        self.spawn(move || ob.recover(crate::calendar::time::now(), &crate::mail::store::today_string()), |r| Event::Recovered(flatten(r)));
    }

    /// Read the lists of drafts and outbox entries from disk (small files; done when something changed).
    pub fn refresh_local(&mut self) {
        self.local_dirty = false;
        self.last_local = Some(Instant::now());
        if let Some(ob) = &self.outbox {
            self.local_drafts = ob.list_drafts(None);
            self.local_items = ob.list(None);
        }
        if let Some((a, i)) = self.outbox_sel.clone() {
            if !self.local_items.iter().any(|x| x.account == a && x.id == i) {
                self.outbox_sel = None;
            }
        }
    }

    /// Every frame: the lists, the autosave of the draft, and sending what is due.
    fn tick_local(&mut self, cfg: &Config) {
        for a in &cfg.mail_accounts {
            if !self.account_names.contains_key(&a.id) {
                self.account_names.insert(a.id.clone(), if a.name.is_empty() { a.host.clone() } else { a.name.clone() });
            }
        }
        if self.outbox.is_none() {
            return;
        }
        let busy_lists = self.local_items.iter().any(|i| matches!(i.state, State::Queued | State::Sending));
        if self.local_dirty || (busy_lists && self.last_local.is_none_or(|t| t.elapsed() >= Duration::from_millis(500))) {
            self.refresh_local();
        }
        // the draft is saved after a pause in typing (MAIL-130)
        if let Some(c) = &self.compose {
            if c.unsaved() && c.ask.is_none() && c.last_edit.is_some_and(|t| t.elapsed() >= crate::mail::compose_ui::AUTOSAVE_AFTER) {
                self.compose_save();
            }
        }
        if !self.recovered {
            return;
        }
        let now = crate::calendar::time::now();
        for a in &cfg.mail_accounts {
            let due = self.local_items.iter().any(|i| i.account == a.id && ((i.state == State::Queued && i.send_after <= now) || (i.state == State::Sent && !i.copy_done)));
            if due {
                self.start_send(a);
            }
        }
    }

    /// Send the due mails of an account as a job (one at a time per account).
    fn start_send(&mut self, account: &MailAccount) {
        if self.send_running.contains(&account.id) {
            return;
        }
        let (Some(store), Some(ob)) = (self.ready_store(), self.outbox.clone()) else { return };
        self.send_running.insert(account.id.clone());
        let connector = LazySmtp { factory: self.smtp_factory.clone(), account: account.clone(), session: self.session_pw.get(&account.id).cloned() };
        let acct = account.clone();
        let id = account.id.clone();
        self.spawn(move || send::send_due(&ob, &store, &acct, &connector, crate::calendar::time::now()), move |r| Event::Sent { account: id, result: r });
    }

    /// The addresses to complete from: the people notes of the vault, then the addresses in the mail index (newest first).
    fn collect_contacts(&self) -> Vec<(String, String)> {
        let mut out: Vec<(String, String)> = vec![];
        let mut seen: HashSet<String> = HashSet::new();
        let mut add = |n: &str, a: &str| {
            let key = a.trim().to_lowercase();
            if key.contains('@') && seen.insert(key.clone()) && out.len() < 3000 {
                out.push((n.trim().to_string(), a.trim().to_string()));
            }
        };
        if let Some(v) = &self.vault {
            for (n, a) in &v.people {
                add(n, a);
            }
        }
        if let Some(r) = &self.reader {
            if let Ok(mut st) = r.conn().prepare("SELECT from_addr, to_addr, cc_addr FROM messages ORDER BY date DESC LIMIT 3000") {
                if let Ok(rows) = st.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))) {
                    for (f, t, c) in rows.flatten() {
                        for list in [f, t, c] {
                            for part in compose::split_list(&list) {
                                if let Some((n, a)) = compose::name_and_address(&part) {
                                    add(&n, &a);
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }

    fn open_compose(&mut self, mut c: Compose, focus: Field) -> bool {
        if self.compose.is_some() {
            self.notices.push("Finish or close the mail you are writing first.".into());
            return false;
        }
        c.contacts = self.collect_contacts();
        c.focus = Some(focus);
        self.compose = Some(c);
        true
    }

    fn account_for_new(&self, cfg: &Config) -> Option<MailAccount> {
        let from_folder = self.folder.and_then(|f| self.folder_row(f)).map(|f| f.account.clone());
        from_folder.and_then(|id| cfg.mail_accounts.iter().find(|a| a.id == id)).or(cfg.mail_accounts.first()).cloned()
    }

    /// `c`: a new mail from the account of the open folder (MAIL-150).
    pub fn compose_new(&mut self, cfg: &Config) {
        let Some(a) = self.account_for_new(cfg) else {
            self.notices.push("Add a mail account first.".into());
            return;
        };
        self.open_compose(Compose::new(send::new_draft(&a)), Field::To);
    }

    /// `r`, `Shift+R`, `Shift+F`: answer or forward the open message, or the one under the cursor.
    pub fn start_reply(&mut self, kind: Kind) {
        let msg = self.open.as_ref().map(|o| o.msg.clone()).or_else(|| self.cursor_message());
        let (Some(msg), Some(store)) = (msg, self.ready_store()) else { return };
        if self.compose.is_some() {
            self.notices.push("Finish or close the mail you are writing first.".into());
            return;
        }
        if msg.state != STATE_FULL || msg.path.is_empty() {
            self.notices.push("Load the message first: it was not downloaded.".into());
            return;
        }
        let m = Box::new(msg.clone());
        self.spawn(move || store.read_mail(&msg).map(Box::new).map_err(|e| e.0), move |r| Event::ReplyMail { kind, msg: m, result: flatten(r) });
    }

    fn open_answer(&mut self, cfg: &Config, kind: Kind, msg: &MessageSummary, mail: &Mail) {
        let Some(a) = cfg.mail_accounts.iter().find(|a| a.id == msg.account) else { return };
        let (name, addr) = send::sender_of(a);
        let d = send::with_crypto_defaults(compose::answer(kind, mail, &a.id, &name, &addr, &a.signature), a);
        self.open_compose(Compose::new(d), if kind == Kind::Forward { Field::To } else { Field::Body });
    }

    /// Open a saved draft in a compose window.
    pub fn open_draft(&mut self, account: &str, id: &str) {
        let Some(ob) = self.outbox.clone() else { return };
        match ob.load_draft(account, id) {
            Ok((d, problems)) => {
                let alerts = ob.att_alerts("drafts", account, id, &d.attachments);
                let mut c = Compose::from_saved(d, crate::calendar::time::now());
                // an attachment with an alert is told by the alert, not twice
                c.notes = problems.into_iter().filter(|p| !alerts.iter().any(|a| p.contains(&format!("“{}”", a.name)))).collect();
                c.att_alerts = alerts.into_iter().map(|alert| crate::mail::compose_ui::AttAlertState { alert, dismissed: false }).collect();
                self.open_compose(c, Field::Body);
            }
            Err(e) => self.problems.push(format!("The draft could not be opened: {e}")),
        }
    }

    /// Save the draft now. Returns false if that failed (the window stays open and says so).
    pub fn compose_save(&mut self) -> bool {
        let (Some(ob), Some(c)) = (self.outbox.clone(), self.compose.as_mut()) else { return false };
        if !c.has_content() && c.saved_rev.is_none() {
            return true; // nothing typed that anyone would miss: no draft file
        }
        let now = crate::calendar::time::now();
        match ob.save_draft(&c.draft, now) {
            Ok(()) => {
                // the sender changed: the draft is in the new account's folder, the old files are put away after that
                if let Some(old) = c.saved_account.clone().filter(|o| o != &c.draft.account) {
                    let _ = ob.discard_draft(&old, &c.draft.id, &crate::mail::store::today_string());
                }
                c.saved_account = Some(c.draft.account.clone());
                c.saved_rev = Some(c.rev);
                c.saved_at = Some(now);
                self.local_dirty = true;
                true
            }
            Err(e) => {
                c.error = Some(format!("The draft could not be saved: {e}"));
                false
            }
        }
    }

    /// Save the open draft now if it has unsaved content, whatever dialog is open (the window is closing, MAIL-201).
    pub fn flush_draft(&mut self) -> Result<(), String> {
        match self.compose.as_ref() {
            Some(c) if c.unsaved() && c.has_content() => {}
            _ => return Ok(()),
        }
        if self.outbox.is_none() {
            return Err("the draft folder is not open".into());
        }
        if self.compose_save() {
            Ok(())
        } else {
            Err(self.compose.as_ref().and_then(|c| c.error.clone()).unwrap_or_else(|| "The draft could not be saved.".into()))
        }
    }

    /// The text of the open draft, for the recovery folder when it could not be saved: its file name and content.
    pub fn draft_rescue_text(&self) -> Option<(PathBuf, String)> {
        let c = self.compose.as_ref().filter(|c| c.has_content())?;
        let d = &c.draft;
        Some((PathBuf::from("mail-drafts").join(format!("{}.txt", d.id)), format!("To: {}\nCc: {}\nSubject: {}\n\n{}\n", d.to, d.cc, d.subject, d.body)))
    }

    /// Esc or Close: with changes the person is asked (MAIL-131).
    pub fn compose_close(&mut self) {
        let Some(c) = self.compose.as_mut() else { return };
        if let Some(cancel) = &c.crypting {
            // Esc while GnuPG works: stop it; the window stays
            cancel.store(true, Ordering::SeqCst);
            return;
        }
        if c.ask.is_some() {
            return;
        }
        if c.rev > 0 && (c.has_content() || c.saved_rev.is_some()) {
            c.ask = Some(Ask::Close);
        } else {
            self.compose = None;
            self.local_dirty = true;
        }
    }

    fn compose_keep(&mut self) {
        if self.compose_save() {
            self.compose = None;
            self.local_dirty = true;
        } else if let Some(c) = self.compose.as_mut() {
            c.ask = None;
        }
    }

    fn compose_discard(&mut self) {
        // even a discarded draft goes through the trash folder, never into nothing: it is saved first if it was not
        if self.compose.as_ref().is_some_and(|c| c.unsaved() && c.has_content()) {
            self.compose_save();
        }
        let (Some(ob), Some(c)) = (self.outbox.clone(), self.compose.as_ref()) else { return };
        let date = crate::mail::store::today_string();
        // every account folder the draft may be in
        let mut failed = None;
        for acct in [c.saved_account.clone(), Some(c.draft.account.clone())].into_iter().flatten().collect::<HashSet<_>>() {
            if let Err(e) = ob.discard_draft(&acct, &c.draft.id, &date) {
                failed = Some(e);
            }
        }
        match failed {
            None => {
                self.compose = None;
                self.local_dirty = true;
            }
            Some(e) => {
                if let Some(c) = self.compose.as_mut() {
                    c.error = Some(format!("The draft could not be moved to the trash folder: {e}"));
                    c.ask = None;
                }
            }
        }
    }

    /// Send pressed (MAIL-132, MAIL-135): check it, ask what has to be asked, then queue it.
    pub fn compose_send(&mut self, cfg: &Config) {
        let Some(c) = self.compose.as_mut() else { return };
        let Some(a) = cfg.mail_accounts.iter().find(|a| a.id == c.draft.account) else {
            c.error = Some("The account of this mail does not exist any more.".into());
            return;
        };
        if a.smtp_host.trim().is_empty() {
            c.error = Some("This account has no SMTP server yet: set it in the account settings (Sending).".into());
            return;
        }
        // never send without an attachment the draft has (MAIL-218), hidden alert or not
        if let Some(al) = c.att_alerts.first() {
            c.error = Some(format!("Not sent. {} Restore the old file or remove the attachment first.", al.alert.message()));
            return;
        }
        if (c.draft.sign || c.draft.encrypt) && a.key_for(c.draft.standard).is_empty() {
            c.error = Some(format!("No {} key is chosen for this account yet: choose one in the account settings (Encryption), or switch Sign and Encrypt off.", c.draft.standard.label()));
            return;
        }
        if c.draft.encrypt && !self.consent {
            c.error = Some("Encrypting needs encryption to be switched on in the settings first: the copy in Sent is stored decrypted on this computer so that you can read it later.".into());
            return;
        }
        // build it once to find out what is wrong with it
        if let Err(e) = compose::finalize_plain(&c.draft, &compose::BuildOptions { now: crate::calendar::time::now(), user_agent: a.user_agent_header, ..Default::default() }) {
            c.error = Some(e.0);
            return;
        }
        let questions = compose::confirmations(&c.draft);
        if questions.is_empty() {
            self.compose_queue(cfg);
        } else {
            c.ask = Some(Ask::Confirm(questions));
        }
    }

    fn compose_queue(&mut self, cfg: &Config) {
        let (Some(ob), Some(c)) = (self.outbox.clone(), self.compose.as_mut()) else { return };
        let Some(a) = cfg.mail_accounts.iter().find(|a| a.id == c.draft.account) else { return };
        c.ask = None;
        let now = crate::calendar::time::now();
        if c.draft.sign || c.draft.encrypt {
            // signing and encrypting are GnuPG's, and may wait for a passphrase dialog: a job, the window stays as it is
            let cancel = Arc::new(AtomicBool::new(false));
            c.crypting = Some(cancel.clone());
            c.error = None;
            let (g, a, draft, undo, id) = (self.gnupg.clone(), a.clone(), c.draft.clone(), cfg.mail_undo_secs, c.draft.id.clone());
            let draft_id = id.clone();
            self.spawn(move || send::queue_with(&ob, &g, &a, &draft, send::SendTiming { undo_secs: undo, now, date: &crate::mail::store::today_string() }, &cancel).map(Box::new), move |r| Event::Queued { draft_id, result: flatten(r) });
            return;
        }
        match send::queue_draft(&ob, a, &c.draft, cfg.mail_undo_secs, now, &crate::mail::store::today_string()) {
            Ok(item) => self.finish_queued(item),
            Err(e) => c.error = Some(format!("The mail could not be put in the outbox: {e}")),
        }
    }

    /// The mail is in the outbox: the compose window is done.
    fn finish_queued(&mut self, item: Item) {
        if let (Some(ob), Some(c)) = (self.outbox.clone(), self.compose.as_ref()) {
            if let Some(old) = c.saved_account.clone().filter(|o| o != &item.account) {
                let _ = ob.discard_draft(&old, &item.id, &crate::mail::store::today_string());
            }
        }
        self.compose = None;
        self.refresh_local();
    }

    /// Looking up the keys of the recipients of a mail that is to be encrypted, in the local keyrings: a short pause after the
    /// last change, then a job (MAIL-192).
    fn tick_compose_keys(&mut self) {
        let Some(c) = self.compose.as_mut() else { return };
        if !c.draft.encrypt {
            c.keys_wanted = None;
            return;
        }
        let sig = c.keys_signature();
        if c.keys_for.as_ref() == Some(&sig) || c.keys_running {
            return;
        }
        match &c.keys_wanted {
            Some((s, since)) if *s == sig => {
                if since.elapsed() < Duration::from_millis(400) {
                    return;
                }
            }
            _ => {
                c.keys_wanted = Some((sig, Instant::now()));
                return;
            }
        }
        c.keys_wanted = None;
        c.keys_running = true;
        let (g, std) = (self.gnupg.clone(), c.draft.standard);
        let addresses: Vec<String> = crypto::recipients_of(&c.draft).into_iter().map(|a| a.0).collect();
        self.spawn(move || Ok::<_, String>(crypto::lookup_keys(&g, std, &addresses, &AtomicBool::new(false))), move |r| Event::Keys { sig, result: flatten(r) });
    }

    /// What the compose view asked for in this frame.
    pub fn apply_compose_out(&mut self, cfg: &Config, out: ComposeOut) {
        if out.cancel_ask {
            if let Some(c) = self.compose.as_mut() {
                c.ask = None;
            }
        }
        if out.cancel_crypt {
            if let Some(cancel) = self.compose.as_ref().and_then(|c| c.crypting.clone()) {
                cancel.store(true, Ordering::SeqCst);
            }
        }
        if let Some(id) = out.account {
            if let (Some(c), Some(a)) = (self.compose.as_mut(), cfg.mail_accounts.iter().find(|a| a.id == id)) {
                let (name, addr) = send::sender_of(a);
                (c.draft.account, c.draft.from_name, c.draft.from_addr) = (id, name, addr);
                if a.key_for(c.draft.standard).is_empty() {
                    // the other account has no key for this standard: back to what it has
                    let d = send::with_crypto_defaults(c.draft.clone(), a);
                    (c.draft.standard, c.draft.sign, c.draft.encrypt) = (d.standard, d.sign, false);
                }
                c.touch();
            }
        }
        if let Some(i) = out.remove_att {
            if let Some(c) = self.compose.as_mut() {
                if i < c.draft.attachments.len() {
                    let gone = c.draft.attachments.remove(i);
                    c.att_alerts.retain(|a| a.alert.file != gone.file);
                    c.touch();
                }
            }
        }
        if let Some(file) = out.att_remove_missing {
            if let Some(c) = self.compose.as_mut() {
                c.draft.attachments.retain(|a| a.file != file);
                c.att_alerts.retain(|a| a.alert.file != file);
                c.touch();
            }
        }
        if let Some(file) = out.att_dismiss {
            if let Some(c) = self.compose.as_mut() {
                c.att_alerts.iter_mut().filter(|a| a.alert.file == file).for_each(|a| a.dismissed = true);
            }
        }
        if let Some(file) = out.att_restore {
            if let (Some(ob), Some(c)) = (self.outbox.clone(), self.compose.as_mut()) {
                match ob.restore_att("drafts", &c.draft.account, &c.draft.id, &file) {
                    Ok(bytes) => {
                        if let Some(a) = c.draft.attachments.iter_mut().find(|a| a.file == file) {
                            a.data = Arc::new(bytes);
                        }
                        c.att_alerts.retain(|a| a.alert.file != file);
                    }
                    Err(e) => c.error = Some(format!("The old file could not be restored: {e}")),
                }
            }
        }
        if out.attach_path {
            let path = self.compose.as_ref().map(|c| c.path.trim().to_string()).unwrap_or_default();
            let r = read_for_attachment(std::path::Path::new(&path));
            if let Some(c) = self.compose.as_mut() {
                match r.and_then(|a| c.add_attachment(a)) {
                    Ok(()) => c.path.clear(),
                    Err(e) => c.error = Some(e),
                }
            }
        }
        if let Some(rel) = out.attach_vault {
            let note = self.vault.as_ref().is_some_and(|v| v.files.iter().any(|(f, n)| f == &rel && *n));
            let r = self
                .vault
                .as_ref()
                .map(|v| read_for_attachment(&v.root.join(&rel)).map(|a| if note { compose::att_from_note(a.name.trim_end_matches(".md"), &String::from_utf8_lossy(&a.data)) } else { a }))
                .unwrap_or_else(|| Err("No vault is open.".into()));
            if let Some(c) = self.compose.as_mut() {
                match r.and_then(|a| c.add_attachment(a)) {
                    Ok(()) => c.picker = None,
                    Err(e) => c.error = Some(e),
                }
            }
        }
        if out.save {
            self.compose_save();
        }
        if out.confirm_yes {
            let more = self.compose.as_mut().and_then(|c| match &mut c.ask {
                Some(Ask::Confirm(q)) if q.len() > 1 => {
                    q.remove(0);
                    Some(())
                }
                _ => None,
            });
            if more.is_none() {
                self.compose_queue(cfg);
            }
        }
        if out.send {
            self.compose_send(cfg);
        }
        if out.keep_and_close {
            self.compose_keep();
        }
        if out.discard_and_close {
            self.compose_discard();
        }
        if out.close {
            self.compose_close();
        }
    }

    /// Actions of the drafts and outbox lists.
    pub fn apply_local_out(&mut self, out: LocalOut) {
        let Some(ob) = self.outbox.clone() else { return };
        let now = crate::calendar::time::now();
        let date = crate::mail::store::today_string();
        if let Some(k) = out.select {
            self.outbox_sel = Some(k);
        }
        if let Some((a, i)) = out.open_draft {
            self.open_draft(&a, &i);
        }
        if let Some((a, i)) = out.discard_draft {
            if let Err(e) = ob.discard_draft(&a, &i, &date) {
                self.problems.push(format!("The draft could not be discarded: {e}"));
            }
        }
        let mut back_to_editor: Option<Result<crate::mail::compose::Draft, String>> = None;
        if let Some((a, i)) = out.undo {
            back_to_editor = Some(ob.undo(&a, &i, now, &date));
        }
        if let Some((a, i)) = out.edit {
            back_to_editor = Some(ob.edit(&a, &i, now, &date));
        }
        if let Some((a, i)) = out.retry {
            if let Err(e) = ob.retry(&a, &i, now) {
                self.problems.push(format!("Retry: {e}"));
            }
        }
        if let Some((a, i)) = out.send_again {
            if let Err(e) = ob.send_again(&a, &i, now) {
                self.problems.push(format!("Send again: {e}"));
            }
        }
        if let Some((a, i)) = out.discard {
            if let Err(e) = ob.discard(&a, &i, &date) {
                self.problems.push(format!("Discard: {e}"));
            }
        }
        match back_to_editor {
            Some(Ok(d)) => {
                let (acct, id) = (d.account.clone(), d.id.clone());
                if self.compose.is_some() {
                    self.notices.push("The mail was taken back. It is in Drafts.".into());
                } else {
                    self.open_draft(&acct, &id);
                }
            }
            Some(Err(e)) => self.problems.push(format!("The mail could not be taken back: {e}")),
            None => {}
        }
        self.refresh_local();
    }

    /// Undo send for the newest waiting mail (the strip's button).
    pub fn undo_newest(&mut self) {
        if let Some((a, i, _)) = crate::mail::compose_ui::queued_text(&self.local_items, crate::calendar::time::now()) {
            self.apply_local_out(LocalOut { undo: Some((a, i)), ..Default::default() });
        }
    }

    /// Tests: the start-up check of drafts and outbox is done.
    #[cfg(test)]
    pub fn outbox_ready(&self) -> bool {
        self.recovered
    }

    /// Open Drafts or Outbox of an account in the middle pane.
    pub fn select_local(&mut self, view: LocalView) {
        self.clear_search();
        self.local_view = Some(view);
        self.cursor = None;
        self.marked.clear();
        self.open = None;
        self.refresh_local();
    }

    /// Entries of the outbox that are not finished (the number in the folder list).
    pub fn outbox_count(&self, account: &str) -> usize {
        self.local_items.iter().filter(|i| i.account == account && i.state != State::Sent).count()
    }
}

/// A file on disk as an attachment (MAIL-127).
fn read_for_attachment(path: &std::path::Path) -> Result<compose::Att, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() as usize > compose::MAX_ATTACHMENTS_TOTAL {
        return Err(format!("{} is bigger than {} MB", path.display(), compose::MAX_ATTACHMENTS_TOTAL >> 20));
    }
    let data = crate::fsutil::read(path).map_err(|e| e.to_string())?;
    Ok(compose::att_from_file(&path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "attachment".into()), data))
}

/// Read a message for the reading pane (a job). A message that was decrypted before comes from its decrypted copy, with
/// the result found then. Anything else that has a signature or is encrypted starts out as "checking" and is
/// handed to GnuPG afterwards; an encrypted mail without the consent never is (MAIL-181).
fn read_for_pane(store: &MailStore, msg: &MessageSummary, consent: bool) -> Result<Loaded, String> {
    let raw = store.read_raw(msg).map_err(|e| e.0)?;
    let structure = crypto::classify(&raw);
    if let Some((bytes, info)) = store.read_decrypted(msg) {
        let std = match &structure {
            Structure::Encrypted { standard, .. } => *standard,
            _ => crate::config::CryptoStandard::OpenPgp,
        };
        let mut sec = info.unwrap_or_else(|| {
            let mut s = Security::new();
            s.notes.push("The signature result of the original mail was not stored with this copy.".into());
            s
        });
        let when = sec.checked;
        sec.encryption = Encryption::DecryptedEarlier(std, when);
        return Ok(loaded(mail_parse(&bytes)?, sec));
    }
    let mut mail = mail_parse(&raw)?;
    let mut sec = Security::new();
    match &structure {
        Structure::Plain => {}
        Structure::Encrypted { .. } | Structure::InlineEncrypted { .. } if !consent => {
            sec.encryption = Encryption::NeedsConsent;
            hide_content(&mut mail);
        }
        Structure::Encrypted { .. } | Structure::InlineEncrypted { .. } => {
            sec.working = Some("Decrypting… (a passphrase or PIN dialog may open; Cancel stops it)".into());
            hide_content(&mut mail);
        }
        Structure::Mixed { .. } | Structure::Signed { .. } | Structure::OpaqueSigned { .. } | Structure::InlineSigned { .. } => sec.working = Some("Checking the signature…".into()),
    }
    Ok(loaded(mail, sec))
}

/// An encrypted mail that is not decrypted shows no body and no attachments: what is in it is ciphertext.
fn hide_content(m: &mut Mail) {
    m.text.clear();
    m.attachments.clear();
}

fn mail_parse(raw: &[u8]) -> Result<Mail, String> {
    crate::jobs::catch(|| crate::mail::parse(raw))
}

fn loaded(mail: Mail, security: Security) -> Loaded {
    let (shown, cut) = cut_text(&mail.text);
    Loaded { mail, shown, cut, security }
}

/// The job behind the status strip: GnuPG over the message, and with the consent the decrypted copy stored.
fn crypto_job(store: &MailStore, g: &Gnupg, msg: &MessageSummary, consent: bool, cancel: &AtomicBool) -> Result<Loaded, String> {
    let raw = store.read_raw(msg).map_err(|e| e.0)?;
    let now = crate::calendar::time::now();
    let mut out = crypto::process(g, &raw, consent, cancel, now);
    if out.decrypted && consent {
        if let Some(d) = &out.display {
            if let Err(e) = store.save_decrypted(msg, d, &out.security, &crate::mail::store::today_string()) {
                out.security.notes.push(format!("The decrypted copy could not be stored (search will not find this mail's text): {e}"));
            }
        }
    }
    let mut mail = match &out.display {
        Some(d) => mail_parse(d)?,
        None => mail_parse(&raw)?,
    };
    if out.display.is_none()
        && matches!(out.security.encryption, Encryption::NeedsConsent | Encryption::Failed(_) | Encryption::NotDecrypted(_))
        && matches!(crypto::classify(&raw), Structure::Encrypted { .. } | Structure::InlineEncrypted { .. })
    {
        hide_content(&mut mail);
    }
    Ok(loaded(mail, out.security))
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
    use crate::calendar::time as t;
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
    use crate::calendar::time as t;
    let (day, secs) = t::utc_to_local(ts);
    let (y, m, d) = t::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02} {}", t::hhmm(secs))
}

/// A fingerprint in groups of four, for people to compare.
pub fn short_fingerprint(fp: &str) -> String {
    fp.to_ascii_uppercase().as_bytes().chunks(4).map(|c| String::from_utf8_lossy(c).into_owned()).collect::<Vec<_>>().join(" ")
}

/// Bytes as "12 KB" / "3.4 MB".
pub fn size_text(n: u64) -> String {
    if n >= 1 << 20 {
        format!("{:.1} MB", n as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", n.div_ceil(1024).max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CryptoStandard::{OpenPgp, Smime};
    use crate::mail::compose::Att;
    use crate::mail::outbox::tests::{cut_off_replacement, temp};

    fn account(id: &str) -> MailAccount {
        MailAccount { id: id.into(), ..MailAccount::default() }
    }

    /// A view whose store is open (in a temporary folder) with an account `a` that has the folders INBOX and Other.
    fn view_with_store(tag: &str) -> (MailView, Config, Arc<MailStore>, Vec<FolderRow>) {
        let root = temp(tag);
        let store = Arc::new(MailStore::open(&root).unwrap());
        let folders = vec![store.ensure_folder("a", "A", "INBOX", None, None, true).unwrap(), store.ensure_folder("a", "A", "Other", None, None, true).unwrap()];
        let mut v = MailView::new(root);
        v.store = StoreState::Ready(store.clone());
        v.reader = store.reader().ok();
        let cfg = Config { mail_accounts: vec![account("a")], ..Config::default() };
        (v, cfg, store, folders)
    }

    fn running_sync() -> SyncRun {
        SyncRun { rx: channel().1, cancel: Arc::new(AtomicBool::new(false)), folder: String::new(), done: 0, total: 0, added: 0, errors: vec![], finished: None }
    }

    // spec: USE-012
    #[test]
    fn the_mail_index_is_not_rebuilt_while_the_store_is_closed_or_a_sync_an_action_a_send_or_a_folder_check_runs() {
        let (mut v, _cfg, store, _) = view_with_store("rebuild-blocked");
        assert_eq!(v.rebuild_blocked(), None, "nothing runs: the rebuild may start");
        // the store is not open (still closed, opening, being rebuilt already)
        for state in [StoreState::Closed, StoreState::Opening, StoreState::Rebuilding(store.clone()), StoreState::Failed("x".into())] {
            v.store = state;
            assert!(v.rebuild_blocked().is_some_and(|w| w.contains("not open")));
        }
        v.store = StoreState::Ready(store);
        // a sync runs; a finished one does not block
        v.syncs.insert("a".into(), running_sync());
        assert!(v.rebuild_blocked().is_some_and(|w| w.contains("syncing")));
        v.syncs.get_mut("a").unwrap().finished = Some(SyncSummary::default());
        assert_eq!(v.rebuild_blocked(), None);
        // actions on their way to the server
        v.ops_running.insert("a".into());
        assert!(v.rebuild_blocked().is_some_and(|w| w.contains("sent to the server")));
        v.ops_running.clear();
        assert_eq!(v.rebuild_blocked(), None);
        // a mail on its way
        v.send_running.insert("a/1".into());
        assert!(v.rebuild_blocked().is_some_and(|w| w.contains("sent to the server")));
        v.send_running.clear();
        // a folder check writes to the index
        v.watch.set_checking(1);
        assert!(v.rebuild_blocked().is_some_and(|w| w.contains("checked for changes")));
        v.watch.set_checking(0);
        assert_eq!(v.rebuild_blocked(), None);
    }

    // spec: USE-012
    #[test]
    fn a_refused_rebuild_leaves_the_store_ready_and_a_started_one_closes_the_message_and_hands_out_the_store_alone() {
        let (mut v, _cfg, store, folders) = view_with_store("rebuild-begin");
        let nm = store.save_message(&folders[0], 1, 1, b"From: a@x.de\r\nSubject: Hi\r\nMessage-ID: <1@x>\r\n\r\nbody\r\n", "", Some(1_794_657_600)).unwrap();
        store.commit(vec![crate::mail::db::Op::Message(Box::new(nm))]).unwrap();
        let msg = v.reader.as_ref().unwrap().list(folders[0].id, 10, 0).unwrap().remove(0);
        v.open_message(msg);
        assert!(v.open.is_some());
        // blocked: the reason comes back, nothing changes
        v.ops_running.insert("a".into());
        let why = v.begin_rebuild().err().expect("refused");
        assert_eq!(Some(why), v.rebuild_blocked());
        assert!(matches!(v.store, StoreState::Ready(_)) && v.open.is_some(), "a refusal changes nothing");
        v.ops_running.clear();
        // allowed: the same store comes back, the message is closed, the store is Rebuilding
        let handed = v.begin_rebuild().unwrap();
        assert!(Arc::ptr_eq(&handed, &store));
        assert!(matches!(v.store, StoreState::Rebuilding(_)));
        assert!(v.open.is_none(), "the open message is closed");
        // meanwhile nothing that writes the index can start: every job starts from ready_store
        assert!(v.ready_store().is_none());
        assert!(v.rebuild_blocked().is_some());
        assert!(v.begin_rebuild().is_err(), "no second rebuild");
        v.open_store();
        assert!(matches!(v.store, StoreState::Rebuilding(_)), "open_store does not take the store back");
    }

    // spec: USE-012
    #[test]
    fn the_end_of_a_rebuild_makes_the_store_ready_again_keeps_the_shown_folder_or_selects_the_first_and_does_nothing_without_a_rebuild() {
        // no rebuild: nothing happens
        let (mut v, cfg, _store, _) = view_with_store("rebuild-end");
        let before = v.ready_store().unwrap();
        v.folder = Some(7);
        v.end_rebuild(&cfg);
        assert!(Arc::ptr_eq(&v.ready_store().unwrap(), &before), "a Ready store stays the same Ready store");
        assert_eq!(v.folder, Some(7), "the shown folder is untouched");
        v.store = StoreState::Failed("x".into());
        v.end_rebuild(&cfg);
        assert!(matches!(&v.store, StoreState::Failed(m) if m == "x"), "a Failed store stays Failed");
        v.store = StoreState::Opening;
        v.end_rebuild(&cfg);
        assert!(matches!(v.store, StoreState::Opening), "an Opening store stays Opening");
        v.store = StoreState::Closed;
        v.end_rebuild(&cfg);
        assert!(matches!(v.store, StoreState::Closed));
        assert!(v.folders.is_empty(), "nothing was read");
        // no folder shown: the first one is selected, and the reader is back
        let (mut v, cfg, _store, folders) = view_with_store("rebuild-end-none");
        v.begin_rebuild().unwrap();
        v.reader = None;
        v.end_rebuild(&cfg);
        assert!(v.ready_store().is_some());
        assert!(v.reader.is_some(), "the reader is opened again");
        assert_eq!(v.folders.len(), 2);
        assert_eq!(v.folder, Some(folders[0].id), "the first folder of the first account");
        // a folder shown stays shown
        let (mut v, cfg, _store, folders) = view_with_store("rebuild-end-keep");
        v.folder = Some(folders[1].id);
        v.begin_rebuild().unwrap();
        v.end_rebuild(&cfg);
        assert!(v.ready_store().is_some());
        assert_eq!(v.folder, Some(folders[1].id));
        assert!(v.rebuild_blocked().is_none(), "work may start again");
    }

    fn compose_view(accounts: Vec<MailAccount>, draft: compose::Draft) -> (MailView, Config) {
        let mut v = MailView::new(temp("compose-out"));
        v.compose = Some(Compose::new(draft));
        (v, Config { mail_accounts: accounts, ..Config::default() })
    }

    fn att(name: &str, file: &str) -> Att {
        Att { name: name.into(), mime: "application/pdf".into(), data: Arc::new(name.as_bytes().to_vec()), file: file.into() }
    }

    fn alert(file: &str) -> crate::mail::compose_ui::AttAlertState {
        crate::mail::compose_ui::AttAlertState { alert: crate::mail::outbox::AttAlert { file: file.into(), name: file.into(), trash: PathBuf::from("t"), trash_present: true }, dismissed: false }
    }

    // spec: MAIL-190, MAIL-192
    #[test]
    fn switching_the_draft_to_another_account_takes_its_sender_and_falls_back_to_its_standard_when_it_has_no_key_for_the_current_one() {
        let a1 = MailAccount { pgp_key: "AAAA".into(), from_name: "Ada".into(), from_address: "ada@x.de".into(), ..account("a1") };
        let a2 = MailAccount { smime_key: "BBBB".into(), preferred_standard: Smime, sign_by_default: true, from_name: "Bea".into(), user: "bea@y.de".into(), ..account("a2") };
        let a3 = MailAccount { pgp_key: "CCCC".into(), from_name: "Cy".into(), from_address: "cy@z.de".into(), ..account("a3") };
        let mut d = compose::new_draft("a1", "Ada", "ada@x.de", "");
        (d.standard, d.sign, d.encrypt) = (OpenPgp, false, true);
        let (mut v, cfg) = compose_view(vec![a1, a2, a3], d);
        // a2 has no OpenPGP key: its own standard and sign default, and Encrypt is off
        v.apply_compose_out(&cfg, ComposeOut { account: Some("a2".into()), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert_eq!((c.draft.account.as_str(), c.draft.from_name.as_str(), c.draft.from_addr.as_str()), ("a2", "Bea", "bea@y.de"), "the user name is the address when none is set");
        assert_eq!((c.draft.standard, c.draft.sign, c.draft.encrypt), (Smime, true, false));
        assert_eq!(c.rev, 1, "the draft counts as changed");
        // a3 has a key for OpenPGP only: the draft is on S/MIME, so it falls back again
        v.compose.as_mut().unwrap().draft.encrypt = true;
        v.apply_compose_out(&cfg, ComposeOut { account: Some("a3".into()), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert_eq!((c.draft.account.as_str(), c.draft.from_addr.as_str(), c.draft.standard, c.draft.encrypt), ("a3", "cy@z.de", OpenPgp, false));
        // an account with a key for the standard in use: standard, Sign and Encrypt stay
        let mut d = compose::new_draft("a3", "Cy", "cy@z.de", "");
        (d.standard, d.sign, d.encrypt) = (OpenPgp, true, true);
        v.compose = Some(Compose::new(d));
        let a4 = MailAccount { pgp_key: "DDDD".into(), from_name: "Di".into(), from_address: "di@z.de".into(), ..account("a4") };
        let cfg4 = Config { mail_accounts: vec![a4], ..Config::default() };
        v.apply_compose_out(&cfg4, ComposeOut { account: Some("a4".into()), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert_eq!((c.draft.account.as_str(), c.draft.standard, c.draft.sign, c.draft.encrypt), ("a4", OpenPgp, true, true));
        // an account that is not configured changes nothing
        v.apply_compose_out(&cfg4, ComposeOut { account: Some("gone".into()), ..Default::default() });
        assert_eq!(v.compose.as_ref().unwrap().draft.account, "a4");
    }

    // spec: MAIL-127, MAIL-216, MAIL-219
    #[test]
    fn removing_an_attachment_by_index_or_a_missing_one_by_file_drops_its_alert_and_an_index_out_of_range_does_nothing() {
        let mut d = compose::new_draft("a", "Ada", "ada@x.de", "");
        d.attachments = vec![att("one.pdf", "0-1-one.pdf"), att("two.pdf", "1-2-two.pdf"), att("three.pdf", "2-3-three.pdf")];
        let (mut v, cfg) = compose_view(vec![account("a")], d);
        v.compose.as_mut().unwrap().att_alerts = vec![alert("0-1-one.pdf"), alert("1-2-two.pdf")];
        let files = |v: &MailView| v.compose.as_ref().unwrap().draft.attachments.iter().map(|a| a.file.clone()).collect::<Vec<_>>();
        let alerts = |v: &MailView| v.compose.as_ref().unwrap().att_alerts.iter().map(|a| a.alert.file.clone()).collect::<Vec<_>>();
        // out of range: nothing changes, not even the revision
        v.apply_compose_out(&cfg, ComposeOut { remove_att: Some(3), ..Default::default() });
        assert_eq!(files(&v).len(), 3);
        assert_eq!(alerts(&v).len(), 2);
        assert_eq!(v.compose.as_ref().unwrap().rev, 0);
        // by index: the attachment and its alert go, the other alert stays
        v.apply_compose_out(&cfg, ComposeOut { remove_att: Some(0), ..Default::default() });
        assert_eq!(files(&v), ["1-2-two.pdf", "2-3-three.pdf"]);
        assert_eq!(alerts(&v), ["1-2-two.pdf"]);
        assert_eq!(v.compose.as_ref().unwrap().rev, 1);
        // a missing one by file name
        v.apply_compose_out(&cfg, ComposeOut { att_remove_missing: Some("1-2-two.pdf".into()), ..Default::default() });
        assert_eq!(files(&v), ["2-3-three.pdf"]);
        assert!(alerts(&v).is_empty());
        assert_eq!(v.compose.as_ref().unwrap().rev, 2);
    }

    // spec: MAIL-216, MAIL-219
    #[test]
    fn dismissing_an_alert_hides_it_but_keeps_the_alert_and_the_attachment() {
        let mut d = compose::new_draft("a", "Ada", "ada@x.de", "");
        d.attachments = vec![att("one.pdf", "0-1-one.pdf"), att("two.pdf", "1-2-two.pdf")];
        let (mut v, cfg) = compose_view(vec![account("a")], d);
        v.compose.as_mut().unwrap().att_alerts = vec![alert("0-1-one.pdf"), alert("1-2-two.pdf")];
        v.apply_compose_out(&cfg, ComposeOut { att_dismiss: Some("1-2-two.pdf".into()), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert_eq!(c.att_alerts.iter().map(|a| a.dismissed).collect::<Vec<_>>(), [false, true]);
        assert_eq!(c.draft.attachments.len(), 2, "dismissing changes nothing on the draft");
        assert_eq!(c.rev, 0);
    }

    /// A compose view of a draft whose attachment replacement was cut off: its alert and its outbox.
    fn view_with_cut_off_attachment(tag: &str, trash_gone: bool) -> (MailView, Config, String, PathBuf) {
        let (root, ob, d) = cut_off_replacement(tag, 2);
        let (back, _) = ob.load_draft("acct1", &d.id).unwrap();
        let al = ob.att_alerts("drafts", "acct1", &d.id, &back.attachments);
        assert_eq!(al.len(), 1);
        assert!(back.attachments[0].data.is_empty(), "the bytes were not read back");
        if trash_gone {
            std::fs::remove_file(&al[0].trash).unwrap();
        }
        let file = al[0].file.clone();
        let (mut v, cfg) = compose_view(vec![account("acct1")], back);
        v.outbox = Some(Arc::new(ob));
        v.compose.as_mut().unwrap().att_alerts = al.into_iter().map(|alert| crate::mail::compose_ui::AttAlertState { alert, dismissed: false }).collect();
        (v, cfg, file, root)
    }

    // spec: MAIL-217
    #[test]
    fn restoring_a_missing_attachment_puts_the_old_bytes_back_and_drops_the_alert() {
        let (mut v, cfg, file, root) = view_with_cut_off_attachment("compose-restore", false);
        v.apply_compose_out(&cfg, ComposeOut { att_restore: Some(file), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert_eq!(c.draft.attachments[0].data.as_slice(), b"old-bytes");
        assert!(c.att_alerts.is_empty());
        assert!(c.error.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    // spec: MAIL-217
    #[test]
    fn a_failed_restore_says_why_and_keeps_the_alert_and_the_empty_attachment() {
        // a file that has no record
        let (mut v, cfg, _file, root) = view_with_cut_off_attachment("compose-restore-none", false);
        v.apply_compose_out(&cfg, ComposeOut { att_restore: Some("no-such-file.pdf".into()), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert!(c.error.as_ref().is_some_and(|e| e.starts_with("The old file could not be restored: ") && e.contains("nothing to restore")), "{:?}", c.error);
        assert_eq!(c.att_alerts.len(), 1);
        assert!(c.draft.attachments[0].data.is_empty());
        let _ = std::fs::remove_dir_all(root);
        // the trash copy is gone
        let (mut v, cfg, file, root) = view_with_cut_off_attachment("compose-restore-gone", true);
        v.apply_compose_out(&cfg, ComposeOut { att_restore: Some(file), ..Default::default() });
        let c = v.compose.as_ref().unwrap();
        assert!(c.error.as_ref().is_some_and(|e| e.contains("not in the trash any more")), "{:?}", c.error);
        assert_eq!(c.att_alerts.len(), 1, "the alert stays");
        assert!(c.draft.attachments[0].data.is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    // spec: JOB-015
    #[test]
    fn job_015_the_store_opens_as_a_task_that_is_gone_before_the_store_is_ready_and_never_twice() {
        let root = temp("open-task");
        let mut v = MailView::new(root.clone());
        let cfg = Config::default();
        // an action with this key runs already: the store does not start to open
        let held = v.busy.begin(MAIL_INDEX, "Checking the mail index…").unwrap();
        v.open_store();
        assert!(matches!(v.store, StoreState::Closed));
        drop(held);
        v.open_store();
        assert!(matches!(v.store, StoreState::Opening));
        let start = Instant::now();
        while v.ready_store().is_none() {
            v.tick(&cfg, false);
            assert!(start.elapsed() < Duration::from_secs(30), "the store opens");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(v.busy.info(&MAIL_INDEX).is_none(), "the view that sees the store ready finds no task");
        let _ = std::fs::remove_dir_all(root);
    }

    // spec: JOB-015
    #[test]
    fn job_015_a_store_that_cannot_be_opened_ends_its_task_too() {
        let root = temp("open-task-fails");
        // the store's folder is a file: the open fails
        let file = root.join("a-file");
        std::fs::write(&file, b"x").unwrap();
        let mut v = MailView::new(file);
        v.open_store();
        let start = Instant::now();
        while !matches!(v.store, StoreState::Failed(_)) {
            v.tick(&Config::default(), false);
            assert!(start.elapsed() < Duration::from_secs(30), "the open fails");
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(v.busy.info(&MAIL_INDEX).is_none());
        let _ = std::fs::remove_dir_all(root);
    }
}
