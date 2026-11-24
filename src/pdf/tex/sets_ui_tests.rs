//! The template sets in Settings → LaTeX (TEX-024) driven through the whole window, headless: the
//! sets of the vault are listed, import, update, logo, test page and removal are buttons whose
//! jobs and files are the real ones (the TeX of the test page is a fake).

use super::install::{Handle, Progress, Stage};
use super::sets::{sets_root, Source};
use super::sets_tests::{make_set, meta_from_local, META, PNG};
use super::sets_ui::*;
use super::settings_ui::TaskKind;
use super::settings_ui_tests::{nothing_found, T};
use std::path::{Path, PathBuf};

const KIND: &str = "name = \"Article\"\nmapping = \"article\"\nclass = \"my\"\n";

/// The window with a set called "Test set" in the vault (`templates/tex/mine`).
fn with_set(tag: &str, kinds: &[(&str, &str)]) -> (T, PathBuf) {
    let mut t = T::new(tag, nothing_found());
    let dir = make_set(&t.dir, "mine", META, kinds);
    t.app.tex.sets.dirty = true;
    (t, dir)
}

/// A folder of a set with only a class file, to import from.
fn source_folder(parent: &Path, name: &str) -> PathBuf {
    let src = parent.join(name);
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("fancy.cls"), "\\ProvidesClass{fancy}").unwrap();
    src
}

// spec: TEX-024
#[test]
fn the_section_has_its_buttons_and_says_when_the_vault_has_no_sets() {
    let mut t = T::new("sets-empty", nothing_found());
    for want in [
        "Template sets",
        "There are no template sets in this vault yet.",
        "Import TU Darmstadt templates",
        "Import from GitHub…",
        "Import from file or folder…",
        "Newest TU Darmstadt templates from CTAN",
        "Not pinned and without a checksum",
    ] {
        assert!(t.shows(want), "{want:?} missing in {:?}", t.painted());
    }
    assert!(t.app.tex.task.is_none(), "looking at the page starts no job");
    assert!(!sets_root(&t.dir).exists(), "and writes nothing to the vault");
}

// spec: TEX-024
#[test]
fn a_set_is_listed_with_its_name_source_version_kinds_and_logo_status() {
    let (mut t, dir) = with_set("sets-list", &[("article", KIND)]);
    for want in ["Test set  1.0", "from /somewhere/set.zip", "imported 2026-11-17", "templates/tex/mine", "1 document kind", "logo missing: exports are not CI-conformant", "Update set", "Choose logo…", "Test page", "Remove set…"] {
        assert!(t.shows(want), "{want:?} missing in {:?}", t.painted());
    }
    assert!(!t.shows("add them by hand"), "a set with a kind needs no explanation");
    // several kinds, a logo, a manifest that cannot be read
    for k in ["slides", "poster"] {
        std::fs::write(dir.join("kinds").join(format!("{k}.toml")), KIND).unwrap();
    }
    std::fs::write(dir.join("kinds/broken.toml"), "name = 3").unwrap();
    std::fs::create_dir_all(dir.join("logo")).unwrap();
    std::fs::write(dir.join("logo/mylogo.pdf"), "%PDF-1.4").unwrap();
    t.app.tex.sets.dirty = true;
    assert!(t.shows("3 document kinds") && t.shows("logo: mylogo.pdf") && t.shows("kinds/broken.toml:") && !t.shows("logo missing"), "{:?}", t.painted());
    // a set without kinds says what to do; one that cannot be read says why and can still be removed
    for k in ["slides", "poster", "article", "broken"] {
        std::fs::remove_file(dir.join("kinds").join(format!("{k}.toml"))).unwrap();
    }
    std::fs::create_dir_all(sets_root(&t.dir).join("odd")).unwrap();
    t.app.tex.sets.dirty = true;
    assert!(t.shows("no document kinds") && t.shows("add them by hand in the folder kinds (see README.md"), "{:?}", t.painted());
    assert!(t.shows("This folder cannot be read as a set: set.toml cannot be read") && t.shows("templates/tex/odd"), "{:?}", t.painted());
}

