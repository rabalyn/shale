//! Git sync in the app: when it runs, what happens to open buffers, and its settings
//! (ADR-0008, ADR-0015; SYNC-002, SYNC-013…SYNC-017).

use super::{muted, ShaleApp};
use crate::busy::{self, Key};
use crate::drawing::DrawingDoc;
use crate::gitsync::wizard::diagnose;
use crate::gitsync::wizard::test_sync::{self, Problem};
use crate::gitsync::{self, lfs, Git, Report, TreeGuard, TreeLock};
use crate::theme;
use crate::vault::rel_to_string;
use crate::vault_state::LfsDownload;
use eframe::egui::{self, Ui};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::mpsc::TryRecvError;
use std::time::{Duration, Instant};

/// The git sync as a task (JOB-003): it cannot be stopped, because a sync that ends between its
/// commit and its merge or push would leave the repository half way (JOB-011).
pub(crate) const GIT_SYNC: Key = Key::new("git-sync");

/// "Store media with Git LFS" as a task; like the sync, it is not stopped half way.
pub(crate) const LFS_SETUP: Key = Key::new("git-lfs-setup");

/// The LFS download queue as a task (JOB-026): one download runs at a time, the others wait in
/// `pulls`. It cannot be stopped, because `git lfs pull` writes the files itself.
pub(crate) const LFS_DOWNLOAD: Key = Key::new("git-lfs-download");

/// A running `git lfs pull`: for one file a player asked for, or for the whole vault (`always`).
struct PullJob {
    vault: PathBuf,
    /// The file (absolute); None: the whole repository.
    file: Option<PathBuf>,
    job: busy::Job<Result<(), String>>,
}

/// The sync's state in the app.
#[derive(Default)]
pub struct SyncState {
    pub git: Git,
    /// The managed git and the choice of git (SYNC-023…SYNC-029).
    pub managed: crate::gitsync::install_ui::ManagedState,
    /// A running sync: its vault and where its result arrives.
    job: Option<(PathBuf, busy::Job<Result<Report, String>>)>,
    /// Held by the job while it changes files in the vault, and by the window while it writes
    /// notes (SYNC-013).
    pub tree: TreeLock,
    /// The "could not wait for the sync" message was shown and nothing was saved since.
    tree_told: bool,
    /// When the last sync started (None = sync as soon as possible).
    pub(crate) last_start: Option<Instant>,
    /// The last result: (local time, summary or error).
    pub last: Option<(String, Result<String, String>)>,
    /// Is git-lfs installed, does this vault store media with it (SYNC-016).
    pub lfs: lfs::Status,
    /// A running "Store media with Git LFS": its vault and where its result arrives (SYNC-017).
    lfs_job: Option<(PathBuf, busy::Job<Result<usize, String>>)>,
    /// The running LFS download (SYNC-020); one at a time, the others wait in `pulls`.
    pull: Option<PullJob>,
    /// Files asked for while a download runs (absolute paths); None: the whole vault.
    pulls: VecDeque<Option<PathBuf>>,
    /// `git --version`, looked up once (None inside = git is missing).
    git_version: Option<Option<String>>,
    /// What the cheap checks found when the last sync failed for an SSH key (SYNC-077): written by the
    /// sync job, read where the error is shown.
    pub(crate) facts: std::sync::Arc<std::sync::Mutex<crate::gitsync::wizard::diagnose::Facts>>,
    /// The report of the last sync that worked (the wizard's test sync tells it in words, SYNC-058).
    pub last_report: Option<Report>,
    /// The "Turn on sync" wizard, the detection of shared folders and the move out (SYNC-047…SYNC-063).
    pub setup: crate::gitsync::wizard::Setup,
    /// The URL field in the settings.
    pub url: String,
    /// `origin` of the open vault.
    pub remote: Option<String>,
    /// How many media files the open vault links from outside it, counted from the index once for
    /// each state of it, with that state (its number and generation): the note in the settings (MEDIA-023).
    outside_media: Option<((u64, u64), usize)>,
    /// The open vault has a repository of its own that nobody was asked about: no sync before the
    /// answer (SYNC-076).
    pub(crate) repo_ask: Option<super::git_ask::RepoAsk>,
    /// Where the settings controls were drawn (for UI tests).
    pub(crate) rects: Vec<(&'static str, egui::Rect)>,
}

impl SyncState {
    /// A state with this managed git (the window's, SYNC-023).
    pub fn with_managed(managed: crate::gitsync::install_ui::ManagedState) -> SyncState {
        SyncState { managed, ..Default::default() }
    }

    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// A Git LFS setup job is running.
    pub fn lfs_job_running(&self) -> bool {
        self.lfs_job.is_some()
    }

    /// Everything of the vault being left (VAULT-037): its jobs, the last result, the remote and
    /// the question about its repository. The git, the managed git, the lock of the files and the
    /// controls' places belong to the window and stay.
    pub(crate) fn drop_vault(&mut self) {
        self.job = None;
        self.lfs_job = None;
        self.pull = None;
        self.pulls.clear();
        self.last_start = None;
        self.last = None;
        self.last_report = None;
        self.facts = Default::default();
        self.lfs = lfs::Status::default();
        self.setup.forget_vault();
        self.url.clear();
        self.remote = None;
        self.outside_media = None;
        self.repo_ask = None;
        self.tree_told = false;
    }

    /// The git changed (SYNC-024): `git --version` is looked up again.
    pub(crate) fn forget_git_version(&mut self) {
        self.git_version = None;
    }
}

impl ShaleApp {
    /// A vault was opened: sync it soon, show its remote.
    pub(crate) fn sync_vault_opened(&mut self) {
        self.sync.last_start = None;
        self.sync.last = None;
        self.sync.setup.forget_vault();
        self.sync.remote = self.vault.as_ref().and_then(|v| gitsync::remote_url(&self.sync.git, &v.root)).map(|u| gitsync::hide_passwords(&u));
        self.sync.url = self.sync.remote.clone().unwrap_or_default();
        self.refresh_lfs();
    }

    /// Look up again whether git-lfs is installed and whether this vault uses it (SYNC-016).
    pub(crate) fn refresh_lfs(&mut self) {
        let root = self.vault.as_ref().map(|v| v.root.clone());
        self.sync.lfs = match root {
            Some(root) if self.git_version().is_some() => lfs::status(&self.sync.git, &root),
            _ => lfs::Status::default(),
        };
    }

    pub(crate) fn sync_options(&self) -> gitsync::Options {
        let s = &self.vdata.settings.sync;
        gitsync::Options { commit_prefix: s.commit_prefix.clone(), commit_suffix: s.commit_suffix.clone(), host: gitsync::host_name(), stamp: gitsync::stamp_now() }
    }

