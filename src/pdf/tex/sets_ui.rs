//! The "Template sets" part of Settings → LaTeX (TEX-024): the sets in the vault with their
//! source, version, kinds and logo, and the buttons that import, update, give a logo to, try out
//! and remove a set. Import and the test page are background jobs of the page (`TexState::start`);
//! a removal goes through the window's own trash (VAULT-017), which the settings tab carries out.

use super::compile::{CompileError, Options};
use super::engine::{Engine, TexConfig};
use super::import::{self, Request, What};
use super::sets::{self, SetEntry, SetInfo, Source};
use super::settings_ui::{Notice, TaskKind, TexState};
use crate::gitsync::TreeLock;
use crate::theme;
use eframe::egui::{self, Context, Ui};
use std::path::{Path, PathBuf};

/// The vault the sets are in.
#[derive(Clone)]
pub struct VaultRef {
    pub root: PathBuf,
    pub tree: TreeLock,
}

/// The form that is open, if any: only one at a time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Form {
    #[default]
    None,
    Github {
        url: String,
        reference: String,
    },
    /// A zip, a `.tar.gz` or a folder.
    File(String),
    /// A logo file for this set: the university's or the department's (TEX-035).
    Logo {
        set: String,
        slot: Slot,
        path: String,
    },
    /// Asking before a set goes to the trash.
    Remove(String),
}

/// Which of a set's two logo slots a logo goes to (TEX-007, TEX-035).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// The university's logo, which some classes need (`logo_file`).
    University,
    /// The optional department logo every set has.
    Department,
}

/// The state of the template sets part of the page.
#[derive(Default)]
pub struct SetsState {
    pub vault: Option<VaultRef>,
    /// The sets found in the vault; none: not read yet.
    pub sets: Option<Vec<SetEntry>>,
    pub form: Form,
    /// Read the sets again (a job ended, a logo was chosen).
    pub dirty: bool,
    /// A set the person confirmed to remove, for the window to move to the vault's trash.
    trash: Option<PathBuf>,
    /// The vault's file list must be read again.
    rescan: bool,
}

impl SetsState {
    /// Tell the page which vault is open (the settings tab does it every frame).
    pub fn bind(&mut self, vault: Option<VaultRef>) {
        if self.vault.as_ref().map(|v| &v.root) != vault.as_ref().map(|v| &v.root) {
            self.sets = None;
            self.form = Form::None;
        }
        self.vault = vault;
    }

    /// The sets of the vault, read when they are not or a change made them stale.
    pub fn refresh(&mut self) {
        if self.sets.is_none() || self.dirty {
            self.sets = self.vault.as_ref().map(|v| sets::scan(&v.root));
            self.dirty = false;
        }
    }

    /// A set to move to the trash, once.
    pub fn take_trash(&mut self) -> Option<PathBuf> {
        let t = self.trash.take();
        if t.is_some() {
            self.dirty = true;
        }
        t
    }

    /// Whether the vault must be read again, once.
    pub fn take_rescan(&mut self) -> bool {
        std::mem::take(&mut self.rescan)
    }

    /// Copy a logo file into a slot of a set and read the sets and the vault again (also for a logo
    /// that was dropped on the window, TEX-007, TEX-035).
    pub(super) fn copy_logo_to(&mut self, set: &str, slot: Slot, from: &Path) -> Notice {
        let entries = self.sets.clone().unwrap_or_default();
        let notice = copy_logo(self, &entries, set, slot, from);
        self.dirty = true;
        self.rescan = true;
        notice
    }

    /// Move a set's department logo to the vault's trash and clear the setting (TEX-035).
    pub(super) fn remove_department_logo_of(&mut self, set: &str) -> Notice {
        let entries = self.sets.clone().unwrap_or_default();
        let notice = remove_department_logo(self, &entries, set);
        self.dirty = true;
        self.rescan = true;
        notice
    }

    /// Mark the end of a job of the sets: the list and the vault's file list are stale.
    pub(super) fn job_ended(&mut self, kind: TaskKind) {
        self.dirty = true;
        self.rescan |= kind == TaskKind::Import;
    }
}

