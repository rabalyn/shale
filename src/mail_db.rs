//! The mail index (SQLite): schema and migrations, writing a batch, and the read queries
//! (list a folder, search). The index is a cache of the `.eml` files and the server state:
//! `mail_store.rs` can rebuild it from the files at any time.
//!
//! Search text from the user never reaches the FTS5 query language raw: [`fts_query`] turns it
//! into quoted phrases, so `"`, `*`, `NEAR`, `-` or `:` in what someone types is only text.

use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::time::Duration;

/// Newest schema version this program knows.
pub const SCHEMA_VERSION: i64 = 2;

/// What went wrong, in words for the person (the store never panics on bad data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailError(pub String);

impl fmt::Display for MailError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MailError {}

impl From<rusqlite::Error> for MailError {
    fn from(e: rusqlite::Error) -> Self {
        MailError(format!("mail index: {e}"))
    }
}

impl From<std::io::Error> for MailError {
    fn from(e: std::io::Error) -> Self {
        MailError(e.to_string())
    }
}

impl From<String> for MailError {
    fn from(e: String) -> Self {
        MailError(e)
    }
}

impl From<&str> for MailError {
    fn from(e: &str) -> Self {
        MailError(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, MailError>;

/// The message file is in the store.
pub const STATE_FULL: i64 = 0;
/// Only the headers are known (a listing of a folder that is not synced, such as Trash).
pub const STATE_HEADER: i64 = 1;
/// The message is bigger than the size limit and was not downloaded.
pub const STATE_TOO_LARGE: i64 = 2;

// ---------------------------------------------------------------- schema

/// Migration `n` (index `n - 1`) brings the schema from version `n - 1` to `n`.
const MIGRATIONS: [&str; 2] = [
    r#"
CREATE TABLE accounts (
    id TEXT PRIMARY KEY,
    display TEXT NOT NULL
);
CREATE TABLE folders (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL REFERENCES accounts(id),
    name TEXT NOT NULL,
    delimiter TEXT,
    special_use TEXT,
    uidvalidity INTEGER,
    uidnext INTEGER,
    highestmodseq INTEGER,
    sync_enabled INTEGER NOT NULL DEFAULT 1,
    last_sync INTEGER,
    UNIQUE (account, name)
);
CREATE TABLE messages (
    id INTEGER PRIMARY KEY,
    folder INTEGER NOT NULL REFERENCES folders(id),
    uidvalidity INTEGER NOT NULL,
    uid INTEGER NOT NULL,
    path TEXT NOT NULL,
    state INTEGER NOT NULL DEFAULT 0,
    flags TEXT NOT NULL DEFAULT '',
    date INTEGER,
    from_addr TEXT NOT NULL DEFAULT '',
    to_addr TEXT NOT NULL DEFAULT '',
    cc_addr TEXT NOT NULL DEFAULT '',
    subject TEXT NOT NULL DEFAULT '',
    message_id TEXT NOT NULL DEFAULT '',
    size INTEGER NOT NULL DEFAULT 0,
    has_attachments INTEGER NOT NULL DEFAULT 0,
    parsed INTEGER NOT NULL DEFAULT 1,
    gone_on_server INTEGER NOT NULL DEFAULT 0,
    missing_file INTEGER NOT NULL DEFAULT 0,
    UNIQUE (folder, uidvalidity, uid)
);
CREATE INDEX messages_by_date ON messages (folder, date DESC);
CREATE INDEX messages_by_message_id ON messages (message_id);
CREATE VIRTUAL TABLE mail_fts USING fts5 (
    subject, from_addr, to_addr, cc_addr, body, attachments,
    tokenize = 'unicode61 remove_diacritics 2'
);
"#,
    // version 2 (phase 2, ADR-0009): what the person asked for and the server has not confirmed yet
    r#"
CREATE TABLE pending_ops (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL,
    kind TEXT NOT NULL,
    folder INTEGER NOT NULL,
    uidvalidity INTEGER NOT NULL,
    uid INTEGER NOT NULL,
    target TEXT,
    flags_add TEXT NOT NULL DEFAULT '',
    flags_remove TEXT NOT NULL DEFAULT '',
    prev_flags TEXT NOT NULL DEFAULT '',
    created INTEGER NOT NULL DEFAULT 0,
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT
);
CREATE INDEX pending_by_message ON pending_ops (folder, uidvalidity, uid);
"#,
];

/// The version the file is at (0 = a new, empty file).
pub fn current_version(c: &Connection) -> Result<i64> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL)")?;
    Ok(c.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0))?)
}

/// Bring the schema up to date, one migration per transaction. A file written by a newer Shale
/// is refused untouched (an older program must not "fix" what it does not understand).
pub fn migrate(c: &Connection) -> Result<()> {
    let now = current_version(c)?;
    if now > SCHEMA_VERSION {
        return Err(MailError(format!("the mail index is from a newer Shale (version {now}, this one knows {SCHEMA_VERSION})")));
    }
    for v in now..SCHEMA_VERSION {
        let tx = c.unchecked_transaction()?;
        tx.execute_batch(MIGRATIONS[v as usize])?;
        tx.execute("DELETE FROM schema_version", [])?;
        tx.execute("INSERT INTO schema_version (version) VALUES (?1)", [v + 1])?;
        tx.commit()?;
    }
    Ok(())
}

/// Open the index for writing: WAL, `synchronous=FULL`, migrated. Only the writer thread uses this.
pub fn open_writer(path: &Path) -> Result<Connection> {
    let c = Connection::open(path)?;
    c.busy_timeout(Duration::from_secs(10))?;
    c.pragma_update(None, "journal_mode", "WAL")?;
    c.pragma_update(None, "synchronous", "FULL")?;
    c.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&c)?;
    Ok(c)
}

/// Open the index for reading only (the window's own connection).
pub fn open_reader(path: &Path) -> Result<Connection> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    c.busy_timeout(Duration::from_secs(10))?;
    Ok(c)
}

/// `PRAGMA quick_check`: Ok if the file is a healthy database of a version we know.
pub fn quick_check(path: &Path) -> Result<()> {
    let c = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    c.busy_timeout(Duration::from_secs(10))?;
    let answer: String = c.query_row("PRAGMA quick_check(1)", [], |r| r.get(0))?;
    if answer != "ok" {
        return Err(MailError(format!("the mail index failed its check: {answer}")));
    }
    // the check passes on an empty file as well; a real index has its tables
    let v: i64 = c.query_row("SELECT COALESCE(MAX(version), 0) FROM schema_version", [], |r| r.get(0))?;
    if v < 1 {
        return Err("the mail index has no tables".into());
    }
    Ok(())
}

// ---------------------------------------------------------------- data

