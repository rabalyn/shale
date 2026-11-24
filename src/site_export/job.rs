//! The publish in the window (SITE-065, SITE-072, SITE-074): the publish and the look at the target
//! run as tasks of the busy registry, so that the page of Settings → Publishing and the indicator
//! show them, a second one cannot start, and Cancel stops the publish with the remote as it was
//! (JOB-003…JOB-006). The state of the page is [`PublishState`]; the page itself is
//! `settings/publishing.rs`.

use super::publish::{self, Failure, Last, Published, Request, Summary, Takeover};
use crate::app::ShaleApp;
use crate::busy::{self, Key};
use crate::gitsync::wizard::diagnose::{self, Facts};
use crate::gitsync::wizard::test_sync::Problem;
use crate::vault_state::PublishSettings;
use eframe::egui;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The publish as a task (JOB-003): it can be stopped, because the push is its last step.
pub(crate) const PUBLISH: Key = Key::new("site-publish");

/// Reading who published the target last, as a task that is listed only when it takes long (JOB-022).
pub(crate) const PUBLISH_LOOK: Key = Key::new("site-publish-look");

type Outcome = Result<Published, Failure>;
type Looked = Result<Option<Last>, Failure>;

/// What the target held when it was looked at: the address and branch that were asked, and the answer.
pub type TargetLook = ((String, String), Result<Option<Last>, String>);

/// The four lists of rules, as the text the page edits: one entry per line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Lists {
    /// The vault they were made for (its `uid`).
    pub vault: u64,
    pub include_folders: String,
    pub include_tags: String,
    pub exclude_folders: String,
    pub exclude_tags: String,
}

impl Lists {
    pub fn of(vault: u64, s: &PublishSettings) -> Lists {
        let text = |v: &[String]| v.join("\n");
        Lists { vault, include_folders: text(&s.include_folders), include_tags: text(&s.include_tags), exclude_folders: text(&s.exclude_folders), exclude_tags: text(&s.exclude_tags) }
    }

    /// Write the text back into the settings; true when a list changed.
    pub fn apply(&self, s: &mut PublishSettings) -> bool {
        let lines = |t: &str| -> Vec<String> {
            let mut out: Vec<String> = Vec::new();
            for l in t.lines().map(str::trim).filter(|l| !l.is_empty()) {
                if !out.iter().any(|o| o == l) {
                    out.push(l.to_string());
                }
            }
            out
        };
        let new = (lines(&self.include_folders), lines(&self.include_tags), lines(&self.exclude_folders), lines(&self.exclude_tags));
        let old = (s.include_folders.clone(), s.include_tags.clone(), s.exclude_folders.clone(), s.exclude_tags.clone());
        if new == old {
            return false;
        }
        (s.include_folders, s.include_tags, s.exclude_folders, s.exclude_tags) = new;
        true
    }
}

/// What the notes chosen by the rules were worked out for.
type ChoiceKey = (u64, u64, PublishSettings, String);

/// The state of Settings → Publishing and of the publish.
#[derive(Default)]
pub struct PublishState {
    job: Option<(PathBuf, busy::Job<Outcome>)>,
    /// What the running publish does now (SITE-065).
    pub(crate) step: Arc<Mutex<String>>,
    /// The last publish of this session: when it ended and how.
    pub last: Option<(String, Result<Summary, String>)>,
    /// The question of SITE-074 that waits for the person's answer.
    pub(crate) takeover: Option<Takeover>,
    look: Option<busy::Job<Looked>>,
    /// What the target held when it was looked at: the address and branch asked and the answer.
    pub target: Option<TargetLook>,
    /// The page looked at the target by itself for this vault already.
    pub(crate) looked_once: bool,
    /// The address or the branch was typed in and the field has not been left yet.
    pub(crate) target_dirty: bool,
    /// The address as typed, which may have a password in it that is not kept (SEC-010).
    pub(crate) url_text: Option<String>,
    /// The address of the site as typed; kept apart until it is allowed (SITE-081).
    pub(crate) site_address_text: Option<String>,
    /// What the checks found when an SSH sign-in failed (SYNC-077), read where the error is shown.
    facts: Arc<Mutex<Facts>>,
    pub lists: Lists,
    chosen: Option<(ChoiceKey, Vec<PathBuf>)>,
    /// The list of the chosen notes is open.
    pub list_open: bool,
}