/// What to import to update a set: the place it came from.
pub fn what_for(source: &Source) -> What {
    match source {
        Source::Github { repo, reference, sha256: Some(_) } if repo == import::PRESET_REPO && reference == import::PRESET_COMMIT => What::Preset,
        Source::Github { repo, reference, .. } => What::Github { url: format!("https://github.com/{repo}"), reference: reference.clone() },
        Source::Ctan { .. } => What::Ctan,
        Source::Local { path } => What::Local(PathBuf::from(path)),
    }
}

/// What the person is told when an import ends.
fn import_notice(r: Result<import::Imported, String>) -> Notice {
    match r {
        Ok(i) => Notice::new(i.notice(), i.kinds > 0),
        Err(e) if e == "cancelled" => Notice::new("The import was cancelled; the vault is as it was.", false),
        Err(e) => Notice::new(format!("The import failed, the vault is as it was: {e}"), false),
    }
}

/// Compile the test page of a set; the window opens the PDF (the notice carries it), or say what went wrong.
fn test_page_notice(engine: &Engine, set: &SetInfo) -> Notice {
    let fail = |text: String, e: Option<CompileError>| Notice { details: e.map(|e| e.log_tail.clone()).filter(|t| !t.is_empty()), ..Notice::new(text, false) };
    let Some((id, kind)) = sets::test_kind(set) else {
        return fail(format!("“{}” has no article kind to make a test page of (see its README)", set.meta.name), None);
    };
    let source = sets::sample_document(kind, &set.settings, set.department_logo.is_some());
    match sets::compile_with_set(engine, set, "test-page.tex", &source, &[], Options::default()) {
        Ok(out) => {
            let logo = if out.placeholder_logo { " It has a placeholder logo, so it is not CI-conformant." } else { "" };
            let text = format!("The test page of “{}” (kind {id}) compiled to a PDF of {} KB.{logo}", set.meta.name, out.output.pdf.len().div_ceil(1024));
            Notice { open: Some((format!("{}-test-page.pdf", set.id), out.output.pdf)), ..Notice::new(text, true) }
        }
        Err(e) => fail(format!("The test page of “{}” did not compile: {e}", set.meta.name), Some(e)),
    }
}

impl TexState {
    /// Start importing a set.
    pub(super) fn start_import(&mut self, ctx: &Context, cfg: &TexConfig, what: What) {
        let Some(vault) = self.sets.vault.clone() else { return };
        let engine = self.detection.as_ref().and_then(|d| d.selected(cfg.engine).ok()).cloned();
        let req = Request { vault: vault.root, what, engine, tree: vault.tree, tree_wait: import::TREE_WAIT };
        self.start(ctx, TaskKind::Import, move |h, n| import_notice(import::import(&req, h, n)));
        self.sets.form = Form::None;
    }
}

/// The part of the page.
pub fn show(ui: &mut Ui, st: &mut TexState, cfg: &TexConfig) {
    let ctx = ui.ctx().clone();
    st.sets.refresh();
    theme::section(ui, "Template sets");
    crate::theme::hint(ui, "The templates a PDF is made with live in your vault, in templates/tex, so they are synced and backed up with your notes. TeX Live supplies only the engine and the generic packages.");
    if st.sets.vault.is_none() {
        ui.label("Open a vault to import template sets.");
        return;
    }
    // a running job
    if let Some(t) = &st.task {
        match t.kind {
            TaskKind::Import => {
                let p = t.handle.progress();
                ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
                if ui.button("Cancel import").on_hover_text("Stop; the vault stays as it was").clicked() {
                    t.handle.cancel();
                }
            }
            TaskKind::TestPage => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Compiling the test page…");
                });
            }
            _ => {}
        }
    }
    let idle = st.task.is_none();
    let entries = st.sets.sets.clone().unwrap_or_default();
    if entries.is_empty() {
        crate::theme::paragraph(ui, "There are no template sets in this vault yet.");
    }
    let mut action: Option<(String, SetAction)> = None;
    for e in &entries {
        ui.group(|ui| set_row(ui, e, idle, &mut action));
    }
    if let Some((id, a)) = action {
        apply(st, &ctx, cfg, &entries, &id, a);
    }
    import_buttons(ui, st, &ctx, cfg, idle);
    forms(ui, st, &ctx, cfg, &entries);
}

