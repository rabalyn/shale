//! Closing a vault and leaving one for another (VAULT-035, VAULT-036, VAULT-037, rabalyn/shale#584):
//! the open vault is saved and its sync waited for, everything the window keeps per vault is
//! dropped in one place ([`ShaleApp::drop_vault_state`]), and Close forgets the vault and, when
//! asked, removes Shale's own folders from it.

use super::vault_load::After;
use super::{ShaleApp, TasksFilter};
use crate::busy::{Cancel, Key, Task};
use crate::known_vaults::canonical;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a switch or a close waits for a running sync (SYNC-013) before the vault stays open.
pub(crate) const SYNC_WAIT: Duration = Duration::from_secs(30);

/// The task of a close or switch that waits for the sync (JOB-003).
pub(crate) const LEAVE_WAIT: Key = Key::new("vault-leave-wait");

/// What waits for the sync.
pub(crate) enum Leave {
    Open(PathBuf, After),
    Close(PathBuf, Remove),
}

/// A close or switch that waits for a running sync. Nothing blocks: the window keeps drawing, and
/// [`ShaleApp::vault_leave_tick`] carries on when the sync has ended.
pub(crate) struct PendingLeave {
    what: Leave,
    // held while it waits: the task is listed as long as this lives
    _task: Task,
    cancel: Cancel,
    since: Instant,
}

/// Which of Shale's folders Close removes from the vault's folder (VAULT-035).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Remove {
    /// `.shale/`: the vault's settings and state
    pub shale: bool,
    /// `.trash/`: the notes deleted from the vault
    pub trash: bool,
}

/// Remove the folders `what` names from the vault folder `root`, and nothing else: not the notes,
/// not the attachments, never a `.git`. There is no system trash in Shale, so this is a plain
/// delete limited to exactly `<root>/.shale` and `<root>/.trash`; a link by that name is removed
/// as a link, never followed. Returns what could not be removed.
pub fn remove_shale_files(root: &Path, what: Remove) -> Vec<String> {
    let mut problems = Vec::new();
    for (wanted, name) in [(what.shale, ".shale"), (what.trash, crate::vault::TRASH_DIR)] {
        if !wanted {
            continue;
        }
        let path = root.join(name);
        let removed = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&path),
            Ok(_) => std::fs::remove_file(&path),
        };
        if let Err(e) = removed {
            problems.push(format!("Could not remove {name}/ from {}: {e}", root.display()));
        }
    }
    problems
}

impl ShaleApp {
    /// Drop everything the window keeps for the vault being left, so the next one shows nothing of
    /// it (VAULT-037): tabs, panes and their editors, the buffers, the caches of the index, the
    /// search, the graphs, the file list, the thread overview, the history, the sync of the vault,
    /// the rescan, the jobs that work on its files and the dialogs that were about it. Mail, the
    /// calendar, the feeds and the global settings stay. The undo histories are written and
    /// dropped too. The vault's own settings and state (`vdata`) are saved before and replaced
    /// after by the caller.
    pub(crate) fn drop_vault_state(&mut self) {
        self.media.stop();
        self.undo.close();
        self.modal = None;
        self.tabbar.tabs.clear();
        self.tabbar.active = 0;
        self.panes.reset();
        self.notes.clear();
        self.drawings.clear();
        self.scenes = Default::default();
        self.status = Default::default();
        self.vault_cache = Default::default();
        self.tree = Default::default();
        self.search = Default::default();
        self.tasks = TasksFilter::default();
        self.graphs = crate::graph::Graphs::new();
        self.thread = Default::default();
        self.tag_learner = crate::import::tagwords::TagLearner::new();
        self.bib.reset();
        self.vault_check = Default::default();
        self.history.drop_vault();
        self.sync.drop_vault();
        self.rescan.reset();
        self.state_tick = (std::time::Instant::now(), None);
        self.trashed.clear();
        self.outside_copy = None;
        self.outside_opens.clear();
        self.link_preview.open = None;
        self.mail.restore = None;
        self.mail.vault = None;
        self.usage.places.vault = None;
        self.usage.places.linked = Default::default();
        // the folders are looked at again the next time the list is shown
        self.presence.clear();
    }

    /// A sync runs: the close or switch `what` waits for it, as a task with Cancel (VAULT-036,
    /// VAULT-037). False: no sync runs, go on now. One wait at a time; a second one is ignored.
    pub(crate) fn defer_leave(&mut self, what: Leave) -> bool {
        if !self.sync.running() {
            return false;
        }
        if self.pending_leave.is_none() {
            if let Some(mut task) = self.busy.begin(LEAVE_WAIT, "Waiting for the sync to finish…") {
                let cancel = task.cancellable();
                self.pending_leave = Some(PendingLeave { what, _task: task, cancel, since: Instant::now() });
            }
        }
        true
    }

    /// Every frame, after the sync's result was taken in: carry out the close or switch that waited.
    pub(crate) fn vault_leave_tick(&mut self) {
        self.vault_leave_step(Instant::now());
    }

    /// [`Self::vault_leave_tick`] at the time `now`: Cancel drops the wait and keeps the vault; a
    /// sync that still runs after [`SYNC_WAIT`] ends it with a message and keeps the vault.
    pub(crate) fn vault_leave_step(&mut self, now: Instant) {
        let Some(p) = &self.pending_leave else { return };
        if p.cancel.is_cancelled() {
            self.pending_leave = None;
        } else if self.sync.running() {
            if now.saturating_duration_since(p.since) >= SYNC_WAIT {
                self.pending_leave = None;
                self.toast("The sync is still running, so the vault is not left. Try again when it is done".to_string());
            }
        } else if let Some(p) = self.pending_leave.take() {
            match p.what {
                Leave::Open(path, after) => self.open_vault_in_background(&path, after),
                Leave::Close(path, remove) => self.close_vault(&path, remove),
            }
        }
    }

    /// Close the vault in `path`: forget it (the notes stay where they are) and remove what `remove`
    /// names (VAULT-035). When it is the open vault, its buffers are saved and its sync waited for,
    /// it is closed, and the start screen shows the vault list (VAULT-036).
    pub(crate) fn close_vault(&mut self, path: &Path, remove: Remove) {
        let path = canonical(path);
        let is_open = self.vault.as_ref().is_some_and(|v| canonical(&v.root) == path);
        if is_open {
            if self.defer_leave(Leave::Close(path.clone(), remove)) || !self.prepare_vault_switch() {
                return;
            }
            self.remember_open_files();
            if self.persist_config {
                if let Err(e) = self.vdata.save() {
                    self.toast(e);
                }
            }
            self.drop_vault_state();
            self.vault = None;
            self.vdata = Default::default();
        }
        self.cfg.vaults.forget(&path);
        if self.cfg.vault.as_deref().is_some_and(|v| canonical(v) == path) {
            self.cfg.vault = None;
        }
        for p in remove_shale_files(&path, remove) {
            self.toast(p);
        }
        self.save_config();
        if is_open {
            self.open_vault_menu();
        }
    }
}
