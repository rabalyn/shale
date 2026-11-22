//! The wizard's dialog and its glue with the window (SYNC-047…SYNC-058): what each step shows,
//! what a click asks for, and the frame-by-frame collection of the jobs. The state is
//! `app.sync.setup.wizard`; the dialog takes it out while it draws, as the history dialog does.

use super::provider::{self, Kind, PRESETS};
use super::test_sync;
use super::{api, ssh, Choice, Method, Msg, Step, Test, Wizard};
use crate::app::{Modal, ShaleApp};
use crate::gitsync::detect::{self, Finding};
use crate::gitsync::install_ui::{self, ManagedState};
use crate::gitsync::managed::GitConfig;
use crate::gitsync::{self, moveout};
use crate::theme;
use eframe::egui::{self, Ui};
use std::path::PathBuf;
use std::time::Duration;

/// What a click in the dialog asked for.
enum Ask {
    Cancel,
    Finish,
    ChooseNext,
    ServerNext,
    SignInNext,
    CheckToken,
    MakeKey,
    CreateRepo,
    RepoNext,
    RunTest,
    Open(String),
    Copy(String),
    MoveOut,
}

/// What the dialog shows besides the wizard's own state.
struct Cx {
    git_ok: bool,
    findings: Vec<Finding>,
    has_dot_git: bool,
    has_remote: bool,
    moving: bool,
    moved: Option<(String, bool)>,
}

impl ShaleApp {
    /// Open the wizard on the open vault (SYNC-047).
    pub(crate) fn open_sync_wizard(&mut self) {
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        let ctx = self.ctx.clone().unwrap_or_default();
        self.sync.setup.detect_for(&root, &ctx);
        self.sync.setup.wizard = Some(Wizard::default());
        self.modal = Some(Modal::SyncWizard);
    }

    /// Draw the dialog and do what was clicked; false: close it.
    pub(crate) fn sync_wizard_dialog(&mut self, ctx: &egui::Context) -> bool {
        let Some(mut w) = self.sync.setup.wizard.take() else { return false };
        let root = self.vault.as_ref().map(|v| v.root.clone());
        let git_ok = self.git_version().is_some();
        let cx = Cx {
            git_ok,
            findings: root.as_deref().map(|r| self.sync.setup.findings(r).to_vec()).unwrap_or_default(),
            has_dot_git: root.as_deref().is_some_and(gitsync::is_repo),
            has_remote: self.sync.remote.is_some(),
            moving: self.sync.setup.moveout.running(),
            moved: self.sync.setup.moveout.result.clone(),
        };
        let mut out: (Option<Ask>, bool) = (None, false);
        let mut state = (&mut w, &mut self.sync.managed, &mut self.cfg.git, &mut out);
        let resp = crate::dialog::Dialog::new("sync-wizard", "Turn on sync").width(580.0).show_with_footer(
            ctx,
            &mut state,
            |ui, (w, managed, gitcfg, out)| **out = body(ui, w, &cx, managed, gitcfg),
            |ui, (w, _, _, out)| {
                if out.0.is_none() {
                    out.0 = footer(ui, w, &cx);
                }
            },
        );
        let (asked, git_changed) = out;
        if git_changed {
            self.save_config();
        }
        let mut keep = !resp.should_close();
        self.sync.setup.wizard = Some(w);
        if let Some(a) = asked {
            keep &= self.wizard_do(ctx, a);
        }
        if !keep {
            self.sync.setup.wizard = None;
        }
        keep
    }

