//! The dialog of the guided setup (TEX-017): a step indicator, the step, and Back, Next and Cancel
//! in the footer. Every step says in plain words what it is for and shows a green "done" when it
//! is done already; none of them needs to know what TeX Live, TEXINPUTS or docstrip are. What a
//! button asks for is an [`Act`]; [`apply`] carries it out on the state of the window.

use super::dialog::save_tex;
use super::engine::{self, TexConfig};
use super::import::{PRESET_COMMIT, PRESET_REPO};
use super::install::{self, DISK_MB, DOWNLOAD_MB};
use super::sets::{Language, LogoStatus};
use super::sets_ui::expand_home;
use super::sets_ui::Slot;
use super::settings_ui::{consent_text, Notice, TaskKind, TexState};
use super::setup::{Choice, Outcome, Step, TexFact};
use crate::app::muted;
use crate::theme;
use eframe::egui::{self, Color32, Context, Ui};
use std::path::Path;

/// The page of the university library that has the logo and the install scripts for it, as the
/// README of tuda-ci says (the logo itself is for members of the university).
pub const LOGO_PAGE: &str = "https://www.ulb.tu-darmstadt.de/finden_nutzen/medien_nutzen/dokumente_erstellen/latex/index.de.jsp";

/// Where members of the university download the logo directly (the README of tuda-ci).
pub const LOGO_DOWNLOAD: &str = "https://download.hrz.tu-darmstadt.de/protected/ULB/tuda_logo.pdf";

/// What a button of the dialog asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    Back,
    Next,
    Cancel,
    Finish,
    Install,
    Import,
    UseLogo,
    UseDepartmentLogo,
    OpenLogoPage,
    RunTest,
    SaveTex,
}

/// The step indicator: the five steps, the open one strong, the done ones green with a tick.
fn indicator(ui: &mut Ui, st: &TexState, cfg: &TexConfig) {
    ui.horizontal_wrapped(|ui| {
        for (i, s) in Step::ALL.into_iter().enumerate() {
            if i > 0 {
                ui.label(muted("›"));
            }
            let done = st.step_done(cfg, s);
            let text = format!("{} {}", if done { "✓".to_string() } else { (i + 1).to_string() }, s.title());
            let t = egui::RichText::new(text);
            ui.label(if s == st.setup.step {
                t.strong().color(theme::palette().text_strong)
            } else if done {
                t.color(theme::palette().ok_text)
            } else {
                t.color(theme::palette().muted)
            });
        }
    });
    ui.separator();
}

/// A line that says something is done.
fn done_line(ui: &mut Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).color(theme::palette().ok_text));
}

/// What the last job said, when it failed or when it has something to add.
fn notice(ui: &mut Ui, n: &Notice) {
    if n.ok {
        ui.label(&n.text);
    } else {
        crate::theme::problem(ui, &n.text);
    }
}

/// The content of the dialog.
pub fn show(ui: &mut Ui, st: &mut TexState, cfg: &TexConfig, acts: &mut Vec<Act>) {
    let ctx = ui.ctx().clone();
    if st.detection.is_none() && st.task.is_none() {
        st.check_again(&ctx);
    }
    indicator(ui, st, cfg);
    match st.setup.step {
        Step::Tex => tex_step(ui, st, cfg, acts),
        Step::Templates => templates_step(ui, st, acts),
        Step::Logo => logo_step(ui, st, acts),
        Step::Details => details_step(ui, st),
        Step::Test => test_step(ui, st, acts),
    }
}

