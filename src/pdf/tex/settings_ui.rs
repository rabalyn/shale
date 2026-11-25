//! Settings → LaTeX (TEX-001 to TEX-003): which TeX is used, the managed TeX Live (install after
//! consent, update, remove), a test compile, and the template sets of the vault (`sets_ui`). The
//! state is one small struct of the window; every slow thing (finding a TeX, the install, the
//! test, an import) is a background job.

use super::compile::compile_test_document;
use super::engine::{self, Detection, Engine, Kind, TexChoice, TexConfig};
use super::install::{self, Env, Handle, DISK_MB, DOWNLOAD_MB};
use crate::busy::{self, Key};
use crate::clickable::CollapsingPointer;
use crate::theme;
use eframe::egui::{self, Context, Ui};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

/// The task of the look for TeX (TEX-001, JOB-023). It cannot be stopped: it runs a few programs
/// with `--version`.
pub const TEX_PROBE: crate::busy::Key = crate::busy::Key::new("tex-probe");

/// What the look for TeX says it does.
pub const TEX_PROBE_TEXT: &str = "Looking for TeX on this computer…";

/// The install, update and removal of the managed TeX Live, the test compile, the import of a
/// template set and the test page of a set (JOB-029): one at a time. An install, an update and an
/// import can be stopped (what they did is taken back); the others cannot.
pub(crate) const TEX_JOB: Key = Key::new("tex-job");

/// What the person is told after a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub text: String,
    pub ok: bool,
    /// The end of a log, shown on request.
    pub details: Option<String>,
    /// A PDF to open when the job is taken by the window (file name, bytes): the opener runs on
    /// the window's thread, not in the job (the test page of a template set, TEX-024).
    pub open: Option<(String, Vec<u8>)>,
}

impl Notice {
    pub(super) fn new(text: impl Into<String>, ok: bool) -> Notice {
        Notice { text: text.into(), ok, details: None, open: None }
    }
}

/// What a running job does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Install,
    Update,
    /// Installing the packages of the list that the managed TeX Live lacks (TEX-052).
    Packages,
    Remove,
    Test,
    /// Importing or updating a template set (TEX-006).
    Import,
    /// Compiling the test page of a template set (TEX-024).
    TestPage,
}

impl TaskKind {
    /// Cancel can stop the job: it looks at the flag of its handle between two steps.
    fn can_stop(self) -> bool {
        matches!(self, TaskKind::Install | TaskKind::Update | TaskKind::Packages | TaskKind::Import)
    }
}

/// A running install, update, removal or test: its progress (the task of the registry has the text and Cancel).
pub struct Task {
    pub kind: TaskKind,
    pub handle: Handle,
    pub(crate) job: busy::Job<Notice>,
}

/// The state of the LaTeX page of the settings.
pub struct TexState {
    /// The user's data folder, where the managed TeX Live lives; none: there is no such folder.
    pub data: Option<PathBuf>,
    /// `PATH`, where the TeX of the computer is looked for.
    path_var: Option<OsString>,
    /// What detection found, until something changes it.
    pub detection: Option<Detection>,
    /// The look for TeX that runs (a job), if any.
    pub(super) detecting: Option<(Receiver<Result<Detection, String>>, crate::busy::Task)>,
    /// the registry that lists the look for TeX (the window's, set by `ShaleApp`)
    pub busy: crate::busy::Busy,
    pub task: Option<Task>,
    /// The person pressed Install and has not agreed yet.
    pub consent: bool,
    /// The person pressed Remove and has not confirmed yet.
    pub confirm_remove: bool,
    /// What the last job said.
    pub notice: Option<Notice>,
    /// The template sets of the open vault.
    pub sets: super::sets_ui::SetsState,
    /// The guided setup (TEX-017).
    pub setup: super::setup::TexSetup,
    /// "Set up LaTeX export…" was pressed: the window opens the guided setup.
    pub setup_requested: bool,
}

