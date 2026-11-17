//! The guided setup of the LaTeX export (TEX-017) driven through the whole window, headless, with
//! real pointer events: every step in each of its states, Back and Next, skipping what is done, the
//! way in (by itself from the export dialog, from its hints, from Settings), the files dropped on
//! it, and the way back. The TeX is a fake (a detection written into the state, a script for
//! LuaLaTeX); the jobs, the import, the files and the writer are the real ones.

use super::engine::Kind;
use super::import::TUDA;
use super::install::{Handle, Progress, Stage};
use super::sets::load_settings;
use super::sets_tests::{make_set, META};
use super::sets_ui::Form;
use super::settings_ui::{Notice, Task, TaskKind};
use super::settings_ui_tests::{found, good, nothing_found, T};
use super::setup::*;
use crate::app::{Modal, SettingsTab};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn usable() -> super::engine::Detection {
    found(Ok(good(Kind::System, &[])), Err("not installed"), None)
}

/// The tuda set (its kinds) in the vault, with this `set.toml`.
fn tuda(t: &T, meta: &str) -> PathBuf {
    let kinds: Vec<(&str, &str)> = TUDA.kinds.iter().map(|(id, text)| (*id, *text)).collect();
    make_set(&t.dir, "tuda", meta, &kinds)
}

fn open(t: &mut T, at: Option<Step>) {
    t.app.open_tex_setup(at, Return::Nothing);
    t.hl.frames(&mut t.app, 4);
}

fn step(t: &T) -> Step {
    t.app.tex.setup.step
}

fn is_open(t: &T) -> bool {
    matches!(t.app.modal, Some(Modal::TexSetup))
}

