//! The "Feed accounts" dialog: add a Mastodon or Bluesky account, sign in again to one (Edit, SOC-043), remove one.
//!
//! Mastodon: type the server, open the sign-in page in the browser, paste the code the server
//! shows (or paste an access token you made yourself). Bluesky: handle and an *app password*.
//! All network steps run on a background thread; the dialog shows a spinner and the error.

use super::mastodon::{self, AppCredentials};
use super::{bluesky, clean_server, Account, Network, SocialAccountEntry, BLUESKY_SERVER};
use crate::theme;
use eframe::egui::{self, Rect, Ui};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

enum Done {
    /// the app is registered on the server and the sign-in page is open
    Opened(String, AppCredentials),
    Added(SocialAccountEntry),
    /// signed in again to the account that is being edited: the entry holds the new secret
    Resigned(SocialAccountEntry),
}

type JobResult = Result<Done, String>;

pub struct SocialDialog {
    pub network: Network,
    /// Mastodon server / Bluesky server
    pub server: String,
    /// Bluesky handle
    pub handle: String,
    /// Bluesky app password / Mastodon access token
    pub secret: crate::secret::Secret,
    /// Mastodon: the code from the sign-in page
    pub code: String,
    /// Mastodon: sign in with an access token instead of the browser
    pub use_token: bool,
    app: Option<(String, AppCredentials)>,
    /// Edit (SOC-043): the account the form signs in to again; network, server and handle are fixed
    pub editing: Option<Account>,
    job: Option<Receiver<JobResult>>,
    pub error: Option<String>,
    pub rects: Vec<(String, Rect)>,
}

#[derive(Debug, PartialEq)]
pub enum DialogAction {
    Close,
    /// a new account signed in (its secret is already in the keyring or the entry)
    Added(SocialAccountEntry),
    /// signed in again to this account: its secret is replaced (the entry carries the new one, or
    /// nothing while the keyring holds it); everything else of the account stays (SOC-043)
    Updated(SocialAccountEntry),
    /// remove the account with this id
    Remove(String),
    /// the number of minutes between looks for new posts was changed (0 = never)
    CheckMinutes(u32),
}

fn spawn(ctx: &egui::Context, f: impl FnOnce() -> JobResult + Send + 'static) -> Receiver<JobResult> {
    let (tx, rx) = channel();
    let ctx = ctx.clone();
    crate::jobs::run(f, move |r| {
        let _ = tx.send(r.and_then(|x| x));
        ctx.request_repaint();
    });
    rx
}

impl SocialDialog {
    pub fn new() -> SocialDialog {
        SocialDialog { network: Network::Mastodon, server: String::new(), handle: String::new(), secret: Default::default(), code: String::new(), use_token: false, app: None, editing: None, job: None, error: None, rects: vec![] }
    }

    /// Edit (SOC-043): the form signs in again to `account`.
    pub fn start_edit(&mut self, account: &Account) {
        self.network = account.network;
        self.server = account.server.clone();
        self.handle = account.handle.clone();
        self.editing = Some(account.clone());
        self.clear_input();
    }

    /// Leave Edit: the form is empty again, ready to add an account.
    pub fn cancel_edit(&mut self) {
        self.editing = None;
        self.server.clear();
        self.handle.clear();
        self.clear_input();
    }

    fn clear_input(&mut self) {
        self.error = None;
        self.secret.clear();
        self.code.clear();
        self.app = None;
        self.use_token = false;
    }

    pub fn busy(&self) -> bool {
        self.job.is_some()
    }

    /// The server as typed, cleaned up; the error text when it is not usable.
    fn server_ok(&self) -> Result<String, String> {
        if self.server.trim().is_empty() {
            return Err("Type the server first, for example mastodon.social.".into());
        }
        clean_server(&self.server).map_err(|e| format!("This server address can't be used: {e}"))
    }

