//! Shale's shortcuts under a modal dialog (KEY-038), driven in the whole window with real key presses.

use crate::app::{Modal, Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{Event, Key, Modifiers};
use std::path::{Path, PathBuf};

const NOTE: &str = "A.md";
const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl W {
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-modal-keys-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(NOTE), "# One\nbody\n# Two\nmore").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.date_prefix_new_notes = false;
        app.cfg.cursor_blink = false;
        app.keymap = crate::keys::Keymap::from_overrides_for(&app.cfg.keybindings, false);
        app.open(Path::new(NOTE), false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 4);
        w
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.hl.frames(&mut self.app, 3);
    }

    fn tabs(&self) -> usize {
        self.app.tabbar.tabs.len()
    }

    fn notes_on_disk(&self) -> usize {
        std::fs::read_dir(&self.dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "md")).count()
    }
}

// spec: KEY-038
#[test]
fn key_038_a_global_key_does_nothing_under_a_modal_dialog_and_works_again_after_it() {
    let mut w = W::new("global");
    w.app.open_settings(crate::settings::SettingsTab::Vault);
    w.hl.frames(&mut w.app, 3);
    let (tabs, notes) = (w.tabs(), w.notes_on_disk());
    w.press(Key::N, CTRL);
    w.press(Key::W, CTRL);
    w.press(Key::E, CTRL);
    assert_eq!((w.tabs(), w.notes_on_disk()), (tabs, notes), "no new note, no closed tab");
    assert!(matches!(w.app.modal, Some(Modal::Settings { .. })), "the dialog stays");
    // Esc closes it, then the same key works
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_none());
    w.press(Key::N, CTRL);
    assert_eq!(w.notes_on_disk(), notes + 1);
}

// spec: KEY-038
#[test]
fn key_038_a_note_key_does_nothing_under_a_modal_dialog() {
    let mut w = W::new("note");
    w.app.focus_editor();
    w.hl.frames(&mut w.app, 3);
    let text = w.app.notes.get(Path::new(NOTE)).unwrap().text.clone();
    w.app.modal = Some(Modal::DataUsage);
    w.hl.frames(&mut w.app, 3);
    w.press(Key::B, CTRL);
    w.press(Key::K, CTRL_SHIFT);
    w.press(Key::B, Modifiers::ALT);
    assert_eq!(w.app.notes.get(Path::new(NOTE)).unwrap().text, text, "the editor's bold key changed nothing");
}

// spec: KEY-038
#[test]
fn key_038_the_check_covers_every_modal_dialog() {
    use crate::app::Modal as M;
    let mut w = W::new("kinds");
    let rel = PathBuf::from(NOTE);
    let dialogs: Vec<(&str, M)> = vec![
        ("quick switcher", M::QuickSwitch { query: String::new(), sel: 0 }),
        ("rename", M::Rename { rel: rel.clone(), name: String::new(), error: None }),
        ("new folder", M::NewFolder { parent: PathBuf::new(), name: String::new() }),
        ("confirm trash", M::ConfirmTrash { rel: rel.clone() }),
        ("settings", M::Settings { vault_path: String::new(), tab: crate::settings::SettingsTab::Vault, keys: Box::default(), find: Box::default() }),
        ("data usage", M::DataUsage),
        ("history", M::History),
        ("changes", M::Changes),
        ("sync wizard", M::SyncWizard),
        ("tex setup", M::TexSetup),
        ("lift paste", M::LiftPaste { text: String::new() }),
        ("outside files", M::OutsideFiles { text: String::new() }),
    ];
    assert!(!w.app.shortcuts_blocked(), "no dialog, no block");
    for (name, m) in dialogs {
        w.app.modal = Some(m);
        assert!(w.app.shortcuts_blocked(), "{name} blocks the shortcuts");
    }
    w.app.modal = None;
    assert!(!w.app.shortcuts_blocked());
}