/// What is known about one folder of an account.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderRow {
    pub id: i64,
    pub account: String,
    /// The name as the server spells it (modified UTF-7), also what `EXAMINE` gets.
    pub name: String,
    pub delimiter: Option<String>,
    pub special_use: Option<String>,
    pub uidvalidity: Option<u32>,
    /// Set only when a sync of the folder finished: everything below it is stored.
    pub uidnext: Option<u32>,
    pub highestmodseq: Option<u64>,
    pub sync_enabled: bool,
    pub last_sync: Option<i64>,
}

/// What the search and the list show of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageSummary {
    pub id: i64,
    pub account: String,
    pub folder_id: i64,
    pub folder: String,
    pub uid: u32,
    pub uidvalidity: u32,
    /// Relative to the store folder; empty if there is no file (see `state`).
    pub path: String,
    pub state: i64,
    pub flags: String,
    pub date: Option<i64>,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub message_id: String,
    pub size: u64,
    pub has_attachments: bool,
    /// False if the message could not be read (it is stored as it came).
    pub parsed: bool,
    pub gone_on_server: bool,
    pub missing_file: bool,
    /// The kind of the newest action on this message that the server has not confirmed yet
    /// (`flags`, `move`, `delete`, `expunge`).
    pub pending: Option<String>,
}

impl MessageSummary {
    pub fn is_flagged(&self) -> bool {
        has_flag(&self.flags, "\\Flagged")
    }

    pub fn is_seen(&self) -> bool {
        self.flags.split(' ').any(|f| f.eq_ignore_ascii_case("\\Seen"))
    }
}

/// The searchable parts of a message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexFields {
    pub date: Option<i64>,
    pub from: String,
    pub to: String,
    pub cc: String,
    pub subject: String,
    pub message_id: String,
    pub has_attachments: bool,
    /// Plain text of the body (cut to a sensible size).
    pub body: String,
    /// File names of the attachments, space separated.
    pub attachments: String,
}

/// A message to put into the index. The file must be on disk already (MAIL-003).
#[derive(Clone, Debug)]
pub struct NewMessage {
    pub folder: i64,
    pub uidvalidity: u32,
    pub uid: u32,
    pub path: String,
    pub state: i64,
    pub flags: String,
    pub size: u64,
    pub parsed: bool,
    pub fields: IndexFields,
}

/// One change to the index. A batch of these is one transaction.
#[derive(Clone, Debug)]
pub enum Op {
    Message(Box<NewMessage>),
    Flags {
        folder: i64,
        uidvalidity: u32,
        uid: u32,
        flags: String,
    },
    Gone {
        folder: i64,
        uidvalidity: u32,
        uid: u32,
        gone: bool,
    },
    MissingFile {
        id: i64,
        missing: bool,
    },
    FolderState {
        folder: i64,
        uidvalidity: Option<u32>,
        uidnext: Option<u32>,
        highestmodseq: Option<u64>,
        last_sync: i64,
    },
    SyncEnabled {
        folder: i64,
        enabled: bool,
    },
    /// Record an action of the person; a `flags` action also changes the local flags in the same
    /// transaction (the previous flags are kept in the row, for a rollback).
    PendingAdd(Box<PendingRow>),
    /// The server confirmed (or the action ended some other way that the caller has handled): forget it.
    PendingDone {
        id: i64,
    },
    /// The server refused: undo the local effect of a `flags` action, then forget it.
    PendingRollback {
        id: i64,
    },
    /// An attempt failed in a way that is worth trying again: count it, remember why.
    PendingTried {
        id: i64,
        error: String,
    },
    /// The search text of a message is replaced by that of its decrypted copy (ADR-0011 §3): body and attachment
    /// names (subject, from, to and cc stay the outer ones).
    Body {
        folder: i64,
        uidvalidity: u32,
        uid: u32,
        body: String,
        attachments: String,
        has_attachments: bool,
    },
    /// The message is not in this folder any more (it moved or was deleted): its row and its
    /// search entry go. The caller has taken care of the file (it is never deleted).
    Remove {
        folder: i64,
        uidvalidity: u32,
        uid: u32,
    },
}

/// One action of the person that waits for the server (MAIL-107).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingRow {
    pub id: i64,
    pub account: String,
    /// `flags`, `move`, `delete` (a move to Trash) or `expunge` (delete forever).
    pub kind: String,
    pub folder: i64,
    pub uidvalidity: u32,
    pub uid: u32,
    /// The folder a `move` or `delete` goes to, as the server spells it.
    pub target: Option<String>,
    /// Flags to set and to clear (`flags` only), separated by spaces.
    pub flags_add: String,
    pub flags_remove: String,
    /// The local flags before the optimistic change.
    pub prev_flags: String,
    pub created: i64,
    pub attempts: i64,
    pub last_error: Option<String>,
}

/// Whether a flag list (separated by spaces) has this flag, ignoring case.
pub fn has_flag(flags: &str, flag: &str) -> bool {
    flags.split(' ').any(|f| f.eq_ignore_ascii_case(flag))
}

/// The flags in the form the index keeps them: sorted ignoring case, separated by spaces.
pub fn canonical_flags(list: impl IntoIterator<Item = String>) -> String {
    let mut v: Vec<String> = list.into_iter().filter(|f| !f.is_empty()).collect();
    v.sort_by_key(|f| f.to_ascii_lowercase());
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    v.join(" ")
}

/// `flags` with the `add` flags set and the `remove` flags cleared.
pub fn change_flags(flags: &str, add: &str, remove: &str) -> String {
    let mut v: Vec<String> = flags.split(' ').filter(|f| !f.is_empty() && !remove.split(' ').any(|r| r.eq_ignore_ascii_case(f))).map(String::from).collect();
    v.extend(add.split(' ').filter(|f| !f.is_empty()).map(String::from));
    canonical_flags(v)
}

/// Undo a flags action on `current`: a flag that the action set goes again unless it was set
/// before, a flag it cleared comes back if it was set before. Other flags (and the changes of
/// other actions) stay as they are.
pub fn undo_flags(current: &str, prev: &str, add: &str, remove: &str) -> String {
    let was = |f: &str| has_flag(prev, f);
    let drop: Vec<&str> = add.split(' ').filter(|f| !f.is_empty() && !was(f)).collect();
    let back: Vec<&str> = remove.split(' ').filter(|f| !f.is_empty() && was(f)).collect();
    change_flags(current, &back.join(" "), &drop.join(" "))
}

impl Op {
    /// Put a message into the index (or replace its row).
    pub fn message(m: NewMessage) -> Op {
        Op::Message(Box::new(m))
    }
}

/// Apply a batch in one transaction: all of it or none of it. `crash_after` is for tests: stop
/// by panicking after that many operations, inside the transaction.
pub fn apply_batch(c: &mut Connection, ops: &[Op], crash_after: Option<usize>) -> Result<()> {
    let tx = c.transaction()?;
    for (i, op) in ops.iter().enumerate() {
        if crash_after == Some(i) {
            panic!("test crash inside a batch");
        }
        apply_op(&tx, op)?;
    }
    tx.commit()?;
    Ok(())
}

