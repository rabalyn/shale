//! Two-way mail (ADR-0009, MAIL-102 to MAIL-108): mark read, flag, move, delete.
//!
//! Every action of the person is first **recorded** ([`enqueue`]): one `pending_ops` row per
//! message, and, for flags, the optimistic change of the local flags, in one transaction. A job
//! ([`run_pending`]) then sends what is recorded, grouped by folder and action, through the
//! write chokepoint of the IMAP client ([`Client::write`]). Only after the server confirmed does
//! the index change (one transaction: the new state and the end of the pending row together).
//! If the server cannot be reached the row stays and is tried again later; if the server refuses,
//! the optimistic change is undone and the person is told why. A failed action never changes local
//! state as if it had worked.
//!
//! Safety rules (the owner's: nothing is lost because of a bug):
//!
//! - Delete is a move to Trash. The only other way a message leaves a folder is "delete forever",
//!   which exists for Trash only and needs a [`Confirmed`] that names the count.
//! - A server without `MOVE` gets `COPY`, a verified copy, then `\Deleted` and `UID EXPUNGE` of the
//!   verified UIDs only. Without `UIDPLUS` that is refused (nothing is changed).
//! - Before a write the folder is opened with `SELECT`, its `UIDVALIDITY` must be the one the index
//!   knows, and a `UID SEARCH` says which of the UIDs still exist (mail that another program moved
//!   or deleted is left alone and marked `gone_on_server`).
//! - A message file is never deleted: it is copied to the new place when the new UID is known, and
//!   the old file goes to `.trash/<date>/` ([`MailStore::archive_file`]).

use crate::imap_client::{Client, CopyUid, Fetched, ImapError, MailFlag, WriteCmd};
use crate::imap_sync::{kind_of, Connector, FolderKind};
use crate::mail_db::{self, change_flags, has_flag, FolderRow, MessageSummary, Op, PendingRow, STATE_FULL};
use crate::mail_store::{today_string, MailStore};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

/// After this many failed tries an action that could not reach the server is given up (rolled back).
pub const MAX_ATTEMPTS: i64 = 12;

/// What the person asked for.
#[derive(Clone, Debug)]
pub enum Action {
    SetFlags {
        add: Vec<MailFlag>,
        remove: Vec<MailFlag>,
    },
    /// Move to a folder of the same account (its name as the server spells it).
    MoveTo {
        folder: String,
    },
    /// Move to the account's Trash folder.
    Delete,
    /// Remove from Trash for good. See [`Confirmed`].
    DeleteForever(Confirmed),
}

/// The person has confirmed "delete forever" for this many messages (MAIL-105). Only the
/// confirmation dialog makes one; [`enqueue`] refuses when the count is not the one asked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Confirmed {
    count: usize,
}

impl Confirmed {
    /// Call this only from the confirmation dialog, with the count the dialog named.
    pub fn after_dialog(count: usize) -> Confirmed {
        Confirmed { count }
    }
}

/// What [`enqueue`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Enqueued {
    pub queued: usize,
    /// Messages that were left out, and why (already gone, already waiting, nothing to change …).
    pub notes: Vec<String>,
}

/// What a run of [`run_pending`] did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OpsReport {
    /// Messages whose action the server did.
    pub done: usize,
    /// Messages found already gone from their folder on the server (moved or deleted elsewhere).
    pub gone: usize,
    /// Actions the server refused or that cannot be done; they were undone. Words for the person.
    pub failed: Vec<String>,
    /// The server could not be reached (or broke the connection): the rest is still pending.
    pub offline: Option<String>,
    /// Actions still waiting after this run.
    pub remaining: usize,
    /// Other things worth telling.
    pub notes: Vec<String>,
}

fn flag_text(f: &[MailFlag]) -> String {
    f.iter().map(|x| x.as_str()).collect::<Vec<_>>().join(" ")
}

fn flags_from(text: &str) -> Vec<MailFlag> {
    let mut v = vec![];
    for f in text.split(' ') {
        if f.eq_ignore_ascii_case("\\Seen") {
            v.push(MailFlag::Seen);
        } else if f.eq_ignore_ascii_case("\\Flagged") {
            v.push(MailFlag::Flagged);
        }
    }
    v
}

