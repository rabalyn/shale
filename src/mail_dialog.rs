//! The mail account settings (MAIL-040…MAIL-043): add, edit and remove accounts, enter the
//! password, test the connection and choose which folders are synced.

use crate::app::muted;
use crate::config::{MailAccount, MailAuth, MailSecurity, OAuthProvider};
use crate::imap_sync::{kind_of, FolderKind};
use crate::mail_ui::{MailView, SignInState, TestState};
use crate::theme;
use eframe::egui::{self, Rect, RichText, Ui};
use std::collections::HashMap;
use std::path::Path;

pub struct MailDialog {
    pub accounts: Vec<MailAccount>,
    /// The port of each account as typed.
    pub ports: Vec<String>,
    /// The SMTP port of each account as typed (MAIL-120).
    pub smtp_ports: Vec<String>,
    /// Seconds a sent mail waits in the outbox, as typed (MAIL-134).
    pub undo_secs: String,
    pub sel: Option<usize>,
    /// Passwords typed this time, by account id; saved when the dialog is closed with Done.
    pub passwords: HashMap<String, String>,
    /// Client secrets typed this time, by account id; saved to the keyring with Done.
    pub client_secrets: HashMap<String, String>,
    /// Ids of accounts that were removed (their keyring entries are cleared with Done).
    pub removed: Vec<String>,
    pub minutes: String,
    /// "Mark as read when opened" and its delay in seconds as typed (MAIL-109).
    pub mark_read: bool,
    pub mark_read_secs: String,
    pub error: Option<String>,
    /// Is there a system keyring to keep passwords in?
    pub keyring: bool,
    pub rects: Vec<(String, Rect)>,
}

#[derive(Debug, PartialEq)]
pub enum MailDialogAction {
    Done,
    Cancel,
    /// Test the connection of account `i` (with the password typed, if any).
    Test(usize),
    /// "Sign in in the browser" for account `i`.
    SignIn(usize),
    /// Stop waiting for the browser of account `i`.
    CancelSignIn(usize),
    /// Tick or untick a folder.
    ToggleFolder {
        folder: i64,
        enabled: bool,
    },
}

impl MailDialog {
    pub fn new(accounts: Vec<MailAccount>, minutes: u32, keyring: bool) -> MailDialog {
        let ports = accounts.iter().map(|a| a.port.to_string()).collect();
        let smtp_ports = accounts.iter().map(|a| a.smtp_port.to_string()).collect();
        let sel = if accounts.is_empty() { None } else { Some(0) };
        MailDialog {
            accounts,
            ports,
            smtp_ports,
            undo_secs: "10".into(),
            sel,
            passwords: HashMap::new(),
            client_secrets: HashMap::new(),
            removed: vec![],
            minutes: minutes.to_string(),
            mark_read: true,
            mark_read_secs: "2".into(),
            error: None,
            keyring,
            rects: vec![],
        }
    }

    /// The "mark as read when opened" settings the dialog starts with.
    pub fn with_read_settings(mut self, on: bool, secs: u32) -> MailDialog {
        self.mark_read = on;
        self.mark_read_secs = secs.to_string();
        self
    }

    /// The undo-send time the dialog starts with.
    pub fn with_undo_secs(mut self, secs: u32) -> MailDialog {
        self.undo_secs = secs.to_string();
        self
    }

    /// The undo-send time in seconds, if it is a number.
    pub fn undo_delay(&self) -> Result<u32, String> {
        self.undo_secs.trim().parse::<u32>().ok().filter(|s| *s <= 600).ok_or_else(|| "“Undo send” must be a whole number of seconds (0 = send at once, at most 600).".to_string())
    }

    /// The delay as a number of seconds, if it is one.
    pub fn read_delay(&self) -> Result<u32, String> {
        self.mark_read_secs.trim().parse::<u32>().ok().filter(|s| *s <= 3600).ok_or_else(|| "“Mark as read after” must be a whole number of seconds (0 = at once, at most 3600).".to_string())
    }

    /// An id for a new account: letters and digits, not used by an account that exists or was
    /// removed in this dialog (its local mail would be mixed up with the new account's).
    pub fn fresh_id(&self, store_root: &Path) -> String {
        (1..).map(|n| format!("mail{n}")).find(|id| !self.accounts.iter().any(|a| &a.id == id) && !self.removed.contains(id) && !store_root.join(id).exists()).unwrap_or_default()
    }

