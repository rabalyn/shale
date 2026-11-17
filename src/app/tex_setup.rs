//! The guided setup of the LaTeX export in the window (TEX-017): opening it (by itself from the
//! export dialog, from a "Set up…" hint, from Settings → LaTeX), its dialog, the test page, the way
//! back to where the person came from, and the files dropped on it or on Settings → LaTeX, which
//! never reach the vault's own drop handling.

use super::{Modal, ShaleApp};
use crate::pdf::export_dialog::{ExportDialog, ExportEngine};
use crate::pdf::tex::dialog::{self, Failure};
use crate::pdf::tex::sets_ui::VaultRef;
use crate::pdf::tex::setup::{self, Outcome, Return, Step};
use crate::pdf::tex::setup_ui;
use eframe::egui;
use std::path::PathBuf;
use std::sync::Arc;

/// Where a file dropped on the window goes first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TexDrop {
    /// The guided setup is open.
    Setup,
    /// Settings is open on the LaTeX tab.
    Settings,
}

impl ShaleApp {
    /// Tell the template sets of the LaTeX state which vault is open.
    pub(crate) fn bind_tex_sets(&mut self) {
        let vault = self.vault.as_ref().map(|v| VaultRef { root: v.root.clone(), tree: self.sync.tree.clone() });
        self.tex.sets.bind(vault);
    }

    /// Open the guided setup at this step (the first one that is not done when none is given); `ret`
    /// is where the person goes afterwards. The setup does not open by itself again in this vault.
    pub fn open_tex_setup(&mut self, at: Option<Step>, ret: Return) {
        if self.vault.is_none() {
            self.toast("Open a vault first: the templates are kept in it.");
            return;
        }
        self.bind_tex_sets();
        self.tex.sets.dirty = true;
        self.tex.sets.refresh();
        self.tex.open_setup(&self.cfg.tex, at, ret);
        self.vdata.state.latex_export.setup_seen = true;
        self.save_config();
        self.modal = Some(Modal::TexSetup);
    }

    /// Where the export dialog sends the person: the step of a "Set up…" button that was pressed, or
    /// (the first time, when something is missing) the step the setup starts at by itself.
    pub(crate) fn tex_setup_wanted(&mut self, d: &mut ExportDialog) -> Option<Step> {
        if let Some(step) = d.tex.as_mut()?.setup_requested.take() {
            return Some(step);
        }
        if d.engine == ExportEngine::Latex && !self.vdata.state.latex_export.setup_seen {
            return d.tex.as_ref()?.auto_setup_step();
        }
        None
    }

    /// The dialog of the setup, one frame. Returns whether it stays open.
    pub(crate) fn tex_setup_dialog(&mut self, ctx: &egui::Context) -> bool {
        self.bind_tex_sets();
        self.tex.setup_tick(ctx);
        self.start_setup_test(ctx);
        if self.tex.sets.take_rescan() {
            if let Some(v) = self.vault.as_mut() {
                v.rescan();
            }
        }
        let mut st = (&mut self.tex, &self.cfg.tex, Vec::new());
        let resp = crate::dialog::Dialog::new("tex-setup", "Set up LaTeX export").width(600.0).show_with_footer(ctx, &mut st, |ui, (t, c, a)| setup_ui::show(ui, t, c, a), |ui, (t, c, a)| setup_ui::footer(ui, t, c, a));
        let acts = st.2;
        let mut end = setup_ui::apply(&mut self.tex, ctx, &self.cfg.tex, acts);
        if resp.should_close() && end.is_none() {
            end = Some(false);
        }
        match end {
            Some(finished) => {
                self.end_tex_setup(finished);
                false
            }
            None => true,
        }
    }

