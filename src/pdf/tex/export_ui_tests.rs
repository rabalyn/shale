//! The LaTeX choice of the note export dialog (TEX-016) driven through the whole window, headless,
//! with real pointer events: the engine choice and what it shows and hides, the set and kind
//! pickers, the fields filled from the note's properties, Export without a TeX, the logo
//! warning, the working state with Cancel, the error view with "Save the .tex", and the choices
//! the dialog remembers. A fake LuaLaTeX (a script) stands in for TeX; the export job is the real one.

use super::engine::Kind;
use super::import::TUDA;
use super::settings_ui_tests::{found, good, nothing_found, T};
use crate::app::Modal;
use crate::pdf::export_dialog::{ExportDialog, ExportEngine};
use eframe::egui::{PointerButton, Rect};
use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
#[cfg(unix)]
use std::time::Duration;

/// The imported set of the tuda kinds in the vault of the window.
fn put_set(t: &T) {
    let kinds: Vec<(&str, &str)> = TUDA.kinds.iter().map(|(id, text)| (*id, *text)).collect();
    super::sets_tests::make_set(&t.dir, "tuda", super::sets_tests::META, &kinds);
}

fn dialog(t: &T) -> &ExportDialog {
    match &t.app.modal {
        Some(Modal::Export(d)) => d,
        _ => panic!("the export dialog is not open"),
    }
}

fn dialog_mut(t: &mut T) -> &mut ExportDialog {
    match &mut t.app.modal {
        Some(Modal::Export(d)) => d,
        _ => panic!("the export dialog is not open"),
    }
}