/// The Trash folder of an account: the special-use one, else the usual names.
pub fn trash_folder(folders: &[FolderRow]) -> Option<&FolderRow> {
    let trash = || folders.iter().filter(|f| kind_of(f) == FolderKind::Trash);
    trash().find(|f| f.special_use.as_deref().unwrap_or("").to_ascii_lowercase().contains("\\trash")).or_else(|| trash().next())
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---------------------------------------------------------------- recording an action

/// Record an action for these messages (MAIL-107). Returns at once; nothing is sent. Flags change
/// in the index right away (the optimistic effect); move and delete leave the message where it is,
/// marked as waiting. Fails (nothing recorded) for a delete without a Trash folder, a move
/// to a folder that is not known, or a delete forever that is not in Trash or not confirmed.
pub fn enqueue(store: &MailStore, msgs: &[MessageSummary], action: &Action, now: i64) -> Result<Enqueued, String> {
    let reader = store.reader().map_err(err)?;
    let mut out = Enqueued::default();
    let mut ops: Vec<Op> = vec![];
    // what an action needs to know about the account, looked up once per account
    let mut accounts: HashMap<String, Vec<FolderRow>> = HashMap::new();
    if let Action::DeleteForever(c) = action {
        if c.count != msgs.len() {
            return Err(format!("delete forever was confirmed for {} message(s), not for {}", c.count, msgs.len()));
        }
    }
    for m in msgs {
        // what the index says now (the list the person clicked in may be a moment old)
        let Some(cur) = reader.message_by_id(m.id).map_err(err)? else {
            out.notes.push(format!("“{}” is not in the local copy any more", short(&m.subject)));
            continue;
        };
        let label = short(&cur.subject);
        if cur.gone_on_server {
            out.notes.push(format!("“{label}” is already gone from the server"));
            continue;
        }
        if cur.pending.as_deref() == Some("append") {
            out.notes.push(format!("“{label}” is still being saved to the server"));
            continue;
        }
        if cur.pending.as_deref().is_some_and(|k| k != "flags") {
            out.notes.push(format!("“{label}” is already waiting to be moved"));
            continue;
        }
        let folders = match accounts.get(&cur.account) {
            Some(f) => f.clone(),
            None => {
                let f = reader.folders(Some(&cur.account)).map_err(err)?;
                accounts.insert(cur.account.clone(), f.clone());
                f
            }
        };
        let here = folders.iter().find(|f| f.id == cur.folder_id).ok_or("the folder of the message is not in the index")?;
        let mut row = PendingRow {
            id: 0,
            account: cur.account.clone(),
            kind: String::new(),
            folder: cur.folder_id,
            uidvalidity: cur.uidvalidity,
            uid: cur.uid,
            target: None,
            flags_add: String::new(),
            flags_remove: String::new(),
            prev_flags: String::new(),
            created: now,
            attempts: 0,
            last_error: None,
        };
        match action {
            Action::SetFlags { add, remove } => {
                let new = change_flags(&cur.flags, &flag_text(add), &flag_text(remove));
                if new == mail_db::canonical_flags(cur.flags.split(' ').map(String::from)) {
                    continue; // nothing to change
                }
                row.kind = "flags".into();
                row.flags_add = flag_text(add);
                row.flags_remove = flag_text(remove);
            }
            Action::MoveTo { folder } => {
                if folder == &here.name {
                    out.notes.push(format!("“{label}” is already in that folder"));
                    continue;
                }
                if !folders.iter().any(|f| &f.name == folder) {
                    return Err(format!("the folder {folder:?} is not known for this account"));
                }
                row.kind = "move".into();
                row.target = Some(folder.clone());
            }
            Action::Delete => {
                let trash = trash_folder(&folders)
                    .ok_or("This account has no Trash folder that Shale knows of, so nothing can be deleted. Sync the account (or use “Test connection”) so that its folders are known, or move the mail to a folder of your choice.")?;
                if trash.id == here.id {
                    out.notes.push(format!("“{label}” is already in Trash (use “Delete forever” there)"));
                    continue;
                }
                row.kind = "delete".into();
                row.target = Some(trash.name.clone());
            }
            Action::DeleteForever(_) => {
                if kind_of(here) != FolderKind::Trash {
                    return Err("“Delete forever” only exists for mail that is already in Trash".into());
                }
                row.kind = "expunge".into();
            }
        }
        ops.push(Op::PendingAdd(Box::new(row)));
        out.queued += 1;
    }
    store.commit(ops).map_err(err)?;
    Ok(out)
}

fn short(s: &str) -> String {
    let t = s.trim();
    if t.is_empty() {
        return "(no subject)".into();
    }
    t.chars().take(40).collect()
}

// ---------------------------------------------------------------- sending what is recorded

/// Only one run per store and account at a time (a second one would repeat the first's work).
static RUNNING: Mutex<Vec<String>> = Mutex::new(Vec::new());

struct RunGuard(String);

impl Drop for RunGuard {
    fn drop(&mut self) {
        RUNNING.lock().unwrap_or_else(|e| e.into_inner()).retain(|k| k != &self.0);
    }
}

/// How a group of actions ended badly.
enum Fail {
    /// Try again later: the server could not be reached, or said it is busy.
    Retry(String),
    /// Will not work: undo the optimistic change and tell the person.
    Refused(String),
}

fn classify(e: ImapError) -> Fail {
    match &e {
        ImapError::Connection(_) | ImapError::Tls(_) | ImapError::Protocol(_) | ImapError::Auth(_) => Fail::Retry(e.to_string()),
        ImapError::Server(t) if t.contains("[UNAVAILABLE]") || t.contains("[INUSE]") || t.contains("[LIMIT]") => Fail::Retry(e.to_string()),
        _ => Fail::Refused(e.to_string()),
    }
}

/// What a group did: the index changes to commit (after the server confirmed), the files to put
/// into the local trash afterwards, and what to tell.
#[derive(Default)]
struct Done {
    ops: Vec<Op>,
    archive: Vec<String>,
    done: usize,
    gone: usize,
    failed: Vec<String>,
    notes: Vec<String>,
}

/// Send the recorded actions of one account (MAIL-107). Blocks: run it as a job. Never panics on
/// bad answers; a panic inside is caught by the caller's job.
pub fn run_pending(store: &MailStore, connector: &dyn Connector, account: &str) -> OpsReport {
    let mut report = OpsReport::default();
    let key = format!("{}|{account}", store.index_path().display());
    {
        let mut running = RUNNING.lock().unwrap_or_else(|e| e.into_inner());
        if running.contains(&key) {
            report.notes.push("another run for this account is going on".into());
            return report;
        }
        running.push(key.clone());
    }
    let _guard = RunGuard(key);
    let reader = match store.reader() {
        Ok(r) => r,
        Err(e) => {
            report.failed.push(err(e));
            return report;
        }
    };
    let all = match reader.pending_ops(Some(account)) {
        Ok(v) => v,
        Err(e) => {
            report.failed.push(err(e));
            return report;
        }
    };
    if all.is_empty() {
        return report;
    }
    let mut client = match connector.connect() {
        Ok(c) => c,
        Err(e) => {
            let text = e.to_string();
            let ops = all.iter().map(|p| Op::PendingTried { id: p.id, error: text.clone() }).collect();
            let _ = store.commit(ops);
            report.offline = Some(text);
            report.remaining = all.len();
            return report;
        }
    };
    for group in groups(&all) {
        let result = crate::jobs::catch(|| exec_group(store, &mut client, &group, &all)).unwrap_or_else(|p| Err(Fail::Refused(format!("an internal error: {p}"))));
        let ids: Vec<i64> = group.iter().map(|p| p.id).collect();
        match result {
            Ok(d) => {
                let mut ops = d.ops;
                let missing: Vec<Op> = ids.iter().filter(|id| !ops_mention(&ops, **id)).map(|id| Op::PendingDone { id: *id }).collect();
                ops.extend(missing);
                match store.commit(ops) {
                    Ok(()) => {
                        let date = today_string();
                        for rel in &d.archive {
                            if let Err(e) = store.archive_file(rel, &date) {
                                report.notes.push(format!("the file {rel} could not be moved to the local trash: {e}"));
                            }
                        }
                        report.done += d.done;
                        report.gone += d.gone;
                        report.failed.extend(d.failed);
                        report.notes.extend(d.notes);
                    }
                    Err(e) => {
                        // the server did it, the index could not follow: the rows stay, and the next try
                        // finds the messages gone from their folder and settles it that way
                        report.failed.push(format!("the server did the change but the local copy could not be updated: {e}"));
                        report.remaining += ids.len();
                    }
                }
            }
            Err(Fail::Retry(text)) => {
                let too_often = group.iter().any(|p| p.attempts + 1 >= MAX_ATTEMPTS);
                if too_often {
                    let _ = store.commit(ids.iter().map(|id| Op::PendingRollback { id: *id }).collect());
                    report.failed.push(format!("gave up after {MAX_ATTEMPTS} tries: {text}"));
                } else {
                    let _ = store.commit(ids.iter().map(|id| Op::PendingTried { id: *id, error: text.clone() }).collect());
                    report.offline = Some(text);
                    // the connection may be out of step: stop here, the rest waits for the next run
                    break;
                }
            }
            Err(Fail::Refused(text)) => {
                let _ = store.commit(ids.iter().map(|id| Op::PendingRollback { id: *id }).collect());
                report.failed.push(text);
            }
        }
    }
    report.remaining = store.reader().and_then(|r| r.pending_ops(Some(account))).map(|v| v.len()).unwrap_or(report.remaining);
    client.logout();
    report
}

fn ops_mention(ops: &[Op], id: i64) -> bool {
    ops.iter().any(|o| matches!(o, Op::PendingDone { id: i } | Op::PendingRollback { id: i } if *i == id))
}

/// Consecutive actions of the same kind on the same folder with the same details go together.
fn groups(all: &[PendingRow]) -> Vec<Vec<PendingRow>> {
    let key = |p: &PendingRow| (if p.kind == "delete" { "move".to_string() } else { p.kind.clone() }, p.folder, p.uidvalidity, p.target.clone(), p.flags_add.clone(), p.flags_remove.clone());
    let mut out: Vec<Vec<PendingRow>> = vec![];
    for p in all {
        match out.last_mut() {
            Some(g) if key(&g[0]) == key(p) => g.push(p.clone()),
            _ => out.push(vec![p.clone()]),
        }
    }
    out
}

/// Open the group's folder for writing and check it is still the folder the index knows.
fn open_for_write(client: &mut Client, folder: &FolderRow, uidvalidity: u32) -> Result<crate::imap_client::Selected, Fail> {
    let sel = client.write(&WriteCmd::Select { folder: folder.name.clone() }).map_err(classify)?.selected.ok_or_else(|| Fail::Retry("the server did not describe the folder".into()))?;
    if sel.uidvalidity != uidvalidity {
        return Err(Fail::Refused(format!(
            "The folder “{}” was renumbered on the server, so the stored message numbers no longer mean the same mails. Nothing was changed; the next sync fetches the folder again.",
            crate::mail_ui::folder_label_of(&folder.name)
        )));
    }
    Ok(sel)
}

fn exec_group(store: &MailStore, client: &mut Client, group: &[PendingRow], all: &[PendingRow]) -> Result<Done, Fail> {
    let reader = store.reader().map_err(|e| Fail::Retry(err(e)))?;
    let first = &group[0];
    let folder = reader.folder_by_id(first.folder).map_err(|e| Fail::Refused(err(e)))?.ok_or_else(|| Fail::Refused("the folder is not in the index any more".into()))?;
    let mut uids: Vec<u32> = group.iter().map(|p| p.uid).collect();
    uids.sort_unstable();
    uids.dedup();
    match first.kind.as_str() {
        "flags" => exec_flags(client, &folder, group, &uids, all),
        "move" | "delete" => exec_move(store, client, &folder, group, &uids),
        "expunge" => exec_expunge(store, client, &folder, group, &uids),
        "append" => exec_append(store, client, &folder, group),
        other => Err(Fail::Refused(format!("an action of kind {other:?} is not known to this version"))),
    }
}

// ---------------------------------------------------------------- flags

fn exec_flags(client: &mut Client, folder: &FolderRow, group: &[PendingRow], uids: &[u32], all: &[PendingRow]) -> Result<Done, Fail> {
    let first = &group[0];
    open_for_write(client, folder, first.uidvalidity)?;
    let present = client.uid_exists(uids).map_err(classify)?;
    let mut d = Done::default();
    let (add, remove) = (flags_from(&first.flags_add), flags_from(&first.flags_remove));
    // conditional store: only if nobody changed the message since the last sync (MAIL-102)
    let mut since = if client.has_condstore() { folder.highestmodseq } else { None };
    let mut modified_left: Vec<u32> = vec![];
    for (flags, set) in [(add, true), (remove, false)] {
        if flags.is_empty() || present.is_empty() {
            continue;
        }
        let out = client.write(&WriteCmd::StoreFlags { uids: present.clone(), add: set, flags: flags.clone(), unchanged_since: since }).map_err(classify)?;
        if !out.modified.is_empty() {
            // another client changed these: read them again and apply our change on top
            let fresh = client.uid_fetch_flags_of(&out.modified).map_err(classify)?;
            let newest = fresh.iter().filter_map(|f| f.modseq).max();
            let retry = client.write(&WriteCmd::StoreFlags { uids: out.modified.clone(), add: set, flags, unchanged_since: newest }).map_err(classify)?;
            modified_left.extend(retry.modified);
        }
        since = None;
    }
    if !modified_left.is_empty() {
        return Err(Fail::Retry("the message was changed on the server again and again; it will be tried later".into()));
    }
    let after: HashMap<u32, Fetched> = client.uid_fetch_flags_of(&present).map_err(classify)?.into_iter().map(|f| (f.uid, f)).collect();
    let gone: HashSet<u32> = uids.iter().copied().filter(|u| !after.contains_key(u)).collect();
    for p in group {
        if let Some(f) = after.get(&p.uid) {
            // server truth, then the actions that are still waiting for this message on top
            let mut flags = f.flags.clone().unwrap_or_default();
            for later in all.iter().filter(|o| o.kind == "flags" && (o.folder, o.uidvalidity, o.uid) == (p.folder, p.uidvalidity, p.uid) && o.id > p.id && !group.iter().any(|g| g.id == o.id)) {
                flags = change_flags(&flags, &later.flags_add, &later.flags_remove);
            }
            d.ops.push(Op::Flags { folder: p.folder, uidvalidity: p.uidvalidity, uid: p.uid, flags });
            d.done += 1;
        } else {
            d.ops.push(Op::PendingRollback { id: p.id });
            d.ops.push(Op::Gone { folder: p.folder, uidvalidity: p.uidvalidity, uid: p.uid, gone: true });
            d.gone += 1;
        }
    }
    if !gone.is_empty() {
        d.notes.push(format!(
            "{} message{} no longer exist{} in this folder on the server (moved or deleted by another program); the local copy is kept",
            gone.len(),
            if gone.len() == 1 { "" } else { "s" },
            if gone.len() == 1 { "s" } else { "" }
        ));
    }
    Ok(d)
}

// ---------------------------------------------------------------- move and delete

/// A message that has left its folder on the server: its UID in the target folder, if known.
struct Moved {
    uid: u32,
    new_uid: Option<u32>,
}

/// The result of moving on the server.
struct MoveResult {
    moved: Vec<Moved>,
    /// Still in the source folder (not moved).
    left: Vec<u32>,
    uidvalidity_target: Option<u32>,
    /// A problem after the message was copied: it exists in both folders now. Words for the person.
    both: Option<String>,
}

fn exec_move(store: &MailStore, client: &mut Client, folder: &FolderRow, group: &[PendingRow], uids: &[u32]) -> Result<Done, Fail> {
    let reader = store.reader().map_err(|e| Fail::Retry(err(e)))?;
    let first = &group[0];
    let target = first.target.clone().ok_or_else(|| Fail::Refused("a move without a target folder".into()))?;
    if target == folder.name {
        return Err(Fail::Refused("the message is already in that folder".into()));
    }
    open_for_write(client, folder, first.uidvalidity)?;
    let present = client.uid_exists(uids).map_err(classify)?;
    let mut d = Done::default();
    let mut res = MoveResult { moved: vec![], left: vec![], uidvalidity_target: None, both: None };
    if !present.is_empty() {
        if client.can_move() {
            let out = client.write(&WriteCmd::Move { uids: present.clone(), to: target.clone() }).map_err(classify)?;
            let map = copyuid_map(&out.copyuid);
            res.uidvalidity_target = out.copyuid.as_ref().map(|c| c.uidvalidity);
            let still = client.uid_exists(&present).map_err(classify)?;
            for u in &present {
                if still.contains(u) {
                    res.left.push(*u);
                } else {
                    res.moved.push(Moved { uid: *u, new_uid: map.get(u).copied() });
                }
            }
        } else {
            copy_then_delete(store, client, folder, &target, &present, first.uidvalidity, group, &mut res)?;
        }
    }
    let target_row = reader.folders(Some(&first.account)).map_err(|e| Fail::Refused(err(e)))?.into_iter().find(|f| f.name == target);
    let verb = if first.kind == "delete" { "deleted" } else { "moved" };
    for p in group {
        let Some(msg) = reader.message_at(p.folder, p.uidvalidity, p.uid).map_err(|e| Fail::Refused(err(e)))? else {
            d.ops.push(Op::PendingDone { id: p.id });
            continue;
        };
        if let Some(m) = res.moved.iter().find(|m| m.uid == p.uid) {
            d.done += 1;
            // the message's row follows it to the new folder when we know its new UID and have its file
            if let (Some(new_uid), Some(uv), Some(trow)) = (m.new_uid, res.uidvalidity_target, target_row.as_ref()) {
                if msg.state == STATE_FULL && !msg.path.is_empty() {
                    if let Ok(raw) = store.read_raw(&msg) {
                        match store.save_message(trow, uv, new_uid, &raw, &msg.flags, msg.date) {
                            Ok(nm) => {
                                if trow.uidvalidity.is_none() {
                                    d.ops.push(Op::FolderState { folder: trow.id, uidvalidity: Some(uv), uidnext: None, highestmodseq: None, last_sync: 0 });
                                }
                                if trow.uidvalidity.is_none() || trow.uidvalidity == Some(uv) {
                                    let carried = store.carry_decrypted(&msg, &nm);
                                    d.ops.push(Op::message(nm));
                                    d.ops.extend(carried);
                                }
                            }
                            Err(e) => d.notes.push(format!("the local copy of “{}” could not be written to the new folder (the next sync fetches it there): {e}", short(&msg.subject))),
                        }
                    }
                }
            }
            d.ops.push(Op::Remove { folder: p.folder, uidvalidity: p.uidvalidity, uid: p.uid });
            if !msg.path.is_empty() {
                d.archive.push(msg.path.clone());
            }
            d.ops.push(Op::PendingDone { id: p.id });
        } else if present.contains(&p.uid) {
            // still in the source folder
            if res.both.is_some() {
                d.ops.push(Op::PendingDone { id: p.id });
            } else {
                d.ops.push(Op::PendingRollback { id: p.id });
                d.failed.push(format!("“{}” could not be {verb}: the server did not do it", short(&msg.subject)));
            }
        } else {
            // not on the server in this folder any more: another program was faster
            d.ops.push(Op::Gone { folder: p.folder, uidvalidity: p.uidvalidity, uid: p.uid, gone: true });
            d.ops.push(Op::PendingDone { id: p.id });
            d.gone += 1;
        }
    }
    if d.gone > 0 {
        d.notes.push(format!("{} message{} had already been moved or deleted on the server by another program; the local copy is kept", d.gone, if d.gone == 1 { "" } else { "s" }));
    }
    if let Some(b) = res.both {
        d.failed.push(b);
    }
    Ok(d)
}

fn copyuid_map(c: &Option<CopyUid>) -> HashMap<u32, u32> {
    c.as_ref().map(|c| c.from.iter().copied().zip(c.to.iter().copied()).collect()).unwrap_or_default()
}

/// The fallback for a server without `MOVE` (MAIL-103): copy, verify the copy, mark the originals
/// `\Deleted`, `UID EXPUNGE` those UIDs. Requires `UIDPLUS`; refused without it, before anything is sent.
#[allow(clippy::too_many_arguments)]
fn copy_then_delete(store: &MailStore, client: &mut Client, folder: &FolderRow, target: &str, present: &[u32], uidvalidity: u32, group: &[PendingRow], res: &mut MoveResult) -> Result<(), Fail> {
    if !client.can_expunge_uids() {
        return Err(Fail::Refused("This server can neither move mail (MOVE) nor remove single mails safely (UIDPLUS), so Shale did not copy or delete anything. Nothing was changed.".into()));
    }
    let reader = store.reader().map_err(|e| Fail::Retry(err(e)))?;
    let out = client.write(&WriteCmd::Copy { uids: present.to_vec(), to: target.to_string() }).map_err(classify)?;
    let mut verified: Vec<Moved> = vec![];
    if let Some(cu) = &out.copyuid {
        res.uidvalidity_target = Some(cu.uidvalidity);
        for (f, t) in cu.from.iter().zip(cu.to.iter()) {
            if present.contains(f) {
                verified.push(Moved { uid: *f, new_uid: Some(*t) });
            }
        }
    } else {
        // no COPYUID: look for each message in the target by its Message-ID
        let tsel = client.examine(target).map_err(classify)?;
        res.uidvalidity_target = Some(tsel.uidvalidity);
        for u in present {
            let mid = group.iter().find(|p| p.uid == *u).and_then(|p| reader.message_at(p.folder, p.uidvalidity, p.uid).ok().flatten()).map(|m| m.message_id).unwrap_or_default();
            if mid.is_empty() {
                continue;
            }
            if let Some(new) = client.uid_search_message_id(&mid).map_err(classify)?.into_iter().max() {
                verified.push(Moved { uid: *u, new_uid: Some(new) });
            }
        }
        // back to the source folder, open for writing again, and check it is the same one
        open_for_write(client, folder, uidvalidity)?;
    }
    // only what is verified in the target is touched in the source
    let verified_uids: Vec<u32> = verified.iter().map(|m| m.uid).collect();
    if verified_uids.is_empty() {
        return Err(Fail::Refused("The copy could not be verified in the target folder, so the originals were left alone. Nothing was lost; the mail may now exist twice.".into()));
    }
    let both = |why: String, client: &mut Client| -> Fail {
        // undo the mark if we can; the mail is safe in both places either way
        let _ = client.write(&WriteCmd::MarkDeleted { uids: verified_uids.clone(), set: false });
        Fail::Refused(format!("The mail was copied to the target folder but the original could not be removed ({why}). It exists in both folders now; nothing was lost."))
    };
    if let Err(e) = client.write(&WriteCmd::MarkDeleted { uids: verified_uids.clone(), set: true }) {
        return Err(both(e.to_string(), client));
    }
    let marked: Vec<u32> = match client.uid_fetch_flags_of(&verified_uids) {
        Ok(v) => v.into_iter().filter(|f| f.flags.as_deref().is_some_and(|fl| has_flag(fl, "\\Deleted"))).map(|f| f.uid).collect(),
        Err(e) => return Err(both(e.to_string(), client)),
    };
    if marked.is_empty() {
        return Err(both("the originals could not be marked".into(), client));
    }
    if let Err(e) = client.write(&WriteCmd::ExpungeUids { uids: marked.clone() }) {
        return Err(both(e.to_string(), client));
    }
    let still = client.uid_exists(&marked).unwrap_or_else(|_| marked.clone());
    for m in verified {
        if marked.contains(&m.uid) && !still.contains(&m.uid) {
            res.moved.push(m);
        }
    }
    res.left = present.iter().copied().filter(|u| !res.moved.iter().any(|m| m.uid == *u)).collect();
    if !res.left.is_empty() {
        res.both = Some("Some mail was copied to the target folder but not removed from the original folder. It exists in both; nothing was lost.".into());
    }
    Ok(())
}

// ---------------------------------------------------------------- delete forever

fn exec_expunge(store: &MailStore, client: &mut Client, folder: &FolderRow, group: &[PendingRow], uids: &[u32]) -> Result<Done, Fail> {
    let reader = store.reader().map_err(|e| Fail::Retry(err(e)))?;
    if kind_of(folder) != FolderKind::Trash {
        return Err(Fail::Refused("“Delete forever” only exists for mail in Trash".into()));
    }
    if !client.can_expunge_uids() {
        // checked before the folder is even opened: nothing is sent
        return Err(Fail::Refused("This server has no UIDPLUS, so single mails cannot be removed safely: nothing was deleted.".into()));
    }
    open_for_write(client, folder, group[0].uidvalidity)?;
    let present = client.uid_exists(uids).map_err(classify)?;
    let mut d = Done::default();
    let mut removed: HashSet<u32> = HashSet::new();
    if !present.is_empty() {
        client.write(&WriteCmd::MarkDeleted { uids: present.clone(), set: true }).map_err(classify)?;
        let marked: Vec<u32> = client.uid_fetch_flags_of(&present).map_err(classify)?.into_iter().filter(|f| f.flags.as_deref().is_some_and(|fl| has_flag(fl, "\\Deleted"))).map(|f| f.uid).collect();
        if !marked.is_empty() {
            client.write(&WriteCmd::ExpungeUids { uids: marked.clone() }).map_err(classify)?;
        }
        let still = client.uid_exists(&marked).map_err(classify)?;
        removed.extend(marked.into_iter().filter(|u| !still.contains(u)));
    }
    for p in group {
        let was_gone = !present.contains(&p.uid);
        if was_gone || removed.contains(&p.uid) {
            if let Some(msg) = reader.message_at(p.folder, p.uidvalidity, p.uid).map_err(|e| Fail::Refused(err(e)))? {
                if !msg.path.is_empty() {
                    d.archive.push(msg.path.clone());
                }
            }
            d.ops.push(Op::Remove { folder: p.folder, uidvalidity: p.uidvalidity, uid: p.uid });
            d.ops.push(Op::PendingDone { id: p.id });
            d.done += 1;
        } else {
            d.ops.push(Op::PendingRollback { id: p.id });
            d.failed.push("a message in Trash could not be deleted: the server did not remove it".into());
        }
    }
    Ok(d)
}

// ---------------------------------------------------------------- the copy of a sent mail (ADR-0010)

/// Local copies of sent mail that the server has not got yet are kept under UIDVALIDITY 0
/// ([`mail_db::LOCAL_UIDVALIDITY`], which a server may not use), with their own file path
/// `<uidvalidity 0>/<uid>.eml`, so no real message can overwrite or merge with one (the index key is
/// folder, UIDVALIDITY, UID). Their UIDs start here, so a list lookup by UID alone finds them apart from
/// real ones in practice too. The sync leaves them alone while the append waits, and replaces them by
/// the server's copy once it has seen one with the same Message-ID ([`drop_local_copies_seen_on_server`]).
pub const SYNTHETIC_UID_BASE: u32 = 4_000_000_000;

/// The Sent folder of an account: the special-use `\Sent` one, else the usual names (MAIL-141).
pub fn sent_folder(folders: &[FolderRow]) -> Option<&FolderRow> {
    let special = folders.iter().find(|f| f.special_use.as_deref().unwrap_or("").to_ascii_lowercase().split(' ').any(|a| a == "\\sent"));
    special.or_else(|| {
        folders.iter().find(|f| {
            let shown = crate::imap_client::decode_mutf7(&f.name);
            let last = shown.rsplit(['/', '.']).next().unwrap_or(&shown).trim().to_lowercase();
            matches!(last.as_str(), "sent" | "sent items" | "sent messages" | "sent mail" | "gesendet" | "gesendete objekte" | "gesendete elemente" | "gesendete nachrichten")
        })
    })
}

/// A mail has been sent: store its exact bytes as a message of the Sent folder (it shows in the list at once)
/// and record the append for the server, in one transaction (MAIL-141). Returns the folder's name, or None
/// when the account has no Sent folder that Shale knows. A copy that is recorded already (same Message-ID in that
/// folder) is not recorded twice.
pub fn record_sent_copy(store: &MailStore, account: &str, raw: &[u8], now: i64) -> Result<Option<String>, String> {
    let reader = store.reader().map_err(err)?;
    let folders = reader.folders(Some(account)).map_err(err)?;
    let Some(sent) = sent_folder(&folders) else { return Ok(None) };
    let mid = crate::mail_store::index_fields(raw, Some(now)).0.message_id;
    if !mid.is_empty() && mail_db::has_message_id(reader.conn(), sent.id, &mid).map_err(err)? {
        return Ok(Some(sent.name.clone()));
    }
    let uv = mail_db::LOCAL_UIDVALIDITY;
    let uid = mail_db::max_uid_from(reader.conn(), sent.id, uv, SYNTHETIC_UID_BASE).map_err(err)?.map_or(SYNTHETIC_UID_BASE, |m| m + 1);
    let nm = store.save_message(sent, uv, uid, raw, "\\Seen", Some(now)).map_err(err)?;
    let row = PendingRow {
        id: 0,
        account: account.into(),
        kind: "append".into(),
        folder: sent.id,
        uidvalidity: uv,
        uid,
        target: Some(sent.name.clone()),
        flags_add: "\\Seen".into(),
        flags_remove: String::new(),
        prev_flags: String::new(),
        created: now,
        attempts: 0,
        last_error: None,
    };
    store.commit(vec![Op::message(nm), Op::PendingAdd(Box::new(row))]).map_err(err)?;
    Ok(Some(sent.name.clone()))
}

/// The server has a message with the Message-ID of a local copy of a sent mail (and nothing waits for the
/// copy): the server's row stands, the local row goes and its file moves to the local trash, never deleted.
/// Returns how many were replaced.
pub fn drop_local_copies_seen_on_server(store: &MailStore, folder: i64, uidvalidity: u32) -> Result<usize, String> {
    let reader = store.reader().map_err(err)?;
    let seen = mail_db::local_copies_seen_on_server(reader.conn(), folder, uidvalidity).map_err(err)?;
    if seen.is_empty() {
        return Ok(0);
    }
    store.commit(seen.iter().map(|(uid, _)| Op::Remove { folder, uidvalidity: mail_db::LOCAL_UIDVALIDITY, uid: *uid }).collect()).map_err(err)?;
    let date = today_string();
    for (_, rel) in &seen {
        if !rel.is_empty() {
            store.archive_file(rel, &date).map_err(err)?;
        }
    }
    Ok(seen.len())
}

/// Append the recorded copies to the server. A copy that the server has (by Message-ID) is not appended again,
/// so a connection that broke after the server stored it never makes a second copy. A send is never repeated
/// because of anything here: this only copies.
fn exec_append(store: &MailStore, client: &mut Client, folder: &FolderRow, group: &[PendingRow]) -> Result<Done, Fail> {
    let reader = store.reader().map_err(|e| Fail::Retry(err(e)))?;
    let mut d = Done::default();
    for p in group {
        let target = p.target.clone().unwrap_or_else(|| folder.name.clone());
        let label = crate::mail_ui::folder_label_of(&target);
        let Some(msg) = reader.message_at(p.folder, p.uidvalidity, p.uid).map_err(|e| Fail::Refused(err(e)))? else {
            d.ops.push(Op::PendingDone { id: p.id });
            continue; // the local copy is gone: nothing to append
        };
        let raw = store.read_raw(&msg).map_err(|e| Fail::Refused(format!("The mail was sent, but its local copy could not be read for the copy to “{label}”: {e}")))?;
        let sel = client.examine(&target).map_err(classify)?;
        let find = |client: &mut Client| -> Result<Option<u32>, Fail> {
            if msg.message_id.is_empty() {
                return Ok(None);
            }
            Ok(client.uid_search_message_id(&msg.message_id).map_err(classify)?.into_iter().max())
        };
        let (uv, uid) = match find(client)? {
            Some(u) => (sel.uidvalidity, Some(u)),
            None => {
                let out = client.write(&WriteCmd::Append { folder: target.clone(), flags: flags_from(&p.flags_add), bytes: raw.clone() }).map_err(|e| match classify(e) {
                    Fail::Refused(t) => Fail::Refused(format!("The mail was sent, but the copy to “{label}” could not be saved there: {t}")),
                    retry => retry,
                })?;
                match out.appenduid {
                    Some((v, u)) => (v, Some(u)),
                    None => (sel.uidvalidity, find(client)?),
                }
            }
        };
        // the synthetic row goes; the server's own row replaces it if its number is known and fits the folder
        if !msg.path.is_empty() {
            d.archive.push(msg.path.clone());
        }
        d.ops.push(Op::Remove { folder: p.folder, uidvalidity: p.uidvalidity, uid: p.uid });
        if let Some(u) = uid {
            if folder.uidvalidity == Some(uv) && reader.message_at(p.folder, uv, u).map_err(|e| Fail::Refused(err(e)))?.is_none() {
                let nm = store.save_message(folder, uv, u, &raw, "\\Seen", msg.date).map_err(|e| Fail::Retry(err(e)))?;
                d.ops.push(Op::message(nm));
            }
        }
        d.ops.push(Op::PendingDone { id: p.id });
        d.done += 1;
    }
    Ok(d)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::config::MailSecurity;
    use crate::imap_client::{ConnectSettings, PlainAuth};
    use crate::imap_fake_tests::{message, server_cert, FakeServer};
    use crate::imap_sync::{sync_account, SyncOptions};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Arc;

    /// 2026-11-14 12:00 UTC.
    pub const NOW: i64 = 1_794_657_600;
    const DAY: i64 = 86_400;

    pub struct TestConnector {
        pub settings: ConnectSettings,
        pub user: String,
        pub password: String,
        /// While set, the server cannot be reached.
        pub offline: Arc<AtomicBool>,
    }

    impl Connector for TestConnector {
        fn connect(&self) -> Result<Client, ImapError> {
            if self.offline.load(Ordering::SeqCst) {
                return Err(ImapError::Connection("the network is down".into()));
            }
            let mut a = PlainAuth::new(&self.user, &self.password);
            Client::connect_test(&self.settings, server_cert(), &mut a)
        }
    }

    pub struct Env {
        pub server: FakeServer,
        pub store: MailStore,
        pub dir: PathBuf,
        pub conn: TestConnector,
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    /// A pretend server with INBOX (5 mails), Trash and Archive, synced into a new store.
    pub fn env(tag: &str) -> Env {
        let server = FakeServer::start(MailSecurity::ImplicitTls);
        {
            let mut s = server.state();
            s.add_folder("INBOX", &[]);
            s.add_folder("Trash", &["\\Trash"]);
            s.add_folder("Archive", &["\\Archive"]);
            for i in 1..=5 {
                s.add_message("INBOX", message(&format!("mail {i}"), "Bob <bob@x.de>", &format!("hello number {i}")), NOW - (10 - i) * DAY);
            }
        }
        let dir = std::env::temp_dir().join(format!("shale-ops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = MailStore::open(&dir).unwrap();
        let st = server.state();
        let conn = TestConnector { settings: server.settings(), user: st.user.clone(), password: st.password.clone(), offline: Arc::new(AtomicBool::new(false)) };
        drop(st);
        let e = Env { server, store, dir, conn };
        let (tx, _rx) = channel();
        let sum = sync_account(&e.store, "acct1", "Test", &e.conn, &SyncOptions { now: Some(NOW), batch_size: 2, ..SyncOptions::default() }, &tx, &AtomicBool::new(false));
        assert!(sum.errors.is_empty(), "{:?}", sum.errors);
        e
    }

    impl Env {
        pub fn folder(&self, name: &str) -> FolderRow {
            self.store.reader().unwrap().folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == name).unwrap_or_else(|| panic!("no folder {name}"))
        }
        /// The messages of a folder as the list shows them, newest first.
        pub fn list(&self, name: &str) -> Vec<MessageSummary> {
            self.store.reader().unwrap().list(self.folder(name).id, 100, 0).unwrap()
        }
        pub fn by_uid(&self, name: &str, uid: u32) -> MessageSummary {
            self.list(name).into_iter().find(|m| m.uid == uid).unwrap_or_else(|| panic!("no uid {uid} in {name}"))
        }
        pub fn run(&self) -> OpsReport {
            run_pending(&self.store, &self.conn, "acct1")
        }
        pub fn pending(&self) -> Vec<PendingRow> {
            self.store.reader().unwrap().pending_ops(None).unwrap()
        }
        pub fn server_uids(&self, folder: &str) -> Vec<u32> {
            self.server.state().messages(folder).iter().map(|m| m.0).collect()
        }
        pub fn server_flags(&self, folder: &str, uid: u32) -> Vec<String> {
            self.server.state().messages(folder).into_iter().find(|m| m.0 == uid).map(|m| m.1).unwrap_or_default()
        }
        pub fn write_commands(&self) -> Vec<String> {
            self.server.state().verbs().into_iter().filter(|v| ["SELECT", "UID STORE", "UID MOVE", "UID COPY", "UID EXPUNGE", "EXPUNGE", "STORE"].iter().any(|w| v.starts_with(w))).collect()
        }
        pub fn seen_op(&self, set: bool) -> Action {
            if set {
                Action::SetFlags { add: vec![MailFlag::Seen], remove: vec![] }
            } else {
                Action::SetFlags { add: vec![], remove: vec![MailFlag::Seen] }
            }
        }
    }

    fn local_trash_files(root: &Path) -> Vec<PathBuf> {
        fn walk(p: &Path, out: &mut Vec<PathBuf>) {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                let q = e.path();
                if q.is_dir() {
                    walk(&q, out);
                } else {
                    out.push(q);
                }
            }
        }
        let mut v = vec![];
        walk(&root.join(".trash"), &mut v);
        v.sort();
        v
    }

    // spec: MAIL-102, MAIL-107
    #[test]
    fn marking_read_shows_at_once_waits_while_offline_survives_a_restart_and_is_sent_when_the_server_is_back() {
        let mut e = env("offline");
        let m = e.by_uid("INBOX", 2);
        assert!(!m.is_seen());
        e.conn.offline.store(true, Ordering::SeqCst);
        let q = enqueue(&e.store, std::slice::from_ref(&m), &e.seen_op(true), NOW).unwrap();
        assert_eq!(q.queued, 1);
        // optimistic: the list shows it read, and marks it as waiting
        let now = e.by_uid("INBOX", 2);
        assert!(now.is_seen());
        assert_eq!(now.pending.as_deref(), Some("flags"));
        let r = e.run();
        assert!(r.offline.as_deref().is_some_and(|t| t.contains("network is down")), "{r:?}");
        assert_eq!((r.done, r.remaining), (0, 1));
        assert!(e.server_flags("INBOX", 2).is_empty(), "the server was not touched");
        let p = e.pending();
        assert_eq!((p.len(), p[0].attempts), (1, 1));
        assert!(p[0].last_error.as_deref().unwrap().contains("network is down"));
        // the queue is in the index: a restart keeps it
        let dir = e.dir.clone();
        let old = std::mem::replace(&mut e.store, MailStore::open(&std::env::temp_dir().join(format!("shale-ops-offline2-{}", std::process::id()))).unwrap());
        drop(old);
        e.store = MailStore::open(&dir).unwrap();
        assert_eq!(e.pending().len(), 1);
        assert_eq!(e.by_uid("INBOX", 2).pending.as_deref(), Some("flags"));
        // the network is back
        e.conn.offline.store(false, Ordering::SeqCst);
        let r = e.run();
        assert_eq!((r.done, r.remaining, r.offline.is_none()), (1, 0, true), "{r:?}");
        assert_eq!(e.server_flags("INBOX", 2), vec!["\\Seen"]);
        let now = e.by_uid("INBOX", 2);
        assert!(now.is_seen() && now.pending.is_none());
        assert!(e.pending().is_empty());
        let _ = std::fs::remove_dir_all(std::env::temp_dir().join(format!("shale-ops-offline2-{}", std::process::id())));
    }

    // spec: MAIL-107
    #[test]
    fn a_change_the_server_refuses_is_rolled_back_visibly_and_never_stays_as_if_it_worked() {
        let e = env("refused");
        let m = e.by_uid("INBOX", 3);
        enqueue(&e.store, &[m], &Action::SetFlags { add: vec![MailFlag::Flagged, MailFlag::Seen], remove: vec![] }, NOW).unwrap();
        assert!(e.by_uid("INBOX", 3).is_flagged());
        e.server.state().fail_next_write = Some("[NOPERM] you may not do that".into());
        let r = e.run();
        assert_eq!((r.done, r.remaining), (0, 0));
        assert_eq!(r.failed.len(), 1, "{r:?}");
        assert!(r.failed[0].contains("NOPERM"), "{:?}", r.failed);
        let back = e.by_uid("INBOX", 3);
        assert!(!back.is_flagged() && !back.is_seen(), "the optimistic change is undone: {back:?}");
        assert!(back.pending.is_none());
        assert!(e.server_flags("INBOX", 3).is_empty());
    }

    // spec: MAIL-107
    #[test]
    fn a_connection_that_breaks_in_the_middle_changes_nothing_and_the_action_is_tried_again() {
        let e = env("hangup");
        let m = e.by_uid("INBOX", 4);
        enqueue(&e.store, &[m], &Action::Delete, NOW).unwrap();
        assert_eq!(e.by_uid("INBOX", 4).pending.as_deref(), Some("delete"));
        e.server.state().hang_up_on = Some("UID MOVE".into());
        let r = e.run();
        assert!(r.offline.is_some(), "{r:?}");
        assert_eq!(e.server_uids("INBOX").len(), 5);
        assert!(e.server_uids("Trash").is_empty());
        assert_eq!(e.list("INBOX").len(), 5, "the mail is still listed where it was");
        assert_eq!(e.by_uid("INBOX", 4).pending.as_deref(), Some("delete"), "and shown as waiting");
        let r = e.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        assert_eq!(e.server_uids("Trash").len(), 1);
    }

    // spec: MAIL-102
    #[test]
    fn a_flag_change_over_another_clients_change_is_read_again_and_applied_on_top() {
        let e = env("condstore");
        // something that changes the server's modification numbers after the sync: the stored one is behind
        e.server.state().set_flags("INBOX", 2, &["\\Answered"]);
        let m = e.by_uid("INBOX", 2);
        enqueue(&e.store, &[m], &e.seen_op(true), NOW).unwrap();
        let r = e.run();
        assert_eq!(r.done, 1, "{r:?}");
        let cmds = e.write_commands();
        assert!(cmds.iter().any(|c| c.contains("UNCHANGEDSINCE")), "{cmds:?}");
        assert_eq!(cmds.iter().filter(|c| c.starts_with("UID STORE")).count(), 2, "first refused (MODIFIED), then applied on top: {cmds:?}");
        assert_eq!(e.server_flags("INBOX", 2), vec!["\\Answered", "\\Seen"], "the other client's flag is kept");
        assert_eq!(e.by_uid("INBOX", 2).flags, "\\Answered \\Seen", "the local copy shows the server's truth");
        // a server without CONDSTORE gets plain STORE
        let e2 = env("nocondstore");
        e2.server.state().condstore = false;
        enqueue(&e2.store, &[e2.by_uid("INBOX", 1)], &e2.seen_op(true), NOW).unwrap();
        assert_eq!(e2.run().done, 1);
        assert!(e2.write_commands().iter().all(|c| !c.contains("UNCHANGEDSINCE")));
    }

    // spec: MAIL-104, MAIL-106
    #[test]
    fn delete_is_a_move_to_trash_the_file_follows_and_the_old_file_goes_to_the_local_trash() {
        let e = env("delete");
        let m = e.by_uid("INBOX", 1);
        let old_path = e.store.abs_path(&m.path);
        assert!(old_path.is_file());
        let raw = std::fs::read(&old_path).unwrap();
        assert_eq!(enqueue(&e.store, std::slice::from_ref(&m), &Action::Delete, NOW).unwrap().queued, 1);
        let r = e.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        assert_eq!(e.server_uids("INBOX"), vec![2, 3, 4, 5]);
        assert_eq!(e.server_uids("Trash"), vec![1]);
        assert!(e.write_commands().iter().all(|c| !c.contains("Deleted") && !c.starts_with("EXPUNGE") && !c.starts_with("UID EXPUNGE")), "{:?}", e.write_commands());
        // the index: gone from the Inbox, in Trash with a file of its own
        assert_eq!(e.list("INBOX").len(), 4);
        let t = e.list("Trash");
        assert_eq!(t.len(), 1);
        assert_eq!((t[0].uid, t[0].subject.as_str(), t[0].state), (1, "mail 1", STATE_FULL));
        assert_eq!(std::fs::read(e.store.abs_path(&t[0].path)).unwrap(), raw, "the same bytes, now at the Trash path");
        // the old file was moved, not deleted
        assert!(!old_path.exists());
        let kept = local_trash_files(&e.dir);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(std::fs::read(&kept[0]).unwrap(), raw);
        assert!(kept[0].to_string_lossy().contains(".trash"));
        assert!(e.pending().is_empty());
    }

    // spec: MAIL-183
    #[test]
    fn a_decrypted_copy_follows_its_message_when_it_is_moved_and_stays_searchable() {
        let e = env("move-decrypted");
        let m = e.by_uid("INBOX", 2);
        let plain = b"From: Bob <bob@x.de>\r\nTo: ada@example.com\r\nSubject: mail 2\r\nMessage-ID: <mail-2@fake>\r\nContent-Type: text/plain\r\n\r\nthe zebrafish plan\r\n";
        e.store.save_decrypted(&m, plain, &crate::mail_crypto::Security::new(), "2026-11-14").unwrap();
        assert_eq!(e.store.reader().unwrap().search("zebrafish", None, 5, 0).unwrap().len(), 1);
        assert_eq!(enqueue(&e.store, std::slice::from_ref(&m), &Action::MoveTo { folder: "Archive".into() }, NOW).unwrap().queued, 1);
        let r = e.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        let moved = e.list("Archive");
        assert_eq!(moved.len(), 1);
        // the copy is beside the message's new file, and the old one's copy went to the local trash with it
        assert_eq!(e.store.read_decrypted(&moved[0]).unwrap().0, plain);
        assert_eq!(e.store.read_mail(&moved[0]).unwrap().text, "the zebrafish plan");
        let hits = e.store.reader().unwrap().search("zebrafish", None, 5, 0).unwrap();
        assert_eq!((hits.len(), hits[0].folder.as_str()), (1, "Archive"), "found in its new folder");
        let kept = local_trash_files(&e.dir);
        assert!(kept.iter().any(|p| p.to_string_lossy().ends_with("2.decrypted.eml")), "{kept:?}");
        assert!(!e.store.abs_path(&m.path).exists());
    }

    // spec: MAIL-103, MAIL-105
    #[test]
    fn without_move_the_copy_is_verified_first_and_only_the_targeted_uid_is_expunged() {
        let e = env("nomove");
        e.server.state().move_cap = false;
        // another program flagged mail 2 as deleted and did not expunge it: it must survive our delete of mail 1
        e.server.state().set_flags("INBOX", 2, &["\\Deleted"]);
        enqueue(&e.store, &[e.by_uid("INBOX", 1)], &Action::Delete, NOW).unwrap();
        let r = e.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        assert_eq!(e.server_uids("INBOX"), vec![2, 3, 4, 5], "mail 2 (flagged \\Deleted by someone else) is still there");
        assert_eq!(e.server_uids("Trash"), vec![1]);
        let cmds = e.write_commands();
        let at = |p: &str| cmds.iter().position(|c| c.starts_with(p)).unwrap_or_else(|| panic!("no {p} in {cmds:?}"));
        assert!(at("UID COPY") < at("UID STORE 1 +FLAGS (\\Deleted)") && at("UID STORE 1 +FLAGS (\\Deleted)") < at("UID EXPUNGE 1"), "{cmds:?}");
        assert!(cmds.iter().all(|c| !c.starts_with("UID MOVE") && c != "EXPUNGE"), "{cmds:?}");
        assert_eq!(e.list("Trash").len(), 1);
        assert_eq!(local_trash_files(&e.dir).len(), 1);
    }

    // spec: MAIL-103
    #[test]
    fn a_copy_is_verified_by_message_id_when_the_server_does_not_say_copyuid() {
        let e = env("nocopyuid");
        {
            let mut s = e.server.state();
            s.move_cap = false;
            s.no_copyuid = true;
        }
        enqueue(&e.store, &[e.by_uid("INBOX", 3)], &Action::Delete, NOW).unwrap();
        let r = e.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        assert_eq!(e.server_uids("Trash"), vec![1]);
        assert!(!e.server_uids("INBOX").contains(&3));
        let t = e.list("Trash");
        assert_eq!(t.len(), 1, "the row follows with the UID found by Message-ID");
        assert_eq!(t[0].uid, 1);
        // a copy that cannot be found is never a reason to remove the original
        let e2 = env("unverifiable");
        {
            let mut s = e2.server.state();
            s.move_cap = false;
            s.no_copyuid = true;
        }
        let m = e2.by_uid("INBOX", 3);
        // the index does not know this mail's Message-ID
        e2.store.commit(vec![]).unwrap();
        let conn = rusqlite::Connection::open(e2.store.index_path()).unwrap();
        conn.execute("UPDATE messages SET message_id = '' WHERE uid = 3", []).unwrap();
        drop(conn);
        enqueue(&e2.store, &[m], &Action::Delete, NOW).unwrap();
        let r = e2.run();
        assert_eq!(r.failed.len(), 1, "{r:?}");
        assert!(r.failed[0].contains("could not be verified"), "{:?}", r.failed);
        assert!(e2.server_uids("INBOX").contains(&3), "the original is untouched");
        assert!(e2.write_commands().iter().all(|c| !c.contains("Deleted") && !c.contains("EXPUNGE")), "{:?}", e2.write_commands());
    }

    // spec: MAIL-103, MAIL-104, MAIL-105
    #[test]
    fn without_uidplus_a_deletion_that_needs_it_is_refused_and_nothing_is_expunged() {
        let e = env("nouidplus");
        {
            let mut s = e.server.state();
            s.move_cap = false;
            s.uidplus = false;
        }
        enqueue(&e.store, &[e.by_uid("INBOX", 1)], &Action::Delete, NOW).unwrap();
        let r = e.run();
        assert_eq!((r.done, r.failed.len()), (0, 1), "{r:?}");
        assert!(r.failed[0].contains("UIDPLUS"), "{:?}", r.failed);
        assert_eq!(e.server_uids("INBOX"), vec![1, 2, 3, 4, 5]);
        assert!(e.server_uids("Trash").is_empty());
        let cmds = e.write_commands();
        assert!(cmds.iter().all(|c| c.starts_with("SELECT")), "only the folder was opened, nothing was copied, marked or expunged: {cmds:?}");
        // rolled back and no longer waiting; the file is where it was
        assert!(e.by_uid("INBOX", 1).pending.is_none());
        assert!(e.store.abs_path(&e.by_uid("INBOX", 1).path).is_file());
        // with MOVE but without UIDPLUS the move works (the server does it atomically), without a new UID to follow
        let e2 = env("moveonly");
        e2.server.state().uidplus = false;
        enqueue(&e2.store, &[e2.by_uid("INBOX", 1)], &Action::Delete, NOW).unwrap();
        let r = e2.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        assert_eq!(e2.server_uids("Trash").len(), 1);
        assert!(e2.list("Trash").is_empty(), "its new UID is unknown: the next listing shows it");
        assert_eq!(local_trash_files(&e2.dir).len(), 1, "the file is kept");
    }

    // spec: MAIL-105
    #[test]
    fn delete_forever_exists_only_in_trash_needs_the_confirmed_count_and_removes_only_the_chosen_uid() {
        let e = env("forever");
        // two mails in Trash, a third flagged \Deleted by another program
        {
            let mut s = e.server.state();
            for i in 1..=3 {
                s.add_message("Trash", message(&format!("bin {i}"), "x@x.de", "in the bin"), NOW - i * DAY);
            }
            s.set_flags("Trash", 3, &["\\Deleted"]);
        }
        // not in Trash: refused, nothing recorded
        let inbox = e.by_uid("INBOX", 1);
        let err = enqueue(&e.store, std::slice::from_ref(&inbox), &Action::DeleteForever(Confirmed::after_dialog(1)), NOW).unwrap_err();
        assert!(err.contains("Trash"), "{err}");
        assert!(e.pending().is_empty());
        // list Trash into the index (as opening the folder does)
        crate::imap_sync::fetch_folder_listing(&e.store, &e.conn, "acct1", "Trash", 100, &SyncOptions { now: Some(NOW), ..SyncOptions::default() }).unwrap();
        let bin1 = e.by_uid("Trash", 1);
        // a confirmation for another count is refused
        let err = enqueue(&e.store, std::slice::from_ref(&bin1), &Action::DeleteForever(Confirmed::after_dialog(2)), NOW).unwrap_err();
        assert!(err.contains("confirmed"), "{err}");
        assert!(e.pending().is_empty());
        assert_eq!(enqueue(&e.store, &[bin1], &Action::DeleteForever(Confirmed::after_dialog(1)), NOW).unwrap().queued, 1);
        let r = e.run();
        assert_eq!((r.done, r.remaining), (1, 0), "{r:?}");
        assert_eq!(e.server_uids("Trash"), vec![2, 3], "only UID 1 is gone, the other program's \\Deleted mail 3 survives");
        let cmds = e.write_commands();
        assert!(cmds.iter().any(|c| c == "UID EXPUNGE 1"), "{cmds:?}");
        assert!(cmds.iter().all(|c| c != "EXPUNGE" && !c.contains(":*")), "{cmds:?}");
        assert!(e.list("Trash").iter().all(|m| m.uid != 1));
    }

    // spec: MAIL-108
    #[test]
    fn mail_that_another_client_moved_or_deleted_meanwhile_ends_gracefully_and_the_local_copy_stays() {
        let e = env("raced");
        let (m2, m3, m4) = (e.by_uid("INBOX", 2), e.by_uid("INBOX", 3), e.by_uid("INBOX", 4));
        enqueue(&e.store, &[m2.clone(), m3.clone()], &Action::Delete, NOW).unwrap();
        enqueue(&e.store, std::slice::from_ref(&m4), &e.seen_op(true), NOW).unwrap();
        // meanwhile another program archives mail 2 and deletes mail 4
        e.server.state().move_message("INBOX", "Archive", 2).unwrap();
        e.server.state().delete_message("INBOX", 4);
        let r = e.run();
        assert!(r.failed.is_empty(), "{r:?}");
        assert_eq!(r.gone, 2, "{r:?}");
        assert_eq!(r.done, 1, "only mail 3 was ours to move: {r:?}");
        assert_eq!(e.server_uids("Trash").len(), 1);
        assert_eq!(e.server_uids("Archive").len(), 1, "the other program's move is untouched");
        let local2 = e.by_uid("INBOX", 2);
        assert!(local2.gone_on_server && local2.pending.is_none());
        assert!(e.store.abs_path(&local2.path).is_file(), "the file stays");
        let local4 = e.by_uid("INBOX", 4);
        assert!(local4.gone_on_server);
        assert!(e.store.abs_path(&local4.path).is_file());
        assert!(e.pending().is_empty());
        // nothing but our own mail 3 was moved
        assert!(e.write_commands().iter().filter(|c| c.starts_with("UID MOVE")).all(|c| c == "UID MOVE 3 \"Trash\""), "{:?}", e.write_commands());
    }

    // spec: MAIL-101, MAIL-108
    #[test]
    fn a_folder_that_was_renumbered_on_the_server_is_not_written_to() {
        let e = env("renumbered");
        enqueue(&e.store, &[e.by_uid("INBOX", 1)], &Action::Delete, NOW).unwrap();
        e.server.state().renumber("INBOX", 5555);
        let r = e.run();
        assert_eq!(r.failed.len(), 1, "{r:?}");
        assert!(r.failed[0].contains("renumbered"), "{:?}", r.failed);
        assert_eq!(e.server_uids("INBOX").len(), 5);
        assert!(e.server_uids("Trash").is_empty());
        let cmds = e.write_commands();
        assert!(cmds.iter().all(|c| c.starts_with("SELECT")), "{cmds:?}");
        assert!(e.pending().is_empty());
    }

    // spec: MAIL-100, MAIL-108
    #[test]
    fn a_sync_never_writes_and_leaves_alone_the_flags_of_a_message_with_an_action_waiting() {
        let e = env("syncpend");
        let (m1, _m2) = (e.by_uid("INBOX", 1), e.by_uid("INBOX", 2));
        e.conn.offline.store(false, Ordering::SeqCst);
        // an action waits (we do not run it yet); the server meanwhile says other things about both mails
        enqueue(&e.store, &[m1], &e.seen_op(true), NOW).unwrap();
        e.server.state().set_flags("INBOX", 1, &["\\Flagged"]);
        e.server.state().set_flags("INBOX", 2, &["\\Flagged"]);
        let before = e.server.state().commands.len();
        let (tx, _rx) = channel();
        let sum = sync_account(&e.store, "acct1", "Test", &e.conn, &SyncOptions { now: Some(NOW), batch_size: 2, ..SyncOptions::default() }, &tx, &AtomicBool::new(false));
        assert!(sum.errors.is_empty(), "{:?}", sum.errors);
        let seen: Vec<String> = e.server.state().verbs().split_off(before);
        assert!(seen.iter().all(|c| !c.split_whitespace().any(|w| matches!(w, "SELECT" | "STORE" | "EXPUNGE" | "MOVE" | "COPY"))), "a sync sent a write: {seen:?}");
        // mail 2: the server wins. Mail 1: the person's change is on its way, the sync leaves it
        assert_eq!(e.by_uid("INBOX", 2).flags, "\\Flagged");
        assert_eq!(e.by_uid("INBOX", 1).flags, "\\Seen");
        // when the action is sent, it is applied on top of what the server has now
        let r = e.run();
        assert_eq!(r.done, 1, "{r:?}");
        assert_eq!(e.server_flags("INBOX", 1), vec!["\\Flagged", "\\Seen"]);
        assert_eq!(e.by_uid("INBOX", 1).flags, "\\Flagged \\Seen");
    }

    // spec: MAIL-107, MAIL-104
    #[test]
    fn enqueue_refuses_what_cannot_work_and_records_nothing() {
        let e = env("refuse");
        let m = e.by_uid("INBOX", 1);
        // unknown folder
        assert!(enqueue(&e.store, std::slice::from_ref(&m), &Action::MoveTo { folder: "Nowhere".into() }, NOW).is_err());
        // the folder it is in
        let r = enqueue(&e.store, std::slice::from_ref(&m), &Action::MoveTo { folder: "INBOX".into() }, NOW).unwrap();
        assert_eq!((r.queued, r.notes.len()), (0, 1));
        // twice: the second finds it already waiting
        assert_eq!(enqueue(&e.store, std::slice::from_ref(&m), &Action::Delete, NOW).unwrap().queued, 1);
        let again = enqueue(&e.store, &[e.by_uid("INBOX", 1)], &Action::MoveTo { folder: "Archive".into() }, NOW).unwrap();
        assert_eq!((again.queued, again.notes.len()), (0, 1), "{again:?}");
        assert_eq!(e.pending().len(), 1);
        // an account without a Trash folder cannot delete
        let rows = e.store.reader().unwrap().folders(Some("acct1")).unwrap();
        assert!(trash_folder(&rows).is_some());
        assert!(trash_folder(&rows.iter().filter(|f| f.name != "Trash").cloned().collect::<Vec<_>>()).is_none());
        // setting a flag that is already set changes nothing and records nothing
        let r = enqueue(&e.store, &[e.by_uid("INBOX", 2)], &e.seen_op(false), NOW).unwrap();
        assert_eq!(r.queued, 0);
    }

    // spec: MAIL-106
    #[test]
    fn an_archived_file_never_overwrites_another_and_a_bad_path_is_refused() {
        let e = env("archive");
        let m = e.by_uid("INBOX", 1);
        let first = e.store.archive_file(&m.path, "2026-11-14").unwrap().unwrap();
        assert!(first.is_file() && !e.store.abs_path(&m.path).exists());
        // the same path again (a file came back, as a re-fetch would do): it gets its own name
        std::fs::write(e.store.abs_path(&m.path), b"second").unwrap();
        let second = e.store.archive_file(&m.path, "2026-11-14").unwrap().unwrap();
        assert_ne!(first, second);
        assert_eq!(std::fs::read(&second).unwrap(), b"second");
        assert!(first.is_file());
        assert_eq!(e.store.archive_file(&m.path, "2026-11-14").unwrap(), None, "no file, nothing to do");
        for bad in ["", "/etc/passwd", "../outside.eml", "a/../../b"] {
            assert!(e.store.archive_file(bad, "2026-11-14").is_err(), "{bad:?}");
        }
    }

    // ------------------------------------------------------------ randomized (MAIL-112)

    /// xorshift64*: small, fixed, no dependency (like `app/data_fuzz_tests.rs`).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// `<id@fake>` of a raw message.
    fn message_id_of(raw: &[u8]) -> Option<String> {
        let text = String::from_utf8_lossy(raw);
        let line = text.lines().find(|l| l.to_ascii_lowercase().starts_with("message-id:"))?;
        Some(line.split_once(':')?.1.trim().to_string())
    }

    fn local_message_ids(root: &Path) -> HashSet<String> {
        fn walk(p: &Path, out: &mut HashSet<String>) {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                let q = e.path();
                if q.is_dir() {
                    walk(&q, out);
                } else if q.extension().is_some_and(|x| x == "eml") {
                    if let Some(id) = message_id_of(&std::fs::read(&q).unwrap()) {
                        out.insert(id);
                    }
                }
            }
        }
        let mut out = HashSet::new();
        walk(root, &mut out);
        out
    }

    fn server_message_ids(e: &Env) -> HashSet<String> {
        let s = e.server.state();
        let mut out = HashSet::new();
        for f in ["INBOX", "Archive", "Trash"] {
            for m in s.messages(f) {
                out.extend(message_id_of(&m.2));
            }
        }
        out
    }

    struct Model {
        /// Every message that ever existed on the server (and that Shale could have stored).
        known: std::collections::BTreeSet<String>,
        /// Messages that another program flagged `\Deleted` (never expunged by it): we must not expunge them.
        foreign_deleted: std::collections::BTreeSet<String>,
        /// Messages for which "delete forever" was confirmed while no local file existed.
        forever: std::collections::BTreeSet<String>,
        /// Messages another program deleted from the server (Shale's local copy is all that is left).
        other_deleted: std::collections::BTreeSet<String>,
        /// Messages the person chose to delete forever (and confirmed): the server may lose these.
        forever_chosen: std::collections::BTreeSet<String>,
        ever_local: HashSet<String>,
    }

    fn check(e: &Env, m: &mut Model, seed: u64, step: usize, what: &str) {
        let (server, local) = (server_message_ids(e), local_message_ids(&e.dir));
        m.ever_local.extend(local.iter().cloned());
        for id in &m.known {
            let exempt = m.forever.contains(id) && !m.ever_local.contains(id);
            assert!(server.contains(id) || local.contains(id) || exempt, "seed {seed} step {step} ({what}): {id} is on neither the server nor in the local store");
        }
        // the server side: only the person's confirmed "delete forever" or another program may remove a mail from it
        for id in m.known.iter().filter(|id| !m.other_deleted.contains(*id) && !m.forever_chosen.contains(*id)) {
            assert!(server.contains(id), "seed {seed} step {step} ({what}): {id} is gone from the server and nobody confirmed that");
        }
        for id in m.foreign_deleted.iter().filter(|id| !m.forever_chosen.contains(*id)) {
            assert!(server.contains(id), "seed {seed} step {step} ({what}): {id} was flagged \\Deleted by another program and has been expunged by us");
        }
        // every index row that has a file points at one that exists
        let r = e.store.reader().unwrap();
        let mut st = r.conn().prepare("SELECT path FROM messages WHERE path != '' AND state = 0 AND missing_file = 0").unwrap();
        let paths: Vec<String> = st.query_map([], |row| row.get(0)).unwrap().map(|x| x.unwrap()).collect();
        for p in paths {
            assert!(e.store.abs_path(&p).is_file(), "seed {seed} step {step} ({what}): the index points at a missing file {p}");
        }
        let verbs = e.server.state().verbs();
        assert!(verbs.iter().all(|v| v != "EXPUNGE" && !v.starts_with("EXPUNGE ")), "seed {seed} step {step}: a plain EXPUNGE was sent");
        assert!(verbs.iter().filter(|v| v.starts_with("UID EXPUNGE") || v.starts_with("UID STORE") || v.starts_with("UID MOVE") || v.starts_with("UID COPY")).all(|v| !v.contains('*')), "seed {seed} step {step}: a set with * in a write");
    }

    fn fuzz_one(seed: u64, steps: usize) {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let e = env(&format!("fuzz{seed}"));
        {
            let mut s = e.server.state();
            s.move_cap = rng.below(2) == 0;
            s.uidplus = rng.below(4) != 0;
            s.condstore = rng.below(2) == 0;
            s.no_copyuid = rng.below(3) == 0;
        }
        let mut m = Model { known: Default::default(), foreign_deleted: Default::default(), forever: Default::default(), forever_chosen: Default::default(), other_deleted: Default::default(), ever_local: HashSet::new() };
        m.known.extend(server_message_ids(&e));
        let opts = SyncOptions { now: Some(NOW), batch_size: 3, ..SyncOptions::default() };
        let mut arrivals = 0;
        let folders = ["INBOX", "Archive", "Trash"];
        let sync = |e: &Env| {
            let (tx, _rx) = channel();
            let _ = sync_account(&e.store, "acct1", "Test", &e.conn, &opts, &tx, &AtomicBool::new(false));
        };
        for step in 0..steps {
            let what = match rng.below(14) {
                0..=2 => {
                    let f = folders[rng.below(3)];
                    let list = e.list(f);
                    if !list.is_empty() {
                        let msg = list[rng.below(list.len())].clone();
                        let flag = if rng.below(2) == 0 { MailFlag::Seen } else { MailFlag::Flagged };
                        let a = if rng.below(2) == 0 { Action::SetFlags { add: vec![flag], remove: vec![] } } else { Action::SetFlags { add: vec![], remove: vec![flag] } };
                        let _ = enqueue(&e.store, &[msg], &a, NOW);
                    }
                    "flag"
                }
                3..=5 => {
                    let f = folders[rng.below(2)]; // Trash only deletes forever
                    let list = e.list(f);
                    if !list.is_empty() {
                        let n = 1 + rng.below(2).min(list.len() - 1);
                        let picked: Vec<MessageSummary> = (0..n).map(|i| list[(rng.below(list.len()) + i) % list.len()].clone()).collect();
                        let mut seen = HashSet::new();
                        let picked: Vec<MessageSummary> = picked.into_iter().filter(|p| seen.insert(p.id)).collect();
                        let _ = enqueue(&e.store, &picked, &Action::Delete, NOW);
                    }
                    "delete"
                }
                6 => {
                    let list = e.list("INBOX");
                    if !list.is_empty() {
                        let target = folders[1 + rng.below(2)];
                        let _ = enqueue(&e.store, &[list[rng.below(list.len())].clone()], &Action::MoveTo { folder: target.into() }, NOW);
                    }
                    "move"
                }
                7 => {
                    // browse Trash, then delete forever one or two (sometimes with a wrong count: must be refused)
                    let _ = crate::imap_sync::fetch_folder_listing(&e.store, &e.conn, "acct1", "Trash", 100, &opts);
                    let list = e.list("Trash");
                    if !list.is_empty() {
                        let picked: Vec<MessageSummary> = list.iter().take(1 + rng.below(2)).cloned().collect();
                        let wrong = rng.below(4) == 0;
                        let count = if wrong { picked.len() + 1 } else { picked.len() };
                        match enqueue(&e.store, &picked, &Action::DeleteForever(Confirmed::after_dialog(count)), NOW) {
                            Ok(q) => {
                                assert!(!wrong, "a wrong count was accepted");
                                if q.queued > 0 {
                                    m.forever_chosen.extend(picked.iter().map(|p| p.message_id.clone()));
                                    for p in &picked {
                                        if p.path.is_empty() {
                                            m.forever.insert(p.message_id.clone());
                                        }
                                    }
                                }
                            }
                            Err(_) => assert!(wrong),
                        }
                    }
                    "forever"
                }
                8 | 9 => {
                    // send the queue, sometimes with the network down or the server failing
                    match rng.below(6) {
                        0 => e.conn.offline.store(true, Ordering::SeqCst),
                        1 => e.server.state().fail_next_write = Some("[UNAVAILABLE] busy".into()),
                        2 => e.server.state().fail_next_write = Some("[NOPERM] no".into()),
                        3 => e.server.state().hang_up_on = Some(["UID MOVE", "UID COPY", "UID STORE", "UID EXPUNGE"][rng.below(4)].into()),
                        _ => {}
                    }
                    let _ = e.run();
                    e.conn.offline.store(false, Ordering::SeqCst);
                    e.server.state().fail_next_write = None;
                    e.server.state().hang_up_on = None;
                    "run"
                }
                10 => {
                    // another program moves a mail
                    let (from, to) = (folders[rng.below(3)], folders[rng.below(3)]);
                    let uids = e.server_uids(from);
                    if from != to && !uids.is_empty() {
                        let u = uids[rng.below(uids.len())];
                        e.server.state().move_message(from, to, u);
                    }
                    "other moves"
                }
                11 => {
                    // another program flags a mail \Deleted (and does not expunge it)
                    let uids = e.server_uids("INBOX");
                    if !uids.is_empty() {
                        let u = uids[rng.below(uids.len())];
                        let raw = e.server.state().messages("INBOX").into_iter().find(|x| x.0 == u).unwrap().2;
                        e.server.state().set_flags("INBOX", u, &["\\Deleted"]);
                        m.foreign_deleted.extend(message_id_of(&raw));
                    }
                    "other flags deleted"
                }
                12 => {
                    arrivals += 1;
                    let id = format!("fresh {seed}-{arrivals}");
                    e.server.state().add_message("INBOX", message(&id, "Eve <eve@x.de>", "new"), NOW - DAY);
                    m.known.insert(format!("<{id}@fake>"));
                    sync(&e);
                    "arrival and sync"
                }
                _ => {
                    // another program deletes a mail that we already hold a copy of
                    let local = local_message_ids(&e.dir);
                    let uids: Vec<(u32, String)> = e.server.state().messages("INBOX").into_iter().filter_map(|x| message_id_of(&x.2).map(|id| (x.0, id))).filter(|(_, id)| local.contains(id)).collect();
                    if !uids.is_empty() {
                        let (u, id) = uids[rng.below(uids.len())].clone();
                        e.server.state().delete_message("INBOX", u);
                        // not "flagged deleted and still there" any more: it was their expunge, not ours
                        m.foreign_deleted.remove(&id);
                        m.other_deleted.insert(id);
                    }
                    "other deletes"
                }
            };
            check(&e, &mut m, seed, step, what);
        }
        // everything settles once the server behaves: nothing waits, nothing was lost
        e.conn.offline.store(false, Ordering::SeqCst);
        e.server.state().fail_next_write = None;
        e.server.state().hang_up_on = None;
        for _ in 0..4 {
            if e.pending().is_empty() {
                break;
            }
            let _ = e.run();
        }
        assert!(e.pending().is_empty(), "seed {seed}: actions are still waiting after the server came back: {:?}", e.pending());
        sync(&e);
        check(&e, &mut m, seed, steps, "settled");
    }

    // spec: MAIL-112
    #[test]
    fn random_mail_actions_with_failures_and_another_client_never_lose_a_message() {
        for seed in 1..=8 {
            fuzz_one(seed, 32);
        }
    }
}
