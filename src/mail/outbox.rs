//! Drafts and the outbox on disk (ADR-0010, MAIL-130 to MAIL-136): a text the person wrote is
//! never lost, and a mail is never sent twice or by accident.
//!
//! ```text
//! <mail store>/drafts/<account>/<id>.eml    the draft as a message (for other programs)
//!                              <id>.json    the fields as typed (the commit point of a save)
//!                              <id>.att/    the attachments, copied here when they are added
//! <mail store>/outbox/<account>/<id>.eml    exactly the bytes that are sent (and appended to Sent)
//!                              <id>.json    state, error, envelope, send-after time, the draft fields
//!                              <id>.att/    the attachments (to edit the mail again)
//! ```
//!
//! Every file is written through `fsutil` (temp, fsync, rename) and nothing is ever deleted: what
//! leaves a folder goes to `<mail store>/.trash/<date>/…` by `rename_noreplace`.
//!
//! The states of an outbox entry, every change one atomic write of its JSON:
//! `Queued → Sending → Sent`, `Sending → Failed` (definitely not sent), `Sending → Unsure` (the end of
//! the message was written and the answer never came; also what a crash in `Sending` becomes at the
//! next start), `Failed → Queued` (Retry), `Unsure → Queued` (Send again: only the person does this),
//! `Queued → Undone` (Undo send, then the draft is written back and the outbox files go to the trash).
//! `Sending` is written before the first byte goes to the network, from `Queued` only, under a lock.

use crate::fsutil;
use crate::mail::compose::{Att, BuildOptions, Built, Draft};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

type Result<T> = std::result::Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The state of an outbox entry.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Sending,
    Sent,
    Failed,
    Unsure,
    Undone,
}

impl State {
    pub fn label(self) -> &'static str {
        match self {
            State::Queued => "waiting to be sent",
            State::Sending => "sending…",
            State::Sent => "sent",
            State::Failed => "not sent",
            State::Unsure => "may have been sent",
            State::Undone => "taken back",
        }
    }
}

/// One mail in the outbox.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub version: u32,
    pub id: String,
    pub account: String,
    pub state: State,
    pub error: Option<String>,
    pub created: i64,
    /// It is not sent before this time (seconds since 1970): the undo window.
    pub send_after: i64,
    pub sent_at: Option<i64>,
    pub message_id: String,
    pub from: String,
    pub recipients: Vec<String>,
    pub subject: String,
    /// The local Sent copy and the append are recorded (or not needed).
    pub copy_done: bool,
    /// The draft this came from, for editing it again.
    pub draft: Draft,
}

/// A draft in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftInfo {
    pub id: String,
    pub account: String,
    pub subject: String,
    pub to: String,
    pub saved: i64,
    pub attachments: usize,
}

#[derive(Serialize, Deserialize)]
struct DraftFile {
    version: u32,
    saved: i64,
    draft: Draft,
}

/// One attachment file that is being replaced (MAIL-215), kept in `<id>.attrepl` next to the draft or mail.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
struct ReplaceRecord {
    /// The file in `<id>.att/`.
    file: String,
    /// The attachment's name as the person knows it.
    name: String,
    /// Where the old file goes, relative to the mail store.
    trash: String,
}

/// An attachment that a cut-off replacement left missing (MAIL-216).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttAlert {
    pub file: String,
    pub name: String,
    pub trash: PathBuf,
    /// The old version is still in the trash.
    pub trash_present: bool,
}

impl AttAlert {
    pub fn message(&self) -> String {
        let tail = if self.trash_present { format!("The previous version is in the trash at {}.", self.trash.display()) } else { "The previous version is not in the trash any more.".to_string() };
        format!("The attachment “{}” is missing: Shale stopped while replacing it. {tail}", self.name)
    }
}

/// What a start-up check found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Recovery {
    /// Mails that were in `Sending` when the program ended: now `Unsure`.
    pub unsure: Vec<String>,
    /// Undone mails whose draft was written back (or already was) and whose outbox files were put away.
    pub undone_finished: usize,
    /// Drafts of a mail that is in the outbox (the program ended while queuing): put away.
    pub drafts_superseded: usize,
}

/// Transitions are done one at a time (the check and the write of the state).
static LOCK: Mutex<()> = Mutex::new(());

pub struct Outbox {
    root: PathBuf,
    /// Tests: the n-th durable step fails (a crash at that point). `usize::MAX`: never.
    crash_at: AtomicUsize,
    steps: AtomicUsize,
}

