//! Ctrl+Tab / Ctrl+Shift+Tab cycle the tabs of the focused pane (KEY-039), driven in the whole
//! window with real key presses.

use crate::app::{Modal, Options, ShaleApp, View};
use crate::keys::{Action, Binding, Keymap};
use crate::ui_test_support::Headless;
use eframe::egui::{Event, Key, Modifiers};
use std::path::{Path, PathBuf};

const CTRL: Modifiers = Modifiers { alt: false, ctrl: true, shift: false, mac_cmd: false, command: true };
const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };
const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

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
    /// Three notes A, B, C open as tabs in this order, C open, the keyboard in the editor.
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-tab-keys-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["A.md", "B.md", "C.md"] {
            std::fs::write(dir.join(n), format!("# {n}\nbody")).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.cursor_blink = false;
        app.keymap = Keymap::from_overrides_for(&app.cfg.keybindings, false);
        app.open(Path::new("A.md"), false);
        for n in ["B.md", "C.md"] {
            app.open(Path::new(n), true);
        }
        assert_eq!(app.tabbar.tabs.len(), 3);
        app.focus_editor();
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 4);
        w
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
        self.hl.frames(&mut self.app, 2);
    }

    /// The note the open tab of pane `i` shows.
    fn open_in(&self, i: usize) -> String {
        let bar = self.app.pane_bar(i);
        match &bar.tabs[bar.active].view {
            View::Note(p) => p.to_string_lossy().into_owned(),
            _ => "other".into(),
        }
    }
}

// spec: KEY-039
#[test]
fn key_039_ctrl_tab_and_ctrl_shift_tab_cycle_the_tabs_and_wrap_both_ways() {
    let mut w = W::new("cycle");
    assert_eq!(w.open_in(0), "C.md");
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), "A.md", "next from the last tab wraps to the first");
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), "B.md");
    w.press(Key::Tab, CTRL_SHIFT);
    assert_eq!(w.open_in(0), "A.md");
    w.press(Key::Tab, CTRL_SHIFT);
    assert_eq!(w.open_in(0), "C.md", "previous from the first tab wraps to the last");
    // the older keys still do the same
    w.press(Key::PageDown, CTRL);
    assert_eq!(w.open_in(0), "A.md");
}

// spec: KEY-039
#[test]
fn key_039_the_order_is_the_tab_bar_order_with_a_pinned_tab_first() {
    let mut w = W::new("pinned");
    // pin C: it moves to the front, the bar reads C A B
    let c = w.app.tabbar.active;
    w.app.set_tab_pinned(c, true);
    assert_eq!(w.app.tabbar.tabs[0].view, View::Note("C.md".into()));
    w.app.tabbar.active = 0;
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), "A.md", "after the pinned tab comes the next one in the bar, not the last used");
    w.press(Key::Tab, CTRL);
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), "C.md", "the pinned tab is part of the cycle");
}

// spec: KEY-039
#[test]
fn key_039_in_a_split_view_it_cycles_the_tabs_of_the_focused_pane_only() {
    let mut w = W::new("split");
    w.app.split_right();
    w.hl.frames(&mut w.app, 3);
    w.app.open(Path::new("A.md"), true);
    w.hl.frames(&mut w.app, 3);
    assert_eq!((w.app.panes.focused, w.app.panes.len()), (1, 2));
    assert!(w.app.pane_bar(1).tabs.len() >= 2);
    let left_before = w.open_in(0);
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), left_before, "the other pane does not move");
    assert_eq!(w.app.panes.focused, 1);
    let right_after = w.open_in(1);
    w.press(Key::Tab, CTRL_SHIFT);
    assert_ne!(w.open_in(1), right_after, "the focused pane moves back");
    // focus the left pane: now that one cycles and the right one stays
    let right = w.open_in(1);
    w.app.focus_pane(0);
    w.hl.frames(&mut w.app, 3);
    w.press(Key::Tab, CTRL);
    assert_ne!(w.open_in(0), left_before);
    assert_eq!(w.open_in(1), right);
}

// spec: KEY-039
#[test]
fn key_039_ctrl_tab_in_the_editor_switches_the_tab_and_inserts_no_tab_character() {
    let mut w = W::new("editor");
    w.app.focus_editor();
    w.hl.frames(&mut w.app, 3);
    let texts = |w: &W| ["A.md", "B.md", "C.md"].map(|n| w.app.notes[Path::new(n)].text.clone());
    let before = texts(&w);
    // the window may send a text event with the key; Ctrl+Tab must not type it
    w.hl.frame(&mut w.app, vec![Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: CTRL }, Event::Text("\t".into())]);
    w.hl.frames(&mut w.app, 3);
    assert_eq!(w.open_in(0), "A.md");
    w.press(Key::Tab, CTRL_SHIFT);
    assert_eq!(texts(&w), before, "no tab character in any note");
}

// spec: KEY-039
#[test]
fn key_039_ctrl_tab_does_nothing_under_a_modal_dialog() {
    let mut w = W::new("modal");
    w.app.open_settings(crate::settings::SettingsTab::Vault);
    w.hl.frames(&mut w.app, 3);
    w.press(Key::Tab, CTRL);
    w.press(Key::Tab, CTRL_SHIFT);
    assert!(matches!(w.app.modal, Some(Modal::Settings { .. })));
    assert_eq!(w.open_in(0), "C.md");
    w.press(Key::Escape, Modifiers::NONE);
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), "A.md", "works again after the dialog");
}

// spec: KEY-039
#[test]
fn key_039_the_keys_are_rebindable_and_clash_free() {
    let linux = Keymap::for_os(false);
    let b = |s: &str| Binding::parse(s).unwrap();
    assert_eq!(linux.bindings(Action::NextTab), vec![b("Ctrl+PageDown"), b("Ctrl+Tab")]);
    assert_eq!(linux.bindings(Action::PrevTab), vec![b("Ctrl+PageUp"), b("Ctrl+Shift+Tab")]);
    // no default clashes with another (KEY-017) and none is reserved (KEY-019), on every system
    for k in [&linux, &Keymap::for_os(true)] {
        for action in [Action::NextTab, Action::PrevTab] {
            for bind in k.bindings(action) {
                assert_eq!(k.check_rules(action, &bind), Ok(()));
            }
        }
    }
    let mut w = W::new("rebind");
    w.app.keymap.set(Action::NextTab, b("Ctrl+Alt+N")).unwrap();
    w.press(Key::Tab, CTRL);
    assert_eq!(w.open_in(0), "C.md", "a rebound action no longer answers Ctrl+Tab");
    w.press(Key::N, CTRL_ALT);
    assert_eq!(w.open_in(0), "A.md");
    w.press(Key::Tab, CTRL_SHIFT);
    assert_eq!(w.open_in(0), "C.md", "the other action keeps its keys");
}