    pub fn add(&mut self, store_root: &Path) {
        let id = self.fresh_id(store_root);
        self.accounts.push(MailAccount { id, ..MailAccount::default() });
        self.ports.push("993".into());
        self.smtp_ports.push("465".into());
        self.sel = Some(self.accounts.len() - 1);
    }

    /// Choosing OAuth2 or a provider: the server and port of the preset, unless the account
    /// already has a server of its own.
    pub fn use_preset_server(&mut self, i: usize) {
        let a = &mut self.accounts[i];
        let presets = [OAuthProvider::Google, OAuthProvider::Microsoft].map(crate::oauth::preset_server);
        let is_preset_host = a.host.trim().is_empty() || presets.iter().flatten().any(|(h, _)| *h == a.host.trim());
        if let (true, Some((host, port))) = (is_preset_host, crate::oauth::preset_server(a.oauth_provider)) {
            a.host = host.to_string();
            a.port = port;
            a.security = MailSecurity::ImplicitTls;
            self.ports[i] = port.to_string();
        }
        // sending: the preset's SMTP server, unless the account has one of its own (MAIL-120)
        let smtp_presets = [OAuthProvider::Google, OAuthProvider::Microsoft].map(crate::oauth::preset_smtp);
        let a = &mut self.accounts[i];
        let is_preset_smtp = a.smtp_host.trim().is_empty() || smtp_presets.iter().flatten().any(|(h, ..)| *h == a.smtp_host.trim());
        if let (true, Some((host, port, sec, stores))) = (is_preset_smtp, crate::oauth::preset_smtp(a.oauth_provider)) {
            a.smtp_host = host.to_string();
            a.smtp_port = port;
            a.smtp_security = sec;
            a.server_stores_sent = stores;
            self.smtp_ports[i] = port.to_string();
        }
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.accounts.len() {
            let a = self.accounts.remove(i);
            self.ports.remove(i);
            self.smtp_ports.remove(i);
            self.removed.push(a.id);
            self.sel = if self.accounts.is_empty() { None } else { Some(i.min(self.accounts.len() - 1)) };
        }
    }

    /// Check what was typed and put the ports into the accounts; the reason if something is wrong.
    pub fn validate(&mut self) -> Result<u32, String> {
        for ((a, p), sp) in self.accounts.iter_mut().zip(&self.ports).zip(&self.smtp_ports) {
            let label = if a.name.trim().is_empty() { a.host.clone() } else { a.name.clone() };
            if a.host.trim().is_empty() {
                return Err("Every account needs a server (host name).".into());
            }
            if a.user.trim().is_empty() {
                return Err(format!("“{label}” needs a user name."));
            }
            if a.auth == MailAuth::OAuth2 {
                if a.oauth_client_id.trim().is_empty() {
                    return Err(format!("“{label}” needs the client ID of your OAuth2 app."));
                }
                if a.oauth_provider == OAuthProvider::Custom {
                    crate::oauth::OAuthConfig::for_account(a, None).map_err(|e| format!("“{label}”: {e}"))?;
                }
            }
            a.port = p.trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("“{label}”: the port must be a number between 1 and 65535."))?;
            a.host = a.host.trim().to_string();
            a.user = a.user.trim().to_string();
            a.name = a.name.trim().to_string();
            // sending: optional, but what is typed must make sense
            a.smtp_host = a.smtp_host.trim().to_string();
            a.smtp_port = sp.trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("“{label}”: the SMTP port must be a number between 1 and 65535."))?;
            a.from_name = a.from_name.trim().to_string();
            a.from_address = a.from_address.trim().to_string();
            if !a.from_address.is_empty() {
                crate::compose::sender(&a.from_name, &a.from_address).map_err(|e| format!("“{label}”: {e}"))?;
            }
            if a.from_name.contains(['\r', '\n']) {
                return Err(format!("“{label}”: the sender name may not contain a line break."));
            }
        }
        self.read_delay()?;
        self.undo_delay()?;
        self.minutes.trim().parse::<u32>().map_err(|_| "“Sync every” must be a whole number of minutes (0 = never).".to_string())
    }
}

