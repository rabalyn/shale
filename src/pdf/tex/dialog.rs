//! The LaTeX choice of the note export dialog (TEX-016): the template set and kind, the fields of
//! the kind, the table of contents, the state of TeX, and the export as a job with Cancel; when it
//! fails, the readable errors and "Save the .tex" for someone who wants to debug it. The dialog
//! itself (`pdf::export_dialog`) shows the engine choice and calls [`TexExport::show`] for the rest.
//! A folder's dialog has the same choice, for the kinds that can hold a folder (BOOK-013).

use super::book::{holds_a_folder, ONLY_ARTICLES_AND_BOOKS};
use super::compile::{CompileError, Options};
use super::engine::Engine;
use super::sets::{self, KindManifest, LogoStatus, Mapping, SetInfo};
use super::setup::Step;
use super::writer::{self, Document};
use crate::busy::{self, Busy, Key};
use crate::clickable::CollapsingPointer;
use crate::clickable::ComboBoxPointer;
use crate::pdf::export::ExportOptions;
use crate::theme;
use eframe::egui::{self, Ui};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// What the dialog knows about TeX: set by the window every frame from what it found.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum TexStatus {
    /// The window is still looking.
    #[default]
    Checking,
    /// A TeX that can be used: "Using the TeX Live that Shale installs (LaTeX kernel 2025-11-01)".
    Ready(String),
    /// No TeX can be used, and why.
    Missing(String),
}

/// A PDF the job made.
pub struct Made {
    pub pdf: Vec<u8>,
    /// The set has no logo yet: the PDF has a placeholder and is not CI-conformant.
    pub placeholder_logo: bool,
    /// Embeds that were missing, as the writer said, then what the compile said (TEX-043).
    pub warnings: Vec<String>,
}

/// Why an export failed, and what is needed to try it by hand.
pub struct Failure {
    pub headline: String,
    /// `main.tex:12: Undefined control sequence.`, `package foo is missing`.
    pub problems: Vec<String>,
    pub log_tail: String,
    /// The document the writer made and its files: "Save the .tex" writes them.
    pub document: Arc<Document>,
    /// The set it was compiled against, for the command line in the saved folder.
    pub set_dir: PathBuf,
    /// The packages of Shale's list (`assets/tex/packages.txt`) whose files TeX did not find (TEX-052).
    pub missing: Box<[String]>,
}

impl Failure {
    /// A failure with nothing but what it says: no log, no document.
    pub fn said(headline: String) -> Failure {
        Failure { headline, problems: vec![], log_tail: String::new(), document: Arc::new(Document::default()), set_dir: PathBuf::new(), missing: Box::default() }
    }
}

/// The PDF export through LaTeX, as a task of the busy registry (JOB-018).
pub const TEX_EXPORT: Key = Key::new("tex-export");

/// A running export of the dialog, a task of the registry. Dropping it (the dialog closed, or
/// Cancel) stops LuaLaTeX.
pub struct ExportJob(pub(super) busy::Job<Result<Made, Box<Failure>>>);

