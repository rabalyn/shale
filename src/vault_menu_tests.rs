//! The vault dropdown (VAULT-030, VAULT-032 to VAULT-034, VAULT-038, VAULT-039), driven through the
//! headless window with real pointer and keyboard events.

use crate::app::{Modal, Options, ShaleApp};
use crate::known_vaults::canonical;
use crate::ui_test_support::Headless;
use crate::vault_menu::{FolderPicker, Purpose, VaultModal};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton};
use std::path::PathBuf;

struct W {
    app: ShaleApp,
    hl: Headless,
    base: PathBuf,
}

impl W {
    /// A window with the vault `one` open and `two` known; `three` is a folder that is not a vault yet.
    fn new(tag: &str) -> W {
        let base = canonical(&std::env::temp_dir()).join(format!("shale-vault-menu-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for name in ["one", "two", "three"] {
            std::fs::create_dir_all(base.join(name)).unwrap();
            std::fs::write(base.join(name).join("Welcome.md"), format!("# {name}\n")).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        app.open_vault(&base.join("two"));
        app.open_vault(&base.join("one"));
        let mut w = W { app, hl: Headless::new(), base };
        w.settle();
        w
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    fn settle(&mut self) {
        self.hl.frames(&mut self.app, 4);
        self.hl.settle(&mut self.app);
    }

    fn painted(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t.trim() == text)
    }

    fn all_text(&self) -> Vec<String> {
        self.hl.text.iter().map(|t| t.0.clone()).collect()
    }

    fn click(&mut self, text: &str, button: PointerButton) {
        let p = self.hl.text.iter().find(|(t, _)| t.trim() == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("“{text}” is not on the screen: {:?}", self.all_text()));
        self.hl.click(&mut self.app, p + egui::vec2(12.0, 8.0), button);
    }

    fn click_text(&mut self, text: &str) {
        self.click(text, PointerButton::Primary);
    }

    fn key(&mut self, key: Key) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.hl.frames(&mut self.app, 2);
    }

    /// Replace the text of the field that has the focus.
    fn type_over(&mut self, text: &str) {
        self.hl.frame(&mut self.app, vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        if text.is_empty() {
            self.key(Key::Backspace);
        } else {
            self.hl.frame(&mut self.app, vec![Event::Text(text.to_string())]);
        }
        self.hl.frames(&mut self.app, 2);
    }

    fn root(&self) -> Option<PathBuf> {
        self.app.vault.as_ref().map(|v| v.root.clone())
    }

    fn open_dropdown(&mut self) {
        let label = self.hl.text.iter().find(|(t, _)| t.starts_with("▣  ") && t.ends_with("  ▾")).map(|(t, _)| t.clone()).expect("the vault label");
        self.click_text(&label);
        assert!(matches!(self.app.modal, Some(Modal::Vaults(_))), "the label opens the dropdown");
    }

    fn names(&self) -> Vec<String> {
        self.app.cfg.vaults.entries().iter().map(|e| e.name.clone()).collect()
    }

    fn toasted(&self, part: &str) -> bool {
        self.app.toasts.iter().any(|(m, _)| m.contains(part))
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// spec: VAULT-030
#[test]
fn vault_030_the_label_opens_a_list_of_the_known_vaults_with_the_open_one_marked() {
    let mut w = W::new("list");
    assert!(w.painted("▣  one  ▾"), "the label is at the bottom left: {:?}", w.all_text());
    w.open_dropdown();
    assert!(w.painted("✓ one") && w.painted("two"), "{:?}", w.all_text());
    assert!(!w.painted("✓ two"), "only the open vault is marked");
    assert!(w.painted("Open…") && w.painted("Create…"));
    // the open vault is listed first: it was opened last
    let at = |w: &W, t: &str| w.hl.text.iter().find(|(x, _)| x.trim() == t).map(|(_, p)| p.y).unwrap();
    assert!(at(&w, "✓ one") < at(&w, "two") && at(&w, "two") < at(&w, "Open…") && at(&w, "Open…") < at(&w, "Create…"));
}

// spec: VAULT-030
#[test]
fn vault_030_a_click_on_a_vault_opens_it_and_a_click_on_the_open_one_does_nothing() {
    let mut w = W::new("click");
    w.open_dropdown();
    w.click_text("✓ one");
    assert!(w.app.modal.is_none() && !w.app.vault_load.is_loading(), "the open vault stays as it is");
    assert_eq!(w.root(), Some(w.dir("one")));
    w.open_dropdown();
    w.click_text("two");
    assert!(w.app.modal.is_none());
    w.settle();
    assert_eq!(w.root(), Some(w.dir("two")));
    assert_eq!(w.names(), ["two", "one"], "the vault opened last is first");
}

// spec: VAULT-030
#[test]
fn vault_030_it_works_by_keyboard_arrows_enter_and_esc() {
    let mut w = W::new("keys");
    w.open_dropdown();
    w.key(Key::ArrowDown);
    w.key(Key::Enter);
    assert!(w.app.modal.is_none(), "Enter picks the selected line");
    w.settle();
    assert_eq!(w.root(), Some(w.dir("two")), "the arrow moved from the open vault to the next one");
    w.open_dropdown();
    w.key(Key::Escape);
    assert!(w.app.modal.is_none(), "Esc closes the list");
    assert_eq!(w.root(), Some(w.dir("two")));
    // the last lines are Open… and Create…
    w.open_dropdown();
    w.key(Key::ArrowUp);
    w.key(Key::Enter);
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Folder(p) if p.purpose == Purpose::Create)), "Up from the first line wraps to Create…");
}

// spec: VAULT-031, VAULT-032
#[test]
fn vault_032_a_right_click_renames_a_vault_and_the_name_shows_in_the_window() {
    let mut w = W::new("rename");
    w.open_dropdown();
    w.click("two", PointerButton::Secondary);
    assert!(w.painted("Rename") && !w.painted("Close"), "the menu of the vault: {:?}", w.all_text());
    w.click_text("Rename");
    assert!(w.painted("Rename vault"));
    w.type_over("Work");
    w.key(Key::Enter);
    assert!(w.app.modal.is_none(), "Enter renames");
    assert_eq!(w.names(), ["one", "Work"]);
    // the open vault's name is the one on the label
    w.app.cfg.vaults.rename(&w.dir("one"), "Main").unwrap();
    w.hl.frames(&mut w.app, 2);
    assert!(w.painted("▣  Main  ▾"), "{:?}", w.all_text());
}

// spec: VAULT-032
#[test]
fn vault_032_an_empty_name_is_refused_with_its_reason_and_the_dialog_stays() {
    let mut w = W::new("rename-empty");
    w.open_dropdown();
    w.click("two", PointerButton::Secondary);
    w.click_text("Rename");
    w.type_over("");
    w.key(Key::Enter);
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Rename { error: Some(_), .. })), "the dialog stays");
    assert!(w.painted("The name cannot be empty."), "{:?}", w.all_text());
    assert_eq!(w.names(), ["one", "two"], "nothing was renamed");
    // Esc cancels
    w.key(Key::Escape);
    assert!(w.app.modal.is_none());
    assert_eq!(w.names(), ["one", "two"]);
}

