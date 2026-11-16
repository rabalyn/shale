//! The Data usage dialog in the window (USE): opening it, what its buttons do, the daily
//! cleanup and what a finished clear means for what the window holds in memory.

use super::{LeftPanel, Modal, ShaleApp};
use crate::data_usage::dialog::Ask;
use crate::data_usage::{finished_text, CacheId, Finished, Kind};
use std::path::Path;
use std::time::{Duration, Instant};

impl ShaleApp {
    /// A function that wakes the window (for the jobs).
    fn usage_wake(&self) -> impl Fn() + Send + Clone + 'static {
        let ctx = self.ctx.clone();
        move || {
            if let Some(c) = &ctx {
                c.request_repaint();
            }
        }
    }

    /// Open the dialog and count again (USE-001, USE-005).
    pub fn open_data_usage(&mut self) {
        self.usage.places.vault = self.vault.as_ref().map(|v| v.root.clone());
        let wake = self.usage_wake();
        self.usage.refresh(wake);
        self.modal = Some(Modal::DataUsage);
    }

    /// What a button of the dialog asked for; true when the dialog closes.
    pub fn usage_ask(&mut self, a: Ask) -> bool {
        match a {
            Ask::Refresh => {
                let wake = self.usage_wake();
                self.usage.refresh(wake);
                false
            }
            Ask::Clear(id) => {
                self.usage_start(id, Kind::Clear);
                false
            }
            Ask::Changed(id) => {
                self.usage.setting_changed(id);
                self.save_config();
                false
            }
            Ask::Open(rel) => {
                self.open(&rel, false);
                true
            }
            Ask::ShowFolder(rel) => {
                self.show_folder(&rel);
                true
            }
        }
    }

    /// Show a folder of the vault in the Files panel, opened.
    fn show_folder(&mut self, rel: &Path) {
        self.left = LeftPanel::Files;
        self.cfg.show_left = true;
        let mut p = Some(rel);
        while let Some(d) = p.filter(|d| !d.as_os_str().is_empty()) {
            self.tree.expanded.insert(d.to_path_buf());
            p = d.parent();
        }
    }

    /// Start a clear or prune of one cache. The mail index is handed over by the mail view,
    /// which refuses while it syncs or sends (USE-012).
    pub fn usage_start(&mut self, id: CacheId, kind: Kind) {
        if self.usage.running(id) {
            return;
        }
        let mail = if id == CacheId::MailIndex && kind == Kind::Clear {
            match self.mail.begin_rebuild() {
                Ok(s) => Some(s),
                Err(why) => {
                    let f = Finished { id, kind, result: Err(format!("not now: {why}")) };
                    self.usage.record.note(&f, crate::calendar::time::now());
                    self.toast(finished_text(&f));
                    return;
                }
            }
        } else {
            None
        };
        let wake = self.usage_wake();
        let retention = self.cfg.caches.clone();
        self.usage.start(id, kind, &retention, mail, wake);
    }

    /// The live owners of the caches follow the retention settings (USE-020).
    fn apply_retention(&mut self) {
        let r = &self.cfg.caches;
        self.social.index.keep_days = r.post_days.map(i64::from);
        self.social.index.keep_posts = r.posts_per_account.map(|n| n as usize);
        if let Some(s) = self.mail.ready_store() {
            s.set_backups_kept(r.mail_backups);
        }
    }

    /// Every frame: collect finished jobs, start the cleanup that is due (USE-023).
    pub fn usage_tick(&mut self) {
        self.usage.places.vault = self.vault.as_ref().map(|v| v.root.clone());
        self.apply_retention();
        let wake = self.usage_wake();
        for f in self.usage.poll(wake) {
            self.usage_finished(f);
        }
        if let Some(p) = self.usage.problem.take() {
            self.toast(p);
        }
        for id in self.usage.due(Instant::now(), crate::calendar::time::now()) {
            self.usage_start(id, Kind::Prune);
        }
        if self.usage.any_running() {
            if let Some(c) = &self.ctx {
                c.request_repaint_after(Duration::from_millis(250));
            }
        }
    }

    /// What a finished clear or prune means for what the window holds, and the message: always
    /// for a clear and a failure, for a prune only when it removed something.
    fn usage_finished(&mut self, f: Finished) {
        match (f.id, f.kind) {
            (CacheId::SocialPictures, _) => {
                if let Some(i) = &self.social.images {
                    i.forget_all();
                }
            }
            (CacheId::TagWords, Kind::Clear) => {
                // learn again at once (USE-015)
                self.tag_learner.forget();
            }
            (CacheId::MailIndex, Kind::Clear) => {
                let cfg = self.cfg.clone();
                self.mail.end_rebuild(&cfg);
            }
            _ => {}
        }
        let say = f.kind == Kind::Clear || f.result.as_ref().map(|r| r.entries > 0).unwrap_or(true);
        if say {
            self.toast(finished_text(&f));
        }
    }
}
