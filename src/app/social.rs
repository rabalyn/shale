//! The feed view in the app: showing it, the accounts dialog, and saved posts going to the
//! Import dialog.

use super::{Modal, ShaleApp, View};
use crate::social::accounts_ui::DialogAction;
use crate::social::feed_ui::{self, Ask, TabCtx};
use crate::social::post_index::PostHit;
use crate::social::state::FeedsState;
use crate::social::FeedRef;
use eframe::egui::{Context, Ui};
use std::path::PathBuf;
use std::time::{Duration, Instant};

impl ShaleApp {
    /// The Feeds tab on screen: `feed` is what its view says.
    pub fn social_view(&mut self, ui: &mut Ui, feed: Option<FeedRef>) {
        let accounts = self.cfg.social_accounts.clone();
        let tab = TabCtx { id: self.tabbar.tabs.get(self.tabbar.active).map(|t| t.id).unwrap_or(0), feed, last: self.cfg.social_last.clone(), keys: self.modal.is_none() };
        let font = self.fonts.area(crate::fonts::Area::Social);
        for a in crate::fonts::with_area(ui, &font, |ui| feed_ui::show(ui, &mut self.social, &accounts, &tab)) {
            match a {
                Ask::Accounts => self.open_social_accounts(),
                Ask::Pick { feed, new_tab } => self.open_feed(feed, new_tab),
                Ask::Resolve(feed) => self.set_tab_feed(feed),
            }
        }
    }

    /// Show a feed the user chose: in this tab (a step in its history) or in a new tab.
    pub fn open_feed(&mut self, feed: FeedRef, new_tab: bool) {
        self.note_feed(&feed);
        self.show_view(View::Social(Some(feed)), new_tab);
        self.remember_open_files();
    }

    /// Open a post the search found in its feed, in a new Feeds tab scrolled to the post (SOC-035).
    /// A feed that is gone shows the account's other feed (the post goes into its list from the index).
    pub fn open_post_hit(&mut self, hit: &PostHit) {
        if !self.cfg.social_accounts.iter().any(|e| e.account.id == hit.account) {
            self.toast("The account of this post is no longer here");
            return;
        }
        self.open_feed(FeedRef { account: hit.account.clone(), feed: hit.feed.clone() }, true);
        let tab = self.tabbar.tabs[self.tabbar.active].id;
        // a post that neither the index nor the list holds any more is fetched first (SOC-039)
        let (key, feed) = (crate::social::state::post_key(&hit.post), (hit.account.clone(), hit.feed.clone()));
        let in_list = self.social.timelines.get(&feed).is_some_and(|t| t.posts.iter().chain(&t.waiting).any(|p| crate::social::state::post_key(p) == key));
        if in_list || self.social.index.contains(&hit.account, &key) {
            self.social.show_hit(tab, hit);
        } else {
            self.social.fetch_wanted.push((tab, hit.account.clone(), hit.feed.clone(), hit.post.id.clone()));
        }
    }

    /// The "Post search index" section of Settings → General (SOC-040): how many posts are
    /// indexed, Rebuild, and Clear with a question first.
    pub fn post_index_settings(&mut self, ui: &mut Ui) {
        ui.label(eframe::egui::RichText::new("Post search index").color(crate::theme::MUTED));
        let n = self.social.index.count(None);
        ui.label(super::muted(format!("{n} posts from your feeds can be found by the search. It is a cache and fills again as feeds load.")).size(12.0));
        if self.social.confirm_clear {
            ui.label("Clear the post search index?");
            ui.horizontal(|ui| {
                let clear = ui.button("Clear");
                self.social.rects.push(("index-clear-yes".to_string(), clear.rect));
                let cancel = ui.button("Cancel");
                self.social.rects.push(("index-clear-no".to_string(), cancel.rect));
                if clear.clicked() {
                    self.social.clear_index();
                    self.toast("The post search index is empty");
                } else if cancel.clicked() {
                    self.social.confirm_clear = false;
                }
            });
            return;
        }
        ui.horizontal(|ui| {
            let rebuild = ui.button("Rebuild").on_hover_text("Clear it and index again the posts your open feeds hold now");
            self.social.rects.push(("index-rebuild".to_string(), rebuild.rect));
            let clear = ui.button("Clear…");
            self.social.rects.push(("index-clear".to_string(), clear.rect));
            if rebuild.clicked() {
                let n = self.social.rebuild_index();
                self.toast(format!("Post search index rebuilt: {n} posts"));
            } else if clear.clicked() {
                self.social.confirm_clear = true;
            }
        });
    }

