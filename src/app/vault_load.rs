//! Loading a vault off the UI thread (NET-019, rabalyn/shale#513, #525): `Vault::open` reads every
//! note, which on a network share or an on-demand cloud folder takes long enough to freeze the
//! window. [`VaultLoad`] runs it on a worker thread as a task of the busy registry; the start
//! screen shows a spinner and Cancel meanwhile, and the main window appears when the vault has
//! arrived. The switch of the open vault (VAULT-037) uses the same state.

use super::ShaleApp;
use crate::busy::{Busy, Cancel, Key, Task};
use crate::vault::Vault;
use eframe::egui;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// The task of a vault being read.
pub const VAULT_OPEN: Key = Key::new("vault-open");

/// How the vault is read; a test replaces it with a slow or gated one.
type Opener = Arc<dyn Fn(&Path) -> Result<Vault, String> + Send + Sync>;

/// What happens once the vault has arrived.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum After {
    #[default]
    Nothing,
    /// `--view`: the view the command line asked for
    View(String),
    /// the user guide's vault (DEV-020): open this note of it. It is not one of the known vaults
    /// and not the vault opened last (VAULT-029).
    Guide(PathBuf),
}

/// The load that runs: for this folder.
struct Flight {
    path: PathBuf,
    // held while the load runs: the task is listed as long as the flight lives, so Cancel frees it at once
    _task: Task,
    cancel: Cancel,
    rx: Receiver<Result<Result<Vault, String>, String>>,
    after: After,
}

/// What a finished load brings.
pub struct Loaded {
    pub path: PathBuf,
    pub result: Result<Vault, String>,
    pub after: After,
}

/// The vault being read, and why the last load failed.
pub struct VaultLoad {
    flight: Option<Flight>,
    /// why the last load failed, for the start screen; cleared by the next start
    pub error: Option<String>,
    opener: Opener,
}

impl Default for VaultLoad {
    fn default() -> Self {
        Self { flight: None, error: None, opener: Arc::new(|p| Vault::open(p).map_err(|e| e.to_string())) }
    }
}

impl VaultLoad {
    pub fn is_loading(&self) -> bool {
        self.flight.is_some()
    }

    /// The folder being read.
    pub fn target(&self) -> Option<&Path> {
        self.flight.as_ref().map(|f| f.path.as_path())
    }

    /// The name the start screen shows: the folder's name.
    pub fn name(&self) -> Option<String> {
        self.target().map(folder_name)
    }

    /// Read the vault in `path` on a worker thread. False: a load already runs.
    pub fn start(&mut self, busy: &Busy, path: &Path, after: After, ctx: Option<egui::Context>) -> bool {
        if self.flight.is_some() {
            return false;
        }
        let Some(mut task) = busy.begin(VAULT_OPEN, format!("Opening {}…", folder_name(path))) else { return false };
        let cancel = task.cancellable();
        self.error = None;
        let (open, root) = (self.opener.clone(), path.to_path_buf());
        let rx = crate::jobs::spawn(
            move || tracing::info_span!("flow.vault_open.scan", peer = "disk").in_scope(|| open(&root)),
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.flight = Some(Flight { path: path.to_path_buf(), _task: task, cancel, rx, after });
        true
    }

    /// Take in a finished load. A load that was cancelled (here or by the indicator) is dropped:
    /// its result is ignored when it arrives.
    pub fn poll(&mut self) -> Option<Loaded> {
        let f = self.flight.as_ref()?;
        if f.cancel.is_cancelled() {
            self.flight = None;
            return None;
        }
        let result = match f.rx.try_recv() {
            Ok(r) => r.and_then(|r| r),
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the load stopped unexpectedly".to_string()),
        };
        let f = self.flight.take()?;
        Some(Loaded { path: f.path, result, after: f.after })
    }

    /// Drop the load; the start screen is usable again at once.
    pub fn cancel(&mut self) {
        self.flight = None;
    }

    #[cfg(test)]
    pub(crate) fn with_opener(&mut self, f: impl Fn(&Path) -> Result<Vault, String> + Send + Sync + 'static) {
        self.opener = Arc::new(f);
    }
}

fn folder_name(p: &Path) -> String {
    p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

impl ShaleApp {
    /// Open a vault without freezing the window: flush the open one, then read the new one on a
    /// worker thread (NET-019). The start screen shows the progress; [`Self::vault_load_tick`]
    /// finishes the open when the vault has arrived.
    pub fn open_vault_in_background(&mut self, path: &Path, after: After) {
        // one load at a time: Cancel it first
        if self.vault_load.is_loading() || !self.prepare_vault_switch() {
            return;
        }
        let ctx = self.ctx.clone();
        self.vault_load.start(&self.busy, path, after, ctx);
    }

    /// Every frame: take in a vault that has arrived and open it, or show why it did not load.
    pub(crate) fn vault_load_tick(&mut self, ctx: &egui::Context) {
        if self.vault_load.is_loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let Some(loaded) = self.vault_load.poll() else { return };
        match loaded.result {
            Ok(v) => {
                let known = !matches!(loaded.after, After::Guide(_));
                self.finish_open_vault(&loaded.path, v, known);
                match loaded.after {
                    After::Nothing => {}
                    After::View(v) => self.show_start_view(Some(&v)),
                    After::Guide(rel) => {
                        if self.vault.as_ref().is_some_and(|v| v.file(&rel).is_some()) {
                            self.open(&rel, false);
                        }
                    }
                }
            }
            Err(e) => self.vault_load.error = Some(format!("Could not open vault: {e}")),
        }
    }
}