    /// Do what a click asked for; false: close the dialog.
    fn wizard_do(&mut self, ctx: &egui::Context, ask: Ask) -> bool {
        let Some(mut w) = self.sync.setup.wizard.take() else { return false };
        let mut keep = true;
        match ask {
            Ask::Cancel => keep = false,
            Ask::Open(url) => {
                if let Err(e) = crate::opener::open_url(&url) {
                    w.say(format!("Could not open the page: {e}"), false);
                }
            }
            Ask::Copy(text) => ctx.copy_text(text),
            Ask::MoveOut => self.start_moveout(),
            Ask::Finish => {
                if w.choice == Some(Choice::Server) {
                    if self.vdata.settings_writable() {
                        self.vdata.settings.sync.enabled = true;
                    }
                    self.sync.remote = self.vault.as_ref().and_then(|v| gitsync::remote_url(&self.sync.git, &v.root)).map(|u| gitsync::hide_passwords(&u));
                    self.save_config();
                    self.toast("Sync is on: Shale keeps this vault in step with the server");
                } else {
                    self.toast("Your notes keep a history on this device");
                }
                keep = false;
            }
            Ask::ChooseNext => match w.choice {
                Some(Choice::Server) => {
                    w.message = None;
                    w.step = Step::Server;
                }
                Some(c) => {
                    // the history of SYNC-030 does both; it only has to be on (SYNC-049)
                    self.cfg.history.enabled = true;
                    self.save_config();
                    w.done = Some(done_text(c));
                    w.step = Step::Done;
                }
                None => {}
            },
            Ask::ServerNext => self.wizard_server_next(&mut w),
            Ask::SignInNext => {
                if w.repo_named.is_some() {
                    self.wizard_repo_next(&mut w, ctx);
                } else {
                    w.message = None;
                    w.step = Step::Repo;
                }
            }
            Ask::CheckToken => {
                if let Some(server) = w.server.clone() {
                    let (user, token) = (w.user.clone(), w.token.clone());
                    w.message = None;
                    self.wizard_job(&mut w, "signing in", ctx, move || super::sign_in(&server, &user, &token).map(Msg::SignedIn));
                }
            }
            Ask::MakeKey => {
                if let (Some(server), Some(home)) = (w.server.clone(), self.sync.setup.home()) {
                    let git = self.sync.git.clone();
                    self.wizard_job(&mut w, "making a key", ctx, move || ssh::generate(&git, &home, &server.host, &gitsync::host_name()).map(Msg::Key));
                }
            }
            Ask::CreateRepo => {
                if let (Some(server), true) = (w.server.clone(), w.signed_in) {
                    let (token, name) = (crate::secrets::git_token(&gitsync::credentials::server_key(&server.base()).unwrap_or_default(), &w.user).unwrap_or_else(|| w.token.clone()), w.new_name.trim().to_string());
                    self.wizard_job(&mut w, "creating the repository", ctx, move || api::create(&server, &token, &name, "Notes, kept by Shale").map(Msg::Created).map_err(|e| e.message()));
                }
            }
            Ask::RepoNext => self.wizard_repo_next(&mut w, ctx),
            Ask::RunTest => {
                w.message = None;
                w.test = Test::Pending;
            }
        }
        self.sync.setup.wizard = Some(w);
        keep
    }

    /// Start a job of the wizard.
    fn wizard_job(&mut self, w: &mut Wizard, what: &'static str, ctx: &egui::Context, f: impl FnOnce() -> Result<Msg, String> + Send + 'static) {
        let ctx = ctx.clone();
        w.job = Some((what, crate::jobs::spawn(f, move || ctx.request_repaint())));
    }

    /// "Next" of the server step: read the address.
    fn wizard_server_next(&mut self, w: &mut Wizard) {
        let text = w.address.trim().to_string();
        w.message = None;
        if super::is_local_remote(&text) {
            if let Err(e) = gitsync::check_url(&text) {
                w.say(e, false);
                return;
            }
            w.local_remote = Some(text.clone());
            w.server = None;
            let ctx = self.ctx.clone().unwrap_or_default();
            self.wizard_connect(w, &ctx, text);
            return;
        }
        w.local_remote = None;
        match provider::parse(&text, w.kind) {
            Ok(p) => {
                w.repo_named = p.repo;
                w.method = if p.ssh { Method::Ssh } else { Method::Token };
                w.kind = p.server.kind;
                w.server = Some(p.server);
                w.signed_in = false;
                w.repos.clear();
                w.repo = None;
                w.keys = self.sync.setup.home().map(|h| ssh::existing(&h)).unwrap_or_default();
                w.key = (w.keys.len() == 1).then_some(0);
                w.step = Step::SignIn;
            }
            Err(e) => w.say(e, false),
        }
    }

    /// The owner and repository the person chose.
    fn wizard_repo_choice(w: &Wizard) -> Result<(String, String), String> {
        if let Some(r) = w.repo.and_then(|i| w.repos.get(i)) {
            return Ok((r.owner.clone(), r.name.clone()));
        }
        let text = w.repo_text.trim();
        if text.is_empty() {
            return w.repo_named.clone().ok_or_else(|| "Pick a repository, or enter its address".to_string());
        }
        if let Ok(p) = provider::parse(text, w.kind) {
            if let Some(r) = p.repo {
                return Ok(r);
            }
        }
        match text.split_once('/') {
            Some((o, r)) if !o.is_empty() && !r.is_empty() && !r.contains('/') => Ok((o.to_string(), r.trim_end_matches(".git").to_string())),
            _ => Err("Enter the repository as owner/name, or paste its address".into()),
        }
    }