impl Drop for ExportJob {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

/// A running compile of the guided setup's test page. Dropping it stops LuaLaTeX.
pub struct Job {
    pub(super) rx: Receiver<Result<Result<Made, Box<Failure>>, String>>,
    pub(super) cancel: Arc<AtomicBool>,
}

impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// The LaTeX part of the dialog of a note.
pub struct TexExport {
    /// The sets of the vault that have document kinds.
    pub sets: Vec<SetInfo>,
    /// The chosen set (an index into `sets`) and kind (its id).
    pub set: usize,
    pub kind: String,
    /// The note's name as a title and its properties: what the fields start from.
    title: String,
    props: Vec<(String, Vec<String>)>,
    /// What is in the field boxes, by key. A value is kept when the kind changes.
    pub values: BTreeMap<String, String>,
    pub status: TexStatus,
    pub job: Option<ExportJob>,
    pub failure: Option<Failure>,
    /// "Save the .tex" was pressed: the window writes the folder.
    pub save_requested: bool,
    /// A "Set up…" button beside a missing TeX, set or logo was pressed: the window opens the
    /// guided setup (TEX-017) at this step.
    pub setup_requested: Option<Step>,
    /// The dialog of a folder: only the kinds of the article and book family (BOOK-013).
    pub folder: bool,
    /// The note's `latex-template:` names a set or kind the vault does not have, or one a folder
    /// cannot use: what was not found and what is used instead (TEX-033). Gone once the person picks a set or kind.
    pub template_warning: Option<String>,
    /// The engine in use is the managed TeX Live (set by the window every frame, TEX-052).
    pub managed: bool,
    /// The packages of the list that this engine lacks, when it is the managed one: a line with an
    /// Install button says so (TEX-052).
    pub missing: Vec<String>,
    /// The packages this document needs besides what every document has (a board: `geometry` and `tools`):
    /// while one of them is in `missing`, Export is off (TEX-052).
    pub needs: Vec<String>,
    /// Install was pressed: the window starts `tlmgr install` of [`TexExport::wanted_packages`].
    pub install_requested: bool,
    /// The install of the packages runs: what it is doing (set by the window).
    pub installing: Option<String>,
    /// Why the install of the packages failed.
    pub install_error: Option<String>,
}

/// The choice a note's properties make: `latex-template: tuda/thesis` (a set and a kind).
pub fn note_choice(props: &[(String, Vec<String>)]) -> Option<(String, String)> {
    let v = props.iter().find(|(k, _)| k == "latex-template")?.1.first()?;
    let (set, kind) = v.split_once('/')?;
    Some((set.trim().to_string(), kind.trim().to_string()))
}

impl TexExport {
    /// The dialog for a note called `title` with this text, in a vault: the sets are read now;
    /// `prefer` is the set and kind to start on (the note's own choice, else the last one).
    pub fn new(vault: &Path, title: String, text: &str, prefer: Option<(String, String)>) -> TexExport {
        Self::build(vault, title, text, prefer, false)
    }

    /// The dialog of a folder's book called `title`; `text` is its top folder note, whose
    /// properties fill the fields (BOOK-015). Only kinds that can hold a folder can be chosen.
    pub fn for_folder(vault: &Path, title: String, text: &str, prefer: Option<(String, String)>) -> TexExport {
        Self::build(vault, title, text, prefer, true)
    }

    fn build(vault: &Path, title: String, text: &str, prefer: Option<(String, String)>, folder: bool) -> TexExport {
        let sets: Vec<SetInfo> = sets::scan(vault).into_iter().filter_map(|e| e.info.ok()).filter(|s| !s.kinds.is_empty()).collect();
        let props = crate::markdown::front_matter_values(text);
        let mut t = TexExport {
            sets,
            set: 0,
            kind: String::new(),
            title,
            props,
            values: BTreeMap::new(),
            status: TexStatus::default(),
            job: None,
            failure: None,
            save_requested: false,
            setup_requested: None,
            folder,
            template_warning: None,
            managed: false,
            missing: vec![],
            needs: vec![],
            install_requested: false,
            installing: None,
            install_error: None,
        };
        let (set, kind) = prefer.unzip();
        if let Some(i) = set.and_then(|id| t.sets.iter().position(|s| s.id == id)) {
            t.set = i;
        }
        t.select_set(t.set, kind.as_deref());
        t.template_warning = t.unknown_template();
        t
    }

    /// What the note's `latex-template:` asks for that the vault does not have or a folder cannot
    /// use, and what the dialog uses instead (TEX-033, BOOK-013); None when it names nothing, or a
    /// set and kind that are there and can be used.
    fn unknown_template(&self) -> Option<String> {
        let raw = self.props.iter().find(|(k, _)| k == "latex-template")?.1.first()?.trim().to_string();
        let set = self.current_set()?;
        let using = format!("“{}” of “{}”", self.current_kind().map_or(self.kind.as_str(), |k| k.name.as_str()), set.meta.name);
        let Some((want_set, want_kind)) = note_choice(&self.props) else {
            return Some(format!("The note's latex-template “{raw}” names no kind (write it as set/kind): using {using} instead."));
        };
        match self.sets.iter().find(|s| s.id == want_set) {
            None => Some(format!("The note's latex-template asks for the set “{want_set}”, which this vault does not have: using {using} instead.")),
            Some(s) if !s.kinds.iter().any(|(id, _)| *id == want_kind) => Some(format!("The set “{}” has no kind “{want_kind}”, which the note's latex-template asks for: using {using} instead.", s.meta.name)),
            Some(s) => {
                let kind = s.kinds.iter().find(|(id, _)| *id == want_kind).map(|(_, k)| k)?;
                (!self.allows(kind)).then(|| format!("The note's latex-template asks for “{}”, which takes a single note, not a folder: using {using} instead.", kind.name))
            }
        }
    }

    pub fn current_set(&self) -> Option<&SetInfo> {
        self.sets.get(self.set)
    }