    fn open_sign_in(&mut self, ctx: &egui::Context) {
        let server = match self.server_ok() {
            Ok(s) => s,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        self.error = None;
        self.job = Some(spawn(ctx, move || {
            let app = mastodon::register_app(&server)?;
            let url = mastodon::authorize_url(&server, &app)?;
            crate::opener::open_signin_url(&url, false)?;
            Ok(Done::Opened(server, app))
        }));
    }

    fn sign_in(&mut self, ctx: &egui::Context) {
        self.error = None;
        let secret = crate::secret::Secret::new(self.secret.expose().trim());
        let editing = self.editing.clone();
        match self.network {
            Network::Mastodon => {
                let server = match self.server_ok() {
                    Ok(s) => s,
                    Err(e) => {
                        self.error = Some(e);
                        return;
                    }
                };
                let (code, app, use_token) = (self.code.trim().to_string(), self.app.clone().filter(|a| a.0 == server).map(|a| a.1), self.use_token);
                if use_token && secret.is_empty() {
                    self.error = Some("Paste the access token first.".into());
                    return;
                }
                if !use_token && (code.is_empty() || app.is_none()) {
                    self.error = Some(if app.is_none() { "Open the sign-in page first, then paste the code it shows." } else { "Paste the code from the sign-in page first." }.into());
                    return;
                }
                self.job = Some(spawn(ctx, move || {
                    // the permissions the server gave are kept with the account (not a secret); a pasted token says nothing
                    let (token, scopes) = match (&app, use_token) {
                        (Some(app), false) => mastodon::exchange_code(&server, app, &code)?,
                        _ => (secret, None),
                    };
                    let who = mastodon::verify(&server, token.expose())?;
                    if let Some(old) = editing {
                        return resigned(old, &who.handle, &token, scopes);
                    }
                    let account = Account { scopes, ..Account::new(Network::Mastodon, &server, &who.handle, &who.display_name) };
                    Ok(Done::Added(SocialAccountEntry::new(account, &token)))
                }));
            }
            Network::Bluesky => {
                let server = if self.server.trim().is_empty() { BLUESKY_SERVER.to_string() } else { self.server.clone() };
                let handle = self.handle.trim().to_string();
                if handle.is_empty() || secret.is_empty() {
                    self.error = Some("Type your handle and an app password.".into());
                    return;
                }
                self.job = Some(spawn(ctx, move || {
                    let server = clean_server(&server).map_err(|e| format!("This server address can't be used: {e}"))?;
                    let session = bluesky::create_session(&server, &handle, secret.expose())?;
                    if let Some(old) = editing {
                        return resigned(old, &session.handle, &secret, None);
                    }
                    Ok(Done::Added(SocialAccountEntry::new(Account::new(Network::Bluesky, &server, &session.handle, &session.handle), &secret)))
                }));
            }
        }
    }

    /// Take the result of the running step. The action of a finished sign-in is returned.
    fn poll(&mut self) -> Option<DialogAction> {
        let rx = self.job.as_ref()?;
        let r = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Err("the sign-in stopped unexpectedly".into()),
        };
        self.job = None;
        match r {
            Ok(Done::Opened(server, app)) => self.app = Some((server, app)),
            Ok(Done::Added(e)) => {
                self.secret.clear();
                self.code.clear();
                self.handle.clear();
                self.server.clear();
                self.app = None;
                return Some(DialogAction::Added(e));
            }
            Ok(Done::Resigned(e)) => {
                self.cancel_edit();
                return Some(DialogAction::Updated(e));
            }
            Err(e) => self.error = Some(e),
        }
        None
    }
}

/// The sign-in of Edit worked: it must be the account's own handle (nothing is stored for another
/// one); then the secret goes to the keyring (or the entry) under the account's own name, and
/// the entry is the account as it was.
fn resigned(old: Account, signed_in_as: &str, secret: &crate::secret::Secret, scopes: Option<String>) -> JobResult {
    if !signed_in_as.trim().trim_start_matches('@').eq_ignore_ascii_case(&old.handle) {
        return Err(format!("This sign-in is for @{}, not for @{}. Nothing was changed.", signed_in_as.trim_start_matches('@'), old.handle));
    }
    Ok(Done::Resigned(SocialAccountEntry::new(Account { scopes, ..old }, secret)))
}