/// What a button of a set asks for.
enum SetAction {
    Update,
    ChooseLogo,
    ChooseDepartmentLogo,
    RemoveDepartmentLogo,
    TestPage,
    Remove,
}

fn set_row(ui: &mut Ui, e: &SetEntry, idle: bool, action: &mut Option<(String, SetAction)>) {
    match &e.info {
        Err(why) => {
            ui.label(egui::RichText::new(format!("templates/tex/{}", e.id)).strong());
            crate::theme::problem(ui, format!("This folder cannot be read as a set: {why}"));
            if ui.add_enabled(idle, egui::Button::new("Remove set…")).clicked() {
                *action = Some((e.id.clone(), SetAction::Remove));
            }
        }
        Ok(s) => {
            let version = s.meta.version.as_ref().map(|v| format!("  {v}")).unwrap_or_default();
            ui.label(egui::RichText::new(format!("{}{version}", s.meta.name)).strong());
            crate::theme::hint(ui, format!("{} · imported {} · templates/tex/{}", s.meta.source.describe(), s.meta.imported_at, s.id));
            let kinds = match s.kinds.len() {
                0 => "no document kinds".to_string(),
                1 => "1 document kind".to_string(),
                n => format!("{n} document kinds"),
            };
            ui.label(kinds);
            if s.kinds.is_empty() {
                crate::theme::hint(ui, "Shale has no document kinds for these templates: add them by hand in the folder kinds (see README.md in the set's folder).");
            }
            let logo = s.logo.text();
            if s.logo == sets::LogoStatus::Missing {
                crate::theme::problem(ui, logo);
            } else {
                ui.label(logo);
            }
            if let Some(text) = s.department_logo_text() {
                ui.label(text);
            }
            for p in &s.problems {
                crate::theme::problem(ui, p);
            }
            ui.horizontal_wrapped(|ui| {
                for (label, tip, a) in [
                    ("Update set", "Download the templates again; your settings and logo are kept", SetAction::Update),
                    ("Choose logo…", "Copy the logo file (PDF or PNG) into the set", SetAction::ChooseLogo),
                    ("Choose department logo…", "Copy the logo of your department or institution (PDF or PNG) into the set: documents show it under the university's logo", SetAction::ChooseDepartmentLogo),
                    ("Remove department logo", "Move the department logo to the vault's trash", SetAction::RemoveDepartmentLogo),
                    ("Test page", "Compile a small document of the set and open the PDF", SetAction::TestPage),
                    ("Remove set…", "Move the set to the vault's trash", SetAction::Remove),
                ] {
                    let enabled = idle
                        && match a {
                            SetAction::ChooseLogo => s.meta.logo_file.is_some(),
                            SetAction::RemoveDepartmentLogo => s.department_logo.is_some(),
                            _ => true,
                        };
                    if ui.add_enabled(enabled, egui::Button::new(label)).on_hover_text(tip).clicked() {
                        *action = Some((s.id.clone(), a));
                    }
                }
            });
        }
    }
}