    /// "Store media with Git LFS": set the repository up in the background, then sync (SYNC-017).
    pub(crate) fn start_lfs_setup(&mut self) {
        if self.sync.running() || self.sync.lfs_job.is_some() || self.history.busy() {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        self.flush_all();
        let (git, tree, opts, ctx, job_root) = (self.sync.git.clone(), self.sync.tree.clone(), self.sync_options(), self.ctx.clone(), root.clone());
        let job = self.busy.spawn(
            LFS_SETUP,
            "Setting up Git LFS…",
            move || lfs::setup(&git, &job_root, &opts, &tree),
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.sync.lfs_job = job.map(|j| (root, j));
    }

    /// Collect a finished LFS setup: say what happened and sync, which uploads the files.
    fn lfs_job_done(&mut self, ctx: &egui::Context) -> bool {
        let Some((root, rx)) = &self.sync.lfs_job else { return false };
        let r = match rx.try_recv() {
            Ok(r) => r.unwrap_or_else(|_| Err("Git LFS setup stopped with an internal error".into())),
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(200));
                return true;
            }
            Err(TryRecvError::Disconnected) => Err("Git LFS setup stopped unexpectedly".into()),
        };
        let root = root.clone();
        self.sync.lfs_job = None;
        if self.vault.as_ref().map(|v| v.root.as_path()) != Some(root.as_path()) {
            return true; // another vault was opened meanwhile
        }
        self.refresh_lfs();
        match r {
            Ok(n) => {
                self.toast(if n > 0 { format!("Media files are stored with Git LFS from now on ({n} file(s) converted); syncing now") } else { "Media files are stored with Git LFS from now on".to_string() });
                self.sync.last_start = None;
                self.start_sync();
            }
            Err(e) => self.toast(format!("Could not set up Git LFS: {e}")),
        }
        true
    }

    /// Every frame: take the downloads the players and pictures asked for, run them one at a time
    /// in the background, and hand each result back (SYNC-020).
    fn lfs_pull_tick(&mut self, ctx: &egui::Context) {
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        for abs in self.media.take_download_requests() {
            self.sync.pulls.push_back(Some(abs));
        }
        if let Some(job) = &self.sync.pull {
            let r = match job.job.try_recv() {
                Ok(r) => r.unwrap_or_else(|e| Err(format!("the download stopped with an internal error: {e}"))),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return;
                }
                Err(TryRecvError::Disconnected) => Err("the download stopped unexpectedly".into()),
            };
            let (file, same_vault) = (job.file.clone(), job.vault == root);
            self.sync.pull = None;
            match (file, same_vault, r) {
                (Some(abs), _, r) => self.media.download_done(ctx, &abs, r),
                (None, true, Err(e)) => self.toast(format!("Downloading the LFS files failed: {e}")),
                _ => {}
            }
            ctx.request_repaint();
        }
        if self.sync.pull.is_none() {
            if let Some(next) = self.sync.pulls.pop_front() {
                if !self.start_pull(ctx, root, next.clone()) {
                    self.sync.pulls.push_front(next); // the action still runs elsewhere: try again next frame
                }
            }
        }
    }

