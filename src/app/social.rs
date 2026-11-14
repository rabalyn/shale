//! The feed view in the app: showing it, the accounts dialog, and saved posts going to the
//! Import dialog.

use super::{Modal, ShaleApp};
use crate::social::accounts_ui::DialogAction;
use crate::social::feed_ui::{self, Ask};
use eframe::egui::{Context, Ui};
use std::path::PathBuf;

impl ShaleApp {
    pub fn social_view(&mut self, ui: &mut Ui) {
        let accounts = self.cfg.social_accounts.clone();
        let last = self.cfg.social_last.clone();
        for a in feed_ui::show(ui, &mut self.social, &accounts, last.as_ref()) {
            match a {
                Ask::Accounts => self.open_social_accounts(),
            }
        }
        // remember the feed shown last
        let now = self.social.selection();
        if now.is_some() && now != self.cfg.social_last {
            self.cfg.social_last = now;
            self.save_config();
        }
    }

    pub fn open_social_accounts(&mut self) {
        self.modal = Some(Modal::Social(Box::default()));
    }

    /// Every frame: collect finished jobs, show their messages, and open the Import dialog for
    /// posts that were fetched for saving (when no other dialog is in the way).
    pub fn social_tick(&mut self, ctx: &Context) {
        if self.social.poll() {
            ctx.request_repaint();
        }
        for m in std::mem::take(&mut self.social.messages) {
            self.toast(m);
        }
        let free = matches!(self.modal, None | Some(Modal::Import(_)));
        if self.social.saved.is_empty() || !free {
            return;
        }
        let replies = self.cfg.social_replies;
        let items: Vec<_> = std::mem::take(&mut self.social.saved).into_iter().map(|(s, network)| crate::import::social_item(s, network, replies)).collect();
        // a dialog that is open keeps its folder; otherwise the folder used for posts last time
        let zone = if matches!(self.modal, Some(Modal::Import(_))) { None } else { Some((PathBuf::from(if self.cfg.last_social_folder.is_empty() { "Social" } else { self.cfg.last_social_folder.as_str() }), None)) };
        self.open_import(items, vec![], zone);
    }

    /// What the accounts dialog asked for. True when it should close.
    pub fn social_dialog_action(&mut self, a: DialogAction) -> bool {
        match a {
            DialogAction::Close => true,
            DialogAction::Added(e) => {
                let id = e.account.id.clone();
                let name = e.account.handle.clone();
                // the same account again: new secret, new login
                self.cfg.social_accounts.retain(|x| x.account.id != id);
                self.cfg.social_accounts.push(e);
                self.social.forget(&id);
                self.social.account = Some(id);
                self.save_config();
                self.toast(format!("Added @{name}"));
                false
            }
            DialogAction::Remove(id) => {
                if let Some(e) = self.cfg.social_accounts.iter().find(|x| x.account.id == id).cloned() {
                    // the keyring may be slow to answer
                    std::thread::spawn(move || crate::secrets::social_clear(&e.account.server, &e.account.handle));
                }
                self.cfg.social_accounts.retain(|x| x.account.id != id);
                self.social.forget(&id);
                self.save_config();
                false
            }
        }
    }
}
