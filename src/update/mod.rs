//! Updating Shale from its GitHub releases (ADR-0023, UPD): the setting, the check, the download
//! with its checksum, the install for each kind of install, and the notices and dialogs. The
//! state of it all is one small [`UpdateState`] on the window; its decisions are functions of the
//! setting, the install kind and the release, so they are tested without a window, and the
//! network, download and install are three replaceable steps ([`Backend`]) so that the state is
//! tested without a network too.

pub mod check;
pub mod install;
pub mod kind;
mod ui;

// the tests use `sh`, Unix paths and (UPD-015) the Linux `install.sh`: they run on Linux and macOS
#[cfg(all(test, unix))]
mod fake_github;
#[cfg(all(test, unix))]
mod logic_tests;
#[cfg(all(test, unix))]
mod ui_tests;

use check::{Release, Source, Staged};
use install::{Installed, Relaunch};
use kind::{Capability, InstallKind};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The version of this program.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");

/// The environment variable that switches updates off for packagers and tests: `SHALE_UPDATES=off`.
pub const ENV: &str = "SHALE_UPDATES";

/// How long after the start the first check waits, how often it runs after that, and when a check
/// that failed is tried again.
const START_DELAY: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(24 * 3600);
const RETRY_AFTER: Duration = Duration::from_secs(3600);
/// How long the new version runs before the backup of the old one is removed.
const BACKUP_KEPT: Duration = Duration::from_secs(30);

/// What Shale does about a new release (UPD-001).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UpdateMode {
    /// Not chosen yet: checks like `Notify`, and the first update found asks which to use.
    #[default]
    Ask,
    /// No network request and no notice.
    Off,
    /// Say that there is a new version.
    Notify,
    /// Download and verify it, then say it is ready.
    Download,
    /// Download, verify and install it, then offer the restart.
    Install,
}

impl UpdateMode {
    /// The modes a person can choose.
    pub const CHOICES: [UpdateMode; 4] = [UpdateMode::Off, UpdateMode::Notify, UpdateMode::Download, UpdateMode::Install];

    pub fn label(self) -> &'static str {
        match self {
            UpdateMode::Ask => "Ask me when an update is found",
            UpdateMode::Off => "Off",
            UpdateMode::Notify => "Notify",
            UpdateMode::Download => "Download",
            UpdateMode::Install => "Install",
        }
    }

    /// One line on what the mode does.
    pub fn about(self) -> &'static str {
        match self {
            UpdateMode::Ask => "Shale looks for updates; when one is found it asks which of the four to use.",
            UpdateMode::Off => "Shale never asks the network about updates and shows nothing about them.",
            UpdateMode::Notify => "Shale tells you when a new version is out; you download it yourself.",
            UpdateMode::Download => "Shale downloads and checks the new version in the background; you install it.",
            UpdateMode::Install => "Shale downloads, checks and installs the new version; you restart when you like.",
        }
    }
}

/// The settings of the updates (UPD-001).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(default)]
pub struct UpdateConfig {
    pub mode: UpdateMode,
    /// The version the person chose "Skip this version" for ("" = none).
    pub skipped_version: String,
    /// When the last successful check was made (unix seconds, 0 = never).
    pub last_check: i64,
    /// A downloaded version that is installed at the next start ("" = none).
    pub install_on_start: String,
}

/// Whether the environment switches updates off (`SHALE_UPDATES=off`).
pub fn env_off(value: Option<&str>) -> bool {
    value.is_some_and(|v| v.trim().eq_ignore_ascii_case("off"))
}

/// The mode that counts: `Off` when the environment says so.
pub fn effective_mode(configured: UpdateMode, env_off: bool) -> UpdateMode {
    if env_off {
        UpdateMode::Off
    } else {
        configured
    }
}

/// What a found update gets done with `mode` on an install with `capability` (UPD-012): a
/// program that is never replaced, or a release without a file for this system, only notifies;
/// where the folder cannot be written, install is download.
pub fn act_as(mode: UpdateMode, capability: &Capability, has_file: bool) -> UpdateMode {
    match (mode, capability) {
        (UpdateMode::Off, _) => UpdateMode::Off,
        (_, _) if !has_file => UpdateMode::Notify,
        (_, Capability::NotifyOnly(_)) => UpdateMode::Notify,
        (UpdateMode::Install, Capability::DownloadOnly(_)) => UpdateMode::Download,
        (m, _) => m,
    }
}

// ---------------------------------------------------------------- the three steps

type CheckFn = Arc<dyn Fn(&AtomicBool) -> Result<Option<Release>, String> + Send + Sync>;
type FetchFn = Arc<dyn Fn(&Release, &AtomicBool) -> Result<Staged, String> + Send + Sync>;
type InstallFn = Arc<dyn Fn(&InstallKind, &Staged, &AtomicBool) -> Result<Installed, String> + Send + Sync>;

