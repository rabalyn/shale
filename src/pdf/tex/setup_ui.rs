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
use super::settings_ui::{consent_text, Notice, TaskKind, TexState};
use super::setup::{Choice, Outcome, Step, TexFact};
use crate::app::muted;
use crate::theme;
use eframe::egui::{self, Color32, Context, Ui};
use std::path::Path;

/// The green of a step that is done.
pub const GREEN: Color32 = Color32::from_rgb(110, 200, 130);

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
    CancelJob,
    Import,
    UseLogo,
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
                t.strong().color(theme::TEXT_STRONG)
            } else if done {
                t.color(GREEN)
            } else {
                t.color(theme::MUTED)
            });
        }
    });
    ui.separator();
}

/// A line that says something is done.
fn done_line(ui: &mut Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text).color(GREEN));
}

/// What the last job said, when it failed or when it has something to add.
fn notice(ui: &mut Ui, n: &Notice) {
    if n.ok {
        ui.label(&n.text);
    } else {
        ui.colored_label(theme::LINK_MISSING, &n.text);
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
    ui.label("To make a PDF from a template, Shale needs a program called TeX.");
    ui.add_space(4.0);
    // the install runs: its progress and Cancel
    if let Some(t) = st.task.as_ref().filter(|t| t.kind == TaskKind::Install) {
        let p = t.handle.progress();
        ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
        if ui.button("Cancel install").on_hover_text("Stop; what was downloaded is removed").clicked() {
            acts.push(Act::CancelJob);
        }
        return;
    }
    match st.tex_fact(cfg) {
        TexFact::Looking => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for TeX on this computer…");
            });
        }
        TexFact::Found(what) => {
            done_line(ui, format!("Found: {what} ✓"));
            ui.label(muted("There is nothing to install.").size(12.0));
        }
        TexFact::Missing(why) => {
            ui.label("It is not on this computer yet. Shale can install its own copy: it lives in a folder of its own and changes nothing else on your computer.");
            ui.add_space(4.0);
            match &st.data {
                None => {
                    ui.colored_label(theme::LINK_MISSING, "This computer has no data folder to keep TeX in, so Shale cannot install it.");
                }
                Some(data) => {
                    let folder = engine::managed_root(data);
                    ui.label(muted(consent_text(cfg.repository.trim_end_matches('/'), &folder.to_string_lossy())).size(12.0));
                    let perl = st.setup.perl.get_or_insert_with(install::perl_problem).clone();
                    if let Some(p) = &perl {
                        ui.colored_label(theme::LINK_MISSING, p);
                    }
                    ui.add_space(4.0);
                    let label = format!("Install (about {DOWNLOAD_MB} MB download, {DISK_MB} MB on disk)");
                    if ui.add_enabled(perl.is_none() && st.task.is_none(), egui::Button::new(label)).on_hover_text("Download and install TeX Live into Shale's own folder").clicked() {
                        acts.push(Act::Install);
                    }
                }
            }
            ui.label(muted(format!("(Details: {why})")).size(11.0));
        }
    }
    if let Some(n) = &st.notice {
        ui.add_space(4.0);
        notice(ui, n);
    }
}