    pub fn current_kind(&self) -> Option<&KindManifest> {
        self.current_set()?.kinds.iter().find(|(id, _)| *id == self.kind).map(|(_, k)| k).filter(|k| self.allows(k))
    }

    /// Whether a kind can be chosen: any for a note, the article and book family for a folder.
    pub fn allows(&self, kind: &KindManifest) -> bool {
        !self.folder || holds_a_folder(kind)
    }

    /// Choose a set, and a kind of it (the first when it has none of that name); the fields of the
    /// kind get their values from the note.
    pub fn select_set(&mut self, i: usize, kind: Option<&str>) {
        self.set = i.min(self.sets.len().saturating_sub(1));
        let Some(set) = self.sets.get(self.set) else { return };
        // the kind asked for, else the one the test page uses (the publication, else an article), else
        // the first; of a folder's dialog, only one that can hold a folder
        let ok = |k: &&(String, KindManifest)| self.allows(&k.1);
        self.kind = kind.and_then(|k| set.kinds.iter().find(|(id, _)| id == k)).filter(ok).or_else(|| sets::test_kind(set).filter(ok)).or_else(|| set.kinds.iter().find(ok)).map(|(id, _)| id.clone()).unwrap_or_default();
        self.fill_fields();
    }

    pub fn select_kind(&mut self, id: &str) {
        if self.current_set().is_some_and(|s| s.kinds.iter().any(|(k, m)| k == id && self.allows(m))) {
            self.kind = id.to_string();
            self.fill_fields();
        }
    }

    /// The fields the kind has that nothing has been typed into yet start with the note's properties.
    fn fill_fields(&mut self) {
        let Some(kind) = self.current_kind() else { return };
        let fill = writer::prefill(&self.title, &self.props, kind);
        let defaults = self.current_set().map(|s| s.settings.clone()).unwrap_or_default();
        for (k, v) in fill {
            // the author and the department of the set's settings start an empty field (TEX-018)
            let v = if v.is_empty() { writer::settings_default(&defaults, &k).unwrap_or(v) } else { v };
            self.values.entry(k).or_insert(v);
        }
    }

    /// Where the setup starts when it opens by itself: the first of TeX and the sets that is
    /// missing (a missing logo is only pointed out, it does not open anything).
    pub fn auto_setup_step(&self) -> Option<Step> {
        match &self.status {
            TexStatus::Missing(_) => Some(Step::Tex),
            _ if self.sets.is_empty() => Some(Step::Templates),
            _ => None,
        }
    }

    /// Whether the kind has a table of contents to switch on (TEX-009).
    pub fn kind_has_toc(&self) -> bool {
        self.current_kind().is_some_and(|k| matches!(k.mapping, Mapping::Article | Mapping::Book))
    }

    /// Whether the kind has a place for the set's department logo (TEX-035).
    pub fn kind_shows_department_logo(&self) -> bool {
        self.current_kind().is_some_and(|k| !k.department_logo.is_empty())
    }

    pub fn working(&self) -> bool {
        self.job.is_some()
    }

    /// Whether Export can be pressed: a TeX, a kind, no export running (in this dialog or in one
    /// that was closed while its export was still stopping, JOB-005).
    pub fn can_export(&self, busy: &Busy) -> bool {
        matches!(self.status, TexStatus::Ready(_)) && self.current_kind().is_some() && self.job.is_none() && !busy.is_running(&TEX_EXPORT) && self.blocked_by().is_empty() && self.installing.is_none()
    }

    /// The packages this document needs that the managed TeX Live lacks: the compile would fail
    /// with "package ... is missing", so Export waits for the Install button (TEX-052).
    pub fn blocked_by(&self) -> Vec<&str> {
        self.needs.iter().map(String::as_str).filter(|n| self.missing.iter().any(|m| m == n)).collect()
    }

    /// What the Install button installs: the packages of the list the managed TeX Live lacks, and
    /// the ones the last compile found missing (nothing for a TeX of this computer).
    pub fn wanted_packages(&self) -> Vec<String> {
        let mut names = if self.managed { self.missing.clone() } else { vec![] };
        for m in self.failure.iter().filter(|_| self.managed).flat_map(|f| &f.missing) {
            if !names.contains(m) {
                names.push(m.clone());
            }
        }
        names
    }

