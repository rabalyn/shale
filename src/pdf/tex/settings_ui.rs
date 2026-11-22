//! Settings → LaTeX (TEX-001 to TEX-003): which TeX is used, the managed TeX Live (install after
//! consent, update, remove), a test compile, and the template sets of the vault (`sets_ui`). The
//! state is one small struct of the window; every slow thing (finding a TeX, the install, the
//! test, an import) is a background job.

use super::compile::compile_test_document;
use super::engine::{self, Detection, Engine, Kind, TexChoice, TexConfig};
use super::install::{self, Env, Handle, DISK_MB, DOWNLOAD_MB};
use crate::theme;
use eframe::egui::{self, Context, Ui};
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::time::Duration;

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
    Remove,
    Test,
    /// Importing or updating a template set (TEX-006).
    Import,
    /// Compiling the test page of a template set (TEX-024).
    TestPage,
}

/// A running install, update, removal or test.
pub struct Task {
    pub kind: TaskKind,
    pub handle: Handle,
    pub(super) rx: Receiver<Result<Notice, String>>,
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
    pub(super) detecting: Option<Receiver<Result<Detection, String>>>,
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
        TexState { data, path_var, detection: None, detecting: None, task: None, consent: false, confirm_remove: false, notice: None, sets: Default::default(), setup: Default::default(), setup_requested: false }
    }

    /// Take what the jobs have finished; call it every frame.
    pub fn poll(&mut self, ctx: &Context) {
        if let Some(rx) = &self.detecting {
            if let Ok(r) = rx.try_recv() {
                self.detecting = None;
                match r {
                    Ok(d) => self.detection = Some(d),
                    Err(e) => self.notice = Some(Notice::new(format!("Looking for TeX failed: {e}"), false)),
                }
            }
        }
        if let Some(t) = &self.task {
            if let Ok(r) = t.rx.try_recv() {
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

    /// Look for a TeX again, in a job.
    pub fn check_again(&mut self, ctx: &Context) {
        if self.detecting.is_some() {
            return;
        }
        self.detection = None;
        let (data, path) = (self.data.clone(), self.path_var.clone());
        let ctx = ctx.clone();
        self.detecting = Some(crate::jobs::spawn(move || engine::detect(data.as_deref(), path.as_deref()), move || ctx.request_repaint()));
    }

    /// Start a job of the page; its answer is a notice.
    pub(super) fn start(&mut self, ctx: &Context, kind: TaskKind, work: impl FnOnce(&Handle, &dyn Fn()) -> Notice + Send + 'static) {
        if self.task.is_some() {
            return;
        }
        let handle = Handle::default();
        let (h, c1, c2) = (handle.clone(), ctx.clone(), ctx.clone());
        let rx = crate::jobs::spawn(move || work(&h, &|| c1.request_repaint()), move || c2.request_repaint());
        self.task = Some(Task { kind, handle, rx });
        self.notice = None;
    }

    fn env(&self, cfg: &TexConfig) -> Option<Env> {
        Some(Env { data: self.data.clone()?, repository: cfg.repository.clone() })
    }

    /// Start installing the managed TeX Live (TEX-002) once the person has agreed: Settings and
    /// the guided setup show the sizes and the folder first and call this from their button.
    pub(super) fn begin_install(&mut self, ctx: &Context, cfg: &TexConfig) {
        if let Some(env) = self.env(cfg) {
            self.start(ctx, TaskKind::Install, move |h, n| done_notice(install::install(&env, h, n), "The install"));
        }
    }
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
    match (&st.detection, st.detecting.is_some()) {
        (_, true) | (None, _) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for a TeX…");
            });
        }
        (Some(d), false) => match d.selected(cfg.engine) {
            Ok(e) => {
                let size = if e.kind == Kind::Managed { d.managed_bytes.map(|b| format!(", {} on disk", crate::data_usage::human_bytes(b))).unwrap_or_default() } else { String::new() };
                ui.label(format!("Using {} (LaTeX kernel {}){size}", engine_name(e), e.kernel));
                crate::theme::hint(ui, e.bin_dir.to_string_lossy());
                if !e.missing.is_empty() {
                    crate::theme::problem(ui, format!("It misses these packages: {}. Use the TeX Live that Shale installs.", e.missing.join(", ")));
                }
            }
            Err(why) => {
                crate::theme::problem(ui, format!("No TeX can be used: {why}"));
            }
        },
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
            if ui.radio_value(&mut cfg.engine, c, c.label()).changed() {
                changed = true;
            }
        }
    });
    crate::settings::anchor::anchor(ui, "tex.test", |ui| {
        ui.horizontal(|ui| {
            if ui.add_enabled(st.detecting.is_none() && st.task.is_none(), egui::Button::new("Check again")).on_hover_text("Look for a TeX again").clicked() {
                st.check_again(&ctx);
            }
            let engine = st.detection.as_ref().and_then(|d| d.selected(cfg.engine).ok()).cloned();
            let can_test = engine.is_some() && st.task.is_none();
            if ui.add_enabled(can_test, egui::Button::new("Test")).on_hover_text("Compile a small document with the chosen TeX: fonts, packages and the bibliography").clicked() {
                if let Some(e) = engine {
                    st.start(&ctx, TaskKind::Test, move |_, _| test_notice(&e));
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
        egui::CollapsingHeader::new("Download address").id_salt("tex-repository").show(ui, |ui| {
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
            egui::CollapsingHeader::new("The end of the log").id_salt("tex-log").show(ui, |ui| {
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
    // a job runs: its progress and Cancel
    if let Some(t) = &st.task {
        let p = t.handle.progress();
        match t.kind {
            TaskKind::Install | TaskKind::Update => {
                ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
                let label = if t.kind == TaskKind::Install { "Cancel install" } else { "Cancel update" };
                if ui.button(label).on_hover_text("Stop; what was downloaded for an install is removed").clicked() {
                    t.handle.cancel();
                }
            }
            TaskKind::Remove => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Removing the TeX Live…");
                });
            }
            TaskKind::Test => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Compiling the test document…");
                });
            }
            // shown with the template sets
            TaskKind::Import | TaskKind::TestPage => {}
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
                    st.start(ctx, TaskKind::Remove, move |_, _| match install::remove(&data) {
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
        if ui.button("Update").on_hover_text("Update TeX Live and its packages (tlmgr update --self --all)").clicked() {
            if let Some(env) = st.env(cfg) {
                st.start(ctx, TaskKind::Update, move |h, n| done_notice(install::update(&env, h, n), "The update"));
            }
        }
        if ui.button("Remove…").on_hover_text("Delete the managed TeX Live").clicked() {
            st.confirm_remove = true;
        }
    });
}