/// What reaches out: asking for the newest release, downloading and verifying it, installing it.
#[derive(Clone)]
pub struct Backend {
    pub check: CheckFn,
    pub fetch: FetchFn,
    pub install: InstallFn,
}

impl Backend {
    /// The real thing: GitHub (or `source` in a test), the staging folder `staging`, this system.
    pub fn real(source: Source, staging: PathBuf) -> Backend {
        Backend::real_for(source, staging, std::env::consts::OS, std::env::consts::ARCH)
    }

    /// [`Backend::real`] for another system (tests of every system's download).
    pub fn real_for(source: Source, staging: PathBuf, os: &'static str, arch: &'static str) -> Backend {
        let (check_source, fetch_source) = (source.clone(), source);
        Backend { check: Arc::new(move |cancel| check::fetch_latest(&check_source, cancel)), fetch: Arc::new(move |release, cancel| check::stage(&fetch_source, release, os, arch, &staging, cancel)), install: Arc::new(install::apply) }
    }
}

// ---------------------------------------------------------------- the state

/// A newer release, and what is wrong with installing it here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub release: Release,
    /// Why there is no file to download for this system, if there is none.
    pub no_file: Option<String>,
}

/// Where the update of a found release is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Downloading {
        then_install: bool,
    },
    /// Downloaded, and its checksum matched.
    Ready(Staged),
    Installing,
    Installed(Installed),
    Failed(String),
}

/// What the last check found, for the settings page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckResult {
    UpToDate(String),
    Available(String),
    Failed(String),
}

/// What the window asks of the state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Download,
    Install,
    InstallAtStart,
    Skip,
    Later,
    RestartNow,
    Dismiss,
    ShowNotes,
}

type Job<T> = Receiver<Result<Result<T, String>, String>>;

pub struct UpdateState {
    /// Checks and downloads happen (not in tests and pictures, which have no network).
    pub enabled: bool,
    pub current: String,
    pub env_off: bool,
    pub kind: InstallKind,
    /// The system and processor whose download is wanted (`std::env::consts`; tests set others).
    pub platform: (&'static str, &'static str),
    pub exe: PathBuf,
    /// The folder an update writes into can be written.
    pub writable: bool,
    pub backend: Backend,
    pub start_delay: Duration,
    started: Instant,
    next_check: Option<Instant>,
    force: bool,
    cancel: Arc<AtomicBool>,
    check_job: Option<Job<Option<Release>>>,
    fetch_job: Option<Job<Staged>>,
    install_job: Option<Job<Installed>>,
    pub found: Option<Found>,
    pub phase: Phase,
    pub result: Option<CheckResult>,
    /// "Later": no notice until the next check.
    pub hidden: bool,
    /// The question which mode to use is open.
    pub ask: bool,
    /// The release notes are open.
    pub notes_open: bool,
    cleaned_at_start: bool,
    cleaned: bool,
    /// Set by "Restart now": the program to start when this one has saved and ended.
    pub restart: Option<Relaunch>,
}

/// The state of a window without updates (tests, pictures): it never looks at the network.
impl Default for UpdateState {
    fn default() -> Self {
        let source = Source::github();
        let staging = check::staging_root();
        UpdateState::with(InstallKind::Development, PathBuf::new(), Backend::real(source, staging), false)
    }
}

impl UpdateState {
    /// A state for an install of `kind` run from `exe`.
    pub fn with(kind: InstallKind, exe: PathBuf, backend: Backend, enabled: bool) -> UpdateState {
        let writable = kind::capability(&kind, true) == Capability::Install && kind::is_writable(&kind::install_folder(&kind));
        UpdateState {
            enabled,
            current: CURRENT.to_string(),
            env_off: env_off(std::env::var(ENV).ok().as_deref()),
            kind,
            platform: (std::env::consts::OS, std::env::consts::ARCH),
            exe,
            writable,
            backend,
            start_delay: START_DELAY,
            started: Instant::now(),
            next_check: None,
            force: false,
            cancel: Arc::new(AtomicBool::new(false)),
            check_job: None,
            fetch_job: None,
            install_job: None,
            found: None,
            phase: Phase::Idle,
            result: None,
            hidden: false,
            ask: false,
            notes_open: false,
            cleaned_at_start: false,
            cleaned: false,
            restart: None,
        }
    }

    /// The state of the running program: its install kind found from its path.
    pub fn for_running_program() -> UpdateState {
        let exe = std::env::current_exe().and_then(|p| p.canonicalize()).unwrap_or_default();
        let kind = kind::detect(&exe, &kind::Facts::gather(&exe));
        let staging = check::staging_root();
        UpdateState::with(kind, exe, Backend::real(Source::github(), staging), true)
    }

