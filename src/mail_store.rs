//! The local mail store: one `.eml` file per message plus the SQLite index (ADR-0007 §2, §3a).
//!
//! ```text
//! <root>/                          ~/.local/share/shale/mail, mode 0700
//!   index.sqlite                   the index (a cache: it can be rebuilt from the files)
//!   index.backup-<date>.sqlite     daily copies, the newest 3
//!   index.sqlite.broken-<date>     a damaged index, kept (never deleted)
//!   <account>/<folder>/<uidvalidity>/<uid>.eml
//! ```
//!
//! Safety, in the order things happen to a message: the `.eml` is written atomically (temp file,
//! fsync, rename, fsync of the folder) and only then its row is inserted, so a crash leaves a
//! file without a row, never a row without a file. One writer thread owns the index connection
//! and applies each batch in one transaction. When the store is opened the index is checked;
//! a damaged one is moved aside, replaced by the newest good backup or by a new one, and then
//! [`reconcile`] indexes every file that has no row and marks rows whose file is gone, so a
//! rebuild loses nothing but flags (the next sync brings them back from the server).
//!
//! Nothing here deletes a message file. A message that moved or was deleted by the person goes
//! to `.trash/<date>/` ([`MailStore::archive_file`]).

use crate::fsutil;
use crate::mail;
use crate::mail_db::{self, FolderRow, IndexFields, MailError, MessageSummary, NewMessage, Op, Result, STATE_FULL, STATE_HEADER, STATE_TOO_LARGE};
use rusqlite::Connection;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread::JoinHandle;

/// Text of the body that goes into the search index (bytes; the rest of a huge mail is not searched).
const MAX_BODY_INDEX: usize = 1 << 20;
/// How many index rows one transaction takes when rebuilding.
const RECONCILE_BATCH: usize = 200;
/// Backups kept.
const BACKUPS_KEPT: usize = 3;
/// Longest folder directory name; longer folder names use a hash (the real name is in `name.txt`).
const MAX_DIR_NAME: usize = 150;

/// Where the mail store lives by default (`~/.local/share/shale/mail`).
pub fn default_root() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("mail")
}

/// Today as `YYYY-MM-DD` (local time), for names of backups and quarantined files.
pub fn today_string() -> String {
    let (y, m, d) = crate::cal_time::civil_from_days(crate::cal_time::today());
    format!("{y:04}-{m:02}-{d:02}")
}

// ---------------------------------------------------------------- names on disk