fn lock() -> std::sync::MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Outbox {
    pub fn new(root: &Path) -> Outbox {
        Outbox { root: root.to_path_buf(), crash_at: AtomicUsize::new(usize::MAX), steps: AtomicUsize::new(0) }
    }

    /// Tests: crash (fail) at the n-th durable step from now on, counting from 0.
    #[cfg(test)]
    pub fn crash_at_step(&self, n: usize) {
        self.steps.store(0, Ordering::SeqCst);
        self.crash_at.store(n, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub fn no_crash(&self) {
        self.crash_at.store(usize::MAX, Ordering::SeqCst);
    }

    /// Every write or move calls this first; in a test it can be made to fail ("the power went out here").
    fn step(&self) -> Result<()> {
        let n = self.steps.fetch_add(1, Ordering::SeqCst);
        if n >= self.crash_at.load(Ordering::SeqCst) {
            return Err("simulated crash".into());
        }
        Ok(())
    }

    fn dir(&self, kind: &str, account: &str) -> Result<PathBuf> {
        if !crate::mail::store::valid_account_id(account) {
            return Err(format!("not a valid account id: {account:?}"));
        }
        Ok(self.root.join(kind).join(account))
    }

    fn path(&self, kind: &str, account: &str, id: &str, ext: &str) -> Result<PathBuf> {
        if !valid_id(id) {
            return Err(format!("not a valid mail id: {id:?}"));
        }
        Ok(self.dir(kind, account)?.join(format!("{id}{ext}")))
    }

    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        self.step()?;
        if let Some(d) = path.parent() {
            fsutil::ensure_private_dir(d).map_err(err)?;
        }
        fsutil::write_atomic_private(path, bytes).map_err(err)
    }

    /// Put a file or folder out of the way, to `.trash/<date>/<kind>/<account>/…` (nothing is deleted).
    fn put_away(&self, kind: &str, account: &str, name: &str, date: &str) -> Result<()> {
        let src = self.dir(kind, account)?.join(name);
        if std::fs::symlink_metadata(&src).is_err() {
            return Ok(());
        }
        self.step()?;
        let dest_dir = self.root.join(".trash").join(date).join(kind).join(account);
        fsutil::ensure_private_dir(&dest_dir).map_err(err)?;
        let mut dest = dest_dir.join(name);
        for n in 2..1000 {
            match fsutil::rename_noreplace(&src, &dest) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => dest = dest_dir.join(format!("{n}-{name}")),
                Err(e) => return Err(err(e)),
            }
        }
        Err("too many files with that name in the local trash".into())
    }

    /// An attachment file that is about to be replaced by other bytes goes to the trash, not into nothing (MAIL-200).
    /// Before it moves, the intent (the file and the trash path it goes to) is written to the replace record (MAIL-215);
    /// the caller removes the record after the new file is written.
    fn put_away_att(&self, kind: &str, account: &str, id: &str, file: &str, name: &str) -> Result<()> {
        let src = self.path(kind, account, id, ".att")?.join(file);
        let rel_dir = Path::new(".trash").join(crate::mail::store::today_string()).join(kind).join(account).join(format!("{id}.att"));
        fsutil::ensure_private_dir(&self.root.join(&rel_dir)).map_err(err)?;
        let mut rel = rel_dir.join(file);
        for n in 2..1000 {
            if std::fs::symlink_metadata(self.root.join(&rel)).is_err() {
                break;
            }
            rel = rel_dir.join(format!("{n}-{file}"));
        }
        let mut recs = self.read_records(kind, account, id);
        recs.retain(|r| r.file != file);
        recs.push(ReplaceRecord { file: file.to_string(), name: name.to_string(), trash: crate::vault::rel_to_string(&rel) });
        self.write_records(kind, account, id, &recs)?;
        self.step()?;
        fsutil::rename_noreplace(&src, &self.root.join(&rel)).map_err(err)
    }

    // ------------------------------------------------------------ the replace record (MAIL-215)

    fn read_records(&self, kind: &str, account: &str, id: &str) -> Vec<ReplaceRecord> {
        self.path(kind, account, id, ".attrepl").ok().and_then(|p| fsutil::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn write_records(&self, kind: &str, account: &str, id: &str, recs: &[ReplaceRecord]) -> Result<()> {
        let p = self.path(kind, account, id, ".attrepl")?;
        if recs.is_empty() {
            // the record is not a document of the person: it goes when it has done its job (the trash copy stays)
            return match std::fs::remove_file(&p) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(err(e)),
                _ => Ok(()),
            };
        }
        self.write(&p, &serde_json::to_vec_pretty(recs).map_err(err)?)
    }

    fn clear_record(&self, kind: &str, account: &str, id: &str, file: &str) -> Result<()> {
        let mut recs = self.read_records(kind, account, id);
        let before = recs.len();
        recs.retain(|r| r.file != file);
        if recs.len() != before {
            self.write_records(kind, account, id, &recs)?;
        }
        Ok(())
    }

    /// Is the attachment file there, whole? The length is in its name (`<n>-<length>-<name>`); a write is atomic, so
    /// a file that is there is whole, and the length is a second check.
    fn att_whole(&self, kind: &str, account: &str, id: &str, file: &str) -> bool {
        let want = file.split('-').nth(1).and_then(|l| l.parse::<u64>().ok());
        self.path(kind, account, id, ".att").ok().and_then(|d| std::fs::metadata(d.join(file)).ok()).is_some_and(|m| m.is_file() && want.is_none_or(|w| w == m.len()))
    }

    /// What is left of a replacement that was cut off (MAIL-216): the records of attachments of this draft or mail
    /// whose file is missing or incomplete. A record whose new file is whole, or whose attachment is not in the
    /// draft any more, is cleared silently. The trash copy is never touched.
    pub fn att_alerts(&self, kind: &str, account: &str, id: &str, atts: &[Att]) -> Vec<AttAlert> {
        let _g = lock();
        let mut out = vec![];
        for r in self.read_records(kind, account, id) {
            if !atts.iter().any(|a| a.file == r.file) || self.att_whole(kind, account, id, &r.file) {
                let _ = self.clear_record(kind, account, id, &r.file);
                continue;
            }
            let trash = self.root.join(&r.trash);
            out.push(AttAlert { file: r.file, name: r.name, trash_present: trash.is_file(), trash });
        }
        out
    }

    /// Put the old version back into the draft (MAIL-217): its bytes are copied from the trash (the trash copy stays) and the
    /// record is cleared. Returns the bytes.
    pub fn restore_att(&self, kind: &str, account: &str, id: &str, file: &str) -> Result<Vec<u8>> {
        let _g = lock();
        let rec = self.read_records(kind, account, id).into_iter().find(|r| r.file == file).ok_or("there is nothing to restore")?;
        let bytes = fsutil::read(self.root.join(&rec.trash)).map_err(|e| format!("the old version is not in the trash any more: {e}"))?;
        if !self.att_whole(kind, account, id, file) {
            self.write(&self.path(kind, account, id, ".att")?.join(file), &bytes)?;
        }
        self.clear_record(kind, account, id, file)?;
        Ok(bytes)
    }

    /// The attachments of a saved entry that are not there, as sentences (MAIL-218).
    fn missing_atts(&self, kind: &str, account: &str, id: &str, atts: &[Att]) -> Vec<String> {
        let recs = self.read_records(kind, account, id);
        atts.iter()
            .filter(|a| a.file.is_empty() || !self.att_whole(kind, account, id, &a.file))
            .map(|a| match recs.iter().find(|r| r.file == a.file) {
                Some(r) => AttAlert { file: r.file.clone(), name: a.name.clone(), trash_present: self.root.join(&r.trash).is_file(), trash: self.root.join(&r.trash) }.message(),
                None => format!("The attachment “{}” is missing.", a.name),
            })
            .collect()
    }

    fn put_away_all(&self, kind: &str, account: &str, id: &str, date: &str) -> Result<()> {
        // the JSON goes first: without it the rest is not a draft or a mail any more, and a crash
        // in between leaves only files that nothing lists
        for ext in [".json", ".eml", ".att", ".attrepl"] {
            self.put_away(kind, account, &format!("{id}{ext}"), date)?;
        }
        Ok(())
    }

    // ------------------------------------------------------------ attachments

    /// Write the attachments' bytes beside a draft or mail. Files that are there are kept.
    fn write_atts(&self, kind: &str, account: &str, id: &str, atts: &mut [Att]) -> Result<()> {
        let dir = self.path(kind, account, id, ".att")?;
        for (i, a) in atts.iter_mut().enumerate() {
            let file = format!("{i}-{}-{}", a.data.len(), crate::mail::compose::safe_file_name(&a.name));
            let p = dir.join(&file);
            // a saved attachment whose file is gone and whose bytes were not read back is kept as it is: an empty file
            // written in its place would become an empty attachment (MAIL-216)
            if a.data.is_empty() && !a.file.is_empty() && std::fs::symlink_metadata(dir.join(&a.file)).is_err() {
                continue;
            }
            // the length is in the name, so only the bytes can tell a replaced file of the same size apart
            let same = std::fs::metadata(&p).map(|m| m.len() == a.data.len() as u64).unwrap_or(false) && fsutil::read(&p).is_ok_and(|b| b == *a.data);
            if !same {
                if std::fs::symlink_metadata(&p).is_ok() {
                    self.put_away_att(kind, account, id, &file, &a.name)?;
                }
                self.write(&p, &a.data)?;
            }
            // whole on disk (or there all along): the replace record, if any, has done its job
            self.clear_record(kind, account, id, &file)?;
            a.file = file;
        }
        Ok(())
    }

    /// Read the attachments back. A missing file is told, the rest is returned.
    fn read_atts(&self, kind: &str, account: &str, id: &str, atts: &mut [Att], problems: &mut Vec<String>) -> Result<()> {
        let dir = self.path(kind, account, id, ".att")?;
        for a in atts.iter_mut() {
            if a.file.is_empty() || a.file.contains(['/', '\\']) || a.file.starts_with('.') {
                problems.push(format!("the attachment “{}” is not in the saved draft", a.name));
                continue;
            }
            match fsutil::read(dir.join(&a.file)) {
                Ok(b) => a.data = std::sync::Arc::new(b),
                Err(e) => problems.push(format!("the attachment “{}” could not be read: {e}", a.name)),
            }
        }
        Ok(())
    }

    // ------------------------------------------------------------ drafts

    /// Save a draft: attachments, the message file, then the JSON (the commit point). A crash at any
    /// point leaves the previous whole draft, or this one.
    pub fn save_draft(&self, draft: &Draft, now: i64) -> Result<()> {
        let _g = lock();
        if self.get_unlocked(&draft.account, &draft.id).is_some_and(|i| i.state != State::Undone) {
            return Err("this mail is in the outbox already".into());
        }
        self.save_draft_locked(draft, now)
    }

    /// Open a draft with its attachments. Attachments that cannot be read are listed in the second part.
    pub fn load_draft(&self, account: &str, id: &str) -> Result<(Draft, Vec<String>)> {
        let raw = fsutil::read(self.path("drafts", account, id, ".json")?).map_err(err)?;
        let f: DraftFile = serde_json::from_slice(&raw).map_err(|e| format!("the draft file is damaged: {e}"))?;
        let mut d = f.draft;
        let mut problems = vec![];
        self.read_atts("drafts", account, id, &mut d.attachments, &mut problems)?;
        Ok((d, problems))
    }

    /// The drafts of one account, or of all (newest first). A damaged JSON is skipped; its files stay on disk.
    pub fn list_drafts(&self, account: Option<&str>) -> Vec<DraftInfo> {
        let mut out = vec![];
        for acct in self.accounts("drafts", account) {
            for id in self.ids("drafts", &acct) {
                let Ok(raw) = self.path("drafts", &acct, &id, ".json").and_then(|p| fsutil::read(p).map_err(err)) else { continue };
                let Ok(f) = serde_json::from_slice::<DraftFile>(&raw) else { continue };
                out.push(DraftInfo { id, account: acct.clone(), subject: f.draft.subject.clone(), to: f.draft.to.clone(), saved: f.saved, attachments: f.draft.attachments.len() });
            }
        }
        out.sort_by(|a, b| b.saved.cmp(&a.saved).then(b.id.cmp(&a.id)));
        out
    }

    /// Put a draft away (Discard): its files go to `.trash/<date>/drafts/…`.
    pub fn discard_draft(&self, account: &str, id: &str, date: &str) -> Result<()> {
        let _g = lock();
        self.put_away_all("drafts", account, id, date)
    }

    fn accounts(&self, kind: &str, only: Option<&str>) -> Vec<String> {
        if let Some(a) = only {
            return vec![a.to_string()];
        }
        let mut v: Vec<String> = std::fs::read_dir(self.root.join(kind)).map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().map(String::from)).filter(|n| crate::mail::store::valid_account_id(n)).collect()).unwrap_or_default();
        v.sort();
        v
    }

    /// The ids that have a `.json` in the folder.
    fn ids(&self, kind: &str, account: &str) -> Vec<String> {
        let Ok(dir) = self.dir(kind, account) else { return vec![] };
        let mut v: Vec<String> = std::fs::read_dir(dir).map(|rd| rd.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".json")).map(String::from)).filter(|id| valid_id(id)).collect()).unwrap_or_default();
        v.sort();
        v
    }

    // ------------------------------------------------------------ the outbox

    fn read_item(&self, account: &str, id: &str) -> Result<Item> {
        let raw = fsutil::read(self.path("outbox", account, id, ".json")?).map_err(err)?;
        serde_json::from_slice(&raw).map_err(|e| format!("the outbox entry is damaged: {e}"))
    }

    fn get_unlocked(&self, account: &str, id: &str) -> Option<Item> {
        self.read_item(account, id).ok()
    }

    fn write_item(&self, item: &Item) -> Result<()> {
        let json = serde_json::to_vec_pretty(item).map_err(err)?;
        self.write(&self.path("outbox", &item.account, &item.id, ".json")?, &json)
    }

    #[cfg(test)]
    pub fn get(&self, account: &str, id: &str) -> Option<Item> {
        let _g = lock();
        self.get_unlocked(account, id)
    }

    /// The outbox entries of an account or of all, oldest first.
    pub fn list(&self, account: Option<&str>) -> Vec<Item> {
        let _g = lock();
        let mut out = vec![];
        for acct in self.accounts("outbox", account) {
            for id in self.ids("outbox", &acct) {
                if let Ok(i) = self.read_item(&acct, &id) {
                    out.push(i);
                }
            }
        }
        out.sort_by(|a, b| a.created.cmp(&b.created).then(a.id.cmp(&b.id)));
        out
    }

    /// The bytes of an entry's message.
    pub fn read_eml(&self, account: &str, id: &str) -> Result<Vec<u8>> {
        fsutil::read(self.path("outbox", account, id, ".eml")?).map_err(err)
    }

    /// Send pressed: the message goes into the outbox (MAIL-132). The draft's files are put away only
    /// after the outbox entry is whole; a crash in between leaves both, and [`Outbox::recover`] puts the draft away.
    pub fn queue(&self, draft: &Draft, built: &Built, now: i64, send_after: i64, date: &str) -> Result<Item> {
        let _g = lock();
        if self.get_unlocked(&draft.account, &draft.id).is_some() {
            return Err("this mail is in the outbox already".into());
        }
        let mut d = draft.clone();
        self.write_atts("outbox", &d.account, &d.id, &mut d.attachments)?;
        self.write(&self.path("outbox", &d.account, &d.id, ".eml")?, &built.bytes)?;
        let item = Item {
            version: 1,
            id: d.id.clone(),
            account: d.account.clone(),
            state: State::Queued,
            error: None,
            created: now,
            send_after,
            sent_at: None,
            message_id: built.message_id.clone(),
            from: built.from.clone(),
            recipients: built.recipients.clone(),
            subject: d.subject.clone(),
            copy_done: false,
            draft: d,
        };
        self.write_item(&item)?;
        self.put_away_all("drafts", &item.account, &item.id, date)?;
        Ok(item)
    }

    /// The transition to `Sending`, written before anything goes to the network (MAIL-133). Only a
    /// `Queued` entry whose time has come; returns the entry and the exact bytes to send.
    pub fn begin_send(&self, account: &str, id: &str, now: i64) -> Result<(Item, Vec<u8>)> {
        let _g = lock();
        let mut item = self.read_item(account, id)?;
        if item.state != State::Queued {
            return Err(format!("this mail is {} and is not sent again by itself", item.state.label()));
        }
        if now < item.send_after {
            return Err("the time to take it back has not passed yet".into());
        }
        // the mail is not sent without an attachment it was written with: it is put away as not sent, with the reason (MAIL-218)
        let missing = self.missing_atts("outbox", account, id, &item.draft.attachments);
        if !missing.is_empty() {
            let msg = format!("{} The mail was not sent.", missing.join(" "));
            item.state = State::Failed;
            item.error = Some(msg.clone());
            self.write_item(&item)?;
            return Err(msg);
        }
        let bytes = fsutil::read(self.path("outbox", account, id, ".eml")?).map_err(err)?;
        item.state = State::Sending;
        item.error = None;
        self.write_item(&item)?;
        Ok((item, bytes))
    }

    /// How a send ended. Only an entry that is `Sending` can end.
    pub fn finish(&self, account: &str, id: &str, outcome: Outcome, now: i64) -> Result<Item> {
        let _g = lock();
        let mut item = self.read_item(account, id)?;
        if item.state != State::Sending {
            return Err(format!("this mail is {}, not being sent", item.state.label()));
        }
        match outcome {
            Outcome::Sent => {
                item.state = State::Sent;
                item.sent_at = Some(now);
                item.error = None;
            }
            Outcome::Failed(e) => {
                item.state = State::Failed;
                item.error = Some(e);
            }
            Outcome::Unsure(e) => {
                item.state = State::Unsure;
                item.error = Some(e);
            }
        }
        self.write_item(&item)?;
        Ok(item)
    }

    /// The local Sent copy and the append are recorded (or not needed): remember it.
    pub fn mark_copy_done(&self, account: &str, id: &str) -> Result<()> {
        let _g = lock();
        let mut item = self.read_item(account, id)?;
        if item.state != State::Sent {
            return Err("only a sent mail has a Sent copy".into());
        }
        item.copy_done = true;
        self.write_item(&item)
    }

    /// Retry a mail that was not sent (MAIL-136): back to `Queued`, to be sent at once.
    pub fn retry(&self, account: &str, id: &str, now: i64) -> Result<Item> {
        self.requeue(account, id, State::Failed, now)
    }

    /// "Send again" for a mail that may have been sent: the person has looked in Sent and decided.
    pub fn send_again(&self, account: &str, id: &str, now: i64) -> Result<Item> {
        self.requeue(account, id, State::Unsure, now)
    }

    fn requeue(&self, account: &str, id: &str, from: State, now: i64) -> Result<Item> {
        let _g = lock();
        let mut item = self.read_item(account, id)?;
        if item.state != from {
            return Err(format!("this mail is {}", item.state.label()));
        }
        item.state = State::Queued;
        item.send_after = now;
        item.error = None;
        self.write_item(&item)?;
        Ok(item)
    }

    /// Undo send (MAIL-134): only while `Queued`. The entry is marked `Undone` first (so a crash can never send
    /// it), then the draft is written back and the outbox files are put away.
    pub fn undo(&self, account: &str, id: &str, now: i64, date: &str) -> Result<Draft> {
        self.to_draft(account, id, &[State::Queued], now, date)
    }

    /// Edit a mail that was not sent, or may have been, as a draft again.
    pub fn edit(&self, account: &str, id: &str, now: i64, date: &str) -> Result<Draft> {
        self.to_draft(account, id, &[State::Failed, State::Unsure], now, date)
    }

    fn to_draft(&self, account: &str, id: &str, allowed: &[State], now: i64, date: &str) -> Result<Draft> {
        let _g = lock();
        let mut item = self.read_item(account, id)?;
        if !allowed.contains(&item.state) {
            return Err(match item.state {
                State::Sending => "it is being sent right now".to_string(),
                s => format!("this mail is {}", s.label()),
            });
        }
        item.state = State::Undone;
        self.write_item(&item)?;
        self.finish_undo(&item, now, date)
    }

    /// The second half of an undo: the draft is written, then the outbox files are put away.
    fn finish_undo(&self, item: &Item, now: i64, date: &str) -> Result<Draft> {
        let mut d = item.draft.clone();
        let mut problems = vec![];
        self.read_atts("outbox", &item.account, &item.id, &mut d.attachments, &mut problems)?;
        if !problems.is_empty() {
            return Err(problems.join("; "));
        }
        // the draft is saved without the lock held a second time (we hold it)
        self.save_draft_locked(&d, now)?;
        self.put_away_all("outbox", &item.account, &item.id, date)?;
        Ok(d)
    }

    fn save_draft_locked(&self, draft: &Draft, now: i64) -> Result<()> {
        let mut d = draft.clone();
        self.write_atts("drafts", &d.account, &d.id, &mut d.attachments)?;
        // the message file is for other programs: built leniently, and left out if it cannot be built (the JSON is the draft)
        if let Ok(b) = crate::mail::compose::build(&d, &BuildOptions { now, lenient: true, ..BuildOptions::default() }) {
            self.write(&self.path("drafts", &d.account, &d.id, ".eml")?, &b.bytes)?;
        }
        let json = serde_json::to_vec_pretty(&DraftFile { version: 1, saved: now, draft: d }).map_err(err)?;
        self.write(&self.path("drafts", &draft.account, &draft.id, ".json")?, &json)
    }

    /// Put a finished entry away (Discard, or dismissing a sent one). Not while waiting or sending.
    pub fn discard(&self, account: &str, id: &str, date: &str) -> Result<()> {
        let _g = lock();
        let item = self.read_item(account, id)?;
        if matches!(item.state, State::Queued | State::Sending) {
            return Err("take it back first (Undo), it is waiting to be sent".into());
        }
        self.put_away_all("outbox", account, id, date)
    }

    /// The check at start-up (MAIL-130, MAIL-133). Run it once before anything is sent.
    pub fn recover(&self, now: i64, date: &str) -> Result<Recovery> {
        let _g = lock();
        let mut r = Recovery::default();
        for acct in self.accounts("outbox", None) {
            for id in self.ids("outbox", &acct) {
                let Ok(mut item) = self.read_item(&acct, &id) else { continue };
                match item.state {
                    State::Sending => {
                        item.state = State::Unsure;
                        item.error = Some("Shale was closed while this mail was being sent".into());
                        self.write_item(&item)?;
                        r.unsure.push(id.clone());
                    }
                    State::Undone => {
                        // an undo that was cut off: the draft may be there already
                        let have_draft = self.path("drafts", &acct, &id, ".json").map(|p| p.is_file()).unwrap_or(false);
                        if !have_draft {
                            let mut d = item.draft.clone();
                            let mut problems = vec![];
                            self.read_atts("outbox", &acct, &id, &mut d.attachments, &mut problems)?;
                            self.save_draft_locked(&d, now)?;
                        }
                        self.put_away_all("outbox", &acct, &id, date)?;
                        r.undone_finished += 1;
                    }
                    _ => {}
                }
            }
        }
        // a draft that has the id of a mail in the outbox: the mail was queued, the draft is stale
        for acct in self.accounts("drafts", None) {
            for id in self.ids("drafts", &acct) {
                if self.read_item(&acct, &id).map(|i| i.state != State::Undone).unwrap_or(false) {
                    self.put_away_all("drafts", &acct, &id, date)?;
                    r.drafts_superseded += 1;
                }
            }
        }
        Ok(r)
    }
}

