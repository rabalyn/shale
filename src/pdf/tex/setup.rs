//! The guided setup of the LaTeX export (TEX-017): the steps TeX, templates, logo, your details and
//! a test page, for people who are not firm in TeX but want to use a university's templates. This
//! file is the state and what the steps do; the dialog is `setup_ui`, and what needs the window
//! (the vault for the test page, the dialogs it returns to) is in `app/tex_setup.rs`. The steps
//! reuse what Settings → LaTeX has: the install (`install`, with its consent text and progress),
//! the import of a set (`import`), the logo slot (`sets::choose_logo`) and the writer; there is no
//! second way to do any of it.

use super::dialog::{self, Failure, Job, Made};
use super::engine::TexConfig;
use super::import::What;
use super::sets::{self, KindManifest, Language, LogoStatus, SetInfo, SetSettings};
use super::sets_ui::{expand_home, Form, VaultRef};
use super::settings_ui::{Notice, TexState};
use super::writer::{self, Document};
use crate::vault::Vault;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The steps, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum Step {
    #[default]
    Tex,
    Templates,
    Logo,
    Details,
    Test,
}

impl Step {
    pub const ALL: [Step; 5] = [Step::Tex, Step::Templates, Step::Logo, Step::Details, Step::Test];

    pub fn title(self) -> &'static str {
        match self {
            Step::Tex => "TeX",
            Step::Templates => "Templates",
            Step::Logo => "Logo",
            Step::Details => "Your details",
            Step::Test => "Test page",
        }
    }

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(0)
    }

    fn next(self) -> Option<Step> {
        Self::ALL.get(self.index() + 1).copied()
    }

    fn prev(self) -> Option<Step> {
        self.index().checked_sub(1).map(|i| Self::ALL[i])
    }
}

/// Where the person goes when the setup ends (finished or cancelled).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Return {
    #[default]
    Nothing,
    /// Settings, on the LaTeX tab: the button there opened the setup.
    Settings,
    /// The export dialog of this note: it opened the setup by itself or by a "Set up…" button.
    Export(PathBuf),
}

/// What the person picked on the templates step.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Choice {
    /// A set that is in the vault already (its folder name).
    Existing(String),
    /// The tested TU Darmstadt version.
    #[default]
    Preset,
    /// The newest version from CTAN.
    Ctan,
    /// A `.zip`, `.tar.gz` or folder (the path is in `TexSetup::file`).
    File,
    /// A GitHub repository (`TexSetup::github_url` and `github_ref`).
    Github,
}

/// What the test page made.
pub enum Outcome {
    /// A PDF; the opener's answer, and whether it has a placeholder logo.
    Made { placeholder: bool, opened: Result<(), String>, warnings: Vec<String> },
    /// The readable error and what is needed to try it by hand ("Save the .tex").
    Failed(Failure),
}

/// What TeX is like right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TexFact {
    /// Shale is still looking.
    Looking,
    /// "the TeX Live that Shale installs (LaTeX kernel 2025-11-01)".
    Found(String),
    /// No usable TeX, and why.
    Missing(String),
}

/// An accent colour of the templates: its code (`9c`) and its sRGB value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Swatch {
    pub code: String,
    pub rgb: [u8; 3],
}

/// The accent colours a tuda-ci set defines: the lines `\definecolor{TUDa-9c}{cmyk/RGB/HTML}{…/…/B90F22}`
/// of its `tudacolors.def`, ordered by row (a to d) and column (0 to 11). Another set has none, and
/// the accent is typed instead.
pub fn read_accents(set_dir: &Path) -> Vec<Swatch> {
    let Ok(text) = crate::fsutil::read_to_string(set_dir.join("tex").join("tudacolors.def")) else { return vec![] };
    parse_accents(&text)
}