    /// The tab named no feed (or one that is gone): this is the one it shows. No history step.
    fn set_tab_feed(&mut self, feed: FeedRef) {
        self.note_feed(&feed);
        if let Some(t) = self.tabbar.tabs.get_mut(self.tabbar.active).filter(|t| matches!(t.view, View::Social(_))) {
            t.view = View::Social(Some(feed));
        }
        self.remember_open_files();
    }

    /// Remember the feed shown last (the next tab without a feed starts with it).
    fn note_feed(&mut self, feed: &FeedRef) {
        let now = Some((feed.account.clone(), feed.feed.clone()));
        if !feed.feed.is_empty() && now != self.cfg.social_last {
            self.cfg.social_last = now;
            self.save_config();
        }
    }

    /// The title of a Feeds tab: "Mastodon · Home" (with the handle when two accounts share a
    /// network), "Feeds" while nothing is known yet.
    pub fn social_title(&self, feed: Option<&FeedRef>) -> String {
        let accounts = &self.cfg.social_accounts;
        let Some(f) = feed.filter(|f| !f.feed.is_empty()) else { return "Feeds".into() };
        let Some(e) = accounts.iter().find(|e| e.account.id == f.account) else { return "Feeds".into() };
        let title = match self.social.feeds.get(&f.account) {
            Some(FeedsState::Ready(list)) => list.iter().find(|x| x.id == f.feed).map(|x| x.title.clone()),
            _ => None,
        };
        let Some(title) = title else { return "Feeds".into() };
        let shared = accounts.iter().filter(|x| x.account.network == e.account.network).count() > 1;
        let who = if shared { format!("@{}", e.account.handle) } else { e.account.network.label().to_string() };
        match self.social.waiting_count(&f.account, &f.feed) {
            0 => format!("{who} · {title}"),
            n => format!("{who} · {title} ({n})"),
        }
    }

    /// The feeds of the open Feeds tabs, in tab order (for the state file).
    pub fn open_feed_refs(&self) -> Vec<FeedRef> {
        self.all_tabs().filter_map(|t| if let View::Social(Some(f)) = &t.view { Some(f.clone()) } else { None }).filter(|f| !f.feed.is_empty()).collect()
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
        // the feeds open in tabs look for new posts every few minutes
        self.social.check_every = Duration::from_secs(u64::from(self.cfg.social_check_minutes) * 60);
        let (accounts, open) = (self.cfg.social_accounts.clone(), self.open_feed_refs());
        self.social.tick(ctx, &accounts, &open, Instant::now());
        self.social.start_fetches(ctx, &accounts);
        let free = matches!(self.modal, None | Some(Modal::Import(_)));
        if self.social.saved.is_empty() || !free {
            return;
        }
        let replies = self.cfg.social_replies;
        let items: Vec<_> = std::mem::take(&mut self.social.saved).into_iter().map(|(s, network)| crate::import::social_item(s, network, replies)).collect();
        // a dialog that is open keeps its folder; otherwise the folder used for posts last time
        let zone = if matches!(self.modal, Some(Modal::Import(_))) { None } else { Some((PathBuf::from(if self.vdata.state.last_social_folder.is_empty() { "Social" } else { self.vdata.state.last_social_folder.as_str() }), None)) };
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
                // the tab that is open shows the new account
                if matches!(self.tabbar.tabs.get(self.tabbar.active).map(|t| &t.view), Some(View::Social(_))) {
                    self.open_feed(FeedRef { account: id, feed: String::new() }, false);
                }
                self.save_config();
                self.toast(format!("Added @{name}"));
                false
            }
            DialogAction::CheckMinutes(m) => {
                self.cfg.social_check_minutes = m;
                self.save_config();
                false
            }
            DialogAction::Remove(id) => {
                if let Some(e) = self.cfg.social_accounts.iter().find(|x| x.account.id == id).cloned() {
                    // the keyring may be slow to answer
                    crate::jobs::run(move || crate::secrets::social_clear(&e.account.server, &e.account.handle), |_| {});
                }
                self.cfg.social_accounts.retain(|x| x.account.id != id);
                self.social.forget(&id);
                self.social.index.remove_account(&id);
                self.save_config();
                false
            }
        }
    }
}