fn templates_step(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    ui.label("The templates give your PDFs the look of your university: the title page, the colours, the fonts. They are kept in your vault, so they are backed up and synced with your notes.");
    ui.add_space(4.0);
    if st.sets.vault.is_none() {
        ui.colored_label(theme::LINK_MISSING, "Open a vault first: the templates are kept in it.");
        return;
    }
    // an import runs: its progress and Cancel
    if let Some(t) = st.task.as_ref().filter(|t| t.kind == TaskKind::Import) {
        let p = t.handle.progress();
        ui.add(egui::ProgressBar::new(p.fraction()).text(p.text()).desired_width(420.0));
        if ui.button("Cancel import").on_hover_text("Stop; the vault stays as it was").clicked() {
            acts.push(Act::CancelJob);
        }
        return;
    }
    if let Some(s) = st.chosen_set() {
        done_line(ui, format!("Using “{}” ✓", s.meta.name));
        ui.add_space(4.0);
    }
    let existing: Vec<(String, String)> = st.usable_sets().iter().map(|s| (s.id.clone(), s.meta.name.clone())).collect();
    for (id, name) in existing {
        ui.radio_value(&mut st.setup.choice, Choice::Existing(id), format!("Use “{name}” (already in your vault)"));
    }
    ui.radio_value(&mut st.setup.choice, Choice::Preset, "TU Darmstadt (tested version)  — recommended");
    ui.label(muted(format!("tuda-ci 4.06 from GitHub ({PRESET_REPO} at {}), the version Shale was tested with.", PRESET_COMMIT.chars().take(7).collect::<String>())).size(12.0));
    ui.radio_value(&mut st.setup.choice, Choice::Ctan, "The newest version from CTAN");
    ui.label(muted("Whatever CTAN has today: newer, but Shale was not tested with it.").size(12.0));
    ui.radio_value(&mut st.setup.choice, Choice::File, "From a file or a folder (a .zip or .tar.gz, or a folder of templates)");
    if st.setup.choice == Choice::File {
        ui.add(egui::TextEdit::singleline(&mut st.setup.file).hint_text("~/Downloads/templates.zip").desired_width(380.0));
        ui.label(muted("You can also drag the file or folder onto this window.").size(12.0));
    }
    ui.radio_value(&mut st.setup.choice, Choice::Github, "From a GitHub repository");
    if st.setup.choice == Choice::Github {
        ui.add(egui::TextEdit::singleline(&mut st.setup.github_url).hint_text("https://github.com/owner/repository").desired_width(380.0));
        ui.add(egui::TextEdit::singleline(&mut st.setup.github_ref).hint_text("branch, tag or commit (empty: the default branch)").desired_width(380.0));
    }
    ui.add_space(4.0);
    let existing_choice = matches!(st.setup.choice, Choice::Existing(_));
    if ui.button(if existing_choice { "Use this set" } else { "Import" }).clicked() {
        acts.push(Act::Import);
    }
    if let Some(n) = &st.notice {
        ui.add_space(4.0);
        notice(ui, n);
    }
}