pub(super) fn parse_accents(def: &str) -> Vec<Swatch> {
    let mut found: Vec<Swatch> = def
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("\\definecolor{TUDa-")?;
            let (code, rest) = rest.split_once('}')?;
            let hex = rest.rsplit_once('{')?.1.strip_suffix('}')?.rsplit('/').next()?.trim();
            let channel = |i: usize| hex.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok());
            let valid = !code.is_empty() && code.chars().all(|c| c.is_ascii_alphanumeric()) && hex.len() == 6;
            let rgb = [channel(0)?, channel(2)?, channel(4)?];
            valid.then(|| Swatch { code: code.to_string(), rgb })
        })
        .collect();
    let key = |s: &Swatch| (s.code.chars().last().unwrap_or('z'), s.code.trim_end_matches(|c: char| c.is_ascii_alphabetic()).parse::<u32>().unwrap_or(99));
    found.sort_by_key(key);
    found
}

/// The choices of the details step.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub accent: String,
    pub language: Language,
    pub department: String,
    pub author: String,
}

impl Details {
    pub fn from_settings(s: &SetSettings) -> Details {
        Details { accent: s.accent.clone(), language: s.language, department: s.department.clone(), author: s.author.clone() }
    }

    /// The settings with these details put in; the logo stays.
    pub fn apply(&self, base: &SetSettings) -> SetSettings {
        SetSettings { accent: self.accent.trim().to_string(), language: self.language, department: self.department.trim().to_string(), author: self.author.trim().to_string(), logo: base.logo.clone() }
    }
}

/// Write the details into the set's `settings.toml` (TEX-017). A re-import keeps that file.
pub fn save_details(vault: &VaultRef, set: &SetInfo, d: &Details) -> Result<(), String> {
    let s = d.apply(&set.settings);
    s.check()?;
    let _lock = vault.tree.hold_for(Duration::from_secs(5)).ok_or("The sync is still changing files in the vault: try again in a moment.")?;
    sets::save_settings(&set.dir, &s)
}

/// The note of the test page.
pub const TEST_NOTE: &str = "# Shale test page\n\nThis page was made by the guided setup of the LaTeX export, from a small note, with your template set and your details.\n\n## A section\n\nSome text with **bold**, *italic* and `code`, and a list:\n\n- one\n- two\n\n## A table\n\n| Name | Value |\n| --- | --- |\n| Accent | as chosen |\n| Language | as chosen |\n";

/// The name the test note has (it is not in the vault).
pub const TEST_NAME: &str = "Shale test page.md";

/// The kind of the test page: the publication; with a department given, the first kind that shows
/// it on its title page (a thesis), so that the page shows the details; else the first article kind.
pub fn test_kind(set: &SetInfo) -> Option<&(String, KindManifest)> {
    let shows_department = |k: &KindManifest| matches!(k.mapping, sets::Mapping::Article | sets::Mapping::Book) && k.fields.iter().any(|f| f.key == "department");
    if !set.settings.department.trim().is_empty() {
        let mut with: Vec<&(String, KindManifest)> = set.kinds.iter().filter(|(_, k)| shows_department(k)).collect();
        with.sort_by_key(|(id, _)| id != "thesis");
        if let Some(k) = with.first() {
            return Some(k);
        }
    }
    sets::test_kind(set)
}

/// The test page written for a set: the sample note as the kind of `test_kind` with the set's
/// settings (the department and author come from them), and a sample text for every other field
/// that has no value of its own (a thesis wants a reviewer, a title page its subtitle). Returns the
/// kind's id and the document.
pub fn test_page(vault: &Vault, set: &SetInfo) -> Result<(String, Document), String> {
    let (id, kind) = test_kind(set).ok_or_else(|| format!("“{}” has no article kind to make a test page of (see its README)", set.meta.name))?;
    let mut fields = BTreeMap::new();
    for f in &kind.fields {
        // the title is the note's name, the date is today's, the rest has a default or the settings' value
        let has_value = matches!(f.key.as_str(), "title" | "subject" | "date") || !f.default.is_empty() || writer::settings_default(&set.settings, &f.key).is_some();
        if !has_value {
            fields.insert(
                f.key.clone(),
                match f.key.as_str() {
                    "author" => "Your Name".to_string(),
                    "subtitle" => "A test of the template set".to_string(),
                    key => format!("Sample {key}"),
                },
            );
        }
    }
    let opts = crate::pdf::export::ExportOptions::for_note();
    let doc = writer::write(&writer::Request { vault, rel: Path::new(TEST_NAME), text: TEST_NOTE, opts: &opts, set, kind, fields: &fields })?;
    Ok((id.clone(), doc))
}