fn apply_op(tx: &rusqlite::Transaction<'_>, op: &Op) -> Result<()> {
    match op {
        Op::Message(m) => insert_message(tx, m)?,
        Op::Flags { folder, uidvalidity, uid, flags } => {
            tx.execute("UPDATE messages SET flags = ?4 WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![folder, uidvalidity, uid, flags])?;
        }
        Op::Gone { folder, uidvalidity, uid, gone } => {
            tx.execute("UPDATE messages SET gone_on_server = ?4 WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![folder, uidvalidity, uid, *gone as i64])?;
        }
        Op::MissingFile { id, missing } => {
            tx.execute("UPDATE messages SET missing_file = ?2 WHERE id = ?1", params![id, *missing as i64])?;
        }
        Op::FolderState { folder, uidvalidity, uidnext, highestmodseq, last_sync } => {
            // rows of any other number are local copies now: no action may go to the server for them (MAIL-204)
            if let Some(new) = uidvalidity {
                tx.execute("UPDATE messages SET gone_on_server = 1 WHERE folder = ?1 AND uidvalidity NOT IN (0, ?2) AND gone_on_server = 0", params![folder, new])?;
            }
            tx.execute("UPDATE folders SET uidvalidity = ?2, uidnext = ?3, highestmodseq = ?4, last_sync = ?5 WHERE id = ?1", params![folder, uidvalidity, uidnext, highestmodseq.map(|m| m as i64), last_sync])?;
        }
        Op::SyncEnabled { folder, enabled } => {
            tx.execute("UPDATE folders SET sync_enabled = ?2 WHERE id = ?1", params![folder, *enabled as i64])?;
        }
        Op::PendingAdd(p) => {
            let mut prev = p.prev_flags.clone();
            if p.kind == "flags" {
                let cur: Option<String> = tx.query_row("SELECT flags FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![p.folder, p.uidvalidity, p.uid], |r| r.get(0)).optional()?;
                let Some(cur) = cur else { return Err(MailError("the message is not in the index".into())) };
                let new = change_flags(&cur, &p.flags_add, &p.flags_remove);
                tx.execute("UPDATE messages SET flags = ?4 WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![p.folder, p.uidvalidity, p.uid, new])?;
                prev = cur;
            }
            tx.execute(
                "INSERT INTO pending_ops (account, kind, folder, uidvalidity, uid, target, flags_add, flags_remove, prev_flags, created) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![p.account, p.kind, p.folder, p.uidvalidity, p.uid, p.target, p.flags_add, p.flags_remove, prev, p.created],
            )?;
        }
        Op::PendingDone { id } => {
            tx.execute("DELETE FROM pending_ops WHERE id = ?1", [id])?;
        }
        Op::PendingRollback { id } => {
            let row = pending_row(tx, *id)?;
            if let Some(p) = row {
                if p.kind == "flags" {
                    let cur: Option<String> = tx.query_row("SELECT flags FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![p.folder, p.uidvalidity, p.uid], |r| r.get(0)).optional()?;
                    if let Some(cur) = cur {
                        let back = undo_flags(&cur, &p.prev_flags, &p.flags_add, &p.flags_remove);
                        tx.execute("UPDATE messages SET flags = ?4 WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![p.folder, p.uidvalidity, p.uid, back])?;
                    }
                }
                // the sync skipped this message while the action waited: have the next sync compare every flag
                tx.execute("UPDATE folders SET highestmodseq = NULL WHERE id = ?1", [p.folder])?;
                tx.execute("DELETE FROM pending_ops WHERE id = ?1", [id])?;
            }
        }
        Op::PendingTried { id, error } => {
            tx.execute("UPDATE pending_ops SET attempts = attempts + 1, last_error = ?2 WHERE id = ?1", params![id, error])?;
        }
        Op::Body { folder, uidvalidity, uid, body, attachments, has_attachments } => {
            let row: Option<(i64, String, String, String, String)> = tx
                .query_row("SELECT id, subject, from_addr, to_addr, cc_addr FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![folder, uidvalidity, uid], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })
                .optional()?;
            if let Some((id, subject, from, to, cc)) = row {
                tx.execute("DELETE FROM mail_fts WHERE rowid = ?1", [id])?;
                tx.execute("INSERT INTO mail_fts (rowid, subject, from_addr, to_addr, cc_addr, body, attachments) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![id, subject, from, to, cc, body, attachments])?;
                tx.execute("UPDATE messages SET has_attachments = ?2 WHERE id = ?1", params![id, *has_attachments as i64])?;
            }
        }
        Op::Remove { folder, uidvalidity, uid } => {
            let id: Option<i64> = tx.query_row("SELECT id FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3", params![folder, uidvalidity, uid], |r| r.get(0)).optional()?;
            if let Some(id) = id {
                tx.execute("DELETE FROM mail_fts WHERE rowid = ?1", [id])?;
                tx.execute("DELETE FROM messages WHERE id = ?1", [id])?;
            }
        }
    }
    Ok(())
}

const PENDING_COLS: &str = "id, account, kind, folder, uidvalidity, uid, target, flags_add, flags_remove, prev_flags, created, attempts, last_error";

fn pending_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<PendingRow> {
    Ok(PendingRow {
        id: r.get(0)?,
        account: r.get(1)?,
        kind: r.get(2)?,
        folder: r.get(3)?,
        uidvalidity: r.get(4)?,
        uid: r.get(5)?,
        target: r.get(6)?,
        flags_add: r.get(7)?,
        flags_remove: r.get(8)?,
        prev_flags: r.get(9)?,
        created: r.get(10)?,
        attempts: r.get(11)?,
        last_error: r.get(12)?,
    })
}

fn pending_row(c: &Connection, id: i64) -> Result<Option<PendingRow>> {
    Ok(c.query_row(&format!("SELECT {PENDING_COLS} FROM pending_ops WHERE id = ?1"), [id], pending_from).optional()?)
}

/// The actions that wait for the server, oldest first (one account, or all).
pub fn pending_ops(c: &Connection, account: Option<&str>) -> Result<Vec<PendingRow>> {
    let mut st = c.prepare(&format!("SELECT {PENDING_COLS} FROM pending_ops WHERE (?1 IS NULL OR account = ?1) ORDER BY id"))?;
    let rows = st.query_map([account], pending_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// After an index was restored from a backup: its queue is old news (the actions were done, undone or given
/// up since), so every row is undone locally like a refusal and forgotten, in one transaction. A local sent
/// copy that waited for its `append` keeps its row and file. Returns how many rows went (MAIL-205).
pub fn drop_restored_queue(c: &mut Connection) -> Result<usize> {
    let tx = c.transaction()?;
    let ids: Vec<i64> = {
        let mut st = tx.prepare("SELECT id FROM pending_ops ORDER BY id DESC")?;
        let rows = st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for id in &ids {
        apply_op(&tx, &Op::PendingRollback { id: *id })?;
    }
    tx.commit()?;
    Ok(ids.len())
}

/// UIDs of a folder (in one UIDVALIDITY) that have an action waiting: the sync leaves their
/// flags and state alone (MAIL-108).
pub fn pending_uids(c: &Connection, folder: i64, uidvalidity: u32) -> Result<HashSet<u32>> {
    let mut st = c.prepare("SELECT DISTINCT uid FROM pending_ops WHERE folder = ?1 AND uidvalidity = ?2")?;
    let rows = st.query_map(params![folder, uidvalidity], |r| r.get(0))?.collect::<rusqlite::Result<HashSet<u32>>>()?;
    Ok(rows)
}

/// Is there a message with this Message-ID in the folder (any UIDVALIDITY)?
pub fn has_message_id(c: &Connection, folder: i64, message_id: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS (SELECT 1 FROM messages WHERE folder = ?1 AND message_id = ?2)", params![folder, message_id], |r| r.get(0))?)
}

/// The UIDVALIDITY of the local-only copies of sent mail (ADR-0011 step 0): RFC 3501 forbids a server to use 0,
/// so a copy kept under it can never meet a row or a file of the server, however high a real UID is.
pub const LOCAL_UIDVALIDITY: u32 = 0;

/// Local-only copies in a folder that nothing waits for any more and whose Message-ID the server's own
/// (fully fetched) copy in `uidvalidity` has: `(uid, path)`. The sync replaces them by the server's copy.
pub fn local_copies_seen_on_server(c: &Connection, folder: i64, uidvalidity: u32) -> Result<Vec<(u32, String)>> {
    let mut st = c.prepare(
        "SELECT l.uid, l.path FROM messages l
         WHERE l.folder = ?1 AND l.uidvalidity = 0 AND l.message_id <> ''
           AND NOT EXISTS (SELECT 1 FROM pending_ops p WHERE p.folder = l.folder AND p.uidvalidity = 0 AND p.uid = l.uid)
           AND EXISTS (SELECT 1 FROM messages s WHERE s.folder = l.folder AND s.uidvalidity = ?2 AND s.message_id = l.message_id AND s.state = 0 AND s.missing_file = 0)",
    )?;
    let rows = st.query_map(params![folder, uidvalidity], |r| Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The highest UID at or above `from` in a folder's UIDVALIDITY, if there is one.
pub fn max_uid_from(c: &Connection, folder: i64, uidvalidity: u32, from: u32) -> Result<Option<u32>> {
    Ok(c.query_row("SELECT MAX(uid) FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND uid >= ?3", params![folder, uidvalidity, from], |r| r.get::<_, Option<u32>>(0))?)
}

/// One message by its row id (any folder, any UIDVALIDITY).
pub fn message_by_id(c: &Connection, id: i64) -> Result<Option<MessageSummary>> {
    Ok(c.query_row(&format!("SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder WHERE m.id = ?1"), [id], summary_from).optional()?)
}

/// One message by folder, UIDVALIDITY and UID.
pub fn message_at(c: &Connection, folder: i64, uidvalidity: u32, uid: u32) -> Result<Option<MessageSummary>> {
    Ok(c.query_row(&format!("SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder WHERE m.folder = ?1 AND m.uidvalidity = ?2 AND m.uid = ?3"), params![folder, uidvalidity, uid], summary_from).optional()?)
}

fn insert_message(tx: &rusqlite::Transaction<'_>, m: &NewMessage) -> Result<()> {
    let f = &m.fields;
    let id: i64 = tx.query_row(
        "INSERT INTO messages (folder, uidvalidity, uid, path, state, flags, date, from_addr, to_addr, cc_addr, subject, message_id, size, has_attachments, parsed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
         ON CONFLICT (folder, uidvalidity, uid) DO UPDATE SET
            path = excluded.path, state = excluded.state, flags = excluded.flags, date = excluded.date,
            from_addr = excluded.from_addr, to_addr = excluded.to_addr, cc_addr = excluded.cc_addr,
            subject = excluded.subject, message_id = excluded.message_id, size = excluded.size,
            has_attachments = excluded.has_attachments, parsed = excluded.parsed,
            gone_on_server = 0, missing_file = 0
         RETURNING id",
        params![m.folder, m.uidvalidity, m.uid, m.path, m.state, m.flags, f.date, f.from, f.to, f.cc, f.subject, f.message_id, m.size as i64, f.has_attachments as i64, m.parsed as i64],
        |r| r.get(0),
    )?;
    tx.execute("DELETE FROM mail_fts WHERE rowid = ?1", [id])?;
    tx.execute("INSERT INTO mail_fts (rowid, subject, from_addr, to_addr, cc_addr, body, attachments) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)", params![id, f.subject, f.from, f.to, f.cc, f.body, f.attachments])?;
    Ok(())
}

/// Make sure the account and folder rows exist; the folder keeps what is known of it.
/// A new folder starts with the given `sync_enabled`; an existing one keeps the person's choice.
pub fn upsert_folder(c: &Connection, account: &str, display: &str, name: &str, delimiter: Option<&str>, special_use: Option<&str>, sync_enabled: bool) -> Result<FolderRow> {
    c.execute("INSERT INTO accounts (id, display) VALUES (?1, ?2) ON CONFLICT (id) DO UPDATE SET display = excluded.display", params![account, display])?;
    c.execute(
        "INSERT INTO folders (account, name, delimiter, special_use, sync_enabled) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (account, name) DO UPDATE SET delimiter = excluded.delimiter, special_use = excluded.special_use",
        params![account, name, delimiter, special_use, sync_enabled as i64],
    )?;
    folder_by_name(c, account, name)?.ok_or_else(|| "folder row missing".into())
}

// ---------------------------------------------------------------- reading

const FOLDER_COLS: &str = "id, account, name, delimiter, special_use, uidvalidity, uidnext, highestmodseq, sync_enabled, last_sync";

fn folder_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<FolderRow> {
    Ok(FolderRow {
        id: r.get(0)?,
        account: r.get(1)?,
        name: r.get(2)?,
        delimiter: r.get(3)?,
        special_use: r.get(4)?,
        uidvalidity: r.get(5)?,
        uidnext: r.get(6)?,
        highestmodseq: r.get::<_, Option<i64>>(7)?.map(|m| m as u64),
        sync_enabled: r.get::<_, i64>(8)? != 0,
        last_sync: r.get(9)?,
    })
}

pub fn folder_by_name(c: &Connection, account: &str, name: &str) -> Result<Option<FolderRow>> {
    Ok(c.query_row(&format!("SELECT {FOLDER_COLS} FROM folders WHERE account = ?1 AND name = ?2"), params![account, name], folder_from).optional()?)
}

pub fn folder_by_id(c: &Connection, id: i64) -> Result<Option<FolderRow>> {
    Ok(c.query_row(&format!("SELECT {FOLDER_COLS} FROM folders WHERE id = ?1"), [id], folder_from).optional()?)
}

/// All folders of an account (or of all accounts), by name.
pub fn folders(c: &Connection, account: Option<&str>) -> Result<Vec<FolderRow>> {
    let mut st = c.prepare(&format!("SELECT {FOLDER_COLS} FROM folders WHERE (?1 IS NULL OR account = ?1) ORDER BY account, name"))?;
    let rows = st.query_map([account], folder_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

const MSG_COLS: &str =
    "m.id, f.account, m.folder, f.name, m.uid, m.uidvalidity, m.path, m.state, m.flags, m.date, m.from_addr, m.to_addr, m.cc_addr, m.subject, m.message_id, m.size, m.has_attachments, m.parsed, m.gone_on_server, m.missing_file,
     (SELECT p.kind FROM pending_ops p WHERE p.folder = m.folder AND p.uidvalidity = m.uidvalidity AND p.uid = m.uid ORDER BY p.id DESC LIMIT 1)";

fn summary_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<MessageSummary> {
    Ok(MessageSummary {
        id: r.get(0)?,
        account: r.get(1)?,
        folder_id: r.get(2)?,
        folder: r.get(3)?,
        uid: r.get(4)?,
        uidvalidity: r.get(5)?,
        path: r.get(6)?,
        state: r.get(7)?,
        flags: r.get(8)?,
        date: r.get(9)?,
        from: r.get(10)?,
        to: r.get(11)?,
        cc: r.get(12)?,
        subject: r.get(13)?,
        message_id: r.get(14)?,
        size: r.get::<_, i64>(15)? as u64,
        has_attachments: r.get::<_, i64>(16)? != 0,
        parsed: r.get::<_, i64>(17)? != 0,
        gone_on_server: r.get::<_, i64>(18)? != 0,
        missing_file: r.get::<_, i64>(19)? != 0,
        pending: r.get(20)?,
    })
}

/// Which rows a list shows (MAIL-204): the folder's current UIDVALIDITY, local copies (0), and rows of an
/// older number unless the same message (same non-empty Message-ID) is there under the current one.
const VISIBLE: &str = "(f.uidvalidity = m.uidvalidity OR m.uidvalidity = 0 OR m.message_id = ''
    OR NOT EXISTS (SELECT 1 FROM messages c INDEXED BY messages_by_message_id WHERE c.message_id = m.message_id AND c.folder = m.folder AND c.uidvalidity = f.uidvalidity))";

/// A page of a folder's messages, newest first. Messages of an older UIDVALIDITY stay (as local copies
/// that are gone on the server) unless the current number has the same message.
pub fn list(c: &Connection, folder: i64, limit: usize, offset: usize) -> Result<Vec<MessageSummary>> {
    let mut st = c.prepare(&format!(
        "SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder AND {VISIBLE}
         WHERE m.folder = ?1 ORDER BY m.date DESC, m.uid DESC LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = st.query_map(params![folder, limit as i64, offset as i64], summary_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Search (see [`fts_query`] for what is understood), newest first. `folder` limits it to one folder.
pub fn search(c: &Connection, query: &str, folder: Option<i64>, limit: usize, offset: usize) -> Result<Vec<MessageSummary>> {
    let Some(q) = fts_query(query) else { return Ok(vec![]) };
    let mut st = c.prepare(&format!(
        "SELECT {MSG_COLS} FROM mail_fts JOIN messages m ON m.id = mail_fts.rowid JOIN folders f ON f.id = m.folder AND {VISIBLE}
         WHERE mail_fts MATCH ?1 AND (?2 IS NULL OR m.folder = ?2) ORDER BY m.date DESC, m.uid DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let rows = st.query_map(params![q, folder, limit as i64, offset as i64], summary_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// One message by folder and UID in the folder's current UIDVALIDITY.
pub fn message(c: &Connection, folder: i64, uid: u32) -> Result<Option<MessageSummary>> {
    Ok(c.query_row(
        &format!("SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder AND (f.uidvalidity = m.uidvalidity OR m.uidvalidity = 0) WHERE m.folder = ?1 AND m.uid = ?2 ORDER BY m.uidvalidity DESC LIMIT 1"),
        params![folder, uid],
        summary_from,
    )
    .optional()?)
}

/// Per folder: (messages shown in its list, how many of them are unread). One pass over the
/// index; the mail view asks only when something changed.
pub fn folder_counts(c: &Connection) -> Result<HashMap<i64, (i64, i64)>> {
    let mut st = c.prepare(&format!(
        "SELECT m.folder, COUNT(*), SUM(CASE WHEN instr(lower(' ' || m.flags || ' '), ' \\seen ') = 0 THEN 1 ELSE 0 END)
         FROM messages m JOIN folders f ON f.id = m.folder AND {VISIBLE} GROUP BY m.folder"
    ))?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0)))))?.collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(rows)
}

/// How many messages a folder's list has.
pub fn count_folder(c: &Connection, folder: i64) -> Result<i64> {
    Ok(c.query_row(&format!("SELECT COUNT(*) FROM messages m JOIN folders f ON f.id = m.folder AND {VISIBLE} WHERE m.folder = ?1"), [folder], |r| r.get(0))?)
}

/// How many messages of the folder's list come before this one (its row number in the list).
pub fn position_in_list(c: &Connection, folder: i64, date: Option<i64>, uid: u32) -> Result<i64> {
    let d = date.unwrap_or(i64::MIN);
    Ok(c.query_row(
        &format!(
            "SELECT COUNT(*) FROM messages m JOIN folders f ON f.id = m.folder AND {VISIBLE}
         WHERE m.folder = ?1 AND (COALESCE(m.date, -9223372036854775807) > ?2 OR (COALESCE(m.date, -9223372036854775807) = ?2 AND m.uid > ?3))"
        ),
        params![folder, d.max(-9223372036854775807), uid],
        |r| r.get(0),
    )?)
}

/// Number of messages in the index (all folders, all UIDVALIDITYs).
#[cfg(test)]
pub fn count_messages(c: &Connection) -> Result<i64> {
    Ok(c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?)
}

/// UIDs of a folder (in one UIDVALIDITY) whose message is stored, or known to be too big:
/// the sync does not fetch these again.
pub fn present_uids(c: &Connection, folder: i64, uidvalidity: u32) -> Result<HashSet<u32>> {
    let mut st = c.prepare("SELECT uid FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND state IN (0, 2) AND missing_file = 0")?;
    let rows = st.query_map(params![folder, uidvalidity], |r| r.get(0))?.collect::<rusqlite::Result<HashSet<u32>>>()?;
    Ok(rows)
}

/// What the sync compares with the server: UID → (flags, gone on server).
pub fn flags_of(c: &Connection, folder: i64, uidvalidity: u32) -> Result<HashMap<u32, (String, bool)>> {
    let mut st = c.prepare("SELECT uid, flags, gone_on_server FROM messages WHERE folder = ?1 AND uidvalidity = ?2")?;
    let rows = st.query_map(params![folder, uidvalidity], |r| Ok((r.get::<_, u32>(0)?, (r.get::<_, String>(1)?, r.get::<_, i64>(2)? != 0))))?.collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(rows)
}

/// UIDs whose file is missing and has to be fetched again.
pub fn missing_uids(c: &Connection, folder: i64, uidvalidity: u32) -> Result<Vec<u32>> {
    let mut st = c.prepare("SELECT uid FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND missing_file = 1")?;
    let rows = st.query_map(params![folder, uidvalidity], |r| r.get(0))?.collect::<rusqlite::Result<Vec<u32>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------- search text

/// The fields a search term can be limited to: `from:anna`, `subject:"year end"`.
const FIELDS: [(&str, &str); 5] = [("from", "from_addr"), ("to", "to_addr"), ("cc", "cc_addr"), ("subject", "subject"), ("body", "body")];
/// At most this many terms and characters of a search are used.
const MAX_TERMS: usize = 24;
const MAX_QUERY_CHARS: usize = 512;

/// Turn what a person typed into an FTS5 query that cannot be anything but text: plain words
/// and "quoted phrases" become quoted phrases (all must match), and `from:`, `to:`, `cc:`,
/// `subject:` and `body:` in front of a word or phrase limit it to that field. Everything else, including
/// `*`, `-`, `NEAR`, `AND`, `OR`, `NOT`, `:` and brackets, is plain text for the tokenizer.
/// None if there is nothing to search for.
pub fn fts_query(input: &str) -> Option<String> {
    let chars: Vec<char> = input.chars().take(MAX_QUERY_CHARS).collect();
    let mut terms: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() && terms.len() < MAX_TERMS {
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        let mut column: Option<&str> = None;
        let mut text = String::new();
        if chars[i] != '"' {
            let start = i;
            while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '"' {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            match word.split_once(':').and_then(|(f, rest)| FIELDS.iter().find(|(n, _)| f.eq_ignore_ascii_case(n)).map(|(_, col)| (*col, rest))) {
                Some((col, rest)) if i < chars.len() && chars[i] == '"' && rest.is_empty() => {
                    column = Some(col);
                }
                Some((col, rest)) => {
                    column = Some(col);
                    text = rest.to_string();
                }
                None => text = word,
            }
        }
        if i < chars.len() && chars[i] == '"' && text.is_empty() {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            text = chars[start..i].iter().collect();
            i += 1; // the closing quote (or the end)
        }
        if !text.chars().any(|c| c.is_alphanumeric()) {
            continue; // nothing the tokenizer would keep
        }
        let quoted = format!("\"{}\"", text.replace('"', "\"\""));
        terms.push(match column {
            Some(col) => format!("{col} : {quoted}"),
            None => quoted,
        });
    }
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c
    }

    pub fn fields(subject: &str, from: &str, body: &str, date: i64) -> IndexFields {
        IndexFields { date: Some(date), from: from.into(), to: "me@example.com".into(), subject: subject.into(), body: body.into(), ..Default::default() }
    }

    fn add(c: &mut Connection, folder: i64, uid: u32, f: IndexFields) {
        let m = NewMessage { folder, uidvalidity: 1, uid, path: format!("a/f/1/{uid}.eml"), state: STATE_FULL, flags: String::new(), size: 10, parsed: true, fields: f };
        apply_batch(c, &[Op::message(m)], None).unwrap();
    }

    fn inbox(c: &Connection) -> FolderRow {
        let f = upsert_folder(c, "a", "A", "INBOX", Some("/"), None, true).unwrap();
        c.execute("UPDATE folders SET uidvalidity = 1 WHERE id = ?1", [f.id]).unwrap();
        f
    }

    // spec: MAIL-015
    #[test]
    fn sqlite_is_bundled_with_fts5() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("CREATE VIRTUAL TABLE t USING fts5(x); INSERT INTO t VALUES ('hello world');").unwrap();
        let n: i64 = c.query_row("SELECT COUNT(*) FROM t WHERE t MATCH 'hello'", [], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
    }

    // spec: MAIL-004
    #[test]
    fn migration_1_creates_the_tables_and_is_repeatable() {
        let c = Connection::open_in_memory().unwrap();
        assert_eq!(current_version(&c).unwrap(), 0);
        migrate(&c).unwrap();
        assert_eq!(current_version(&c).unwrap(), SCHEMA_VERSION);
        migrate(&c).unwrap(); // nothing to do the second time
        for t in ["accounts", "folders", "messages", "mail_fts", "schema_version", "pending_ops"] {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = ?1", [t], |r| r.get(0)).unwrap();
            assert_eq!(n, 1, "table {t}");
        }
    }

    // spec: MAIL-004, MAIL-107
    #[test]
    fn migration_2_adds_the_pending_table_to_a_version_1_index_and_keeps_its_mail() {
        let c = Connection::open_in_memory().unwrap();
        current_version(&c).unwrap();
        c.execute_batch(MIGRATIONS[0]).unwrap();
        c.execute("INSERT INTO schema_version (version) VALUES (1)", []).unwrap();
        let mut c = c;
        let f = inbox(&c);
        add(&mut c, f.id, 1, fields("old mail", "a@x.de", "text", 5));
        migrate(&c).unwrap();
        assert_eq!(current_version(&c).unwrap(), 2);
        assert_eq!(count_messages(&c).unwrap(), 1, "the mail of version 1 is untouched");
        assert_eq!(list(&c, f.id, 10, 0).unwrap()[0].pending, None);
        assert!(pending_ops(&c, None).unwrap().is_empty());
    }

    // spec: MAIL-004
    #[test]
    fn a_newer_index_is_refused_and_left_alone() {
        let c = Connection::open_in_memory().unwrap();
        migrate(&c).unwrap();
        c.execute("UPDATE schema_version SET version = 99", []).unwrap();
        let e = migrate(&c).unwrap_err();
        assert!(e.0.contains("newer"), "{e}");
        assert_eq!(current_version(&c).unwrap(), 99);
    }

    // spec: MAIL-004
    #[test]
    fn the_writer_uses_wal_and_full_sync() {
        let dir = std::env::temp_dir().join(format!("shale-maildb-{}", std::process::id()));
        crate::fsutil::ensure_private_dir(&dir).unwrap();
        let p = dir.join("index.sqlite");
        let c = open_writer(&p).unwrap();
        let mode: String = c.query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        let sync: i64 = c.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
        assert_eq!((mode.as_str(), sync), ("wal", 2));
        drop(c);
        quick_check(&p).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    // spec: MAIL-005
    #[test]
    fn a_batch_cut_off_in_the_middle_leaves_nothing() {
        let mut c = db();
        let f = inbox(&c);
        let ops: Vec<Op> = (1..=5).map(|uid| Op::message(NewMessage { folder: f.id, uidvalidity: 1, uid, path: format!("{uid}.eml"), state: 0, flags: String::new(), size: 1, parsed: true, fields: IndexFields::default() })).collect();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| apply_batch(&mut c, &ops, Some(3))));
        assert!(r.is_err());
        assert_eq!(count_messages(&c).unwrap(), 0, "no partial batch");
        apply_batch(&mut c, &ops, None).unwrap();
        assert_eq!(count_messages(&c).unwrap(), 5);
    }

    // spec: MAIL-010
    #[test]
    fn search_ignores_case_and_accents_and_sorts_newest_first() {
        let mut c = db();
        let f = inbox(&c);
        add(&mut c, f.id, 1, fields("Grüße aus Köln", "Hans Müller <h@x.de>", "Das Büro bleibt zu", 100));
        add(&mut c, f.id, 2, fields("Re: Müller Angebot", "Anna <a@x.de>", "Angebot anbei", 200));
        add(&mut c, f.id, 3, fields("Other", "Bob <b@x.de>", "nothing here", 300));
        let hits = |q: &str| search(&c, q, None, 10, 0).unwrap().iter().map(|m| m.uid).collect::<Vec<_>>();
        assert_eq!(hits("muller"), vec![2, 1]);
        assert_eq!(hits("MÜLLER"), vec![2, 1]);
        assert_eq!(hits("buro"), vec![1]);
        assert_eq!(hits("grüße köln"), vec![1]);
        assert_eq!(hits("\"angebot anbei\""), vec![2]);
        assert_eq!(hits("zzz"), Vec::<u32>::new());
        assert_eq!(search(&c, "muller", Some(f.id + 1), 10, 0).unwrap().len(), 0);
        assert_eq!(search(&c, "muller", Some(f.id), 1, 1).unwrap().len(), 1, "paging");
    }

    // spec: MAIL-010
    #[test]
    fn attachment_names_and_cc_are_searchable() {
        let mut c = db();
        let f = inbox(&c);
        let mut x = fields("Invoice", "a@x.de", "see attached", 1);
        x.attachments = "Rechnung-2026.pdf".into();
        x.cc = "Carol <carol@x.de>".into();
        add(&mut c, f.id, 1, x);
        assert_eq!(search(&c, "rechnung", None, 10, 0).unwrap().len(), 1);
        assert_eq!(search(&c, "cc:carol", None, 10, 0).unwrap().len(), 1);
    }

    // spec: MAIL-012
    #[test]
    fn field_prefixes_limit_a_term_to_that_field() {
        let mut c = db();
        let f = inbox(&c);
        add(&mut c, f.id, 1, fields("Lunch plan", "Anna <anna@x.de>", "meet at noon with Bob", 1));
        add(&mut c, f.id, 2, fields("Anna's report", "Bob <bob@x.de>", "see numbers", 2));
        let hits = |q: &str| search(&c, q, None, 10, 0).unwrap().iter().map(|m| m.uid).collect::<Vec<_>>();
        assert_eq!(hits("anna"), vec![2, 1]);
        assert_eq!(hits("from:anna"), vec![1]);
        assert_eq!(hits("subject:anna"), vec![2]);
        assert_eq!(hits("subject:\"lunch plan\""), vec![1]);
        assert_eq!(hits("to:me"), vec![2, 1]);
        assert_eq!(hits("from:bob subject:report"), vec![2]);
        assert_eq!(hits("body:noon"), vec![1]);
        assert_eq!(hits("foo:noon"), Vec::<u32>::new(), "an unknown prefix is plain text, not a field");
    }

    // spec: MAIL-011
    #[test]
    fn hostile_search_text_is_plain_text() {
        let mut c = db();
        let f = inbox(&c);
        add(&mut c, f.id, 1, fields("near and or not", "a@x.de", "star* minus-sign colon:here \"quoted\"", 1));
        for q in [
            "\"",
            "\"\"",
            "*",
            "NEAR",
            "NEAR(a b)",
            "a AND",
            "OR",
            "NOT",
            "-",
            "-foo",
            "a:",
            ":",
            "subject:",
            "from:\"",
            "(",
            ")",
            "^a",
            "a*",
            "{a}",
            "x\"y",
            "\"unterminated phrase",
            "col:umn:x",
            "\u{0}",
            "'; DROP TABLE messages; --",
            "subject:subject:x",
            "a OR b",
            "a NOT b",
        ] {
            assert!(search(&c, q, None, 10, 0).is_ok(), "query {q:?} must not be an error");
        }
        assert_eq!(count_messages(&c).unwrap(), 1);
        let hits = |q: &str| search(&c, q, None, 10, 0).unwrap().len();
        assert_eq!(hits("near"), 1, "NEAR is a word");
        assert_eq!(hits("NEAR or"), 1);
        assert_eq!(hits("star*"), 1);
        assert_eq!(hits("minus-sign"), 1);
        assert_eq!(hits("colon:here"), 1, "a colon after an unknown word is text");
        assert_eq!(hits("\"quoted\""), 1);
        assert_eq!(hits("*"), 0);
        assert_eq!(hits("-"), 0);
    }

    // spec: MAIL-011
    #[test]
    fn the_query_builder_quotes_everything() {
        assert_eq!(fts_query("hello world").unwrap(), "\"hello\" \"world\"");
        assert_eq!(fts_query("\"a b\" c").unwrap(), "\"a b\" \"c\"");
        assert_eq!(fts_query("from:\"Anna M\"").unwrap(), "from_addr : \"Anna M\"");
        assert_eq!(fts_query("x\"y\"").unwrap(), "\"x\" \"y\"");
        assert_eq!(fts_query("NEAR/2"), Some("\"NEAR/2\"".into()));
        assert_eq!(fts_query("   "), None);
        assert_eq!(fts_query("\" * - \""), None);
        let long = "word ".repeat(1000);
        assert!(fts_query(&long).unwrap().split(' ').count() <= MAX_TERMS);
    }

    // spec: MAIL-013
    #[test]
    fn the_list_is_newest_first_in_pages_and_hides_an_old_uidvalidity_copy_the_current_one_has() {
        let mut c = db();
        let f = inbox(&c);
        for uid in 1..=5u32 {
            let mut fl = fields("s", "a@x.de", "b", uid as i64 * 10);
            fl.message_id = format!("<m{uid}@x>");
            add(&mut c, f.id, uid, fl);
        }
        // a copy from before a UIDVALIDITY change that the current number has as well
        let mut ofl = fields("old", "a@x.de", "b", 999);
        ofl.message_id = "<m3@x>".into();
        let old = NewMessage { folder: f.id, uidvalidity: 7, uid: 9, path: "old.eml".into(), state: 0, flags: String::new(), size: 1, parsed: true, fields: ofl };
        apply_batch(&mut c, &[Op::message(old)], None).unwrap();
        {
            let page = |off| list(&c, f.id, 2, off).unwrap().iter().map(|m| m.uid).collect::<Vec<_>>();
            assert_eq!(page(0), vec![5, 4]);
            assert_eq!(page(2), vec![3, 2]);
            assert_eq!(page(4), vec![1]);
            assert_eq!(page(6), Vec::<u32>::new());
            assert!(search(&c, "old", None, 10, 0).unwrap().is_empty());
        }
        // a local copy of sent mail (UIDVALIDITY 0) always shows
        let local = NewMessage { folder: f.id, uidvalidity: LOCAL_UIDVALIDITY, uid: 4_000_000_000, path: "0/x.eml".into(), state: 0, flags: String::new(), size: 1, parsed: true, fields: fields("mine", "a@x.de", "b", 1000) };
        apply_batch(&mut c, &[Op::message(local)], None).unwrap();
        let first = list(&c, f.id, 2, 0).unwrap().iter().map(|m| m.uid).collect::<Vec<_>>();
        assert_eq!(first, vec![4_000_000_000, 5]);
    }

    fn old_copy(folder: i64, uid: u32, mid: &str, date: i64) -> Op {
        let mut fl = fields(&format!("old {uid}"), "a@x.de", "kept body", date);
        fl.message_id = mid.into();
        Op::message(NewMessage { folder, uidvalidity: 7, uid, path: format!("a/f/7/{uid}.eml"), state: 0, flags: String::new(), size: 1, parsed: true, fields: fl })
    }

    // spec: MAIL-204
    #[test]
    fn a_new_uidvalidity_keeps_older_mail_visible_as_gone_unless_the_current_number_has_it() {
        let mut c = db();
        let f = inbox(&c); // number 1
        apply_batch(&mut c, &[old_copy(f.id, 1, "<a@x>", 10), old_copy(f.id, 2, "<b@x>", 20), old_copy(f.id, 3, "", 30)], None).unwrap();
        // the old ones are under number 7, the folder is at 1: nothing renumbered yet, so they show but are not gone
        c.execute("UPDATE messages SET uidvalidity = 1 WHERE uidvalidity = 7", []).unwrap();
        apply_batch(&mut c, &[Op::FolderState { folder: f.id, uidvalidity: Some(2), uidnext: None, highestmodseq: None, last_sync: 0 }], None).unwrap();
        let mut fl = fields("again", "a@x.de", "b", 21);
        fl.message_id = "<b@x>".into();
        let m = NewMessage { folder: f.id, uidvalidity: 2, uid: 1, path: "a/f/2/1.eml".into(), state: 0, flags: String::new(), size: 1, parsed: true, fields: fl };
        apply_batch(&mut c, &[Op::message(m)], None).unwrap();
        let shown = list(&c, f.id, 10, 0).unwrap();
        let subjects: Vec<&str> = shown.iter().map(|m| m.subject.as_str()).collect();
        assert_eq!(subjects, vec!["old 3", "again", "old 1"], "the old copy of <b@x> is hidden, the one without a Message-ID stays");
        assert!(shown.iter().filter(|m| m.uidvalidity == 1).all(|m| m.gone_on_server));
        assert!(!shown[1].gone_on_server);
        assert_eq!(search(&c, "kept", None, 10, 0).unwrap().len(), 2);
        assert_eq!(count_folder(&c, f.id).unwrap(), 3);
        assert_eq!(folder_counts(&c).unwrap()[&f.id].0, 3);
        for (i, m) in shown.iter().enumerate() {
            assert_eq!(position_in_list(&c, f.id, m.date, m.uid).unwrap(), i as i64);
        }
        // a folder that had no number yet: rows of other numbers are old for it, too (a rebuilt index)
        let g = upsert_folder(&c, "a", "A", "Other", Some("/"), None, true).unwrap();
        apply_batch(&mut c, &[old_copy(g.id, 1, "<z@x>", 5)], None).unwrap();
        apply_batch(&mut c, &[Op::FolderState { folder: g.id, uidvalidity: Some(3), uidnext: None, highestmodseq: None, last_sync: 0 }], None).unwrap();
        assert!(list(&c, g.id, 10, 0).unwrap().iter().all(|m| m.gone_on_server), "rows of number 7 are old for number 3, too");
    }

    // spec: MAIL-204
    #[test]
    fn the_list_of_a_folder_with_old_and_new_rows_stays_fast() {
        let mut c = db();
        let f = inbox(&c);
        let mut ops = Vec::new();
        for uid in 1..=4000u32 {
            let mut fl = fields("s", "a@x.de", "b", uid as i64);
            fl.message_id = format!("<m{uid}@x>");
            ops.push(Op::message(NewMessage { folder: f.id, uidvalidity: 7, uid, path: format!("o/{uid}.eml"), state: 0, flags: String::new(), size: 1, parsed: true, fields: fl.clone() }));
            if uid % 2 == 0 {
                ops.push(Op::message(NewMessage { folder: f.id, uidvalidity: 1, uid, path: format!("n/{uid}.eml"), state: 0, flags: String::new(), size: 1, parsed: true, fields: fl }));
            }
        }
        apply_batch(&mut c, &ops, None).unwrap();
        let t = std::time::Instant::now();
        let page = list(&c, f.id, 50, 0).unwrap();
        let deep = list(&c, f.id, 50, 3950).unwrap();
        let n = count_folder(&c, f.id).unwrap();
        let took = t.elapsed();
        assert_eq!(page.len(), 50);
        assert_eq!(deep.len(), 50);
        assert_eq!(n, 4000, "2000 old ones the server has again are hidden, 2000 new and 2000 only-old show");
        assert!(took < std::time::Duration::from_secs(2), "took {took:?}");
        eprintln!("list+deep page+count over 6000 rows: {took:?}");
        let plan: Vec<String> = {
            let mut st = c.prepare(&format!("EXPLAIN QUERY PLAN SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder AND {VISIBLE} WHERE m.folder = 1 ORDER BY m.date DESC, m.uid DESC LIMIT 50")).unwrap();
            st.query_map([], |r| r.get::<_, String>(3)).unwrap().map(|r| r.unwrap()).collect()
        };
        assert!(plan.iter().any(|p| p.contains("messages_by_message_id")), "the duplicate check uses the Message-ID index: {plan:?}");
    }

    #[test]
    fn upserting_a_message_replaces_its_search_text() {
        let mut c = db();
        let f = inbox(&c);
        add(&mut c, f.id, 1, fields("first", "a@x.de", "alpha", 1));
        add(&mut c, f.id, 1, fields("second", "a@x.de", "beta", 1));
        assert_eq!(count_messages(&c).unwrap(), 1);
        assert!(search(&c, "alpha", None, 10, 0).unwrap().is_empty());
        assert_eq!(search(&c, "beta", None, 10, 0).unwrap().len(), 1);
    }
}
