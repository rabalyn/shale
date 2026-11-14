//! The mail view in the app: opening it, the account settings dialog, and what the view asks
//! for (Import to vault, saving an attachment). See `mail_ui.rs` and `mail_view.rs`.

use super::{Modal, ShaleApp, View};
use crate::mail_dialog::{MailDialog, MailDialogAction};
use crate::mail_ui::MailAction;
use crate::secrets;
use crate::vault::rel_to_string;
use eframe::egui;

impl ShaleApp {
    /// Every frame: let the mail work go on (store, syncs, timers) and wake the window for it.
    pub fn mail_tick(&mut self, ctx: &egui::Context) {
        self.mail.set_context(ctx);
        let wanted = self.tabs.get(self.active).is_some_and(|t| t.view == View::Mail) || matches!(self.modal, Some(Modal::MailAccounts(_)));
        if self.mail.tick(&self.cfg, wanted) {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
        for n in std::mem::take(&mut self.mail.notices) {
            self.toast(n);
        }
    }

    pub fn open_mail_settings(&mut self) {
        for a in self.cfg.mail_accounts.clone() {
            self.mail.probe_sign_in(&a);
        }
        self.modal = Some(Modal::MailAccounts(Box::new(MailDialog::new(self.cfg.mail_accounts.clone(), self.cfg.mail_sync_minutes, secrets::keyring_available()))));
    }

    /// Apply an action of the account settings. Returns true when the dialog should close.
    pub fn mail_dialog_action(&mut self, d: &mut MailDialog, a: MailDialogAction) -> bool {
        match a {
            MailDialogAction::Cancel => true,
            MailDialogAction::Test(i) => {
                if let Some(acct) = d.accounts.get(i).cloned() {
                    // for an OAuth2 account the typed value is the client secret
                    let typed = if acct.auth == crate::config::MailAuth::OAuth2 { d.client_secrets.get(&acct.id) } else { d.passwords.get(&acct.id) }.filter(|p| !p.is_empty()).cloned();
                    // use what is in the fields now, even if it is not valid yet to be saved
                    let mut acct = acct;
                    if let Ok(p) = d.ports.get(i).map(|p| p.trim()).unwrap_or("").parse::<u16>() {
                        acct.port = p;
                    }
                    self.mail.test_connection(&acct, typed);
                }
                false
            }
            MailDialogAction::SignIn(i) => {
                if let Some(mut acct) = d.accounts.get(i).cloned() {
                    acct.oauth_client_id = acct.oauth_client_id.trim().to_string();
                    let typed = d.client_secrets.get(&acct.id).filter(|s| !s.is_empty()).cloned();
                    self.mail.start_sign_in(&acct, typed);
                }
                false
            }
            MailDialogAction::CancelSignIn(i) => {
                if let Some(a) = d.accounts.get(i) {
                    self.mail.cancel_sign_in(&a.id);
                }
                false
            }
            MailDialogAction::ToggleFolder { folder, enabled } => {
                if let Err(e) = self.mail.set_folder_sync(folder, enabled) {
                    self.toast(format!("Could not change the folder: {e}"));
                }
                false
            }
            MailDialogAction::Done => {
                let minutes = match d.validate() {
                    Ok(m) => m,
                    Err(e) => {
                        d.error = Some(e);
                        return false;
                    }
                };
                self.cfg.mail_accounts = d.accounts.clone();
                self.cfg.mail_sync_minutes = minutes;
                for id in std::mem::take(&mut d.removed) {
                    self.mail.forget_account(&id);
                    self.toast(format!("The account was removed. Its mail already copied stays in {}.", self.mail.root.display()));
                }
                for (id, pw) in std::mem::take(&mut d.passwords) {
                    if !pw.is_empty() && self.cfg.mail_accounts.iter().any(|a| a.id == id) {
                        self.mail.save_password(&id, pw);
                    }
                }
                for (id, secret) in std::mem::take(&mut d.client_secrets) {
                    if !secret.is_empty() && self.cfg.mail_accounts.iter().any(|a| a.id == id && a.auth == crate::config::MailAuth::OAuth2) {
                        self.mail.save_client_secret(&id, secret);
                    }
                }
                self.save_config();
                // new accounts are synced right away
                if self.mail.ready_store().is_some() {
                    self.mail.mark_dirty();
                    let cfg = self.cfg.clone();
                    for a in &cfg.mail_accounts {
                        if !self.mail.syncs.contains_key(&a.id) {
                            self.mail.start_sync(&cfg, &a.id);
                        }
                    }
                }
                true
            }
        }
    }

    /// Do what the mail view asked for.
    pub fn mail_action(&mut self, a: MailAction) {
        match a {
            MailAction::OpenAccounts => self.open_mail_settings(),
            MailAction::Toast(t) => self.toast(t),
            MailAction::SaveAttachment { loaded, index } => {
                let (Some(v), Some(att)) = (self.vault.as_ref(), loaded.mail.attachments.get(index)) else {
                    self.toast("Open a vault first to save the attachment into it.");
                    return;
                };
                match crate::media::save_bytes(&v.root, &att.name, &att.data, crate::media::local_date()) {
                    Ok(rel) => {
                        self.toast(format!("Saved as {}", rel_to_string(&rel)));
                        self.actions.push(super::Action::Rescan);
                    }
                    Err(e) => self.toast(format!("Could not save the attachment: {e}")),
                }
            }
            MailAction::Import(msgs) => self.import_mails(msgs),
        }
    }

    /// Hand the stored `.eml` files to the Import dialog, as a dropped `.eml` would arrive
    /// (MAIL-070). Reading happens in a job; nothing is written to the vault here.
    pub fn import_mails(&mut self, msgs: Vec<crate::mail_db::MessageSummary>) {
        let Some(store) = self.mail.ready_store() else { return };
        if self.vault.is_none() {
            self.toast("Open a vault first to import into it.");
            return;
        }
        let ctx = self.ctx.clone();
        let rx = crate::jobs::spawn(
            move || {
                let mut items = Vec::new();
                let mut notes = Vec::new();
                let mut skipped = 0;
                for m in &msgs {
                    if m.state != crate::mail_db::STATE_FULL || m.path.is_empty() {
                        skipped += 1;
                        continue;
                    }
                    match store.read_raw(m) {
                        Ok(raw) => items.push(crate::import::eml_item(raw, None)),
                        Err(e) => notes.push(format!("Could not read “{}”: {e}", if m.subject.trim().is_empty() { "(no subject)" } else { m.subject.as_str() })),
                    }
                }
                if skipped > 0 {
                    notes.push(format!("{skipped} message{} not downloaded, so left out (open {} and press “Load this message” first).", if skipped == 1 { " was" } else { "s were" }, if skipped == 1 { "it" } else { "them" }));
                }
                (items, notes)
            },
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
        self.pending_drops.push((rx, None));
    }
}