/// The dialog's content.
pub fn show(ui: &mut Ui, d: &mut MailDialog, mail: &MailView) -> Option<MailDialogAction> {
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut out = None;
    ui.set_width(720.0);
    ui.label(RichText::new("Mail accounts").size(18.0).color(theme::TEXT_STRONG));
    ui.label(
        muted(
            "Shale reads your mail over IMAP and keeps a local copy. Syncing never changes anything on the server; only what you do yourself does (mark read, flag, move, delete). The local copy is not encrypted: use full-disk encryption.",
        )
        .size(12.5),
    );
    ui.add_space(8.0);
    // the form scrolls when the window is small; the buttons below it never do
    let max_h = (ui.ctx().content_rect().height() - 230.0).clamp(200.0, 620.0);
    egui::ScrollArea::vertical().id_salt("mail-dialog-body").max_height(max_h).auto_shrink([true, true]).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            // the list of accounts
            ui.vertical(|ui| {
                ui.set_width(170.0);
                for i in 0..d.accounts.len() {
                    let a = &d.accounts[i];
                    let label = if !a.name.trim().is_empty() {
                        a.name.clone()
                    } else if !a.host.is_empty() {
                        a.host.clone()
                    } else {
                        "New account".to_string()
                    };
                    let r = ui.selectable_label(d.sel == Some(i), label);
                    rects.push((format!("md:acct:{i}"), r.rect));
                    if r.clicked() {
                        d.sel = Some(i);
                    }
                }
                ui.add_space(4.0);
                let r = ui.button("Add account");
                rects.push(("md:add".into(), r.rect));
                if r.clicked() {
                    d.add(&mail.root);
                }
                if let Some(i) = d.sel {
                    let r = ui.button("Remove account");
                    rects.push(("md:remove".into(), r.rect));
                    if r.on_hover_text("The settings and the saved password go; the mail already copied stays on disk").clicked() {
                        d.remove(i);
                    }
                }
            });
            ui.add_space(12.0);
            ui.vertical(|ui| {
                ui.set_width(520.0);
                let Some(i) = d.sel.filter(|i| *i < d.accounts.len()) else {
                    ui.label(muted("Add an account to read your mail here."));
                    return;
                };
                account_form(ui, d, i, mail, &mut rects, &mut out);
            });
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Fetch new mail every");
            let r = ui.add(egui::TextEdit::singleline(&mut d.minutes).desired_width(36.0));
            rects.push(("md:minutes".into(), r.rect));
            ui.label("minutes while Shale runs (0 = only with the Sync button)");
        });
        ui.horizontal(|ui| {
            let r = ui.checkbox(&mut d.mark_read, "Mark as read when opened, after");
            rects.push(("md:mark-read".into(), r.rect));
            let r = ui.add_enabled(d.mark_read, egui::TextEdit::singleline(&mut d.mark_read_secs).desired_width(36.0));
            rects.push(("md:mark-read-secs".into(), r.rect));
            ui.label("seconds (the server is told; 0 = at once)");
        });
        ui.horizontal(|ui| {
            ui.label("Undo send: a sent mail waits");
            let r = ui.add(egui::TextEdit::singleline(&mut d.undo_secs).desired_width(36.0));
            rects.push(("md:undo-secs".into(), r.rect));
            ui.label("seconds in the outbox, where you can take it back (0 = send at once)");
        });
    });
    if let Some(e) = &d.error {
        ui.add_space(4.0);
        ui.label(RichText::new(e.as_str()).color(theme::LINK_MISSING));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let r = ui.button("Done");
        rects.push(("md:done".into(), r.rect));
        if r.clicked() {
            out = Some(MailDialogAction::Done);
        }
        let r = ui.button("Cancel");
        rects.push(("md:cancel".into(), r.rect));
        if r.clicked() {
            out = Some(MailDialogAction::Cancel);
        }
    });
    d.rects = rects;
    out
}

