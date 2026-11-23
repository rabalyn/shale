//! Closing a vault and leaving one for another (VAULT-035, VAULT-036, VAULT-037, rabalyn/shale#584):
//! the open vault is saved and its sync waited for, everything the window keeps per vault is
//! dropped in one place ([`ShaleApp::drop_vault_state`]), and Close forgets the vault and, when
//! asked, removes Shale's own folders from it.

use super::{ShaleApp, TasksFilter};
use crate::known_vaults::canonical;
use std::path::Path;
use std::time::Duration;

/// How long a switch or a close waits for a running sync (SYNC-013) before the vault stays open.
pub(crate) const SYNC_WAIT: Duration = Duration::from_secs(30);

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

    /// Close the vault in `path`: forget it (the notes stay where they are) and remove what `remove`
    /// names (VAULT-035). When it is the open vault, its buffers are saved and its sync waited for,
    /// it is closed, and the start screen shows the vault list (VAULT-036).
    pub(crate) fn close_vault(&mut self, path: &Path, remove: Remove) {
        let path = canonical(path);
        let is_open = self.vault.as_ref().is_some_and(|v| canonical(&v.root) == path);
        if is_open {
            if !self.prepare_vault_switch() {
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