impl Default for SocialDialog {
    fn default() -> Self {
        Self::new()
    }
}

/// `check_minutes`: how often the feeds open in tabs look for new posts (a setting of the app).
pub fn show(ui: &mut Ui, d: &mut SocialDialog, accounts: &[SocialAccountEntry], check_minutes: u32) -> Option<DialogAction> {
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut rec = |k: String, r: egui::Response| -> egui::Response {
        rects.push((k, r.rect));
        r
    };
    let mut action = None;
    if let Some(a) = d.poll() {
        action = Some(a);
    }
    if d.busy() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::palette().muted);
    // ---- the list
    if accounts.is_empty() {
        crate::theme::prose(ui, muted("No account yet. Add a Mastodon or Bluesky account below to read its feeds here."));
    }
    let mut start_edit = None;
    for (i, e) in accounts.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(e.account.network.label()).size(11.0).color(theme::palette().accent));
            let name = if e.account.display_name.trim().is_empty() { e.account.handle.clone() } else { format!("{} (@{})", e.account.display_name, e.account.handle) };
            ui.label(name);
            ui.label(muted(&e.account.server));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (editing, busy) = (d.editing.as_ref().is_some_and(|a| a.id == e.account.id), d.busy());
                if rec(format!("remove:{i}"), ui.small_button("Remove")).on_hover_text("Forget this account and its stored password").clicked() {
                    action = Some(DialogAction::Remove(e.account.id.clone()));
                }
                if rec(format!("edit:{i}"), ui.add_enabled(!busy && !editing, egui::Button::new("Edit").small())).on_hover_text("Sign in to this account again, for example with a new app password").clicked() {
                    start_edit = Some(e.account.clone());
                }
            });
        });
    }
    if let Some(a) = start_edit {
        d.start_edit(&a);
    }
    theme::section_break(ui);
    ui.horizontal(|ui| {
        ui.label(muted("Look for new posts every"));
        let mut m = check_minutes;
        if rec("check-minutes".into(), ui.add(egui::DragValue::new(&mut m).range(0..=240).suffix(" min"))).on_hover_text("0 = never. Only feeds open in a tab are checked, and only while Shale runs.").changed() && action.is_none() {
            action = Some(DialogAction::CheckMinutes(m));
        }
    });
    // ---- add
    if let Some(a) = &d.editing {
        theme::section(ui, format!("Sign in again as @{}", a.handle));
        ui.add(
            egui::Label::new(crate::theme::spaced(
                ui,
                muted(&format!(
                    "{} account on {}. The network, server and handle stay as they are; a successful sign-in replaces the saved secret, and the account keeps its feeds and posts. Cancel leaves this form.",
                    a.network.label(),
                    a.server
                )),
            ))
            .wrap(),
        );
    } else {
        theme::section(ui, "Add an account");
    }
    let (fixed, current) = (d.editing.is_some(), d.network);
    ui.horizontal(|ui| {
        for n in [Network::Mastodon, Network::Bluesky].into_iter().filter(|n| !fixed || *n == current) {
            if rec(format!("network:{}", n.key()), ui.selectable_label(d.network == n, n.label())).clicked() && d.network != n {
                d.network = n;
                d.error = None;
                d.secret.clear();
                d.code.clear();
            }
        }
    });
    let busy = d.busy();
    match d.network {
        Network::Mastodon => {
            egui::Grid::new("social-mastodon").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(muted("Server"));
                rec("server".into(), ui.add_enabled(!fixed, egui::TextEdit::singleline(&mut d.server).hint_text("mastodon.social").desired_width(380.0)));
                ui.end_row();
                if d.use_token {
                    ui.label(muted("Access token"));
                    rec("token".into(), crate::secrets::password_edit(ui, &mut d.secret, "created in Preferences → Development", 380.0));
                    ui.end_row();
                } else {
                    ui.label(muted("Step 1"));
                    ui.horizontal(|ui| {
                        if rec("open-browser".into(), ui.add_enabled(!busy, egui::Button::new("Open sign-in page in browser")))
                            .on_hover_text("Shale asks the server for a sign-in that can read and write (to like, boost and post); you log in and allow it on the server's own page")
                            .clicked()
                        {
                            d.open_sign_in(ui.ctx());
                        }
                        if d.app.is_some() {
                            ui.label(egui::RichText::new("opened").size(12.0).color(theme::palette().muted));
                        }
                    });
                    ui.end_row();
                    ui.label(muted("Step 2"));
                    rec("code".into(), ui.add(egui::TextEdit::singleline(&mut d.code).hint_text("paste the code the page shows").desired_width(380.0)));
                    ui.end_row();
                }
            });
            if rec("use-token".into(), ui.checkbox(&mut d.use_token, "I already have an access token")).changed() {
                d.error = None;
            }
            crate::theme::prose(ui, muted("Shale only asks for permission to read. The code or token is not kept; the access token goes to your system keyring."));
        }
        Network::Bluesky => {
            egui::Grid::new("social-bluesky").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(muted("Handle"));
                rec("handle".into(), ui.add_enabled(!fixed, egui::TextEdit::singleline(&mut d.handle).hint_text("you.bsky.social").desired_width(380.0)));
                ui.end_row();
                ui.label(muted("App password"));
                rec("app-password".into(), crate::secrets::password_edit(ui, &mut d.secret, "xxxx-xxxx-xxxx-xxxx", 380.0));
                ui.end_row();
                ui.label(muted("Server"));
                rec("server".into(), ui.add_enabled(!fixed, egui::TextEdit::singleline(&mut d.server).hint_text(BLUESKY_SERVER).desired_width(380.0)));
                ui.end_row();
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Use an app password, never your main password: Bluesky → Settings → Privacy and security → App passwords. It can be revoked there at any time. It goes to your system keyring.")
                        .size(12.0)
                        .color(theme::palette().link_missing.gamma_multiply(0.85)),
                )
                .wrap(),
            );
        }
    }
    d.rects = rects;
    action
}