/// An account id is used as a folder name: letters, digits, `-` and `_` only. `drafts` and `outbox`
/// are the folders of phase 3 (ADR-0010) and cannot be account ids.
pub fn valid_account_id(id: &str) -> bool {
    !id.is_empty() && !matches!(id, "drafts" | "outbox") && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The directory name of an IMAP folder: the name with everything but letters, digits, `-` and
/// `_` written as `%XX`, so it is safe on every file system and can be turned back into the name.
pub fn folder_dir_name(name: &str) -> String {
    let mut out = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    if out.is_empty() {
        out.push('_');
    }
    if out.len() > MAX_DIR_NAME {
        // FNV-1a of the name: not reversible, so `name.txt` in the folder keeps the real name
        let mut h: u64 = 0xcbf29ce484222325;
        for b in name.bytes() {
            h = (h ^ b as u64).wrapping_mul(0x100000001b3);
        }
        out = format!("long-{h:016x}");
    }
    out
}

/// The folder name for a directory name made by [`folder_dir_name`] (None for a `long-` one).
pub fn folder_name_from_dir(dir: &str) -> Option<String> {
    if dir.starts_with("long-") || dir == "_" {
        return None;
    }
    let b = dir.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(h, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Path of a message file relative to the store: `<account>/<folder>/<uidvalidity>/<uid>.eml`.
pub fn eml_rel_path(account: &str, folder_name: &str, uidvalidity: u32, uid: u32) -> String {
    format!("{account}/{}/{uidvalidity}/{uid}.eml", folder_dir_name(folder_name))
}

// ---------------------------------------------------------------- parsing for the index

/// What the index keeps of a raw message, and whether it could be read at all. This never
/// panics: whatever the bytes are, the answer is fields (empty ones if need be) and `false`.
pub fn index_fields(raw: &[u8], fallback_date: Option<i64>) -> (IndexFields, bool) {
    match crate::jobs::catch(|| mail::parse(raw)) {
        Ok(m) => {
            let readable = !(m.from.is_empty() && m.subject.is_empty() && m.date_raw.is_empty() && m.message_id.is_empty());
            let names: Vec<&str> = m.attachments.iter().filter(|a| !a.inline).map(|a| a.name.as_str()).filter(|n| !n.is_empty()).collect();
            let fields = IndexFields {
                date: m.date.or(fallback_date),
                from: m.from.clone(),
                to: m.to.clone(),
                cc: m.cc.clone(),
                subject: if readable { m.subject.clone() } else { COULD_NOT_PARSE.to_string() },
                message_id: m.message_id.clone(),
                has_attachments: m.attachments.iter().any(|a| !a.inline),
                body: cut_text(&m.text, MAX_BODY_INDEX),
                attachments: names.join(" "),
            };
            (fields, readable)
        }
        Err(_) => (IndexFields { date: fallback_date, subject: COULD_NOT_PARSE.into(), ..Default::default() }, false),
    }
}

/// The subject shown for a message that could not be read.
pub const COULD_NOT_PARSE: &str = "(could not parse this message)";

/// At most `max` bytes of `s`, cut at a character boundary.
fn cut_text(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

// ---------------------------------------------------------------- the store

enum Cmd {
    Batch { ops: Vec<Op>, crash_after: Option<usize>, reply: Sender<Result<()>> },
    Folder { account: String, display: String, name: String, delimiter: Option<String>, special_use: Option<String>, sync_enabled: bool, reply: Sender<Result<FolderRow>> },
    Backup { date: String, reply: Sender<Result<Option<PathBuf>>> },
    Stop,
}

/// The mail store: files, the index and its one writer thread. Share it with `Arc`.
pub struct MailStore {
    root: PathBuf,
    tx: Sender<Cmd>,
    writer: Mutex<Option<JoinHandle<()>>>,
    /// What opening found and repaired.
    pub opened: OpenReport,
}

/// What happened when the store was opened.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpenReport {
    /// A damaged index was moved here.
    pub quarantined: Option<PathBuf>,
    /// The backup the index was restored from.
    pub restored_from: Option<PathBuf>,
    /// Files without a row that were indexed.
    pub indexed_files: usize,
    /// Rows whose file is gone (they are fetched again by the next sync).
    pub missing_files: usize,
}

impl MailStore {
    /// Open (or create) the store at `root`. Checks and if need be repairs the index and makes
    /// it match the files (see the module comment). Can take a while for a big store: call it
    /// from a background job.
    pub fn open(root: &Path) -> Result<MailStore> {
        fsutil::ensure_private_dir(root)?;
        let mut report = OpenReport::default();
        let mut conn = prepare_index(root, &today_string(), &mut report)?;
        let (indexed, missing) = reconcile(&mut conn, root)?;
        report.indexed_files = indexed;
        report.missing_files = missing;
        let (tx, rx) = channel::<Cmd>();
        let writer_root = root.to_path_buf();
        let handle = std::thread::Builder::new().name("mail-index-writer".into()).spawn(move || writer_loop(conn, writer_root, rx)).map_err(|e| MailError(e.to_string()))?;
        Ok(MailStore { root: root.to_path_buf(), tx, writer: Mutex::new(Some(handle)), opened: report })
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join("index.sqlite")
    }

    /// The full path of a message file given its relative path from the index.
    pub fn abs_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// A new read-only connection for listing and searching (one per thread).
    pub fn reader(&self) -> Result<Reader> {
        Ok(Reader { conn: mail_db::open_reader(&self.index_path())? })
    }

    fn ask<T>(&self, make: impl FnOnce(Sender<Result<T>>) -> Cmd) -> Result<T> {
        let (reply, answer) = channel();
        self.tx.send(make(reply)).map_err(|_| MailError("the mail index writer has stopped".into()))?;
        answer.recv().map_err(|_| MailError("the mail index writer has stopped".into()))?
    }

    /// Apply a batch of changes in one transaction; returns when it is committed (durable).
    pub fn commit(&self, ops: Vec<Op>) -> Result<()> {
        if ops.is_empty() {
            return Ok(());
        }
        self.ask(|reply| Cmd::Batch { ops, crash_after: None, reply })
    }

    /// Test aid: a batch that "crashes" (panics inside the transaction) after `n` operations.
    #[cfg(test)]
    pub fn commit_crashing(&self, ops: Vec<Op>, n: usize) -> Result<()> {
        self.ask(|reply| Cmd::Batch { ops, crash_after: Some(n), reply })
    }

    /// Make sure the account and the folder are in the index; returns the folder's row.
    pub fn ensure_folder(&self, account: &str, display: &str, name: &str, delimiter: Option<&str>, special_use: Option<&str>, sync_enabled: bool) -> Result<FolderRow> {
        if !valid_account_id(account) {
            return Err(MailError(format!("not a valid account id: {account:?}")));
        }
        self.ask(|reply| Cmd::Folder { account: account.into(), display: display.into(), name: name.into(), delimiter: delimiter.map(String::from), special_use: special_use.map(String::from), sync_enabled, reply })
    }

    /// Make a backup of the index if there is none from `date` yet (`YYYY-MM-DD`); keeps the
    /// newest 3. Returns the new backup's path, or None if one existed.
    pub fn backup(&self, date: &str) -> Result<Option<PathBuf>> {
        self.ask(|reply| Cmd::Backup { date: date.into(), reply })
    }

    /// The daily backup: today's, if there is none yet. Called after a sync that finished well.
    pub fn backup_if_due(&self) -> Result<Option<PathBuf>> {
        self.backup(&today_string())
    }

    /// Write a message file (atomically, durably) and describe it for the index. The row is
    /// only added when the caller commits the returned message, so the file is always first.
    pub fn save_message(&self, folder: &FolderRow, uidvalidity: u32, uid: u32, raw: &[u8], flags: &str, internal_date: Option<i64>) -> Result<NewMessage> {
        let rel = eml_rel_path(&folder.account, &folder.name, uidvalidity, uid);
        let path = self.root.join(&rel);
        let dir = path.parent().ok_or("bad message path")?;
        fsutil::ensure_private_dir(dir)?;
        self.remember_folder_name(folder)?;
        fsutil::write_atomic(&path, raw)?;
        let (fields, parsed) = index_fields(raw, internal_date);
        Ok(NewMessage { folder: folder.id, uidvalidity, uid, path: rel, state: STATE_FULL, flags: flags.into(), size: raw.len() as u64, parsed, fields })
    }

    /// An index row for a message of which only the headers are known (a listing of Trash).
    pub fn header_only(&self, folder: &FolderRow, uidvalidity: u32, uid: u32, headers: &[u8], flags: &str, internal_date: Option<i64>, size: u64) -> NewMessage {
        let (fields, parsed) = index_fields(headers, internal_date);
        NewMessage { folder: folder.id, uidvalidity, uid, path: String::new(), state: STATE_HEADER, flags: flags.into(), size, parsed, fields: IndexFields { body: String::new(), ..fields } }
    }

    /// An index row for a message that is over the size limit and was not downloaded.
    pub fn too_large(&self, folder: &FolderRow, uidvalidity: u32, uid: u32, flags: &str, internal_date: Option<i64>, size: u64) -> NewMessage {
        let fields = IndexFields { date: internal_date, subject: "(too large to download)".into(), ..Default::default() };
        NewMessage { folder: folder.id, uidvalidity, uid, path: String::new(), state: STATE_TOO_LARGE, flags: flags.into(), size, parsed: false, fields }
    }

    /// `name.txt` in a folder's directory holds the real folder name, so the store can be
    /// rebuilt even where the directory name is a hash.
    fn remember_folder_name(&self, folder: &FolderRow) -> Result<()> {
        let dir = self.root.join(&folder.account).join(folder_dir_name(&folder.name));
        let file = dir.join("name.txt");
        if !file.exists() {
            fsutil::write_atomic(&file, folder.name.as_bytes())?;
        }
        Ok(())
    }

    /// Put a message file out of the way without deleting it (MAIL-106): it moves, by
    /// `fsutil::rename_noreplace`, to `<root>/.trash/<date>/<rel>` (a number is added to the name
    /// if that exists). Returns the new place, or None if there was no such file.
    pub fn archive_file(&self, rel: &str, date: &str) -> Result<Option<PathBuf>> {
        let rel_path = Path::new(rel);
        if rel.is_empty() || rel_path.is_absolute() || rel_path.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
            return Err(MailError(format!("not a path inside the mail store: {rel:?}")));
        }
        let src = self.root.join(rel_path);
        if !src.is_file() {
            return Ok(None);
        }
        let mut dest = self.root.join(".trash").join(date).join(rel_path);
        fsutil::ensure_private_dir(dest.parent().ok_or("bad path")?)?;
        for n in 2..1000 {
            match fsutil::rename_noreplace(&src, &dest) {
                Ok(()) => return Ok(Some(dest)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stem = rel_path.file_stem().and_then(|s| s.to_str()).unwrap_or("message");
                    dest = dest.with_file_name(format!("{stem}-{n}.eml"));
                }
                Err(e) => return Err(e.into()),
            }
        }
        Err(MailError("too many files with that name in the local trash".into()))
    }

    /// Read a message file.
    pub fn read_raw(&self, m: &MessageSummary) -> Result<Vec<u8>> {
        if m.path.is_empty() {
            return Err(MailError("this message has not been downloaded".into()));
        }
        Ok(fsutil::read(self.abs_path(&m.path))?)
    }

    /// Read and parse a message.
    pub fn read_mail(&self, m: &MessageSummary) -> Result<mail::Mail> {
        let raw = self.read_raw(m)?;
        crate::jobs::catch(|| mail::parse(&raw)).map_err(MailError)
    }
}

impl Drop for MailStore {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Stop);
        if let Some(h) = self.writer.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = h.join();
        }
    }
}