fn tex_step(ui: &mut Ui, st: &mut TexState, cfg: &TexConfig, acts: &mut Vec<Act>) {
    crate::theme::paragraph(ui, "To make a PDF from a template, Shale needs a program called TeX.");
    // the install runs: its progress, what it does and Cancel (what was downloaded is removed)
    if let Some(t) = st.task.as_ref().filter(|t| t.kind == TaskKind::Install) {
        let p = t.handle.progress();
        ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
        crate::busy::ui::status(ui, &st.busy, &super::settings_ui::TEX_JOB);
        return;
    }
    match st.tex_fact(cfg) {
        TexFact::Looking => {
            // (no probe task: the state of a page whose own job keeps it from looking again)
            if !crate::busy::ui::status(ui, &st.busy, &super::settings_ui::TEX_PROBE) {
                ui.label(super::settings_ui::TEX_PROBE_TEXT);
            }
        }
        TexFact::Found(what) => {
            done_line(ui, format!("Found: {what} ✓"));
            crate::theme::hint(ui, "There is nothing to install.");
        }
        TexFact::Missing(why) => {
            crate::theme::paragraph(ui, "It is not on this computer yet. Shale can install its own copy: it lives in a folder of its own and changes nothing else on your computer.");
            match &st.data {
                None => {
                    crate::theme::problem(ui, "This computer has no data folder to keep TeX in, so Shale cannot install it.");
                }
                Some(data) => {
                    let folder = engine::managed_root(data);
                    crate::theme::hint(ui, consent_text(cfg.repository.trim_end_matches('/'), &folder.to_string_lossy()));
                    let perl = st.setup.perl.get_or_insert_with(install::perl_problem).clone();
                    if let Some(p) = &perl {
                        crate::theme::problem(ui, p);
                    }
                    let label = format!("Install (about {DOWNLOAD_MB} MB download, {DISK_MB} MB on disk)");
                    if ui.add_enabled(perl.is_none() && st.task.is_none(), egui::Button::new(label)).on_hover_text("Download and install TeX Live into Shale's own folder").clicked() {
                        acts.push(Act::Install);
                    }
                }
            }
            crate::theme::prose(ui, muted(format!("(Details: {why})")).size(11.0));
        }
    }
    if let Some(n) = &st.notice {
        notice(ui, n);
    }
}

fn templates_step(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    crate::theme::paragraph(ui, "The templates give your PDFs the look of your university: the title page, the colours, the fonts. They are kept in your vault, so they are backed up and synced with your notes.");
    if st.sets.vault.is_none() {
        crate::theme::problem(ui, "Open a vault first: the templates are kept in it.");
        return;
    }
    // an import runs: its progress, what it does and Cancel (the vault stays as it was)
    if let Some(t) = st.task.as_ref().filter(|t| t.kind == TaskKind::Import) {
        let p = t.handle.progress();
        ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
        crate::busy::ui::status(ui, &st.busy, &super::settings_ui::TEX_JOB);
        return;
    }
    if let Some(s) = st.chosen_set() {
        done_line(ui, format!("Using “{}” ✓", s.meta.name));
    }
    let existing: Vec<(String, String)> = st.usable_sets().iter().map(|s| (s.id.clone(), s.meta.name.clone())).collect();
    for (id, name) in existing {
        ui.radio_value(&mut st.setup.choice, Choice::Existing(id), format!("Use “{name}” (already in your vault)"));
    }
    ui.radio_value(&mut st.setup.choice, Choice::Preset, "TU Darmstadt (tested version)  — recommended");
    crate::theme::hint(ui, format!("tuda-ci 4.06 from GitHub ({PRESET_REPO} at {}), the version Shale was tested with.", PRESET_COMMIT.chars().take(7).collect::<String>()));
    ui.radio_value(&mut st.setup.choice, Choice::Ctan, "The newest version from CTAN");
    crate::theme::hint(ui, "Whatever CTAN has today: newer, but Shale was not tested with it.");
    ui.radio_value(&mut st.setup.choice, Choice::File, "From a file or a folder (a .zip or .tar.gz, or a folder of templates)");
    if st.setup.choice == Choice::File {
        ui.add(egui::TextEdit::singleline(&mut st.setup.file).hint_text("~/Downloads/templates.zip").desired_width(380.0));
        crate::theme::hint(ui, "You can also drag the file or folder onto this window.");
    }
    ui.radio_value(&mut st.setup.choice, Choice::Github, "From a GitHub repository");
    if st.setup.choice == Choice::Github {
        ui.add(egui::TextEdit::singleline(&mut st.setup.github_url).hint_text("https://github.com/owner/repository").desired_width(380.0));
        ui.add(egui::TextEdit::singleline(&mut st.setup.github_ref).hint_text("branch, tag or commit (empty: the default branch)").desired_width(380.0));
    }
    let existing_choice = matches!(st.setup.choice, Choice::Existing(_));
    if ui.button(if existing_choice { "Use this set" } else { "Import" }).clicked() {
        acts.push(Act::Import);
    }
    if let Some(n) = &st.notice {
        notice(ui, n);
    }
}