    /// "Next" of the repository step: set `origin`, ask the server.
    fn wizard_repo_next(&mut self, w: &mut Wizard, ctx: &egui::Context) {
        let Some(server) = w.server.clone() else { return };
        match Self::wizard_repo_choice(w) {
            Ok((owner, repo)) => {
                let url = super::remote_for(&server, w.method, &w.user, &owner, &repo);
                self.wizard_connect(w, ctx, url);
            }
            Err(e) => w.say(e, false),
        }
    }

    /// `git init` where needed, set `origin`, ask the server what it holds (a job).
    fn wizard_connect(&mut self, w: &mut Wizard, ctx: &egui::Context, url: String) {
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        let git = self.sync.git.clone();
        let key = (w.method == Method::Ssh && w.local_remote.is_none()).then(|| w.chosen_key().map(|k| k.private.clone())).flatten();
        // a key of ~/.ssh that git finds by itself needs no command of its own
        let key = key.filter(|k| k.file_name().is_some_and(|n| n.to_string_lossy().starts_with("shale_")));
        if self.sync.running() || self.history.busy() || self.sync.lfs_job_running() {
            w.say("A sync or a snapshot is running; try again in a moment", false);
            return;
        }
        w.remote = Some(url.clone());
        w.message = None;
        // a local-only history moves into the new `.git` (SYNC-072): saved notes first, no snapshot beside it
        self.flush_all();
        let (tree, history_root, guard) = (self.sync.tree.clone(), self.usage.places.history.clone(), self.sync.setup.joining.begin());
        self.wizard_job(w, "asking the server", ctx, move || {
            let _guard = guard;
            super::connect_and_inspect(&git, &root, &url, key.as_deref(), &history_root, &tree).map(|(r, n, m)| Msg::Inspected(r, n, m))
        });
    }

    /// A finished job.
    fn wizard_job_done(&mut self, ctx: &egui::Context, w: &mut Wizard, r: Result<Result<Msg, String>, String>) {
        match r.unwrap_or_else(Err) {
            Ok(Msg::SignedIn(s)) => {
                w.signed_in = true;
                w.user = s.login.clone();
                w.repos = s.repos;
                w.repo = None;
                w.token.clear();
                w.say(format!("Signed in as {}. The token is kept in the system keyring.", s.login), true);
                if w.repo_named.is_some() {
                    // a repository was named by the address: nothing to pick
                    self.wizard_repo_next(w, ctx);
                } else {
                    w.step = Step::Repo;
                }
            }
            Ok(Msg::Key(k)) => {
                w.keys.push(k);
                w.key = Some(w.keys.len() - 1);
                w.say("A new key was made. Add its public key to your account on the server, then press Next.", true);
            }
            Ok(Msg::Created(info)) => {
                w.say(format!("Created the private repository {}.", info.full_name()), true);
                w.repos.insert(0, info);
                w.repo = Some(0);
            }
            Ok(Msg::Inspected(remote, notes, moved)) => {
                w.inspected = Some(remote);
                w.vault_has_notes = notes;
                w.history_moved |= moved;
                w.test = Test::Idle;
                w.message = None;
                w.step = Step::Test;
                w.choice = Some(Choice::Server);
            }
            Err(e) => w.say(e, false),
        }
    }

