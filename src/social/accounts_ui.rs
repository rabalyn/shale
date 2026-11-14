//! The "Feed accounts" dialog: add a Mastodon or Bluesky account, remove one.
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
}

type JobResult = Result<Done, String>;

pub struct SocialDialog {
    pub network: Network,
    /// Mastodon server / Bluesky server
    pub server: String,
    /// Bluesky handle
    pub handle: String,
    /// Bluesky app password / Mastodon access token
    pub secret: String,
    /// Mastodon: the code from the sign-in page
    pub code: String,
    /// Mastodon: sign in with an access token instead of the browser
    pub use_token: bool,
    app: Option<(String, AppCredentials)>,
    job: Option<Receiver<JobResult>>,
    pub error: Option<String>,
    pub rects: Vec<(String, Rect)>,
}

#[derive(Debug, PartialEq)]
pub enum DialogAction {
    Close,
    /// a new account signed in (its secret is already in the keyring or the entry)
    Added(SocialAccountEntry),
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
        SocialDialog { network: Network::Mastodon, server: String::new(), handle: String::new(), secret: String::new(), code: String::new(), use_token: false, app: None, job: None, error: None, rects: vec![] }
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
        let secret = self.secret.trim().to_string();
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
                    let token = match (&app, use_token) {
                        (Some(app), false) => mastodon::exchange_code(&server, app, &code)?,
                        _ => secret,
                    };
                    let who = mastodon::verify(&server, &token)?;
                    Ok(Done::Added(SocialAccountEntry::new(Account::new(Network::Mastodon, &server, &who.handle, &who.display_name), &token)))
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
                    let session = bluesky::create_session(&server, &handle, &secret)?;
                    Ok(Done::Added(SocialAccountEntry::new(Account::new(Network::Bluesky, &server, &session.handle, &session.handle), &secret)))
                }));
            }
        }
    }

    /// Take the result of the running step. An added account is returned.
    fn poll(&mut self) -> Option<SocialAccountEntry> {
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
                return Some(e);
            }
            Err(e) => self.error = Some(e),
        }
        None
    }
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
    if let Some(e) = d.poll() {
        action = Some(DialogAction::Added(e));
    }
    if d.busy() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
    ui.set_width(560.0);
    ui.label(egui::RichText::new("Feed accounts").size(18.0).color(theme::TEXT_STRONG));
    ui.add_space(4.0);
    // ---- the list
    if accounts.is_empty() {
        ui.label(muted("No account yet. Add a Mastodon or Bluesky account below to read its feeds here."));
    }
    for (i, e) in accounts.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(e.account.network.label()).size(11.0).color(theme::ACCENT));
            let name = if e.account.display_name.trim().is_empty() { e.account.handle.clone() } else { format!("{} (@{})", e.account.display_name, e.account.handle) };
            ui.label(name);
            ui.label(muted(&e.account.server));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if rec(format!("remove:{i}"), ui.small_button("Remove")).on_hover_text("Forget this account and its stored password").clicked() {
                    action = Some(DialogAction::Remove(e.account.id.clone()));
                }
            });
        });
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(muted("Look for new posts every"));
        let mut m = check_minutes;
        if rec("check-minutes".into(), ui.add(egui::DragValue::new(&mut m).range(0..=240).suffix(" min"))).on_hover_text("0 = never. Only feeds open in a tab are checked, and only while Shale runs.").changed() && action.is_none() {
            action = Some(DialogAction::CheckMinutes(m));
        }
    });
    ui.separator();
    // ---- add
    ui.label(egui::RichText::new("Add an account").size(14.0).color(theme::TEXT));
    ui.horizontal(|ui| {
        for n in [Network::Mastodon, Network::Bluesky] {
            if rec(format!("network:{}", n.key()), ui.selectable_label(d.network == n, n.label())).clicked() && d.network != n {
                d.network = n;
                d.error = None;
                d.secret.clear();
                d.code.clear();
            }
        }
    });
    ui.add_space(2.0);
    let busy = d.busy();
    match d.network {
        Network::Mastodon => {
            egui::Grid::new("social-mastodon").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(muted("Server"));
                rec("server".into(), ui.add(egui::TextEdit::singleline(&mut d.server).hint_text("mastodon.social").desired_width(380.0)));
                ui.end_row();
                if d.use_token {
                    ui.label(muted("Access token"));
                    rec("token".into(), ui.add(egui::TextEdit::singleline(&mut d.secret).password(true).hint_text("created in Preferences → Development").desired_width(380.0)));
                    ui.end_row();
                } else {
                    ui.label(muted("Step 1"));
                    ui.horizontal(|ui| {
                        if rec("open-browser".into(), ui.add_enabled(!busy, egui::Button::new("Open sign-in page in browser")))
                            .on_hover_text("Shale asks the server for a read-only sign-in; you log in and allow it on the server's own page")
                            .clicked()
                        {
                            d.open_sign_in(ui.ctx());
                        }
                        if d.app.is_some() {
                            ui.label(egui::RichText::new("opened").size(12.0).color(theme::MUTED));
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
            ui.label(muted("Shale only asks for permission to read. The code or token is not kept; the access token goes to your system keyring."));
        }
        Network::Bluesky => {
            egui::Grid::new("social-bluesky").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label(muted("Handle"));
                rec("handle".into(), ui.add(egui::TextEdit::singleline(&mut d.handle).hint_text("you.bsky.social").desired_width(380.0)));
                ui.end_row();
                ui.label(muted("App password"));
                rec("app-password".into(), ui.add(egui::TextEdit::singleline(&mut d.secret).password(true).hint_text("xxxx-xxxx-xxxx-xxxx").desired_width(380.0)));
                ui.end_row();
                ui.label(muted("Server"));
                rec("server".into(), ui.add(egui::TextEdit::singleline(&mut d.server).hint_text(BLUESKY_SERVER).desired_width(380.0)));
                ui.end_row();
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Use an app password, never your main password: Bluesky → Settings → Privacy and security → App passwords. It can be revoked there at any time. It goes to your system keyring.")
                        .size(12.0)
                        .color(theme::LINK_MISSING.gamma_multiply(0.85)),
                )
                .wrap(),
            );
        }
    }
    if let Some(e) = &d.error {
        ui.add(egui::Label::new(egui::RichText::new(e).color(theme::LINK_MISSING)).wrap());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if rec("sign-in".into(), ui.add_enabled(!busy, egui::Button::new(egui::RichText::new("Sign in").strong()))).clicked() {
            d.sign_in(ui.ctx());
        }
        if busy {
            ui.spinner();
            ui.label(muted("Talking to the server…"));
        }
        if rec("close".into(), ui.button("Close")).clicked() {
            action = Some(DialogAction::Close);
        }
    });
    d.rects = rects;
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
                res = show(ui, d, accounts, 5);
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
        let acct = SocialAccountEntry { account: Account::new(Network::Bluesky, BLUESKY_SERVER, "ada.bsky.social", "ada.bsky.social"), secret: String::new() };
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
}