/// What a file dropped on the setup or on Settings → LaTeX is for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dropped {
    /// A PDF or a PNG: the logo.
    Logo,
    /// A `.zip`, a `.tar.gz` or a folder: template sets.
    Archive,
    Other,
}

/// What a dropped path is, by what it is called (the import and the logo check the contents).
pub fn classify(path: &Path) -> Dropped {
    let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    if path.is_dir() || name.ends_with(".zip") || name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        Dropped::Archive
    } else if name.ends_with(".pdf") || name.ends_with(".png") {
        Dropped::Logo
    } else {
        Dropped::Other
    }
}

/// The state of the guided setup (TEX-017), kept next to the rest of the LaTeX state.
#[derive(Default)]
pub struct TexSetup {
    pub step: Step,
    pub ret: Return,
    /// The set the setup works on (its folder name).
    pub set: Option<String>,
    pub choice: Choice,
    pub file: String,
    pub github_url: String,
    pub github_ref: String,
    pub logo_path: String,
    pub details: Details,
    pub accents: Vec<Swatch>,
    /// What the last action of the step said (a logo copied, details that could not be saved).
    pub message: Option<Notice>,
    /// `perl` is missing (the install needs it on Linux and macOS), found out when the TeX step first needs it.
    pub perl: Option<Option<String>>,
    /// The set ids before an import the setup started, to find the new one afterwards.
    pub before: Vec<String>,
    /// An import started by the setup is running.
    pub importing: bool,
    /// The test page: the compile job, what it made, and the window is to start it.
    pub job: Option<Job>,
    pub outcome: Option<Outcome>,
    pub run_test: bool,
    /// Where "Save the .tex" put the files.
    pub saved_tex: Option<PathBuf>,
}

impl TexState {
    /// What TeX is like: found, still looked for, or why none is usable.
    pub fn tex_fact(&self, cfg: &TexConfig) -> TexFact {
        match &self.detection {
            None => TexFact::Looking,
            Some(d) => match d.selected(cfg.engine) {
                Ok(e) => TexFact::Found(format!("{} (LaTeX kernel {})", super::settings_ui::engine_name(e), e.kernel)),
                Err(why) => TexFact::Missing(why),
            },
        }
    }

    /// The sets of the vault that have document kinds.
    pub fn usable_sets(&self) -> Vec<&SetInfo> {
        self.sets.sets.iter().flatten().filter_map(|e| e.info.as_ref().ok()).filter(|s| !s.kinds.is_empty()).collect()
    }

    /// The set the setup works on.
    pub fn chosen_set(&self) -> Option<&SetInfo> {
        let id = self.setup.set.as_deref()?;
        self.usable_sets().into_iter().find(|s| s.id == id)
    }

    /// Whether a step is done already: it shows green, and Next passes it.
    pub fn step_done(&self, cfg: &TexConfig, step: Step) -> bool {
        let set = self.chosen_set();
        match step {
            Step::Tex => matches!(self.tex_fact(cfg), TexFact::Found(_)),
            Step::Templates => set.is_some(),
            Step::Logo => set.is_some_and(|s| s.logo != LogoStatus::Missing),
            Step::Details => set.is_some_and(|s| !s.settings.accent.is_empty()),
            Step::Test => matches!(self.setup.outcome, Some(Outcome::Made { .. })),
        }
    }

    /// Open the setup, at this step or at the first of TeX, templates and logo that is not done
    /// (the details when all three are). Call it with the sets of the vault read.
    pub fn open_setup(&mut self, cfg: &TexConfig, at: Option<Step>, ret: Return) {
        self.setup = TexSetup { ret, ..TexSetup::default() };
        self.notice = None;
        self.setup.set = self.usable_sets().first().map(|s| s.id.clone());
        if let Some(id) = &self.setup.set {
            self.setup.choice = Choice::Existing(id.clone());
        }
        let step = at.unwrap_or_else(|| [Step::Tex, Step::Templates, Step::Logo].into_iter().find(|s| !self.step_done(cfg, *s)).unwrap_or(Step::Details));
        self.setup_go(step);
    }