    /// Compile the test page when the step asked for it: the sample note through the writer, with the
    /// chosen set and details, as a job.
    fn start_setup_test(&mut self, ctx: &egui::Context) {
        if !std::mem::take(&mut self.tex.setup.run_test) {
            return;
        }
        let failed = |headline: String| Outcome::Failed(Failure { headline, problems: vec![], log_tail: String::new(), document: Default::default(), set_dir: PathBuf::new() });
        let engine = match self.tex.detection.as_ref().map(|d| d.selected(self.cfg.tex.engine)) {
            Some(Ok(e)) => e.clone(),
            Some(Err(why)) => {
                self.tex.setup.outcome = Some(failed(format!("No TeX can be used: {why}")));
                return;
            }
            None => {
                self.tex.setup.outcome = Some(failed("Shale is still looking for a TeX: press Run again in a moment.".into()));
                return;
            }
        };
        let (Some(set), Some(vault)) = (self.tex.chosen_set().cloned(), self.vault.as_ref()) else {
            self.tex.setup.outcome = Some(failed("There are no templates to make a test page with.".into()));
            return;
        };
        match setup::test_page(vault, &set) {
            Ok((_, doc)) => {
                let ctx = ctx.clone();
                self.tex.setup.job = Some(dialog::spawn_compile(engine, set, Arc::new(doc), move || ctx.request_repaint()));
            }
            Err(e) => self.tex.setup.outcome = Some(failed(e)),
        }
    }

    /// The setup is over (finished or cancelled): stop what it started, and go back to where the
    /// person came from, on the LaTeX engine with the set that was chosen.
    fn end_tex_setup(&mut self, finished: bool) {
        let set = self.tex.chosen_set().map(|s| s.id.clone());
        if let Some(t) = self.tex.task.as_ref().filter(|_| !finished) {
            t.handle.cancel();
        }
        let ret = std::mem::take(&mut self.tex.setup.ret);
        self.tex.setup = Default::default();
        match ret {
            Return::Nothing => {}
            Return::Settings => self.open_settings(crate::app::SettingsTab::Latex),
            Return::Export(rel) => {
                self.open_export_dialog(&rel);
                if let Some(Modal::Export(d)) = &mut self.modal {
                    d.engine = ExportEngine::Latex;
                    if let Some(tex) = d.tex.as_mut() {
                        if let Some(i) = set.and_then(|id| tex.sets.iter().position(|s| s.id == id)) {
                            tex.select_set(i, None);
                        }
                    }
                }
            }
        }
    }

    /// Where a file dropped on the window goes first, if not into the vault: the setup while it
    /// is open, Settings on its LaTeX tab.
    pub(crate) fn tex_drop_target(&self) -> Option<TexDrop> {
        match &self.modal {
            Some(Modal::TexSetup) => Some(TexDrop::Setup),
            Some(Modal::Settings { tab: crate::app::SettingsTab::Latex, .. }) => Some(TexDrop::Settings),
            _ => None,
        }
    }

    /// Take dropped files for the setup or for Settings → LaTeX; a toast says when nothing was taken.
    pub(crate) fn tex_drop(&mut self, target: TexDrop, ctx: &egui::Context, paths: &[PathBuf]) {
        self.bind_tex_sets();
        let told = match target {
            TexDrop::Setup => self.tex.setup_drop(ctx, &self.cfg.tex, paths),
            TexDrop::Settings => self.tex.settings_drop(ctx, &self.cfg.tex, paths),
        };
        if let Some(t) = told {
            self.toast(t);
        }
    }

    /// What a drag over the window says while the setup or Settings → LaTeX takes the files.
    pub(crate) fn tex_drop_hint(&self, target: TexDrop) -> &'static str {
        match (target, self.tex.setup.step) {
            (TexDrop::Setup, Step::Logo) => "Drop the logo here (a PDF or a PNG file)",
            (TexDrop::Setup, Step::Templates) => "Drop the templates here (a .zip or .tar.gz file, or a folder)",
            (TexDrop::Setup, _) => "Nothing to drop on this step",
            (TexDrop::Settings, _) => "Drop templates (a .zip, a .tar.gz or a folder) or a logo (a PDF or PNG) here",
        }
    }
}

/// The files a drop from another program names (its `text/uri-list`; Wayland, DEV-022).
#[cfg(target_os = "linux")]
pub(crate) fn dropped_paths(parts: &[(String, Vec<u8>)]) -> Vec<PathBuf> {
    let Some((_, data)) = parts.iter().find(|(m, _)| m == "text/uri-list") else { return vec![] };
    String::from_utf8_lossy(data).lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(crate::media::file_uri_path).collect()
}
