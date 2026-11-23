//! The thread overview in the right sidebar (NAV-070) and its keys (KEY-050), in the whole window
//! with real key presses and clicks.

use crate::app::{Modal, Options, RightPanel, ShaleApp, View};
use crate::keys::{Action, Keymap, Scope};
use crate::ui_test_support::Headless;
use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use std::path::{Path, PathBuf};

const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };

fn mail(id: &str, date: &str, root: &str, from: &str) -> String {
    format!("---\ntype: email\nsubject: \"{id}\"\nfrom: \"{from}\"\ndate: {date}\nmessage-id: \"<{id}@h>\"\nthread-id: \"<{root}@h>\"\n---\n\n# {id}\n")
}

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
    /// Three mails of one thread (written out of order on purpose: the date decides), a mail of
    /// another thread and a note without a thread. The first mail is open.
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-thread-ui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in [
            ("2026-11-17 Second.md", mail("b", "2026-11-17 09:30", "a", "Bob")),
            ("2026-11-16 First.md", mail("a", "2026-11-16 08:00", "a", "Ann")),
            ("2026-11-18 Third.md", mail("c", "2026-11-18 18:45", "a", "Cy")),
            ("2026-11-17 Elsewhere.md", mail("x", "2026-11-17 10:00", "x", "Xu")),
            ("Plain.md", "# Plain\n".to_string()),
        ] {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.cursor_blink = false;
        app.cfg.show_right = true;
        app.keymap = Keymap::from_overrides_for(&app.cfg.keybindings, false);
        app.open(Path::new("2026-11-16 First.md"), false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 4);
        w
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
        self.hl.frames(&mut self.app, 3);
    }

    fn shown(&self) -> String {
        match &self.app.tabbar.tabs[self.app.tabbar.active].view {
            View::Note(p) => p.to_string_lossy().into_owned(),
            _ => "other".into(),
        }
    }

    /// What the right sidebar says.
    fn right(&self) -> Vec<(String, Pos2)> {
        let x = self.app.side.right_rect.expect("the right sidebar is drawn").min.x;
        self.hl.text.iter().filter(|(_, p)| p.x >= x).cloned().collect()
    }

    fn right_has(&self, text: &str) -> bool {
        self.right().iter().any(|(t, _)| t.contains(text))
    }

    fn show_thread(&mut self) {
        self.app.side.layout.show_panel(RightPanel::Thread);
        self.hl.frames(&mut self.app, 3);
    }
}

// spec: KEY-050, KEY-001
#[test]
fn key_050_the_three_keys_are_global_definable_and_clash_with_nothing() {
    let k = Keymap::from_overrides_for(&Default::default(), false);
    let text = |a: Action| k.bindings(a).iter().map(|b| b.to_text()).collect::<Vec<_>>();
    assert_eq!(text(Action::ThreadPrevious), ["Ctrl+Shift+PageUp"]);
    assert_eq!(text(Action::ThreadNext), ["Ctrl+Shift+PageDown"]);
    assert_eq!(text(Action::ThreadOverview), ["Ctrl+Shift+T"]);
    for (a, id) in [(Action::ThreadPrevious, "global.thread_previous"), (Action::ThreadNext, "global.thread_next"), (Action::ThreadOverview, "global.thread_overview")] {
        let def = crate::keys::DEFS.iter().find(|d| d.action == a).unwrap();
        assert_eq!((def.id, def.scope), (id, Scope::Global));
        assert!(def.defaults.iter().all(|d| !d.contains("Alt")), "no Alt key");
    }
    // the same with Cmd on macOS: the keys name Ctrl, which the mac keymap shows as Cmd (KEY-019 reserves none of them)
    let mac = Keymap::from_overrides_for(&Default::default(), true);
    assert_eq!(mac.bindings(Action::ThreadOverview).len(), 1);
}