// spec: VAULT-033
#[test]
fn vault_033_open_asks_for_a_folder_adds_it_to_the_known_vaults_and_opens_it() {
    let mut w = W::new("open");
    w.open_dropdown();
    w.click_text("Open…");
    assert!(w.painted("Open a folder as vault"), "{:?}", w.all_text());
    std::fs::create_dir_all(w.dir("blank")).unwrap();
    w.type_over(&w.dir("blank").to_string_lossy());
    w.key(Key::Enter);
    w.settle();
    assert_eq!(w.root(), Some(w.dir("blank")), "Enter chooses the folder in the field");
    assert!(w.app.modal.is_none());
    assert_eq!(w.app.cfg.vaults.entries()[0].path, w.dir("blank"));
    assert_eq!(only_written(&w.dir("blank")), Vec::<String>::new(), "no marker, no settings, no state");
    // a folder that is not there is refused
    w.open_dropdown();
    w.click_text("Open…");
    w.type_over(&w.dir("nowhere").to_string_lossy());
    w.key(Key::Enter);
    assert!(w.painted("That folder does not exist."));
    assert!(!w.dir("nowhere").exists(), "nothing is created");
}

// spec: VAULT-033
#[test]
fn vault_033_the_picker_lists_the_folders_and_goes_into_and_up_by_click() {
    let mut w = W::new("picker");
    std::fs::create_dir_all(w.dir("one").join("Sub")).unwrap();
    std::fs::create_dir_all(w.dir("one").join(".hidden")).unwrap();
    let mut p = FolderPicker::new(Purpose::Open);
    p.go(w.dir("one"));
    assert_eq!(p.subfolders(), ["Sub"], "hidden folders are not listed");
    w.app.modal = Some(Modal::Vaults(Box::new(VaultModal::Folder(Box::new(p)))));
    w.hl.frames(&mut w.app, 2);
    w.click_text("📁  Sub");
    let text = |w: &W| match &w.app.modal {
        Some(Modal::Vaults(m)) => match &**m {
            VaultModal::Folder(p) => p.text.clone(),
            _ => panic!("the picker"),
        },
        _ => panic!("the picker"),
    };
    assert_eq!(text(&w), w.dir("one").join("Sub").to_string_lossy());
    w.click_text("↑  Up");
    assert_eq!(text(&w), w.dir("one").to_string_lossy());
}