// spec: TEX-024, TEX-006
#[test]
fn import_from_a_file_or_folder_asks_for_the_path_runs_as_a_job_and_lists_the_new_set() {
    let mut t = T::new("sets-import", nothing_found());
    let src = source_folder(&t.dir.parent().unwrap().join(format!("shale-sets-src-{}", std::process::id())), "Fancy Set");
    t.click("Import from file or folder…");
    assert!(t.shows("Path of a .zip or .tar.gz file, or of a folder") && t.shows("Import set") && t.shows("Cancel"), "{:?}", t.painted());
    assert!(!sets_root(&t.dir).exists(), "asking imports nothing");
    t.click("Cancel");
    assert_eq!(t.app.tex.sets.form, Form::None);
    t.app.tex.sets.form = Form::File(src.to_string_lossy().into_owned());
    t.click("Import set");
    t.wait_for_job();
    let n = t.app.tex.notice.clone().expect("a notice");
    assert!(!n.ok && n.text.contains("Fancy Set was imported, but Shale has no document kinds") && n.text.contains("added by hand"), "{n:?}");
    assert!(t.shows("Fancy Set") && t.shows("no document kinds") && t.shows("from "), "the set is listed: {:?}", t.painted());
    assert!(sets_root(&t.dir).join("fancy-set/tex/fancy.cls").is_file());
    // the file list of the vault knows the new note
    assert!(t.app.vault.as_ref().unwrap().files.iter().any(|f| f.rel == Path::new("templates/tex/fancy-set/README.md")));
    // a failure is said and nothing is left
    t.app.tex.sets.form = Form::File(t.dir.join("not-there.zip").to_string_lossy().into_owned());
    t.click("Import set");
    t.wait_for_job();
    let n = t.app.tex.notice.clone().unwrap();
    assert!(!n.ok && n.text.contains("The import failed, the vault is as it was") && n.text.contains("cannot be read"), "{n:?}");
    let _ = std::fs::remove_dir_all(src.parent().unwrap());
}

// spec: TEX-024, TEX-006
#[test]
fn the_github_form_checks_its_address_and_the_ctan_and_preset_buttons_start_a_job() {
    let mut t = T::new("sets-github", nothing_found());
    t.click("Import from GitHub…");
    for want in ["GitHub repository", "Branch, tag or commit (empty: the default branch)", "Import set"] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    t.app.tex.sets.form = Form::Github { url: "https://gitlab.com/o/r".into(), reference: String::new() };
    t.click("Import set");
    t.wait_for_job();
    assert!(t.app.tex.notice.clone().unwrap().text.contains("is not a GitHub repository address"), "{:?}", t.app.tex.notice);
    assert_eq!(t.app.tex.sets.form, Form::None);
    // the two fixed sources start a job at once (here with no TeX, which they say; nothing is downloaded before docstrip is possible? no: the download comes first, so only the job is checked)
    for label in ["Import TU Darmstadt templates", "Newest TU Darmstadt templates from CTAN"] {
        t.click(label);
        assert!(t.app.tex.task.as_ref().is_some_and(|k| k.kind == TaskKind::Import), "{label}");
        t.app.tex.task.as_ref().unwrap().handle.cancel();
        t.wait_for_job();
        assert!(t.app.tex.notice.is_some(), "{label}");
    }
}

// spec: TEX-024, TEX-006
#[test]
fn a_running_import_shows_its_progress_and_cancel_asks_the_job_to_stop() {
    let mut t = T::new("sets-progress", nothing_found());
    let handle = Handle::default();
    handle.set(|p| *p = Progress { stage: Stage::Generate, ..Progress::default() });
    let (_tx, rx) = std::sync::mpsc::channel();
    t.app.tex.inject(TaskKind::Import, handle.clone(), rx);
    assert!(t.shows("Generating the class files with docstrip") && t.shows("Cancel"), "{:?}", t.painted());
    assert!(!t.shows("Installing TeX Live…"), "it is not shown with the TeX Live");
    t.click("Cancel");
    assert!(handle.cancelled());
}