    /// Every frame: the wizard's jobs and the test sync, the detection, the move (SYNC-058, SYNC-061, SYNC-063).
    pub(crate) fn wizard_tick(&mut self, ctx: &egui::Context) {
        self.sync.setup.poll_detect();
        self.moveout_tick(ctx);
        self.join_tick(ctx);
        let Some(mut w) = self.sync.setup.wizard.take() else { return };
        if let Some((_, rx)) = &w.job {
            match rx.try_recv() {
                Ok(r) => {
                    w.job = None;
                    self.wizard_job_done(ctx, &mut w, r);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(100)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    w.job = None;
                    w.say("The job stopped unexpectedly", false);
                }
            }
        }
        match w.test {
            Test::Pending => {
                let mut git = self.sync.git.clone();
                if let Some(cmd) = ssh::ssh_command(w.chosen_key().map(|k| k.private.as_path()).filter(|k| k.file_name().is_some_and(|n| n.to_string_lossy().starts_with("shale_"))), true) {
                    git.env.push(("GIT_SSH_COMMAND".into(), cmd.into()));
                }
                self.sync.last = None;
                if self.start_sync_with(git) {
                    w.test = Test::Running;
                }
                ctx.request_repaint_after(Duration::from_millis(100));
            }
            Test::Running if !self.sync.running() => {
                w.test = match &self.sync.last {
                    Some((_, Ok(_))) => Test::Worked(test_sync::worked(self.sync.last_report.as_ref().unwrap_or(&gitsync::Report::default()))),
                    Some((_, Err(e))) => Test::Failed(test_sync::classify(e)),
                    None => Test::Running,
                };
            }
            Test::Running => ctx.request_repaint_after(Duration::from_millis(100)),
            _ => {}
        }
        self.sync.setup.wizard = Some(w);
    }

    /// Move `.git` out of the vault (SYNC-063), in a job.
    pub(crate) fn start_moveout(&mut self) {
        if self.sync.setup.moveout.running() {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        if self.sync.running() || self.history.busy() || self.sync.lfs_job_running() {
            self.toast("A sync or a snapshot is running; try again in a moment");
            return;
        }
        self.flush_all();
        let (git, tree, history_root, ctx) = (self.sync.git.clone(), self.sync.tree.clone(), self.usage.places.history.clone(), self.ctx.clone());
        let job_root = root.clone();
        let rx = crate::jobs::spawn(
            move || moveout::move_out(&git, &job_root, &history_root, &tree, moveout::TREE_WAIT),
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.sync.setup.moveout = super::MoveOut { job: Some((root, rx)), result: None };
    }

    /// Advanced → Connect for a vault with a local-only history: the move and the connection run in a
    /// job (SYNC-072); without a local history `connect` is quick and runs at once.
    pub(crate) fn start_join(&mut self, root: PathBuf, url: String) {
        if self.sync.setup.joining.job.is_some() {
            return;
        }
        if self.sync.running() || self.history.busy() || self.sync.lfs_job_running() {
            self.toast("A sync or a snapshot is running; try again in a moment");
            return;
        }
        self.flush_all();
        let (git, tree, history_root, ctx) = (self.sync.git.clone(), self.sync.tree.clone(), self.usage.places.history.clone(), self.ctx.clone());
        let (job_root, job_url, guard) = (root.clone(), url.clone(), self.sync.setup.joining.begin());
        let rx = crate::jobs::spawn(
            move || {
                let _guard = guard;
                gitsync::connect_moving(&git, &job_root, &job_url, &history_root, &tree)
            },
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.sync.setup.joining.job = Some((root, url, rx));
    }

    /// Take a finished Advanced connection (SYNC-072).
    fn join_tick(&mut self, ctx: &egui::Context) {
        let Some((root, url, rx)) = &self.sync.setup.joining.job else { return };
        let r = match rx.try_recv() {
            Ok(r) => r.unwrap_or_else(Err),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the connection stopped unexpectedly".into()),
        };
        let (root, url) = (root.clone(), url.clone());
        self.sync.setup.joining.job = None;
        let same_vault = self.vault.as_ref().map(|v| v.root.as_path()) == Some(root.as_path());
        match r {
            Ok(moved) if same_vault => {
                self.sync.remote = Some(url);
                self.sync.last_start = None;
                self.toast(if moved { "Remote set; the versions kept on this computer moved into the vault; syncing now" } else { "Remote set; syncing now" });
                self.refresh_lfs();
                self.start_sync();
            }
            Ok(_) => {}
            Err(e) => self.toast(format!("Could not set the remote: {e}")),
        }
    }

    /// Take a finished move.
    fn moveout_tick(&mut self, ctx: &egui::Context) {
        let Some((root, rx)) = &self.sync.setup.moveout.job else { return };
        let r = match rx.try_recv() {
            Ok(r) => r.unwrap_or_else(Err),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err("the move stopped unexpectedly".into()),
        };
        let root = root.clone();
        self.sync.setup.moveout.job = None;
        self.sync.setup.moveout.result = Some(match r {
            Ok(dir) => (format!("The repository now lives on this computer ({}), outside the vault. The history goes on there.", dir.display()), true),
            Err(e) => (format!("Nothing was changed: {e}"), false),
        });
        if self.vault.as_ref().map(|v| v.root.as_path()) == Some(root.as_path()) {
            self.sync.remote = None;
            self.refresh_lfs();
        }
    }
}

/// What the final screen of the history choices says.
fn done_text(c: Choice) -> String {
    match c {
        Choice::Local => "Shale keeps a history of your notes on this computer, outside the vault folder. Open “Versions of this note…” in a note's ⋯ menu to go back to an older text. Nothing is sent anywhere.".into(),
        _ => "The files of this folder travel through the app that syncs it (Nextcloud, Dropbox, OneDrive, iCloud, a network drive): Shale does not touch that. The history of your notes stays on each computer, outside the folder, and no .git folder is put in it. Folders of Nextcloud, Dropbox, OneDrive and Syncthing are safe for your notes.".into(),
    }
}

/// The warnings about where the vault is, and the offer to move the repository out (SYNC-061,
/// SYNC-062). True: the person pressed **Move the repository out**.
pub(crate) fn warnings_ui(ui: &mut Ui, findings: &[Finding], has_dot_git: bool, has_remote: bool, moving: bool, moved: Option<&(String, bool)>) -> bool {
    let mut ask = false;
    if has_dot_git {
        if let Some(w) = detect::repo_warning(findings, has_remote) {
            theme::problem(ui, w);
            if !has_remote {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!moving, egui::Button::new("Move the repository out")).on_hover_text("Copy the repository to a private place on this computer, check the copy, then remove the .git folder of the vault").clicked() {
                        ask = true;
                    }
                    if moving {
                        ui.spinner();
                    }
                });
            }
        }
    } else if let Some(w) = detect::warning(findings) {
        theme::hint(ui, w);
    }
    if let Some((text, ok)) = moved {
        if *ok {
            theme::hint(ui, text.clone());
        } else {
            theme::problem(ui, text.clone());
        }
    }
    ask
}