/// A read-only view of the index for one thread.
pub struct Reader {
    conn: Connection,
}

impl Reader {
    pub fn conn(&self) -> &Connection {
        &self.conn
    }
    pub fn folders(&self, account: Option<&str>) -> Result<Vec<FolderRow>> {
        mail_db::folders(&self.conn, account)
    }
    /// A page of a folder, newest first (MAIL-013).
    pub fn list(&self, folder: i64, limit: usize, offset: usize) -> Result<Vec<MessageSummary>> {
        mail_db::list(&self.conn, folder, limit, offset)
    }
    /// Search (MAIL-010…MAIL-012); `folder` limits it to one folder.
    pub fn search(&self, query: &str, folder: Option<i64>, limit: usize, offset: usize) -> Result<Vec<MessageSummary>> {
        mail_db::search(&self.conn, query, folder, limit, offset)
    }
    pub fn message(&self, folder: i64, uid: u32) -> Result<Option<MessageSummary>> {
        mail_db::message(&self.conn, folder, uid)
    }
    #[cfg(test)]
    pub fn count_messages(&self) -> Result<i64> {
        mail_db::count_messages(&self.conn)
    }
    /// (messages, unread) per folder id (MAIL-051).
    pub fn folder_counts(&self) -> Result<std::collections::HashMap<i64, (i64, i64)>> {
        mail_db::folder_counts(&self.conn)
    }
    pub fn count_folder(&self, folder: i64) -> Result<i64> {
        mail_db::count_folder(&self.conn, folder)
    }
    /// The row number of a message in its folder's list.
    pub fn position_in_list(&self, folder: i64, date: Option<i64>, uid: u32) -> Result<i64> {
        mail_db::position_in_list(&self.conn, folder, date, uid)
    }
    pub fn present_uids(&self, folder: i64, uidvalidity: u32) -> Result<HashSet<u32>> {
        mail_db::present_uids(&self.conn, folder, uidvalidity)
    }
    pub fn flags_of(&self, folder: i64, uidvalidity: u32) -> Result<std::collections::HashMap<u32, (String, bool)>> {
        mail_db::flags_of(&self.conn, folder, uidvalidity)
    }
    pub fn missing_uids(&self, folder: i64, uidvalidity: u32) -> Result<Vec<u32>> {
        mail_db::missing_uids(&self.conn, folder, uidvalidity)
    }
    /// The actions waiting for the server, oldest first (MAIL-107).
    pub fn pending_ops(&self, account: Option<&str>) -> Result<Vec<mail_db::PendingRow>> {
        mail_db::pending_ops(&self.conn, account)
    }
    pub fn pending_uids(&self, folder: i64, uidvalidity: u32) -> Result<HashSet<u32>> {
        mail_db::pending_uids(&self.conn, folder, uidvalidity)
    }
    pub fn message_by_id(&self, id: i64) -> Result<Option<MessageSummary>> {
        mail_db::message_by_id(&self.conn, id)
    }
    pub fn message_at(&self, folder: i64, uidvalidity: u32, uid: u32) -> Result<Option<MessageSummary>> {
        mail_db::message_at(&self.conn, folder, uidvalidity, uid)
    }
    pub fn folder_by_id(&self, id: i64) -> Result<Option<FolderRow>> {
        mail_db::folder_by_id(&self.conn, id)
    }
}

