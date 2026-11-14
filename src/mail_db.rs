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
pub const SCHEMA_VERSION: i64 = 1;

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
const MIGRATIONS: [&str; 1] = [r#"
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
"#];

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
}

impl MessageSummary {
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
    Flags { folder: i64, uidvalidity: u32, uid: u32, flags: String },
    Gone { folder: i64, uidvalidity: u32, uid: u32, gone: bool },
    MissingFile { id: i64, missing: bool },
    FolderState { folder: i64, uidvalidity: Option<u32>, uidnext: Option<u32>, highestmodseq: Option<u64>, last_sync: i64 },
    SyncEnabled { folder: i64, enabled: bool },
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
            tx.execute("UPDATE folders SET uidvalidity = ?2, uidnext = ?3, highestmodseq = ?4, last_sync = ?5 WHERE id = ?1", params![folder, uidvalidity, uidnext, highestmodseq.map(|m| m as i64), last_sync])?;
        }
        Op::SyncEnabled { folder, enabled } => {
            tx.execute("UPDATE folders SET sync_enabled = ?2 WHERE id = ?1", params![folder, *enabled as i64])?;
        }
    }
    Ok(())
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
    "m.id, f.account, m.folder, f.name, m.uid, m.uidvalidity, m.path, m.state, m.flags, m.date, m.from_addr, m.to_addr, m.cc_addr, m.subject, m.message_id, m.size, m.has_attachments, m.parsed, m.gone_on_server, m.missing_file";

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
    })
}

/// A page of a folder's messages, newest first. Only the folder's current UIDVALIDITY shows:
/// messages of an older one are copies the sync has fetched again (their files stay on disk).
pub fn list(c: &Connection, folder: i64, limit: usize, offset: usize) -> Result<Vec<MessageSummary>> {
    let mut st = c.prepare(&format!(
        "SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder AND f.uidvalidity = m.uidvalidity
         WHERE m.folder = ?1 ORDER BY m.date DESC, m.uid DESC LIMIT ?2 OFFSET ?3"
    ))?;
    let rows = st.query_map(params![folder, limit as i64, offset as i64], summary_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Search (see [`fts_query`] for what is understood), newest first. `folder` limits it to one folder.
pub fn search(c: &Connection, query: &str, folder: Option<i64>, limit: usize, offset: usize) -> Result<Vec<MessageSummary>> {
    let Some(q) = fts_query(query) else { return Ok(vec![]) };
    let mut st = c.prepare(&format!(
        "SELECT {MSG_COLS} FROM mail_fts JOIN messages m ON m.id = mail_fts.rowid JOIN folders f ON f.id = m.folder AND f.uidvalidity = m.uidvalidity
         WHERE mail_fts MATCH ?1 AND (?2 IS NULL OR m.folder = ?2) ORDER BY m.date DESC, m.uid DESC LIMIT ?3 OFFSET ?4"
    ))?;
    let rows = st.query_map(params![q, folder, limit as i64, offset as i64], summary_from)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// One message by folder and UID in the folder's current UIDVALIDITY.
pub fn message(c: &Connection, folder: i64, uid: u32) -> Result<Option<MessageSummary>> {
    Ok(c.query_row(&format!("SELECT {MSG_COLS} FROM messages m JOIN folders f ON f.id = m.folder AND f.uidvalidity = m.uidvalidity WHERE m.folder = ?1 AND m.uid = ?2"), params![folder, uid], summary_from).optional()?)
}

/// Per folder: (messages shown in its list, how many of them are unread). One pass over the
/// index; the mail view asks only when something changed.
pub fn folder_counts(c: &Connection) -> Result<HashMap<i64, (i64, i64)>> {
    let mut st = c.prepare(
        "SELECT m.folder, COUNT(*), SUM(CASE WHEN instr(lower(' ' || m.flags || ' '), ' \\seen ') = 0 THEN 1 ELSE 0 END)
         FROM messages m JOIN folders f ON f.id = m.folder AND f.uidvalidity = m.uidvalidity GROUP BY m.folder",
    )?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?.unwrap_or(0)))))?.collect::<rusqlite::Result<HashMap<_, _>>>()?;
    Ok(rows)
}

/// How many messages a folder's list has.
pub fn count_folder(c: &Connection, folder: i64) -> Result<i64> {
    Ok(c.query_row("SELECT COUNT(*) FROM messages m JOIN folders f ON f.id = m.folder AND f.uidvalidity = m.uidvalidity WHERE m.folder = ?1", [folder], |r| r.get(0))?)
}

/// How many messages of the folder's list come before this one (its row number in the list).
pub fn position_in_list(c: &Connection, folder: i64, date: Option<i64>, uid: u32) -> Result<i64> {
    let d = date.unwrap_or(i64::MIN);
    Ok(c.query_row(
        "SELECT COUNT(*) FROM messages m JOIN folders f ON f.id = m.folder AND f.uidvalidity = m.uidvalidity
         WHERE m.folder = ?1 AND (COALESCE(m.date, -9223372036854775807) > ?2 OR (COALESCE(m.date, -9223372036854775807) = ?2 AND m.uid > ?3))",
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
        assert_eq!(current_version(&c).unwrap(), 1);
        migrate(&c).unwrap(); // nothing to do the second time
        for t in ["accounts", "folders", "messages", "mail_fts", "schema_version"] {
            let n: i64 = c.query_row("SELECT COUNT(*) FROM sqlite_master WHERE name = ?1", [t], |r| r.get(0)).unwrap();
            assert_eq!(n, 1, "table {t}");
        }
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
    fn the_list_is_newest_first_in_pages_and_shows_the_current_uidvalidity_only() {
        let mut c = db();
        let f = inbox(&c);
        for uid in 1..=5u32 {
            add(&mut c, f.id, uid, fields("s", "a@x.de", "b", uid as i64 * 10));
        }
        // a copy from before a UIDVALIDITY change
        let old = NewMessage { folder: f.id, uidvalidity: 0, uid: 9, path: "old.eml".into(), state: 0, flags: String::new(), size: 1, parsed: true, fields: fields("old", "a@x.de", "b", 999) };
        apply_batch(&mut c, &[Op::message(old)], None).unwrap();
        let page = |off| list(&c, f.id, 2, off).unwrap().iter().map(|m| m.uid).collect::<Vec<_>>();
        assert_eq!(page(0), vec![5, 4]);
        assert_eq!(page(2), vec![3, 2]);
        assert_eq!(page(4), vec![1]);
        assert_eq!(page(6), Vec::<u32>::new());
        assert!(search(&c, "old", None, 10, 0).unwrap().is_empty());
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