fn logo_step(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    let Some(set) = st.chosen_set() else {
        ui.colored_label(theme::LINK_MISSING, "Choose the templates first.");
        return;
    };
    let (status, name) = (set.logo.clone(), set.meta.name.clone());
    match &status {
        LogoStatus::NotNeeded => {
            done_line(ui, format!("“{name}” needs no logo ✓"));
            return;
        }
        LogoStatus::Present(f) => {
            done_line(ui, format!("The logo is there: {f} ✓"));
            ui.label(muted("To use another one, drop it onto this window.").size(12.0));
        }
        LogoStatus::Missing => {
            ui.label("The templates put the university's logo on the title page. The logo may not be shipped with Shale or with the templates, so you need to get it yourself.");
            ui.label("Members of the TU Darmstadt can get it from the university library's page about LaTeX at the corporate design of the university (in the university network, or through its VPN):");
            ui.horizontal(|ui| {
                ui.label(muted(LOGO_PAGE).size(11.0));
            });
            ui.horizontal(|ui| {
                if ui.button("Open that page").clicked() {
                    acts.push(Act::OpenLogoPage);
                }
                ui.label(muted(format!("Direct download in the university network: {LOGO_DOWNLOAD}")).size(11.0));
            });
            ui.add_space(4.0);
            ui.label("Then give the file to Shale: drag it onto this window, or type or paste where it is.");
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut st.setup.logo_path).hint_text("~/Downloads/tuda_logo.pdf").desired_width(320.0));
                if ui.add_enabled(!st.setup.logo_path.trim().is_empty(), egui::Button::new("Use this logo")).clicked() {
                    acts.push(Act::UseLogo);
                }
            });
            ui.label(muted("Not now? Press Later: exports then carry a grey placeholder and are not CI-conformant. You can add the logo any time in Settings → LaTeX.").size(12.0));
        }
    }
    if let Some(n) = &st.setup.message {
        ui.add_space(4.0);
        notice(ui, n);
    }
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
        ui.colored_label(theme::LINK_MISSING, "Choose the templates first.");
        return;
    };
    let id = set.id.clone();
    ui.label("These choices are used for every PDF you make with these templates. You can run this setup again from Settings → LaTeX whenever you want to change them.");
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Accent colour").color(theme::MUTED));
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
                let stroke = if on { egui::Stroke::new(2.5, theme::ACCENT) } else { egui::Stroke::new(1.0, theme::BORDER) };
                if ui.add(egui::Button::new(text).fill(colour).stroke(stroke).min_size(egui::vec2(34.0, 24.0))).on_hover_text(format!("{} (#{:02X}{:02X}{:02X})", s.code, s.rgb[0], s.rgb[1], s.rgb[2])).clicked() {
                    st.setup.details.accent = s.code.clone();
                }
            }
        });
        ui.label(muted(format!("Chosen: {}", if st.setup.details.accent.is_empty() { "none".to_string() } else { st.setup.details.accent.clone() })).size(12.0));
    }
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Language of your documents").color(theme::MUTED));
    ui.horizontal(|ui| {
        ui.radio_value(&mut st.setup.details.language, Language::De, "German (de)");
        ui.radio_value(&mut st.setup.details.language, Language::En, "English (en)");
    });
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Department").color(theme::MUTED));
    ui.add(egui::TextEdit::singleline(&mut st.setup.details.department).hint_text("for example your department's name").desired_width(300.0));
    ui.label(egui::RichText::new("Your name (optional)").color(theme::MUTED));
    ui.add(egui::TextEdit::singleline(&mut st.setup.details.author).hint_text("filled in as the author of your documents").desired_width(300.0));
    ui.label(muted(format!("Saved in templates/tex/{id}/settings.toml when you press Next.")).size(12.0));
    if let Some(n) = &st.setup.message {
        ui.add_space(4.0);
        notice(ui, n);
    }
}

fn test_step(ui: &mut Ui, st: &mut TexState, acts: &mut Vec<Act>) {
    ui.label("Shale now makes a small PDF with your templates and your details, to show that everything works.");
    ui.add_space(4.0);
    if st.setup.job.is_some() || st.setup.run_test {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label("Making the test page… The first time can take a minute or two while TeX prepares its fonts.");
        });
        return;
    }
    match &st.setup.outcome {
        None => {}
        Some(Outcome::Made { placeholder, opened, warnings }) => {
            done_line(ui, "All set. Export a note with File → Export to PDF → LaTeX template. ✓");
            match opened {
                Ok(()) => ui.label(muted("The test page was opened in your PDF viewer.").size(12.0)),
                Err(e) => ui.colored_label(theme::LINK_MISSING, format!("The test page could not be opened: {e}")),
            };
            if *placeholder {
                ui.colored_label(theme::LINK_MISSING, "It carries a grey placeholder instead of the logo, so it is not CI-conformant. You can add the logo any time in Settings → LaTeX.");
            }
            for w in warnings {
                ui.label(muted(w).size(12.0));
            }
        }
        Some(Outcome::Failed(f)) => {
            ui.colored_label(theme::LINK_MISSING, "The test page did not work.");
            ui.colored_label(theme::LINK_MISSING, &f.headline);
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
    ui.add_space(4.0);
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
            Act::CancelJob => {
                if let Some(t) = &st.task {
                    t.handle.cancel();
                }
            }
            Act::Import => st.setup_import(ctx, cfg),
            Act::UseLogo => {
                let p = expand_home(&st.setup.logo_path);
                st.setup_logo(&p);
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
