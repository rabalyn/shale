//! The mail account settings (MAIL-040…MAIL-043): add, edit and remove accounts, enter the
//! password, test the connection and choose which folders are synced.

use crate::app::muted;
use crate::config::{MailAccount, MailAuth, MailSecurity};
use crate::imap_sync::{kind_of, FolderKind};
use crate::mail_ui::{MailView, TestState};
use crate::theme;
use eframe::egui::{self, Rect, RichText, Ui};
use std::collections::HashMap;
use std::path::Path;

pub struct MailDialog {
    pub accounts: Vec<MailAccount>,
    /// The port of each account as typed.
    pub ports: Vec<String>,
    pub sel: Option<usize>,
    /// Passwords typed this time, by account id; saved when the dialog is closed with Done.
    pub passwords: HashMap<String, String>,
    /// Ids of accounts that were removed (their keyring entries are cleared with Done).
    pub removed: Vec<String>,
    pub minutes: String,
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
    /// Tick or untick a folder.
    ToggleFolder {
        folder: i64,
        enabled: bool,
    },
}

impl MailDialog {
    pub fn new(accounts: Vec<MailAccount>, minutes: u32, keyring: bool) -> MailDialog {
        let ports = accounts.iter().map(|a| a.port.to_string()).collect();
        let sel = if accounts.is_empty() { None } else { Some(0) };
        MailDialog { accounts, ports, sel, passwords: HashMap::new(), removed: vec![], minutes: minutes.to_string(), error: None, keyring, rects: vec![] }
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
        self.sel = Some(self.accounts.len() - 1);
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.accounts.len() {
            let a = self.accounts.remove(i);
            self.ports.remove(i);
            self.removed.push(a.id);
            self.sel = if self.accounts.is_empty() { None } else { Some(i.min(self.accounts.len() - 1)) };
        }
    }

    /// Check what was typed and put the ports into the accounts; the reason if something is wrong.
    pub fn validate(&mut self) -> Result<u32, String> {
        for (a, p) in self.accounts.iter_mut().zip(&self.ports) {
            let label = if a.name.trim().is_empty() { a.host.clone() } else { a.name.clone() };
            if a.host.trim().is_empty() {
                return Err("Every account needs a server (host name).".into());
            }
            if a.user.trim().is_empty() {
                return Err(format!("“{label}” needs a user name."));
            }
            a.port = p.trim().parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("“{label}”: the port must be a number between 1 and 65535."))?;
            a.host = a.host.trim().to_string();
            a.user = a.user.trim().to_string();
            a.name = a.name.trim().to_string();
        }
        self.minutes.trim().parse::<u32>().map_err(|_| "“Sync every” must be a whole number of minutes (0 = never).".to_string())
    }
}

/// The dialog's content.
pub fn show(ui: &mut Ui, d: &mut MailDialog, mail: &MailView) -> Option<MailDialogAction> {
    let mut rects: Vec<(String, Rect)> = Vec::new();
    let mut out = None;
    ui.set_width(720.0);
    ui.label(RichText::new("Mail accounts").size(18.0).color(theme::TEXT_STRONG));
    ui.label(muted("Shale reads your mail over IMAP and keeps a local copy. It never changes anything on the server. The local copy is not encrypted: use full-disk encryption.").size(12.5));
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
            ui.add_enabled(false, egui::RadioButton::new(false, "OAuth2")).on_disabled_hover_text("coming in a later step");
            ui.label(muted("coming in a later step").size(12.0));
        });
        ui.end_row();
        ui.label("Password");
        let pw = d.passwords.entry(id.clone()).or_default();
        let r = ui.add(egui::TextEdit::singleline(pw).password(true).hint_text("leave empty to keep the saved one").desired_width(300.0));
        rects.push(("md:password".into(), r.rect));
        ui.end_row();
    });
    ui.label(
        muted(if d.keyring {
            "The password is kept in the system keyring, never in the settings file."
        } else {
            "No system keyring was found (install libsecret / secret-tool to use one). The password is not saved anywhere: Shale asks for it again each time it starts and keeps it in memory only."
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