fn account_form(ui: &mut Ui, d: &mut MailDialog, i: usize, mail: &MailView, rects: &mut Vec<(String, Rect)>, out: &mut Option<MailDialogAction>) {
    let id = d.accounts[i].id.clone();
    egui::Grid::new(("mail-form", i)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
        ui.label("Name");
        let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].name).hint_text("shown in the folder list").desired_width(300.0));
        rects.push(("md:name".into(), r.rect));
        ui.end_row();
        ui.label("Server");
        let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].host).hint_text("imap.example.com").desired_width(300.0));
        rects.push(("md:host".into(), r.rect));
        ui.end_row();
        ui.label("Security");
        ui.horizontal(|ui| {
            for (sec, text) in [(MailSecurity::ImplicitTls, "TLS (usually port 993)"), (MailSecurity::StartTls, "STARTTLS (usually port 143)")] {
                let r = ui.radio(d.accounts[i].security == sec, text);
                rects.push((format!("md:sec:{sec:?}"), r.rect));
                if r.clicked() && d.accounts[i].security != sec {
                    // the port follows the mode as long as it is still the usual one of the old mode
                    if d.ports[i].trim() == d.accounts[i].security.default_port().to_string() {
                        d.ports[i] = sec.default_port().to_string();
                    }
                    d.accounts[i].security = sec;
                }
            }
        });
        ui.end_row();
        ui.label("Port");
        let r = ui.add(egui::TextEdit::singleline(&mut d.ports[i]).desired_width(70.0));
        rects.push(("md:port".into(), r.rect));
        ui.end_row();
        ui.label("User name");
        let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].user).desired_width(300.0));
        rects.push(("md:user".into(), r.rect));
        ui.end_row();
        ui.label("Sign in with");
        ui.horizontal(|ui| {
            let r = ui.radio(d.accounts[i].auth == MailAuth::Password, "Password");
            rects.push(("md:auth-password".into(), r.rect));
            if r.clicked() {
                d.accounts[i].auth = MailAuth::Password;
            }
            let r = ui.radio(d.accounts[i].auth == MailAuth::OAuth2, "OAuth2 (in the browser)");
            rects.push(("md:auth-oauth2".into(), r.rect));
            if r.clicked() {
                d.accounts[i].auth = MailAuth::OAuth2;
                d.use_preset_server(i);
            }
        });
        ui.end_row();
        if d.accounts[i].auth == MailAuth::Password {
            ui.label("Password");
            let pw = d.passwords.entry(id.clone()).or_default();
            let r = ui.add(egui::TextEdit::singleline(pw).password(true).hint_text("leave empty to keep the saved one").desired_width(300.0));
            rects.push(("md:password".into(), r.rect));
            ui.end_row();
        } else {
            oauth_rows(ui, d, i, &id, rects);
        }
    });
    // sending (MAIL-120): folded away until it is needed, so the form stays short
    ui.add_space(8.0);
    let open_now = !d.accounts[i].smtp_host.trim().is_empty();
    let head = egui::CollapsingHeader::new(RichText::new("Sending (SMTP)").color(theme::TEXT)).id_salt(("mail-smtp", i)).default_open(open_now).show(ui, |ui| {
        egui::Grid::new(("mail-smtp-form", i)).num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
            ui.label("SMTP server");
            let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].smtp_host).hint_text("smtp.example.com (empty: this account does not send)").desired_width(300.0));
            rects.push(("md:smtp-host".into(), r.rect));
            ui.end_row();
            ui.label("Security");
            ui.horizontal(|ui| {
                for (sec, text) in [(MailSecurity::ImplicitTls, "TLS (usually port 465)"), (MailSecurity::StartTls, "STARTTLS (usually port 587)")] {
                    let r = ui.radio(d.accounts[i].smtp_security == sec, text);
                    rects.push((format!("md:smtp-sec:{sec:?}"), r.rect));
                    if r.clicked() && d.accounts[i].smtp_security != sec {
                        if d.smtp_ports[i].trim() == d.accounts[i].smtp_security.smtp_default_port().to_string() {
                            d.smtp_ports[i] = sec.smtp_default_port().to_string();
                        }
                        d.accounts[i].smtp_security = sec;
                    }
                }
            });
            ui.end_row();
            ui.label("Port");
            let r = ui.add(egui::TextEdit::singleline(&mut d.smtp_ports[i]).desired_width(70.0));
            rects.push(("md:smtp-port".into(), r.rect));
            ui.end_row();
            ui.label("Your name");
            let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].from_name).hint_text("as the recipient sees it").desired_width(300.0));
            rects.push(("md:from-name".into(), r.rect));
            ui.end_row();
            ui.label("Your address");
            let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].from_address).hint_text("empty: the user name, if it is an address").desired_width(300.0));
            rects.push(("md:from-addr".into(), r.rect));
            ui.end_row();
            ui.label("Signature");
            let r = ui.add(egui::TextEdit::multiline(&mut d.accounts[i].signature).desired_rows(3).desired_width(300.0));
            rects.push(("md:signature".into(), r.rect));
            ui.end_row();
        });
        let r = ui.checkbox(&mut d.accounts[i].server_stores_sent, "The server saves sent mail in Sent by itself (Gmail does): Shale then does not add a copy");
        rects.push(("md:server-stores".into(), r.rect));
        let r = ui.checkbox(&mut d.accounts[i].user_agent_header, "Add a User-Agent header (tells the recipient this was written with Shale; off for privacy)");
        rects.push(("md:user-agent".into(), r.rect));
        ui.label(muted("Sending always uses an encrypted connection and the sign-in above (the same password, or the same OAuth2 sign-in). “Test connection” also signs in to the SMTP server; it never sends a mail.").size(12.0));
    });
    rects.push(("md:smtp-header".into(), head.header_response.rect));
    if d.accounts[i].auth == MailAuth::OAuth2 {
        ui.label(muted("Shale needs an OAuth2 client ID that you register yourself with Google or Microsoft (see the README). Shale opens your browser for the sign-in and keeps only a refresh token, in the system keyring.").size(12.0));
        sign_in_row(ui, i, &id, mail, rects, out);
    }
    ui.label(
        muted(if d.keyring {
            "Passwords, client secrets and sign-in tokens are kept in the system keyring, never in the settings file."
        } else {
            "No system keyring was found (install libsecret / secret-tool to use one). A password is not saved anywhere: Shale asks for it again each time it starts and keeps it in memory only. An OAuth2 sign-in lasts until Shale quits."
        })
        .size(12.0),
    );
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let running = matches!(mail.tests.get(&id), Some(TestState::Running));
        let r = ui.add_enabled(!running, egui::Button::new("Test connection"));
        rects.push(("md:test".into(), r.rect));
        if r.clicked() {
            *out = Some(MailDialogAction::Test(i));
        }
        match mail.tests.get(&id) {
            Some(TestState::Running) => {
                ui.label(muted("Connecting…"));
            }
            Some(TestState::Ok(t)) => {
                ui.label(RichText::new(t.as_str()).color(theme::TAG));
            }
            Some(TestState::Failed(e)) => {
                ui.label(RichText::new(e.as_str()).color(theme::LINK_MISSING));
            }
            None => {}
        }
    });
    // which folders are synced
    let folders = mail.account_folders(&id);
    ui.add_space(6.0);
    ui.label(RichText::new("Folders to sync").color(theme::TEXT));
    if folders.is_empty() {
        ui.label(muted("The folders appear after “Test connection” or the first sync.").size(12.0));
        return;
    }
    egui::ScrollArea::vertical().id_salt(("mail-folder-boxes", i)).max_height(170.0).auto_shrink([false, true]).show(ui, |ui| {
        for f in &folders {
            let kind = kind_of(f);
            let never = matches!(kind, FolderKind::Trash | FolderKind::Junk);
            let mut on = f.sync_enabled && !never;
            ui.horizontal(|ui| {
                let (depth, label) = mail.tree_entry(f);
                ui.add_space(depth as f32 * 14.0);
                let r = ui.add_enabled(!never, egui::Checkbox::new(&mut on, label));
                rects.push((format!("md:folder:{}", f.id), r.rect));
                if r.changed() {
                    *out = Some(MailDialogAction::ToggleFolder { folder: f.id, enabled: on });
                }
                match kind {
                    FolderKind::Trash | FolderKind::Junk => {
                        ui.label(muted("never synced automatically; loaded when you open it").size(12.0));
                    }
                    FolderKind::All => {
                        ui.label(muted("every message again: off by default; loaded when you open it").size(12.0));
                    }
                    FolderKind::Normal => {}
                }
            });
        }
    });
}