// ---------------------------------------------------------------- the writer thread

fn writer_loop(mut conn: Connection, root: PathBuf, rx: std::sync::mpsc::Receiver<Cmd>) {
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Stop => break,
            Cmd::Batch { ops, crash_after, reply } => {
                let r = crate::jobs::catch(|| mail_db::apply_batch(&mut conn, &ops, crash_after)).unwrap_or_else(|e| Err(MailError(e)));
                let _ = reply.send(r);
            }
            Cmd::Folder { account, display, name, delimiter, special_use, sync_enabled, reply } => {
                let r = mail_db::upsert_folder(&conn, &account, &display, &name, delimiter.as_deref(), special_use.as_deref(), sync_enabled);
                let _ = reply.send(r);
            }
            Cmd::Backup { date, reply } => {
                let r = crate::jobs::catch(|| make_backup(&conn, &root, &date)).unwrap_or_else(|e| Err(MailError(e)));
                let _ = reply.send(r);
            }
        }
    }
}

// ---------------------------------------------------------------- start-up check and repair

fn backup_name(date: &str) -> String {
    format!("index.backup-{date}.sqlite")
}

/// The backups in `root`, newest first (the date is in the name).
fn backups(root: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> =
        std::fs::read_dir(root).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with("index.backup-") && n.ends_with(".sqlite")).unwrap_or(false)).collect()).unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// `VACUUM INTO` a temporary name, check the copy, then give it its final name. The copy only
/// replaces nothing: an existing backup of that day stays. Backups older than the newest 3 are
/// deleted: they are copies of a cache that can be rebuilt from the message files, and keeping
/// them in a trash folder would add a whole index every day, forever.
fn make_backup(conn: &Connection, root: &Path, date: &str) -> Result<Option<PathBuf>> {
    let dest = root.join(backup_name(date));
    if dest.exists() {
        return Ok(None);
    }
    let tmp = root.join(format!("{}.tmp", backup_name(date)));
    let _ = std::fs::remove_file(&tmp);
    let tmp_str = tmp.to_str().ok_or("the mail folder's name is not valid text")?;
    let result = conn.execute("VACUUM INTO ?1", [tmp_str]).map_err(MailError::from).and_then(|_| Ok(fsutil::sync_file(&tmp)?)).and_then(|_| mail_db::quick_check(&tmp));
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    fsutil::rename_noreplace(&tmp, &dest)?;
    for old in backups(root).into_iter().skip(BACKUPS_KEPT) {
        let _ = std::fs::remove_file(old);
    }
    Ok(Some(dest))
}