/// Frames until the test page has an outcome.
fn until_outcome(t: &mut T) {
    for _ in 0..600 {
        t.hl.frames(&mut t.app, 1);
        if t.app.tex.setup.outcome.is_some() {
            t.hl.frames(&mut t.app, 3);
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the test page did not end");
}

/// A folder of templates that is recognised as the tuda set (the class file is there already).
fn tuda_source(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("shale-setup-src-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, text) in [
        ("tex/tudapub.cls", "\\ProvidesExplClass{tudapub}{2026-06-08}{4.06}{Publications}"),
        ("tex/tudacolors.def", "\\definecolor{TUDa-9c}{cmyk/RGB/HTML}{.3,1,.9,0/185,15,34/B90F22}\n\\definecolor{TUDa-1a}{cmyk/RGB/HTML}{.7,.4,0,0/93,133,195/5D85C3}\n"),
    ] {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    dir
}

fn a_logo(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, super::sets::placeholder_logo_pdf()).unwrap();
    p
}

// spec: TEX-017
#[test]
fn the_tex_step_says_what_it_found_and_next_passes_on() {
    let mut t = T::new("su-found", usable());
    open(&mut t, Some(Step::Tex));
    assert!(is_open(&t));
    for want in ["Set up LaTeX export", "Templates", "Logo", "Your details", "Test page", "Found: the TeX of this computer (LaTeX kernel 2025-11-01) ✓", "There is nothing to install."] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    assert!(!t.shows("Install (about") && t.app.tex.task.is_none(), "nothing to install when a TeX is found");
    t.click("Back");
    assert_eq!(step(&t), Step::Tex, "there is no step before the first");
    t.click("Next");
    assert_eq!(step(&t), Step::Templates);
    t.click("Back");
    assert_eq!(step(&t), Step::Tex);
}

// spec: TEX-017
#[test]
fn a_missing_tex_is_explained_in_plain_words_and_installing_shows_progress_and_cancel() {
    let mut t = T::new("su-missing", nothing_found());
    t.app.tex.setup.perl = Some(None);
    open(&mut t, None);
    assert_eq!(step(&t), Step::Tex, "the setup starts at the first step that is not done");
    t.app.tex.setup.perl = Some(None);
    for want in [
        "Shale needs a program called TeX",
        "It is not on this computer yet. Shale can install its own copy",
        "Nothing is downloaded before you agree",
        "Install (about 100 MB download, 330 MB on disk)",
        "(Details: the managed TeX Live: not installed; the TeX of this computer: no lualatex in PATH)",
    ] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    assert!(!t.shows("Found:") && t.app.tex.task.is_none(), "nothing is downloaded before the person presses Install");
    t.click("Next");
    assert_eq!(step(&t), Step::Tex, "Next waits until there is a TeX");
    // an install that fails says why in the step and the step is still not done
    t.app.cfg.tex.repository = "not an address".into();
    t.click("Install (about 100 MB download, 330 MB on disk)");
    t.wait_for_job();
    let n = t.app.tex.notice.clone().expect("a notice");
    assert!(!n.ok && n.text.contains("The install failed") && t.shows("is not a repository address") && step(&t) == Step::Tex, "{n:?}");
    // a running install: its progress and Cancel
    let handle = Handle::default();
    handle.set(|p| *p = Progress { stage: Stage::Download, ..Progress::default() });
    let (_tx, rx) = std::sync::mpsc::channel();
    t.app.tex.task = Some(Task { kind: TaskKind::Install, handle: handle.clone(), rx });
    assert!(t.shows("Cancel install") && !t.shows("Install (about"), "{:?}", t.painted());
    t.click("Cancel install");
    assert!(handle.cancelled());
    t.app.tex.task = None;
    // once there is a TeX, the step is done
    t.app.tex.detection = Some(usable());
    assert!(t.shows("Found: the TeX of this computer"));
    t.click("Next");
    assert_eq!(step(&t), Step::Templates);
}

// spec: TEX-017
#[test]
fn a_missing_perl_is_explained_and_the_install_waits_for_it() {
    let mut t = T::new("su-perl", nothing_found());
    t.app.tex.setup.perl = Some(Some("TeX Live is installed with perl, and there is none on this computer: install perl first (for example `sudo apt install perl`, or Xcode's command line tools on a Mac), then try again.".into()));
    open(&mut t, Some(Step::Tex));
    t.app.tex.setup.perl = Some(Some("TeX Live is installed with perl, and there is none on this computer: install perl first (for example `sudo apt install perl`, or Xcode's command line tools on a Mac), then try again.".into()));
    assert!(t.shows("there is none on this computer: install perl first"), "{:?}", t.painted());
    t.click("Install (about 100 MB download, 330 MB on disk)");
    assert!(t.app.tex.task.is_none(), "the button is off while perl is missing");
}

// spec: TEX-017
#[test]
fn a_tex_that_is_still_being_looked_for_is_said_and_a_computer_without_a_data_folder_cannot_install() {
    let mut t = T::new("su-looking", usable());
    // (a job of the page keeps the setup from looking again by itself, so the state stays)
    let (_tx, rx) = std::sync::mpsc::channel();
    t.app.tex.task = Some(Task { kind: TaskKind::Test, handle: Handle::default(), rx });
    t.app.tex.detection = None;
    open(&mut t, Some(Step::Tex));
    assert!(t.shows("Looking for TeX on this computer…"), "{:?}", t.painted());
    let mut t = T::new("su-nodata", nothing_found());
    t.app.tex.data = None;
    open(&mut t, Some(Step::Tex));
    assert!(t.shows("has no data folder to keep TeX in") && !t.shows("Install (about"), "{:?}", t.painted());
}

// spec: TEX-017
#[test]
fn the_templates_step_offers_its_choices_and_imports_with_progress() {
    let mut t = T::new("su-templates", usable());
    open(&mut t, None);
    assert_eq!(step(&t), Step::Templates, "TeX is done, so the setup starts at the templates");
    for want in [
        "The templates give your PDFs the look of your university",
        "TU Darmstadt (tested version)  — recommended",
        "tuda-ci 4.06 from GitHub",
        "The newest version from CTAN",
        "From a file or a folder",
        "From a GitHub repository",
        "Import",
    ] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    assert_eq!(t.app.tex.setup.choice, Choice::Preset, "the tested version is the recommended and chosen one");
    t.click("Next");
    assert_eq!(step(&t), Step::Templates, "Next waits for a set");
    // the file or folder choice asks for a path
    t.click("From a file or a folder (a .zip or .tar.gz, or a folder of templates)");
    assert!(t.shows("You can also drag the file or folder onto this window."), "{:?}", t.painted());
    t.click("From a GitHub repository");
    assert_eq!(t.app.tex.setup.choice, Choice::Github);
    // a path that is not there fails and says so
    t.app.tex.setup.choice = Choice::File;
    t.app.tex.setup.file = t.dir.join("nothing-here.zip").to_string_lossy().into_owned();
    t.click("Import");
    t.wait_for_job();
    assert!(t.shows("The import failed, the vault is as it was") && step(&t) == Step::Templates && t.app.tex.chosen_set().is_none(), "{:?}", t.painted());
    // a folder of templates is imported as a job and the new set is chosen
    let src = tuda_source("templates");
    t.app.tex.setup.file = src.to_string_lossy().into_owned();
    t.click("Import");
    assert!(t.app.tex.task.as_ref().is_some_and(|k| k.kind == TaskKind::Import) || t.app.tex.setup.importing || t.app.tex.chosen_set().is_some());
    t.wait_for_job();
    t.hl.frames(&mut t.app, 3);
    assert_eq!(t.app.tex.chosen_set().map(|s| s.id.as_str()), Some("tuda"), "{:?}", t.painted());
    assert!(t.shows("Using “TU Darmstadt (tuda-ci)” ✓") && t.dir.join("templates/tex/tuda/kinds/pub.toml").is_file(), "{:?}", t.painted());
    t.click("Next");
    assert_eq!(step(&t), Step::Logo);
    let _ = std::fs::remove_dir_all(src);
}

// spec: TEX-017
#[test]
fn a_running_import_shows_progress_and_cancel() {
    let mut t = T::new("su-importing", usable());
    open(&mut t, Some(Step::Templates));
    let handle = Handle::default();
    handle.set(|p| *p = Progress { stage: Stage::Generate, ..Progress::default() });
    let (_tx, rx) = std::sync::mpsc::channel();
    t.app.tex.task = Some(Task { kind: TaskKind::Import, handle: handle.clone(), rx });
    assert!(t.shows("Generating the class files with docstrip") && t.shows("Cancel import") && !t.shows("recommended"), "{:?}", t.painted());
    t.click("Cancel import");
    assert!(handle.cancelled());
    t.app.tex.task = None;
}

// spec: TEX-017
#[test]
fn a_set_that_is_in_the_vault_is_offered_and_a_done_step_shows_green_and_is_passed_by_next() {
    let mut t = T::new("su-existing", usable());
    tuda(&t, META);
    open(&mut t, None);
    assert_eq!(step(&t), Step::Logo, "TeX and templates are done: the setup starts at the first step that is not");
    t.click("Back");
    assert!(t.shows("Using “Test set” ✓") && t.shows("Use “Test set” (already in your vault)") && t.shows("Use this set"), "{:?}", t.painted());
    assert_eq!(t.app.tex.setup.choice, Choice::Existing("tuda".into()));
    assert!(t.app.tex.step_done(&t.app.cfg.tex, Step::Tex) && t.app.tex.step_done(&t.app.cfg.tex, Step::Templates) && !t.app.tex.step_done(&t.app.cfg.tex, Step::Logo));
    t.click("Use this set");
    assert_eq!(t.app.tex.setup.set.as_deref(), Some("tuda"));
    t.click("Back");
    assert_eq!(step(&t), Step::Tex);
    t.click("Next");
    t.click("Next");
    assert_eq!(step(&t), Step::Logo);
    // with everything done, the setup starts at the details
    let mut t = T::new("su-alldone", usable());
    let dir = tuda(&t, META);
    std::fs::create_dir_all(dir.join("logo")).unwrap();
    a_logo(&dir.join("logo"), "mylogo.pdf");
    open(&mut t, None);
    assert_eq!(step(&t), Step::Details);
}

// spec: TEX-017, TEX-007
#[test]
fn the_logo_step_explains_where_it_comes_from_takes_a_path_and_can_wait() {
    let mut t = T::new("su-logo", usable());
    let dir = tuda(&t, META);
    open(&mut t, None);
    assert_eq!(step(&t), Step::Logo);
    for want in [
        "may not be shipped with Shale or with the templates",
        "the university library's page about LaTeX",
        "https://www.ulb.tu-darmstadt.de/finden_nutzen/medien_nutzen/dokumente_erstellen/latex/index.de.jsp",
        "drag it onto this window",
        "Use this logo",
        "exports then carry a grey placeholder and are not CI-conformant",
    ] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    assert!(t.shows("Later") && !t.shows("Next"), "the skip is called Later: {:?}", t.painted());
    // a file that is no logo is refused and the slot stays empty
    let text = t.dir.join("note.txt");
    std::fs::write(&text, "no logo").unwrap();
    t.app.tex.setup.logo_path = text.to_string_lossy().into_owned();
    t.click("Use this logo");
    assert!(t.shows("The logo was not copied: the logo must be a PDF or a PNG file") && !dir.join("logo/mylogo.pdf").exists(), "{:?}", t.painted());
    // a PDF is copied into the set, and the step is done
    let pdf = a_logo(&t.dir, "any.pdf");
    t.app.tex.setup.logo_path = pdf.to_string_lossy().into_owned();
    t.click("Use this logo");
    assert!(t.shows("The logo is there: mylogo.pdf ✓") && t.shows("Next") && !t.shows("Later"), "{:?}", t.painted());
    assert_eq!(load_settings(&dir).unwrap().logo.as_deref(), Some("mylogo.pdf"));
    assert!(dir.join("logo/mylogo.pdf").is_file());
    t.click("Next");
    assert_eq!(step(&t), Step::Details);
    // a set that needs no logo has nothing to ask
    let mut t = T::new("su-nologo", usable());
    make_set(&t.dir, "plain", &META.replace("logo_file = \"mylogo\"\n", ""), &[("article", "name = \"A\"\nmapping = \"article\"\nclass = \"my\"\n")]);
    open(&mut t, Some(Step::Logo));
    assert!(t.shows("“Test set” needs no logo ✓") && !t.shows("Use this logo"), "{:?}", t.painted());
    // Later passes the step without a logo
    let mut t = T::new("su-later", usable());
    let dir = tuda(&t, META);
    open(&mut t, Some(Step::Logo));
    t.click("Later");
    assert_eq!(step(&t), Step::Details);
    assert!(!dir.join("logo").exists());
}

#[cfg(unix)]
mod fake {
    use super::super::sets::Language;
    use super::*;
    use crate::pdf::tex::compile_tests::fake::Fake;

    // spec: TEX-017, TEX-007
    #[test]
    fn the_page_with_the_logo_can_be_opened_from_the_step() {
        let opener = crate::opener::FakeXdgOpen::install();
        let mut t = T::new("su-logo-page", usable());
        tuda(&t, META);
        open(&mut t, Some(Step::Logo));
        t.click("Open that page");
        let calls = opener.calls(1);
        assert!(calls[0].iter().any(|a| a == crate::pdf::tex::setup_ui::LOGO_PAGE), "{calls:?}");
    }

    // spec: TEX-017
    #[test]
    fn the_details_are_saved_when_leaving_the_step_and_the_test_page_uses_them() {
        let opener = crate::opener::FakeXdgOpen::install();
        let f = Fake::new("su-details");
        f.lualatex();
        f.put("log.last", "ok\n");
        let mut t = T::new("su-details", found(Ok(f.engine()), Err("not installed"), None));
        let dir = tuda(&t, META);
        std::fs::write(dir.join("tex/tudacolors.def"), "\\definecolor{TUDa-9c}{cmyk/RGB/HTML}{.3,1,.9,0/185,15,34/B90F22}\n\\definecolor{TUDa-1a}{cmyk/RGB/HTML}{.7,.4,0,0/93,133,195/5D85C3}\n").unwrap();
        std::fs::create_dir_all(dir.join("logo")).unwrap();
        a_logo(&dir.join("logo"), "mylogo.pdf");
        std::fs::write(dir.join("settings.toml"), "accent = \"9c\"\nlogo = \"mylogo.pdf\"\n").unwrap();
        open(&mut t, None);
        assert_eq!(step(&t), Step::Details);
        for want in ["Accent colour", "9c", "1a", "Chosen: 9c", "Language of your documents", "German (de)", "English (en)", "Department", "Your name (optional)", "Saved in templates/tex/tuda/settings.toml when you press Next."] {
            assert!(t.shows(want), "{want:?} in {:?}", t.painted());
        }
        t.click("1a");
        assert_eq!(t.app.tex.setup.details.accent, "1a");
        t.click("German (de)");
        t.app.tex.setup.details.department = "Informatik".into();
        t.app.tex.setup.details.author = "Ada Lovelace".into();
        assert!(load_settings(&dir).unwrap().department.is_empty(), "nothing is written before Next");
        t.click("Next");
        let s = load_settings(&dir).unwrap();
        assert_eq!((s.accent.as_str(), s.language, s.department.as_str(), s.author.as_str(), s.logo.as_deref()), ("1a", Language::De, "Informatik", "Ada Lovelace", Some("mylogo.pdf")));
        // the test page is made with them: a thesis, since a department was given
        assert_eq!(step(&t), Step::Test);
        until_outcome(&mut t);
        assert!(t.shows("All set. Export a note with File → Export to PDF → LaTeX template. ✓") && t.shows("The test page was opened in your PDF viewer."), "{:?}", t.painted());
        assert!(!t.shows("placeholder"), "the logo is there");
        assert!(f.get("args").contains("main.tex"), "{}", f.get("args"));
        let calls = opener.calls(1);
        assert!(calls[0].iter().any(|a| a.ends_with("shale-test-page.pdf")), "{calls:?}");
        // going back to the details shows what was saved
        t.click("Back");
        assert_eq!((step(&t), t.app.tex.setup.details.accent.as_str(), t.app.tex.setup.details.department.as_str()), (Step::Details, "1a", "Informatik"));
        // Back saves too
        t.app.tex.setup.details.department = "Maschinenbau".into();
        t.click("Back");
        assert_eq!(load_settings(&dir).unwrap().department, "Maschinenbau");
        // a colour that could end a command is refused, and the step stays
        t.click("Next");
        t.app.tex.setup.details.accent = "9c}".into();
        t.click("Next");
        assert!(t.shows("Your details were not saved: ") && t.shows("may have only letters, digits") && step(&t) == Step::Details, "{:?}", t.painted());
        assert_eq!(load_settings(&dir).unwrap().accent, "1a");
    }

    // spec: TEX-017
    #[test]
    fn a_failed_test_page_shows_the_readable_error_and_save_the_tex_and_run_again_works() {
        let f = Fake::new("su-failed");
        f.lualatex();
        f.put("log.last", "! Undefined control sequence.\nl.21 \\foo\n");
        f.put("exit", "1");
        let mut t = T::new("su-failed", found(Ok(f.engine()), Err("not installed"), None));
        let dir = tuda(&t, META);
        std::fs::write(dir.join("settings.toml"), "accent = \"3b\"\n").unwrap();
        open(&mut t, Some(Step::Details));
        t.click("Next");
        until_outcome(&mut t);
        for want in ["The test page did not work.", "Save the .tex", "Run again", "The end of the log"] {
            assert!(t.shows(want), "{want:?} in {:?}", t.painted());
        }
        assert!(!t.shows("All set."), "{:?}", t.painted());
        t.click("Save the .tex");
        let saved = t.dir.join("exports/shale-test-page-tex");
        assert!(t.shows("Saved main.tex and its files in") && saved.join("README.txt").is_file(), "{:?}", t.painted());
        let tex = std::fs::read_to_string(saved.join("main.tex")).unwrap();
        assert!(tex.contains("accentcolor=3b") && tex.contains("\\documentclass["), "the document of the test page: {tex}");
        // fixed: Run again works and says the placeholder logo is not conformant
        std::fs::remove_file(f.state().join("exit")).unwrap();
        f.put("log.last", "ok\n");
        t.click("Run again");
        until_outcome(&mut t);
        assert!(t.shows("All set. Export a note with File → Export to PDF → LaTeX template. ✓") && t.shows("grey placeholder instead of the logo, so it is not CI-conformant"), "{:?}", t.painted());
        // Finish ends the setup; Back is off while nothing runs? it is on
        t.click("Finish");
        assert!(!is_open(&t), "Finish closes the setup");
    }
}

// spec: TEX-017
#[test]
fn the_test_page_without_a_usable_tex_says_so_and_a_swatchless_set_takes_the_colour_typed() {
    let mut t = T::new("su-notex", nothing_found());
    let dir = tuda(&t, META);
    open(&mut t, Some(Step::Details));
    assert!(t.shows("the name of a colour of the templates") || t.app.tex.setup.accents.is_empty(), "no tudacolors.def: no swatches");
    t.app.tex.setup.details.accent = "9c".into();
    t.click("Next");
    until_outcome(&mut t);
    assert!(t.shows("No TeX can be used") && t.shows("The test page did not work.") && !t.shows("Save the .tex"), "{:?}", t.painted());
    assert_eq!(load_settings(&dir).unwrap().accent, "9c");
}

// spec: TEX-017
#[test]
fn cancel_closes_the_setup_and_stops_what_it_started() {
    let mut t = T::new("su-cancel", usable());
    open(&mut t, Some(Step::Templates));
    let handle = Handle::default();
    let (_tx, rx) = std::sync::mpsc::channel();
    t.app.tex.task = Some(Task { kind: TaskKind::Import, handle: handle.clone(), rx });
    t.hl.frames(&mut t.app, 4);
    t.click("Cancel");
    assert!(!is_open(&t), "closed");
    assert!(handle.cancelled(), "a running job is asked to stop");
    t.app.tex.task = None;
}

fn note(t: &mut T, name: &str, text: &str) {
    std::fs::write(t.dir.join(name), text).unwrap();
    t.app.vault.as_mut().unwrap().rescan();
}

// spec: TEX-017, TEX-016
#[test]
fn choosing_the_latex_template_opens_the_setup_by_itself_once_when_something_is_missing() {
    let mut t = T::new("su-auto", nothing_found());
    t.app.vdata.state.latex_export.setup_seen = false;
    tuda(&t, META);
    note(&mut t, "Plan.md", "# Plan\n");
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 4);
    assert!(matches!(t.app.modal, Some(Modal::Export(_))), "the built-in engine is the default: nothing opens");
    t.hl.frames(&mut t.app, 2);
    let r = t.hl.find_label(&mut t.app, "LaTeX template");
    t.hl.click(&mut t.app, r.center(), eframe::egui::PointerButton::Primary);
    t.hl.frames(&mut t.app, 4);
    assert!(is_open(&t), "no TeX: the setup opens");
    assert_eq!((step(&t), t.app.tex.setup.ret.clone()), (Step::Tex, Return::Export(PathBuf::from("Plan.md"))));
    assert!(t.app.vdata.state.latex_export.setup_seen, "it is remembered for the vault");
    // cancelling brings the export dialog back on the LaTeX engine, and the setup does not open again
    t.click("Cancel");
    assert!(!is_open(&t));
    t.hl.frames(&mut t.app, 6);
    match &t.app.modal {
        Some(Modal::Export(d)) => assert!(d.latex() && d.tex.as_ref().unwrap().current_set().unwrap().id == "tuda"),
        _ => panic!("the export dialog is back"),
    }
    // another dialog later: still no setup
    t.app.modal = None;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 6);
    assert!(matches!(t.app.modal, Some(Modal::Export(_))), "once is enough");
}

