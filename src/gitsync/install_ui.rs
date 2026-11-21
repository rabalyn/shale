//! Settings → Sync, the git part (SYNC-023…SYNC-029): which git there is and which one is used, the
//! choice, the managed git (install, update, remove) and the opt-in check for a new version. The
//! state is one small struct of the sync state; every slow thing (looking, the install, the check)
//! is a background job (JOB-001).

use super::install::{self, Env, Handle};
use super::managed::{self, Detection, GitChoice, GitConfig, GitUpdates, Installed, Origin};
use super::sources::{GitHelp, Have, Offer, Plan, Platform, Sources};
use crate::app::ShaleApp;
use crate::data_usage::human_bytes;
use crate::theme;
use eframe::egui::{self, Context, Ui};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long after the start the first check for a new version waits, and how often it runs after that.
const CHECK_DELAY: Duration = Duration::from_secs(30);
const CHECK_EVERY: Duration = Duration::from_secs(7 * 24 * 3600);
/// A failed check is tried again after this.
const CHECK_RETRY: Duration = Duration::from_secs(3600);

/// What the person is told after a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub ok: bool,
}

impl Notice {
    fn new(text: impl Into<String>, ok: bool) -> Notice {
        Notice { text: text.into(), ok }
    }
}

/// What a running job does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Install,
    Update,
    Remove,
}

/// A running install, update or removal.
pub struct Task {
    pub kind: TaskKind,
    pub handle: Handle,
    pub(super) rx: Receiver<Result<Notice, String>>,
}

type CheckJob = Receiver<Result<Result<Option<Plan>, String>, String>>;

/// The git part of the sync page.
pub struct ManagedState {
    /// The user's data folder, where the managed git lives; none: there is no such folder.
    pub data: Option<PathBuf>,
    path_var: Option<OsString>,
    /// This state decides which git the window runs (not in tests and pictures, which keep their own).
    pub select: bool,
    /// The check for a new version runs by itself (not in tests and pictures, which have no network).
    pub checks: bool,
    pub sources: Sources,
    pub platform: Platform,
    /// `/etc/os-release`, for the install command of the distribution.
    pub os_release: Option<String>,
    /// What the look found, until something changes it.
    pub detection: Option<Detection>,
    /// The choice the detection was made for.
    detected_for: GitChoice,
    detecting: Option<Receiver<Result<Detection, String>>>,
    pub task: Option<Task>,
    pub notice: Option<Notice>,
    /// The person pressed Remove and has not confirmed yet.
    pub confirm_remove: bool,
    /// A newer release than the installed one, found by the check.
    pub update: Option<Plan>,
    checking: Option<(CheckJob, bool)>,
    cancel_check: Arc<AtomicBool>,
    started: Instant,
    next_check: Option<Instant>,
    /// The choice the window's git was selected for; a different one in the settings selects again.
    applied: Option<GitChoice>,
    /// The window's git has to be selected again (an install, an update or a removal ended).
    reselect: bool,
    /// Apple's installer was started from this page.
    xcode_started: bool,
}

impl Default for ManagedState {
    fn default() -> Self {
        ManagedState {
            data: None,
            path_var: None,
            select: false,
            checks: false,
            sources: Sources::real(),
            platform: Platform::current(),
            os_release: None,
            detection: None,
            detected_for: GitChoice::Auto,
            detecting: None,
            task: None,
            notice: None,
            confirm_remove: false,
            update: None,
            checking: None,
            cancel_check: Arc::default(),
            started: Instant::now(),
            next_check: None,
            applied: None,
            reselect: false,
            xcode_started: false,
        }
    }
}

impl ManagedState {
    /// `persist`: the real data folder, and the git it picks is the window's; otherwise (tests and
    /// screenshots) a temporary data folder named after `tag` and nothing is selected or checked by itself.
    pub fn new(persist: bool, tag: &str) -> ManagedState {
        let data = if persist {
            dirs::data_dir()
        } else {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            // screenshots name a data folder to show (SHALE_FAKE_DATA_DIR); nothing is written there
            let fake = std::env::var_os("SHALE_FAKE_DATA_DIR").map(PathBuf::from);
            Some(fake.unwrap_or_else(|| std::env::temp_dir().join(format!("shale-gitdata-{tag}-{}", NEXT.fetch_add(1, Ordering::SeqCst)))))
        };
        let os_release = if cfg!(target_os = "linux") { crate::fsutil::read_to_string("/etc/os-release").ok() } else { None };
        ManagedState { data, path_var: std::env::var_os("PATH"), select: persist, checks: persist, os_release, ..Default::default() }
    }