// spec: JOB-029
#[test]
fn job_029_an_import_is_a_task_that_can_stop_and_cancel_of_the_registry_ends_it() {
    let mut t = T::new("job035-import", nothing_found());
    t.click("Import TU Darmstadt templates");
    let task = t.app.busy.info(&super::settings_ui::TEX_JOB).expect("the import is a task");
    assert_eq!((task.label.as_str(), task.cancellable), ("Importing the template set…", true));
    assert!(t.shows("Importing the template set…") && t.shows("Cancel"), "{:?}", t.painted());
    assert_eq!(t.app.busy.list().len(), 1, "the indicator lists it");
    // the indicator's Cancel is the registry's: it reaches the job through the flag of its handle
    assert!(t.app.busy.cancel(task.id));
    t.wait_for_job();
    assert!(t.app.tex.notice.is_some());
    assert!(t.app.busy.list().is_empty());
}

// spec: TEX-024, TEX-006
#[test]
fn update_imports_again_from_the_place_the_set_came_from() {
    let (mut t, dir) = with_set("sets-update", &[("article", KIND)]);
    let src = source_folder(&t.dir.parent().unwrap().join(format!("shale-sets-upd-{}", std::process::id())), "mine");
    std::fs::write(dir.join("set.toml"), meta_from_local(&src.to_string_lossy())).unwrap();
    std::fs::write(dir.join("settings.toml"), "accent = \"3b\"\n").unwrap();
    t.app.tex.sets.dirty = true;
    t.click("Update set");
    t.wait_for_job();
    let n = t.app.tex.notice.clone().unwrap();
    assert!(n.text.contains("mine was updated") && n.text.contains("settings and logo were kept"), "{n:?}");
    assert_eq!(std::fs::read_to_string(dir.join("settings.toml")).unwrap(), "accent = \"3b\"\n");
    assert!(dir.join("tex/fancy.cls").is_file());
    let _ = std::fs::remove_dir_all(src.parent().unwrap());
    // the source of an update is the one in set.toml
    let github = Source::Github { repo: "o/r".into(), reference: "v1".into(), sha256: None };
    assert_eq!(what_for(&github), super::import::What::Github { url: "https://github.com/o/r".into(), reference: "v1".into() });
    let pinned = Source::Github { repo: super::import::PRESET_REPO.into(), reference: super::import::PRESET_COMMIT.into(), sha256: Some(super::import::PRESET_SHA256.into()) };
    assert_eq!(what_for(&pinned), super::import::What::Preset);
    let other_pin = Source::Github { repo: super::import::PRESET_REPO.into(), reference: "main".into(), sha256: None };
    assert_ne!(what_for(&other_pin), super::import::What::Preset, "only the tested commit with its checksum is the preset");
    assert_eq!(what_for(&Source::Ctan { url: "x".into() }), super::import::What::Ctan);
    assert_eq!(what_for(&Source::Local { path: "/a/b.zip".into() }), super::import::What::Local("/a/b.zip".into()));
}

// spec: TEX-024, TEX-007
#[test]
fn choose_logo_asks_for_the_file_copies_it_and_the_status_follows() {
    let (mut t, dir) = with_set("sets-logo", &[("article", KIND)]);
    assert!(t.shows("logo missing"));
    t.click("Choose logo…");
    assert!(t.shows("Path of the logo file (PDF or PNG)") && t.shows("Copy logo into the set"), "{:?}", t.painted());
    // a file that is no logo is refused and the slot stays empty
    let text = t.dir.join("note.txt");
    std::fs::write(&text, "no logo").unwrap();
    t.app.tex.sets.form = Form::Logo { set: "mine".into(), slot: Slot::University, path: text.to_string_lossy().into_owned() };
    t.click("Copy logo into the set");
    assert!(t.shows("The logo was not copied: the logo must be a PDF or a PNG file") && t.shows("logo missing"), "{:?}", t.painted());
    assert!(!dir.join("logo/mylogo.pdf").exists());
    // a PDF is copied
    let pdf = t.dir.join("any.pdf");
    std::fs::write(&pdf, super::sets::placeholder_logo_pdf()).unwrap();
    t.click("Choose logo…");
    t.app.tex.sets.form = Form::Logo { set: "mine".into(), slot: Slot::University, path: pdf.to_string_lossy().into_owned() };
    t.click("Copy logo into the set");
    assert!(t.shows("The logo mylogo.pdf is in the set “Test set” now.") && t.shows("logo: mylogo.pdf") && !t.shows("logo missing"), "{:?}", t.painted());
    assert!(dir.join("logo/mylogo.pdf").is_file());
    assert!(t.app.vault.as_ref().unwrap().files.iter().any(|f| f.rel == Path::new("templates/tex/mine/logo/mylogo.pdf")), "the vault knows the new file");
    t.click("Choose logo…");
    t.click("Cancel");
    assert_eq!(t.app.tex.sets.form, Form::None);
}