// spec: TEX-017, TEX-016
#[test]
fn nothing_opens_when_nothing_is_missing_and_a_vault_without_sets_starts_at_the_templates() {
    let mut t = T::new("su-auto-none", usable());
    t.app.vdata.state.latex_export.setup_seen = false;
    tuda(&t, META);
    note(&mut t, "Plan.md", "# Plan\n");
    t.app.vdata.state.latex_export.latex = true;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 6);
    assert!(matches!(t.app.modal, Some(Modal::Export(_))) && !t.app.vdata.state.latex_export.setup_seen, "TeX and a set are there: no setup, nothing remembered");
    // no set: the templates
    let mut t = T::new("su-auto-nosets", usable());
    t.app.vdata.state.latex_export.setup_seen = false;
    note(&mut t, "Plan.md", "# Plan\n");
    t.app.vdata.state.latex_export.latex = true;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 6);
    assert!(is_open(&t) && step(&t) == Step::Templates, "{:?}", t.app.tex.setup.step);
}

// spec: TEX-017, TEX-016
#[test]
fn the_hints_of_the_export_dialog_open_the_setup_at_the_right_step_and_finishing_returns_with_the_set_chosen() {
    // no TeX
    let mut t = T::new("su-hint-tex", nothing_found());
    tuda(&t, META);
    note(&mut t, "Plan.md", "# Plan\n");
    t.app.vdata.state.latex_export.latex = true;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 4);
    click_key(&mut t, "setup:tex");
    assert!(is_open(&t) && step(&t) == Step::Tex);
    // no set
    let mut t = T::new("su-hint-sets", usable());
    note(&mut t, "Plan.md", "# Plan\n");
    t.app.vdata.state.latex_export.latex = true;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 4);
    click_key(&mut t, "setup:sets");
    assert!(is_open(&t) && step(&t) == Step::Templates);
    // no logo: the setup starts at the logo, and finishing returns with the set and the publication chosen
    let mut t = T::new("su-hint-logo", nothing_found());
    tuda(&t, META);
    note(&mut t, "Plan.md", "# Plan\n");
    t.app.tex.detection = Some(usable());
    t.app.vdata.state.latex_export.latex = true;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 4);
    click_key(&mut t, "setup:logo");
    assert!(is_open(&t) && step(&t) == Step::Logo);
    t.click("Later");
    t.click("Next");
    until_outcome(&mut t);
    t.click("Finish");
    t.hl.frames(&mut t.app, 4);
    match &t.app.modal {
        Some(Modal::Export(d)) => {
            let tex = d.tex.as_ref().unwrap();
            assert!(d.latex() && tex.current_set().unwrap().id == "tuda" && tex.kind == "pub", "the set and the kind are chosen");
        }
        _ => panic!("the export dialog is back"),
    }
}