fn logo_step(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    let Some(set) = st.chosen_set() else {
        crate::theme::problem(ui, "Choose the templates first.");
        return;
    };
    let (status, name) = (set.logo.clone(), set.meta.name.clone());
    match &status {
        LogoStatus::NotNeeded => {
            done_line(ui, format!("“{name}” needs no logo ✓"));
        }
        LogoStatus::Present(f) => {
            done_line(ui, format!("The logo is there: {f} ✓"));
            crate::theme::hint(ui, "To use another one, choose it in Settings → LaTeX → Template sets.");
        }
        LogoStatus::Missing => {
            crate::theme::paragraph(ui, "The templates put the university's logo on the title page. The logo may not be shipped with Shale or with the templates, so you need to get it yourself.");
            crate::theme::paragraph(ui, "Members of the TU Darmstadt can get it from the university library's page about LaTeX at the corporate design of the university (in the university network, or through its VPN):");
            ui.horizontal(|ui| {
                crate::theme::prose(ui, muted(LOGO_PAGE).size(11.0));
            });
            ui.horizontal(|ui| {
                if ui.button("Open that page").clicked() {
                    acts.push(Act::OpenLogoPage);
                }
                crate::theme::prose(ui, muted(format!("Direct download in the university network: {LOGO_DOWNLOAD}")).size(11.0));
            });
            crate::theme::paragraph(ui, "Then give the file to Shale: drag it onto this window, or type or paste where it is.");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.setup.logo_path).hint_text("~/Downloads/tuda_logo.pdf").desired_width(320.0));
                if ui.add_enabled(!st.setup.logo_path.trim().is_empty(), egui::Button::new("Use this logo")).clicked() {
                    acts.push(Act::UseLogo);
                }
            });
            crate::theme::hint(ui, "Not now? Press Later: exports then carry a grey placeholder and are not CI-conformant. You can add the logo any time in Settings → LaTeX.");
        }
    }
    department_logo(ui, st, acts);
    if let Some(n) = &st.setup.message {
        notice(ui, n);
    }
}

/// The optional second field of the logo step: the logo of the person's department (TEX-035).
fn department_logo(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    let Some(set) = st.chosen_set() else { return };
    theme::section(ui, "Department logo (optional)");
    match set.department_logo_text() {
        Some(text) => {
            ui.label(format!("{text} ✓"));
        }
        None => {
            crate::theme::hint(ui, "Your department or institution may have a logo of its own: documents show it under the university's logo.");
        }
    }
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(&mut st.setup.department_logo_path).hint_text("~/Downloads/department_logo.png").desired_width(320.0));
        if ui.add_enabled(!st.setup.department_logo_path.trim().is_empty(), egui::Button::new("Use this department logo")).clicked() {
            acts.push(Act::UseDepartmentLogo);
        }
    });
}

