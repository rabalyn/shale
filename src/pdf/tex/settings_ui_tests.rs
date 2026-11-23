//! Settings → LaTeX (TEX-023, CFG-044) driven through the whole window, headless: the controls are
//! there, nothing is downloaded before the person agrees, a removal asks first, and the progress of
//! a job is shown. The engine and the install are fakes (a detection written into the state, a
//! temporary data folder); the jobs themselves are the real ones.

use super::engine::*;
use super::install::{Handle, Progress, Stage};
use super::settings_ui::*;
use crate::app::{Options, SettingsTab, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::PointerButton;
use std::path::PathBuf;
use std::time::Duration;

pub(super) struct T {
    pub app: ShaleApp,
    pub hl: Headless,
    pub dir: PathBuf,
}

pub(super) fn found(system: Result<Engine, &str>, managed: Result<Engine, &str>, bytes: Option<u64>) -> Detection {
    Detection { system: system.map_err(String::from), managed: managed.map_err(String::from), managed_bytes: bytes }
}

pub(super) fn good(kind: Kind, missing: &[&str]) -> Engine {
    let mut e = Engine::new(kind, PathBuf::from(if kind == Kind::System { "/usr/bin" } else { "/data/texlive/bin/x86_64-linux" }));
    e.kernel = "2025-11-01".into();
    e.missing = missing.iter().map(|s| s.to_string()).collect();
    e
}

impl T {
    /// The window with Settings on the LaTeX tab and this detection (so no job looks at the machine).
    pub(super) fn new(tag: &str, detection: Detection) -> T {
        let dir = std::env::temp_dir().join(format!("shale-tex-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.tex.detection = Some(detection);
        // the guided setup does not open by itself unless a test asks for it (TEX-017)
        app.vdata.state.latex_export.setup_seen = true;
        app.open_settings(SettingsTab::Latex);
        let mut t = T { app, hl: Headless::new(), dir };
        // the LaTeX page with its forms is longer than the 900 points of the usual window since the
        // dialogs have more room between their lines and items (NAV-042); it would scroll
        t.hl.set_size(eframe::egui::Vec2::new(1500.0, 1400.0));
        t.hl.frames(&mut t.app, 4);
        t
    }

    /// Whether this text is painted on screen after the layout has settled.
    pub(super) fn shows(&mut self, text: &str) -> bool {
        self.hl.frames(&mut self.app, 3);
        self.hl.text.iter().any(|(t, _)| t.contains(text))
    }

    pub(super) fn painted(&self) -> Vec<&str> {
        self.hl.text.iter().map(|(t, _)| t.as_str()).collect()
    }

    pub(super) fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    /// Frames, with a rest between them, until the job of the page has ended and the page is
    /// quiet: an install, update or removal ends by looking for TeX again (a second job, which
    /// changes what the page shows and so the height of its dialog), so that has to end too,
    /// and the dialog has to settle at its new height, before what is painted can be checked
    /// (raba/shale#308).
    pub(super) fn wait_for_job(&mut self) {
        let busy = |app: &ShaleApp| app.tex.task.is_some() || app.tex.detecting.is_some();
        for _ in 0..300 {
            self.hl.frames(&mut self.app, 1);
            if !busy(&self.app) {
                // a frame of the page may start the look for TeX: then wait for that as well
                self.hl.frames(&mut self.app, 3);
                if !busy(&self.app) {
                    return;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("the job did not end");
    }

    /// The data folder the managed TeX Live would live in.
    pub(super) fn data(&self) -> PathBuf {
        self.app.tex.data.clone().unwrap()
    }
}

impl Drop for T {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
        if let Some(d) = &self.app.tex.data {
            let _ = std::fs::remove_dir_all(d);
        }
    }
}

pub(super) fn nothing_found() -> Detection {
    found(Err("no lualatex in PATH"), Err("not installed"), None)
}

// spec: TEX-023
#[test]
fn the_latex_tab_has_its_controls_and_says_why_no_tex_can_be_used() {
    let mut t = T::new("controls", nothing_found());
    for want in [
        "LaTeX",
        "Which TeX is used",
        "No TeX can be used",
        "no lualatex in PATH",
        "not installed",
        "Automatic",
        "The TeX of this computer",
        "The TeX Live that Shale installs",
        "Check again",
        "Test",
        "Shale's own TeX Live",
        "Install TeX Live…",
        "Download address",
    ] {
        assert!(t.shows(want), "{want:?} missing in {:?}", t.painted());
    }
    assert!(t.app.tex.task.is_none(), "looking at the page starts no job");
    assert!(!t.shows("Remove…") && !t.painted().contains(&"Update"), "nothing to remove or update while nothing is installed");
}

// spec: NAV-048
#[test]
fn nav_048_the_sections_of_the_latex_page_have_the_section_gap_above_their_headings() {
    let mut t = T::new("sections", nothing_found());
    t.hl.frames(&mut t.app, 4);
    let at = |t: &T, text: &str| t.hl.text.iter().find(|(s, _)| s == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("{text:?} missing in {:?}", t.painted()));
    // a text row is at least 12 points high: the heading's top is at least that and the section
    // gap below the top of the text before it
    let row = 12.0;
    for heading in ["Shale's own TeX Live", "Template sets"] {
        let h = at(&t, heading);
        let before = t.hl.text.iter().filter(|(_, p)| p.x >= h.x - 1.0 && p.y < h.y - 1.0).map(|(_, p)| p.y).fold(f32::MIN, f32::max);
        assert!(h.y - before >= crate::theme::SECTION_GAP + row, "{heading:?} is {} below the text before it, the section gap is {}", h.y - before, crate::theme::SECTION_GAP);
    }
    // its first heading follows the hint and the setup button, also a section break
    let (which, hint) = (at(&t, "Which TeX is used"), at(&t, "Set up LaTeX export…"));
    assert!(which.y - hint.y >= crate::theme::SECTION_GAP + row, "{} below the setup button", which.y - hint.y);
}

// spec: TEX-023
#[test]
fn a_usable_tex_is_shown_with_its_kind_kernel_and_folder_and_a_system_tex_with_missing_packages_is_flagged() {
    let mut t = T::new("system", found(Ok(good(Kind::System, &[])), Err("not installed"), None));
    assert!(t.shows("Using the TeX of this computer (LaTeX kernel 2025-11-01)") && t.shows("/usr/bin"), "{:?}", t.painted());
    assert!(!t.shows("It misses"));
    t.app.tex.detection = Some(found(Ok(good(Kind::System, &["roboto", "xcharter"])), Err("not installed"), None));
    t.app.cfg.tex.engine = TexChoice::System;
    assert!(t.shows("It misses these packages: roboto, xcharter. Use the TeX Live that Shale installs."), "{:?}", t.painted());
    // automatic: the incomplete system TeX is not taken, and the page says why
    t.app.cfg.tex.engine = TexChoice::Auto;
    assert!(t.shows("No TeX can be used") && t.shows("misses roboto, xcharter; use the managed one"), "{:?}", t.painted());
    // the managed one is first for Auto, and shows its disk use
    t.app.tex.detection = Some(found(Ok(good(Kind::System, &[])), Ok(good(Kind::Managed, &[])), Some(330 * 1024 * 1024)));
    assert!(t.shows("Using the TeX Live that Shale installs (LaTeX kernel 2025-11-01), 330.0 MB on disk"), "{:?}", t.painted());
}

// spec: CFG-044
#[test]
fn the_engine_choice_is_a_setting_changed_by_a_click() {
    let mut t = T::new("choice", found(Ok(good(Kind::System, &[])), Ok(good(Kind::Managed, &[])), Some(1)));
    assert_eq!(t.app.cfg.tex.engine, TexChoice::Auto);
    t.click("The TeX of this computer");
    assert_eq!(t.app.cfg.tex.engine, TexChoice::System);
    assert!(t.shows("Using the TeX of this computer"));
    t.click("The TeX Live that Shale installs");
    assert_eq!(t.app.cfg.tex.engine, TexChoice::Managed);
    t.click("Automatic");
    assert_eq!(t.app.cfg.tex.engine, TexChoice::Auto);
}

// spec: TEX-002, TEX-023
#[test]
fn install_asks_first_with_the_sizes_and_downloads_nothing_until_the_person_agrees() {
    let mut t = T::new("consent", nothing_found());
    t.click("Install TeX Live…");
    assert!(t.app.tex.consent && t.app.tex.task.is_none(), "pressing Install only asks");
    let text = t.painted().join("\n");
    assert!(text.contains("about 100 MB") && text.contains("about 330 MB on disk"), "the sizes come first: {text}");
    assert!(text.contains("https://mirror.ctan.org/systems/texlive/tlnet") && text.contains(&*managed_root(&t.data()).to_string_lossy()), "from where and into which folder: {text}");
    assert!(text.contains("Nothing is downloaded before you agree"));
    assert!(t.shows("Download and install") && t.shows("Not now"));
    // not now: back to the button, nothing happened
    t.click("Not now");
    assert!(!t.app.tex.consent && t.app.tex.task.is_none() && t.shows("Install TeX Live…"));
    assert!(!t.data().join("shale").exists(), "nothing was made on disk");
}

// spec: TEX-002, TEX-023
#[test]
fn agreeing_starts_the_install_job_and_its_failure_is_reported_with_nothing_left() {
    let mut t = T::new("agree", nothing_found());
    // an address that is no repository: the job fails at once, without touching the network
    t.app.cfg.tex.repository = "not an address".into();
    t.click("Install TeX Live…");
    t.click("Download and install");
    assert!(t.app.tex.task.as_ref().is_some_and(|k| k.kind == TaskKind::Install) || t.app.tex.notice.is_some(), "the install ran as a job");
    t.wait_for_job();
    let n = t.app.tex.notice.clone().expect("a notice");
    assert!(!n.ok && n.text.contains("The install failed") && n.text.contains("is not a repository address"), "{n:?}");
    assert!(t.shows("The install failed"), "the page says so: {:?}", t.painted());
    assert!(!super::engine::partial_root(&t.data()).exists() && !super::engine::managed_root(&t.data()).exists());
}

// spec: TEX-002, TEX-023
#[test]
fn the_progress_of_a_running_install_is_shown_and_cancel_asks_the_job_to_stop() {
    let mut t = T::new("progress", nothing_found());
    let handle = Handle::default();
    handle.set(|p| *p = Progress { stage: Stage::Packages, done: 31, total: 52, package: "koma-script".into() });
    // a job that never ends by itself, standing in for the install
    let (tx, rx) = std::sync::mpsc::channel();
    t.app.tex.inject(TaskKind::Install, handle.clone(), rx);
    let task = t.app.busy.info(&super::settings_ui::TEX_JOB).expect("the install is a task of the registry");
    assert_eq!((task.label.as_str(), task.cancellable), ("Installing TeX Live…", true));
    assert!(t.shows("Installing the packages: koma-script (31 of 52)") && t.shows("Installing TeX Live…"), "{:?}", t.painted());
    assert!(!t.shows("Install TeX Live…"), "no second install while one runs");
    handle.set(|p| *p = Progress { stage: Stage::Fonts, ..Progress::default() });
    assert!(t.shows("Building the font cache"), "{:?}", t.painted());
    assert!(!handle.cancelled());
    t.click("Cancel");
    assert!(handle.cancelled(), "Cancel stops the job");
    drop(tx);
    t.wait_for_job();
    // an update is a task with its own text and Cancel
    let handle = Handle::default();
    let (tx, rx) = std::sync::mpsc::channel();
    t.app.tex.inject(TaskKind::Update, handle.clone(), rx);
    assert!(t.shows("Updating TeX Live…") && t.shows("Cancel") && !t.shows("Installing TeX Live…"));
    // the end of the job is taken by the window and the page looks again
    tx.send(Ok(Notice::new("The update is done.", true))).unwrap();
    t.wait_for_job();
    assert!(t.shows("The update is done."), "{:?}", t.painted());
    assert!(t.app.tex.task.is_none() && t.app.busy.list().is_empty());
}

/// A tree where the managed TeX Live would be, with a file in it and one beside it.
fn installed_tree(t: &T) -> PathBuf {
    let root = managed_root(&t.data());
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin").join("tool"), "x").unwrap();
    std::fs::write(t.data().join("shale").join("keep.txt"), "mine").unwrap();
    root
}

// spec: TEX-003, TEX-023
#[test]
fn remove_asks_naming_the_folder_and_its_size_and_removes_that_folder_only_after_the_confirmation() {
    let mut t = T::new("remove", found(Err("no lualatex in PATH"), Ok(good(Kind::Managed, &[])), Some(330 * 1024 * 1024)));
    let root = installed_tree(&t);
    assert!(t.shows(&root.to_string_lossy()) && t.shows("330.0 MB") && t.shows("Update") && t.shows("Remove…"), "{:?}", t.painted());
    t.click("Remove…");
    let text = t.painted().join("\n");
    assert!(text.contains(&format!("This deletes the folder {}", root.display())) && text.contains("(330.0 MB)") && text.contains("Nothing else is removed"), "{text}");
    assert!(root.is_dir(), "asking removes nothing");
    // keep it
    t.click("Keep it");
    assert!(root.is_dir() && !t.app.tex.confirm_remove && t.app.tex.task.is_none());
    // confirm
    t.click("Remove…");
    t.click("Remove the TeX Live folder");
    t.wait_for_job();
    assert!(!root.exists(), "the folder is gone");
    assert!(t.data().join("shale").join("keep.txt").is_file(), "and nothing else");
    assert!(t.shows("The TeX Live was removed."), "{:?}", t.painted());
}

// spec: TEX-003, TEX-023
#[test]
fn update_starts_a_job_and_a_failure_is_reported() {
    let mut t = T::new("update", found(Err("no lualatex in PATH"), Ok(good(Kind::Managed, &[])), Some(1)));
    installed_tree(&t);
    t.app.cfg.tex.repository = "not an address".into();
    t.click("Update");
    t.wait_for_job();
    let n = t.app.tex.notice.clone().expect("a notice");
    assert!(!n.ok && n.text.contains("The update failed"), "{n:?}");
}

// spec: TEX-001, TEX-023
#[test]
fn check_again_looks_for_a_tex_in_a_job_and_shows_what_it_found() {
    let mut t = T::new("again", found(Ok(good(Kind::System, &[])), Ok(good(Kind::Managed, &[])), Some(1)));
    let ctx = t.hl.ctx().clone();
    t.app.tex.check_again(&ctx);
    assert!(t.app.tex.detection.is_none(), "the old answer is dropped while the job runs");
    t.wait_for_job();
    let d = t.app.tex.detection.clone().expect("the job answered");
    // a window of a test has no PATH and a data folder with nothing in it
    assert_eq!((d.system.unwrap_err().as_str(), d.managed.unwrap_err().as_str()), ("no lualatex in PATH", "not installed"));
    // the button does the same (it is off while a look runs, so only now; raba/shale#308)
    t.app.tex.detection = Some(found(Ok(good(Kind::System, &[])), Ok(good(Kind::Managed, &[])), Some(1)));
    t.click("Check again");
    t.wait_for_job();
    let d = t.app.tex.detection.clone().expect("the job answered");
    assert_eq!((d.system.unwrap_err().as_str(), d.managed.unwrap_err().as_str()), ("no lualatex in PATH", "not installed"), "the button looked again");
    assert!(t.shows("No TeX can be used"), "{:?}", t.painted());
}

// spec: TEX-001, TEX-023
#[test]
fn the_page_looks_for_a_tex_by_itself_when_it_is_shown_for_the_first_time() {
    let dir = std::env::temp_dir().join(format!("shale-tex-ui-first-{}", std::process::id()));
    crate::vault::write_sample_vault(&dir).unwrap();
    let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
    assert!(app.tex.detection.is_none());
    app.open_settings(SettingsTab::Latex);
    let mut hl = Headless::new();
    for _ in 0..300 {
        hl.frames(&mut app, 1);
        if app.tex.detection.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(app.tex.detection.is_some(), "detection ran without a click");
    let _ = std::fs::remove_dir_all(&dir);
    if let Some(d) = &app.tex.data {
        let _ = std::fs::remove_dir_all(d);
    }
}

#[cfg(unix)]
mod fake {
    use super::*;
    use crate::pdf::tex::compile_tests::fake::Fake;

    // spec: JOB-035
    #[test]
    fn job_035_the_test_compile_is_a_task_that_cannot_stop_and_its_button_is_off_while_it_runs() {
        let f = Fake::new("job035-test");
        f.lualatex();
        f.put("log.last", "ok\n");
        let gate = f.hold();
        let mut t = T::new("job035-test", found(Ok(f.engine()), Err("not installed"), None));
        t.click("Test");
        let task = t.app.busy.info(&super::super::settings_ui::TEX_JOB).expect("the test is a task");
        assert_eq!((task.label.as_str(), task.cancellable), ("Compiling the test document…", false));
        assert!(t.shows("Compiling the test document…") && !t.shows("Cancel"), "{:?}", t.painted());
        t.click("Test");
        assert_eq!(t.app.busy.list().len(), 1, "a second test is not started");
        std::fs::write(&gate, "").unwrap();
        t.wait_for_job();
        assert!(t.app.busy.list().is_empty());
        assert!(t.app.tex.notice.clone().expect("a notice").ok);
    }

    // spec: TEX-004, TEX-023
    #[test]
    fn test_compiles_the_small_document_with_the_chosen_tex_as_a_job_and_reports() {
        let f = Fake::new("ui-test");
        f.lualatex();
        f.put("log.last", "ok\n");
        let mut t = T::new("test-ok", found(Ok(f.engine()), Err("not installed"), None));
        t.click("Test");
        t.wait_for_job();
        let n = t.app.tex.notice.clone().expect("a notice");
        assert!(n.ok && n.text.starts_with("The test document compiled to a PDF of 1 KB in 1 run"), "{n:?}");
        // a failure shows the readable problems and the end of the log on request
        f.put("log.last", "! LaTeX Error: File `roboto.sty' not found.\n");
        f.put("exit", "1");
        let mut t = T::new("test-fail", found(Ok(f.engine()), Err("not installed"), None));
        t.click("Test");
        t.wait_for_job();
        let n = t.app.tex.notice.clone().expect("a notice");
        assert!(!n.ok && n.text.contains("package roboto is missing") && n.details.as_deref().is_some_and(|d| d.contains("roboto.sty")), "{n:?}");
        assert!(t.shows("package roboto is missing") && t.shows("The end of the log"), "{:?}", t.painted());
    }
}

// spec: JOB-035
#[test]
fn job_035_a_removal_is_a_task_that_cannot_stop_and_the_looks_for_tex_are_quiet_tasks() {
    let mut t = T::new("job035-remove", nothing_found());
    let (tx, rx) = std::sync::mpsc::channel();
    t.app.tex.inject(TaskKind::Remove, Handle::default(), rx);
    let task = t.app.busy.info(&super::settings_ui::TEX_JOB).expect("the removal is a task");
    assert_eq!((task.label.as_str(), task.cancellable), ("Removing the TeX Live…", false));
    assert!(t.shows("Removing the TeX Live…") && !t.shows("Cancel"), "{:?}", t.painted());
    assert!(t.app.busy.begin(super::settings_ui::TEX_JOB, "again").is_none(), "no second job beside it");
    drop(tx);
    t.wait_for_job();
    assert!(t.app.busy.list().is_empty());
    // the look for TeX: not started while its task is listed, shown by the page, quiet in the indicator
    let held = t.app.busy.begin(super::settings_ui::TEX_DETECT, "Looking for a TeX…").unwrap();
    t.app.tex.detection = None;
    let ctx = eframe::egui::Context::default();
    t.app.tex.check_again(&ctx);
    assert!(t.app.tex.detecting.is_none(), "not started while the task is listed");
    assert!(t.app.busy.list().is_empty(), "a quiet task is not in the indicator at once");
    drop(held);
    t.app.tex.check_again(&ctx);
    assert!(t.app.tex.detecting.is_some());
    t.wait_for_job();
    assert!(t.app.busy.list().is_empty() && t.app.tex.detection.is_some());
}

// spec: TEX-002, TEX-003
#[test]
fn the_end_of_an_install_or_update_says_which_mirror_it_came_from() {
    use super::install::Done;
    let done = |warning: Option<&str>, mirrors: &[&str]| Done { warning: warning.map(String::from), mirrors: mirrors.iter().map(|m| m.to_string()).collect() };
    let n = done_notice(Ok(done(None, &["https://a.example/tlnet"])), "The install");
    assert_eq!((n.text.as_str(), n.ok), ("The install is done, from the mirror https://a.example/tlnet.", true));
    let n = done_notice(Ok(done(None, &["https://a.example/tlnet", "https://b.example/tlnet"])), "The update");
    assert_eq!(n.text, "The update is done, from the mirrors https://a.example/tlnet and https://b.example/tlnet.");
    assert_eq!(done_notice(Ok(done(None, &[])), "The update").text, "The update is done.");
    // the warning already names the mirror
    let n = done_notice(Ok(done(Some("TeX Live is installed from the mirror https://a.example/tlnet, but the test document did not compile: x"), &["https://a.example/tlnet"])), "The install");
    assert!(!n.ok && n.text.contains("from the mirror https://a.example/tlnet, but the test document"), "{}", n.text);
    assert_eq!(done_notice(Err("cancelled".into()), "The install").text, "The install was cancelled; nothing of it is left.");
    assert_eq!(done_notice(Err("boom".into()), "The update").text, "The update failed: boom");
}