    pub fn capability(&self) -> Capability {
        kind::capability(&self.kind, self.writable)
    }

    pub fn mode(&self, cfg: &UpdateConfig) -> UpdateMode {
        effective_mode(cfg.mode, self.env_off)
    }

    /// What the next check does: "Check for updates now".
    pub fn check_now(&mut self) {
        self.force = true;
        self.result = None;
    }

    /// Whether a check is running or about to start.
    pub fn check_pending(&self) -> bool {
        self.check_job.is_some() || self.force
    }

    /// Whether a check, download or install is running.
    pub fn busy(&self) -> bool {
        self.check_job.is_some() || self.fetch_job.is_some() || self.install_job.is_some()
    }

    fn start_job<T: Send + 'static>(&mut self, ctx: &eframe::egui::Context, f: impl FnOnce(&AtomicBool) -> Result<T, String> + Send + 'static) -> Job<T> {
        self.cancel = Arc::new(AtomicBool::new(false));
        let (cancel, ctx) = (self.cancel.clone(), ctx.clone());
        crate::jobs::spawn(move || f(&cancel), move || ctx.request_repaint())
    }

    fn start_check(&mut self, ctx: &eframe::egui::Context) {
        self.force = false;
        let check = self.backend.check.clone();
        self.check_job = Some(self.start_job(ctx, move |c| check(c)));
    }

    fn start_fetch(&mut self, ctx: &eframe::egui::Context, then_install: bool) {
        let Some(found) = self.found.clone().filter(|f| f.no_file.is_none()) else { return };
        let fetch = self.backend.fetch.clone();
        self.phase = Phase::Downloading { then_install };
        self.fetch_job = Some(self.start_job(ctx, move |c| fetch(&found.release, c)));
    }

    fn start_install(&mut self, ctx: &eframe::egui::Context, staged: Staged) {
        let (install, kind) = (self.backend.install.clone(), self.kind.clone());
        self.phase = Phase::Installing;
        self.install_job = Some(self.start_job(ctx, move |c| install(&kind, &staged, c)));
    }

    /// A release was found that is newer than this program: what the mode does with it (UPD-007 to UPD-010).
    fn on_found(&mut self, ctx: &eframe::egui::Context, cfg: &mut UpdateConfig) -> bool {
        let Some(found) = self.found.clone() else { return false };
        let version = found.release.version.clone();
        if cfg.skipped_version == version {
            return false;
        }
        let mut changed = false;
        if cfg.install_on_start == version {
            cfg.install_on_start.clear();
            changed = true;
            if self.capability() == Capability::Install && found.no_file.is_none() {
                self.start_fetch(ctx, true);
                return changed;
            }
        }
        match self.mode(cfg) {
            UpdateMode::Ask => self.ask = true,
            UpdateMode::Off => {}
            mode => match act_as(mode, &self.capability(), found.no_file.is_none()) {
                UpdateMode::Download => self.start_fetch(ctx, false),
                UpdateMode::Install => self.start_fetch(ctx, true),
                _ => {}
            },
        }
        changed
    }

    /// The answer of the question which mode to use (UPD-007): it is kept, and the found update gets it.
    pub fn choose(&mut self, ctx: &eframe::egui::Context, cfg: &mut UpdateConfig, mode: UpdateMode) {
        cfg.mode = mode;
        self.ask = false;
        if mode == UpdateMode::Off {
            self.quiet();
        } else {
            self.on_found(ctx, cfg);
        }
    }

    /// Updates are off: nothing is running, nothing is shown.
    fn quiet(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.check_job = None;
        self.fetch_job = None;
        self.install_job = None;
        self.found = None;
        self.phase = Phase::Idle;
        self.ask = false;
        self.notes_open = false;
        self.force = false;
    }

    fn handle_check(&mut self, ctx: &eframe::egui::Context, cfg: &mut UpdateConfig, got: Result<Option<Release>, String>) -> bool {
        match got {
            Ok(release) => {
                cfg.last_check = crate::calendar::time::now();
                self.next_check = Some(Instant::now() + CHECK_EVERY);
                match release.filter(|r| check::is_newer(&self.current, &r.version)) {
                    Some(r) => {
                        let no_file = check::asset_for(&r, self.platform.0, self.platform.1).err();
                        let same = self.found.as_ref().is_some_and(|f| f.release.version == r.version);
                        self.result = Some(CheckResult::Available(r.version.clone()));
                        self.found = Some(Found { release: r, no_file });
                        if same && self.phase != Phase::Idle {
                            return true;
                        }
                        self.hidden = false;
                        self.on_found(ctx, cfg);
                    }
                    None => {
                        self.found = None;
                        self.result = Some(CheckResult::UpToDate(self.current.clone()));
                        cfg.install_on_start.clear();
                    }
                }
                true
            }
            Err(e) => {
                self.next_check = Some(Instant::now() + RETRY_AFTER);
                self.result = Some(CheckResult::Failed(e));
                false
            }
        }
    }

    /// One frame of the updates: finished jobs are taken in, a due check is started (UPD-003).
    /// Returns whether the settings changed and have to be saved.
    pub fn tick(&mut self, ctx: &eframe::egui::Context, cfg: &mut UpdateConfig) -> bool {
        if !self.enabled {
            return false;
        }
        self.clean_backups();
        if self.mode(cfg) == UpdateMode::Off {
            self.quiet();
            return false;
        }
        let mut changed = false;
        if let Some(r) = self.check_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.check_job = None;
            changed |= self.handle_check(ctx, cfg, r.and_then(|x| x));
        }
        if let Some(r) = self.fetch_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.fetch_job = None;
            match r.and_then(|x| x) {
                Ok(staged) if matches!(self.phase, Phase::Downloading { then_install: true }) && self.capability() == Capability::Install => self.start_install(ctx, staged),
                Ok(staged) => self.phase = Phase::Ready(staged),
                Err(e) if e == "cancelled" => self.phase = Phase::Idle,
                Err(e) => self.phase = Phase::Failed(e),
            }
        }
        if let Some(r) = self.install_job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.install_job = None;
            match r.and_then(|x| x) {
                Ok(installed) => self.phase = Phase::Installed(installed),
                Err(e) if e == "cancelled" => self.phase = Phase::Idle,
                Err(e) => self.phase = Phase::Failed(e),
            }
        }
        if self.check_job.is_none() {
            let due = self.force || self.next_check.map_or(self.started.elapsed() >= self.start_delay, |t| Instant::now() >= t);
            if due {
                self.start_check(ctx);
            }
        }
        if self.busy() {
            ctx.request_repaint_after(Duration::from_millis(300));
        } else if let Some(t) = self.next_check {
            ctx.request_repaint_after(t.saturating_duration_since(Instant::now()).max(Duration::from_secs(1)));
        } else {
            ctx.request_repaint_after(self.start_delay.saturating_sub(self.started.elapsed()).max(Duration::from_secs(1)));
        }
        changed
    }

    /// The backup of the old version goes once the new one has run for a moment; the Windows
    /// leftover is tried at the very start too (UPD-018).
    fn clean_backups(&mut self) {
        if !self.cleaned_at_start {
            self.cleaned_at_start = true;
            if matches!(self.kind, InstallKind::Windows { .. }) {
                install::remove_leftovers(&self.kind, &self.exe);
            }
        }
        if !self.cleaned && self.started.elapsed() >= BACKUP_KEPT {
            self.cleaned = true;
            install::remove_leftovers(&self.kind, &self.exe);
        }
    }

    /// The window did something (UPD-008 to UPD-010, UPD-019). Returns whether the settings changed.
    pub fn act(&mut self, ctx: &eframe::egui::Context, cfg: &mut UpdateConfig, action: Action) -> bool {
        let version = self.found.as_ref().map(|f| f.release.version.clone()).unwrap_or_default();
        match action {
            Action::Download => self.start_fetch(ctx, false),
            Action::Install => match std::mem::replace(&mut self.phase, Phase::Idle) {
                Phase::Ready(staged) => self.start_install(ctx, staged),
                other => {
                    self.phase = other;
                    self.start_fetch(ctx, true);
                }
            },
            Action::InstallAtStart => {
                cfg.install_on_start = version;
                self.hidden = true;
                return true;
            }
            Action::Skip => {
                cfg.skipped_version = version;
                self.hidden = true;
                return true;
            }
            Action::Later => self.hidden = true,
            Action::RestartNow => {
                if let Phase::Installed(i) = &self.phase {
                    self.restart = Some(i.relaunch.clone());
                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                }
            }
            Action::Dismiss => {
                if matches!(self.phase, Phase::Failed(_)) {
                    self.phase = Phase::Idle;
                }
                self.hidden = true;
            }
            Action::ShowNotes => self.notes_open = true,
        }
        false
    }

    /// Start the program again if "Restart now" asked for it; called when the window has saved and is ending.
    pub fn start_restart(&mut self) {
        if let Some(r) = self.restart.take() {
            if let Err(e) = r.spawn() {
                eprintln!("shale: {e}");
            }
        }
    }
}

/// The time of a check as `2026-11-19 14:05` in local time.
pub fn when(utc: i64) -> String {
    let (days, secs) = crate::calendar::time::utc_to_local(utc);
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02} {}", crate::calendar::time::hhmm(secs))
}
