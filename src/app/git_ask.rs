//! The question about a folder that has its own `.git` (SYNC-076, rabalyn/shale#585): when such a
//! folder is opened or added as a vault, Shale asks whether to use git sync with it, and runs no
//! sync before the answer.

use super::ShaleApp;
use crate::gitsync;
use eframe::egui;
use std::path::PathBuf;

/// The open vault has a repository that Shale did not make and has not asked about.
pub(crate) struct RepoAsk {
    pub root: PathBuf,
    /// Esc or the ✕: the dialog is gone for now, there is still no sync, and the question comes
    /// again when the vault is opened next time.
    pub later: bool,
}

/// What the person answered.
enum Answer {
    Use,
    Dont,
    Later,
}

impl ShaleApp {
    /// A vault was opened: ask when it is a repository of its own that Shale does not know yet.
    /// `known` false (the user guide, the demo vault) never asks.
    pub(crate) fn ask_about_existing_repo(&mut self, known: bool) {
        self.sync.repo_ask = None;
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        if known && self.persist_config && gitsync::is_repo(&root) && !gitsync::known::is_known(&root) {
            self.sync.repo_ask = Some(RepoAsk { root, later: false });
        }
    }

    /// The dialog is on screen: the keyboard belongs to it.
    pub(crate) fn repo_ask_shown(&self) -> bool {
        self.sync.repo_ask.as_ref().is_some_and(|a| !a.later)
    }

    /// Every frame: show the question, apply the answer.
    pub(crate) fn repo_ask_tick(&mut self, ctx: &egui::Context) {
        if !self.repo_ask_shown() {
            return;
        }
        let mut answer = None;
        let resp = crate::dialog::Dialog::new("existing-repo", "Use git sync?").width(480.0).show(ctx, |ui| {
            crate::theme::paragraph(ui, "This folder is a git repository.");
            crate::theme::paragraph(ui, "If you use git sync, Shale turns on auto sync: files that are not committed yet are committed automatically, and pushed when the repository has a remote.");
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Use git sync").strong()).on_hover_text("Keep auto sync on").clicked() {
                    answer = Some(Answer::Use);
                }
                if ui.button("Don't sync").on_hover_text("Turn auto sync off for this vault; you can turn it on in Settings → Sync").clicked() {
                    answer = Some(Answer::Dont);
                }
            });
            crate::theme::hint(ui, "Esc: decide later. Nothing is synced until you answer, and Shale asks again the next time you open this vault.");
        });
        if answer.is_none() && resp.should_close() {
            answer = Some(Answer::Later);
        }
        let Some(answer) = answer else { return };
        let Some(ask) = self.sync.repo_ask.as_mut() else { return };
        match answer {
            Answer::Later => ask.later = true,
            Answer::Use | Answer::Dont => {
                let root = ask.root.clone();
                self.sync.repo_ask = None;
                if matches!(answer, Answer::Dont) {
                    self.vdata.settings.sync.enabled = false;
                    if let Err(e) = self.vdata.save() {
                        self.toast(e);
                    }
                }
                if let Err(e) = gitsync::known::mark_known(&root) {
                    self.toast(format!("Could not remember the answer: {e}"));
                }
                // the first sync is due now
                self.sync.last_start = None;
            }
        }
        ctx.request_repaint();
    }
}
