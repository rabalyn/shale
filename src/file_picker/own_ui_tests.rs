//! Shale's own picker (PICK-004 to PICK-009), driven through the headless window with real
//! pointer and keyboard events.

use super::places::Group;
use super::{Filter, Mode, Picked, Request};
use crate::app::{Options, ShaleApp};
use crate::ui_test_support::{poll_until, Headless};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton};
use std::path::PathBuf;

struct W {
    app: ShaleApp,
    hl: Headless,
    base: PathBuf,
}

impl W {
    fn new(tag: &str) -> W {
        let base = crate::known_vaults::canonical(&std::env::temp_dir()).join(format!("shale-own-picker-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        app.cfg.system_file_dialog = false;
        W { app, hl: Headless::new(), base }
    }

    /// A window with the sample vault open (in `base/vault`).
    fn with_vault(tag: &str) -> W {
        let mut w = W::new(tag);
        let vault = w.base.join("vault");
        std::fs::create_dir_all(vault.join("Book")).unwrap();
        crate::vault::write_sample_vault(&vault).unwrap();
        std::fs::write(vault.join("Book").join("one.md"), "# One\n").unwrap();
        w.app = ShaleApp::build(Options { screenshot: None, vault: Some(vault), view: None, keys: None }, false);
        w.hl.settle(&mut w.app);
        w
    }

    /// The system's dialog answers `paths` from now on.
    fn system_answers(&mut self, paths: Vec<PathBuf>) {
        self.app.cfg.system_file_dialog = true;
        self.app.pick = super::Host::with_native(std::sync::Arc::new(super::Answers(paths)));
    }

    /// Click the `n`th (from the top) text on the screen that is `text`.
    fn click_nth(&mut self, text: &str, n: usize) {
        let mut found: Vec<eframe::egui::Pos2> = self.hl.text.iter().filter(|(t, _)| t.trim() == text).map(|(_, p)| *p).collect();
        found.sort_by(|a, b| a.y.total_cmp(&b.y));
        let p = *found.get(n).unwrap_or_else(|| panic!("no {n}th “{text}”: {:?}", self.all_text()));
        self.hl.click(&mut self.app, p + egui::vec2(12.0, 8.0), PointerButton::Primary);
    }

    /// Frames until the host has nothing open or running, and a few more.
    fn idle(&mut self) {
        poll_until("the file picker is idle", || {
            self.hl.frame(&mut self.app, vec![]);
            !self.app.pick.running() && !self.app.pick.confirm_open() && self.app.pick.own_picker().is_none()
        });
        self.hl.frames(&mut self.app, 4);
    }

    /// Ask for `req` and wait until Shale's own picker is there with its shortcuts looked up.
    fn open(&mut self, req: Request) {
        self.app.pick.ask("k", req);
        poll_until("the picker with its shortcuts", || {
            self.hl.frame(&mut self.app, vec![]);
            self.app.pick.own_picker().is_some_and(|p| !p.scanning())
        });
        self.hl.frames(&mut self.app, 3);
    }

    fn picker(&mut self) -> &mut super::own::OwnPicker {
        self.app.pick.own_picker().expect("the picker is open")
    }

    fn painted(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t.trim() == text)
    }

    fn all_text(&self) -> Vec<String> {
        self.hl.text.iter().map(|t| t.0.clone()).collect()
    }

    fn click(&mut self, text: &str) {
        let p = self.hl.text.iter().rev().find(|(t, _)| t.trim() == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("“{text}” is not on the screen: {:?}", self.all_text()));
        self.hl.click(&mut self.app, p + egui::vec2(12.0, 8.0), PointerButton::Primary);
    }

    fn key(&mut self, key: Key) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.hl.frames(&mut self.app, 2);
    }