impl TexState {
    /// `persist`: the real data folder and `PATH`; otherwise (tests and screenshots) a temporary
    /// data folder named after `tag` and no `PATH`, so nothing of the machine is touched or found.
    pub fn new(persist: bool, tag: &str) -> TexState {
        let (data, path_var) = if persist {
            (dirs::data_dir(), std::env::var_os("PATH"))
        } else {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            // screenshots name a data folder to show (SHALE_FAKE_DATA_DIR, like SHALE_FAKE_NOW); nothing is written there
            let fake = std::env::var_os("SHALE_FAKE_DATA_DIR").map(PathBuf::from);
            (Some(fake.unwrap_or_else(|| std::env::temp_dir().join(format!("shale-texdata-{tag}-{}", NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst))))), None)
        };
        TexState { data, path_var, detection: None, detecting: None, busy: Default::default(), task: None, consent: false, confirm_remove: false, notice: None, sets: Default::default(), setup: Default::default(), setup_requested: false }
    }

    /// Take what the jobs have finished; call it every frame.
    pub fn poll(&mut self, ctx: &Context) {
        if let Some((rx, _)) = &self.detecting {
            if let Ok(r) = rx.try_recv() {
                self.detecting = None;
                match r {
                    Ok(d) => self.detection = Some(d),
                    Err(e) => self.notice = Some(Notice::new(format!("Looking for TeX failed: {e}"), false)),
                }
            }
        }
        if let Some(t) = &self.task {
            if let Ok(r) = t.job.try_recv() {
                let kind = t.kind;
                self.task = None;
                let mut notice = r.unwrap_or_else(|e| Notice::new(e, false));
                if let Some((name, pdf)) = notice.open.take() {
                    match crate::opener::open_temp_copy(&name, &pdf) {
                        Ok(()) => notice.text.push_str(" It was opened in your PDF viewer."),
                        Err(e) => {
                            notice.ok = false;
                            notice.text.push_str(&format!(" It could not be opened: {e}"));
                        }
                    }
                }
                self.notice = Some(notice);
                if matches!(kind, TaskKind::Import | TaskKind::TestPage) {
                    self.sets.job_ended(kind);
                } else {
                    // the install, the update and the removal change what is there
                    self.detection = None;
                }
            }
        }
        if self.task.is_some() || self.detecting.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// The dialog that started the work was closed (JOB-010): ask the install, the update and the
    /// import to stop; they end at the next safe point and take back what they did. A removal and
    /// the compiles finish first.
    pub(crate) fn stop_jobs(&self) {
        if let Some(t) = &self.task {
            t.job.cancel();
        }
    }

    /// Look for a TeX again, in a job.
    pub fn check_again(&mut self, ctx: &Context) {
        if self.detecting.is_some() {
            return;
        }
        self.detection = None;
        let (data, path) = (self.data.clone(), self.path_var.clone());
        let ctx = ctx.clone();
        let Some(task) = self.busy.begin(TEX_PROBE, TEX_PROBE_TEXT) else { return };
        self.detecting = Some((crate::jobs::spawn(move || engine::detect(data.as_deref(), path.as_deref()), move || ctx.request_repaint()), task));
    }

    /// Start a job of the page; its answer is a notice.
    pub(super) fn start(&mut self, ctx: &Context, kind: TaskKind, label: impl Into<String>, work: impl FnOnce(&Handle, &dyn Fn()) -> Notice + Send + 'static) {
        self.start_with(ctx, kind, label, Handle::default(), work);
    }

    /// The same with a handle that exists already.
    fn start_with(&mut self, ctx: &Context, kind: TaskKind, label: impl Into<String>, handle: Handle, work: impl FnOnce(&Handle, &dyn Fn()) -> Notice + Send + 'static) {
        if self.task.is_some() {
            return;
        }
        let Some(mut task) = self.busy.begin(TEX_JOB, label) else { return };
        if kind.can_stop() {
            task.cancellable_with(handle.token());
        }
        let (h, c1, c2) = (handle.clone(), ctx.clone(), ctx.clone());
        let job = task.run(move || work(&h, &|| c1.request_repaint()), move || c2.request_repaint());
        self.task = Some(Task { kind, handle, job });
        self.notice = None;
    }

    /// Tests: a job of this kind that runs until `rx` sends its notice (or is dropped), with a
    /// handle the test made, standing in for an install, an update or an import.
    #[cfg(test)]
    pub(crate) fn inject(&mut self, kind: TaskKind, handle: Handle, rx: std::sync::mpsc::Receiver<Result<Notice, String>>) {
        let label = match kind {
            TaskKind::Install => "Installing TeX Live…",
            TaskKind::Update => "Updating TeX Live…",
            TaskKind::Packages => "Installing the missing TeX Live packages…",
            TaskKind::Remove => "Removing the TeX Live…",
            TaskKind::Test => "Compiling the test document…",
            TaskKind::Import => "Importing the template set…",
            TaskKind::TestPage => "Compiling the test page…",
        };
        self.start_with(&Context::default(), kind, label, handle, move |_, _| loop {
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok(r) => return r.unwrap_or_else(|e| Notice::new(e, false)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Notice::new("The job of the test ended.", true),
            }
        });
    }

    fn env(&self, cfg: &TexConfig) -> Option<Env> {
        Some(Env { data: self.data.clone()?, repository: cfg.repository.clone() })
    }

    /// Start installing the managed TeX Live (TEX-002) once the person has agreed: Settings and
    /// the guided setup show the sizes and the folder first and call this from their button.
    pub(super) fn begin_install(&mut self, ctx: &Context, cfg: &TexConfig) {
        if let Some(env) = self.env(cfg) {
            self.start(ctx, TaskKind::Install, "Installing TeX Live…", move |h, n| done_notice(install::install(&env, h, n), "The install"));
        }
    }

    /// Start installing the packages of the list that the managed TeX Live lacks (TEX-052), once the
    /// person pressed Install: exactly `names`, as a job with progress and Cancel. The look for TeX
    /// is done again when it ends, so the line that offered it goes away.
    pub fn begin_packages(&mut self, ctx: &Context, cfg: &TexConfig, names: Vec<String>) {
        if let Some(env) = self.env(cfg) {
            self.start(ctx, TaskKind::Packages, "Installing the missing TeX Live packages…", move |h, n| done_notice(install::install_missing(&env, &names, h, n), "The install of the packages"));
        }
    }
}

