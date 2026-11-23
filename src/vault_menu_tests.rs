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

    /// A line that starts with `text` (a hit of the quick switcher has its detail after the title).
    fn painted_start(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t.starts_with(text))
    }

    /// Click the last text on the screen that is `text`: the button, not the title of the same name.
    fn click_last(&mut self, text: &str) {
        let p = self.hl.text.iter().rev().find(|(t, _)| t.trim() == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("“{text}” is not on the screen: {:?}", self.all_text()));
        self.hl.click(&mut self.app, p + egui::vec2(12.0, 8.0), PointerButton::Primary);
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
        // the folders are looked up on a worker: wait for the answers
        self.settle();
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

// spec: VAULT-030, VAULT-044
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
    // the last lines are Open…, Create… and Close current vault (VAULT-044)
    w.open_dropdown();
    w.key(Key::ArrowUp);
    w.key(Key::ArrowUp);
    w.key(Key::Enter);
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Folder(p) if p.purpose == Purpose::Create)), "Up twice from the first line wraps to Create…");
}

// spec: VAULT-031, VAULT-032
#[test]
fn vault_032_a_right_click_renames_a_vault_and_the_name_shows_in_the_window() {
    let mut w = W::new("rename");
    w.open_dropdown();
    w.click("two", PointerButton::Secondary);
    assert!(w.painted("Rename") && w.painted("Close"), "the menu of the vault: {:?}", w.all_text());
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
    w.settle();
    assert!(!w.app.vault_load.is_loading() && !w.dir("two").exists(), "the load fails and does not make the folder");
    assert_eq!(w.root(), Some(w.dir("one")), "the open vault stays");
    assert!(w.toasted("its folder is missing"), "{:?}", w.app.toasts);
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
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Menu { .. })), "the list is shown again");
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

// spec: VAULT-038
#[test]
fn vault_038_the_folders_are_looked_up_once_on_a_worker_and_not_in_every_frame() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    let mut w = W::new("presence");
    let (calls, threads) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
    let (c, t) = (calls.clone(), threads.clone());
    w.app.presence.with_checker(move |p| {
        c.fetch_add(1, Ordering::SeqCst);
        t.lock().unwrap().push(std::thread::current().id());
        p.is_dir()
    });
    // the start screen: no vault is open
    w.app.presence.clear();
    w.app.vault = None;
    w.hl.frames(&mut w.app, 3);
    w.settle();
    w.hl.frames(&mut w.app, 30);
    assert_eq!(calls.load(Ordering::SeqCst), 2, "one look-up for each of the two known vaults, however many frames");
    assert!(threads.lock().unwrap().iter().all(|id| *id != std::thread::current().id()), "not on the UI thread");
    assert!(w.painted("two"), "{:?}", w.all_text());
    // the dropdown looks again when it is opened, and once
    w.app.open_vault(&w.dir("one"));
    w.settle();
    let before = calls.load(Ordering::SeqCst);
    w.open_dropdown();
    w.hl.frames(&mut w.app, 30);
    assert_eq!(calls.load(Ordering::SeqCst) - before, 2, "again when the list opens, not in every frame");
}

// spec: VAULT-038
#[test]
fn vault_038_a_folder_that_has_not_been_looked_up_yet_is_checking_and_cannot_be_opened() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    let mut w = W::new("checking");
    let gate = Arc::new(AtomicBool::new(false));
    let g = gate.clone();
    w.app.presence.with_checker(move |p| {
        while !g.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        p.is_dir()
    });
    let label = "▣  one  ▾";
    w.click_text(label);
    w.hl.frames(&mut w.app, 3);
    assert!(w.painted("checking…"), "{:?}", w.all_text());
    w.click_text("two");
    assert!(w.app.modal.is_some() && !w.app.vault_load.is_loading(), "a click waits for the answer");
    gate.store(true, Ordering::SeqCst);
    w.settle();
    assert!(!w.painted("checking…"));
    w.click_text("two");
    w.settle();
    assert_eq!(w.root(), Some(w.dir("two")));
}