    /// Show a step (the details are read from the set's settings, the test page is started).
    pub fn setup_go(&mut self, step: Step) {
        self.setup.step = step;
        self.setup.message = None;
        self.notice = None;
        match step {
            Step::Details => {
                let (details, accents) = self.chosen_set().map(|s| (Details::from_settings(&s.settings), read_accents(&s.dir))).unwrap_or_default();
                self.setup.details = details;
                self.setup.accents = accents;
            }
            Step::Test => {
                self.setup.outcome = None;
                self.setup.saved_tex = None;
                self.setup.run_test = true;
            }
            _ => {}
        }
    }

    /// Whether Next can be pressed: a step that is not done must be done first, except the logo
    /// (it can wait) and the details.
    pub fn setup_can_next(&self, cfg: &TexConfig) -> bool {
        match self.setup.step {
            Step::Tex | Step::Templates => self.step_done(cfg, self.setup.step) && self.task.is_none(),
            Step::Logo | Step::Details => self.chosen_set().is_some() && self.task.is_none(),
            Step::Test => false,
        }
    }

    /// Save what the details step holds (a no-op on the other steps).
    pub fn setup_save_details(&mut self) -> Result<(), String> {
        if self.setup.step != Step::Details {
            return Ok(());
        }
        let (Some(vault), Some(set)) = (self.sets.vault.clone(), self.chosen_set().cloned()) else { return Ok(()) };
        save_details(&vault, &set, &self.setup.details)?;
        self.sets.dirty = true;
        self.sets.refresh();
        Ok(())
    }

    /// Next: the next step. Leaving the details saves them; when that fails the step stays and says why.
    pub fn setup_next(&mut self, cfg: &TexConfig) {
        if !self.setup_can_next(cfg) {
            return;
        }
        if let Err(e) = self.setup_save_details() {
            self.setup.message = Some(Notice::new(format!("Your details were not saved: {e}"), false));
            return;
        }
        if let Some(n) = self.setup.step.next() {
            self.setup_go(n);
        }
    }

    /// Back: the step before. Leaving the details saves them.
    pub fn setup_back(&mut self) {
        if let Err(e) = self.setup_save_details() {
            self.setup.message = Some(Notice::new(format!("Your details were not saved: {e}"), false));
            return;
        }
        if let Some(p) = self.setup.step.prev() {
            self.setup_go(p);
        }
    }

    /// Use a set that is in the vault.
    pub fn setup_use_set(&mut self, id: &str) {
        if self.usable_sets().iter().any(|s| s.id == id) {
            self.setup.set = Some(id.to_string());
            self.setup.choice = Choice::Existing(id.to_string());
        }
    }

    /// Import what the templates step has chosen, as a job with progress and Cancel.
    pub fn setup_import(&mut self, ctx: &eframe::egui::Context, cfg: &TexConfig) {
        let what = match &self.setup.choice {
            Choice::Existing(id) => {
                let id = id.clone();
                self.setup_use_set(&id);
                return;
            }
            Choice::Preset => What::Preset,
            Choice::Ctan => What::Ctan,
            Choice::File => What::Local(expand_home(&self.setup.file)),
            Choice::Github => What::Github { url: self.setup.github_url.clone(), reference: self.setup.github_ref.clone() },
        };
        self.setup.before = self.usable_sets().iter().map(|s| s.id.clone()).collect();
        self.setup.importing = true;
        self.start_import(ctx, cfg, what);
        if self.task.is_none() {
            self.setup.importing = false;
        }
    }

    /// Copy a logo file into the chosen set and say how it went.
    pub fn setup_logo(&mut self, from: &Path) {
        let Some(id) = self.chosen_set().map(|s| s.id.clone()) else { return };
        self.setup.message = Some(self.sets.copy_logo_to(&id, from));
        self.sets.refresh();
    }