    /// Compile a document against the chosen set as a task of the registry (TEX-004, JOB-018).
    /// False: a LaTeX export already runs, and nothing starts.
    pub fn start(&mut self, busy: &Busy, engine: Engine, document: Document, notify: impl FnOnce() + Send + 'static) -> bool {
        let Some(set) = self.current_set().cloned() else { return true };
        let Some(mut task) = busy.begin(TEX_EXPORT, format!("Exporting “{}” to PDF with LaTeX…", self.title)) else { return false };
        self.failure = None;
        let (doc, cancel) = (Arc::new(document), task.cancellable().flag());
        self.job = Some(ExportJob(task.run(move || compile_job(&engine, &set, &doc, cancel), notify)));
        true
    }

    /// What the finished job made, once; a failure is kept for the dialog to show.
    pub fn poll(&mut self) -> Option<Made> {
        let got = self.job.as_ref()?.0.try_recv().ok()?;
        self.job = None;
        match got {
            Ok(Ok(made)) => Some(made),
            Ok(Err(f)) => {
                self.failure = Some(*f);
                None
            }
            Err(crash) => {
                self.failure = Some(Failure::said(crash));
                None
            }
        }
    }

    /// The line "The managed TeX Live misses: geometry" with its Install button, what the install is
    /// doing, and why Export waits (TEX-052).
    fn packages_block(&mut self, ui: &mut Ui, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
        let wanted = self.wanted_packages();
        if let Some(what) = &self.installing {
            ui.horizontal(|ui| {
                ui.spinner();
                rec("installing", ui.label(what));
            });
        } else if !wanted.is_empty() {
            ui.horizontal_wrapped(|ui| {
                rec("missing-packages", crate::theme::problem(ui, super::settings_ui::missing_line(&wanted)));
                if rec("install-packages", ui.button("Install")).on_hover_text(format!("tlmgr install {}", wanted.join(" "))).clicked() {
                    self.install_requested = true;
                }
            });
            let blocked = self.blocked_by();
            if !blocked.is_empty() {
                rec("blocked", crate::theme::prose(ui, muted_text(&format!("This document needs {}: install it, then export.", blocked.join(", ")))));
            }
        }
        if let Some(e) = &self.install_error {
            rec("install-error", crate::theme::problem(ui, e));
        }
    }