/// A name for a damaged file that does not exist yet: `<file>.broken-<date>`, `-2`, `-3` …
fn broken_name(root: &Path, file: &str, date: &str) -> PathBuf {
    let first = root.join(format!("{file}.broken-{date}"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| root.join(format!("{file}.broken-{date}-{n}"))).find(|p| !p.exists()).unwrap_or(first)
}

/// Check the index and give back an open writer connection, repairing what is needed. Nothing
/// is ever deleted: a damaged index is renamed.
fn prepare_index(root: &Path, date: &str, report: &mut OpenReport) -> Result<Connection> {
    let idx = root.join("index.sqlite");
    let exists_and_used = idx.metadata().map(|m| m.len() > 0).unwrap_or(false);
    if exists_and_used {
        if let Err(why) = mail_db::quick_check(&idx) {
            eprintln!("mail index damaged ({why}); keeping it aside and rebuilding");
            let kept = quarantine(root, date)?;
            report.quarantined = Some(kept);
            report.restored_from = restore_backup(root, &idx);
        }
    }
    // a newer index (or any other failure to open) is reported, not "repaired"
    mail_db::open_writer(&idx)
}

/// Rename the damaged index (and its WAL files) so nothing is lost; returns the new name.
fn quarantine(root: &Path, date: &str) -> Result<PathBuf> {
    let main = broken_name(root, "index.sqlite", date);
    let suffix = main.file_name().and_then(|n| n.to_str()).and_then(|n| n.strip_prefix("index.sqlite")).unwrap_or(".broken").to_string();
    fsutil::rename_noreplace(&root.join("index.sqlite"), &main)?;
    for side in ["-wal", "-shm"] {
        let from = root.join(format!("index.sqlite{side}"));
        if from.exists() {
            fsutil::rename_noreplace(&from, &root.join(format!("index.sqlite{side}{suffix}")))?;
        }
    }
    Ok(main)
}

/// Copy the newest backup that passes the check to `index.sqlite`; None if there is none.
fn restore_backup(root: &Path, idx: &Path) -> Option<PathBuf> {
    backups(root).into_iter().find(|b| mail_db::quick_check(b).is_ok() && fsutil::copy_new(b, idx).is_ok())
}

// ---------------------------------------------------------------- files and rows agree

/// Make the index match the files: files without a row are indexed, rows whose file is gone are
/// marked `missing_file` (and unmarked when the file is back). Returns (indexed, missing).
pub fn reconcile(conn: &mut Connection, root: &Path) -> Result<(usize, usize)> {
    let indexed = index_unknown_files(conn, root)?;
    let missing = mark_missing_files(conn, root)?;
    Ok((indexed, missing))
}

fn sorted_dir(path: &Path) -> Vec<(String, PathBuf)> {
    let mut v: Vec<(String, PathBuf)> = std::fs::read_dir(path).map(|rd| rd.flatten().filter_map(|e| Some((e.file_name().to_str()?.to_string(), e.path()))).collect()).unwrap_or_default();
    v.sort();
    v
}

/// Index every `<account>/<folder>/<uidvalidity>/<uid>.eml` that has no row.
fn index_unknown_files(conn: &mut Connection, root: &Path) -> Result<usize> {
    let mut total = 0;
    let mut batch: Vec<Op> = Vec::new();
    for (account, apath) in sorted_dir(root) {
        if !apath.is_dir() || !valid_account_id(&account) {
            continue;
        }
        for (fdir, fpath) in sorted_dir(&apath) {
            if !fpath.is_dir() {
                continue;
            }
            let Some(name) = fsutil::read_to_string(fpath.join("name.txt")).ok().or_else(|| folder_name_from_dir(&fdir)) else { continue };
            let mut folder: Option<FolderRow> = None;
            for (uvdir, uvpath) in sorted_dir(&fpath) {
                let Ok(uv) = uvdir.parse::<u32>() else { continue };
                for (file, path) in sorted_dir(&uvpath) {
                    let Some(uid) = file.strip_suffix(".eml").and_then(|u| u.parse::<u32>().ok()) else { continue };
                    if folder.is_none() {
                        folder = Some(mail_db::upsert_folder(conn, &account, &account, &name, None, None, true)?);
                    }
                    let f = folder.as_ref().ok_or("no folder")?;
                    let known: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM messages WHERE folder = ?1 AND uidvalidity = ?2 AND uid = ?3)", rusqlite::params![f.id, uv, uid], |r| r.get(0))?;
                    if known {
                        continue;
                    }
                    let Ok(raw) = fsutil::read(&path) else { continue };
                    let (fields, parsed) = index_fields(&raw, None);
                    batch.push(Op::message(NewMessage { folder: f.id, uidvalidity: uv, uid, path: format!("{account}/{fdir}/{uv}/{file}"), state: STATE_FULL, flags: String::new(), size: raw.len() as u64, parsed, fields }));
                    total += 1;
                    if batch.len() >= RECONCILE_BATCH {
                        mail_db::apply_batch(conn, &std::mem::take(&mut batch), None)?;
                    }
                }
            }
            if let Some(f) = folder {
                adopt_uidvalidity(conn, &f, &fpath)?;
            }
        }
    }
    mail_db::apply_batch(conn, &batch, None)?;
    Ok(total)
}

/// A folder rebuilt from files has no UIDVALIDITY yet: it is the newest one on disk, so a sync
/// that finds the same number carries on instead of fetching everything again.
fn adopt_uidvalidity(conn: &mut Connection, f: &FolderRow, fpath: &Path) -> Result<()> {
    let current = mail_db::folder_by_id(conn, f.id)?.ok_or("folder vanished")?;
    if current.uidvalidity.is_some() {
        return Ok(());
    }
    let newest = sorted_dir(fpath).iter().filter_map(|(n, _)| n.parse::<u32>().ok()).max();
    if let Some(uv) = newest {
        mail_db::apply_batch(conn, &[Op::FolderState { folder: f.id, uidvalidity: Some(uv), uidnext: None, highestmodseq: None, last_sync: 0 }], None)?;
    }
    Ok(())
}