    /// Tests: a state with this data folder, `PATH` and sources.
    #[cfg(all(test, unix))]
    pub(crate) fn for_test(data: PathBuf, path_var: Option<OsString>, sources: Sources, platform: Platform) -> ManagedState {
        ManagedState { data: Some(data), path_var, sources, platform, ..Default::default() }
    }

    /// Tests: what the look found, as if a job had found it for this choice.
    #[cfg(all(test, unix))]
    pub(crate) fn set_detection(&mut self, d: Detection, choice: GitChoice) {
        self.detection = Some(d);
        self.detected_for = choice;
    }

    /// Tests: the next frame's check for a new version is due.
    #[cfg(all(test, unix))]
    pub(crate) fn check_is_due(&mut self) {
        self.next_check = Some(Instant::now());
    }

    /// The `Git` to run for this choice (SYNC-024).
    pub fn selected(&self, choice: GitChoice) -> super::Git {
        let mut git = managed::selected_git(choice, self.data.as_deref(), self.path_var.as_deref());
        // git asks this executable for tokens of the wizard's sign-ins (SYNC-054)
        if self.select {
            if let Ok(exe) = std::env::current_exe() {
                super::credentials::attach(&mut git, &exe);
            }
        }
        git
    }

    /// Whether the window's git has to be selected again for this choice; call it every frame.
    pub fn take_reselect(&mut self, choice: GitChoice) -> bool {
        let again = std::mem::take(&mut self.reselect) || self.applied != Some(choice);
        self.applied = Some(choice);
        again
    }

    fn running(&self) -> bool {
        self.task.is_some()
    }

    /// Look at this computer again, in a job.
    pub fn check_again(&mut self, ctx: &Context, choice: GitChoice) {
        if self.detecting.is_some() {
            return;
        }
        self.detection = None;
        self.detected_for = choice;
        let (data, path) = (self.data.clone(), self.path_var.clone());
        let ctx = ctx.clone();
        self.detecting = Some(crate::jobs::spawn(move || managed::detect(choice, data.as_deref(), path.as_deref()), move || ctx.request_repaint()));
    }