impl PublishState {
    /// The vault is being left: a running publish or look is asked to stop, so that it does not
    /// push on for a vault that is closed.
    pub(crate) fn stop(&self) {
        if let Some((_, job)) = &self.job {
            job.cancel();
        }
        if let Some(job) = &self.look {
            job.cancel();
        }
    }

    /// A publish or a look at the target runs (the tests wait for it).
    #[cfg(test)]
    pub fn running(&self) -> bool {
        self.job.is_some() || self.look.is_some()
    }
}

/// "14:05", the local time now.
fn now_hhmm() -> String {
    let (_, sod) = crate::calendar::time::utc_to_local(crate::calendar::time::now());
    crate::calendar::time::hhmm(sod)
}

impl ShaleApp {
    /// The notes the rules of the open vault choose now, in path order (SITE-058): the same notes
    /// as `shale export-site --dry-run` lists. Worked out again only when the vault or the rules change.
    pub(crate) fn publish_chosen(&mut self) -> &[PathBuf] {
        let Some(v) = self.vault.as_ref() else { return &[] };
        let key = (v.uid, v.generation, self.vdata.settings.publish.clone(), self.vdata.settings.people_dir.clone());
        if self.publishing.chosen.as_ref().is_none_or(|(k, _)| *k != key) {
            let notes = super::plan::chosen_notes(v, &self.vdata.settings);
            self.publishing.chosen = Some((key, notes));
        }
        self.publishing.chosen.as_ref().map_or(&[], |(_, n)| n.as_slice())
    }

    /// What a failed publish or look says in plain words (SYNC-077…SYNC-081), as for git sync; None
    /// when the text of git is all there is.
    pub(crate) fn publish_problem(&self, error: &str) -> Option<Problem> {
        let facts = self.publishing.facts.lock().map(|f| *f).unwrap_or_default();
        Some(diagnose::diagnose(error, &facts)).filter(|p| !matches!(p, Problem::Other(_)))
    }

    /// The folder of the publish repositories (SITE-063): below the data folder, as the managed git is.
    fn publish_base(&self) -> PathBuf {
        self.sync.managed.data.as_deref().map_or_else(crate::device_local::default_base, |d| d.join("shale").join("vaults"))
    }