// spec: TEX-024, TEX-035
#[test]
fn choose_department_logo_copies_it_the_status_says_so_and_remove_department_logo_trashes_it() {
    let (mut t, dir) = with_set("sets-dept", &[("article", KIND)]);
    // an empty slot is no warning, and there is nothing to remove
    assert!(t.shows("Choose department logo…") && t.shows("Remove department logo") && !t.shows("department logo:"), "{:?}", t.painted());
    t.click("Remove department logo");
    assert!(t.app.tex.notice.is_none(), "a disabled button does nothing");
    t.click("Choose department logo…");
    assert!(t.shows("Path of the department logo file (PDF or PNG)") && t.shows("Copy logo into the set"), "{:?}", t.painted());
    // a file that is no logo is refused
    let text = t.dir.join("note.txt");
    std::fs::write(&text, "no logo").unwrap();
    t.app.tex.sets.form = Form::Logo { set: "mine".into(), slot: Slot::Department, path: text.to_string_lossy().into_owned() };
    t.click("Copy logo into the set");
    assert!(t.shows("The department logo was not copied: the logo must be a PDF or a PNG file") && !t.shows("department logo:"), "{:?}", t.painted());
    // a PNG is copied; the university's logo is untouched and still missing
    let png = t.dir.join("hrz.png");
    std::fs::write(&png, PNG).unwrap();
    t.click("Choose department logo…");
    t.app.tex.sets.form = Form::Logo { set: "mine".into(), slot: Slot::Department, path: png.to_string_lossy().into_owned() };
    t.click("Copy logo into the set");
    assert!(t.shows("The department logo department_logo.png is in the set “Test set” now.") && t.shows("department logo: department_logo.png") && t.shows("logo missing"), "{:?}", t.painted());
    assert_eq!(std::fs::read(dir.join("logo/department_logo.png")).unwrap(), PNG);
    assert!(t.app.vault.as_ref().unwrap().files.iter().any(|f| f.rel == Path::new("templates/tex/mine/logo/department_logo.png")), "the vault knows the new file");
    t.click("Choose department logo…");
    t.click("Cancel");
    assert_eq!(t.app.tex.sets.form, Form::None);
    // removed: to the vault's trash, the setting cleared, the status gone
    t.click("Remove department logo");
    assert!(t.shows("The department logo of the set “Test set” is in the vault's trash now.") && !t.shows("department logo: department_logo.png"), "{:?}", t.painted());
    assert!(!dir.join("logo/department_logo.png").exists() && super::sets::load_settings(&dir).unwrap().department_logo.is_none());
    let day = std::fs::read_dir(t.dir.join(".trash")).unwrap().next().unwrap().unwrap().path();
    assert_eq!(std::fs::read(day.join("templates/tex/mine/logo/department_logo.png")).unwrap(), PNG);
    // a set that needs no university logo has the slot too
    make_set(&t.dir, "plain", &META.replace("logo_file = \"mylogo\"\n", ""), &[("article", KIND)]);
    t.app.tex.sets.dirty = true;
    assert!(t.shows("no logo needed"));
}

// spec: TEX-024, TEX-007
#[test]
fn a_set_that_needs_no_logo_has_no_choose_logo_button_to_press() {
    let mut t = T::new("sets-nologo", nothing_found());
    make_set(&t.dir, "plain", &META.replace("logo_file = \"mylogo\"\n", ""), &[("article", KIND)]);
    t.app.tex.sets.dirty = true;
    assert!(t.shows("no logo needed"));
    let before = t.app.tex.sets.form.clone();
    t.click("Choose logo…");
    assert_eq!(t.app.tex.sets.form, before, "a disabled button does nothing");
}