    /// Take what the jobs have finished; call it every frame.
    pub fn poll(&mut self, ctx: &Context) {
        if let Some(rx) = &self.detecting {
            if let Ok(r) = rx.try_recv() {
                self.detecting = None;
                match r {
                    Ok(d) => self.detection = Some(d),
                    Err(e) => self.notice = Some(Notice::new(format!("Looking for git failed: {e}"), false)),
                }
            }
        }
        if let Some(t) = &self.task {
            if let Ok(r) = t.rx.try_recv() {
                self.task = None;
                self.notice = Some(r.unwrap_or_else(|e| Notice::new(e, false)));
                // the install, the update and the removal change what is there and what is in use
                self.detection = None;
                self.reselect = true;
                self.update = None;
            }
        }
        if self.task.is_some() || self.detecting.is_some() || self.checking.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    fn start(&mut self, ctx: &Context, kind: TaskKind, work: impl FnOnce(&Handle, &dyn Fn()) -> Notice + Send + 'static) {
        if self.task.is_some() {
            return;
        }
        let handle = Handle::default();
        let (h, c1, c2) = (handle.clone(), ctx.clone(), ctx.clone());
        let rx = crate::jobs::spawn(move || work(&h, &|| c1.request_repaint()), move || c2.request_repaint());
        self.task = Some(Task { kind, handle, rx });
        self.notice = None;
    }

    /// Download and install what `plan` says (SYNC-025), from the button that shows its sizes.
    pub fn begin_install(&mut self, ctx: &Context, plan: Plan) {
        if let Some(data) = self.data.clone() {
            let what = plan.what();
            self.start(ctx, TaskKind::Install, move |h, n| done_notice(install::install(&Env { data }, &plan, h, n), &format!("The install of {what}")));
        }
    }

    /// Replace the managed git by the newer release found (SYNC-027).
    pub fn begin_update(&mut self, ctx: &Context) {
        if let (Some(data), Some(plan)) = (self.data.clone(), self.update.clone()) {
            self.start(ctx, TaskKind::Update, move |h, n| done_notice(install::update(&Env { data }, &plan, h, n), "The update"));
        }
    }

    /// Look for a newer release in a job (SYNC-027); `forced`: the person asked, so the answer is shown.
    pub fn begin_check(&mut self, ctx: &Context, forced: bool) {
        if self.checking.is_some() {
            return;
        }
        let Some(installed) = self.data.as_deref().and_then(|d| Installed::read(&managed::managed_root(d))) else { return };
        let (sources, platform) = (self.sources.clone(), self.platform.clone());
        let cancel = self.cancel_check.clone();
        let ctx = ctx.clone();
        let rx = crate::jobs::spawn(move || install::check_latest(&sources, &platform, &installed, &|url| install::fetch_api(url, sources.allow_http, &cancel)), move || ctx.request_repaint());
        self.checking = Some((rx, forced));
    }

    /// One frame of the checks (SYNC-027): the answer of a running check is taken, a due check is
    /// started (30 seconds after the start, then every 7 days, a failed one again after an hour), and a
    /// newer release is installed when the setting says so. Returns a message for the window to show.
    pub fn tick(&mut self, ctx: &Context, cfg: &GitConfig) -> Option<String> {
        self.poll(ctx);
        let mut message = None;
        if let Some((rx, forced)) = &self.checking {
            if let Ok(r) = rx.try_recv() {
                let forced = *forced;
                self.checking = None;
                match r.and_then(|x| x) {
                    Ok(found) => {
                        self.next_check = Some(Instant::now() + CHECK_EVERY);
                        let versions = found.as_ref().map(Plan::versions_text);
                        self.update = found;
                        match (versions, cfg.updates) {
                            (Some(v), GitUpdates::Install) if self.task.is_none() => {
                                self.begin_update(ctx);
                                message = Some(format!("Updating the git that Shale installed ({v})"));
                            }
                            (Some(v), _) => message = Some(format!("A newer git is available ({v}): Settings → Sync")),
                            (None, _) if forced => self.notice = Some(Notice::new("The git that Shale installed is up to date.", true)),
                            (None, _) => {}
                        }
                    }
                    Err(e) => {
                        self.next_check = Some(Instant::now() + CHECK_RETRY);
                        if forced {
                            self.notice = Some(Notice::new(format!("Looking for a new version failed: {e}"), false));
                        }
                    }
                }
            }
        }
        let wanted = self.checks && cfg.updates != GitUpdates::Off && !crate::update::env_off(std::env::var(crate::update::ENV).ok().as_deref());
        let due = self.next_check.unwrap_or(self.started + CHECK_DELAY) <= Instant::now();
        if wanted && due && !self.running() && self.checking.is_none() {
            self.begin_check(ctx, false);
        }
        message
    }
}

/// The notice for what an install or update ended with.
fn done_notice(r: Result<install::Done, String>, what: &str) -> Notice {
    match r {
        Ok(d) => Notice::new(format!("{what} is done: {}.", d.installed.versions_text()), true),
        Err(e) if e == "cancelled" => Notice::new(format!("{what} was cancelled; nothing of it is left."), false),
        Err(e) => Notice::new(format!("{what} failed: {e}"), false),
    }
}

/// The sentence with the sizes that comes before anything is downloaded (SYNC-025).
pub fn consent_text(plan: &Plan, folder: &str) -> String {
    format!(
        "Shale will download {} ({}) from github.com, about {}, and install it into {folder}, which takes about {} on disk. Nothing is downloaded before you press the button, and nothing outside that folder is changed.",
        plan.what(),
        plan.versions_text(),
        human_bytes(plan.download_bytes()),
        human_bytes(plan.disk_bytes())
    )
}

fn origin_text(o: Origin) -> &'static str {
    match o {
        Origin::System => "this computer's",
        Origin::Managed => "Shale's",
    }
}