/// What Close leaves in a vault folder to look at: Shale's folders and the repository.
fn shale_files(dir: &std::path::Path) {
    for f in [".shale/state.json", ".trash/2026-11-23/Old.md", ".git/HEAD"] {
        let p = dir.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
}

// spec: VAULT-044, VAULT-036
#[test]
fn vault_044_close_current_vault_asks_first_and_closes_with_the_list_open_on_the_start_screen() {
    let mut w = W::new("close-current");
    shale_files(&w.dir("one"));
    w.open_dropdown();
    assert!(w.painted("Close current vault"), "{:?}", w.all_text());
    w.click_text("Close current vault");
    assert!(w.painted("Close vault") && w.painted("Cancel"), "the question: {:?}", w.all_text());
    assert_eq!(w.root(), Some(w.dir("one")), "nothing happened yet");
    // Esc cancels
    w.key(Key::Escape);
    assert!(w.app.modal.is_none() && w.root().is_some());
    w.open_dropdown();
    w.click_text("Close current vault");
    w.click_last("Close vault");
    w.settle();
    assert_eq!(w.root(), None, "the vault is closed");
    assert_eq!(w.names(), ["two"], "and forgotten");
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Menu { .. })), "the list is open");
    assert!(w.painted("Open a folder as vault"), "the start dialog is shown: {:?}", w.all_text());
    assert!(w.dir("one").join("Welcome.md").is_file() && w.dir("one").join(".shale/state.json").is_file() && w.dir("one").join(".trash/2026-11-23/Old.md").is_file(), "by default nothing is removed");
    assert!(!w.painted("Close current vault"), "no vault is open to close");
}

// spec: VAULT-044, VAULT-035
#[test]
fn vault_044_a_right_click_close_asks_which_folders_to_remove_and_removes_only_those() {
    let mut w = W::new("close-right-click");
    shale_files(&w.dir("two"));
    w.open_dropdown();
    w.click("two", PointerButton::Secondary);
    w.click_text("Close");
    assert!(w.painted("Close vault") && w.painted("Also remove .shale/ (this vault's settings and state)") && w.painted("Also remove .trash/"), "{:?}", w.all_text());
    assert!(!w.painted(".trash/ holds the notes you deleted from this vault. Removing it deletes them for good."), "the warning comes with the box");
    w.click_text("Also remove .trash/");
    assert!(w.painted(".trash/ holds the notes you deleted from this vault. Removing it deletes them for good."), "{:?}", w.all_text());
    w.click_last("Close vault");
    w.settle();
    assert_eq!(w.names(), ["one"], "forgotten");
    assert_eq!(w.root(), Some(w.dir("one")), "the open vault stays open");
    assert!(w.dir("two").join(".shale/state.json").is_file(), ".shale/ was not ticked");
    assert!(!w.dir("two").join(".trash").exists(), ".trash/ was ticked");
    assert!(w.dir("two").join(".git/HEAD").is_file() && w.dir("two").join("Welcome.md").is_file(), "the repository and the notes are never touched");
}

// spec: VAULT-035
#[test]
fn vault_035_cancel_and_esc_keep_the_vault_in_the_list_and_remove_nothing() {
    let mut w = W::new("close-cancel");
    shale_files(&w.dir("two"));
    w.open_dropdown();
    w.click("two", PointerButton::Secondary);
    w.click_text("Close");
    w.click_text("Also remove .shale/ (this vault's settings and state)");
    w.click_text("Cancel");
    assert!(w.app.modal.is_none());
    assert_eq!(w.names(), ["one", "two"]);
    assert!(w.dir("two").join(".shale/state.json").is_file());
    // Enter in the question closes, with the boxes as they are
    w.open_dropdown();
    w.click("two", PointerButton::Secondary);
    w.click_text("Close");
    w.key(Key::Enter);
    w.settle();
    assert_eq!(w.names(), ["one"]);
    assert!(w.dir("two").join(".shale/state.json").is_file() && w.dir("two").join(".trash").is_dir(), "both boxes were off");
}

// spec: VAULT-044
#[test]
fn vault_044_the_quick_switcher_has_a_switch_vault_command_that_opens_the_list() {
    let mut w = W::new("switch-command");
    w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
    w.hl.frames(&mut w.app, 2);
    w.hl.frame(&mut w.app, vec![Event::Text("switch vau".to_string())]);
    w.hl.frames(&mut w.app, 2);
    assert!(w.painted_start("Switch vault"), "the command is listed: {:?}", w.all_text());
    w.key(Key::Enter);
    w.settle();
    assert!(matches!(&w.app.modal, Some(Modal::Vaults(m)) if matches!(&**m, VaultModal::Menu { .. })), "the same list as the label's");
    assert!(w.painted("Close current vault") && w.painted("two"));
    // no command without typing, and none for a text that is not part of its name
    for typed in ["", "zzz"] {
        w.key(Key::Escape);
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        w.hl.frames(&mut w.app, 2);
        if !typed.is_empty() {
            w.hl.frame(&mut w.app, vec![Event::Text(typed.to_string())]);
            w.hl.frames(&mut w.app, 2);
        }
        assert!(!w.painted_start("Switch vault"), "{typed:?}: {:?}", w.all_text());
    }
}