// spec: TEX-024, TEX-005
#[test]
fn remove_asks_first_and_moves_the_set_to_the_vault_trash() {
    let (mut t, dir) = with_set("sets-remove", &[("article", KIND)]);
    std::fs::write(dir.join("settings.toml"), "accent = \"3b\"\n").unwrap();
    t.click("Remove set…");
    let text = t.painted().join("\n");
    assert!(text.contains("Move the set “Test set” (templates/tex/mine) to the vault's .trash folder?") && text.contains("You can restore it from there"), "{text}");
    assert!(dir.is_dir(), "asking removes nothing");
    t.click("Keep the set");
    assert!(dir.is_dir() && t.app.tex.sets.form == Form::None);
    t.click("Remove set…");
    t.click("Move set to trash");
    t.hl.frames(&mut t.app, 3);
    assert!(!dir.exists(), "the set is gone from the vault");
    let day = std::fs::read_dir(t.dir.join(".trash")).unwrap().next().unwrap().unwrap().path();
    assert!(day.join("templates/tex/mine/set.toml").is_file() && day.join("templates/tex/mine/settings.toml").is_file(), "all of it is in the trash, nothing is deleted");
    assert!(t.shows("There are no template sets in this vault yet."), "{:?}", t.painted());
    assert!(t.app.can_restore_trashed(), "and the window's restore knows it");
    t.app.restore_trashed();
    assert!(dir.join("set.toml").is_file(), "restored");
}

// spec: TEX-024
#[test]
fn the_sets_belong_to_the_vault_that_is_open() {
    let mut s = SetsState::default();
    let tree = crate::gitsync::TreeLock::default();
    let a = VaultRef { root: PathBuf::from("/a"), tree: tree.clone() };
    s.bind(Some(a.clone()));
    s.sets = Some(vec![]);
    s.form = Form::File("x".into());
    s.bind(Some(a));
    assert!(s.sets.is_some() && s.form != Form::None, "the same vault: nothing changes");
    s.bind(Some(VaultRef { root: PathBuf::from("/b"), tree }));
    assert!(s.sets.is_none() && s.form == Form::None, "another vault: read again, no form");
    assert_eq!(s.take_trash(), None);
    assert!(!s.take_rescan());
}

#[cfg(unix)]
mod fake {
    use super::super::settings_ui_tests::found;
    use super::*;
    use crate::pdf::tex::compile_tests::fake::Fake;

    // spec: JOB-029
    #[test]
    fn job_029_the_test_page_of_a_set_is_a_task_that_names_the_set_and_cannot_stop() {
        let f = Fake::new("job035-page");
        f.lualatex();
        f.put("log.last", "ok\n");
        let gate = f.hold();
        let opener = crate::opener::FakeXdgOpen::install();
        let mut t = T::new("job035-page", found(Ok(f.engine()), Err("not installed"), None));
        make_set(&t.dir, "mine", META, &[("pub", KIND)]);
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        let task = t.app.busy.info(&super::super::settings_ui::TEX_JOB).expect("the test page is a task");
        assert_eq!((task.label.as_str(), task.cancellable), ("Compiling the test page of “Test set”…", false));
        assert!(t.shows("Compiling the test page of “Test set”…") && !t.shows("Cancel"), "{:?}", t.painted());
        std::fs::write(&gate, "").unwrap();
        t.wait_for_job();
        assert!(t.app.busy.list().is_empty());
        assert!(t.app.tex.notice.clone().expect("a notice").ok);
        assert_eq!(opener.calls(1).len(), 1);
    }