/// The page. Returns whether a setting changed.
pub fn show(ui: &mut Ui, st: &mut ManagedState, cfg: &mut GitConfig) -> bool {
    let ctx = ui.ctx().clone();
    let mut changed = false;
    if (st.detection.is_none() || st.detected_for != cfg.choice) && st.detecting.is_none() && st.task.is_none() {
        st.check_again(&ctx, cfg.choice);
    }
    match (&st.detection, st.detecting.is_some()) {
        (_, true) | (None, _) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for git…");
            });
        }
        (Some(d), false) => detection_lines(ui, d),
    }
    ui.label("Git to use");
    for c in GitChoice::ALL {
        if ui.radio_value(&mut cfg.choice, c, c.label()).changed() {
            changed = true;
        }
    }
    ui.horizontal(|ui| {
        if ui.add_enabled(st.detecting.is_none() && st.task.is_none(), egui::Button::new("Check again")).on_hover_text("Look for git again").clicked() {
            st.check_again(&ctx, cfg.choice);
        }
    });
    theme::section(ui, "Shale's own git");
    managed_section(ui, st, cfg, &ctx, &mut changed);
    if let Some(n) = &st.notice {
        if n.ok {
            ui.label(&n.text);
        } else {
            theme::problem(ui, &n.text);
        }
    }
    changed
}

/// What was found, in lines (SYNC-023).
fn detection_lines(ui: &mut Ui, d: &Detection) {
    let line = |ui: &mut Ui, what: &str, v: &Option<String>| match v {
        Some(v) => {
            ui.label(format!("{what} of this computer: {v}"));
        }
        None => {
            ui.label(format!("{what} of this computer: not found"));
        }
    };
    line(ui, "git", &d.system_git);
    line(ui, "git-lfs", &d.system_lfs);
    match &d.managed {
        Some(m) => ui.label(format!("Shale's own git: {} ({})", m.installed.versions_text(), human_bytes(m.bytes))),
        None => ui.label("Shale's own git: not installed"),
    };
    match (&d.git_in_use, &d.lfs_in_use) {
        (None, _) => {
            theme::problem(ui, "No git can be used.");
        }
        (Some((g, go)), lfs) => {
            let lfs_text = lfs.as_ref().map_or_else(|| "no git-lfs".to_string(), |(l, lo)| format!("{l} ({})", origin_text(*lo)));
            ui.label(format!("In use: {g} ({}), {lfs_text}", origin_text(*go)));
        }
    }
}

/// Install, update and remove of the managed git.
fn managed_section(ui: &mut Ui, st: &mut ManagedState, cfg: &mut GitConfig, ctx: &Context, changed: &mut bool) {
    let Some(data) = st.data.clone() else {
        theme::paragraph(ui, "This computer has no data folder to keep a git in.");
        return;
    };
    let folder = managed::managed_root(&data);
    if let Some(t) = &st.task {
        match t.kind {
            TaskKind::Install | TaskKind::Update => {
                let p = t.handle.progress();
                ui.add(egui::ProgressBar::new(p.fraction).text(p.text).desired_width(420.0));
                let label = if t.kind == TaskKind::Install { "Cancel install" } else { "Cancel update" };
                if ui.button(label).on_hover_text("Stop; what was downloaded is removed").clicked() {
                    t.handle.cancel();
                }
            }
            TaskKind::Remove => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Removing Shale's git…");
                });
            }
        }
        return;
    }
    let Some(d) = st.detection.clone() else { return };
    let there = folder.exists();
    if !there {
        let have = Have { git: d.system_git.is_some(), lfs: d.system_lfs.is_some() };
        offer_ui(ui, st, ctx, st.sources.offer(&st.platform, have, st.os_release.as_deref()), &folder.to_string_lossy());
        return;
    }
    let size = d.managed.as_ref().map(|m| human_bytes(m.bytes)).unwrap_or_else(|| "?".into());
    match &d.managed {
        Some(m) => ui.label(format!("{} ({}, {size})", folder.display(), m.installed.versions_text())),
        None => ui.label(format!("{} holds no working git ({size})", folder.display())),
    };
    if st.confirm_remove {
        ui.group(|ui| {
            theme::paragraph(ui, format!("This deletes the folder {} and everything in it ({size}). Nothing else is removed. You can install it again later.", folder.display()));
            ui.horizontal(|ui| {
                if ui.button("Remove Shale's git folder").clicked() {
                    st.confirm_remove = false;
                    st.start(ctx, TaskKind::Remove, move |_, _| match install::remove(&data) {
                        Ok(()) => Notice::new("Shale's git was removed.", true),
                        Err(e) => Notice::new(e, false),
                    });
                }
                if ui.button("Keep it").clicked() {
                    st.confirm_remove = false;
                }
            });
        });
        return;
    }
    if let Some(plan) = st.update.clone() {
        theme::paragraph(ui, format!("A newer version is out: {} (about {} to download).", plan.versions_text(), human_bytes(plan.download_bytes())));
        if ui.button("Update").on_hover_text("Download the new version and replace Shale's git; the old one stays until the new one runs").clicked() {
            st.begin_update(ctx);
        }
    }
    ui.horizontal(|ui| {
        if d.managed.is_some() && ui.add_enabled(st.checking.is_none(), egui::Button::new("Look for a new version")).on_hover_text("Ask GitHub for the newest release now").clicked() {
            st.begin_check(ctx, true);
        }
        if ui.button("Remove…").on_hover_text("Delete Shale's git folder").clicked() {
            st.confirm_remove = true;
        }
    });
    if d.managed.is_some() {
        ui.label("New versions");
        for u in GitUpdates::ALL {
            if ui.radio_value(&mut cfg.updates, u, u.label()).changed() {
                *changed = true;
            }
        }
        theme::hint(ui, "Off sends no request. A check asks GitHub for the newest release of git-for-windows or git-lfs 30 seconds after Shale starts and then weekly; its files are checked against the SHA-256 GitHub lists.");
    }
}