    /// Show the choices: set, kind, fields, contents, the state of TeX and of the export.
    pub fn show(&mut self, ui: &mut Ui, opts: &mut ExportOptions, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
        let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::palette().muted);
        if self.sets.is_empty() {
            ui.horizontal_wrapped(|ui| {
                rec("no-sets", crate::theme::problem(ui, "This vault has no template sets yet: import one in Settings → LaTeX → Template sets."));
                if rec("setup:sets", ui.button("Set up…")).on_hover_text("The guided setup imports the templates").clicked() {
                    self.setup_requested = Some(Step::Templates);
                }
            });
        }
        if let Some(w) = &self.template_warning {
            rec("template-warning", crate::theme::problem(ui, w));
        }
        let busy = self.working();
        egui::Grid::new("export-tex-grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
            if busy {
                ui.disable();
            }
            {
                ui.label(muted("Template set"));
                let mut picked = None;
                let shown = self.current_set().map(|s| s.meta.name.clone()).unwrap_or_else(|| "none".into());
                let combo = egui::ComboBox::from_id_salt("export-tex-set").width(250.0).selected_text(shown).show_pointer(ui, |ui| {
                    for (i, s) in self.sets.iter().enumerate() {
                        if rec(&format!("set:{}", s.id), ui.selectable_label(self.set == i, &s.meta.name)).clicked() {
                            picked = Some(i);
                        }
                    }
                });
                rec("set", combo.response);
                if let Some(i) = picked {
                    self.select_set(i, None);
                    self.template_warning = None;
                }
                ui.end_row();

                ui.label(muted("Kind"));
                let mut picked = None;
                let shown = self.current_kind().map(|k| k.name.clone()).unwrap_or_else(|| "none".into());
                let combo = egui::ComboBox::from_id_salt("export-tex-kind").width(250.0).height(360.0).selected_text(shown).show_pointer(ui, |ui| {
                    for (id, k) in self.current_set().map(|s| s.kinds.clone()).unwrap_or_default() {
                        // a folder's dialog greys the kinds that cannot hold a folder, and says why
                        let label = ui.add_enabled(self.allows(&k), egui::Button::selectable(self.kind == id, &k.name));
                        if rec(&format!("kind:{id}"), label).on_hover_text(&k.description).on_disabled_hover_text(ONLY_ARTICLES_AND_BOOKS).clicked() {
                            picked = Some(id);
                        }
                    }
                });
                rec("kind", combo.response);
                if let Some(id) = picked {
                    self.select_kind(&id);
                    self.template_warning = None;
                }
                ui.end_row();

                let fields = self.current_kind().map(|k| k.fields.clone()).unwrap_or_default();
                for f in fields {
                    let label = if f.label.is_empty() { f.key.clone() } else { f.label.clone() };
                    ui.label(muted(&format!("{label}{}", if f.required { " *" } else { "" })));
                    let hint = if f.default.is_empty() { String::new() } else { format!("{} if empty", f.default) };
                    let value = self.values.entry(f.key.clone()).or_default();
                    rec(&format!("field:{}", f.key), ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(250.0)));
                    ui.end_row();
                }
                ui.label(muted("Include"));
                ui.vertical(|ui| {
                    let toc = self.kind_has_toc();
                    ui.add_enabled_ui(toc, |ui| {
                        // a kind without a table of contents shows the box off and greyed
                        let mut off = false;
                        let on = if toc { &mut opts.toc } else { &mut off };
                        rec("toc", crate::clickable::pointer(ui.checkbox(on, "Table of contents"))).on_disabled_hover_text("This kind of document has no table of contents");
                    });
                    let on = toc && opts.toc;
                    crate::pdf::export_dialog::toc_break_box(ui, opts, on, rec);
                    crate::pdf::export_dialog::break_before_box(ui, opts, toc, rec);
                    rec("embeds", crate::clickable::pointer(ui.checkbox(&mut opts.embeds, "Embedded pictures, drawings and PDFs")));
                    // the set's department logo (TEX-035): off and greyed for a kind that has no place for it
                    if self.current_set().is_some_and(|s| s.department_logo.is_some()) {
                        let shows = self.kind_shows_department_logo();
                        ui.add_enabled_ui(shows, |ui| {
                            let mut off = false;
                            let on = if shows { &mut opts.department_logo } else { &mut off };
                            rec("department-logo", crate::clickable::pointer(ui.checkbox(on, "Department logo"))).on_disabled_hover_text("This kind does not show a department logo");
                        });
                    }
                });
                ui.end_row();
            }
        });
        if self.folder {
            rec("folder-kinds", ui.label(muted(&format!("{ONLY_ARTICLES_AND_BOOKS}."))));
        }
        match &self.status {
            TexStatus::Checking => {
                rec("tex-status", ui.label(muted("Looking for a TeX…")));
            }
            TexStatus::Ready(t) => {
                rec("tex-status", crate::theme::prose(ui, muted(t)));
            }
            TexStatus::Missing(why) => {
                ui.horizontal_wrapped(|ui| {
                    rec("tex-status", crate::theme::problem(ui, "No TeX: install it in Settings → LaTeX"));
                    if rec("setup:tex", ui.button("Set up…")).on_hover_text("The guided setup installs TeX for you").clicked() {
                        self.setup_requested = Some(Step::Tex);
                    }
                });
                crate::theme::prose(ui, muted(why));
            }
        }
        if let (Some(set), Some(kind)) = (self.current_set(), self.current_kind()) {
            if kind.logo && set.logo == LogoStatus::Missing {
                ui.horizontal_wrapped(|ui| {
                    rec("logo-warning", crate::theme::problem(ui, "The set's logo is missing: the PDF gets a grey placeholder and is not CI-conformant. Choose the logo in Settings → LaTeX → Template sets."));
                    if rec("setup:logo", ui.button("Set up…")).on_hover_text("The guided setup asks for the logo").clicked() {
                        self.setup_requested = Some(Step::Logo);
                    }
                });
            }
        }
        self.packages_block(ui, rec);
        if self.working() {
            ui.horizontal(|ui| {
                ui.spinner();
                rec("working", ui.label("Compiling with LuaLaTeX… Cancel stops it."));
            });
        }
        if let Some(f) = &self.failure {
            crate::theme::problem(ui, &f.headline);
            if self.managed && !f.missing.is_empty() {
                rec("missing-pointer", crate::theme::problem(ui, format!("{} of Shale's package list {} not in the managed TeX Live: press Install above.", f.missing.join(", "), if f.missing.len() == 1 { "is" } else { "are" })));
            }
            for p in f.problems.iter().take(6) {
                rec("problem", ui.label(egui::RichText::new(p).monospace().size(12.0)));
            }
            if !f.log_tail.is_empty() {
                egui::CollapsingHeader::new("The end of the log").show_pointer(ui, |ui| {
                    ui.label(egui::RichText::new(&f.log_tail).monospace().size(11.0));
                });
            }
            if !f.document.tex.is_empty() && rec("save-tex", ui.button("Save the .tex")).on_hover_text("Write main.tex and its pictures into a folder next to the PDF, to debug it with your own TeX").clicked() {
                self.save_requested = true;
            }
        }
    }
}