/// "The managed TeX Live misses: geometry", the line with the Install button (TEX-052).
pub fn missing_line(missing: &[String]) -> String {
    format!("The managed TeX Live misses: {}", missing.join(", "))
}

/// The notice for what an install or update ended with.
pub(super) fn done_notice(r: Result<install::Done, String>, what: &str) -> Notice {
    match r {
        Ok(d) => match d.warning {
            None => Notice::new(if d.mirrors.is_empty() { format!("{what} is done.") } else { format!("{what} is done, {}.", d.source_text()) }, true),
            Some(w) => Notice::new(w, false),
        },
        Err(e) if e == "cancelled" => Notice::new(format!("{what} was cancelled; nothing of it is left."), false),
        Err(e) => Notice::new(format!("{what} failed: {e}"), false),
    }
}

/// The sentence with the sizes that comes before anything is downloaded (TEX-002).
pub fn consent_text(repository: &str, folder: &str) -> String {
    format!("Shale will download about {DOWNLOAD_MB} MB from {repository} and install TeX Live into {folder}, which takes about {DISK_MB} MB on disk. Nothing is downloaded before you agree, and nothing outside that folder is changed.")
}

/// What the test compile says.
fn test_notice(engine: &Engine) -> Notice {
    match compile_test_document(engine) {
        Ok(o) => Notice::new(
            format!(
                "The test document compiled to a PDF of {} KB in {} run{}{}.",
                o.pdf.len().div_ceil(1024),
                o.runs,
                if o.runs == 1 { "" } else { "s" },
                if o.font_cache_built { " (the font cache was built, so the first run was slow)" } else { "" }
            ),
            true,
        ),
        Err(e) => Notice { details: Some(e.log_tail.clone()), ..Notice::new(format!("The test document did not compile: {e}"), false) },
    }
}

pub fn engine_name(e: &Engine) -> &'static str {
    match e.kind {
        Kind::Managed => "the TeX Live that Shale installs",
        Kind::System => "the TeX of this computer",
    }
}