    /// Publish the open vault in the background (SITE-065). `take_over`: the person confirmed
    /// that this vault replaces what another vault published (SITE-074).
    pub(crate) fn start_publish(&mut self, take_over: bool) {
        if self.publishing.job.is_some() || self.publishing.look.is_some() {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        // the notes as they are now, and the rules as they are now: the publish reads both from the vault's files
        self.flush_all();
        if let Err(e) = self.vdata.save() {
            self.toast(e);
            return;
        }
        self.publishing.takeover = None;
        let (git, libraries, base, device) = (self.sync.git.clone(), self.cfg.media_libraries.clone(), self.publish_base(), crate::gitsync::host_name());
        let (step, facts, home, ctx, job_root) = (self.publishing.step.clone(), self.publishing.facts.clone(), self.sync.setup.home().unwrap_or_default(), self.ctx.clone(), root.clone());
        if let Ok(mut s) = step.lock() {
            *s = "Starting…".into();
        }
        let job = self.busy.spawn_cancellable(
            PUBLISH,
            "Publishing the site…",
            move |cancel| {
                let say = |what: &str| {
                    if let Ok(mut s) = step.lock() {
                        *s = what.to_string();
                    }
                    if let Some(c) = &ctx {
                        c.request_repaint();
                    }
                };
                let r = publish::publish(&git, &Request { vault: &job_root, libraries: &libraries, base: &base, device: &device, take_over }, cancel, &say);
                // a failed sign-in with an SSH key is looked at here, off the frame (SYNC-077)
                let found = match &r {
                    Err(Failure::Error(e)) => diagnose::gather(&git, &job_root, &home, e),
                    _ => Facts::default(),
                };
                if let Ok(mut f) = facts.lock() {
                    *f = found;
                }
                r
            },
            {
                let ctx = self.ctx.clone();
                move || {
                    if let Some(c) = ctx {
                        c.request_repaint();
                    }
                }
            },
        );
        self.publishing.job = job.map(|j| (root, j));
    }

    /// Ask the target who published it last (SITE-072), in the background.
    pub(crate) fn start_publish_look(&mut self) {
        if self.publishing.job.is_some() || self.publishing.look.is_some() {
            return;
        }
        let Some(root) = self.vault.as_ref().map(|v| v.root.clone()) else { return };
        if publish::target(&self.vdata.settings.publish).is_err() {
            return;
        }
        // the address and branch as they are typed now
        if let Err(e) = self.vdata.save() {
            self.toast(e);
            return;
        }
        self.publishing.looked_once = true;
        let (git, base, facts, home, ctx) = (self.sync.git.clone(), self.publish_base(), self.publishing.facts.clone(), self.sync.setup.home().unwrap_or_default(), self.ctx.clone());
        self.publishing.look = self.busy.spawn_cancellable_quiet(
            PUBLISH_LOOK,
            "Looking at the publish target…",
            move |cancel| {
                let r = publish::last_published(&git, &root, &base, cancel);
                let found = match &r {
                    Err(Failure::Error(e)) => diagnose::gather(&git, &root, &home, e),
                    _ => Facts::default(),
                };
                if let Ok(mut f) = facts.lock() {
                    *f = found;
                }
                r
            },
            move || {
                if let Some(c) = ctx {
                    c.request_repaint();
                }
            },
        );
    }

    /// The address and branch of the settings, as the look at the target is keyed.
    fn publish_key(&self) -> Option<(String, String)> {
        publish::target(&self.vdata.settings.publish).ok()
    }

    /// Every frame: take what the jobs have finished, ask the question of SITE-074.
    pub(crate) fn publish_tick(&mut self, ctx: &egui::Context) {
        if let Some((root, job)) = &self.publishing.job {
            match job.try_recv() {
                Ok(r) => {
                    let root = root.clone();
                    self.publishing.job = None;
                    self.publish_done(&root, r.unwrap_or_else(|e| Err(Failure::Error(format!("the publish stopped with an internal error: {e}")))));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.publishing.job = None;
                    self.publishing.last = Some((now_hhmm(), Err("the publish stopped unexpectedly".into())));
                }
            }
        }
        if let Some(job) = &self.publishing.look {
            match job.try_recv() {
                Ok(r) => {
                    self.publishing.look = None;
                    let answer = match r.unwrap_or_else(|e| Err(Failure::Error(e))) {
                        Ok(last) => Ok(last),
                        Err(f) => Err(f.words()),
                    };
                    if let Some(key) = self.publish_key() {
                        self.publishing.target = Some((key, answer));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(Duration::from_millis(200)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.publishing.look = None,
            }
        }
        self.publish_takeover_dialog(ctx);
    }

    fn publish_done(&mut self, root: &std::path::Path, r: Outcome) {
        if self.vault.as_ref().map(|v| v.root.as_path()) != Some(root) {
            return; // another vault was opened meanwhile
        }
        match r {
            Ok(done) => {
                self.toast(done.summary.line());
                if let Some(key) = self.publish_key() {
                    self.publishing.target = Some((key, Ok(Some(Last { meta: done.meta.clone(), this_vault: true }))));
                }
                self.publishing.last = Some((now_hhmm(), Ok(done.summary)));
            }
            Err(Failure::Takeover(t)) => self.publishing.takeover = Some(t),
            Err(Failure::Cancelled) => {
                self.publishing.last = Some((now_hhmm(), Err(Failure::Cancelled.words())));
            }
            Err(Failure::Error(e)) => {
                self.toast(format!("Publishing failed: {}", self.publish_problem(&e).map_or(e.clone(), |p| p.words())));
                self.publishing.last = Some((now_hhmm(), Err(e)));
            }
        }
    }

    /// The question of SITE-074: another vault published to the target (or nobody did), and publishing replaces it.
    fn publish_takeover_dialog(&mut self, ctx: &egui::Context) {
        let Some(t) = self.publishing.takeover.clone() else { return };
        let mut answer = None;
        let resp = crate::dialog::Dialog::new("site-takeover", "Replace what is published there?").width(480.0).show(ctx, |ui| {
            crate::theme::paragraph(ui, t.words());
            crate::theme::paragraph(ui, "The branch is replaced by one snapshot of this vault. What it held before is gone from the branch.");
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("Replace it").strong()).clicked() {
                    answer = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    answer = Some(false);
                }
            });
        });
        if answer.is_none() && resp.should_close() {
            answer = Some(false);
        }
        match answer {
            Some(true) => self.start_publish(true),
            Some(false) => self.publishing.takeover = None,
            None => {}
        }
    }

    /// The question is on screen: the keyboard belongs to it.
    pub(crate) fn publish_ask_shown(&self) -> bool {
        self.publishing.takeover.is_some()
    }
}