    // spec: TEX-024, TEX-026
    #[test]
    fn test_page_compiles_a_document_of_the_set_with_the_chosen_tex_and_opens_the_pdf() {
        let opener = crate::opener::FakeXdgOpen::install();
        let f = Fake::new("sets-test-page");
        f.lualatex();
        f.put("log.last", "ok\n");
        let mut t = T::new("sets-page", found(Ok(f.engine()), Err("not installed"), None));
        let dir = make_set(&t.dir, "mine", META, &[("pub", KIND)]);
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        t.wait_for_job();
        let n = t.app.tex.notice.clone().expect("a notice");
        assert!(n.ok && n.text.contains("The test page of “Test set” (kind pub) compiled to a PDF of 1 KB.") && n.text.ends_with("placeholder logo, so it is not CI-conformant. It was opened in your PDF viewer."), "{n:?}");
        let calls = opener.calls(1);
        assert!(calls[0].iter().any(|a| a.ends_with("mine-test-page.pdf")), "{calls:?}");
        assert!(f.get("args").contains("test-page.tex"));
        assert!(!dir.join("logo").exists(), "the placeholder is not in the vault");
        // with a logo there is no word about a placeholder
        std::fs::create_dir_all(dir.join("logo")).unwrap();
        std::fs::write(dir.join("logo/mylogo.pdf"), super::super::sets::placeholder_logo_pdf()).unwrap();
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        t.wait_for_job();
        let n = t.app.tex.notice.clone().unwrap();
        assert!(n.ok && !n.text.contains("placeholder"), "{n:?}");
        // a failure shows the readable problems and the end of the log
        f.put("log.last", "! LaTeX Error: File `my.cls' not found.\n");
        f.put("exit", "1");
        t.click("Test page");
        t.wait_for_job();
        let n = t.app.tex.notice.clone().unwrap();
        assert!(!n.ok && n.text.contains("did not compile") && n.text.contains("class my is missing") && n.details.as_deref().is_some_and(|d| d.contains("my.cls")), "{n:?}");
    }

    // spec: TEX-024, TEX-035
    #[test]
    fn the_test_page_shows_the_department_logo_when_the_set_has_one() {
        use crate::pdf::tex::compile_tests::fake::script;
        let _opener = crate::opener::FakeXdgOpen::install();
        let f = Fake::new("sets-test-page-dept");
        let keep = f.state();
        script(&f.dir.join("bin"), "lualatex", &format!("cp test-page.tex '{0}/page.tex'; echo ok > test-page.log; echo '%PDF-1.5 fake' > test-page.pdf", keep.display()));
        let mut t = T::new("sets-page-dept", found(Ok(f.engine()), Err("not installed"), None));
        let dir = make_set(&t.dir, "mine", META, &[("pub", &format!("{KIND}department_logo = [\"\\\\x{{department_logo}}\"]\n"))]);
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        t.wait_for_job();
        assert!(!std::fs::read_to_string(keep.join("page.tex")).unwrap().contains("department_logo"), "an empty slot: no department logo");
        std::fs::create_dir_all(dir.join("logo")).unwrap();
        std::fs::write(dir.join("logo/department_logo.png"), PNG).unwrap();
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        t.wait_for_job();
        let page = std::fs::read_to_string(keep.join("page.tex")).unwrap();
        assert!(page.contains("\\xdepartment_logo\n") && page.find("\\xdepartment_logo").unwrap() < page.find("\\begin{document}").unwrap(), "{page}");
    }

    // spec: TEX-024
    #[test]
    fn test_page_says_when_there_is_no_tex_or_no_article_kind() {
        let mut t = T::new("sets-page-none", nothing_found());
        make_set(&t.dir, "mine", META, &[("pub", KIND)]);
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        let n = t.app.tex.notice.clone().expect("a notice");
        assert!(!n.ok && n.text.contains("No TeX can be used for the test page") && n.text.contains("no lualatex in PATH"), "{n:?}");
        assert!(t.app.tex.task.is_none(), "no job was started");
        let f = Fake::new("sets-page-nokind");
        f.lualatex();
        let mut t = T::new("sets-page-nokind", found(Ok(f.engine()), Err("x"), None));
        make_set(&t.dir, "mine", META, &[("slides", "name = \"S\"\nmapping = \"beamer\"\nclass = \"my\"\n")]);
        t.app.tex.sets.dirty = true;
        t.click("Test page");
        t.wait_for_job();
        let n = t.app.tex.notice.clone().unwrap();
        assert!(!n.ok && n.text.contains("has no article kind"), "{n:?}");
    }
}