fn apply(st: &mut TexState, ctx: &Context, cfg: &TexConfig, entries: &[SetEntry], id: &str, action: SetAction) {
    let info = entries.iter().find(|e| e.id == id).and_then(|e| e.info.as_ref().ok());
    match action {
        SetAction::Update => {
            if let Some(s) = info {
                st.start_import(ctx, cfg, what_for(&s.meta.source));
            }
        }
        SetAction::ChooseLogo => st.sets.form = Form::Logo { set: id.to_string(), slot: Slot::University, path: String::new() },
        SetAction::ChooseDepartmentLogo => st.sets.form = Form::Logo { set: id.to_string(), slot: Slot::Department, path: String::new() },
        SetAction::RemoveDepartmentLogo => st.notice = Some(st.sets.remove_department_logo_of(id)),
        SetAction::Remove => st.sets.form = Form::Remove(id.to_string()),
        SetAction::TestPage => {
            let Some(set) = info.cloned() else { return };
            match st.detection.as_ref().map(|d| d.selected(cfg.engine)) {
                Some(Ok(engine)) => {
                    let engine = engine.clone();
                    st.start(ctx, TaskKind::TestPage, move |_, _| test_page_notice(&engine, &set));
                }
                Some(Err(why)) => st.notice = Some(Notice::new(format!("No TeX can be used for the test page: {why}"), false)),
                None => st.notice = Some(Notice::new("Shale is still looking for a TeX: try again in a moment.", false)),
            }
        }
    }
}

fn import_buttons(ui: &mut Ui, st: &mut TexState, ctx: &Context, cfg: &TexConfig, idle: bool) {
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(idle, egui::Button::new("Import TU Darmstadt templates")).on_hover_text("tuda-ci 4.06 from GitHub, the version Shale was tested with (checked by its checksum)").clicked() {
            st.start_import(ctx, cfg, What::Preset);
        }
        if ui.add_enabled(idle, egui::Button::new("Import from GitHub…")).on_hover_text("A repository of templates, at a branch, tag or commit").clicked() {
            st.sets.form = Form::Github { url: String::new(), reference: String::new() };
        }
        if ui.add_enabled(idle, egui::Button::new("Import from file or folder…")).on_hover_text("A .zip or .tar.gz file, or a folder").clicked() {
            st.sets.form = Form::File(String::new());
        }
    });
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(idle, egui::Button::new("Newest TU Darmstadt templates from CTAN")).clicked() {
            st.start_import(ctx, cfg, What::Ctan);
        }
        crate::theme::hint(ui, "Not pinned and without a checksum: whatever CTAN has today, which Shale was not tested with.");
    });
}

fn forms(ui: &mut Ui, st: &mut TexState, ctx: &Context, cfg: &TexConfig, entries: &[SetEntry]) {
    match st.sets.form.clone() {
        Form::None => {}
        Form::Github { mut url, mut reference } => {
            ui.group(|ui| {
                ui.label("GitHub repository");
                ui.add(egui::TextEdit::singleline(&mut url).hint_text("https://github.com/owner/repository").desired_width(380.0));
                ui.label("Branch, tag or commit (empty: the default branch)");
                ui.add(egui::TextEdit::singleline(&mut reference).desired_width(380.0));
                crate::theme::hint(ui, "Sets Shale does not know get no document kinds: add them by hand (see the README of the set).");
                let (go, cancel) = form_buttons(ui, "Import set");
                if go {
                    st.start_import(ctx, cfg, What::Github { url: url.clone(), reference: reference.clone() });
                } else if cancel {
                    st.sets.form = Form::None;
                } else {
                    st.sets.form = Form::Github { url, reference };
                }
            });
        }
        Form::File(mut path) => {
            ui.group(|ui| {
                ui.label("Path of a .zip or .tar.gz file, or of a folder");
                ui.add(egui::TextEdit::singleline(&mut path).hint_text("~/Downloads/templates.zip").desired_width(380.0));
                let (go, cancel) = form_buttons(ui, "Import set");
                if go {
                    st.start_import(ctx, cfg, What::Local(expand_home(&path)));
                } else if cancel {
                    st.sets.form = Form::None;
                } else {
                    st.sets.form = Form::File(path);
                }
            });
        }
        Form::Logo { set, slot, mut path } => {
            ui.group(|ui| {
                let (what, hint) = match slot {
                    Slot::University => ("Path of the logo file (PDF or PNG)", "~/Downloads/tuda_logo.pdf"),
                    Slot::Department => ("Path of the department logo file (PDF or PNG)", "~/Downloads/hrz_logo.png"),
                };
                ui.label(what);
                ui.add(egui::TextEdit::singleline(&mut path).hint_text(hint).desired_width(380.0));
                let (go, cancel) = form_buttons(ui, "Copy logo into the set");
                if go {
                    st.sets.form = Form::None;
                    st.notice = Some(st.sets.copy_logo_to(&set, slot, &expand_home(&path)));
                } else if cancel {
                    st.sets.form = Form::None;
                } else {
                    st.sets.form = Form::Logo { set, slot, path };
                }
            });
        }
        Form::Remove(id) => {
            let name = entries.iter().find(|e| e.id == id).and_then(|e| e.info.as_ref().ok()).map_or(id.clone(), |s| s.meta.name.clone());
            ui.group(|ui| {
                crate::theme::paragraph(ui, format!("Move the set “{name}” (templates/tex/{id}) to the vault's .trash folder? Its settings and logo go with it. You can restore it from there."));
                ui.horizontal(|ui| {
                    if ui.button("Move set to trash").clicked() {
                        st.sets.trash = Some(Path::new(sets::SETS_DIR).join(&id));
                        st.sets.form = Form::None;
                    }
                    if ui.button("Keep the set").clicked() {
                        st.sets.form = Form::None;
                    }
                });
            });
        }
    }
}