    /// Take what jobs have finished (an import, the test page); call it every frame while the setup
    /// is open: the import ends with the new set chosen, the test page with its outcome.
    pub fn setup_tick(&mut self, ctx: &eframe::egui::Context) {
        self.sets.refresh();
        if self.setup.importing && self.task.is_none() {
            self.setup.importing = false;
            let ok = self.notice.as_ref().is_some_and(|n| n.ok);
            let before = std::mem::take(&mut self.setup.before);
            let usable: Vec<String> = self.usable_sets().iter().map(|s| s.id.clone()).collect();
            if ok || !usable.is_empty() {
                let pick = usable.iter().find(|id| !before.contains(id)).or_else(|| self.setup.set.as_ref().filter(|id| usable.contains(id))).or(usable.first()).cloned();
                if let Some(id) = pick {
                    self.setup_use_set(&id);
                }
            }
        }
        if let Some(job) = &self.setup.job {
            if let Ok(got) = job.rx.try_recv() {
                self.setup.job = None;
                self.setup.outcome = Some(match got {
                    Ok(Ok(made)) => made_outcome(made),
                    Ok(Err(f)) => Outcome::Failed(f),
                    Err(crash) => Outcome::Failed(dialog::Failure { headline: crash, problems: vec![], log_tail: String::new(), document: Default::default(), set_dir: PathBuf::new() }),
                });
            }
            ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// Files dropped on the open setup: on the logo step a PDF or PNG becomes the logo, on the
    /// templates step an archive or a folder is imported. Returns what to tell the person when
    /// nothing was taken. The files are never imported into the vault as notes.
    pub fn setup_drop(&mut self, ctx: &eframe::egui::Context, cfg: &TexConfig, paths: &[PathBuf]) -> Option<String> {
        match self.setup.step {
            Step::Logo => match paths.iter().find(|p| classify(p) == Dropped::Logo) {
                Some(p) => {
                    self.setup.logo_path = p.to_string_lossy().into_owned();
                    self.setup_logo(p);
                    None
                }
                None => Some("Drop the logo here as a PDF or a PNG file.".into()),
            },
            Step::Templates if self.task.is_none() => match paths.iter().find(|p| classify(p) == Dropped::Archive) {
                Some(p) => {
                    self.setup.choice = Choice::File;
                    self.setup.file = p.to_string_lossy().into_owned();
                    self.setup_import(ctx, cfg);
                    None
                }
                None => Some("Drop the templates here as a .zip or .tar.gz file or as a folder.".into()),
            },
            Step::Templates => Some("Wait for the import that is running.".into()),
            _ => Some("There is nothing to drop on this step: the templates and the logo have their steps.".into()),
        }
    }

    /// Files dropped on Settings → LaTeX: an archive or folder is imported as a set, a PDF or PNG
    /// becomes the logo of the set whose "Choose logo…" is open, else of the only set that has a
    /// logo slot. Returns what to tell the person when nothing was taken.
    pub fn settings_drop(&mut self, ctx: &eframe::egui::Context, cfg: &TexConfig, paths: &[PathBuf]) -> Option<String> {
        if let Some(p) = paths.iter().find(|p| classify(p) == Dropped::Archive) {
            if self.task.is_some() {
                return Some("Wait for the job that is running.".into());
            }
            self.start_import(ctx, cfg, What::Local(p.clone()));
            return None;
        }
        let Some(logo) = paths.iter().find(|p| classify(p) == Dropped::Logo) else { return Some("Drop a .zip or .tar.gz file or a folder of templates, or a logo as a PDF or PNG file.".into()) };
        let with_slot: Vec<String> = self.sets.sets.iter().flatten().filter_map(|e| e.info.as_ref().ok()).filter(|s| s.meta.logo_file.is_some()).map(|s| s.id.clone()).collect();
        let target = match &self.sets.form {
            Form::Logo { set, .. } => Some(set.clone()),
            _ if with_slot.len() == 1 => with_slot.first().cloned(),
            _ => None,
        };
        match target {
            Some(id) => {
                self.sets.form = Form::None;
                self.notice = Some(self.sets.copy_logo_to(&id, logo));
                None
            }
            None => Some("Press “Choose logo…” of the set the logo is for, then drop it.".into()),
        }
    }
}

fn made_outcome(made: Made) -> Outcome {
    let opened = crate::opener::open_temp_copy("shale-test-page.pdf", &made.pdf);
    Outcome::Made { placeholder: made.placeholder_logo, opened, warnings: made.warnings }
}