/// The page. Returns whether a setting changed.
pub fn show(ui: &mut Ui, st: &mut TexState, cfg: &mut TexConfig) -> bool {
    let ctx = ui.ctx().clone();
    let mut changed = false;
    if st.detection.is_none() && st.detecting.is_none() && st.task.is_none() {
        st.check_again(&ctx);
    }
    crate::theme::hint(ui, "Notes can be exported to PDF through LaTeX templates. That needs LuaLaTeX and biber: Shale uses the ones of this computer when they are new enough, or installs its own TeX Live.");
    crate::settings::anchor::anchor(ui, "tex.setup", |ui| {
        ui.horizontal(|ui| {
            if ui.button("Set up LaTeX export…").on_hover_text("A guided setup: TeX, the templates, the logo and your details, step by step").clicked() {
                st.setup_requested = true;
            }
            crate::theme::hint(ui, "Not firm in TeX? This takes you through it.");
        });
    });

    theme::section(ui, "Which TeX is used");
    let mut install_packages = None;
    match (&st.detection, st.detecting.is_some()) {
        (_, true) | (None, _) => {
            crate::busy::ui::status(ui, &st.busy, &TEX_PROBE);
        }
        (Some(d), false) => match d.selected(cfg.engine) {
            Ok(e) => {
                let size = if e.kind == Kind::Managed { d.managed_bytes.map(|b| format!(", {} on disk", crate::data_usage::human_bytes(b))).unwrap_or_default() } else { String::new() };
                ui.label(format!("Using {} (LaTeX kernel {}){size}", engine_name(e), e.kernel));
                crate::theme::hint(ui, e.bin_dir.to_string_lossy());
                if !e.missing.is_empty() && e.kind == Kind::Managed {
                    // the packages of the list that came after its install (TEX-052)
                    ui.horizontal_wrapped(|ui| {
                        crate::theme::problem(ui, missing_line(&e.missing));
                        if ui.add_enabled(st.task.is_none(), egui::Button::new("Install")).on_hover_text(format!("tlmgr install {}", e.missing.join(" "))).clicked() {
                            install_packages = Some(e.missing.clone());
                        }
                    });
                } else if !e.missing.is_empty() {
                    crate::theme::problem(ui, format!("It misses these packages: {}. Use the TeX Live that Shale installs.", e.missing.join(", ")));
                }
            }
            Err(why) => {
                crate::theme::problem(ui, format!("No TeX can be used: {why}"));
            }
        },
    }
    if let Some(names) = install_packages {
        st.begin_packages(&ctx, cfg, names);
    }
    if let Some(d) = &st.detection {
        if cfg.engine == TexChoice::Auto {
            let reason = d.system_reason();
            if d.managed.is_ok() && !reason.is_empty() {
                crate::theme::hint(ui, reason);
            }
        }
    }

    crate::settings::anchor::anchor(ui, "tex.engine", |ui| {
        ui.label("TeX to use");
        for c in TexChoice::ALL {
            if crate::clickable::pointer(ui.radio_value(&mut cfg.engine, c, c.label())).changed() {
                changed = true;
            }
        }
    });
    crate::settings::anchor::anchor(ui, "tex.test", |ui| {
        ui.horizontal(|ui| {
            let again = ui.add_enabled_ui(st.task.is_none(), |ui| crate::busy::ui::button(ui, &st.busy, &[&TEX_PROBE], egui::Button::new("Check again"))).inner;
            if again.on_hover_text("Look for a TeX again").clicked() {
                st.check_again(&ctx);
            }
            let engine = st.detection.as_ref().and_then(|d| d.selected(cfg.engine).ok()).cloned();
            let can_test = engine.is_some();
            let test = ui.add_enabled_ui(can_test, |ui| busy::ui::button(ui, &st.busy, &[&TEX_JOB], egui::Button::new("Test")).on_hover_text("Compile a small document with the chosen TeX: fonts, packages and the bibliography")).inner;
            if test.clicked() {
                if let Some(e) = engine {
                    st.start(&ctx, TaskKind::Test, "Compiling the test document…", move |_, _| test_notice(&e));
                }
            }
        });
    });

    theme::section(ui, "Shale's own TeX Live");
    crate::settings::anchor::anchor(ui, "tex.managed", |ui| managed_section(ui, st, cfg, &ctx));

    // (the heading of the sets is inside; a scope has no gap above its first line)
    theme::section_break(ui);
    crate::settings::anchor::anchor(ui, "tex.sets", |ui| super::sets_ui::show(ui, st, cfg));

    crate::settings::anchor::anchor(ui, "tex.repository", |ui| {
        egui::CollapsingHeader::new("Download address").id_salt("tex-repository").show_pointer(ui, |ui| {
            crate::theme::hint(ui, "The TeX Live repository the install and the update download from.");
            if ui.add(egui::TextEdit::singleline(&mut cfg.repository).desired_width(360.0)).changed() {
                changed = true;
            }
            if let Err(e) = install::check_repository(&cfg.repository) {
                crate::theme::problem(ui, e);
            }
            if ui.button("Use the default").clicked() && cfg.repository != engine::DEFAULT_REPOSITORY {
                cfg.repository = engine::DEFAULT_REPOSITORY.to_string();
                changed = true;
            }
        });
    });

    if let Some(n) = &st.notice {
        if n.ok {
            ui.label(&n.text);
        } else {
            crate::theme::problem(ui, &n.text);
        }
        if let Some(d) = &n.details {
            egui::CollapsingHeader::new("The end of the log").id_salt("tex-log").show_pointer(ui, |ui| {
                ui.add(egui::Label::new(egui::RichText::new(d).monospace().size(11.0)).wrap());
            });
        }
    }
    changed
}