/// The buttons of a form: (go, cancel).
fn form_buttons(ui: &mut Ui, go: &str) -> (bool, bool) {
    ui.horizontal(|ui| (ui.button(go).clicked(), ui.button("Cancel").clicked())).inner
}

/// `~/x` as the home folder's `x`.
pub(super) fn expand_home(text: &str) -> PathBuf {
    let text = text.trim();
    match (text.strip_prefix("~/"), dirs::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(text),
    }
}

/// The set of the page that a button of a logo asks for, and the vault's tree lock held for the write.
fn locked_set<'a>(sets_state: &'a SetsState, entries: &'a [SetEntry], set: &str) -> Result<(&'a VaultRef, &'a SetInfo, crate::gitsync::TreeGuard), Notice> {
    let (Some(vault), Some(info)) = (&sets_state.vault, entries.iter().find(|e| e.id == set).and_then(|e| e.info.as_ref().ok())) else {
        return Err(Notice::new("The set is not there any more.", false));
    };
    match vault.tree.hold_for(std::time::Duration::from_secs(5)) {
        Some(lock) => Ok((vault, info, lock)),
        None => Err(Notice::new("The sync is still changing files in the vault: try again in a moment.", false)),
    }
}

/// Copy the chosen logo into the set (a few milliseconds, so in the frame), holding the vault's tree lock.
fn copy_logo(sets_state: &SetsState, entries: &[SetEntry], set: &str, slot: Slot, from: &Path) -> Notice {
    let (vault, info, _lock) = match locked_set(sets_state, entries, set) {
        Ok(found) => found,
        Err(notice) => return notice,
    };
    match slot {
        Slot::University => match sets::choose_logo(&vault.root, info, from) {
            Ok(name) => Notice::new(format!("The logo {name} is in the set “{}” now.", info.meta.name), true),
            Err(e) => Notice::new(format!("The logo was not copied: {e}"), false),
        },
        Slot::Department => match sets::choose_department_logo(&vault.root, info, from) {
            Ok(name) => Notice::new(format!("The department logo {name} is in the set “{}” now.", info.meta.name), true),
            Err(e) => Notice::new(format!("The department logo was not copied: {e}"), false),
        },
    }
}

/// Move the department logo of the set to the vault's trash, holding the vault's tree lock.
fn remove_department_logo(sets_state: &SetsState, entries: &[SetEntry], set: &str) -> Notice {
    let (vault, info, _lock) = match locked_set(sets_state, entries, set) {
        Ok(found) => found,
        Err(notice) => return notice,
    };
    match sets::remove_department_logo(&vault.root, info) {
        Ok(()) => Notice::new(format!("The department logo of the set “{}” is in the vault's trash now.", info.meta.name), true),
        Err(e) => Notice::new(format!("The department logo was not removed: {e}"), false),
    }
}