/// Rows with a file: is it still there? Marks and unmarks `missing_file`; returns how many are missing.
fn mark_missing_files(conn: &mut Connection, root: &Path) -> Result<usize> {
    let rows: Vec<(i64, String, bool)> = {
        let mut st = conn.prepare("SELECT id, path, missing_file FROM messages WHERE state = 0 AND path != ''")?;
        let v = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        v
    };
    let mut ops = Vec::new();
    let mut missing = 0;
    for (id, path, was_missing) in rows {
        let gone = !root.join(&path).is_file();
        missing += gone as usize;
        if gone != was_missing {
            ops.push(Op::MissingFile { id, missing: gone });
        }
    }
    mail_db::apply_batch(conn, &ops, None)?;
    Ok(missing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    pub fn temp_root(tag: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let p = std::env::temp_dir().join(format!("shale-mailstore-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    pub fn sample(subject: &str, from: &str, body: &str) -> Vec<u8> {
        format!("From: {from}\r\nTo: me@example.com\r\nSubject: {subject}\r\nDate: Thu, 13 Nov 2026 10:22:33 +0100\r\nMessage-ID: <{subject}@x>\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}\r\n").into_bytes()
    }

    fn put(store: &MailStore, folder: &FolderRow, uid: u32, raw: &[u8]) {
        let m = store.save_message(folder, 7, uid, raw, "\\Seen", None).unwrap();
        store.commit(vec![Op::message(m)]).unwrap();
        store.commit(vec![Op::FolderState { folder: folder.id, uidvalidity: Some(7), uidnext: None, highestmodseq: None, last_sync: 1 }]).unwrap();
    }

    fn count(store: &MailStore) -> i64 {
        store.reader().unwrap().count_messages().unwrap()
    }

    // spec: MAIL-001
    #[test]
    fn the_store_folder_is_private() {
        let root = temp_root("private");
        let _store = MailStore::open(&root.join("mail")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(root.join("mail")).unwrap().permissions().mode() & 0o777, 0o700);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-002
    #[test]
    fn folder_names_are_safe_on_disk_and_come_back() {
        for name in ["INBOX", "INBOX/Sub folder", "../../etc", "a.b", "Gesendet/Über uns", ".", "..", "x%y", "&AOQ-", ""] {
            let d = folder_dir_name(name);
            assert!(d.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'%'), "{d}");
            assert!(d != "." && d != "..");
            if !name.is_empty() {
                assert_eq!(folder_name_from_dir(&d).as_deref(), Some(name));
            }
        }
        let long = "x".repeat(400);
        let d = folder_dir_name(&long);
        assert!(d.starts_with("long-") && d.len() < 40);
        assert_eq!(folder_dir_name(&long), d, "stable");
        assert_ne!(folder_dir_name(&"y".repeat(400)), d);
        assert!(valid_account_id("acct-1_x") && !valid_account_id("../x") && !valid_account_id("") && !valid_account_id("a/b") && !valid_account_id("drafts") && !valid_account_id("outbox"));
    }

    // spec: MAIL-002, MAIL-003
    #[test]
    fn a_message_is_a_file_at_the_documented_path_and_then_a_row() {
        let root = temp_root("path");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "Work", "INBOX/Projects", Some("/"), None, true).unwrap();
        let raw = sample("Hello", "Ada <ada@x.de>", "body text");
        let m = store.save_message(&f, 7, 42, &raw, "\\Seen", None).unwrap();
        assert_eq!(m.path, "acct1/INBOX%2FProjects/7/42.eml");
        assert_eq!(std::fs::read(root.join(&m.path)).unwrap(), raw);
        assert_eq!(count(&store), 0, "no row before the caller commits it");
        store.commit(vec![Op::message(m)]).unwrap();
        assert_eq!(count(&store), 1);
        let s = store.reader().unwrap().conn().query_row("SELECT subject FROM messages", [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(s, "Hello");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-003, MAIL-007
    #[test]
    fn a_file_without_a_row_is_indexed_at_the_next_start() {
        let root = temp_root("crash-file");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        put(&store, &f, 1, &sample("one", "a@x.de", "first"));
        // the crash: the file of message 2 is written, the row never is
        let _lost_row = store.save_message(&f, 7, 2, &sample("two", "b@x.de", "second"), "", None).unwrap();
        drop(store);
        let again = MailStore::open(&root).unwrap();
        assert_eq!(again.opened.indexed_files, 1);
        assert_eq!(count(&again), 2);
        assert_eq!(again.reader().unwrap().search("second", None, 5, 0).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-007
    #[test]
    fn a_row_without_a_file_is_marked_for_refetch_and_unmarked_when_the_file_returns() {
        let root = temp_root("missing");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        put(&store, &f, 1, &sample("one", "a@x.de", "first"));
        let path = root.join("acct1/INBOX/7/1.eml");
        let saved = std::fs::read(&path).unwrap();
        drop(store);
        std::fs::remove_file(&path).unwrap();
        let store = MailStore::open(&root).unwrap();
        assert_eq!(store.opened.missing_files, 1);
        let r = store.reader().unwrap();
        assert_eq!(r.missing_uids(f.id, 7).unwrap(), vec![1]);
        assert!(r.present_uids(f.id, 7).unwrap().is_empty(), "the sync fetches it again");
        drop(r);
        drop(store);
        std::fs::write(&path, saved).unwrap();
        let store = MailStore::open(&root).unwrap();
        assert!(store.reader().unwrap().missing_uids(f.id, 7).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-005
    #[test]
    fn a_batch_that_dies_in_the_writer_leaves_no_partial_rows() {
        let root = temp_root("batch");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        let ops: Vec<Op> = (1..=6).map(|uid| Op::message(store.save_message(&f, 7, uid, &sample(&format!("m{uid}"), "a@x.de", "text"), "", None).unwrap())).collect();
        let err = store.commit_crashing(ops.clone(), 4).unwrap_err();
        assert!(err.0.contains("crashed"), "{err}");
        assert_eq!(count(&store), 0);
        // the writer lives on, and the whole batch goes through the next time
        store.commit(ops).unwrap();
        assert_eq!(count(&store), 6);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-006
    #[test]
    fn a_damaged_index_is_renamed_and_rebuilt_from_the_files() {
        let root = temp_root("corrupt");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "Archive 2026", Some("/"), None, true).unwrap();
        for uid in 1..=8 {
            put(&store, &f, uid, &sample(&format!("subject {uid}"), "a@x.de", &format!("text {uid}")));
        }
        drop(store);
        // damage: overwrite the middle of the file with garbage
        let idx = root.join("index.sqlite");
        let mut bytes = std::fs::read(&idx).unwrap();
        assert!(bytes.len() > 8192);
        for b in bytes.iter_mut().skip(100).take(6000) {
            *b = 0xA5;
        }
        std::fs::write(&idx, &bytes).unwrap();
        let store = MailStore::open(&root).unwrap();
        let broken = store.opened.quarantined.clone().expect("moved aside");
        assert!(broken.file_name().unwrap().to_str().unwrap().starts_with("index.sqlite.broken-"));
        assert_eq!(std::fs::read(&broken).unwrap(), bytes, "kept as it was, not deleted");
        assert_eq!(store.opened.restored_from, None);
        assert_eq!(store.opened.indexed_files, 8);
        assert_eq!(count(&store), 8, "same message count");
        let r = store.reader().unwrap();
        assert_eq!(r.search("text 5", None, 5, 0).unwrap().len(), 1);
        assert_eq!(r.folders(Some("acct1")).unwrap()[0].name, "Archive 2026", "folder name came back from the path");
        drop(r);
        drop(store);
        // a second damage the same day does not overwrite the first quarantine
        std::fs::write(&idx, b"this is not a database at all, but longer than nothing").unwrap();
        let store = MailStore::open(&root).unwrap();
        let second = store.opened.quarantined.clone().unwrap();
        assert_ne!(second, broken);
        assert!(broken.exists() && second.exists());
        assert_eq!(count(&store), 8);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-006, MAIL-008
    #[test]
    fn a_damaged_index_is_restored_from_the_newest_good_backup() {
        let root = temp_root("restore");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        put(&store, &f, 1, &sample("one", "a@x.de", "first"));
        store.backup("2026-11-01").unwrap().unwrap();
        put(&store, &f, 2, &sample("two", "a@x.de", "second"));
        store.backup("2026-11-02").unwrap().unwrap();
        put(&store, &f, 3, &sample("three", "a@x.de", "third"));
        drop(store);
        // the newest backup is itself damaged: the one before it is used
        std::fs::write(root.join("index.backup-2026-11-02.sqlite"), b"garbage garbage garbage").unwrap();
        std::fs::write(root.join("index.sqlite"), b"garbage garbage garbage").unwrap();
        let store = MailStore::open(&root).unwrap();
        assert_eq!(store.opened.restored_from, Some(root.join("index.backup-2026-11-01.sqlite")));
        // the backup had 1 message; reconcile found the files of the others
        assert_eq!(store.opened.indexed_files, 2);
        assert_eq!(count(&store), 3);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-008
    #[test]
    fn backups_are_made_once_a_day_and_the_newest_three_are_kept() {
        let root = temp_root("backups");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        put(&store, &f, 1, &sample("one", "a@x.de", "first"));
        let first = store.backup("2026-11-01").unwrap().unwrap();
        assert!(first.exists());
        assert_eq!(store.backup("2026-11-01").unwrap(), None, "one a day");
        for d in ["2026-11-02", "2026-11-03", "2026-11-04", "2026-11-05"] {
            store.backup(d).unwrap().unwrap();
        }
        let names: Vec<String> = backups(&root).iter().map(|p| p.file_name().unwrap().to_str().unwrap().to_string()).collect();
        assert_eq!(names, vec!["index.backup-2026-11-05.sqlite", "index.backup-2026-11-04.sqlite", "index.backup-2026-11-03.sqlite"]);
        assert!(std::fs::read_dir(&root).unwrap().flatten().all(|e| !e.file_name().to_str().unwrap().ends_with(".tmp")), "no temporary file left");
        mail_db::quick_check(&root.join("index.backup-2026-11-05.sqlite")).unwrap();
        assert!(store.backup_if_due().unwrap().is_some());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-014
    #[test]
    fn a_message_that_cannot_be_read_is_stored_raw_with_a_could_not_parse_row() {
        let root = temp_root("garbage");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        let junk: Vec<u8> = (0..3000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let m = store.save_message(&f, 7, 1, &junk, "", Some(500)).unwrap();
        assert!(!m.parsed);
        assert_eq!(m.fields.subject, COULD_NOT_PARSE);
        assert_eq!(m.fields.date, Some(500), "falls back to the server's date");
        store.commit(vec![Op::message(m)]).unwrap();
        assert_eq!(std::fs::read(root.join("acct1/INBOX/7/1.eml")).unwrap(), junk, "the bytes are kept as they came");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-014
    #[test]
    fn random_and_hostile_bytes_never_panic_the_parse_and_index_path() {
        let mut seed: u64 = 0x9E3779B97F4A7C15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let pieces: [&[u8]; 14] = [
            b"From: ",
            b"Subject: =?utf-8?B?",
            b"\r\n\r\n",
            b"Content-Type: multipart/mixed; boundary=\"",
            b"--b",
            b"=?",
            b"?=",
            b"\r\n",
            b"Content-Transfer-Encoding: base64\r\n",
            b"quoted-printable",
            b"\xff\xfe\x00",
            b"=3D=\r\n",
            b"boundary=",
            b"charset=\"x\"",
        ];
        for round in 0..400 {
            let len = (next() % 600) as usize;
            let mut data: Vec<u8> = Vec::new();
            while data.len() < len {
                if next() % 3 == 0 {
                    data.extend_from_slice(pieces[(next() % pieces.len() as u64) as usize]);
                } else {
                    data.push(next() as u8);
                }
            }
            let (fields, _) = index_fields(&data, None);
            assert!(fields.body.len() <= MAX_BODY_INDEX, "round {round}");
        }
        // deep nesting and a huge header line
        let mut deep = Vec::new();
        for i in 0..300 {
            deep.extend_from_slice(format!("Content-Type: multipart/mixed; boundary=\"b{i}\"\r\n\r\n--b{i}\r\n").as_bytes());
        }
        let _ = index_fields(&deep, None);
        let _ = index_fields(&[b"Subject: ".as_slice(), &vec![b'a'; 2_000_000]].concat(), None);
        let _ = index_fields(b"", None);
    }

    #[test]
    fn long_texts_are_cut_at_a_character() {
        assert_eq!(cut_text("äöü", 3), "ä");
        assert_eq!(cut_text("abc", 10), "abc");
    }

    // spec: MAIL-009
    #[test]
    fn nothing_here_deletes_a_message_file() {
        let sources = [include_str!("mail_store.rs"), include_str!("imap_sync.rs"), include_str!("mail_ops.rs")];
        for src in sources {
            let code = src.split("#[cfg(test)]").next().unwrap();
            let removals: Vec<&str> = code.lines().filter(|l| !l.trim_start().starts_with("//") && (l.contains("remove_file") || l.contains("remove_dir"))).collect();
            // the only deletions are of our own temporary backup copy and of backups beyond the newest 3
            assert!(removals.iter().all(|l| l.contains("&tmp") || l.contains("(old)")), "{removals:?}");
        }
    }

    #[test]
    fn rebuilding_adopts_the_newest_uidvalidity_and_keeps_old_ones_unlisted() {
        let root = temp_root("uv");
        let store = MailStore::open(&root).unwrap();
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        for (uv, uid) in [(5u32, 1u32), (9, 1), (9, 2)] {
            let m = store.save_message(&f, uv, uid, &sample(&format!("s{uv}-{uid}"), "a@x.de", "x"), "", None).unwrap();
            store.commit(vec![Op::message(m)]).unwrap();
        }
        drop(store);
        std::fs::remove_file(root.join("index.sqlite")).unwrap();
        let store = MailStore::open(&root).unwrap();
        let r = store.reader().unwrap();
        let folder = &r.folders(None).unwrap()[0];
        assert_eq!(folder.uidvalidity, Some(9));
        assert_eq!(r.list(folder.id, 10, 0).unwrap().len(), 2, "the old UIDVALIDITY is not listed");
        assert_eq!(r.count_messages().unwrap(), 3, "but its file is still indexed");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn two_threads_can_read_while_the_writer_commits() {
        let root = temp_root("threads");
        let store = std::sync::Arc::new(MailStore::open(&root).unwrap());
        let f = store.ensure_folder("acct1", "A", "INBOX", Some("/"), None, true).unwrap();
        let s2 = store.clone();
        let reader = std::thread::spawn(move || {
            let r = s2.reader().unwrap();
            for _ in 0..50 {
                r.count_messages().unwrap();
            }
        });
        for uid in 1..=30 {
            put(&store, &f, uid, &sample(&format!("m{uid}"), "a@x.de", "t"));
        }
        reader.join().unwrap();
        assert_eq!(count(&store), 30);
        drop(store);
        let _ = std::fs::remove_dir_all(&root);
    }
}