/// Install, update and remove of the managed TeX Live.
fn managed_section(ui: &mut Ui, st: &mut TexState, cfg: &TexConfig, ctx: &Context) {
    let Some(data) = st.data.clone() else {
        crate::theme::paragraph(ui, "This computer has no data folder to keep a TeX Live in.");
        return;
    };
    let folder = engine::managed_root(&data);
    // a job runs: its progress, what it does and Cancel where it can stop
    if let Some(t) = &st.task {
        // an import and a test page are shown with the template sets
        if !matches!(t.kind, TaskKind::Import | TaskKind::TestPage) {
            if matches!(t.kind, TaskKind::Install | TaskKind::Update | TaskKind::Packages) {
                let p = t.handle.progress();
                ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
            }
            busy::ui::status(ui, &st.busy, &TEX_JOB);
            crate::theme::hint(ui, "Closing this window stops it at the next safe point; a removal finishes first.");
        }
        return;
    }
    let installed = st.detection.as_ref().is_some_and(|d| d.managed.is_ok());
    let there = folder.exists();
    if !installed && !there {
        if st.detection.is_none() {
            return;
        }
        if st.consent {
            ui.group(|ui| {
                crate::theme::paragraph(ui, consent_text(cfg.repository.trim_end_matches('/'), &folder.to_string_lossy()));
                ui.horizontal(|ui| {
                    if ui.button("Download and install").clicked() {
                        st.consent = false;
                        st.begin_install(ctx, cfg);
                    }
                    if ui.button("Not now").clicked() {
                        st.consent = false;
                    }
                });
            });
        } else if ui.button("Install TeX Live…").on_hover_text(format!("About {DOWNLOAD_MB} MB to download, {DISK_MB} MB on disk")).clicked() {
            st.consent = true;
        }
        return;
    }
    // installed
    let size = st.detection.as_ref().and_then(|d| d.managed_bytes).map(crate::data_usage::human_bytes).unwrap_or_else(|| "?".into());
    ui.label(format!("{} ({size})", folder.display()));
    if st.confirm_remove {
        ui.group(|ui| {
            crate::theme::paragraph(ui, format!("This deletes the folder {} and everything in it ({size}). Nothing else is removed. You can install it again later.", folder.display()));
            ui.horizontal(|ui| {
                if ui.button("Remove the TeX Live folder").clicked() {
                    st.confirm_remove = false;
                    st.start(ctx, TaskKind::Remove, "Removing the TeX Live…", move |_, _| match install::remove(&data) {
                        Ok(()) => Notice::new("The TeX Live was removed.", true),
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
    ui.horizontal(|ui| {
        if ui.button("Update").on_hover_text("Update TeX Live and its packages (tlmgr update --self --all), and install the packages of Shale's list that it lacks").clicked() {
            if let Some(env) = st.env(cfg) {
                st.start(ctx, TaskKind::Update, "Updating TeX Live…", move |h, n| done_notice(install::update(&env, h, n), "The update"));
            }
        }
        if ui.button("Remove…").on_hover_text("Delete the managed TeX Live").clicked() {
            st.confirm_remove = true;
        }
    });
}