fn message(ui: &mut Ui, w: &Wizard) {
    if let Some((text, ok)) = &w.message {
        if *ok {
            theme::hint(ui, text.clone());
        } else {
            theme::problem(ui, text.clone());
        }
    }
}

fn spinner_line(ui: &mut Ui, w: &Wizard, text: &str) {
    if w.job.is_some() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(text);
        });
    }
}

/// The content of the step; returns what was clicked, and whether the git settings changed.
fn body(ui: &mut Ui, w: &mut Wizard, cx: &Cx, managed: &mut ManagedState, gitcfg: &mut GitConfig) -> (Option<Ask>, bool) {
    let mut ask = None;
    let mut changed = false;
    match w.step {
        Step::Choose => {
            ui.label(egui::RichText::new("How do you want to keep your notes safe?").strong().size(15.0));
            ui.add_space(4.0);
            if !cx.git_ok {
                theme::paragraph(ui, "Shale needs git for this, and there is none on this computer yet. Install it here; the choices appear as soon as git is there.");
                changed = install_ui::show(ui, managed, gitcfg);
                return (None, changed);
            }
            if warnings_ui(ui, &cx.findings, cx.has_dot_git, cx.has_remote, cx.moving, cx.moved.as_ref()) {
                ask = Some(Ask::MoveOut);
            }
            for (choice, title, text) in [
                (Choice::Local, "History on this device only", "Shale keeps earlier versions of your notes on this computer, outside your vault folder. No server and no account."),
                (
                    Choice::Synced,
                    "My folder is already synced by another app",
                    "Nextcloud, Dropbox, OneDrive, iCloud or a network drive. The files travel through that app; the history stays on each computer, and no .git folder goes into the folder. That is safe for your notes.",
                ),
                (Choice::Server, "Sync with a server", "Forgejo, Gitea, GitHub, GitLab or any git address. Your notes are sent to the server and to your other computers; this vault gets a .git folder."),
            ] {
                ui.add_space(6.0);
                if ui.radio(w.choice == Some(choice), egui::RichText::new(title).strong()).clicked() {
                    w.choice = Some(choice);
                }
                ui.indent(title, |ui| theme::hint(ui, text));
            }
        }
        Step::Server => {
            ui.label(egui::RichText::new("Which server?").strong().size(15.0));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                for (label, kind, host) in PRESETS {
                    if ui.button(label).clicked() {
                        w.address = host.to_string();
                        w.kind = kind;
                    }
                }
            });
            ui.add_space(4.0);
            ui.label("Or the address of your server, or of a repository:");
            ui.add(egui::TextEdit::singleline(&mut w.address).hint_text("git.example.org, https://… or git@host:owner/repo.git").desired_width(460.0));
            let known = provider::parse(&w.address, Kind::Other).map(|p| p.server.kind != Kind::Other).unwrap_or(false);
            if !known && !super::is_local_remote(&w.address) {
                ui.label("What kind of server is it?");
                ui.horizontal_wrapped(|ui| {
                    for k in [Kind::Forgejo, Kind::GitHub, Kind::GitLab, Kind::Other] {
                        ui.radio_value(&mut w.kind, k, k.label());
                    }
                });
                theme::hint(ui, "Forgejo, Gitea and GitHub let Shale list and create repositories for you. For any other server you paste the address of a repository.");
            }
            theme::hint(ui, "A folder on this computer or a network share also works (an absolute path): no sign-in is needed.");
            message(ui, w);
            spinner_line(ui, w, "Asking the server…");
        }
        Step::SignIn => {
            let Some(server) = w.server.clone() else { return (None, false) };
            ui.label(egui::RichText::new(format!("Sign in to {}", server.host)).strong().size(15.0));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.radio_value(&mut w.method, Method::Token, "A personal access token");
                ui.radio_value(&mut w.method, Method::Ssh, "An SSH key");
            });
            ui.add_space(4.0);
            match w.method {
                Method::Token => {
                    theme::hint(ui, "A token is a password made for one app. Make one on the server's website and paste it here; Shale keeps it in your system's keyring (never in a file) and gives it to git when it syncs.");
                    if let Some(page) = server.token_page() {
                        if ui.button("Open the page where you make a token").clicked() {
                            ask = Some(Ask::Open(page));
                        }
                    }
                    if !server.has_api() {
                        ui.horizontal(|ui| {
                            ui.label("User name");
                            ui.add(egui::TextEdit::singleline(&mut w.user).desired_width(240.0));
                        });
                    }
                    ui.horizontal(|ui| {
                        ui.label("Token");
                        crate::secrets::password_edit(ui, &mut w.token, "Paste the token", 320.0);
                    });
                    ui.horizontal(|ui| {
                        if ui.add_enabled(w.job.is_none() && !w.token.is_empty(), egui::Button::new("Check the token")).clicked() {
                            ask = Some(Ask::CheckToken);
                        }
                    });
                }
                Method::Ssh => {
                    theme::hint(ui, "An SSH key is a pair of files: you keep the private one on this computer and put the public one on the server.");
                    for (i, k) in w.keys.clone().iter().enumerate() {
                        if ui.radio(w.key == Some(i), k.name()).clicked() {
                            w.key = Some(i);
                        }
                    }
                    if ui.add_enabled(w.job.is_none(), egui::Button::new("Make a new key")).on_hover_text("ssh-keygen -t ed25519, without a passphrase, in ~/.ssh").clicked() {
                        ask = Some(Ask::MakeKey);
                    }
                    if let Some(k) = w.chosen_key() {
                        ui.add_space(4.0);
                        ui.label("Add this public key to your account on the server:");
                        ui.add(egui::Label::new(egui::RichText::new(&k.public).monospace().size(11.0)).selectable(true).wrap());
                        ui.horizontal(|ui| {
                            if ui.button("Copy the public key").clicked() {
                                ask = Some(Ask::Copy(k.public.clone()));
                            }
                            if let Some(page) = server.ssh_keys_page() {
                                if ui.button("Open the page where you add it").clicked() {
                                    ask = Some(Ask::Open(page));
                                }
                            }
                        });
                    }
                }
            }
            message(ui, w);
            spinner_line(ui, w, "Working…");
        }
        Step::Repo => {
            let Some(server) = w.server.clone() else { return (None, false) };
            ui.label(egui::RichText::new("Which repository holds your notes?").strong().size(15.0));
            ui.add_space(4.0);
            if w.method == Method::Token && w.signed_in && server.has_api() {
                if w.repos.is_empty() {
                    theme::hint(ui, "You have no repository yet. Make one:");
                } else {
                    egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                        for (i, r) in w.repos.clone().iter().enumerate() {
                            let label = format!("{}{}", r.full_name(), if r.empty { "  (empty)" } else { "" });
                            if ui.radio(w.repo == Some(i), label).clicked() {
                                w.repo = Some(i);
                            }
                        }
                    });
                }
                ui.add_space(4.0);
                ui.label("Or make a new private repository:");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut w.new_name).desired_width(200.0));
                    if ui.add_enabled(w.job.is_none(), egui::Button::new("Create a private repository")).clicked() {
                        ask = Some(Ask::CreateRepo);
                    }
                });
            } else {
                if let Some(page) = server.new_repo_page() {
                    theme::hint(ui, "Make an empty repository on the server's website (or use one you have), then enter its address here.");
                    if ui.button("Open the page where you make a repository").clicked() {
                        ask = Some(Ask::Open(page));
                    }
                }
                ui.add(egui::TextEdit::singleline(&mut w.repo_text).hint_text("owner/name or the address of the repository").desired_width(460.0));
            }
            message(ui, w);
            spinner_line(ui, w, "Asking the server…");
        }
        Step::Test => {
            ui.label(egui::RichText::new("Test the sync").strong().size(15.0));
            ui.add_space(4.0);
            if let Some(r) = w.remote.as_ref() {
                theme::hint(ui, format!("Server: {}", gitsync::hide_passwords(r)));
            }
            if let Some(remote) = w.inspected {
                theme::paragraph(ui, test_sync::what_happens(remote, w.vault_has_notes));
            }
            if w.history_moved {
                theme::paragraph(ui, test_sync::history_moved());
            }
            ui.add_space(6.0);
            match &w.test {
                Test::Idle => {
                    theme::hint(ui, "The test is one real sync: Shale fetches from the server and uploads your notes.");
                }
                Test::Pending | Test::Running => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Syncing…");
                    });
                }
                Test::Worked(text) => {
                    theme::paragraph(ui, text.clone());
                }
                Test::Failed(p) => {
                    theme::problem(ui, p.words());
                }
            }
        }
        Step::Done => {
            ui.label(egui::RichText::new("Done").strong().size(15.0));
            ui.add_space(4.0);
            if let Some(t) = &w.done {
                theme::paragraph(ui, t.clone());
            }
            if warnings_ui(ui, &cx.findings, cx.has_dot_git, cx.has_remote, cx.moving, cx.moved.as_ref()) {
                ask = Some(Ask::MoveOut);
            }
        }
    }
    (ask, changed)
}