/// Black or white, whichever reads on this colour.
fn contrast(rgb: [u8; 3]) -> Color32 {
    let lum = 0.299 * f32::from(rgb[0]) + 0.587 * f32::from(rgb[1]) + 0.114 * f32::from(rgb[2]);
    if lum > 140.0 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

fn details_step(ui: &mut Ui, st: &mut TexState) {
    let Some(set) = st.chosen_set() else {
        crate::theme::problem(ui, "Choose the templates first.");
        return;
    };
    let id = set.id.clone();
    crate::theme::paragraph(ui, "These choices are used for every PDF you make with these templates. You can run this setup again from Settings → LaTeX whenever you want to change them.");
    theme::section(ui, "Accent colour");
    if st.setup.accents.is_empty() {
        ui.add(egui::TextEdit::singleline(&mut st.setup.details.accent).hint_text("the name of a colour of the templates").desired_width(200.0));
    } else {
        let swatches = st.setup.accents.clone();
        let mut row = swatches.first().and_then(|s| s.code.chars().last());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(3.0, 3.0);
            for s in &swatches {
                // one row for each of the letters a to d
                if s.code.chars().last() != row {
                    row = s.code.chars().last();
                    ui.end_row();
                }
                let on = st.setup.details.accent == s.code;
                let colour = Color32::from_rgb(s.rgb[0], s.rgb[1], s.rgb[2]);
                let text = egui::RichText::new(&s.code).size(11.0).color(contrast(s.rgb));
                let stroke = if on { egui::Stroke::new(2.5, theme::palette().accent) } else { egui::Stroke::new(1.0, theme::palette().border) };
                if ui.add(egui::Button::new(text).fill(colour).stroke(stroke).min_size(egui::vec2(34.0, 24.0))).on_hover_text(format!("{} (#{:02X}{:02X}{:02X})", s.code, s.rgb[0], s.rgb[1], s.rgb[2])).clicked() {
                    st.setup.details.accent = s.code.clone();
                }
            }
        });
        crate::theme::hint(ui, format!("Chosen: {}", if st.setup.details.accent.is_empty() { "none".to_string() } else { st.setup.details.accent.clone() }));
    }
    theme::section(ui, "Language of your documents");
    ui.horizontal(|ui| {
        ui.radio_value(&mut st.setup.details.language, Language::De, "German (de)");
        ui.radio_value(&mut st.setup.details.language, Language::En, "English (en)");
    });
    theme::section(ui, "Department");
    ui.add(egui::TextEdit::singleline(&mut st.setup.details.department).hint_text("for example your department's name").desired_width(300.0));
    theme::section(ui, "Your name (optional)");
    ui.add(egui::TextEdit::singleline(&mut st.setup.details.author).hint_text("filled in as the author of your documents").desired_width(300.0));
    crate::theme::hint(ui, format!("Saved in templates/tex/{id}/settings.toml when you press Next."));
    if let Some(n) = &st.setup.message {
        notice(ui, n);
    }
}

fn test_step(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    crate::theme::paragraph(ui, "Shale now makes a small PDF with your templates and your details, to show that everything works.");
    if st.setup.job.is_some() || st.setup.run_test {
        ui.horizontal(|ui| {
            ui.spinner();
            crate::theme::paragraph(ui, "Making the test page… The first time can take a minute or two while TeX prepares its fonts.");
        });
        return;
    }
    match &st.setup.outcome {
        None => {}
        Some(Outcome::Made { placeholder, opened, warnings }) => {
            done_line(ui, "All set. Export a note with its ⋯ menu → Export to PDF… → Engine: LaTeX template. ✓");
            match opened {
                Ok(()) => crate::theme::hint(ui, "The test page was opened in your PDF viewer."),
                Err(e) => crate::theme::problem(ui, format!("The test page could not be opened: {e}")),
            };
            if *placeholder {
                crate::theme::problem(ui, "It carries a grey placeholder instead of the logo, so it is not CI-conformant. You can add the logo any time in Settings → LaTeX.");
            }
            for w in warnings {
                crate::theme::hint(ui, w);
            }
        }
        Some(Outcome::Failed(f)) => {
            crate::theme::problem(ui, "The test page did not work.");
            crate::theme::problem(ui, &f.headline);
            for p in f.problems.iter().take(6) {
                ui.label(egui::RichText::new(p).monospace().size(12.0));
            }
            if !f.log_tail.is_empty() {
                egui::CollapsingHeader::new("The end of the log").id_salt("setup-log").show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(&f.log_tail).monospace().size(11.0)).wrap());
                });
            }
            if !f.document.tex.is_empty() && ui.button("Save the .tex").on_hover_text("Write main.tex and its files into a folder of the vault's exports folder, to debug it with your own TeX").clicked() {
                acts.push(Act::SaveTex);
            }
        }
    }
    if let Some(dir) = &st.setup.saved_tex {
        ui.label(format!("Saved main.tex and its files in {}", dir.display()));
    }
    if let Some(n) = &st.setup.message {
        notice(ui, n);
    }
    if st.setup.outcome.is_some() && ui.button("Run again").clicked() {
        acts.push(Act::RunTest);
    }
}