/// The dialog's footer (NAV-028): the error of the last try, Sign in with the spinner while a
/// step runs, and Close. Call it after [`show`] in the same frame.
pub fn footer(ui: &mut Ui, d: &mut SocialDialog) -> Option<DialogAction> {
    let mut action = None;
    let mut cancel_edit = false;
    let busy = d.busy();
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::palette().muted);
    if let Some(e) = &d.error {
        ui.add(egui::Label::new(egui::RichText::new(e).color(theme::palette().link_missing)).wrap());
    }
    ui.horizontal(|ui| {
        let mut rec = |k: &str, r: egui::Response| -> egui::Response {
            d.rects.push((k.to_string(), r.rect));
            r
        };
        let sign_in = rec("sign-in", ui.add_enabled(!busy, egui::Button::new(egui::RichText::new("Sign in").strong()))).clicked();
        if busy {
            ui.spinner();
            ui.label(muted("Talking to the server…"));
        }
        let editing = d.editing.is_some();
        if editing && rec("edit-cancel", ui.add_enabled(!busy, egui::Button::new("Cancel"))).clicked() {
            cancel_edit = true;
        }
        if rec("close", ui.button("Close")).clicked() {
            action = Some(DialogAction::Close);
        }
        if sign_in {
            d.sign_in(ui.ctx());
        }
    });
    if cancel_edit {
        d.cancel_edit();
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event as Ev, Modifiers, PointerButton, Pos2, RawInput, Vec2};

    fn frame(ctx: &egui::Context, t: &mut f64, events: Vec<Ev>, d: &mut SocialDialog, accounts: &[SocialAccountEntry]) -> Option<DialogAction> {
        *t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 900.0))), time: Some(*t), events, ..Default::default() };
        let mut res = None;
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                // the content and the footer, as the dialog window shows them together
                let a = show(ui, d, accounts, 5);
                res = footer(ui, d).or(a);
            });
        });
        out.textures_delta.clear();
        res
    }

    fn click(ctx: &egui::Context, t: &mut f64, key: &str, d: &mut SocialDialog, accounts: &[SocialAccountEntry]) -> Option<DialogAction> {
        let p = d.rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no {key}: {:?}", d.rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1.center();
        let b = |pressed| Ev::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        let mut r = frame(ctx, t, vec![Ev::PointerMoved(p)], d, accounts);
        r = frame(ctx, t, vec![b(true)], d, accounts).or(r);
        r = frame(ctx, t, vec![b(false)], d, accounts).or(r);
        frame(ctx, t, vec![], d, accounts).or(r)
    }

    // spec: SOC-004, SOC-005
    #[test]
    fn the_account_dialog_checks_its_input_before_it_asks_the_network() {
        let ctx = egui::Context::default();
        let mut t = 0.0;
        let acct = SocialAccountEntry { account: Account::new(Network::Bluesky, BLUESKY_SERVER, "ada.bsky.social", "ada.bsky.social"), secret: Default::default() };
        let accounts = vec![acct.clone()];
        let mut d = SocialDialog::new();
        frame(&ctx, &mut t, vec![], &mut d, &accounts);
        // Mastodon: no server typed
        click(&ctx, &mut t, "open-browser", &mut d, &accounts);
        assert!(d.error.as_deref().unwrap().contains("server"), "{:?}", d.error);
        assert!(!d.busy());
        click(&ctx, &mut t, "sign-in", &mut d, &accounts);
        assert!(!d.busy());
        // the token form
        click(&ctx, &mut t, "use-token", &mut d, &accounts);
        assert!(d.use_token);
        d.server = "mastodon.social".into();
        click(&ctx, &mut t, "sign-in", &mut d, &accounts);
        assert!(d.error.as_deref().unwrap().contains("token"), "{:?}", d.error);
        // Bluesky: handle and app password are needed, and the hint names app passwords
        click(&ctx, &mut t, "network:bluesky", &mut d, &accounts);
        assert_eq!(d.network, Network::Bluesky);
        click(&ctx, &mut t, "sign-in", &mut d, &accounts);
        assert!(d.error.as_deref().unwrap().contains("app password"), "{:?}", d.error);
        assert!(!d.busy());
        // removing an account is reported
        assert_eq!(click(&ctx, &mut t, "remove:0", &mut d, &accounts), Some(DialogAction::Remove("bluesky:ada.bsky.social".into())));
        assert_eq!(click(&ctx, &mut t, "close", &mut d, &accounts), Some(DialogAction::Close));
    }

    /// Frames until the dialog's job has ended (the spinner is gone); the action it produced, if any.
    fn until_idle(ctx: &egui::Context, t: &mut f64, d: &mut SocialDialog) -> Option<DialogAction> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let r = frame(ctx, t, vec![], d, &[]);
            if r.is_some() || !d.busy() {
                return r;
            }
            assert!(std::time::Instant::now() < deadline, "the spinner never stopped");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// Click and wait for the job that the click started.
    fn click_and_wait(ctx: &egui::Context, t: &mut f64, key: &str, d: &mut SocialDialog) -> Option<DialogAction> {
        click(ctx, t, key, d, &[]).or_else(|| until_idle(ctx, t, d))
    }

    fn has_curl() -> bool {
        std::process::Command::new("curl").arg("--version").output().is_ok()
    }

    // spec: SOC-001, SOC-004, SOC-010, SOC-056
    #[test]
    fn a_mastodon_sign_in_opens_the_browser_takes_the_code_and_adds_the_account() {
        if !has_curl() {
            return;
        }
        use super::super::fake_server::{pretend_browser, FakeServer, Person};
        let server = FakeServer::start();
        let (_guard, log) = pretend_browser(server.port, Person::Allows);
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        frame(&ctx, &mut t, vec![], &mut d, &[]);
        d.server = server.address();
        assert!(click_and_wait(&ctx, &mut t, "open-browser", &mut d).is_none());
        assert!(d.error.is_none(), "{:?}", d.error);
        assert!(d.app.is_some(), "the app is registered, the dialog says \"opened\"");
        // the person reads the code off the server's page and pastes it
        d.code = log.wait_for_code();
        assert_eq!(log.opened.lock().unwrap().len(), 1);
        assert!(log.problems.lock().unwrap().is_empty(), "{:?}", log.problems.lock().unwrap());
        let added = click_and_wait(&ctx, &mut t, "sign-in", &mut d);
        let Some(DialogAction::Added(e)) = added else { panic!("no account was added: {:?}", d.error) };
        assert_eq!((e.account.network, e.account.handle.as_str(), e.account.display_name.as_str(), e.account.server.as_str()), (Network::Mastodon, "ada@127.0.0.1", "Ada L.", server.address().as_str()));
        assert!(e.secret.is_empty() && server.state.lock().unwrap().tokens.iter().any(|t| t == e.secret().expose()), "the access token the server made is in the keyring, not in the entry");
        // spec: SOC-056: the permissions the server gave are kept with the account (and can write)
        assert_eq!((e.account.scopes.as_deref(), e.account.may_write()), (Some("read write"), Some(true)));
        crate::secrets::social_clear(&e.account.server, &e.account.handle);
        assert_eq!(server.state.lock().unwrap().requests, ["POST /api/v1/apps", "GET /oauth/authorize", "POST /oauth/token", "GET /api/v1/accounts/verify_credentials"]);
        assert!(d.code.is_empty() && d.server.is_empty() && d.app.is_none(), "the dialog is ready for the next account");
    }

    // spec: SOC-004, SOC-010
    #[test]
    fn a_no_on_the_sign_in_page_or_a_wrong_code_ends_with_a_message_and_adds_nothing() {
        if !has_curl() {
            return;
        }
        use super::super::fake_server::{pretend_browser, FakeServer, Person};
        let server = FakeServer::start();
        let (_guard, log) = pretend_browser(server.port, Person::Denies);
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        frame(&ctx, &mut t, vec![], &mut d, &[]);
        d.server = server.address();
        click_and_wait(&ctx, &mut t, "open-browser", &mut d);
        assert!(d.app.is_some());
        // the person said no (or closed the tab): there is no code, and signing in says so
        click(&ctx, &mut t, "sign-in", &mut d, &[]);
        assert!(!d.busy() && d.error.as_deref().unwrap().contains("Paste the code"), "{:?}", d.error);
        // a made-up code is refused by the server: the error is shown and the spinner stops
        d.code = "made-up".into();
        assert!(click_and_wait(&ctx, &mut t, "sign-in", &mut d).is_none(), "no account");
        assert!(d.error.as_deref().is_some_and(|e| e.contains("400") && e.contains("invalid")), "{:?}", d.error);
        assert!(!d.busy());
        assert!(d.app.is_some(), "the person can paste another code");
        assert!(server.state.lock().unwrap().tokens.is_empty());
        assert!(log.code.lock().unwrap().is_none() && log.problems.lock().unwrap().is_empty());
    }

    // spec: SOC-004, SEC-006
    #[test]
    fn the_feeds_sign_in_page_is_only_opened_for_https_servers() {
        if !has_curl() {
            return;
        }
        // no pretend browser is installed: a plain http server must not reach any program
        let server = super::super::fake_server::FakeServer::start();
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        frame(&ctx, &mut t, vec![], &mut d, &[]);
        d.server = server.address();
        click_and_wait(&ctx, &mut t, "open-browser", &mut d);
        assert!(d.error.as_deref().is_some_and(|e| e.contains("https://")), "{:?}", d.error);
        assert!(d.app.is_none());
    }

    // spec: SOC-002, SOC-004
    #[test]
    fn a_bluesky_sign_in_with_a_wrong_app_password_says_so_and_the_right_one_adds_the_account() {
        if !has_curl() {
            return;
        }
        let server = super::super::fake_server::FakeServer::start();
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        frame(&ctx, &mut t, vec![], &mut d, &[]);
        click(&ctx, &mut t, "network:bluesky", &mut d, &[]);
        d.server = server.address();
        d.handle = "ada.test".into();
        d.secret = "wrong-wrong-wrong-wrong".into();
        assert!(click_and_wait(&ctx, &mut t, "sign-in", &mut d).is_none() && !d.busy());
        assert_eq!(d.error.as_deref(), Some("Bluesky refused the handle or app password"));
        d.secret = "abcd-efgh-ijkl-mnop".into();
        let Some(DialogAction::Added(e)) = click_and_wait(&ctx, &mut t, "sign-in", &mut d) else { panic!("no account was added: {:?}", d.error) };
        assert_eq!((e.account.network, e.account.handle.as_str(), e.secret.expose(), e.secret().expose()), (Network::Bluesky, "ada.test", "", "abcd-efgh-ijkl-mnop"), "the app password is in the keyring");
        crate::secrets::social_clear(&e.account.server, &e.account.handle);
        assert!(d.secret.is_empty() && d.handle.is_empty(), "the app password does not stay in the dialog");
    }

    /// An account that is already in the list, its secret in the keyring.
    fn existing(network: Network, server: &str, handle: &str, name: &str, secret: &str) -> SocialAccountEntry {
        let e = SocialAccountEntry::new(Account::new(network, server, handle, name), &crate::secret::Secret::new(secret));
        assert!(e.secret.is_empty(), "the keyring holds it");
        e
    }

    // spec: SOC-043
    #[test]
    fn soc_043_edit_signs_in_again_to_a_bluesky_account_with_a_new_app_password_and_keeps_the_account() {
        if !has_curl() {
            return;
        }
        let server = super::super::fake_server::FakeServer::start();
        let old = existing(Network::Bluesky, &server.address(), "ada.test", "Ada the Custom", "old-app-password");
        let accounts = vec![old.clone()];
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        frame(&ctx, &mut t, vec![], &mut d, &accounts);
        click(&ctx, &mut t, "edit:0", &mut d, &accounts);
        assert_eq!(d.editing.as_ref().map(|a| a.id.as_str()), Some("bluesky:ada.test"));
        assert_eq!((d.network, d.handle.as_str(), d.server.as_str()), (Network::Bluesky, "ada.test", server.address().as_str()), "network, handle and server are fixed to the account's");
        assert!(!d.rects.iter().any(|r| r.0 == "network:mastodon") && d.rects.iter().any(|r| r.0 == "edit-cancel"), "no other network, a Cancel instead");
        // a wrong app password: the error is shown, the old secret stays
        d.secret = "wrong-wrong-wrong-wrong".into();
        assert!(click_and_wait(&ctx, &mut t, "sign-in", &mut d).is_none());
        assert_eq!(d.error.as_deref(), Some("Bluesky refused the handle or app password"));
        assert!(d.editing.is_some(), "still in the form");
        assert_eq!(crate::secrets::social_lookup(&old.account.server, "ada.test").as_ref().map(|s| s.expose()), Some("old-app-password"));
        // the right one
        d.secret = "abcd-efgh-ijkl-mnop".into();
        let Some(DialogAction::Updated(e)) = click_and_wait(&ctx, &mut t, "sign-in", &mut d) else { panic!("not signed in again: {:?}", d.error) };
        assert_eq!(e.account, old.account, "id, display name and server are the account's own");
        assert_eq!(crate::secrets::social_lookup(&old.account.server, "ada.test").as_ref().map(|s| s.expose()), Some("abcd-efgh-ijkl-mnop"), "the keyring has the new app password");
        assert!(d.editing.is_none() && d.secret.is_empty() && d.handle.is_empty() && d.server.is_empty(), "the form is empty again");
        crate::secrets::social_clear(&e.account.server, &e.account.handle);
    }

    // spec: SOC-043
    #[test]
    fn soc_043_cancel_leaves_the_edit_form_and_changes_nothing() {
        let old = existing(Network::Bluesky, "https://cancel.soc043.test", "ada.cancel", "Ada", "keep-me");
        let accounts = vec![old.clone()];
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        frame(&ctx, &mut t, vec![], &mut d, &accounts);
        click(&ctx, &mut t, "edit:0", &mut d, &accounts);
        d.secret = "typed".into();
        assert_eq!(click(&ctx, &mut t, "edit-cancel", &mut d, &accounts), None);
        assert!(d.editing.is_none() && d.secret.is_empty() && d.handle.is_empty() && d.server.is_empty() && d.error.is_none());
        assert!(d.rects.iter().any(|r| r.0 == "network:mastodon") && !d.rects.iter().any(|r| r.0 == "edit-cancel"), "the add form is back");
        assert_eq!(crate::secrets::social_lookup("https://cancel.soc043.test", "ada.cancel").as_ref().map(|s| s.expose()), Some("keep-me"));
        crate::secrets::social_clear("https://cancel.soc043.test", "ada.cancel");
    }

    // spec: SOC-043
    #[test]
    fn soc_043_a_mastodon_token_of_another_handle_is_refused_and_the_account_s_own_replaces_the_secret() {
        if !has_curl() {
            return;
        }
        let server = super::super::fake_server::FakeServer::start();
        server.state.lock().unwrap().tokens.insert("new-token".into());
        // the server says the token is for "ada@127.0.0.1", the account is bob's
        let bob = existing(Network::Mastodon, &server.address(), "bob@127.0.0.1", "Bob", "bob-old");
        let (ctx, mut t, mut d) = (egui::Context::default(), 0.0, SocialDialog::new());
        let accounts = vec![bob.clone()];
        frame(&ctx, &mut t, vec![], &mut d, &accounts);
        click(&ctx, &mut t, "edit:0", &mut d, &accounts);
        assert_eq!((d.network, d.server.as_str()), (Network::Mastodon, server.address().as_str()));
        click(&ctx, &mut t, "use-token", &mut d, &accounts);
        d.secret = "new-token".into();
        assert!(click_and_wait(&ctx, &mut t, "sign-in", &mut d).is_none(), "refused");
        let msg = d.error.clone().unwrap_or_default();
        assert!(msg.contains("ada@127.0.0.1") && msg.contains("bob@127.0.0.1") && msg.contains("Nothing was changed"), "{msg}");
        assert_eq!(crate::secrets::social_lookup(&bob.account.server, "bob@127.0.0.1").as_ref().map(|s| s.expose()), Some("bob-old"), "the old secret stays");
        assert_eq!(crate::secrets::social_lookup(&bob.account.server, "ada@127.0.0.1"), None, "nothing is stored for the other handle");
        crate::secrets::social_clear(&bob.account.server, "bob@127.0.0.1");
        // the account is ada's: the same token is fine
        let ada = existing(Network::Mastodon, &server.address(), "ada@127.0.0.1", "Ada Own Name", "ada-old");
        let accounts = vec![ada.clone()];
        d.cancel_edit();
        frame(&ctx, &mut t, vec![], &mut d, &accounts);
        click(&ctx, &mut t, "edit:0", &mut d, &accounts);
        click(&ctx, &mut t, "use-token", &mut d, &accounts);
        d.secret = "new-token".into();
        let Some(DialogAction::Updated(e)) = click_and_wait(&ctx, &mut t, "sign-in", &mut d) else { panic!("{:?}", d.error) };
        assert_eq!(e.account, ada.account, "the display name of the account stays, not the server's \"Ada L.\"");
        assert_eq!(crate::secrets::social_lookup(&ada.account.server, "ada@127.0.0.1").as_ref().map(|s| s.expose()), Some("new-token"));
        crate::secrets::social_clear(&ada.account.server, "ada@127.0.0.1");
    }
}
