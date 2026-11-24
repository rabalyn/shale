//! The start dialog (the window with no vault, VAULT-021 to VAULT-023), driven through the headless
//! harness with real pointer and keyboard events: `ShaleApp::build` without a vault shows it,
//! `Headless` finds the painted texts, and the clicks go where the texts are.

use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct W {
    app: ShaleApp,
    hl: Headless,
    /// a folder of its own for this test
    dir: PathBuf,
}

impl W {
    fn new() -> W {
        let dir = std::env::temp_dir().join(format!("shale-startdlg-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // as the app stores a vault's folder (VAULT-029): macOS's /var is /private/var, Windows has 8.3 names
        let dir = crate::known_vaults::canonical(&dir);
        let app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 3);
        w
    }

    fn painted(&self, text: &str) -> Option<Pos2> {
        self.hl.text.iter().find(|(t, _)| t == text).map(|(_, p)| *p)
    }

    fn click_text(&mut self, text: &str) {
        let p = self.painted(text).unwrap_or_else(|| panic!("“{text}” is not on the screen; there is: {:?}", self.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>()));
        self.hl.click(&mut self.app, p + eframe::egui::vec2(12.0, 8.0), PointerButton::Primary);
    }

    /// Click into the path field (below its explanation), select all and type `path`.
    fn type_path(&mut self, path: &str) {
        let hint = self.painted("Any folder works, including an existing markdown vault. Type its path:").expect("the explanation of the field");
        let field = hint + eframe::egui::vec2(40.0, 34.0);
        self.hl.click(&mut self.app, field, PointerButton::Primary);
        self.hl.frame(&mut self.app, vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        self.hl.frame(&mut self.app, vec![Event::Text(path.to_string())]);
        self.hl.frames(&mut self.app, 2);
        assert_eq!(self.app.welcome_path, path, "the typed text is in the field");
    }

    fn settle(&mut self) {
        self.hl.frames(&mut self.app, 6);
        // a vault is read in the background (NET-019)
        self.hl.settle(&mut self.app);
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: VAULT-021
#[test]
fn without_a_vault_the_start_dialog_offers_open_and_create() {
    let w = W::new();
    assert!(w.app.vault.is_none());
    for t in ["Shale", "Open a folder as vault", "Open folder", "Create vault with sample notes", "Create empty vault"] {
        assert!(w.painted(t).is_some(), "“{t}” is on the start dialog");
    }
    assert!(w.painted("Recent vaults").is_none(), "no recent vaults, no heading");
}

// spec: VAULT-021
#[test]
fn a_typed_folder_is_created_with_sample_notes_and_opened() {
    let mut w = W::new();
    let vault = w.dir.join("my vault");
    w.type_path(&vault.to_string_lossy());
    w.click_text("Create vault with sample notes");
    w.settle();
    assert!(vault.join("Welcome.md").is_file(), "the sample notes were written");
    assert_eq!(w.app.vault.as_ref().map(|v| v.root.clone()), Some(vault.clone()), "the vault is open");
    assert!(w.painted("Open a folder as vault").is_none(), "the start dialog is gone");
    assert_eq!(w.app.cfg.vaults.entries().first().map(|e| e.path.clone()), Some(vault.clone()), "and it is the first of the known vaults");
}

// spec: VAULT-021
#[test]
fn an_empty_vault_has_no_notes() {
    let mut w = W::new();
    let vault = w.dir.join("empty");
    w.type_path(&vault.to_string_lossy());
    w.click_text("Create empty vault");
    w.settle();
    assert!(vault.is_dir() && std::fs::read_dir(&vault).unwrap().flatten().all(|e| e.file_name().to_string_lossy().starts_with('.')), "nothing but Shale's own hidden folder");
    assert_eq!(w.app.vault.as_ref().map(|v| v.root.clone()), Some(vault));
}

// spec: VAULT-021
#[test]
fn open_folder_opens_an_existing_folder_and_refuses_one_that_does_not_exist() {
    let mut w = W::new();
    let missing = w.dir.join("not yet");
    w.type_path(&missing.to_string_lossy());
    w.click_text("Open folder");
    assert!(w.app.vault.is_none() && !missing.exists(), "nothing is created or opened");
    assert!(w.hl.text.iter().any(|(t, _)| t.contains("That folder doesn't exist yet")), "the toast says so and points to “Create vault”");
    // an existing folder with a note in it
    let there = w.dir.join("there");
    std::fs::create_dir_all(&there).unwrap();
    std::fs::write(there.join("Mine.md"), "# Mine\n").unwrap();
    w.type_path(&there.to_string_lossy());
    w.click_text("Open folder");
    w.settle();
    let v = w.app.vault.as_ref().expect("the folder is open as vault");
    assert_eq!(v.root, there);
    assert!(v.files.iter().any(|f| f.rel == std::path::Path::new("Mine.md")), "its notes are listed");
}

// spec: VAULT-021
#[test]
fn a_vault_cannot_be_created_where_a_file_is() {
    let mut w = W::new();
    let file = w.dir.join("in the way");
    std::fs::write(&file, "x").unwrap();
    w.type_path(&file.join("vault").to_string_lossy());
    w.click_text("Create empty vault");
    w.settle();
    assert!(w.app.vault.is_none());
    assert!(w.hl.text.iter().any(|(t, _)| t.contains("Could not create the vault")), "the toast gives the reason");
}

// spec: VAULT-022, VAULT-006
#[test]
fn recent_vaults_are_listed_and_one_click_opens_them() {
    let mut w = W::new();
    let (a, b) = (w.dir.join("alpha"), w.dir.join("beta"));
    for d in [&a, &b] {
        std::fs::create_dir_all(d).unwrap();
    }
    w.app.cfg.vaults.add(&b);
    w.app.cfg.vaults.add(&a);
    w.settle();
    assert!(w.painted("Recent vaults").is_some());
    assert!(w.painted("alpha").is_some() && w.painted("beta").is_some(), "the names are listed, not the paths");
    assert!(w.painted(&a.to_string_lossy()).is_none());
    w.click_text("beta");
    w.settle();
    let root = w.app.vault.as_ref().map(|v| v.root.clone());
    assert_eq!(root, Some(b.clone()));
    let paths: Vec<_> = w.app.cfg.vaults.entries().iter().map(|e| e.path.clone()).collect();
    assert_eq!(paths, [crate::known_vaults::canonical(&b), crate::known_vaults::canonical(&a)], "the opened one moves to the top, without a duplicate");
}

// spec: VAULT-031
#[test]
fn vault_031_the_start_dialog_shows_the_parent_folder_of_vaults_with_the_same_name() {
    let mut w = W::new();
    let (a, b) = (w.dir.join("x").join("notes"), w.dir.join("y").join("notes"));
    for d in [&a, &b] {
        std::fs::create_dir_all(d).unwrap();
        w.app.cfg.vaults.add(d);
    }
    w.settle();
    let parent = |p: &PathBuf| crate::known_vaults::canonical(p).parent().unwrap().to_string_lossy().into_owned();
    assert!(w.painted(&format!("notes ({})", parent(&a))).is_some() && w.painted(&format!("notes ({})", parent(&b))).is_some());
}

// spec: VAULT-038
#[test]
fn vault_038_the_start_dialog_greys_out_a_vault_whose_folder_is_missing_and_does_not_create_it() {
    let mut w = W::new();
    let gone = w.dir.join("gone");
    w.app.cfg.vaults.add(&gone);
    w.settle();
    assert!(w.painted("gone (folder missing)").is_some(), "{:?}", w.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>());
    w.click_text("gone (folder missing)");
    w.settle();
    assert!(w.app.vault.is_none() && !gone.exists(), "a click does not make the folder");
}