fn muted_text(t: &str) -> egui::RichText {
    egui::RichText::new(t).size(12.0).color(theme::palette().muted)
}

/// Compile a written document against a set as a job (TEX-004); the export dialog and the test
/// page of the guided setup (TEX-017) both run it.
pub fn spawn_compile(engine: Engine, set: SetInfo, doc: Arc<Document>, notify: impl FnOnce() + Send + 'static) -> Job {
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let rx = crate::jobs::spawn(move || compile_job(&engine, &set, &doc, c), notify);
    Job { rx, cancel }
}

/// Compile a written document against a set and turn the answer into what the dialog needs.
fn compile_job(engine: &Engine, set: &SetInfo, doc: &Arc<Document>, cancel: Arc<AtomicBool>) -> Result<Made, Box<Failure>> {
    let files: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let fail = |e: CompileError| {
        let mut missing: Vec<String> = vec![];
        for pkg in e.problems.iter().filter_map(|p| p.missing.as_deref()).filter_map(super::engine::package_of_file) {
            if !missing.iter().any(|m| m == pkg) {
                missing.push(pkg.to_string());
            }
        }
        Failure { headline: e.headline.clone(), problems: e.problems.iter().map(|p| p.text()).collect(), log_tail: e.log_tail, document: doc.clone(), set_dir: set.dir.clone(), missing: missing.into() }
    };
    match sets::compile_with_set(engine, set, "main.tex", &doc.tex, &files, Options { cancel: Some(cancel), fonts: doc.fonts.clone(), ..Options::default() }) {
        Ok(out) => {
            let warnings = doc.warnings.iter().cloned().chain(out.output.warnings).collect();
            Ok(Made { pdf: out.output.pdf, placeholder_logo: out.placeholder_logo, warnings })
        }
        Err(e) => Err(Box::new(fail(e))),
    }
}

/// The `README.txt` of a saved `.tex` (TEX-016): the command that points `TEXINPUTS` at the set's
/// `tex` folder (`tex_dir`, the trailing `//` searches its folders) for the system, which is
/// `windows` here so both forms can be tested: cmd and PowerShell take no `VAR=… command`, and
/// kpathsea separates with `;` there (`compile::SEARCH_PATH_SEP`).
pub(crate) fn readme_text(tex_dir: &str, windows: bool) -> String {
    let how = if windows {
        format!("In cmd:\n\n  set \"TEXINPUTS={tex_dir}//;\"\n  lualatex main.tex\n\nIn PowerShell:\n\n  $env:TEXINPUTS=\"{tex_dir}//;\"\n  lualatex main.tex\n")
    } else {
        format!("  TEXINPUTS=\"{tex_dir}//:\" lualatex main.tex\n")
    };
    format!(
        "This is what Shale wrote for the export that failed: main.tex and the files it uses.\n\nTo compile it with your own TeX, point TEXINPUTS at the template set (the trailing // searches its folders), then run LuaLaTeX and biber:\n\n{how}\nIf the set needs a logo (tuda_logo.pdf) that is not in its logo folder, put a PDF of that name next to main.tex.\n"
    )
}

/// "Save the .tex": `main.tex` and its pictures in a folder next to the PDF (`name-tex`), and a
/// note on how to compile it with the template set. Returns the folder.
pub fn save_tex(failure: &Failure, pdf_path: &Path) -> Result<PathBuf, String> {
    let stem = pdf_path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "export".into());
    let dir = pdf_path.with_file_name(format!("{stem}-tex"));
    std::fs::create_dir_all(&dir).map_err(|e| format!("The folder {} could not be made: {e}", dir.display()))?;
    let write = |name: &str, bytes: &[u8]| crate::fsutil::write_atomic(&dir.join(name), bytes).map_err(|e| format!("{name} could not be written: {e}"));
    write("main.tex", failure.document.tex.as_bytes())?;
    for (name, bytes) in &failure.document.files {
        write(name, bytes)?;
    }
    let readme = readme_text(&failure.set_dir.join("tex").display().to_string(), cfg!(windows));
    write("README.txt", readme.as_bytes())?;
    Ok(dir)
}