/// Back, Cancel and the main button.
fn footer(ui: &mut Ui, w: &mut Wizard, cx: &Cx) -> Option<Ask> {
    let mut ask = None;
    ui.horizontal(|ui| {
        let busy = w.busy();
        if w.step != Step::Choose && w.step != Step::Done && ui.add_enabled(!busy, egui::Button::new("Back")).clicked() {
            w.back();
        }
        if ui.button("Cancel").clicked() {
            ask = Some(Ask::Cancel);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let (label, ok, then): (&str, bool, Option<Ask>) = match w.step {
                Step::Choose => ("Next", cx.git_ok && w.choice.is_some(), Some(Ask::ChooseNext)),
                Step::Server => ("Next", !busy && !w.address.trim().is_empty(), Some(Ask::ServerNext)),
                Step::SignIn => {
                    let ready = match w.method {
                        Method::Token => w.signed_in,
                        Method::Ssh => w.chosen_key().is_some(),
                    };
                    ("Next", !busy && ready, Some(Ask::SignInNext))
                }
                Step::Repo => ("Next", !busy, Some(Ask::RepoNext)),
                Step::Test => match &w.test {
                    Test::Worked(_) => ("Finish", true, Some(Ask::Finish)),
                    Test::Failed(_) => ("Try again", true, Some(Ask::RunTest)),
                    Test::Idle => ("Run the test sync", w.inspected.is_some(), Some(Ask::RunTest)),
                    Test::Pending | Test::Running => ("Syncing…", false, None),
                },
                Step::Done => ("Finish", true, Some(Ask::Finish)),
            };
            if ui.add_enabled(ok, egui::Button::new(label)).clicked() {
                ask = then;
            }
        });
    });
    ask
}
