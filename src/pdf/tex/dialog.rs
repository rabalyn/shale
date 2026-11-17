//! The LaTeX choice of the note export dialog (TEX-016): the template set and kind, the fields of
//! the kind, the table of contents, the state of TeX, and the export as a job with Cancel; when it
//! fails, the readable errors and "Save the .tex" for someone who wants to debug it. The dialog
//! itself (`pdf::export_dialog`) shows the engine choice and calls [`TexExport::show`] for the rest.

use super::compile::{CompileError, Options};
use super::engine::Engine;
use super::sets::{self, KindManifest, LogoStatus, Mapping, SetInfo};
use super::setup::Step;
use super::writer::{self, Document};
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
    /// Embeds that were missing, as the writer said.
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
}

/// A running export. Dropping it (the dialog closed) stops LuaLaTeX.
pub struct Job {
    pub(super) rx: Receiver<Result<Result<Made, Failure>, String>>,
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
    pub job: Option<Job>,
    pub failure: Option<Failure>,
    /// "Save the .tex" was pressed: the window writes the folder.
    pub save_requested: bool,
    /// A "Set up…" button beside a missing TeX, set or logo was pressed: the window opens the
    /// guided setup (TEX-017) at this step.
    pub setup_requested: Option<Step>,
    /// The note's `latex-template:` names a set or kind the vault does not have: what was not
    /// found and what is used instead (TEX-033). Gone once the person picks a set or kind.
    pub template_warning: Option<String>,
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
        let sets: Vec<SetInfo> = sets::scan(vault).into_iter().filter_map(|e| e.info.ok()).filter(|s| !s.kinds.is_empty()).collect();
        let props = crate::markdown::front_matter_values(text);
        let mut t = TexExport { sets, set: 0, kind: String::new(), title, props, values: BTreeMap::new(), status: TexStatus::default(), job: None, failure: None, save_requested: false, setup_requested: None, template_warning: None };
        let (set, kind) = prefer.unzip();
        if let Some(i) = set.and_then(|id| t.sets.iter().position(|s| s.id == id)) {
            t.set = i;
        }
        t.select_set(t.set, kind.as_deref());
        t.template_warning = t.unknown_template();
        t
    }

    /// What the note's `latex-template:` asks for that the vault does not have, and what the
    /// dialog uses instead (TEX-033); None when it names nothing, or a set and kind that are there.
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
            Some(_) => None,
        }
    }

    pub fn current_set(&self) -> Option<&SetInfo> {
        self.sets.get(self.set)
    }

    pub fn current_kind(&self) -> Option<&KindManifest> {
        self.current_set()?.kinds.iter().find(|(id, _)| *id == self.kind).map(|(_, k)| k)
    }

    /// Choose a set, and a kind of it (the first when it has none of that name); the fields of the
    /// kind get their values from the note.
    pub fn select_set(&mut self, i: usize, kind: Option<&str>) {
        self.set = i.min(self.sets.len().saturating_sub(1));
        let Some(set) = self.sets.get(self.set) else { return };
        // the kind asked for, else the one the test page uses (the publication, else an article), else the first
        self.kind = kind.and_then(|k| set.kinds.iter().find(|(id, _)| id == k)).or_else(|| sets::test_kind(set)).or_else(|| set.kinds.first()).map(|(id, _)| id.clone()).unwrap_or_default();
        self.fill_fields();
    }

    pub fn select_kind(&mut self, id: &str) {
        if self.current_set().is_some_and(|s| s.kinds.iter().any(|(k, _)| k == id)) {
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

    pub fn working(&self) -> bool {
        self.job.is_some()
    }

    /// Whether Export can be pressed: a TeX, a kind, no export running.
    pub fn can_export(&self) -> bool {
        matches!(self.status, TexStatus::Ready(_)) && self.current_kind().is_some() && self.job.is_none()
    }

    /// Compile a document against the chosen set as a job (TEX-004).
    pub fn start(&mut self, engine: Engine, document: Document, notify: impl FnOnce() + Send + 'static) {
        let Some(set) = self.current_set().cloned() else { return };
        self.failure = None;
        self.job = Some(spawn_compile(engine, set, Arc::new(document), notify));
    }

    /// What the finished job made, once; a failure is kept for the dialog to show.
    pub fn poll(&mut self) -> Option<Made> {
        let got = self.job.as_ref()?.rx.try_recv().ok()?;
        self.job = None;
        match got {
            Ok(Ok(made)) => Some(made),
            Ok(Err(f)) => {
                self.failure = Some(f);
                None
            }
            Err(crash) => {
                self.failure = Some(Failure { headline: crash, problems: vec![], log_tail: String::new(), document: Arc::new(Document::default()), set_dir: PathBuf::new() });
                None
            }
        }
    }

    /// Show the choices: set, kind, fields, contents, the state of TeX and of the export.
    pub fn show(&mut self, ui: &mut Ui, opts: &mut ExportOptions, rec: &mut dyn FnMut(&str, egui::Response) -> egui::Response) {
        let muted = |t: &str| egui::RichText::new(t).size(12.0).color(theme::MUTED);
        if self.sets.is_empty() {
            ui.horizontal_wrapped(|ui| {
                rec("no-sets", ui.colored_label(theme::LINK_MISSING, "This vault has no template sets yet: import one in Settings → LaTeX → Template sets."));
                if rec("setup:sets", ui.button("Set up…")).on_hover_text("The guided setup imports the templates").clicked() {
                    self.setup_requested = Some(Step::Templates);
                }
            });
        }
        if let Some(w) = &self.template_warning {
            rec("template-warning", ui.colored_label(theme::LINK_MISSING, w));
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
                let combo = egui::ComboBox::from_id_salt("export-tex-set").width(250.0).selected_text(shown).show_ui(ui, |ui| {
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
                let combo = egui::ComboBox::from_id_salt("export-tex-kind").width(250.0).height(360.0).selected_text(shown).show_ui(ui, |ui| {
                    for (id, k) in self.current_set().map(|s| s.kinds.clone()).unwrap_or_default() {
                        if rec(&format!("kind:{id}"), ui.selectable_label(self.kind == id, &k.name)).on_hover_text(&k.description).clicked() {
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
                        rec("toc", ui.checkbox(on, "Table of contents")).on_disabled_hover_text("This kind of document has no table of contents");
                    });
                    rec("embeds", ui.checkbox(&mut opts.embeds, "Embedded pictures, drawings and PDFs"));
                });
                ui.end_row();
            }
        });
        ui.add_space(4.0);
        match &self.status {
            TexStatus::Checking => {
                rec("tex-status", ui.label(muted("Looking for a TeX…")));
            }
            TexStatus::Ready(t) => {
                rec("tex-status", ui.label(muted(t)));
            }
            TexStatus::Missing(why) => {
                ui.horizontal_wrapped(|ui| {
                    rec("tex-status", ui.colored_label(theme::LINK_MISSING, "No TeX: install it in Settings → LaTeX"));
                    if rec("setup:tex", ui.button("Set up…")).on_hover_text("The guided setup installs TeX for you").clicked() {
                        self.setup_requested = Some(Step::Tex);
                    }
                });
                ui.label(muted(why));
            }
        }
        if let (Some(set), Some(kind)) = (self.current_set(), self.current_kind()) {
            if kind.logo && set.logo == LogoStatus::Missing {
                ui.horizontal_wrapped(|ui| {
                    rec("logo-warning", ui.colored_label(theme::LINK_MISSING, "The set's logo is missing: the PDF gets a grey placeholder and is not CI-conformant. Choose the logo in Settings → LaTeX → Template sets."));
                    if rec("setup:logo", ui.button("Set up…")).on_hover_text("The guided setup asks for the logo").clicked() {
                        self.setup_requested = Some(Step::Logo);
                    }
                });
            }
        }
        if self.working() {
            ui.horizontal(|ui| {
                ui.spinner();
                rec("working", ui.label("Compiling with LuaLaTeX… Cancel stops it."));
            });
        }
        if let Some(f) = &self.failure {
            ui.add_space(4.0);
            ui.colored_label(theme::LINK_MISSING, &f.headline);
            for p in f.problems.iter().take(6) {
                rec("problem", ui.label(egui::RichText::new(p).monospace().size(12.0)));
            }
            if !f.log_tail.is_empty() {
                egui::CollapsingHeader::new("The end of the log").show(ui, |ui| {
                    ui.label(egui::RichText::new(&f.log_tail).monospace().size(11.0));
                });
            }
            if !f.document.tex.is_empty() && rec("save-tex", ui.button("Save the .tex")).on_hover_text("Write main.tex and its pictures into a folder next to the PDF, to debug it with your own TeX").clicked() {
                self.save_requested = true;
            }
        }
    }
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
fn compile_job(engine: &Engine, set: &SetInfo, doc: &Arc<Document>, cancel: Arc<AtomicBool>) -> Result<Made, Failure> {
    let files: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let fail = |e: CompileError| Failure { headline: e.headline.clone(), problems: e.problems.iter().map(|p| p.text()).collect(), log_tail: e.log_tail, document: doc.clone(), set_dir: set.dir.clone() };
    match sets::compile_with_set(engine, set, "main.tex", &doc.tex, &files, Options { cancel: Some(cancel), ..Options::default() }) {
        Ok(out) => Ok(Made { pdf: out.output.pdf, placeholder_logo: out.placeholder_logo, warnings: doc.warnings.clone() }),
        Err(e) => Err(fail(e)),
    }
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
    let readme = format!(
        "This is what Shale wrote for the export that failed: main.tex and the files it uses.\n\nTo compile it with your own TeX, point TEXINPUTS at the template set (the trailing // searches its folders), then run LuaLaTeX and biber:\n\n  TEXINPUTS=\"{}//:\" lualatex main.tex\n\nIf the set needs a logo (tuda_logo.pdf) that is not in its logo folder, put a PDF of that name next to main.tex.\n",
        failure.set_dir.join("tex").display()
    );
    write("README.txt", readme.as_bytes())?;
    Ok(dir)
}