    /// `git lfs pull` in the background, holding the tree lock: for one file (SYNC-020, VAULT-018
    /// checks its path) or for the whole vault.
    /// False: a download task is still listed, nothing was started.
    fn start_pull(&mut self, ctx: &egui::Context, root: PathBuf, file: Option<PathBuf>) -> bool {
        let (git, tree, ctx2, job_root) = (self.sync.git.clone(), self.sync.tree.clone(), ctx.clone(), root.clone());
        let rel = file.as_ref().map(|abs| abs.strip_prefix(&root).map(Path::to_path_buf).map_err(|_| "the file is not in the vault".to_string()));
        let label = match &file {
            Some(abs) => format!("Downloading {} from Git LFS…", abs.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            None => "Downloading the LFS files of the vault…".to_string(),
        };
        let job = self.busy.spawn(
            LFS_DOWNLOAD,
            label,
            move || match rel {
                None => lfs::pull_all(&git, &job_root, &tree),
                Some(Ok(rel)) => lfs::download(&git, &job_root, &rel, &tree),
                Some(Err(e)) => Err(e),
            },
            move || ctx2.request_repaint(),
        );
        let started = job.is_some();
        self.sync.pull = job.map(|job| PullJob { vault: root, file, job });
        started
    }

    pub(crate) fn git_version(&mut self) -> Option<String> {
        if self.sync.git_version.is_none() {
            self.sync.git_version = Some(self.sync.git.version());
        }
        self.sync.git_version.clone().flatten()
    }

    /// Take the lock on the vault's files before the window writes to them, waiting (a local
    /// merge is quick) up to 30 seconds for a running sync. None: the sync is still changing
    /// files; the caller writes nothing and the buffers stay dirty (SYNC-013).
    pub(crate) fn hold_sync_tree(&mut self) -> Option<TreeGuard> {
        let g = self.sync.tree.hold_for(Duration::from_secs(30));
        if g.is_some() {
            self.sync.tree_told = false;
        } else if !self.sync.tree_told {
            self.sync.tree_told = true;
            self.toast("The sync is still changing files in the vault, so nothing was saved yet. Your edits are kept and are saved when it is done".to_string());
        }
        g
    }

    /// A sync that only waits `run` and then reports `ok` (a panic when not), for the tests of the
    /// waits for a sync.
    #[cfg(test)]
    pub(crate) fn sync_test_job(&mut self, run: Duration, ok: bool) {
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        let job = self.busy.spawn(
            GIT_SYNC,
            "Syncing with git…",
            move || {
                std::thread::sleep(run);
                assert!(ok, "the sync of the test dies");
                Ok(Report::default())
            },
            || {},
        );
        self.sync.job = job.map(|j| (root, j));
    }

    /// Every frame: collect a finished sync, start one when it is due (SYNC-002).
    pub(crate) fn sync_tick(&mut self, ctx: &egui::Context) {
        self.wizard_tick(ctx);
        self.repo_ask_tick(ctx);
        self.lfs_pull_tick(ctx);
        if self.lfs_job_done(ctx) {
            return;
        }
        if let Some((root, rx)) = &self.sync.job {
            let r = match rx.try_recv() {
                Ok(r) => r.unwrap_or_else(|_| Err("the sync stopped with an internal error".into())),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200));
                    return;
                }
                Err(TryRecvError::Disconnected) => Err("the sync stopped unexpectedly".into()),
            };
            let root = root.clone();
            self.sync.job = None;
            self.after_sync(&root, r);
            return;
        }
        if !self.persist_config || !self.vdata.settings.sync.enabled || self.sync.repo_ask.is_some() {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        let interval = self.vdata.settings.sync.interval();
        if self.sync.last_start.is_some_and(|t| t.elapsed() < interval) {
            ctx.request_repaint_after(interval.saturating_sub(self.sync.last_start.map(|t| t.elapsed()).unwrap_or_default()).max(Duration::from_secs(1)));
            return;
        }
        if !gitsync::is_repo(&root) || self.git_version().is_none() {
            self.sync.last_start = Some(Instant::now());
            return;
        }
        self.start_sync();
    }

    /// What a failed sync says in plain words (SYNC-077…SYNC-081); None when git's own message is all there is.
    pub(crate) fn sync_problem(&self, error: &str) -> Option<Problem> {
        let facts = self.sync.facts.lock().map(|f| *f).unwrap_or_default();
        Some(diagnose::diagnose(error, &facts)).filter(|p| !matches!(p, Problem::Other(_)))
    }

    /// Save everything, then sync in the background.
    pub(crate) fn start_sync(&mut self) {
        let git = self.sync.git.clone();
        self.start_sync_with(git);
    }

    /// Like [`start_sync`](Self::start_sync) with this git (the wizard's test sync trusts a new SSH
    /// host once, SYNC-058). False: nothing started (another job runs, or there is no vault).
    pub(crate) fn start_sync_with(&mut self, git: Git) -> bool {
        // a snapshot and the sync never run on the repository at once (SYNC-035)
        if self.sync.running() || self.sync.lfs_job.is_some() || self.history.busy() || self.sync.repo_ask.is_some() {
            return false;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return false };
        self.sync.last_start = Some(Instant::now());
        self.flush_all();
        self.remember_open_files();
        self.save_config();
        let opts = self.sync_options();
        let tree = self.sync.tree.clone();
        let ctx = self.ctx.clone();
        let job_root = root.clone();
        let (facts, home) = (self.sync.facts.clone(), self.sync.setup.home().unwrap_or_default());
        let job = self.busy.spawn(
            GIT_SYNC,
            "Syncing with git…",
            move || {
                // a failed sign-in with an SSH key is looked at here, off the frame (SYNC-077)
                let r = gitsync::sync(&git, &job_root, &opts, &tree);
                let found = match &r {
                    Err(e) => diagnose::gather(&git, &job_root, &home, e),
                    Ok(_) => Default::default(),
                };
                if let Ok(mut f) = facts.lock() {
                    *f = found;
                }
                r
            },
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.sync.job = job.map(|j| (root, j));
        self.sync.job.is_some()
    }

    /// Bring buffers in line with what the sync changed (SYNC-013) and tell the user (SYNC-014).
    pub(crate) fn after_sync(&mut self, root: &Path, r: Result<Report, String>) {
        let when = {
            let (_, sod) = crate::calendar::time::utc_to_local(crate::calendar::time::now());
            crate::calendar::time::hhmm(sod)
        };
        let report = match r {
            Ok(rep) => rep,
            Err(e) => {
                let repeated = matches!(&self.sync.last, Some((_, Err(old))) if *old == e);
                if !repeated {
                    self.toast(format!("Git sync failed: {}", self.sync_problem(&e).map_or(e.clone(), |p| p.words())));
                }
                self.sync.last = Some((when, Err(e)));
                return;
            }
        };
        self.sync.last = Some((when, Ok(report.summary())));
        self.sync.last_report = Some(report.clone());
        self.refresh_lfs();
        if lfs::pull_after_sync(&self.vdata.settings.sync.lfs_download, root) && self.vault.as_ref().map(|v| v.root.as_path()) == Some(root) {
            self.sync.pulls.push_back(None);
        }
        if self.vault.as_ref().map(|v| v.root.as_path()) != Some(root) {
            return; // another vault was opened meanwhile
        }
        self.sync.remote = self.vault.as_ref().and_then(|v| gitsync::remote_url(&self.sync.git, &v.root)).map(|u| gitsync::hide_passwords(&u));
        let mut copies: Vec<PathBuf> = vec![];
        let mut failures: Vec<String> = vec![];
        for rel in &report.changed {
            if rel == Path::new(".shale").join(crate::vault_state::SETTINGS_FILE).as_path() {
                if let Some(p) = self.vdata.reload_settings() {
                    failures.push(p);
                }
                continue;
            }
            let abs = root.join(rel);
            // edited while the sync ran: the buffer goes to a conflict copy, never over the file
            let note_copy = match self.notes.get(rel) {
                Some(doc) if doc.dirty => Some(doc.text.clone().into_bytes()),
                _ => None,
            };
            let drawing_copy = match self.drawings.get(rel) {
                Some(d) if d.dirty => d.file_text().ok().map(String::into_bytes),
                _ => None,
            };
            let mut keep_buffer = false;
            if let Some(bytes) = note_copy.or(drawing_copy) {
                match crate::vault::write_conflict_copy(root, rel, &bytes) {
                    Ok(copy) => copies.push(copy),
                    Err(e) => {
                        failures.push(format!("Could not save your edits of “{}” as a copy: {e}", rel_to_string(rel)));
                        keep_buffer = true;
                    }
                }
            }
            if keep_buffer {
                continue;
            }
            let text = match crate::fsutil::read_to_string(&abs) {
                Ok(t) => t,
                Err(e) => {
                    // read-only with the reason, never an empty buffer (DATA-008)
                    if let Some(doc) = self.notes.get_mut(rel) {
                        doc.reload(String::new(), Some(e.to_string()));
                    }
                    continue;
                }
            };
            if let Some(doc) = self.notes.get_mut(rel) {
                doc.reload(text, None);
            } else if self.drawings.contains_key(rel) {
                let d = DrawingDoc::new(rel.clone(), &text);
                self.scenes.put(rel, d.scene.clone());
                self.drawings.insert(rel.clone(), d);
            }
        }
        if let Some(v) = self.vault.as_mut() {
            v.rescan();
        }
        self.rescan();
        let incoming = report.changed.len().saturating_sub(report.conflicts.len());
        let mut msg = vec![];
        if incoming > 0 {
            msg.push(format!("{incoming} file(s) updated from the remote"));
        }
        let all_copies: Vec<String> = report.conflicts.iter().map(|(_, c)| c).chain(copies.iter()).map(|c| rel_to_string(c)).collect();
        if !all_copies.is_empty() {
            msg.push(format!("both versions kept, see {}", all_copies.join(", ")));
        }
        if !msg.is_empty() {
            self.toast(format!("Git sync: {}", msg.join("; ")));
        }
        for f in failures {
            self.toast(f);
        }
    }

    /// How many media files the notes of the open vault link from outside the vault (each link
    /// target once): they are not synced and not in the file history (MEDIA-023). Counted from
    /// the index, no file system access.
    pub(crate) fn outside_media_count(&mut self) -> usize {
        let Some(v) = self.vault.as_ref() else { return 0 };
        let key = (v.uid, v.generation);
        match self.sync.outside_media {
            Some((k, n)) if k == key => n,
            _ => {
                let n = v.outside_links().links.iter().map(|(_, t)| t.as_str()).collect::<std::collections::HashSet<_>>().len();
                self.sync.outside_media = Some((key, n));
                n
            }
        }
    }

    /// The note in Settings → Sync for a vault with links to media outside it (MEDIA-023).
    pub(crate) fn outside_media_note(&mut self, ui: &mut Ui) {
        let n = self.outside_media_count();
        if n == 0 {
            return;
        }
        ui.add_space(8.0);
        crate::theme::hint(
            ui,
            format!(
                "{n} media {} linked from outside the vault and {} not synced or kept in the file history. A library link finds its file on another device when that device has the library folder set up (Settings → Import).",
                if n == 1 { "file is" } else { "files are" },
                if n == 1 { "is" } else { "are" }
            ),
        );
    }

    /// The "Git sync" part of the settings. Returns whether a setting changed.
    pub(crate) fn sync_settings_ui(&mut self, ui: &mut Ui) -> bool {
        self.sync.rects.clear();
        ui.label(egui::RichText::new("Git sync").color(theme::palette().muted));
        let mut changed = self.git_settings_ui(ui);
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return changed };
        if self.git_version().is_none() {
            crate::theme::paragraph(ui, "git is not installed. Install git to keep a history of this vault or to sync it with a git repository.");
            // the wizard's first step is the install (SYNC-048)
            let b = crate::settings::anchor::anchor(ui, "sync.turn_on", |ui| ui.button("Turn on sync…"));
            self.sync.rects.push(("turn-on", b.rect));
            if b.clicked() {
                self.open_sync_wizard();
            }
            return changed;
        }
        let repo = gitsync::is_repo(&root);
        let ctx = ui.ctx().clone();
        // where the vault is, looked up once in a background job (SYNC-061)
        self.sync.setup.detect_for(&root, &ctx, &self.busy);
        if repo {
            match &self.sync.remote {
                Some(u) => crate::theme::hint(ui, format!("Remote: {u}")),
                None => crate::theme::hint(ui, "No remote: changes are committed in the vault's own history only."),
            };
        }
        let findings = self.sync.setup.findings(&root).to_vec();
        let moved = self.sync.setup.moveout.result.clone();
        if crate::gitsync::wizard::warnings_ui(ui, &findings, repo, self.sync.remote.is_some(), &self.busy, moved.as_ref()) {
            self.start_moveout();
        }
        if self.sync.remote.is_none() {
            crate::theme::hint(ui, "Keep your notes safe with a history on this device, or sync them with a server (Forgejo, Gitea, GitHub, GitLab or any git address). Shale asks what you need and does the rest.");
            let b = crate::settings::anchor::anchor(ui, "sync.turn_on", |ui| ui.button("Turn on sync…"));
            self.sync.rects.push(("turn-on", b.rect));
            if b.clicked() {
                self.open_sync_wizard();
            }
        }
        let advanced = self.sync.setup.advanced;
        let b = ui.button(if advanced { "Advanced ▾" } else { "Advanced ▸" });
        self.sync.rects.push(("advanced", b.rect));
        if b.clicked() {
            self.sync.setup.advanced = !advanced;
        }
        if advanced {
            let remote = ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.sync.url).hint_text("https://… or git@host:notes.git").desired_width(300.0));
                self.sync.rects.push(("url", r.rect));
                let label = if self.sync.remote.is_some() { "Change remote" } else { "Connect" };
                let b = busy::ui::button(ui, &self.busy, &[&crate::gitsync::wizard::CONNECT], egui::Button::new(label));
                self.sync.rects.push(("connect", b.rect));
                if b.clicked() {
                    let url = self.sync.url.trim().to_string();
                    if crate::gitsync::movein::local_history_of(&root, &self.usage.places.history).is_some() {
                        // the versions kept on this computer move into the vault first: a job (SYNC-072)
                        self.start_join(root.clone(), url);
                    } else {
                        match gitsync::connect(&self.sync.git, &root, &url) {
                            Ok(()) => {
                                self.sync.remote = Some(url);
                                self.sync.last_start = None;
                                self.toast("Remote set; syncing now");
                                self.start_sync();
                            }
                            Err(e) => self.toast(format!("Could not set the remote: {e}")),
                        }
                    }
                }
                busy::ui::status(ui, &self.busy, &crate::gitsync::wizard::CONNECT);
            });
            crate::settings::anchor::mark(ui, "sync.remote", remote.response.rect);
        }
        if !repo {
            return changed;
        }
        crate::settings::anchor::anchor(ui, "sync.lfs", |ui| self.lfs_settings_ui(ui));
        let writable = self.vdata.settings_writable();
        let lfs_installed = self.sync.lfs.version.is_some();
        let rects = ui.add_enabled_ui(writable, |ui| {
            let s = &mut self.vdata.settings.sync;
            let r = crate::clickable::pointer(ui.checkbox(&mut s.enabled, "Sync this vault automatically"));
            changed |= r.changed();
            let enabled_rect = r.rect;
            let mut prefix_rect = egui::Rect::NOTHING;
            let every = ui.horizontal(|ui| {
                ui.label("Every");
                changed |= ui.add(egui::DragValue::new(&mut s.interval_minutes).range(1..=1440)).changed();
                ui.label("minutes");
            });
            crate::settings::anchor::mark(ui, "sync.automatic", enabled_rect.union(every.response.rect));
            let commit = ui.horizontal(|ui| {
                ui.label("Commit message prefix");
                let r = ui.add(egui::TextEdit::singleline(&mut s.commit_prefix).desired_width(120.0));
                changed |= r.changed();
                prefix_rect = r.rect;
                ui.label("suffix");
                changed |= ui.add(egui::TextEdit::singleline(&mut s.commit_suffix).desired_width(120.0)).changed();
            });
            crate::settings::anchor::mark(ui, "sync.commit_message", commit.response.rect);
            crate::theme::hint(ui, format!("Commits look like: {}Sync from {}: 3 file(s){}", s.commit_prefix, gitsync::host_name(), s.commit_suffix));
            let mut rects = vec![("enabled", enabled_rect), ("prefix", prefix_rect)];
            if lfs_installed {
                // SYNC-020: `sync.lfs_download`
                let mut always = s.lfs_download == LfsDownload::Always;
                let r = crate::clickable::pointer(ui.checkbox(&mut always, "Download media files after every sync (otherwise when you play or open them)"));
                if r.changed() {
                    s.lfs_download = if always { LfsDownload::Always } else { LfsDownload::OnDemand };
                    changed = true;
                }
                crate::settings::anchor::mark(ui, "sync.lfs_download", r.rect);
                rects.push(("lfs-always", r.rect));
            }
            rects
        });
        self.sync.rects.extend(rects.inner);
        let now = ui.horizontal(|ui| {
            let b = busy::ui::button(ui, &self.busy, &[&GIT_SYNC, &LFS_SETUP], egui::Button::new("Sync now"));
            self.sync.rects.push(("sync-now", b.rect));
            if b.clicked() {
                self.start_sync();
            }
            if !busy::ui::status(ui, &self.busy, &GIT_SYNC) {
                let problem = match &self.sync.last {
                    Some((_, Err(e))) => self.sync_problem(e),
                    _ => None,
                };
                let status = match &self.sync.last {
                    Some((t, Ok(s))) => muted(format!("Last sync {t}: {s}")),
                    Some((t, Err(e))) => {
                        let what = problem.as_ref().map_or(e.clone(), |p| p.words());
                        egui::RichText::new(format!("Last sync {t} failed: {what}")).color(theme::palette().link_missing)
                    }
                    None => muted("Not synced yet in this session"),
                };
                ui.label(status.size(12.0));
                if let Some(heading) = problem.and_then(|p| p.guide()) {
                    if ui.link("How to fix this").clicked() {
                        self.open_user_guide_at(test_sync::GUIDE_NOTE, heading);
                    }
                }
            }
        });
        crate::settings::anchor::mark(ui, "sync.now", now.response.rect);
        changed
    }

    /// Git LFS: installed or not, and the button that stores media with it (SYNC-016, SYNC-017).
    fn lfs_settings_ui(&mut self, ui: &mut Ui) {
        busy::ui::status(ui, &self.busy, &LFS_DOWNLOAD);
        let Some(version) = self.sync.lfs.version.clone() else {
            crate::theme::hint(ui, format!("Git LFS is not installed, so media files (audio, video) are stored like any other file. {}", lfs::INSTALL_HINT));
            return;
        };
        if self.sync.lfs.stores_media {
            crate::theme::hint(ui, format!("Media files (audio, video) of this repository are stored with Git LFS ({version})."));
            return;
        }
        ui.horizontal(|ui| {
            let b = busy::ui::button(ui, &self.busy, &[&GIT_SYNC, &LFS_SETUP], egui::Button::new("Store media with Git LFS"));
            self.sync.rects.push(("lfs-setup", b.rect));
            if b.clicked() {
                self.start_lfs_setup();
            }
            if !busy::ui::status(ui, &self.busy, &LFS_SETUP) {
                crate::theme::hint(ui, format!("Audio and video go to the LFS store of the remote instead of the history ({version})."));
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Options, View};
    use std::ffi::OsString;

    /// A hermetic git that finds no git-lfs (`hide_lfs`), so these tests are the same with and
    /// without git-lfs on the machine.
    fn hermetic() -> Git {
        let mut git = hermetic_with_lfs();
        crate::gitsync::hide_lfs(&mut git);
        git
    }

    /// The same with the real git-lfs: None (and a printed note) when it is not installed.
    fn hermetic_lfs() -> Option<Git> {
        if Git::default().lfs_version().is_none() {
            eprintln!("skipping: git-lfs is not installed here (CI installs it)");
            return None;
        }
        Some(hermetic_with_lfs())
    }

    fn hermetic_with_lfs() -> Git {
        let env = [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1"), ("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "init.defaultBranch"), ("GIT_CONFIG_VALUE_0", "main")];
        Git { program: "git".into(), env: env.iter().map(|(k, v)| (OsString::from(k), OsString::from(v))).collect() }
    }

    fn app_on(vault: &Path) -> ShaleApp {
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(vault.to_path_buf()), view: None, keys: None }, false);
        app.sync.git = hermetic();
        // the vault was opened with the computer's git: look again with the test's (SYNC-016)
        app.refresh_lfs();
        app
    }

    fn wait(app: &mut ShaleApp) {
        let ctx = egui::Context::default();
        let t = Instant::now();
        while app.sync.running() || app.sync.lfs_job.is_some() {
            assert!(t.elapsed() < Duration::from_secs(30), "sync did not finish");
            std::thread::sleep(Duration::from_millis(10));
            app.sync_tick(&ctx);
        }
    }

    // spec: SYNC-002
    #[test]
    fn sync_002_a_sync_starts_when_the_vault_opens_and_every_interval_but_only_while_enabled() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-timer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let v = root.join("vault");
        std::fs::create_dir_all(&v).unwrap();
        gitsync::connect(&git, &v, remote.to_str().unwrap()).unwrap();
        let mut app = app_on(&v);
        app.persist_config = true;
        let ctx = egui::Context::default();
        assert_eq!((app.vdata.settings.sync.enabled, app.vdata.settings.sync.interval()), (true, Duration::from_secs(300)), "on, every 5 minutes by default");
        app.vdata.settings.sync.interval_minutes = 0;
        assert_eq!(app.vdata.settings.sync.interval(), Duration::from_secs(60), "at least 1 minute");
        app.vdata.settings.sync.interval_minutes = 5;

        // off: the open vault is not synced
        app.vdata.settings.sync.enabled = false;
        app.sync_tick(&ctx);
        assert!(!app.sync.running() && app.sync.last_start.is_none());
        // on: the first tick of an opened vault starts a sync
        app.vdata.settings.sync.enabled = true;
        app.sync_tick(&ctx);
        assert!(app.sync.running(), "a sync starts when the vault is open");
        wait(&mut app);
        // inside the interval nothing starts, after it the next sync does
        app.sync.last_start = Some(Instant::now() - Duration::from_secs(290));
        app.sync_tick(&ctx);
        assert!(!app.sync.running(), "not before the interval has passed");
        app.sync.last_start = Some(Instant::now() - Duration::from_secs(301));
        app.sync_tick(&ctx);
        assert!(app.sync.running(), "the next sync starts after the interval");
        wait(&mut app);
        // the interval setting is the one that counts; a switched-off sync stays off
        app.vdata.settings.sync.interval_minutes = 1;
        app.sync.last_start = Some(Instant::now() - Duration::from_secs(61));
        app.vdata.settings.sync.enabled = false;
        app.sync_tick(&ctx);
        assert!(!app.sync.running());
        app.vdata.settings.sync.enabled = true;
        app.sync_tick(&ctx);
        assert!(app.sync.running());
        wait(&mut app);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-013, SYNC-014
    #[test]
    fn buffers_follow_the_sync_and_edits_made_meanwhile_become_copies() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let a = root.join("a");
        let b = root.join("b");
        for v in [&a, &b] {
            std::fs::create_dir_all(v).unwrap();
            gitsync::connect(&git, v, remote.to_str().unwrap()).unwrap();
        }
        crate::fsutil::write_atomic(&a.join("Clean.md"), b"base\n").unwrap();
        crate::fsutil::write_atomic(&a.join("Busy.md"), b"base\n").unwrap();
        let busy = TreeLock::default();
        let opts = gitsync::Options { host: "a".into(), stamp: "2026-11-14 1200".into(), ..Default::default() };
        gitsync::sync(&git, &a, &opts, &busy).unwrap();

        let mut app = app_on(&b);
        app.start_sync();
        wait(&mut app);
        assert!(matches!(&app.sync.last, Some((_, Ok(_)))), "{:?}", app.sync.last);
        app.open(Path::new("Clean.md"), false);
        app.open(Path::new("Busy.md"), true);
        assert_eq!(app.notes[Path::new("Clean.md")].text, "base\n");

        // the other machine changes both notes
        crate::fsutil::write_atomic(&a.join("Clean.md"), b"remote clean\n").unwrap();
        crate::fsutil::write_atomic(&a.join("Busy.md"), b"remote busy\n").unwrap();
        gitsync::sync(&git, &a, &opts, &busy).unwrap();

        // a sync whose result arrives while "Busy.md" has unsaved edits
        let rep = gitsync::sync(&git, &b, &opts, &busy).unwrap();
        let doc = app.notes.get_mut(Path::new("Busy.md")).unwrap();
        doc.text = "typed during the sync\n".into();
        doc.dirty = true;
        app.after_sync(&b, Ok(rep));

        assert_eq!(app.notes[Path::new("Clean.md")].text, "remote clean\n");
        assert_eq!(app.notes[Path::new("Busy.md")].text, "remote busy\n");
        assert!(!app.notes[Path::new("Busy.md")].dirty);
        assert_eq!(crate::fsutil::read_to_string(b.join("Busy.md")).unwrap(), "remote busy\n");
        let copies: Vec<_> = std::fs::read_dir(&b).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("Busy (conflict ")).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(crate::fsutil::read_to_string(b.join(&copies[0])).unwrap(), "typed during the sync\n");
        assert!(app.toasts.iter().any(|(t, _)| t.contains("both versions kept")), "{:?}", app.toasts);
        assert!(matches!(app.tabbar.tabs[app.tabbar.active].view, View::Note(_)));

        // the copy reaches the other machine with the next sync
        app.start_sync();
        wait(&mut app);
        gitsync::sync(&git, &a, &opts, &busy).unwrap();
        assert_eq!(crate::fsutil::read_to_string(a.join(&copies[0])).unwrap(), "typed during the sync\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: DATA-003, SYNC-013
    #[test]
    fn a_pull_under_unsaved_edits_gives_a_conflict_copy_even_if_autosave_comes_first() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let (a, b) = (root.join("a"), root.join("b"));
        for v in [&a, &b] {
            std::fs::create_dir_all(v).unwrap();
            gitsync::connect(&git, v, remote.to_str().unwrap()).unwrap();
        }
        crate::fsutil::write_atomic(&a.join("N.md"), b"base\n").unwrap();
        let busy = TreeLock::default();
        let opts = gitsync::Options { host: "a".into(), stamp: "2026-11-14 1200".into(), ..Default::default() };
        gitsync::sync(&git, &a, &opts, &busy).unwrap();
        let mut app = app_on(&b);
        app.start_sync();
        wait(&mut app);
        app.open(Path::new("N.md"), false);
        crate::fsutil::write_atomic(&a.join("N.md"), b"remote edit\n").unwrap();
        gitsync::sync(&git, &a, &opts, &busy).unwrap();

        // typing, then the pull lands in the folder (the sync result is not collected yet) and
        // the autosave of the buffer runs first
        let doc = app.notes.get_mut(Path::new("N.md")).unwrap();
        doc.text = "typed edit\n".into();
        doc.dirty = true;
        doc.last_edit = Instant::now() - Duration::from_secs(5);
        let rep = gitsync::sync(&git, &b, &opts, &busy).unwrap();
        app.autosave();
        assert_eq!(crate::fsutil::read_to_string(b.join("N.md")).unwrap(), "remote edit\n", "the pulled file is not overwritten");
        let copies: Vec<_> = std::fs::read_dir(&b).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("N (conflict ")).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        assert_eq!(crate::fsutil::read_to_string(b.join(&copies[0])).unwrap(), "typed edit\n");
        // the result is collected afterwards: no second copy, the buffer shows the pulled text
        app.after_sync(&b, Ok(rep));
        assert_eq!(app.notes[Path::new("N.md")].text, "remote edit\n");
        let copies: Vec<_> = std::fs::read_dir(&b).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("N (conflict ")).collect();
        assert_eq!(copies.len(), 1, "{copies:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Headless frames of the settings section (like the calendar dialog tests).
    struct H {
        ctx: egui::Context,
        t: f64,
    }

    impl H {
        fn frame(&mut self, app: &mut ShaleApp, events: Vec<egui::Event>) -> bool {
            self.t += 0.05;
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0))), time: Some(self.t), events, ..Default::default() };
            let mut changed = false;
            let mut out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| changed = app.sync_settings_ui(ui));
            });
            out.textures_delta.clear();
            changed
        }

        /// Every text a frame of the page paints.
        fn texts(&mut self, app: &mut ShaleApp) -> Vec<String> {
            fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
                match shape {
                    egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
                    egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
                    _ => {}
                }
            }
            self.frame(app, vec![]);
            self.t += 0.05;
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 900.0))), time: Some(self.t), ..Default::default() };
            let out = self.ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| app.sync_settings_ui(ui));
            });
            let mut texts = vec![];
            out.shapes.iter().for_each(|c| collect(&c.shape, &mut texts));
            texts
        }

        /// Frames until the page has its final layout (#659). The first frame starts the look for
        /// git and for where the vault is; when they end, the lines above the buttons change
        /// (the spinner row becomes the detection lines), so a click at a rect of an earlier frame
        /// misses the button, and only on a loaded machine. A job's task ends before its result is
        /// sent: once no job is left, one frame takes the results and the next lays them out.
        fn settle(&mut self, app: &mut ShaleApp) {
            let t = Instant::now();
            loop {
                self.frame(app, vec![]);
                let jobs = [&crate::gitsync::install_ui::GIT_DETECT, &crate::gitsync::install_ui::GIT_CHECK, &crate::gitsync::wizard::VAULT_PLACE];
                if !jobs.iter().any(|k| app.busy.is_running(k)) {
                    let before = app.sync.rects.clone();
                    self.frame(app, vec![]);
                    if app.sync.rects == before {
                        return;
                    }
                }
                assert!(t.elapsed() < Duration::from_secs(30), "the page did not settle");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn click(&mut self, app: &mut ShaleApp, key: &str) -> bool {
            let p = app.sync.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", app.sync.rects)).1.center();
            let b = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            let mut changed = self.frame(app, vec![egui::Event::PointerMoved(p)]);
            changed |= self.frame(app, vec![b(true)]);
            changed |= self.frame(app, vec![b(false)]);
            changed | self.frame(app, vec![])
        }

        fn typ(&mut self, app: &mut ShaleApp, key: &str, text: &str) -> bool {
            let mut changed = self.click(app, key);
            changed |= self.frame(app, vec![egui::Event::Text(text.into())]);
            changed | self.frame(app, vec![])
        }
    }

    // spec: SYNC-004, SYNC-014, SYNC-015, SYNC-047
    #[test]
    fn settings_connect_a_remote_and_change_the_sync_settings() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let v = root.join("vault");
        std::fs::create_dir_all(&v).unwrap();
        crate::fsutil::write_atomic(&v.join("Note.md"), b"hi\n").unwrap();
        let mut app = app_on(&v);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(&mut app, vec![]);
        assert!(!app.sync.rects.iter().any(|r| r.0 == "enabled"), "no sync settings before there is a repository");
        assert!(app.sync.rects.iter().any(|r| r.0 == "turn-on") && !app.sync.rects.iter().any(|r| r.0 == "url"), "the wizard is the way in, the address field is under Advanced");
        h.click(&mut app, "advanced");
        h.typ(&mut app, "url", remote.to_str().unwrap());
        h.click(&mut app, "connect");
        assert!(gitsync::is_repo(&v));
        assert_eq!(gitsync::remote_url(&git, &v).as_deref(), remote.to_str());
        wait(&mut app);
        assert!(matches!(&app.sync.last, Some((_, Ok(s))) if s.contains("up to date")), "{:?}", app.sync.last);
        let o = std::process::Command::new("git").arg("-C").arg(&remote).args(["show", "main:Note.md"]).envs(git.env.iter().cloned()).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n");

        h.frame(&mut app, vec![]);
        assert!(h.click(&mut app, "enabled"));
        assert!(!app.vdata.settings.sync.enabled);
        assert!(h.typ(&mut app, "prefix", "[n] "));
        assert_eq!(app.vdata.settings.sync.commit_prefix, "[n] ");
        crate::fsutil::write_atomic(&v.join("Note.md"), b"changed\n").unwrap();
        h.click(&mut app, "sync-now");
        wait(&mut app);
        let o = std::process::Command::new("git").arg("-C").arg(&remote).args(["log", "-1", "--format=%s", "main"]).envs(git.env.iter().cloned()).output().unwrap();
        assert!(String::from_utf8_lossy(&o.stdout).starts_with("[n] Sync from "), "{}", String::from_utf8_lossy(&o.stdout));
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-014
    #[test]
    fn sync_014_a_failed_sync_shows_gits_error_in_the_settings_and_once_as_a_message_and_a_good_one_shows_its_time() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-result-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let v = root.join("vault");
        std::fs::create_dir_all(&v).unwrap();
        gitsync::connect(&git, &v, remote.to_str().unwrap()).unwrap();
        crate::fsutil::write_atomic(&v.join("Note.md"), b"hi\n").unwrap();
        let mut app = app_on(&v);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        let has = |texts: &[String], part: &str| texts.iter().any(|t| t.contains(part));

        app.after_sync(&v, Err("git push: remote said no".into()));
        let texts = h.texts(&mut app);
        assert!(has(&texts, "failed: git push: remote said no"), "{texts:?}");
        assert_eq!(app.toasts.iter().filter(|(t, _)| t.contains("Git sync failed: git push: remote said no")).count(), 1, "{:?}", app.toasts);
        // the same error again: shown in the settings, not announced again
        app.after_sync(&v, Err("git push: remote said no".into()));
        assert_eq!(app.toasts.iter().filter(|(t, _)| t.contains("Git sync failed")).count(), 1, "{:?}", app.toasts);
        // another error is announced
        app.after_sync(&v, Err("git fetch: no network".into()));
        assert_eq!(app.toasts.iter().filter(|(t, _)| t.contains("Git sync failed")).count(), 2, "{:?}", app.toasts);

        // a good sync replaces the error with its time and what it did
        app.start_sync();
        wait(&mut app);
        let texts = h.texts(&mut app);
        assert!(has(&texts, "Last sync ") && has(&texts, "up to date with the remote") && !has(&texts, "failed"), "{texts:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: JOB-011
    #[test]
    fn job_011_a_running_sync_is_a_task_the_button_waits_for_that_cannot_stop_and_that_ends_with_the_sync() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-task-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut git = hermetic();
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let v = root.join("vault");
        std::fs::create_dir_all(&v).unwrap();
        gitsync::connect(&git, &v, remote.to_str().unwrap()).unwrap();
        // the commit of the sync waits in a hook until the test opens the gate, so the sync runs as long as the test wants
        let hooks = root.join("hooks");
        std::fs::create_dir_all(&hooks).unwrap();
        let hook = hooks.join("pre-commit");
        std::fs::write(&hook, "#!/bin/sh\nwhile [ ! -e .git/shale-test-gate ]; do sleep 0.05; done\n").unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        git.env.retain(|(k, _)| k != "GIT_CONFIG_COUNT");
        git.env.extend([("GIT_CONFIG_COUNT", "2"), ("GIT_CONFIG_KEY_1", "core.hooksPath"), ("GIT_CONFIG_VALUE_1", hooks.to_str().unwrap())].map(|(k, v)| (OsString::from(k), OsString::from(v))));
        crate::fsutil::write_atomic(&v.join("Note.md"), b"hi\n").unwrap();

        let mut app = app_on(&v);
        app.sync.git = git;
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.settle(&mut app);
        assert!(app.busy.list().is_empty());
        h.click(&mut app, "sync-now");
        let task = app.busy.info(&GIT_SYNC).expect("the running sync is listed");
        assert_eq!((task.label.as_str(), task.cancellable), ("Syncing with git…", false), "a sync is not stopped half way");
        assert!(app.sync.running());
        // no second sync from the button, from a key or from the timer
        assert!(!app.start_sync_with(app.sync.git.clone()));
        h.click(&mut app, "sync-now");
        assert_eq!(app.busy.list().len(), 1);
        std::fs::write(v.join(".git/shale-test-gate"), "").unwrap();
        wait(&mut app);
        assert!(app.busy.list().is_empty(), "the task ends with the sync");
        assert!(matches!(&app.sync.last, Some((_, Ok(_)))), "{:?}", app.sync.last);
        let o = std::process::Command::new("git").arg("-C").arg(&remote).args(["show", "main:Note.md"]).envs(app.sync.git.env.iter().cloned()).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&o.stdout), "hi\n", "the sync did its work");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-013
    #[test]
    fn autosave_waits_while_the_sync_changes_files() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-busy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join("N.md"), b"disk\n").unwrap();
        let mut app = app_on(&root);
        app.open(Path::new("N.md"), false);
        let doc = app.notes.get_mut(Path::new("N.md")).unwrap();
        doc.text = "edited\n".into();
        doc.dirty = true;
        doc.last_edit = Instant::now() - Duration::from_secs(5);
        let guard = app.sync.tree.try_hold().unwrap();
        app.autosave();
        assert_eq!(crate::fsutil::read_to_string(root.join("N.md")).unwrap(), "disk\n");
        assert!(app.notes[Path::new("N.md")].dirty);
        drop(guard);
        app.autosave();
        assert_eq!(crate::note_props::unstamped(&crate::fsutil::read_to_string(root.join("N.md")).unwrap()), "edited\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-013
    #[test]
    fn autosave_skips_while_another_thread_holds_the_tree_and_saves_after_it_lets_go() {
        let root = std::env::temp_dir().join(format!("shale-app-sync-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join("N.md"), b"disk\n").unwrap();
        let mut app = app_on(&root);
        app.open(Path::new("N.md"), false);
        let doc = app.notes.get_mut(Path::new("N.md")).unwrap();
        doc.text = "edited\n".into();
        doc.dirty = true;
        doc.last_edit = Instant::now() - Duration::from_secs(5);
        let lock = app.sync.tree.clone();
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let job = std::thread::spawn(move || {
            let _g = lock.hold();
            held_tx.send(()).unwrap();
            let _ = release_rx.recv();
        });
        held_rx.recv().unwrap();
        app.autosave();
        assert_eq!(crate::fsutil::read_to_string(root.join("N.md")).unwrap(), "disk\n");
        assert!(app.notes[Path::new("N.md")].dirty);
        release_tx.send(()).unwrap();
        job.join().unwrap();
        app.autosave();
        assert_eq!(crate::note_props::unstamped(&crate::fsutil::read_to_string(root.join("N.md")).unwrap()), "edited\n");
        assert!(!app.notes[Path::new("N.md")].dirty);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-013
    #[test]
    fn a_panic_inside_the_sync_does_not_keep_the_tree_locked() {
        let lock = TreeLock::default();
        let l2 = lock.clone();
        let _ = std::thread::spawn(move || {
            let _g = l2.hold();
            panic!("the job failed");
        })
        .join();
        assert!(lock.try_hold().is_some());
        let g = lock.try_hold().unwrap();
        assert!(lock.try_hold().is_none());
        assert!(lock.hold_for(Duration::from_millis(20)).is_none());
        drop(g);
        assert!(lock.hold_for(Duration::from_millis(20)).is_some());
    }

    // spec: CFG-005, CFG-006, SYNC-015
    #[test]
    fn vault_settings_and_state_live_in_the_vault() {
        let root = std::env::temp_dir().join(format!("shale-app-vdata-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join("One.md"), b"1\n").unwrap();
        std::fs::create_dir_all(root.join(".shale")).unwrap();
        crate::fsutil::write_atomic(&root.join(".shale/settings.json"), br#"{"version": 1, "people_dir": "Kontakte", "sync": {"interval_minutes": 2}}"#).unwrap();
        crate::fsutil::write_atomic(&root.join(".shale/state.json"), br#"{"version": 1, "open_files": ["One.md"], "last_import_folder": "Inbox"}"#).unwrap();
        let mut app = app_on(&root);
        assert_eq!(app.vdata.settings.people_dir, "Kontakte");
        assert_eq!(app.vdata.settings.sync.interval(), Duration::from_secs(120));
        assert_eq!(app.current_path().as_deref(), Some(Path::new("One.md")));
        assert_eq!(app.vdata.state.last_import_folder, "Inbox");
        app.vdata.state.recent_tags = vec!["t".into()];
        app.vdata.save().unwrap();
        let text = crate::fsutil::read_to_string(root.join(".shale/state.json")).unwrap();
        assert!(text.contains("\"t\""), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-016
    #[test]
    fn the_settings_say_that_git_lfs_is_missing_and_offer_nothing() {
        let root = std::env::temp_dir().join(format!("shale-app-lfs-missing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join("Note.md"), b"hi\n").unwrap();
        let git = hermetic();
        assert!(std::process::Command::new("git").arg("-C").arg(&root).args(["init", "--quiet"]).envs(git.env.iter().cloned()).status().unwrap().success());
        let mut app = app_on(&root);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.frame(&mut app, vec![]);
        assert_eq!(app.sync.lfs, lfs::Status::default());
        assert!(app.sync.rects.iter().any(|r| r.0 == "enabled"), "the other sync settings are there");
        assert!(!app.sync.rects.iter().any(|r| r.0 == "lfs-setup"), "{:?}", app.sync.rects);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-016, SYNC-017, SYNC-018
    #[test]
    fn the_button_stores_media_with_git_lfs_and_the_sync_uploads_it() {
        let Some(git) = hermetic_lfs() else { return };
        let root = std::env::temp_dir().join(format!("shale-app-lfs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let v = root.join("vault");
        std::fs::create_dir_all(&v).unwrap();
        // a vault that was a repository before Shale knew about LFS, with a song in its history
        assert!(std::process::Command::new("git").arg("-C").arg(&v).args(["init", "--quiet"]).envs(git.env.iter().cloned()).status().unwrap().success());
        crate::fsutil::write_atomic(&v.join("Song.mp3"), &[200u8; 50]).unwrap();
        gitsync::connect(&git, &v, remote.to_str().unwrap()).unwrap();
        let mut app = app_on(&v);
        app.sync.git = git.clone();
        app.start_sync();
        wait(&mut app);
        app.refresh_lfs();
        assert!(app.sync.lfs.version.is_some() && !app.sync.lfs.stores_media);
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        h.settle(&mut app);
        h.click(&mut app, "lfs-setup");
        wait(&mut app);
        assert!(app.sync.lfs.stores_media, "{:?}", app.toasts);
        assert!(app.toasts.iter().any(|(t, _)| t.contains("stored with Git LFS")), "{:?}", app.toasts);
        assert!(matches!(&app.sync.last, Some((_, Ok(s))) if s.contains("up to date")), "{:?}", app.sync.last);
        h.frame(&mut app, vec![]);
        assert!(!app.sync.rects.iter().any(|r| r.0 == "lfs-setup"), "the button is gone once it is done");
        let o = std::process::Command::new("git").arg("-C").arg(&remote).args(["show", "main:Song.mp3"]).envs(git.env.iter().cloned()).output().unwrap();
        assert!(String::from_utf8_lossy(&o.stdout).starts_with("version https://git-lfs"), "the remote holds a pointer");
        assert_eq!(std::fs::read(v.join("Song.mp3")).unwrap(), vec![200u8; 50]);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Frames until no download runs and none waits.
    fn wait_pulls(app: &mut ShaleApp) {
        let ctx = egui::Context::default();
        let t = Instant::now();
        loop {
            app.lfs_pull_tick(&ctx);
            if app.sync.pull.is_none() && app.sync.pulls.is_empty() {
                return;
            }
            assert!(t.elapsed() < Duration::from_secs(30), "the download did not finish");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    // spec: SYNC-020
    #[test]
    fn a_finished_sync_is_followed_by_a_pull_only_when_the_setting_is_always() {
        let root = std::env::temp_dir().join(format!("shale-app-lfs-always-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::fsutil::write_atomic(&root.join(".gitattributes"), b"*.mp3 filter=lfs diff=lfs merge=lfs -text\n").unwrap();
        let git = hermetic();
        assert!(std::process::Command::new("git").arg("-C").arg(&root).args(["init", "--quiet"]).envs(git.env.iter().cloned()).status().unwrap().success());
        let mut app = app_on(&root);
        // on demand, the default: nothing is pulled
        assert_eq!(app.vdata.settings.sync.lfs_download, LfsDownload::OnDemand);
        app.after_sync(&root, Ok(Report::default()));
        assert!(app.sync.pulls.is_empty() && app.sync.pull.is_none());
        // always: the pull is queued, runs in the background, and says why when it cannot (no git-lfs here)
        app.vdata.settings.sync.lfs_download = LfsDownload::Always;
        app.after_sync(&root, Ok(Report::default()));
        assert_eq!(app.sync.pulls.len(), 1);
        wait_pulls(&mut app);
        assert!(app.toasts.iter().any(|(t, _)| t.contains("Downloading the LFS files failed") && t.contains("not installed")), "{:?}", app.toasts);
        // a failed sync pulls nothing
        app.after_sync(&root, Err("offline".into()));
        assert!(app.sync.pulls.is_empty());
        // and a vault without LFS lines has nothing to pull
        std::fs::remove_file(root.join(".gitattributes")).unwrap();
        app.after_sync(&root, Ok(Report::default()));
        assert!(app.sync.pulls.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: SYNC-020
    #[test]
    fn a_download_the_player_asks_for_runs_in_the_background_and_hands_the_file_back() {
        let Some(git) = hermetic_lfs() else { return };
        let root = std::env::temp_dir().join(format!("shale-app-lfs-pull-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let remote = root.join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        assert!(std::process::Command::new("git").args(["init", "--quiet", "--bare"]).arg(&remote).envs(git.env.iter().cloned()).status().unwrap().success());
        let (a, b) = (root.join("a"), root.join("b"));
        for v in [&a, &b] {
            std::fs::create_dir_all(v).unwrap();
            gitsync::connect(&git, v, remote.to_str().unwrap()).unwrap();
        }
        let song = vec![77u8; 3000];
        crate::fsutil::write_atomic(&a.join("Song.mp3"), &song).unwrap();
        let opts = gitsync::Options { host: "a".into(), stamp: "2026-11-14 1200".into(), ..Default::default() };
        gitsync::sync(&git, &a, &opts, &TreeLock::default()).unwrap();
        let mut app = app_on(&b);
        app.sync.git = git.clone();
        app.start_sync();
        wait(&mut app);
        let abs = b.join("Song.mp3");
        assert!(lfs::parse_pointer(&std::fs::read(&abs).unwrap()).is_some(), "synced as a pointer");
        // the player asks; the app downloads and tells the player
        let ctx = egui::Context::default();
        let looks = crate::media::look::Looks::of(&ctx);
        assert!(looks.wait_for(&ctx, &abs).is_some_and(|f| f.pointer.is_some()), "the look-up sees the pointer");
        app.media.toggle(&ctx, &abs, Path::new("Note.md"));
        app.media.use_silent_output();
        wait_pulls(&mut app);
        assert_eq!(std::fs::read(&abs).unwrap(), song);
        looks.forget(&abs);
        assert!(looks.wait_for(&ctx, &abs).is_some_and(|f| f.pointer.is_none()), "the look-up sees the real file");
        assert_eq!(app.media.notice(&ctx, &abs), None);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: JOB-026
    #[test]
    fn job_026_the_lfs_download_queue_is_a_task_that_names_the_file_cannot_stop_and_shows_in_the_settings() {
        let root = std::env::temp_dir().join(format!("shale-app-lfs-task-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let git = hermetic();
        assert!(std::process::Command::new("git").arg("-C").arg(&root).args(["init", "--quiet"]).envs(git.env.iter().cloned()).status().unwrap().success());
        let mut app = app_on(&root);
        let ctx = egui::Context::default();
        // the download waits for the tree lock, so it runs as long as the test wants
        let guard = app.sync.tree.try_hold().expect("the lock is free");
        app.sync.pulls.push_back(Some(root.join("Song.mp3")));
        app.sync.pulls.push_back(None);
        app.lfs_pull_tick(&ctx);
        let task = app.busy.info(&LFS_DOWNLOAD).expect("the download is a task");
        assert_eq!((task.label.as_str(), task.cancellable), ("Downloading Song.mp3 from Git LFS…", false));
        assert_eq!(app.sync.pulls.len(), 1, "the other one waits behind it");
        // the settings show it with a spinner
        let mut h = H { ctx: egui::Context::default(), t: 0.0 };
        let shown = h.texts(&mut app);
        assert!(shown.iter().any(|t| t == "Downloading Song.mp3 from Git LFS…"), "{shown:?}");
        drop(guard);
        wait_pulls(&mut app);
        assert!(app.busy.list().is_empty(), "no task is left");
        // the whole vault has its own text
        let guard = app.sync.tree.try_hold().expect("the lock is free");
        app.sync.pulls.push_back(None);
        app.lfs_pull_tick(&ctx);
        assert_eq!(app.busy.info(&LFS_DOWNLOAD).map(|t| t.label), Some("Downloading the LFS files of the vault…".to_string()));
        drop(guard);
        wait_pulls(&mut app);
        // a download that finds its task still listed waits for the next frame and is not lost
        let held = app.busy.begin(LFS_DOWNLOAD, "held by the test").unwrap();
        app.sync.pulls.push_back(None);
        app.lfs_pull_tick(&ctx);
        assert!(app.sync.pull.is_none() && app.sync.pulls.len() == 1);
        drop(held);
        wait_pulls(&mut app);
        assert!(app.busy.list().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