fn click_key(t: &mut T, key: &str) {
    let rect = match &t.app.modal {
        Some(Modal::Export(d)) => d.rects.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("no control {key:?}: {:?}", d.rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1,
        _ => panic!("the export dialog is not open"),
    };
    t.hl.click(&mut t.app, rect.center(), eframe::egui::PointerButton::Primary);
    t.hl.frames(&mut t.app, 3);
}

// spec: TEX-017
#[test]
fn settings_has_a_button_that_opens_the_setup_and_cancel_goes_back_to_settings() {
    let mut t = T::new("su-settings", usable());
    assert!(t.shows("Set up LaTeX export…") && t.shows("Not firm in TeX? This takes you through it."), "{:?}", t.painted());
    t.click("Set up LaTeX export…");
    assert!(is_open(&t) && t.app.tex.setup.ret == Return::Settings);
    t.click("Cancel");
    assert!(matches!(&t.app.modal, Some(Modal::Settings { tab: SettingsTab::Latex, .. })), "back on the LaTeX tab");
}

// spec: TEX-018, TEX-017
#[test]
fn the_author_of_the_settings_is_in_the_author_field_of_the_export_dialog_unless_the_note_names_one() {
    let mut t = T::new("su-author", usable());
    let dir = tuda(&t, META);
    std::fs::write(dir.join("settings.toml"), "accent = \"9c\"\nauthor = \"Ada Lovelace\"\ndepartment = \"Informatik\"\n").unwrap();
    note(&mut t, "Plan.md", "# Plan\n");
    note(&mut t, "Own.md", "---\nauthor: Bob\n---\n# Own\n");
    for (name, author) in [("Plan.md", "Ada Lovelace"), ("Own.md", "Bob")] {
        t.app.vdata.state.latex_export.latex = true;
        t.app.open_export_dialog(Path::new(name));
        t.hl.frames(&mut t.app, 4);
        let Some(Modal::Export(d)) = &mut t.app.modal else { panic!("the export dialog") };
        let tex = d.tex.as_mut().unwrap();
        tex.select_kind("thesis");
        assert_eq!((tex.values["author"].as_str(), tex.values["department"].as_str()), (author, "Informatik"), "{name}");
        t.app.modal = None;
    }
}

// spec: TEX-017
#[test]
fn a_logo_dropped_on_the_logo_step_goes_into_the_set_and_not_into_the_vault() {
    let mut t = T::new("su-drop-logo", usable());
    let dir = tuda(&t, META);
    open(&mut t, Some(Step::Logo));
    let outside = std::env::temp_dir().join(format!("shale-su-drop-{}", std::process::id()));
    std::fs::create_dir_all(&outside).unwrap();
    let logo = a_logo(&outside, "tuda_logo.pdf");
    // dropped through the window system
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(logo.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(is_open(&t), "no Import dialog: the setup keeps the drop");
    assert!(dir.join("logo/mylogo.pdf").is_file() && load_settings(&dir).unwrap().logo.as_deref() == Some("mylogo.pdf"), "the logo is in the set and in settings.toml");
    assert!(t.shows("The logo is there: mylogo.pdf ✓"), "{:?}", t.painted());
    let vault_files: Vec<String> = t.app.vault.as_ref().unwrap().files.iter().map(|f| f.rel.to_string_lossy().into_owned()).collect();
    assert!(!vault_files.iter().any(|f| f.contains("tuda_logo")) && !t.dir.join("media").join("tuda_logo.pdf").exists(), "not imported as a media file: {vault_files:?}");
    // a file that is not a logo is said and changes nothing
    let notes = outside.join("notes.md");
    std::fs::write(&notes, "# x").unwrap();
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(notes.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(is_open(&t) && t.app.toasts.iter().any(|(m, _)| m.contains("Drop the logo here as a PDF or a PNG file.")), "{:?}", t.app.toasts.iter().map(|x| &x.0).collect::<Vec<_>>());
    // a drop from another program (Wayland) takes the same way
    #[cfg(target_os = "linux")]
    {
        std::fs::remove_file(dir.join("logo/mylogo.pdf")).unwrap();
        let other = a_logo(&outside, "other.png.pdf");
        let list = format!("{}\r\n", crate::import::file_uri(&other));
        t.app.drag.fake_drops.push(crate::import::wl_drop::DropEvent::Drop { x: 10.0, y: 10.0, parts: vec![("text/uri-list".into(), list.into_bytes())], offered: vec!["text/uri-list".into()], report: vec![] });
        t.hl.frames(&mut t.app, 4);
        assert!(is_open(&t) && dir.join("logo/mylogo.pdf").is_file(), "the logo of the drop from another program is in the set");
    }
    let _ = std::fs::remove_dir_all(outside);
}

// spec: TEX-017
#[test]
fn a_folder_dropped_on_the_templates_step_is_imported_as_a_set() {
    let mut t = T::new("su-drop-set", usable());
    open(&mut t, Some(Step::Templates));
    let src = tuda_source("drop");
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(src.clone()))));
    t.wait_for_job();
    t.hl.frames(&mut t.app, 3);
    assert!(is_open(&t) && t.app.tex.chosen_set().is_some_and(|s| s.id == "tuda"), "{:?}", t.painted());
    assert!(t.dir.join("templates/tex/tuda/tex/tudapub.cls").is_file());
    // a file that is no template is said
    let junk = src.join("junk.txt");
    std::fs::write(&junk, "x").unwrap();
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(junk.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(t.app.toasts.iter().any(|(m, _)| m.contains("Drop the templates here as a .zip or .tar.gz file or as a folder.")));
    // and nothing on a step that has nothing to take
    t.click("Back");
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(junk.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(is_open(&t) && t.app.toasts.iter().any(|(m, _)| m.contains("There is nothing to drop on this step")));
    let _ = std::fs::remove_dir_all(src);
}

// spec: TEX-017, TEX-024
#[test]
fn drops_on_settings_latex_import_an_archive_and_give_a_pdf_to_the_set_whose_logo_is_asked_for() {
    let mut t = T::new("su-drop-settings", usable());
    let src = tuda_source("settings");
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(src.clone()))));
    t.wait_for_job();
    assert!(matches!(t.app.modal, Some(Modal::Settings { .. })) && t.dir.join("templates/tex/tuda/set.toml").is_file(), "the folder was imported, no Import dialog");
    let dir = t.dir.join("templates/tex/tuda");
    // a logo goes to the only set that has a slot
    let logo = a_logo(&std::env::temp_dir(), &format!("shale-su-settings-logo-{}.pdf", std::process::id()));
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(logo.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(dir.join("logo/tuda_logo.pdf").is_file(), "the logo is in the set");
    assert!(matches!(t.app.modal, Some(Modal::Settings { .. })));
    // with two sets, the one whose "Choose logo…" is open takes it, else the person is told
    make_set(&t.dir, "other", META, &[("article", "name = \"A\"\nmapping = \"article\"\nclass = \"my\"\n")]);
    t.app.tex.sets.dirty = true;
    std::fs::remove_file(dir.join("logo/tuda_logo.pdf")).unwrap();
    t.hl.frames(&mut t.app, 3);
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(logo.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(!dir.join("logo/tuda_logo.pdf").exists() && t.app.toasts.iter().any(|(m, _)| m.contains("Press “Choose logo…” of the set the logo is for")));
    t.app.tex.sets.form = Form::Logo { set: "other".into(), path: String::new() };
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(logo.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(t.dir.join("templates/tex/other/logo/mylogo.pdf").is_file() && t.app.tex.sets.form == Form::None);
    // a file that is neither is said
    let junk = t.dir.join("junk.txt");
    std::fs::write(&junk, "x").unwrap();
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(junk.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(t.app.toasts.iter().any(|(m, _)| m.contains("Drop a .zip or .tar.gz file or a folder of templates, or a logo")));
    let _ = std::fs::remove_dir_all(src);
    let _ = std::fs::remove_file(logo);
}

// spec: TEX-017
#[test]
fn drops_still_open_the_import_dialog_when_neither_the_setup_nor_settings_latex_is_open() {
    let mut t = T::new("su-drop-normal", usable());
    t.app.open_settings(SettingsTab::Vim);
    t.hl.frames(&mut t.app, 3);
    let file = t.dir.join("dropped.md");
    std::fs::write(&file, "# x\n").unwrap();
    t.hl.frame_with(&mut t.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(crate::app::ScriptDrop(file.clone()))));
    t.hl.frames(&mut t.app, 3);
    assert!(matches!(t.app.modal, Some(Modal::Import(_))), "another tab of Settings: the drop is an import, as before");
}

// spec: TEX-017
#[test]
fn the_notice_type_is_shown_for_what_the_last_job_said() {
    let mut t = T::new("su-notice", usable());
    open(&mut t, Some(Step::Templates));
    t.app.tex.notice = Some(Notice::new("The import was cancelled; the vault is as it was.", false));
    assert!(t.shows("The import was cancelled; the vault is as it was."));
    t.app.tex.notice = Some(Notice::new("Something worked.", true));
    assert!(t.shows("Something worked."));
}