/// How a send ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Failed(String),
    Unsure(String),
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::mail::compose::{build, new_draft};
    use std::sync::Arc;

    pub const NOW: i64 = 1_794_657_600;
    pub const DATE: &str = "2026-11-14";

    pub fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-outbox-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    pub fn draft(account: &str, subject: &str, body: &str) -> Draft {
        let mut d = new_draft(account, "Ada", "ada@example.com", "");
        d.to = "Bob <bob@x.de>".into();
        d.subject = subject.into();
        d.body = body.into();
        d
    }

    fn built(d: &Draft) -> Built {
        build(d, &BuildOptions { now: NOW, ..BuildOptions::default() }).unwrap()
    }

    fn files(root: &Path) -> Vec<String> {
        fn walk(p: &Path, base: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                let q = e.path();
                if q.is_dir() {
                    walk(&q, base, out);
                } else {
                    // always with "/", so the expectations read the same on every system
                    out.push(q.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"));
                }
            }
        }
        let mut v = vec![];
        walk(root, root, &mut v);
        v.sort();
        v
    }

    // spec: MAIL-130
    #[test]
    fn a_draft_is_saved_as_eml_json_and_attachments_and_comes_back_whole() {
        let root = temp("draft");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "Plans für Köln", "Hallo\n\nText äöü");
        d.attachments.push(Att { name: "Grundriß EG.pdf".into(), mime: "application/pdf".into(), data: Arc::new((0..=255u8).collect()), file: String::new() });
        d.bcc = "hidden@x.de".into();
        ob.save_draft(&d, NOW).unwrap();
        let f = files(&root);
        assert!(f.iter().any(|p| p == &format!("drafts/acct1/{}.eml", d.id)), "{f:?}");
        assert!(f.iter().any(|p| p == &format!("drafts/acct1/{}.json", d.id)));
        assert!(f.iter().any(|p| p.starts_with(&format!("drafts/acct1/{}.att/0-256-", d.id))), "{f:?}");
        // the message file is a message other programs can read (and has the Bcc of a draft)
        let eml = crate::mail::parse(&std::fs::read(root.join(format!("drafts/acct1/{}.eml", d.id))).unwrap());
        assert_eq!(eml.subject, "Plans für Köln");
        assert_eq!(eml.attachments.len(), 1);
        let (back, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert!(problems.is_empty());
        assert_eq!((back.subject.as_str(), back.body.as_str(), back.bcc.as_str()), ("Plans für Köln", "Hallo\n\nText äöü", "hidden@x.de"));
        assert_eq!(back.attachments[0].data.as_slice(), (0..=255u8).collect::<Vec<_>>().as_slice());
        // saving again changes the text and keeps the attachment file
        let mut d2 = back.clone();
        d2.body = "changed".into();
        ob.save_draft(&d2, NOW + 5).unwrap();
        assert_eq!(ob.load_draft("acct1", &d.id).unwrap().0.body, "changed");
        let list = ob.list_drafts(None);
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].subject.as_str(), list[0].attachments, list[0].saved), ("Plans für Köln", 1, NOW + 5));
        // a half-typed address, a line break in a field, no recipient at all: still saved (nothing to lose)
        let mut d3 = draft("acct1", "x\ny", "UNTYPED-TEXT");
        d3.to = "half typed, ok@x.de".into();
        d3.attachments.push(Att { name: "a\r\nb".into(), mime: "x/y".into(), data: Arc::new(vec![1]), file: String::new() });
        ob.save_draft(&d3, NOW).unwrap();
        assert_eq!(ob.load_draft("acct1", &d3.id).unwrap().0.to, "half typed, ok@x.de", "the draft keeps what was typed");
        let m = crate::mail::parse(&std::fs::read(root.join(format!("drafts/acct1/{}.eml", d3.id))).unwrap());
        assert_eq!(m.to, "ok@x.de");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-130
    #[test]
    fn a_crash_at_any_step_of_a_save_leaves_the_last_whole_draft() {
        let root = temp("draft-crash");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "S", "version one");
        d.attachments.push(Att { name: "a.txt".into(), mime: "text/plain".into(), data: Arc::new(b"one".to_vec()), file: String::new() });
        ob.save_draft(&d, NOW).unwrap();
        for n in 0..6 {
            let mut d2 = d.clone();
            d2.body = format!("version two {n}");
            d2.attachments.push(Att { name: format!("b{n}.txt"), mime: "text/plain".into(), data: Arc::new(vec![n as u8; 3]), file: String::new() });
            ob.crash_at_step(n);
            let r = ob.save_draft(&d2, NOW + 1);
            ob.no_crash();
            let (now_draft, problems) = ob.load_draft("acct1", &d.id).unwrap();
            assert!(problems.is_empty(), "step {n}: {problems:?}");
            if r.is_ok() {
                assert_eq!(now_draft.body, format!("version two {n}"));
            } else {
                // the JSON is the commit point: until it is written the draft is the old whole one
                assert!(now_draft.body == "version one" || now_draft.body == format!("version two {n}"), "step {n}: {}", now_draft.body);
                assert_eq!(now_draft.attachments.iter().map(|a| a.data.len()).sum::<usize>(), if now_draft.body == "version one" { 3 } else { 3 + 3 }, "step {n}");
            }
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-131
    #[test]
    fn discarding_a_draft_moves_its_files_to_the_trash_and_nothing_is_deleted() {
        let root = temp("discard");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "S", "precious text");
        d.attachments.push(Att { name: "a.txt".into(), mime: "text/plain".into(), data: Arc::new(b"x".to_vec()), file: String::new() });
        ob.save_draft(&d, NOW).unwrap();
        let before = files(&root).len();
        ob.discard_draft("acct1", &d.id, DATE).unwrap();
        assert!(ob.list_drafts(None).is_empty());
        let f = files(&root);
        assert_eq!(f.len(), before, "every file is still on disk: {f:?}");
        assert!(f.iter().all(|p| p.starts_with(&format!(".trash/{DATE}/drafts/acct1/"))), "{f:?}");
        // discarding again (nothing there) is fine, and a second draft with the same id does not overwrite the first
        ob.discard_draft("acct1", &d.id, DATE).unwrap();
        ob.save_draft(&d, NOW).unwrap();
        ob.discard_draft("acct1", &d.id, DATE).unwrap();
        assert_eq!(files(&root).len(), before * 2);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-132
    #[test]
    fn queueing_writes_the_exact_bytes_and_the_state_machine_allows_only_its_transitions() {
        let root = temp("states");
        let ob = Outbox::new(&root);
        let d = draft("acct1", "Hello", "body");
        ob.save_draft(&d, NOW).unwrap();
        let b = built(&d);
        let item = ob.queue(&d, &b, NOW, NOW + 10, DATE).unwrap();
        assert_eq!(item.state, State::Queued);
        assert_eq!(ob.read_eml("acct1", &d.id).unwrap(), b.bytes);
        assert!(ob.list_drafts(None).is_empty(), "the draft is put away");
        assert!(files(&root).iter().any(|p| p.starts_with(".trash/")));
        assert!(ob.queue(&d, &b, NOW, NOW, DATE).is_err_and(|e| e.contains("already")), "no second entry for the same mail");
        // not before the time
        assert!(ob.begin_send("acct1", &d.id, NOW + 9).is_err_and(|e| e.contains("not passed")));
        assert_eq!(ob.get("acct1", &d.id).unwrap().state, State::Queued);
        let (it, bytes) = ob.begin_send("acct1", &d.id, NOW + 10).unwrap();
        assert_eq!((it.state, bytes), (State::Sending, b.bytes.clone()));
        // sending: no second begin, no undo, no retry, no discard, no edit
        assert!(ob.begin_send("acct1", &d.id, NOW + 99).is_err());
        assert!(ob.undo("acct1", &d.id, NOW, DATE).is_err_and(|e| e.contains("being sent")));
        assert!(ob.retry("acct1", &d.id, NOW).is_err());
        assert!(ob.send_again("acct1", &d.id, NOW).is_err());
        assert!(ob.discard("acct1", &d.id, DATE).is_err());
        assert!(ob.edit("acct1", &d.id, NOW, DATE).is_err());
        let it = ob.finish("acct1", &d.id, Outcome::Sent, NOW + 11).unwrap();
        assert_eq!((it.state, it.sent_at), (State::Sent, Some(NOW + 11)));
        // sent: nothing sends it again, not even by retry; finishing twice is refused
        assert!(ob.begin_send("acct1", &d.id, NOW + 99).is_err());
        assert!(ob.retry("acct1", &d.id, NOW).is_err());
        assert!(ob.send_again("acct1", &d.id, NOW).is_err());
        assert!(ob.finish("acct1", &d.id, Outcome::Sent, NOW).is_err());
        ob.mark_copy_done("acct1", &d.id).unwrap();
        assert!(ob.get("acct1", &d.id).unwrap().copy_done);
        // dismissing a sent mail puts it away
        ob.discard("acct1", &d.id, DATE).unwrap();
        assert!(ob.list(None).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-136
    #[test]
    fn a_failed_mail_waits_for_the_person_and_retry_edit_and_discard_are_theirs() {
        let root = temp("failed");
        let ob = Outbox::new(&root);
        let d = draft("acct1", "F", "text");
        ob.queue(&d, &built(&d), NOW, NOW, DATE).unwrap();
        ob.begin_send("acct1", &d.id, NOW).unwrap();
        let it = ob.finish("acct1", &d.id, Outcome::Failed("550 no such user".into()), NOW).unwrap();
        assert_eq!((it.state, it.error.as_deref()), (State::Failed, Some("550 no such user")));
        // nothing else moves it: a start-up check leaves it alone, and it cannot be begun
        assert_eq!(ob.recover(NOW, DATE).unwrap(), Recovery::default());
        assert!(ob.begin_send("acct1", &d.id, NOW).is_err());
        assert!(ob.send_again("acct1", &d.id, NOW).is_err(), "send again is for the unsure ones");
        let it = ob.retry("acct1", &d.id, NOW + 5).unwrap();
        assert_eq!((it.state, it.send_after, it.error), (State::Queued, NOW + 5, None));
        ob.begin_send("acct1", &d.id, NOW + 5).unwrap();
        ob.finish("acct1", &d.id, Outcome::Failed("again".into()), NOW).unwrap();
        let back = ob.edit("acct1", &d.id, NOW, DATE).unwrap();
        assert_eq!(back.body, "text");
        assert_eq!(ob.load_draft("acct1", &d.id).unwrap().0.body, "text");
        assert!(ob.list(None).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-134
    #[test]
    fn undo_gives_the_draft_back_with_attachments_and_only_while_queued() {
        let root = temp("undo");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "U", "typed text");
        d.attachments.push(Att { name: "a.bin".into(), mime: "application/octet-stream".into(), data: Arc::new(vec![9, 8, 7]), file: String::new() });
        ob.queue(&d, &built(&d), NOW, NOW + 10, DATE).unwrap();
        let back = ob.undo("acct1", &d.id, NOW + 3, DATE).unwrap();
        assert_eq!((back.body.as_str(), back.attachments[0].data.as_slice()), ("typed text", &[9u8, 8, 7][..]));
        assert!(ob.list(None).is_empty());
        let (loaded, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert!(problems.is_empty());
        assert_eq!(loaded.body, "typed text");
        // it can be queued again (the same id is free again)
        ob.queue(&loaded, &built(&loaded), NOW, NOW, DATE).unwrap();
        assert!(ob.begin_send("acct1", &d.id, NOW).is_ok());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-133
    #[test]
    fn a_mail_found_sending_at_start_up_is_unsure_and_never_sent_by_itself() {
        let root = temp("unsure");
        let ob = Outbox::new(&root);
        let d = draft("acct1", "X", "text");
        ob.queue(&d, &built(&d), NOW, NOW, DATE).unwrap();
        ob.begin_send("acct1", &d.id, NOW).unwrap();
        // the program dies here; a new run starts
        let ob = Outbox::new(&root);
        let r = ob.recover(NOW + 60, DATE).unwrap();
        assert_eq!(r.unsure, vec![d.id.clone()]);
        let it = ob.get("acct1", &d.id).unwrap();
        assert_eq!(it.state, State::Unsure);
        assert!(it.error.unwrap().contains("closed while"));
        assert!(ob.begin_send("acct1", &d.id, NOW + 99).is_err(), "not sent again by itself");
        assert_eq!(ob.recover(NOW + 99, DATE).unwrap(), Recovery::default(), "and the check changes nothing a second time");
        // only the person's explicit choice queues it again
        let it = ob.send_again("acct1", &d.id, NOW + 100).unwrap();
        assert_eq!(it.state, State::Queued);
        // the end of a send without an answer is the same state
        ob.begin_send("acct1", &d.id, NOW + 100).unwrap();
        let it = ob.finish("acct1", &d.id, Outcome::Unsure("no answer".into()), NOW + 101).unwrap();
        assert_eq!(it.state, State::Unsure);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Every typed text is in a draft, in the outbox or in the trash; none is gone.
    fn text_survives(root: &Path, text: &str) -> bool {
        fn walk(p: &Path, text: &str) -> bool {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                let q = e.path();
                if q.is_dir() {
                    if walk(&q, text) {
                        return true;
                    }
                } else if q.extension().is_some_and(|x| x == "json") && std::fs::read_to_string(&q).is_ok_and(|s| s.contains(text)) {
                    return true;
                }
            }
            false
        }
        walk(root, text)
    }

    // spec: MAIL-130, MAIL-132, MAIL-133
    #[test]
    fn a_crash_at_every_step_of_queuing_undoing_and_sending_loses_no_text_and_sends_nothing() {
        for n in 0..12 {
            let root = temp(&format!("crash-q{n}"));
            let ob = Outbox::new(&root);
            let d = draft("acct1", "Subject", "UNIQUE-TEXT-queue");
            ob.save_draft(&d, NOW).unwrap();
            ob.crash_at_step(n);
            let queued = ob.queue(&d, &built(&d), NOW, NOW + 10, DATE);
            ob.no_crash();
            assert!(text_survives(&root, "UNIQUE-TEXT-queue"), "queue, crash at {n}");
            // the next start
            let ob = Outbox::new(&root);
            ob.recover(NOW + 1, DATE).unwrap();
            let (drafts, items) = (ob.list_drafts(None), ob.list(None));
            assert!(drafts.len() + items.len() >= 1, "queue, crash at {n}: the mail is somewhere");
            assert!(drafts.is_empty() || items.is_empty(), "queue, crash at {n}: not both a draft and a queued mail: {} drafts, {} items", drafts.len(), items.len());
            assert!(items.iter().all(|i| i.state == State::Queued), "queued, never sending");
            if queued.is_ok() {
                assert_eq!(items.len(), 1);
            }
            let _ = std::fs::remove_dir_all(&root);
        }
        for n in 0..14 {
            let root = temp(&format!("crash-u{n}"));
            let ob = Outbox::new(&root);
            let d = draft("acct1", "Subject", "UNIQUE-TEXT-undo");
            ob.queue(&d, &built(&d), NOW, NOW + 10, DATE).unwrap();
            ob.crash_at_step(n);
            let _ = ob.undo("acct1", &d.id, NOW + 2, DATE);
            ob.no_crash();
            assert!(text_survives(&root, "UNIQUE-TEXT-undo"), "undo, crash at {n}");
            let ob = Outbox::new(&root);
            ob.recover(NOW + 3, DATE).unwrap();
            // an undone mail is never queued for sending, whatever happened
            for i in ob.list(None) {
                assert!(i.state == State::Queued && n < 1, "undo, crash at {n}: only a crash before the mark leaves it queued ({:?})", i.state);
                assert!(ob.begin_send("acct1", &i.id, NOW + 99).is_ok() || n >= 1);
            }
            let drafts = ob.list_drafts(None);
            let items = ob.list(None);
            assert_eq!(drafts.len() + items.len(), 1, "undo, crash at {n}: the mail is in exactly one place");
            let _ = std::fs::remove_dir_all(&root);
        }
        for n in 0..4 {
            let root = temp(&format!("crash-s{n}"));
            let ob = Outbox::new(&root);
            let d = draft("acct1", "Subject", "UNIQUE-TEXT-send");
            ob.queue(&d, &built(&d), NOW, NOW, DATE).unwrap();
            ob.crash_at_step(n);
            let began = ob.begin_send("acct1", &d.id, NOW);
            ob.no_crash();
            let ob = Outbox::new(&root);
            ob.recover(NOW + 1, DATE).unwrap();
            let it = ob.get("acct1", &d.id).unwrap();
            // a crash before the state was written: still queued (nothing went out); after it: unsure, never queued again by itself
            assert!(if began.is_ok() { it.state == State::Unsure } else { it.state == State::Queued }, "send, crash at {n}: {:?}", it.state);
            assert!(text_survives(&root, "UNIQUE-TEXT-send"));
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    // spec: MAIL-130, MAIL-132
    #[test]
    fn a_draft_and_an_outbox_entry_with_the_same_id_leave_only_the_outbox_entry() {
        let root = temp("both");
        let ob = Outbox::new(&root);
        let d = draft("acct1", "B", "both");
        ob.save_draft(&d, NOW).unwrap();
        // queuing got as far as the outbox JSON and no further (the draft was not put away)
        ob.crash_at_step(2);
        assert!(ob.queue(&d, &built(&d), NOW, NOW, DATE).is_err());
        assert_eq!((ob.list_drafts(None).len(), ob.list(None).len()), (1, 1), "the cut-off state really has both");
        ob.no_crash();
        let ob = Outbox::new(&root);
        let _ = ob.recover(NOW, DATE).unwrap();
        assert_eq!((ob.list_drafts(None).len(), ob.list(None).len()), (0, 1));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-130
    #[test]
    fn a_damaged_json_is_skipped_and_ids_and_accounts_cannot_escape_the_folder() {
        let root = temp("evil");
        let ob = Outbox::new(&root);
        let d = draft("acct1", "ok", "ok");
        ob.save_draft(&d, NOW).unwrap();
        std::fs::write(root.join("drafts/acct1/broken.json"), b"{ not json").unwrap();
        assert_eq!(ob.list_drafts(None).len(), 1);
        assert!(ob.load_draft("acct1", "broken").is_err());
        assert!(ob.load_draft("acct1", "../../etc/passwd").is_err());
        assert!(ob.load_draft("../x", "id").is_err());
        assert!(ob.discard_draft("a/b", "id", DATE).is_err());
        assert!(ob.get("acct1", "..").is_none());
        assert!(files(&root).iter().any(|p| p == "drafts/acct1/broken.json"), "the damaged file is left where it is");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-130
    #[test]
    fn a_missing_attachment_file_is_told_and_the_rest_of_the_draft_opens() {
        let root = temp("missing-att");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "M", "text");
        d.attachments.push(Att { name: "a.txt".into(), mime: "text/plain".into(), data: Arc::new(b"x".to_vec()), file: String::new() });
        ob.save_draft(&d, NOW).unwrap();
        let dir = root.join(format!("drafts/acct1/{}.att", d.id));
        for e in std::fs::read_dir(&dir).unwrap().flatten() {
            std::fs::rename(e.path(), root.join("moved-away")).unwrap();
        }
        let (back, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert_eq!(back.body, "text");
        assert!(problems[0].contains("a.txt"), "{problems:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-200
    #[test]
    fn an_attachment_replaced_by_other_bytes_of_the_same_size_is_saved_and_the_old_file_goes_to_the_trash() {
        let root = temp("same-size");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "S", "text");
        d.attachments.push(Att { name: "a.txt".into(), mime: "text/plain".into(), data: Arc::new(b"old-bytes".to_vec()), file: String::new() });
        ob.save_draft(&d, NOW).unwrap();
        d.attachments[0].data = Arc::new(b"new-bytes".to_vec());
        ob.save_draft(&d, NOW + 1).unwrap();
        let (back, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back.attachments[0].data.as_slice(), b"new-bytes");
        let trashed: Vec<String> = files(&root).into_iter().filter(|p| p.starts_with(".trash/") && p.contains(".att/")).collect();
        assert_eq!(trashed.len(), 1, "{trashed:?}");
        assert_eq!(std::fs::read(root.join(&trashed[0])).unwrap(), b"old-bytes");
        // saving the same bytes again moves nothing
        ob.save_draft(&d, NOW + 2).unwrap();
        assert_eq!(files(&root).iter().filter(|p| p.starts_with(".trash/") && p.contains(".att/")).count(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
    /// A draft with one attachment "report.pdf" saved, then the same name and size with other bytes, cut off at step `n`.
    pub(crate) fn cut_off_replacement(tag: &str, n: usize) -> (PathBuf, Outbox, Draft) {
        let root = temp(tag);
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "R", "text");
        d.attachments.push(Att { name: "report.pdf".into(), mime: "application/pdf".into(), data: Arc::new(b"old-bytes".to_vec()), file: String::new() });
        ob.save_draft(&d, NOW).unwrap();
        d.attachments[0].data = Arc::new(b"new-bytes".to_vec());
        ob.crash_at_step(n);
        assert!(ob.save_draft(&d, NOW + 1).is_err());
        ob.no_crash();
        let ob = Outbox::new(&root);
        (root, ob, d)
    }

    fn trashed_atts(root: &Path) -> Vec<String> {
        files(root).into_iter().filter(|p| p.starts_with(".trash/") && p.contains(".att/")).collect()
    }

    // spec: MAIL-215
    #[test]
    fn the_intent_to_replace_an_attachment_is_on_disk_before_the_old_file_moves_and_gone_after_the_new_one_is_written() {
        // cut off after the record, before the move: the record names the file and the trash path, the old file is still there
        let (root, _, d) = cut_off_replacement("repl-record", 1);
        let rec = std::fs::read_to_string(root.join(format!("drafts/acct1/{}.attrepl", d.id))).unwrap();
        assert!(rec.contains("0-9-report.pdf") && rec.contains(".trash/") && rec.contains("report.pdf"), "{rec}");
        assert!(files(&root).iter().any(|p| p.ends_with(".att/0-9-report.pdf")));
        assert!(trashed_atts(&root).is_empty());
        let _ = std::fs::remove_dir_all(&root);
        // an uncut save leaves no record, and the old file is in the trash
        let root = temp("repl-done");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "R", "text");
        d.attachments.push(Att { name: "report.pdf".into(), mime: "application/pdf".into(), data: Arc::new(b"old-bytes".to_vec()), file: String::new() });
        ob.save_draft(&d, NOW).unwrap();
        d.attachments[0].data = Arc::new(b"new-bytes".to_vec());
        ob.save_draft(&d, NOW + 1).unwrap();
        assert!(!files(&root).iter().any(|p| p.ends_with(".attrepl")), "{:?}", files(&root));
        assert_eq!(trashed_atts(&root).len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-216
    #[test]
    fn after_a_cut_off_replacement_the_draft_says_the_attachment_is_missing_and_where_the_old_one_is() {
        let (root, ob, d) = cut_off_replacement("repl-alert", 2);
        let (back, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert_eq!(problems.len(), 1, "the file is missing");
        let alerts = ob.att_alerts("drafts", "acct1", &d.id, &back.attachments);
        assert_eq!(alerts.len(), 1);
        let m = alerts[0].message();
        assert!(m.starts_with("The attachment “report.pdf” is missing: Shale stopped while replacing it. The previous version is in the trash at "), "{m}");
        assert!(m.contains(&alerts[0].trash.display().to_string()) && alerts[0].trash_present);
        // asking again keeps the record (nothing was decided)
        assert_eq!(ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).len(), 1);
        // saving the draft as it is (the bytes were not read back) does not turn the missing file into an empty attachment
        ob.save_draft(&back, NOW + 2).unwrap();
        assert!(!files(&root).iter().any(|p| p.contains(".att/0-0-")), "{:?}", files(&root));
        assert_eq!(ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).len(), 1);
        // cut off before the move: the old file is still the attachment, the record goes silently
        let _ = std::fs::remove_dir_all(&root);
        let (root, ob, d) = cut_off_replacement("repl-silent", 1);
        let (back, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert!(problems.is_empty());
        assert!(ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).is_empty());
        assert!(!files(&root).iter().any(|p| p.ends_with(".attrepl")));
        let _ = std::fs::remove_dir_all(&root);
        // a record whose new file was written completely (cut off after the write): silent too
        let (root, ob, d) = cut_off_replacement("repl-whole", 2);
        let dir = root.join(format!("drafts/acct1/{}.att", d.id));
        std::fs::write(dir.join("0-9-report.pdf"), b"new-bytes").unwrap();
        let (back, _) = ob.load_draft("acct1", &d.id).unwrap();
        assert!(ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).is_empty());
        assert!(!files(&root).iter().any(|p| p.ends_with(".attrepl")));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-217
    #[test]
    fn the_old_attachment_is_restored_from_the_trash_and_the_trash_copy_stays() {
        let (root, ob, d) = cut_off_replacement("repl-restore", 2);
        let (back, _) = ob.load_draft("acct1", &d.id).unwrap();
        let alert = ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).remove(0);
        assert_eq!(ob.restore_att("drafts", "acct1", &d.id, &alert.file).unwrap(), b"old-bytes");
        let (back, problems) = ob.load_draft("acct1", &d.id).unwrap();
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(back.attachments[0].data.as_slice(), b"old-bytes");
        assert!(ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).is_empty());
        assert_eq!(std::fs::read(&alert.trash).unwrap(), b"old-bytes", "the trash copy is not deleted");
        assert!(ob.restore_att("drafts", "acct1", &d.id, &alert.file).is_err());
        let _ = std::fs::remove_dir_all(&root);
        // the old file gone from the trash too: said, not invented
        let (root, ob, d) = cut_off_replacement("repl-restore-gone", 2);
        let (back, _) = ob.load_draft("acct1", &d.id).unwrap();
        let alert = ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).remove(0);
        std::fs::rename(&alert.trash, root.join("elsewhere")).unwrap();
        assert!(ob.restore_att("drafts", "acct1", &d.id, &alert.file).is_err());
        let again = ob.att_alerts("drafts", "acct1", &d.id, &back.attachments);
        assert!(!again[0].trash_present && again[0].message().contains("not in the trash any more"));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: MAIL-218
    #[test]
    fn an_outbox_entry_with_a_missing_attachment_file_is_not_sent_and_says_why() {
        let root = temp("send-missing");
        let ob = Outbox::new(&root);
        let mut d = draft("acct1", "S", "text");
        d.attachments.push(Att { name: "report.pdf".into(), mime: "application/pdf".into(), data: Arc::new(b"bytes".to_vec()), file: String::new() });
        let item = ob.queue(&d, &built(&d), NOW, NOW, DATE).unwrap();
        let att = root.join(format!("outbox/acct1/{}.att/0-5-report.pdf", d.id));
        std::fs::rename(&att, root.join("elsewhere")).unwrap();
        let e = ob.begin_send("acct1", &item.id, NOW + 1).unwrap_err();
        assert!(e.contains("“report.pdf” is missing") && e.contains("was not sent"), "{e}");
        let now = ob.get("acct1", &item.id).unwrap();
        assert_eq!(now.state, State::Failed);
        assert_eq!(now.error.as_deref(), Some(e.as_str()));
        assert!(ob.begin_send("acct1", &item.id, NOW + 2).is_err(), "and it stays unsent");
        // with the file there, a queued entry is sent as before
        let root2 = temp("send-present");
        let ob2 = Outbox::new(&root2);
        let item2 = ob2.queue(&d, &built(&d), NOW, NOW, DATE).unwrap();
        assert!(ob2.begin_send("acct1", &item2.id, NOW + 1).is_ok());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    // spec: MAIL-219
    #[test]
    fn removing_the_missing_attachment_from_the_draft_ends_the_alert_and_never_deletes_the_trash_copy() {
        let (root, ob, d) = cut_off_replacement("repl-remove", 2);
        let (mut back, _) = ob.load_draft("acct1", &d.id).unwrap();
        let alert = ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).remove(0);
        back.attachments.clear();
        ob.save_draft(&back, NOW + 3).unwrap();
        assert!(ob.att_alerts("drafts", "acct1", &d.id, &back.attachments).is_empty());
        assert!(!files(&root).iter().any(|p| p.ends_with(".attrepl")));
        assert_eq!(std::fs::read(&alert.trash).unwrap(), b"old-bytes");
        // discarding a draft with a record puts the record away with it
        let (root2, ob2, d2) = cut_off_replacement("repl-discard", 2);
        ob2.discard_draft("acct1", &d2.id, DATE).unwrap();
        assert!(!files(&root2).iter().any(|p| p.starts_with("drafts/")), "{:?}", files(&root2));
        assert!(files(&root2).iter().any(|p| p.starts_with(".trash/") && p.ends_with(".attrepl")));
        assert_eq!(trashed_atts(&root2).iter().filter(|p| !p.ends_with(".attrepl")).count(), 1, "{:?}", trashed_atts(&root2));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }
}