// spec: VAULT-034
#[test]
fn vault_034_create_refuses_a_folder_with_content_and_offers_to_open_it() {
    let mut w = W::new("create-full");
    let mut p = FolderPicker::new(Purpose::Create);
    p.text = w.dir("three").to_string_lossy().into_owned();
    assert!(!w.app.choose_folder(&mut p), "Welcome.md is content");
    assert!(p.error.as_deref().is_some_and(|e| e.contains("has content")), "{:?}", p.error);
    assert!(w.app.actions.is_empty(), "nothing opens");
    // through the dialog: the offer is a button
    w.app.modal = Some(Modal::Vaults(Box::new(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Create))))));
    w.hl.frames(&mut w.app, 2);
    w.type_over(&w.dir("three").to_string_lossy());
    w.key(Key::Enter);
    assert!(w.painted("Open it as a vault instead"), "{:?}", w.all_text());
    w.click_text("Open it as a vault instead");
    w.settle();
    assert_eq!(w.root(), Some(w.dir("three")));
}

// spec: VAULT-034
#[test]
fn vault_034_hidden_files_do_not_count_and_the_sample_box_writes_sample_notes() {
    let mut w = W::new("create-ok");
    std::fs::create_dir_all(w.dir("empty")).unwrap();
    std::fs::write(w.dir("empty").join(".DS_Store"), "x").unwrap();
    assert!(crate::vault_menu::is_empty_folder(&w.dir("empty")).unwrap());
    assert!(!crate::vault_menu::is_empty_folder(&w.dir("three")).unwrap());
    w.app.modal = Some(Modal::Vaults(Box::new(VaultModal::Folder(Box::new(FolderPicker::new(Purpose::Create))))));
    w.hl.frames(&mut w.app, 2);
    assert!(w.painted("Start with sample notes"));
    w.click_text("Start with sample notes");
    w.type_over(&w.dir("empty").to_string_lossy());
    w.key(Key::Enter);
    w.settle();
    assert_eq!(w.root(), Some(w.dir("empty")));
    assert!(w.dir("empty").join("Welcome.md").is_file(), "the sample notes are there");
    assert_eq!(w.app.cfg.vaults.entries()[0].path, w.dir("empty"));
}

// spec: VAULT-034
#[test]
fn vault_034_a_folder_that_does_not_exist_is_made_and_starts_empty_without_the_box() {
    let mut w = W::new("create-new");
    let mut p = FolderPicker::new(Purpose::Create);
    p.text = w.dir("fresh").join("deep").to_string_lossy().into_owned();
    assert!(w.app.choose_folder(&mut p));
    assert!(w.dir("fresh").join("deep").is_dir());
    assert_eq!(std::fs::read_dir(w.dir("fresh").join("deep")).unwrap().count(), 0, "no sample notes without the box");
    let file = w.dir("one").join("Welcome.md");
    p.text = file.to_string_lossy().into_owned();
    assert!(!w.app.choose_folder(&mut p) && p.error.is_some(), "a file is not a folder");
}

// spec: VAULT-038
#[test]
fn vault_038_a_vault_whose_folder_is_missing_is_greyed_out_and_is_never_created() {
    let mut w = W::new("missing");
    std::fs::remove_dir_all(w.dir("two")).unwrap();
    w.open_dropdown();
    assert!(w.painted("folder missing") && w.painted("Locate…") && w.painted("Forget"), "{:?}", w.all_text());
    w.click_text("two");
    assert!(w.app.modal.is_some(), "a click on it does nothing");
    assert!(!w.dir("two").exists(), "the folder is not made");
    // nor by another way in
    w.app.open_vault_in_background(&w.dir("two"), crate::app::vault_load::After::Nothing);
    assert!(!w.app.vault_load.is_loading() && !w.dir("two").exists());
    assert!(w.toasted("is missing"), "{:?}", w.app.toasts);
    // Rename works as for any entry
    w.click("two", PointerButton::Secondary);
    assert!(w.painted("Rename"));
}