/// The OAuth2 rows of the form: provider, endpoints (Custom), client ID and secret.
fn oauth_rows(ui: &mut Ui, d: &mut MailDialog, i: usize, id: &str, rects: &mut Vec<(String, Rect)>) {
    ui.label("Provider");
    ui.horizontal(|ui| {
        for p in [OAuthProvider::Google, OAuthProvider::Microsoft, OAuthProvider::Custom] {
            let r = ui.radio(d.accounts[i].oauth_provider == p, p.label());
            rects.push((format!("md:provider:{}", p.label()), r.rect));
            if r.clicked() && d.accounts[i].oauth_provider != p {
                d.accounts[i].oauth_provider = p;
                d.use_preset_server(i);
            }
        }
    });
    ui.end_row();
    if d.accounts[i].oauth_provider == OAuthProvider::Custom {
        ui.label("Authorization address");
        let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].oauth_auth_url).hint_text("https://id.example.com/authorize").desired_width(300.0));
        rects.push(("md:oauth-auth-url".into(), r.rect));
        ui.end_row();
        ui.label("Token address");
        let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].oauth_token_url).hint_text("https://id.example.com/token").desired_width(300.0));
        rects.push(("md:oauth-token-url".into(), r.rect));
        ui.end_row();
        ui.label("Scope");
        let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].oauth_scope).hint_text("mail").desired_width(300.0));
        rects.push(("md:oauth-scope".into(), r.rect));
        ui.end_row();
    }
    ui.label("Client ID");
    let r = ui.add(egui::TextEdit::singleline(&mut d.accounts[i].oauth_client_id).hint_text("from your Google or Microsoft app registration").desired_width(300.0));
    rects.push(("md:oauth-client-id".into(), r.rect));
    ui.end_row();
    ui.label("Client secret");
    let sec = d.client_secrets.entry(id.to_string()).or_default();
    let r = ui.add(egui::TextEdit::singleline(sec).password(true).hint_text("only if the provider asks for one (Google does); empty keeps the saved one").desired_width(300.0));
    rects.push(("md:oauth-client-secret".into(), r.rect));
    ui.end_row();
}