/// The footer: Back, Next (Later on the logo step while there is none, Finish on the last) and Cancel.
pub fn footer(ui: &mut Ui, st: &TexState, cfg: &TexConfig, acts: &mut Vec<Act>) {
    ui.horizontal(|ui| {
        let step = st.setup.step;
        let last = step == Step::Test;
        let busy = st.task.is_some() || st.setup.job.is_some();
        if ui.add_enabled(step != Step::Tex && !busy, egui::Button::new("Back")).clicked() {
            acts.push(Act::Back);
        }
        if last {
            if ui.add_enabled(!busy, egui::Button::new(egui::RichText::new("Finish").strong())).clicked() {
                acts.push(Act::Finish);
            }
        } else {
            let later = step == Step::Logo && !st.step_done(cfg, Step::Logo);
            let label = if later { "Later" } else { "Next" };
            if ui.add_enabled(st.setup_can_next(cfg), egui::Button::new(egui::RichText::new(label).strong())).clicked() {
                acts.push(Act::Next);
            }
        }
        if ui.button("Cancel").clicked() {
            acts.push(Act::Cancel);
        }
    });
}

/// Carry out what the buttons asked for. Returns `Some(finished)` when the setup is over: finished
/// with the last step's button, or cancelled.
pub fn apply(st: &mut TexState, ctx: &Context, cfg: &TexConfig, acts: Vec<Act>) -> Option<bool> {
    let mut end = None;
    for a in acts {
        match a {
            Act::Back => st.setup_back(),
            Act::Next => st.setup_next(cfg),
            Act::Cancel => end = Some(false),
            Act::Finish => end = Some(true),
            Act::Install => st.begin_install(ctx, cfg),
            Act::Import => st.setup_import(ctx, cfg),
            Act::UseLogo => {
                let p = expand_home(&st.setup.logo_path);
                st.setup_logo(Slot::University, &p);
            }
            Act::UseDepartmentLogo => {
                let p = expand_home(&st.setup.department_logo_path);
                st.setup_logo(Slot::Department, &p);
            }
            Act::OpenLogoPage => {
                if let Err(e) = crate::opener::open_url(LOGO_PAGE) {
                    st.setup.message = Some(Notice::new(e, false));
                }
            }
            Act::RunTest => st.setup_go(Step::Test),
            Act::SaveTex => save_failed_tex(st),
        }
    }
    end
}

/// "Save the .tex" of a failed test page: into `exports/` of the vault, next to where PDFs go.
fn save_failed_tex(st: &mut TexState) {
    let (Some(vault), Some(Outcome::Failed(f))) = (&st.sets.vault, &st.setup.outcome) else { return };
    let target = Path::new(&vault.root).join("exports").join("shale-test-page.pdf");
    match save_tex(f, &target) {
        Ok(dir) => st.setup.saved_tex = Some(dir),
        Err(e) => st.setup.message = Some(Notice::new(e, false)),
    }
}