// spec: VAULT-038
#[test]
fn vault_038_forget_removes_the_entry_and_leaves_the_folder() {
    let mut w = W::new("forget");
    std::fs::rename(w.dir("two"), w.dir("moved")).unwrap();
    w.open_dropdown();
    w.click_text("Forget");
    assert_eq!(w.names(), ["one"]);
    assert!(w.dir("moved").is_dir(), "no folder was touched");
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Menu { missing, .. } if missing.is_empty())), "the list is shown again");
    assert!(!w.painted("Forget"));
}

// spec: VAULT-038
#[test]
fn vault_038_locate_points_the_entry_at_the_new_folder_and_keeps_its_name() {
    let mut w = W::new("locate");
    w.app.cfg.vaults.rename(&w.dir("two"), "Garden").unwrap();
    std::fs::rename(w.dir("two"), w.dir("moved")).unwrap();
    w.open_dropdown();
    w.click_text("Locate…");
    assert!(w.painted("Locate the folder of the vault"), "{:?}", w.all_text());
    w.type_over(&w.dir("moved").to_string_lossy());
    w.key(Key::Enter);
    assert!(w.app.modal.is_none());
    let e = &w.app.cfg.vaults.entries()[1];
    assert_eq!((e.name.as_str(), &e.path), ("Garden", &w.dir("moved")), "{:?}", w.app.cfg.vaults);
    assert_eq!(w.root(), Some(w.dir("one")), "Locate does not open it");
    // a folder that is not there is refused, and one that is known already
    let mut p = FolderPicker::new(Purpose::Locate(w.dir("moved")));
    p.text = w.dir("nowhere").to_string_lossy().into_owned();
    assert!(!w.app.choose_folder(&mut p));
    p.text = w.dir("one").to_string_lossy().into_owned();
    assert!(!w.app.choose_folder(&mut p) && p.error.as_deref().is_some_and(|e| e.contains("already known")), "{:?}", p.error);
}

// spec: VAULT-039
#[test]
fn vault_039_a_folder_inside_or_around_a_known_vault_is_allowed_and_warned_once() {
    let mut w = W::new("nested");
    let inner = w.dir("one").join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    let mut p = FolderPicker::new(Purpose::Open);
    p.text = inner.to_string_lossy().into_owned();
    assert!(w.app.choose_folder(&mut p), "it is allowed");
    w.app.actions.clear();
    w.app.open_vault_in_background(&inner, crate::app::vault_load::After::Nothing);
    w.settle();
    assert_eq!(w.root(), Some(inner.clone()));
    assert!(w.toasted("lies inside the vault “one”"), "{:?}", w.app.toasts);
    // opened again, it is known: no second warning
    w.app.toasts.clear();
    w.app.open_vault_in_background(&w.dir("one"), crate::app::vault_load::After::Nothing);
    w.settle();
    w.app.open_vault_in_background(&inner, crate::app::vault_load::After::Nothing);
    w.settle();
    assert!(!w.toasted("inside") && !w.toasted("contains"), "{:?}", w.app.toasts);
    // a folder around known vaults
    w.app.open_vault_in_background(&w.base, crate::app::vault_load::After::Nothing);
    w.settle();
    assert!(w.toasted("contains the vault"), "{:?}", w.app.toasts);
}

// spec: VAULT-039
#[test]
fn vault_039_locate_into_a_nested_folder_warns_too() {
    let mut w = W::new("nested-locate");
    let inner = w.dir("one").join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::remove_dir_all(w.dir("two")).unwrap();
    let mut p = FolderPicker::new(Purpose::Locate(w.dir("two")));
    p.text = inner.to_string_lossy().into_owned();
    assert!(w.app.choose_folder(&mut p));
    assert!(w.toasted("lies inside the vault “one”"), "{:?}", w.app.toasts);
}

// spec: VAULT-033
#[test]
fn vault_033_a_vault_with_notes_is_not_written_to_either_apart_from_the_learned_tag_words() {
    let mut w = W::new("writes");
    w.app.open_vault_in_background(&w.dir("three"), crate::app::vault_load::After::Nothing);
    w.settle();
    assert_eq!(w.root(), Some(w.dir("three")));
    assert_eq!(only_written(&w.dir("three")), Vec::<String>::new());
}

/// What the app wrote into `dir` (not the notes the test made), apart from the cache of the tag
/// learner (TAG-003): the names in `.shale/`, and a `.shale` that has other things than that.
fn only_written(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != "Welcome.md").collect();
    if let Ok(d) = std::fs::read_dir(dir.join(".shale")) {
        names.retain(|n| n != ".shale");
        names.extend(d.flatten().map(|e| format!(".shale/{}", e.file_name().to_string_lossy())).filter(|n| n != ".shale/tag-words.json"));
    }
    names
}