/// The sign-in button with its waiting note, Cancel and the result.
fn sign_in_row(ui: &mut Ui, i: usize, id: &str, mail: &MailView, rects: &mut Vec<(String, Rect)>, out: &mut Option<MailDialogAction>) {
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        let state = mail.signins.get(id);
        let waiting = matches!(state, Some(SignInState::Waiting { .. }));
        let known_in = matches!(state, Some(SignInState::Done { .. })) || mail.signed_in.get(id) == Some(&true);
        let r = ui.add_enabled(!waiting, egui::Button::new(if known_in { "Sign in again" } else { crate::oauth::SIGN_IN_BUTTON }));
        rects.push(("md:signin".into(), r.rect));
        if r.clicked() {
            *out = Some(MailDialogAction::SignIn(i));
        }
        match state {
            Some(SignInState::Waiting { url, .. }) => {
                ui.label(muted("Waiting for the browser… (it is given up after 5 minutes)"));
                let r = ui.button("Cancel");
                rects.push(("md:signin-cancel".into(), r.rect));
                if r.clicked() {
                    *out = Some(MailDialogAction::CancelSignIn(i));
                }
                if let Some(u) = url {
                    let r = ui.button("Copy the sign-in address").on_hover_text("If no browser opened, paste this into one");
                    rects.push(("md:signin-copy".into(), r.rect));
                    if r.clicked() {
                        ui.ctx().copy_text(u.clone());
                    }
                }
            }
            Some(SignInState::Done { user, kept }) => {
                let who = user.as_ref().map(|u| format!(" as {u}")).unwrap_or_default();
                ui.label(RichText::new(format!("Signed in{who}.")).color(theme::TAG));
                if !kept {
                    ui.label(muted("(not saved: no keyring)"));
                }
            }
            Some(SignInState::Failed(e)) => {
                ui.label(RichText::new(e.as_str()).color(theme::LINK_MISSING));
            }
            None => match mail.signed_in.get(id) {
                Some(true) => {
                    ui.label(muted("Signed in. The refresh token is in the keyring."));
                }
                Some(false) => {
                    ui.label(muted("Not signed in."));
                }
                None => {}
            },
        }
    });
    ui.add_space(4.0);
}