// spec: KEY-050
#[test]
fn key_050_page_down_and_page_up_step_through_the_thread_in_date_order_and_stop_at_the_ends() {
    let mut w = W::new("step");
    assert_eq!(w.shown(), "2026-11-16 First.md");
    w.press(Key::PageUp, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-16 First.md", "nothing before the first");
    w.press(Key::PageDown, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-17 Second.md");
    w.press(Key::PageDown, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-18 Third.md", "the date order, not the other thread's mail of the 17th");
    w.press(Key::PageDown, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-18 Third.md", "nothing after the last");
    w.press(Key::PageUp, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-17 Second.md");
    // a note without a thread: the keys do nothing
    w.app.open(Path::new("Plain.md"), false);
    w.press(Key::PageDown, CTRL_SHIFT);
    assert_eq!(w.shown(), "Plain.md");
}

// spec: KEY-050, KEY-038
#[test]
fn key_050_the_keys_do_nothing_while_a_dialog_is_open() {
    let mut w = W::new("modal");
    w.app.modal = Some(Modal::DataUsage);
    w.hl.frames(&mut w.app, 2);
    w.press(Key::PageDown, CTRL_SHIFT);
    w.press(Key::T, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-16 First.md");
    assert!(!w.app.thread.kbd, "the overview did not take the keyboard");
    w.app.modal = None;
    w.hl.frames(&mut w.app, 2);
    w.press(Key::PageDown, CTRL_SHIFT);
    assert_eq!(w.shown(), "2026-11-17 Second.md", "the same key works when the dialog is gone");
}

// spec: NAV-070
#[test]
fn nav_070_the_thread_view_lists_the_mails_oldest_first_with_date_sender_and_title() {
    let mut w = W::new("list");
    w.show_thread();
    let rows = w.right();
    let at = |t: &str| rows.iter().find(|(s, _)| s == t).unwrap_or_else(|| panic!("{t:?} not in {rows:?}")).1;
    let (first, second, third) = (at("2026-11-16 First"), at("2026-11-17 Second"), at("2026-11-18 Third"));
    assert!(first.y < second.y && second.y < third.y, "oldest first");
    assert!(w.right_has("2026-11-17 09:30  ·  Bob") && w.right_has("2026-11-16 08:00  ·  Ann"), "date and sender: {rows:?}");
    assert!(!w.right_has("Elsewhere") && !w.right_has("Plain"), "only this thread");
    // the open note is marked by an accent fill behind its row
    let accent = crate::theme::palette().accent.gamma_multiply(0.25);
    let marked: Vec<f32> = w.hl.rects.iter().filter(|(_, c)| *c == accent).map(|(r, _)| r.center().y).collect();
    assert!(marked.iter().any(|y| (*y - first.y).abs() < 25.0) && !marked.iter().any(|y| (*y - third.y).abs() < 20.0), "{marked:?} for {first:?} {third:?}");
    // it follows the note
    w.app.open(Path::new("2026-11-18 Third.md"), false);
    w.hl.frames(&mut w.app, 3);
    let accent_rows: Vec<f32> = w.hl.rects.iter().filter(|(_, c)| *c == accent).map(|(r, _)| r.center().y).collect();
    assert!(accent_rows.iter().any(|y| (*y - third.y).abs() < 25.0), "{accent_rows:?}");
}

// spec: NAV-070
#[test]
fn nav_070_the_view_says_so_without_other_notes_and_hints_without_a_thread_id() {
    let mut w = W::new("empty");
    w.show_thread();
    w.app.open(Path::new("2026-11-17 Elsewhere.md"), false);
    w.hl.frames(&mut w.app, 3);
    assert!(w.right_has("No other note in this thread yet."), "{:?}", w.right());
    w.app.open(Path::new("Plain.md"), false);
    w.hl.frames(&mut w.app, 3);
    assert!(w.right_has("This note has no thread"), "{:?}", w.right());
}

// spec: NAV-070, NAV-062
#[test]
fn nav_070_a_click_opens_the_note_and_a_pinned_tab_is_not_replaced() {
    let mut w = W::new("click");
    w.show_thread();
    let p = w.right().iter().find(|(t, _)| t == "2026-11-18 Third").unwrap().1;
    w.hl.click(&mut w.app, p + eframe::egui::vec2(4.0, 4.0), PointerButton::Primary);
    w.hl.frames(&mut w.app, 3);
    assert_eq!(w.shown(), "2026-11-18 Third.md");
    assert_eq!(w.app.tabbar.tabs.len(), 1, "the tab shows the note");
    // pinned: it opens in a new tab and the pinned one stays
    let a = w.app.tabbar.active;
    w.app.set_tab_pinned(a, true);
    w.hl.frames(&mut w.app, 3);
    let p = w.right().iter().find(|(t, _)| t == "2026-11-16 First").unwrap().1;
    w.hl.click(&mut w.app, p + eframe::egui::vec2(4.0, 4.0), PointerButton::Primary);
    w.hl.frames(&mut w.app, 3);
    assert_eq!(w.app.tabbar.tabs.len(), 2);
    assert!(w.app.tabbar.tabs.iter().any(|t| t.pinned && t.view == View::Note("2026-11-18 Third.md".into())), "the pinned tab keeps its note");
    assert_eq!(w.shown(), "2026-11-16 First.md");
}

// spec: KEY-050
#[test]
fn key_050_the_overview_key_opens_the_view_and_the_arrows_enter_and_escape_work_in_it() {
    let mut w = W::new("overview");
    w.app.cfg.show_right = false;
    w.hl.frames(&mut w.app, 2);
    w.press(Key::T, CTRL_SHIFT);
    assert!(w.app.cfg.show_right && w.app.thread.kbd, "the sidebar is open and the list has the keyboard");
    assert!(w.right_has("2026-11-17 Second"), "{:?}", w.right());
    assert_eq!(w.app.thread.sel, 0, "it marks the open note");
    w.press(Key::ArrowDown, Modifiers::NONE);
    w.press(Key::ArrowDown, Modifiers::NONE);
    w.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(w.app.thread.sel, 2, "stops at the last row");
    w.press(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(w.app.thread.sel, 1);
    assert_eq!(w.shown(), "2026-11-16 First.md", "moving opens nothing");
    w.press(Key::Enter, Modifiers::NONE);
    assert_eq!(w.shown(), "2026-11-17 Second.md", "Enter jumps");
    assert!(!w.app.thread.kbd);
    // Esc gives the keyboard back to the note, and the note stays
    w.press(Key::T, CTRL_SHIFT);
    assert_eq!(w.app.thread.sel, 1, "the open note is marked");
    w.press(Key::Escape, Modifiers::NONE);
    assert!(!w.app.thread.kbd);
    assert_eq!(w.shown(), "2026-11-17 Second.md");
}