fn rect(t: &T, key: &str) -> Rect {
    dialog(t).rects.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("no control {key:?}: {:?}", dialog(t).rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1
}

fn has(t: &T, key: &str) -> bool {
    dialog(t).rects.iter().any(|(k, _)| k == key)
}

fn click(t: &mut T, key: &str) {
    t.hl.frames(&mut t.app, 2);
    let r = rect(t, key);
    t.hl.click(&mut t.app, r.center(), PointerButton::Primary);
    t.hl.frames(&mut t.app, 2);
}

/// A note in the vault, and the dialog opened on it.
fn open_on(t: &mut T, name: &str, text: &str) {
    std::fs::write(t.dir.join(name), text).unwrap();
    t.app.vault.as_mut().unwrap().rescan();
    t.app.open_export_dialog(Path::new(name));
    t.hl.frames(&mut t.app, 3);
}

/// A folder of notes in the vault, a book (BOOK-013), and its dialog opened.
fn open_on_folder(t: &mut T) {
    for (name, text) in [("Book/index.md", "---\ntitle: The Book\nauthor: Ada\n---\nIntro text\n"), ("Book/One.md", "# One\nSee [[Two]].\n"), ("Book/Part/Two.md", "## Head\nText\n")] {
        let path = t.dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    t.app.vault.as_mut().unwrap().rescan();
    t.app.open_export_folder_dialog(Path::new("Book"));
    t.hl.frames(&mut t.app, 3);
}

/// Frames until `done` holds.
#[cfg(unix)]
fn until(t: &mut T, what: &str, done: impl Fn(&T) -> bool) {
    for _ in 0..600 {
        t.hl.frames(&mut t.app, 1);
        if done(t) {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for: {what}");
}

fn usable() -> super::engine::Detection {
    found(Ok(good(Kind::System, &[])), Err("not installed"), None)
}

// spec: TEX-016
#[test]
fn the_engine_choice_starts_on_the_built_in_export_and_the_template_shows_its_own_choices() {
    let mut t = T::new("dlg-engine", usable());
    put_set(&t);
    open_on(&mut t, "Plan.md", "# Plan\n");
    assert_eq!(dialog(&t).engine, ExportEngine::Builtin, "the built-in engine stays selected until the person chooses otherwise");
    for want in ["Engine", "Built-in", "LaTeX template", "Paper", "Margins", "Text size", "Table of contents"] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    assert!(!t.shows("Template set") && !has(&t, "set"), "no template choices for the built-in engine");
    click(&mut t, "engine:latex");
    assert_eq!(dialog(&t).engine, ExportEngine::Latex);
    for want in ["Template set", "Kind", "Table of contents", "Save as", "Using the TeX of this computer (LaTeX kernel 2025-11-01)"] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    // the template decides paper, margins and text size
    for gone in ["Paper", "Orientation", "Margins", "Text size", "Page numbers", "Export plain text"] {
        assert!(!t.shows(gone), "{gone:?} is hidden: {:?}", t.painted());
    }
    assert!(has(&t, "set") && has(&t, "kind") && has(&t, "toc") && has(&t, "embeds") && !has(&t, "paper") && !has(&t, "margin") && !has(&t, "font"));
    // and back
    click(&mut t, "engine:builtin");
    assert!(t.shows("Paper") && !t.shows("Template set"));
    // a drawing has the built-in export only
    t.app.open_export_dialog(Path::new("Sketch.excalidraw"));
    t.hl.frames(&mut t.app, 3);
    assert!(dialog(&t).tex.is_none() && !has(&t, "engine:latex") && !t.shows("LaTeX template"));
}

// spec: TEX-016
#[test]
fn the_pickers_offer_the_sets_of_the_vault_and_the_kinds_of_the_set() {
    let mut t = T::new("dlg-pickers", usable());
    put_set(&t);
    // a second set with one kind, and a folder that is no set
    super::sets_tests::make_set(&t.dir, "other", super::sets_tests::META.replace("Test set", "Other set").as_str(), &[("note", "name = \"A note\"\nmapping = \"article\"\nclass = \"my\"\n")]);
    std::fs::create_dir_all(t.dir.join("templates/tex/empty")).unwrap();
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    let tex = dialog(&t).tex.as_ref().unwrap();
    assert_eq!(tex.sets.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["other", "tuda"], "the sets that have kinds, by name");
    // the first set is chosen, with its first kind
    assert_eq!((tex.current_set().unwrap().id.as_str(), tex.kind.as_str()), ("other", "note"));
    click(&mut t, "set");
    click(&mut t, "set:tuda");
    assert_eq!(dialog(&t).tex.as_ref().unwrap().current_set().unwrap().id, "tuda");
    click(&mut t, "kind");
    for k in ["pub", "report", "exercise", "thesis", "phd", "beamer", "letter", "poster", "sciposter", "leaflet", "announcement"] {
        assert!(has(&t, &format!("kind:{k}")), "kind {k} is offered");
    }
    click(&mut t, "kind:beamer");
    assert_eq!(dialog(&t).tex.as_ref().unwrap().kind, "beamer");
    assert!(t.shows("Slides") && t.shows("Subtitle") && t.shows("Institute"), "the fields of the kind: {:?}", t.painted());
    // the fields of another kind replace them
    click(&mut t, "kind");
    click(&mut t, "kind:letter");
    assert!(t.shows("Recipient") && t.shows("Subject") && !t.shows("Institute"), "{:?}", t.painted());
}

// spec: BOOK-013
#[test]
fn a_folder_has_the_engine_choice_and_only_the_kinds_that_can_hold_a_book() {
    let mut t = T::new("dlg-folder", usable());
    put_set(&t);
    open_on_folder(&mut t);
    assert_eq!(dialog(&t).engine, ExportEngine::Builtin, "the built-in engine until the person chooses otherwise");
    assert!(t.shows("Engine") && t.shows("LaTeX template") && t.shows("Each note on a new page") && has(&t, "order"), "{:?}", t.painted());
    click(&mut t, "engine:latex");
    for want in ["Template set", "Kind", "Reading order", "Save as", "A folder is printed as an article or a book"] {
        assert!(t.shows(want), "{want:?} in {:?}", t.painted());
    }
    assert!(!t.shows("Paper") && !t.shows("Each note on a new page") && has(&t, "order") && has(&t, "folder-kinds"), "{:?}", t.painted());
    // the kind of the test page, the fields from the top folder note
    let tex = dialog(&t).tex.as_ref().unwrap();
    assert_eq!((tex.kind.as_str(), tex.values["title"].as_str(), tex.values["author"].as_str()), ("pub", "The Book", "Ada"));
    for k in ["report", "exercise", "phd", "thesis"] {
        click(&mut t, "kind");
        click(&mut t, &format!("kind:{k}"));
        assert_eq!(dialog(&t).tex.as_ref().unwrap().kind, k, "{k} can hold a folder");
    }
    for k in ["beamer", "letter", "poster", "sciposter", "leaflet", "announcement"] {
        click(&mut t, "kind");
        assert!(has(&t, &format!("kind:{k}")), "{k} is shown, greyed");
        click(&mut t, &format!("kind:{k}"));
        assert_eq!(dialog(&t).tex.as_ref().unwrap().kind, "thesis", "{k} cannot be chosen for a folder");
    }
}

// spec: BOOK-013, TEX-033
#[test]
fn a_folder_note_that_names_a_kind_a_book_cannot_use_says_so_and_what_is_used_instead() {
    let mut t = T::new("dlg-folder-beamer", usable());
    put_set(&t);
    let top = |t: &mut T, template: &str| {
        let path = t.dir.join("Book/index.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("---\nlatex-template: {template}\n---\nIntro\n")).unwrap();
        std::fs::write(t.dir.join("Book/One.md"), "# One\n").unwrap();
        t.app.vault.as_mut().unwrap().rescan();
        t.app.modal = None;
        t.app.open_export_folder_dialog(Path::new("Book"));
        t.hl.frames(&mut t.app, 3);
    };
    top(&mut t, "tuda/beamer");
    let d = dialog(&t);
    assert_eq!(d.engine, ExportEngine::Latex, "the note's own template picks the engine");
    let tex = d.tex.as_ref().unwrap();
    assert_eq!(tex.kind, "pub", "a kind a book cannot use is not taken");
    let warning = tex.template_warning.clone().expect("a warning");
    let using = format!("“{}” of “{}”", tex.current_kind().unwrap().name, tex.current_set().unwrap().meta.name);
    let beamer = tex.current_set().unwrap().kinds.iter().find(|(id, _)| id == "beamer").unwrap().1.name.clone();
    assert_eq!(warning, format!("The note's latex-template asks for “{beamer}”, which takes a single note, not a folder: using {using} instead."));
    assert!(has(&t, "template-warning") && t.shows("takes a single note, not a folder"), "it is shown: {:?}", t.painted());
    // picking a kind by hand takes it away
    click(&mut t, "kind");
    click(&mut t, "kind:report");
    assert!(dialog(&t).tex.as_ref().unwrap().template_warning.is_none() && !has(&t, "template-warning"));
    // a kind a book can use needs no warning
    top(&mut t, "tuda/thesis");
    let tex = dialog(&t).tex.as_ref().unwrap();
    assert_eq!(tex.kind, "thesis");
    assert!(tex.template_warning.is_none() && !has(&t, "template-warning"));
}

// spec: BOOK-013, TEX-017
#[test]
fn set_up_from_a_folder_goes_back_to_the_folders_dialog_on_the_latex_engine() {
    let mut t = T::new("dlg-folder-setup", usable());
    open_on_folder(&mut t);
    click(&mut t, "engine:latex");
    click(&mut t, "setup:sets");
    t.hl.frames(&mut t.app, 4);
    assert!(matches!(t.app.modal, Some(Modal::TexSetup)), "the guided setup is open");
    assert_eq!(t.app.tex.setup.ret, super::setup::Return::ExportFolder(std::path::PathBuf::from("Book")));
    t.click("Cancel");
    t.hl.frames(&mut t.app, 6);
    let d = dialog(&t);
    assert!(d.book.as_ref().is_some_and(|b| b.folder == Path::new("Book")) && d.latex(), "the folder's dialog is back, on the LaTeX engine");
}

// spec: TEX-016
#[test]
fn a_vault_without_sets_says_where_to_import_one_and_export_stays_off() {
    let mut t = T::new("dlg-nosets", usable());
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    assert!(t.shows("This vault has no template sets yet: import one in Settings → LaTeX → Template sets."), "{:?}", t.painted());
    assert!(!dialog(&t).tex.as_ref().unwrap().can_export());
    click(&mut t, "export");
    assert!(t.app.modal.is_some() && dialog(&t).tex.as_ref().unwrap().job.is_none(), "Export does nothing without a kind");
}

// spec: TEX-016, TEX-013
#[test]
fn the_fields_are_filled_from_the_notes_properties_and_the_notes_name() {
    let mut t = T::new("dlg-fields", usable());
    put_set(&t);
    open_on(&mut t, "261117_Report.md", "---\nauthor: [Ada, Bob]\nsubtitle: A study\nreviewer: Prof. X\n---\n# Report\n");
    click(&mut t, "engine:latex");
    click(&mut t, "kind");
    click(&mut t, "kind:thesis");
    let v = &dialog(&t).tex.as_ref().unwrap().values;
    assert_eq!((v["title"].as_str(), v["author"].as_str(), v["subtitle"].as_str(), v["reviewer"].as_str(), v["department"].as_str()), ("Report", "Ada \\and Bob", "A study", "Prof. X", ""), "the note's name without the date is the title");
    assert!(t.shows("Ada \\and Bob") && t.shows("A study"), "{:?}", t.painted());
    assert!(t.shows("Author *") && t.shows("Title *") && !t.shows("Subtitle *"), "the required fields have a star: {:?}", t.painted());
    // an edit stays when the kind changes
    dialog_mut(&mut t).tex.as_mut().unwrap().values.insert("author".into(), "Carol".into());
    click(&mut t, "kind");
    click(&mut t, "kind:pub");
    assert_eq!(dialog(&t).tex.as_ref().unwrap().values["author"], "Carol");
}

// spec: TEX-016
#[test]
fn the_contents_box_is_greyed_for_a_kind_without_contents() {
    let mut t = T::new("dlg-toc", usable());
    put_set(&t);
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    click(&mut t, "kind");
    click(&mut t, "kind:pub");
    assert!(dialog(&t).opts.toc);
    click(&mut t, "toc");
    assert!(!dialog(&t).opts.toc, "an article has contents to switch");
    click(&mut t, "toc");
    assert!(dialog(&t).opts.toc);
    for k in ["beamer", "letter", "poster"] {
        click(&mut t, "kind");
        click(&mut t, &format!("kind:{k}"));
        let before = dialog(&t).opts.toc;
        click(&mut t, "toc");
        assert_eq!(dialog(&t).opts.toc, before, "{k}: the box is greyed and a click changes nothing");
    }
}

// spec: TEX-040
#[test]
fn the_box_for_the_page_break_after_the_contents_is_on_and_needs_a_kind_with_contents() {
    let mut t = T::new("dlg-tocbreak", usable());
    put_set(&t);
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    click(&mut t, "kind");
    click(&mut t, "kind:pub");
    assert!(dialog(&t).opts.toc_break, "on by default");
    click(&mut t, "toc-break");
    assert!(!dialog(&t).opts.toc_break);
    click(&mut t, "toc-break");
    assert!(dialog(&t).opts.toc_break);
    // a kind without contents: greyed, a click changes nothing
    click(&mut t, "kind");
    click(&mut t, "kind:beamer");
    click(&mut t, "toc-break");
    assert!(dialog(&t).opts.toc_break);
}

// spec: TEX-016
#[test]
fn without_a_tex_the_status_says_so_and_export_is_off_while_looking_it_is_off_too() {
    let mut t = T::new("dlg-notex", nothing_found());
    put_set(&t);
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    assert!(t.shows("No TeX: install it in Settings → LaTeX"), "{:?}", t.painted());
    assert!(t.shows("no lualatex in PATH"), "and why: {:?}", t.painted());
    click(&mut t, "export");
    assert!(t.app.modal.is_some() && dialog(&t).tex.as_ref().unwrap().job.is_none() && !t.dir.join("exports/Plan.pdf").exists(), "Export is disabled");
    // the built-in export still works
    click(&mut t, "engine:builtin");
    click(&mut t, "export");
    assert!(t.app.modal.is_none() && t.dir.join("exports/Plan.pdf").is_file());
    // a TeX that is still being looked for
    t.app.tex.detection = None;
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 3);
    assert_eq!(dialog(&t).engine, ExportEngine::Builtin, "the built-in export was the last");
}

// spec: TEX-016, TEX-007
#[test]
fn the_logo_warning_is_shown_while_the_set_has_no_logo() {
    let mut t = T::new("dlg-logo", usable());
    put_set(&t);
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    assert!(has(&t, "logo-warning") && t.shows("The set's logo is missing"), "{:?}", t.painted());
    // a logo in the slot: no warning
    std::fs::create_dir_all(t.dir.join("templates/tex/tuda/logo")).unwrap();
    std::fs::write(t.dir.join("templates/tex/tuda/logo/mylogo.pdf"), super::sets::placeholder_logo_pdf()).unwrap();
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 3);
    assert!(!has(&t, "logo-warning") && !t.shows("The set's logo is missing"));
}

/// A department logo in the slot of the set `tuda` of the window's vault (TEX-035).
fn put_department_logo(t: &T) {
    let logo = t.dir.join("templates/tex/tuda/logo");
    std::fs::create_dir_all(&logo).unwrap();
    std::fs::write(logo.join("mylogo.pdf"), super::sets::placeholder_logo_pdf()).unwrap();
    std::fs::write(logo.join("department_logo.png"), super::sets_tests::PNG).unwrap();
}

// spec: TEX-016, TEX-035
#[test]
fn the_department_logo_box_is_there_when_the_set_has_one_on_by_default_and_greyed_for_a_kind_without_a_place() {
    let mut t = T::new("dlg-dept", usable());
    put_set(&t);
    open_on(&mut t, "Plan.md", "# Plan\n");
    click(&mut t, "engine:latex");
    // no department logo in the set: no box and no warning about it
    assert!(!has(&t, "department-logo") && !t.shows("Department logo"), "{:?}", t.painted());
    put_department_logo(&t);
    t.app.open_export_dialog(Path::new("Plan.md"));
    t.hl.frames(&mut t.app, 3);
    click(&mut t, "engine:latex");
    click(&mut t, "kind");
    click(&mut t, "kind:pub");
    assert!(has(&t, "department-logo") && t.shows("Department logo") && dialog(&t).opts.department_logo, "on by default: {:?}", t.painted());
    click(&mut t, "department-logo");
    assert!(!dialog(&t).opts.department_logo);
    click(&mut t, "department-logo");
    assert!(dialog(&t).opts.department_logo);
    // a kind with no place for it: the box shows off and a click changes nothing
    for k in ["announcement", "exercise"] {
        click(&mut t, "kind");
        click(&mut t, &format!("kind:{k}"));
        assert!(!dialog(&t).tex.as_ref().unwrap().kind_shows_department_logo() && has(&t, "department-logo"), "{k}");
        let before = dialog(&t).opts.department_logo;
        click(&mut t, "department-logo");
        assert_eq!(dialog(&t).opts.department_logo, before, "{k}: the box is greyed and a click changes nothing");
    }
    for k in ["letter", "poster", "beamer", "thesis"] {
        click(&mut t, "kind");
        click(&mut t, &format!("kind:{k}"));
        assert!(dialog(&t).tex.as_ref().unwrap().kind_shows_department_logo(), "{k}");
    }
}

/// A LuaLaTeX that keeps the document it was given and makes a PDF.
#[cfg(unix)]
fn recording_tex(tag: &str) -> (crate::pdf::tex::compile_tests::fake::Fake, PathBuf) {
    use crate::pdf::tex::compile_tests::fake::{script, Fake};
    let f = Fake::new(tag);
    let keep = f.state();
    script(&f.dir.join("bin"), "lualatex", &format!("cp main.tex '{0}/main.tex'; ls > '{0}/ls'; echo ok > main.log; echo '%PDF-1.5 fake' > main.pdf", keep.display()));
    (f, keep)
}

#[cfg(unix)]
mod fake {
    use super::*;

    // spec: TEX-016, TEX-004, TEX-008
    #[test]
    fn export_writes_the_note_as_the_kind_compiles_it_as_a_job_and_saves_the_pdf_where_the_path_says() {
        let mut t = T::new("dlg-run", usable());
        put_set(&t);
        let (f, keep) = recording_tex("run");
        t.app.tex.detection = Some(found(Ok(f.engine()), Err("not installed"), None));
        open_on(&mut t, "Report.md", "---\nauthor: Ada\n---\n# Report\n## Intro\nText & more\n");
        click(&mut t, "engine:latex");
        click(&mut t, "kind");
        click(&mut t, "kind:pub");
        click(&mut t, "export");
        until(&mut t, "the export is done", |t| t.app.modal.is_none());
        let pdf = std::fs::read(t.dir.join("exports/Report.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.5 fake"), "the PDF LuaLaTeX made is the file");
        let tex = std::fs::read_to_string(keep.join("main.tex")).unwrap();
        assert!(tex.contains("\\documentclass[english]{tudapub}") && tex.contains("\\author{Ada}") && tex.contains("\\section{Intro}") && tex.contains("Text \\& more"), "{tex}");
        assert!(t.app.toasts.iter().any(|(m, _)| m.starts_with("Exported through LaTeX") && m.contains("placeholder")), "the missing logo is said: {:?}", t.app.toasts);
        assert!(t.app.toasts.iter().any(|(m, _)| m.starts_with("Exported to") && m.contains("Report.pdf")), "{:?}", t.app.toasts);
        // the choices are remembered for the vault, and the dialog starts on them
        let last = &t.app.vdata.state.latex_export;
        assert_eq!((last.latex, last.set.as_str(), last.kind.as_str()), (true, "tuda", "pub"));
        t.app.open_export_dialog(Path::new("Report.md"));
        t.hl.frames(&mut t.app, 3);
        assert_eq!((dialog(&t).engine, dialog(&t).tex.as_ref().unwrap().kind.as_str()), (ExportEngine::Latex, "pub"));
        // the built-in export is remembered too
        click(&mut t, "engine:builtin");
        click(&mut t, "export");
        assert!(!t.app.vdata.state.latex_export.latex && t.app.vdata.state.latex_export.kind == "pub");
        t.app.open_export_dialog(Path::new("Report.md"));
        t.hl.frames(&mut t.app, 3);
        assert_eq!(dialog(&t).engine, ExportEngine::Builtin);
    }

    // spec: TEX-016, TEX-035
    #[test]
    fn export_writes_the_department_logo_lines_only_while_the_box_is_on_and_the_slot_is_filled() {
        let mut t = T::new("dlg-dept-run", usable());
        put_set(&t);
        let (f, keep) = recording_tex("dept-run");
        t.app.tex.detection = Some(found(Ok(f.engine()), Err("not installed"), None));
        let export = |t: &mut T, name: &str, department_logo: bool| {
            open_on(t, name, "---\nauthor: Ada\n---\n# Report\n");
            click(t, "engine:latex");
            click(t, "kind");
            click(t, "kind:pub");
            if has(t, "department-logo") && dialog(t).opts.department_logo != department_logo {
                click(t, "department-logo");
            }
            click(t, "export");
            until(t, "the export is done", |t| t.app.modal.is_none());
            std::fs::read_to_string(keep.join("main.tex")).unwrap()
        };
        // an empty slot: nothing, and no box
        assert!(!export(&mut t, "A.md", true).contains("department_logo"));
        put_department_logo(&t);
        let on = export(&mut t, "B.md", true);
        assert!(on.contains("\\addTitleBoxLogo*{\\includegraphics[height=\\c_ptxcd_logoheight_dim]{department_logo}}"), "{on}");
        assert!(!export(&mut t, "C.md", false).contains("department_logo"), "the box is off");
    }

    // spec: BOOK-013, BOOK-014, BOOK-016
    #[test]
    fn a_folder_is_written_as_one_book_compiled_as_a_job_and_saved_and_its_engine_is_remembered() {
        let mut t = T::new("dlg-folder-run", usable());
        put_set(&t);
        let (f, keep) = recording_tex("folder-run");
        t.app.tex.detection = Some(found(Ok(f.engine()), Err("not installed"), None));
        open_on_folder(&mut t);
        // an edit in the editor that is not saved yet is in the book
        t.app.open(Path::new("Book/One.md"), false);
        t.hl.frames(&mut t.app, 2);
        let doc = t.app.notes.get_mut(Path::new("Book/One.md")).unwrap();
        doc.text.push_str("\nUnsaved line\n");
        doc.dirty = true;
        t.app.open_export_folder_dialog(Path::new("Book"));
        t.hl.frames(&mut t.app, 3);
        click(&mut t, "engine:latex");
        click(&mut t, "kind");
        click(&mut t, "kind:report");
        click(&mut t, "export");
        until(&mut t, "the export is done", |t| t.app.modal.is_none());
        let pdf = std::fs::read(t.dir.join("exports/Book.pdf")).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.5 fake"), "the PDF LuaLaTeX made is the file");
        let tex = std::fs::read_to_string(keep.join("main.tex")).unwrap();
        for want in [
            "{tudapub}",
            "\\title{The Book}",
            "\\author{Ada}",
            "Intro text",
            "\\section{One}\\shaleanchor{shale-2}",
            "See \\hyperlink{shale-4}{\\textcolor[RGB]{26,95,180}{Two}}",
            "Unsaved line",
            "\\section{Part}\\shaleanchor{shale-3}",
            "\\subsection{Two}\\shaleanchor{shale-4}",
            "\\subsubsection{Head}",
        ] {
            assert!(tex.contains(want), "{want:?} in\n{tex}");
        }
        assert!(t.app.toasts.iter().any(|(m, _)| m.starts_with("Exported to") && m.contains("Book.pdf")), "{:?}", t.app.toasts);
        let last = &t.app.vdata.state.latex_export;
        assert_eq!((last.latex, last.kind.as_str()), (true, "report"));
        // the next dialog of a folder starts on it; the built-in export of a folder is remembered too
        open_on_folder(&mut t);
        assert_eq!((dialog(&t).engine, dialog(&t).tex.as_ref().unwrap().kind.as_str()), (ExportEngine::Latex, "report"));
        click(&mut t, "engine:builtin");
        click(&mut t, "export");
        until(&mut t, "the built-in book is done", |t| t.app.modal.is_none());
        assert!(!t.app.vdata.state.latex_export.latex);
        open_on_folder(&mut t);
        assert_eq!(dialog(&t).engine, ExportEngine::Builtin);
    }

    // spec: TEX-016
    #[test]
    fn a_required_field_that_is_empty_stops_the_export_and_names_it_without_starting_a_job() {
        let mut t = T::new("dlg-required", usable());
        put_set(&t);
        open_on(&mut t, "Plan.md", "# Plan\ntext\n");
        click(&mut t, "engine:latex");
        click(&mut t, "kind");
        click(&mut t, "kind:pub");
        click(&mut t, "export");
        assert!(dialog(&t).error.as_deref().is_some_and(|e| e.starts_with("“Author” is empty")), "{:?}", dialog(&t).error);
        assert!(t.shows("“Author” is empty") && dialog(&t).tex.as_ref().unwrap().job.is_none());
        // typing the author in the field is enough
        dialog_mut(&mut t).tex.as_mut().unwrap().values.insert("author".into(), "Ada".into());
        let (f, _keep) = recording_tex("required");
        t.app.tex.detection = Some(found(Ok(f.engine()), Err("not installed"), None));
        click(&mut t, "export");
        until(&mut t, "the export is done", |t| t.app.modal.is_none());
        assert!(t.dir.join("exports/Plan.pdf").is_file());
    }

    // spec: TEX-016
    #[test]
    fn the_readme_of_a_saved_tex_gives_the_command_of_the_system() {
        use crate::pdf::tex::dialog::readme_text;
        let unix = readme_text("/v/templates/tex/tuda/tex", false);
        assert!(unix.contains("  TEXINPUTS=\"/v/templates/tex/tuda/tex//:\" lualatex main.tex\n"), "{unix}");
        assert!(!unix.contains("PowerShell") && !unix.contains("set \"") && !unix.contains(';'), "{unix}");
        let win = readme_text("C:\\v\\templates\\tex\\tuda\\tex", true);
        assert!(win.contains("  set \"TEXINPUTS=C:\\v\\templates\\tex\\tuda\\tex//;\"\n  lualatex main.tex\n"), "cmd: {win}");
        assert!(win.contains("  $env:TEXINPUTS=\"C:\\v\\templates\\tex\\tuda\\tex//;\"\n  lualatex main.tex\n"), "PowerShell: {win}");
        assert!(!win.contains("TEXINPUTS=\"C:\\v\\templates\\tex\\tuda\\tex//:") && !win.contains("\n  TEXINPUTS="), "no `VAR=… command` on Windows: {win}");
    }

    // spec: TEX-016, TEX-004
    #[test]
    fn a_failed_export_shows_the_readable_errors_and_save_the_tex_writes_the_files_for_debugging() {
        let mut t = T::new("dlg-fail", usable());
        put_set(&t);
        let f = crate::pdf::tex::compile_tests::fake::Fake::new("fail");
        f.lualatex();
        f.put("log.last", "./main.tex:21: Undefined control sequence.\nl.21 \\nosuchcommand\n! LaTeX Error: File `nosuch.sty' not found.\n");
        f.put("exit", "1");
        t.app.tex.detection = Some(found(Ok(f.engine()), Err("not installed"), None));
        open_on(&mut t, "Plan.md", "---\nauthor: Ada\n---\n# Plan\n![[Sketch.excalidraw]]\n");
        click(&mut t, "engine:latex");
        click(&mut t, "kind");
        click(&mut t, "kind:pub");
        click(&mut t, "export");
        until(&mut t, "the export failed", |t| dialog(t).tex.as_ref().unwrap().failure.is_some());
        assert!(t.shows("LuaLaTeX stopped with an error") && t.shows("main.tex:21: Undefined control sequence.") && t.shows("package nosuch is missing"), "{:?}", t.painted());
        assert!(!t.dir.join("exports/Plan.pdf").exists() && t.app.modal.is_some(), "no PDF, the dialog stays");
        assert!(t.shows("Save the .tex") && t.shows("The end of the log"), "{:?}", t.painted());
        click(&mut t, "save-tex");
        let folder = t.dir.join("exports/Plan-tex");
        let main = std::fs::read_to_string(folder.join("main.tex")).unwrap();
        assert!(main.contains("\\begin{document}") && main.contains("\\author{Ada}"), "{main}");
        assert!(folder.join("drawing-1.pdf").is_file(), "the files of the document are with it");
        let readme = std::fs::read_to_string(folder.join("README.txt")).unwrap();
        let set_tex = Path::new("templates").join("tex").join("tuda").join("tex");
        assert!(readme.contains("TEXINPUTS=") && readme.contains(&format!("{}//", set_tex.display())), "{readme}");
        assert!(t.app.toasts.iter().any(|(m, _)| m.starts_with("Saved main.tex and its files in") && m.contains("Plan-tex")), "{:?}", t.app.toasts);
        // trying again clears the error; the dialog can be closed with Cancel
        std::fs::remove_file(f.state().join("exit")).unwrap();
        f.put("log.last", "all well\n");
        click(&mut t, "export");
        until(&mut t, "the export is done", |t| t.app.modal.is_none());
        assert!(t.dir.join("exports/Plan.pdf").is_file());
    }

    // spec: TEX-016, TEX-004
    #[test]
    fn export_shows_that_it_is_working_and_cancel_stops_the_compile_and_closes_the_dialog() {
        let mut t = T::new("dlg-cancel", usable());
        put_set(&t);
        let marker = std::env::temp_dir().join(format!("shale-dlg-cancel-{}", std::process::id()));
        let _ = std::fs::remove_file(&marker);
        let f = crate::pdf::tex::compile_tests::fake::Fake::new("cancel");
        crate::pdf::tex::compile_tests::fake::script(&f.dir.join("bin"), "lualatex", &format!("touch '{0}.started'; sleep 30; touch '{0}'", marker.display()));
        t.app.tex.detection = Some(found(Ok(f.engine()), Err("not installed"), None));
        open_on(&mut t, "Plan.md", "---\nauthor: Ada\n---\n# Plan\n");
        click(&mut t, "engine:latex");
        click(&mut t, "kind");
        click(&mut t, "kind:pub");
        click(&mut t, "export");
        until(&mut t, "LuaLaTeX has started", |_| marker.with_extension("started").exists());
        assert!(dialog(&t).tex.as_ref().unwrap().working() && t.shows("Compiling with LuaLaTeX"), "{:?}", t.painted());
        // the choices cannot change while it runs, and Export is off
        let kind = dialog(&t).tex.as_ref().unwrap().kind.clone();
        click(&mut t, "engine:builtin");
        assert_eq!((dialog(&t).engine, dialog(&t).tex.as_ref().unwrap().kind.clone()), (ExportEngine::Latex, kind));
        click(&mut t, "cancel");
        assert!(t.app.modal.is_none(), "the dialog is closed");
        std::thread::sleep(Duration::from_millis(500));
        assert!(!marker.exists(), "the compile was killed, not waited for");
        let _ = std::fs::remove_file(marker.with_extension("started"));
        assert!(!t.dir.join("exports/Plan.pdf").exists());
    }
}

// spec: TEX-016
#[test]
fn a_note_that_names_its_template_is_exported_through_it_whatever_was_chosen_last() {
    let mut t = T::new("dlg-noted", usable());
    put_set(&t);
    open_on(&mut t, "Slides.md", "---\nlatex-template: tuda/beamer\nauthor: Ada\n---\n# Slides\n");
    let d = dialog(&t);
    assert_eq!(d.engine, ExportEngine::Latex, "the note's own template picks the engine");
    let tex = d.tex.as_ref().unwrap();
    assert_eq!((tex.current_set().unwrap().id.as_str(), tex.kind.as_str()), ("tuda", "beamer"));
    assert!(tex.template_warning.is_none() && !has(&t, "template-warning"), "a template that is there needs no warning");
}

// spec: TEX-033
#[test]
fn a_template_the_vault_does_not_have_is_said_in_the_dialog_with_what_is_used_instead() {
    let mut t = T::new("dlg-unknown", usable());
    put_set(&t);
    // a set the vault does not have: the first set, its publication kind, and the dialog says so
    open_on(&mut t, "Other.md", "---\nlatex-template: nosuch/thing\n---\n# Other\n");
    let tex = dialog(&t).tex.as_ref().unwrap();
    assert_eq!((tex.current_set().unwrap().id.as_str(), tex.kind.as_str()), ("tuda", "pub"));
    let warning = tex.template_warning.clone().expect("a warning");
    let pub_name = tex.current_kind().unwrap().name.clone();
    assert_eq!(warning, format!("The note's latex-template asks for the set “nosuch”, which this vault does not have: using “{pub_name}” of “{}” instead.", tex.current_set().unwrap().meta.name));
    assert!(has(&t, "template-warning"), "it is shown");
    // a kind the set does not have
    open_on(&mut t, "Kind.md", "---\nlatex-template: tuda/nokind\n---\n# Kind\n");
    let w = dialog(&t).tex.as_ref().unwrap().template_warning.clone().unwrap();
    assert!(w.starts_with("The set “") && w.contains("has no kind “nokind”") && w.contains(&format!("using “{pub_name}”")), "{w}");
    // a value that names no kind picks no engine, and the LaTeX choice says why it is not used
    open_on(&mut t, "Bare.md", "---\nlatex-template: tuda\n---\n# Bare\n");
    assert_eq!(dialog(&t).engine, ExportEngine::Builtin);
    click(&mut t, "engine:latex");
    assert!(has(&t, "template-warning"));
    let w = dialog(&t).tex.as_ref().unwrap().template_warning.clone().unwrap();
    assert!(w.starts_with("The note's latex-template “tuda” names no kind (write it as set/kind)"), "{w}");
    // picking a kind by hand: the warning has done its work
    click(&mut t, "kind");
    click(&mut t, "kind:report");
    let tex = dialog(&t).tex.as_ref().unwrap();
    assert_eq!(tex.kind, "report");
    assert!(tex.template_warning.is_none() && !has(&t, "template-warning"));
}

// spec: TEX-016
#[test]
fn a_job_that_crashed_is_an_error_in_the_dialog_and_a_dropped_job_asks_the_compile_to_stop() {
    use super::dialog::{Job, TexExport};
    let t = T::new("dlg-crash", usable());
    put_set(&t);
    let mut tex = TexExport::new(&t.dir, "Plan".into(), "# Plan\n", None);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    tex.job = Some(Job { rx, cancel: cancel.clone() });
    assert!(tex.working() && tex.poll().is_none(), "nothing yet");
    tx.send(Err("the background job crashed: boom".to_string())).unwrap();
    assert!(tex.poll().is_none() && !tex.working());
    let failure = tex.failure.as_ref().expect("the crash is shown");
    assert_eq!(failure.headline, "the background job crashed: boom");
    // the dialog closing drops the job: LuaLaTeX is told to stop
    cancel.store(false, std::sync::atomic::Ordering::SeqCst);
    let (_tx, rx) = std::sync::mpsc::channel();
    tex.job = Some(Job { rx, cancel: cancel.clone() });
    drop(tex);
    assert!(cancel.load(std::sync::atomic::Ordering::SeqCst));
}

// spec: TEX-016
#[test]
fn save_the_tex_says_when_the_folder_cannot_be_made_and_a_note_names_its_template_as_set_slash_kind() {
    use super::dialog::{note_choice, save_tex, Failure};
    use super::writer::Document;
    let dir = std::env::temp_dir().join(format!("shale-save-tex-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("in-the-way"), "a file").unwrap();
    let failure = Failure { headline: "x".into(), problems: vec![], log_tail: String::new(), document: std::sync::Arc::new(Document { tex: "\\documentclass{x}".into(), ..Document::default() }), set_dir: dir.join("set") };
    let e = save_tex(&failure, &dir.join("in-the-way/out.pdf")).unwrap_err();
    assert!(e.starts_with("The folder") && e.contains("could not be made"), "{e}");
    let ok = save_tex(&failure, &dir.join("out.pdf")).unwrap();
    assert_eq!(ok, dir.join("out-tex"));
    assert_eq!(std::fs::read_to_string(ok.join("main.tex")).unwrap(), "\\documentclass{x}");
    let _ = std::fs::remove_dir_all(&dir);
    let props = |text: &str| crate::markdown::front_matter_values(text);
    assert_eq!(note_choice(&props("---\nlatex-template: tuda / thesis\n---\n")), Some(("tuda".to_string(), "thesis".to_string())));
    assert_eq!(note_choice(&props("---\nlatex-template: tuda\n---\n")), None, "a set with no kind is no choice");
    assert_eq!(note_choice(&props("---\nauthor: x\n---\n")), None);
}