/// What to offer while there is no managed folder (SYNC-025, SYNC-029).
fn offer_ui(ui: &mut Ui, st: &mut ManagedState, ctx: &Context, offer: Offer, folder: &str) {
    if let Some(why) = &offer.unsupported {
        theme::hint(ui, why);
    }
    if let Some(help) = &offer.help {
        match help {
            GitHelp::Xcode => {
                theme::paragraph(ui, "There is no git on this Mac. Apple's command line tools bring one: the button opens Apple's installer (a system dialog, about 1 to 2 GB); press Check again when it is done.");
                let label = if st.xcode_started { "Apple's installer was started" } else { "Install git (Apple's command line tools)" };
                if ui.add_enabled(!st.xcode_started, egui::Button::new(label)).clicked() {
                    st.xcode_started = true;
                    st.notice = Some(match install::start_xcode_select() {
                        Ok(()) => Notice::new("Apple's installer was opened. Follow its dialog, then press Check again.", true),
                        Err(e) => Notice::new(format!("Apple's installer could not be started: {e}"), false),
                    });
                }
            }
            GitHelp::Package(p) => {
                theme::paragraph(ui, format!("There is no git on this computer. On {} install it with your package manager, then press Check again:", p.distro));
                ui.add(egui::Label::new(egui::RichText::new(&p.command).monospace()).selectable(true));
                if ui.button("Copy command").clicked() {
                    ui.ctx().copy_text(p.command.clone());
                }
            }
            GitHelp::Unknown => {
                theme::paragraph(ui, "There is no git on this computer. Install the packages git and git-lfs with your package manager, then press Check again.");
            }
        }
    }
    if let Some(plan) = offer.download {
        theme::paragraph(ui, consent_text(&plan, folder));
        let label = format!("Download and install {}", if plan.what() == "git-lfs" { "git-lfs" } else { "git" });
        if ui.button(label).clicked() {
            st.begin_install(ctx, plan);
        }
    }
}

// ---------------------------------------------------------------- the window

impl ShaleApp {
    /// Select the git every real use runs again (SYNC-024): at the start, when the choice changed and when an
    /// install, update or removal ended.
    pub(crate) fn refresh_git(&mut self) {
        self.sync.git = self.sync.managed.selected(self.cfg.git.choice);
        self.sync.forget_git_version();
        self.refresh_lfs();
    }

    /// One frame of the managed git: its jobs, the due check for a new version, and the selection.
    pub(crate) fn git_tick(&mut self, ctx: &Context) {
        if let Some(m) = self.sync.managed.tick(ctx, &self.cfg.git) {
            self.toast(m);
        }
        if self.sync.managed.take_reselect(self.cfg.git.choice) && self.sync.managed.select {
            self.refresh_git();
        }
    }

    /// The git part of the Sync tab. Returns whether a setting changed.
    pub(crate) fn git_settings_ui(&mut self, ui: &mut Ui) -> bool {
        show(ui, &mut self.sync.managed, &mut self.cfg.git)
    }
}