    fn answer(&mut self) -> Option<Picked> {
        self.hl.frames(&mut self.app, 2);
        self.app.pick.take("k")
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// spec: PICK-006
#[test]
fn pick_006_the_shortcuts_list_the_user_folders_that_exist_and_a_click_goes_there() {
    let mut w = W::new("places");
    let docs = crate::userdirs::document_dir().unwrap();
    let downloads = crate::userdirs::download_dir().unwrap();
    std::fs::create_dir_all(&docs).unwrap();
    std::fs::create_dir_all(&downloads).unwrap();
    std::fs::create_dir_all(docs.join("Inside")).unwrap();
    w.open(Request::folder("Choose").start(w.base.clone()));
    let places: Vec<(String, PathBuf)> = w.picker().places.iter().filter(|p| p.group == Group::Places).map(|p| (p.label.clone(), p.path.clone())).collect();
    assert!(places.contains(&("Documents".into(), docs.clone())) && places.contains(&("Downloads".into(), downloads)), "{places:?}");
    assert!(places.iter().all(|(_, p)| p.is_dir()), "only folders that exist: {places:?}");
    assert!(w.painted("Documents") && w.painted("Downloads"), "{:?}", w.all_text());
    w.click("Documents");
    assert_eq!(w.picker().dir, docs);
    assert_eq!(w.picker().text, docs.to_string_lossy());
    assert!(w.painted("📁  Inside"), "the folder is listed: {:?}", w.all_text());
}

// spec: PICK-006
#[test]
fn pick_006_the_open_vault_the_known_vaults_and_the_libraries_are_shortcuts() {
    let mut w = W::new("vaults");
    for n in ["alpha", "beta", "lib"] {
        std::fs::create_dir_all(w.base.join(n)).unwrap();
    }
    w.app.open_vault(&w.base.join("alpha"));
    w.app.open_vault(&w.base.join("beta"));
    w.app.cfg.media_libraries.push(crate::media::outside::Library { name: "Videos".into(), path: w.base.join("lib") });
    w.hl.settle(&mut w.app);
    w.open(Request::folder("Choose"));
    let listed: Vec<(Group, String)> = w.picker().places.iter().filter(|p| p.group != Group::Places && p.group != Group::Drives).map(|p| (p.group, p.label.clone())).collect();
    assert_eq!(listed, [(Group::Vaults, "beta".to_string()), (Group::Vaults, "alpha".to_string()), (Group::Libraries, "@Videos".to_string())], "the open vault is first");
    w.click("@Videos");
    let want = w.base.join("lib");
    assert_eq!(w.picker().dir, want);
}

// spec: PICK-004, VAULT-033
#[test]
fn pick_004_folders_are_listed_and_a_click_goes_in_and_up_and_the_path_field_chooses_a_folder() {
    let mut w = W::new("folders");
    std::fs::create_dir_all(w.base.join("Sub")).unwrap();
    std::fs::create_dir_all(w.base.join(".hidden")).unwrap();
    std::fs::write(w.base.join("file.txt"), "x").unwrap();
    w.open(Request::folder("Choose").start(w.base.clone()));
    assert_eq!(w.picker().folders, ["Sub"], "hidden folders are not listed");
    assert!(w.picker().files.is_empty(), "a folder picker lists no files");
    w.click("📁  Sub");
    let want = w.base.join("Sub");
    assert_eq!(w.picker().dir, want);
    w.click("↑ Up");
    let want = w.base.clone();
    assert_eq!(w.picker().dir, want);
    // Enter in the path field chooses what is typed
    w.key(Key::Enter);
    assert_eq!(w.answer().unwrap().paths, [w.base.clone()]);
    assert!(w.app.pick.own_picker().is_none(), "the dialog is closed");
}

// spec: PICK-005
#[test]
fn pick_005_several_files_are_chosen_with_clicks_and_a_filter_narrows_the_list() {
    let mut w = W::new("files");
    for n in ["a.eml", "b.EML", "c.txt", "d.ics"] {
        std::fs::write(w.base.join(n), "x").unwrap();
    }
    w.open(Request::files("Add").start(w.base.clone()).filters(vec![Filter::new("All files", &[]), Filter::new("Mail", &["eml"])]));
    assert_eq!(w.picker().files, ["a.eml", "b.EML", "c.txt", "d.ics"]);
    w.picker().filter = 1;
    let dir = w.base.clone();
    w.picker().go(dir);
    assert_eq!(w.picker().files, ["a.eml", "b.EML"], "the filter keeps the mails, in any case");
    w.hl.frames(&mut w.app, 2);
    w.click("📄  a.eml");
    w.click("📄  b.EML");
    assert_eq!(w.picker().selected, ["a.eml", "b.EML"]);
    w.click("📄  a.eml");
    assert_eq!(w.picker().selected, ["b.EML"], "a second click takes a file out");
    w.click("📄  a.eml");
    w.click("Add");
    let got = w.answer().unwrap();
    assert_eq!(got.paths, [w.base.join("b.EML"), w.base.join("a.eml")]);
}

// spec: PICK-005
#[test]
fn pick_005_one_file_and_the_save_name_and_the_files_of_a_typed_path() {
    let mut w = W::new("one");
    std::fs::write(w.base.join("a.pdf"), "x").unwrap();
    w.open(Request::file("Choose").start(w.base.clone()));
    w.click("📄  a.pdf");
    w.click("Choose");
    assert_eq!(w.answer().unwrap().paths, [w.base.join("a.pdf")]);
    // a file that is typed is chosen by Enter
    w.open(Request::file("Choose").start(w.base.clone()));
    let typed = w.base.join("a.pdf").to_string_lossy().into_owned();
    w.picker().text = typed;
    w.key(Key::Enter);
    assert_eq!(w.answer().unwrap().paths, [w.base.join("a.pdf")]);
    // Save: the name is the one offered, a click on a file takes its name, a bad name is refused
    w.open(Request::save("Save", "out.pdf").start(w.base.clone()));
    assert_eq!(w.picker().req.mode, Mode::Save);
    w.picker().name = "a/b.pdf".into();
    w.click("Save");
    assert!(w.picker().error.as_deref().is_some_and(|e| e.contains("cannot contain")), "{:?}", w.picker().error);
    w.click("📄  a.pdf");
    assert_eq!(w.picker().name, "a.pdf");
    w.picker().name = "new.pdf".into();
    w.click("Save");
    assert_eq!(w.answer().unwrap().paths, [w.base.join("new.pdf")]);
}

// spec: PICK-008
#[test]
fn pick_008_new_folder_makes_the_folder_inside_the_shown_one_and_uses_it_with_enter() {
    let mut w = W::new("newfolder");
    w.open(Request::folder("Choose").start(w.base.clone()));
    assert!(w.painted("New folder"), "{:?}", w.all_text());
    w.click("Name, made in the folder shown above");
    w.hl.frame(&mut w.app, vec![Event::Text("Fresh".into())]);
    w.key(Key::Enter);
    assert!(w.base.join("Fresh").is_dir(), "the folder is made");
    assert_eq!(w.answer().unwrap().paths, [w.base.join("Fresh")], "and chosen");
}

// spec: PICK-008
#[test]
fn pick_008_refused_names_and_a_failure_show_their_reason_and_make_nothing() {
    let mut w = W::new("refused");
    w.open(Request::folder("Choose").start(w.base.clone()));
    for (typed, part) in [("", "Type a name"), ("a/b", "cannot contain"), ("..", "not a name"), ("CON", "reserved name"), ("a:b", "not allowed on Windows")] {
        w.picker().new_folder = typed.into();
        w.click("Make and use");
        assert!(w.picker().error.as_deref().is_some_and(|e| e.contains(part)), "{typed:?}: {:?}", w.picker().error);
        assert!(w.app.pick.own_picker().is_some(), "the dialog stays open");
    }
    assert_eq!(std::fs::read_dir(&w.base).unwrap().count(), 0, "nothing was made");
    // the shown folder is gone: the reason of the system is shown and nothing opens
    std::fs::remove_dir_all(&w.base).unwrap();
    w.picker().new_folder = "Late".into();
    w.click("Make and use");
    assert!(w.picker().error.as_deref().is_some_and(|e| e.starts_with("Could not make the folder “Late”")), "{:?}", w.picker().error);
    assert!(w.answer().is_none());
}

// spec: PICK-008
#[test]
fn pick_008_a_name_that_exists_offers_to_use_that_folder() {
    let mut w = W::new("exists");
    std::fs::create_dir_all(w.base.join("Taken")).unwrap();
    std::fs::write(w.base.join("Taken").join("keep.txt"), "x").unwrap();
    w.open(Request::folder("Choose").start(w.base.clone()));
    w.picker().new_folder = "Taken".into();
    w.click("Make and use");
    assert!(w.picker().error.as_deref().is_some_and(|e| e.contains("there already")), "{:?}", w.picker().error);
    assert!(w.painted("Use that folder"), "{:?}", w.all_text());
    assert!(w.answer().is_none(), "nothing is chosen until the offer is taken");
    w.click("Use that folder");
    assert_eq!(w.answer().unwrap().paths, [w.base.join("Taken")]);
    assert!(w.base.join("Taken").join("keep.txt").is_file(), "the folder is not touched");
}

// spec: PICK-008
#[test]
fn pick_008_in_a_save_dialog_the_new_folder_is_entered_and_a_file_picker_has_no_such_field() {
    let mut w = W::new("savefolder");
    w.open(Request::save("Save", "x.pdf").start(w.base.clone()));
    w.picker().new_folder = "Out".into();
    w.click("Make and use");
    let want = w.base.join("Out");
    assert_eq!(w.picker().dir, want, "the picker goes into it");
    assert!(w.base.join("Out").is_dir());
    w.key(Key::Escape);
    assert!(w.app.pick.own_picker().is_none());
    w.open(Request::files("Add").start(w.base.clone()));
    assert!(!w.painted("New folder"), "{:?}", w.all_text());
}

// spec: PICK-009, VAULT-023
#[test]
fn pick_009_escape_and_cancel_close_the_picker_without_an_answer() {
    let mut w = W::new("cancel");
    w.open(Request::folder("Choose").start(w.base.clone()));
    w.key(Key::Escape);
    assert!(w.app.pick.own_picker().is_none() && w.answer().is_none());
    w.open(Request::folder("Choose").start(w.base.clone()));
    w.click("Cancel");
    assert!(w.app.pick.own_picker().is_none() && w.answer().is_none());
}

/// A system dialog that answers one folder.
struct Answers(PathBuf);

impl super::Native for Answers {
    fn pick(&self, _: &Request) -> super::Outcome {
        super::Outcome::Picked(vec![self.0.clone()])
    }
}

// spec: PICK-007
#[test]
fn pick_007_the_new_library_row_browses_a_folder_and_fills_the_field() {
    let mut w = W::new("library");
    let lib = w.base.join("Videos");
    std::fs::create_dir_all(&lib).unwrap();
    w.app.cfg.system_file_dialog = true;
    w.app.pick = super::Host::with_native(std::sync::Arc::new(Answers(lib.clone())));
    w.app.open_settings(crate::app::SettingsTab::Import);
    w.hl.frames(&mut w.app, 3);
    w.click("Browse…");
    let shown = lib.to_string_lossy().into_owned();
    poll_until("the folder in the field", || {
        w.hl.frame(&mut w.app, vec![]);
        w.painted(&shown)
    });
}

// spec: PICK-004, VAULT-033
#[test]
fn pick_004_a_typed_path_that_is_not_there_is_kept_with_the_reason_it_was_refused() {
    let mut w = W::new("typed");
    let missing = w.base.join("nowhere").join("deep");
    w.open(Request::folder("Choose").start(missing.clone()).refused("That folder does not exist.").offer("Open it instead"));
    assert_eq!(w.picker().text, missing.to_string_lossy());
    let want = w.base.clone();
    assert_eq!(w.picker().dir, want, "the nearest folder that is there is shown");
    assert!(w.painted("That folder does not exist.") && w.painted("Open it instead"), "{:?}", w.all_text());
    w.click("Open it instead");
    let got = w.answer().unwrap();
    assert!(got.offered && got.paths == [missing]);
}

// spec: PICK-007
#[test]
fn pick_007_a_library_row_that_moves_or_goes_while_its_dialog_is_open_gets_no_other_answer() {
    use crate::media::outside::Library;
    let mut w = W::new("library-stable");
    let (a, b, c, chosen) = (w.base.join("a"), w.base.join("b"), w.base.join("c"), w.base.join("chosen"));
    for d in [&a, &b, &c, &chosen] {
        std::fs::create_dir_all(d).unwrap();
    }
    w.app.cfg.media_libraries = vec![Library { name: "First".into(), path: a.clone() }, Library { name: "Second".into(), path: b.clone() }];
    w.system_answers(vec![chosen.clone()]);
    w.app.open_settings(crate::app::SettingsTab::Import);
    w.hl.frames(&mut w.app, 3);
    // the second row asks, and the first goes: the answer is the second row's, which is the first now
    w.click_nth("Browse…", 1);
    w.app.cfg.media_libraries.remove(0);
    poll_until("the answer in the row", || {
        w.hl.frame(&mut w.app, vec![]);
        w.app.cfg.media_libraries[0].path == chosen
    });
    assert_eq!(w.app.cfg.media_libraries.len(), 1);
    // the row asks again and is removed; a new row takes its place and gets nothing
    w.click_nth("Browse…", 0);
    w.app.cfg.media_libraries.clear();
    w.app.cfg.media_libraries.push(Library { name: "Third".into(), path: c.clone() });
    w.idle();
    assert_eq!(w.app.cfg.media_libraries[0].path, c, "the answer of a row that is gone is not taken by another");
}

// spec: PICK-010
#[test]
fn pick_010_browse_in_the_pdf_the_book_and_the_word_dialog_puts_the_answer_into_the_save_path() {
    use crate::app::Modal;
    let mut w = W::with_vault("export-browse");
    let path_of = |app: &ShaleApp| match &app.modal {
        Some(Modal::Export(d)) => d.path.clone(),
        Some(Modal::ExportDocx(d)) => d.path.clone(),
        _ => panic!("no export dialog"),
    };
    for (what, ext) in [("pdf", "pdf"), ("book", "pdf"), ("word", "docx")] {
        let chosen = w.base.join(format!("elsewhere-{what}.{ext}"));
        w.system_answers(vec![chosen.clone()]);
        match what {
            "pdf" => w.app.open_export_dialog(std::path::Path::new("Welcome.md")),
            "book" => w.app.open_export_folder_dialog(std::path::Path::new("Book")),
            _ => w.app.open_docx_export_dialog(std::path::Path::new("Welcome.md")),
        }
        w.hl.frames(&mut w.app, 6);
        assert_ne!(path_of(&w.app), chosen.to_string_lossy(), "{what}");
        w.click("Browse…");
        poll_until(what, || {
            w.hl.frame(&mut w.app, vec![]);
            path_of(&w.app) == chosen.to_string_lossy()
        });
        w.app.modal = None;
    }
}

// spec: VAULT-021, VAULT-033
#[test]
fn vault_033_the_start_screen_has_a_choose_button_that_opens_the_folder_like_open_in_the_dropdown() {
    let mut w = W::new("start-choose");
    let vault = w.base.join("my-notes");
    std::fs::create_dir_all(&vault).unwrap();
    std::fs::write(vault.join("Note.md"), "# Note\n").unwrap();
    w.system_answers(vec![vault.clone()]);
    w.hl.frames(&mut w.app, 4);
    assert!(w.app.vault.is_none() && w.painted("Choose…") && w.painted("Open folder"), "{:?}", w.all_text());
    w.click("Choose…");
    poll_until("the vault is open", || {
        w.hl.frame(&mut w.app, vec![]);
        w.app.vault.is_some()
    });
    w.hl.settle(&mut w.app);
    assert_eq!(w.app.vault.as_ref().map(|v| crate::known_vaults::canonical(&v.root)), Some(vault));
}
