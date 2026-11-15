//! The keybindings dialog (KEY-006): every definable action by scope with its binding(s), a way
//! to record a new binding, reset one, reset all.

use crate::keys::{Action, Binding, Chord, Keymap, Scope, DEFS};
use crate::theme;
use eframe::egui::{self, Event, Key, Modifiers, Rect, RichText, Ui};

#[derive(Default)]
pub struct KeyDialog {
    /// The action whose binding is being recorded, and the chords pressed so far.
    pub recording: Option<(Action, Vec<Chord>)>,
    /// Why the last binding was refused.
    pub error: Option<String>,
    /// Where the controls were drawn (`key:<id>`, `reset:<id>`, `reset-all`), for tests.
    pub rects: Vec<(String, Rect)>,
}

impl KeyDialog {
    fn start(&mut self, a: Action) {
        self.recording = Some((a, vec![]));
        self.error = None;
    }

    /// Take the keys of one frame while recording: chords add up, Enter saves, Esc cancels.
    /// Returns true when a binding was saved.
    fn record(&mut self, keys: &mut Keymap, events: &[Event]) -> bool {
        let mut saved = false;
        for e in events {
            let Event::Key { key, pressed: true, repeat: false, modifiers, .. } = e else { continue };
            let Some((action, chords)) = &mut self.recording else { break };
            let bare = !modifiers.any();
            if bare && *key == Key::Escape {
                self.recording = None;
            } else if bare && *key == Key::Enter && !chords.is_empty() {
                match keys.set(*action, Binding(std::mem::take(chords))) {
                    Ok(()) => {
                        self.recording = None;
                        self.error = None;
                        saved = true;
                    }
                    Err(msg) => {
                        self.error = Some(msg);
                        self.recording = None;
                    }
                }
            } else {
                chords.push(Chord::new(*modifiers, *key));
            }
        }
        saved
    }
}

/// Draw the dialog's content. Returns true when a binding changed (the caller saves).
pub fn show(ui: &mut Ui, d: &mut KeyDialog, keys: &mut Keymap) -> bool {
    let mut changed = false;
    d.rects.clear();
    if d.recording.is_some() {
        // the recording takes every key: Esc must not close the dialog, nothing may press a button
        let events = ui.input(|i| i.events.clone());
        changed |= d.record(keys, &events);
        ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        ui.ctx().memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    ui.label(muted("Press Change, then the keys: one key or chord, or several in a row (a sequence). Enter saves, Esc cancels."));
    if let Some(e) = &d.error {
        ui.add_space(4.0);
        ui.label(RichText::new(e.as_str()).color(theme::LINK_MISSING));
    }
    ui.add_space(6.0);
    let mut start = None;
    let mut reset = None;
    for scope in Scope::ALL {
        ui.add_space(6.0);
        ui.label(RichText::new(scope.label()).strong());
        egui::Grid::new(("keys", scope.label())).num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
            for def in DEFS.iter().filter(|d| d.scope == scope) {
                ui.label(def.label);
                match &d.recording {
                    Some((a, chords)) if *a == def.action => {
                        let typed = Binding(chords.clone()).display();
                        ui.label(RichText::new(if chords.is_empty() { "press the keys…".to_string() } else { typed }).color(theme::ACCENT));
                    }
                    _ => {
                        let text = keys.display(def.action);
                        ui.label(if keys.is_default(def.action) { RichText::new(text) } else { RichText::new(format!("{text}  (changed)")).color(theme::ACCENT) });
                    }
                }
                ui.horizontal(|ui| {
                    let change = ui.button("Change");
                    d.rects.push((format!("key:{}", def.id), change.rect));
                    if change.clicked() {
                        start = Some(def.action);
                    }
                    let r = ui.add_enabled(!keys.is_default(def.action), egui::Button::new("Reset"));
                    d.rects.push((format!("reset:{}", def.id), r.rect));
                    if r.clicked() {
                        reset = Some(def.action);
                    }
                });
                ui.end_row();
            }
        });
    }
    if let Some(a) = start {
        d.start(a);
    }
    if let Some(a) = reset {
        keys.reset(a);
        d.error = None;
        changed = true;
    }
    ui.add_space(8.0);
    let r = ui.button("Reset all");
    d.rects.push(("reset-all".into(), r.rect));
    if r.clicked() {
        keys.reset_all();
        d.recording = None;
        d.error = None;
        changed = true;
    }
    keys.recording = d.recording.is_some();
    changed
}

fn muted(t: impl Into<String>) -> RichText {
    RichText::new(t).color(theme::MUTED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Modal, Options, ShaleApp, View};
    use eframe::egui::{Pos2, RawInput, Vec2};
    use std::path::Path;

    struct W {
        app: ShaleApp,
        ctx: egui::Context,
        t: f64,
        dir: std::path::PathBuf,
    }

    impl W {
        fn new(tag: &str) -> W {
            let dir = std::env::temp_dir().join(format!("shale-keysui-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for n in ["A", "B", "C"] {
                std::fs::write(dir.join(format!("{n}.md")), format!("# {n}\n")).unwrap();
            }
            std::fs::create_dir_all(dir.join("projects")).unwrap();
            std::fs::write(dir.join("projects/Roadmap.md"), "# Roadmap\nbanana\n").unwrap();
            let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
            app.open(Path::new("A.md"), false);
            let mut w = W { app, ctx: egui::Context::default(), t: 0.0, dir };
            w.frame(vec![]);
            w
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 1600.0))), time: Some(self.t), events, ..Default::default() };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
            out.textures_delta.clear();
        }

        fn press(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
            self.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
        }

        fn open_dialog(&mut self) {
            self.app.modal = Some(Modal::Keybindings(Box::default()));
            for _ in 0..4 {
                self.frame(vec![]);
            }
        }

        fn dialog(&self) -> &KeyDialog {
            match &self.app.modal {
                Some(Modal::Keybindings(d)) => d,
                _ => panic!("the keybindings dialog is not open"),
            }
        }

        fn click(&mut self, key: &str) {
            let r = self.dialog().rects.iter().find(|r| r.0 == key).unwrap_or_else(|| panic!("no control {key}")).1;
            let p = r.center();
            let b = |pressed| Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(p)]);
            self.frame(vec![b(true)]);
            self.frame(vec![b(false)]);
            self.frame(vec![]);
        }

        fn reading(&self) -> bool {
            self.app.tabs[self.app.active].reading
        }
    }

    impl Drop for W {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const CTRL: Modifiers = Modifiers::COMMAND;
    const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

    // spec: KEY-006, KEY-008, KEY-007
    #[test]
    fn changing_a_binding_moves_the_shortcut() {
        let mut w = W::new("change");
        // before: Ctrl+E switches to the reading view
        w.press(Key::E, CTRL);
        assert!(w.reading());
        w.press(Key::E, CTRL);
        assert!(!w.reading());

        w.press(Key::Comma, CTRL);
        w.frame(vec![]);
        w.frame(vec![]);
        assert!(matches!(w.app.modal, Some(Modal::Settings { .. })), "Ctrl+, opens the settings");
        w.app.modal = None;
        w.open_dialog();
        w.click("key:global.toggle_reading");
        assert!(w.dialog().recording.is_some());
        // while recording, keys trigger nothing and Esc does not close the dialog: Ctrl+E is recorded, not run
        w.press(Key::E, CTRL_ALT);
        assert!(!w.reading(), "a key typed to record does nothing else");
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().recording.is_none());
        assert_eq!(w.dialog().error, None);
        assert_eq!(w.app.cfg.keybindings.get("global.toggle_reading"), Some(&vec!["Ctrl+Alt+E".to_string()]), "the change is in the config to be saved");
        w.app.modal = None;
        w.frame(vec![]);
        // after: the new key works, the old one does not
        w.press(Key::E, CTRL);
        assert!(!w.reading(), "the old key does nothing now");
        w.press(Key::E, CTRL_ALT);
        assert!(w.reading(), "the new key works");

        // Esc cancels a recording and leaves the dialog open
        w.open_dialog();
        w.click("key:global.save");
        w.press(Key::S, CTRL_ALT);
        w.press(Key::Escape, Modifiers::NONE);
        assert!(w.dialog().recording.is_none());
        assert!(w.app.keymap.is_default(Action::Save));
        assert!(!w.app.keymap.recording, "triggers work again after the recording");
    }

    // spec: KEY-004
    #[test]
    fn a_double_binding_is_refused_with_the_name_of_the_other_action() {
        let mut w = W::new("double");
        w.open_dialog();
        w.click("key:global.focus_editor");
        w.press(Key::H, CTRL);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error.as_deref(), Some("Ctrl+H is already used by “Keyboard into the left sidebar”"));
        assert!(w.app.keymap.is_default(Action::FocusEditor), "nothing changed");
        assert!(w.app.cfg.keybindings.is_empty());
        // a binding that is taken in the other direction too: a Global key against a list key
        w.click("key:global.back");
        w.press(Key::J, CTRL);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error.as_deref(), Some("Ctrl+J is already used by “Next folder”"));
        // a plain key is no global shortcut
        w.click("key:global.back");
        w.press(Key::G, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().error.as_deref().unwrap().contains("needs Ctrl or Alt"));
        // a sequence can be recorded in a list scope
        w.click("key:files.toggle_folder");
        w.press(Key::Space, Modifiers::NONE);
        w.press(Key::F, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error, None);
        assert_eq!(w.app.cfg.keybindings.get("files.toggle_folder"), Some(&vec!["Space F".to_string()]));
    }

    // spec: KEY-006
    #[test]
    fn reset_and_reset_all_restore_the_defaults() {
        let mut w = W::new("reset");
        w.app.keymap.set(Action::ToggleReading, Binding::parse("Ctrl+Alt+E").unwrap()).unwrap();
        w.app.keymap.set(Action::NewNote, Binding::parse("Ctrl+Alt+N").unwrap()).unwrap();
        w.app.keymap.set(Action::TreeNextFile, Binding::parse("Ctrl+Alt+J").unwrap()).unwrap();
        w.open_dialog();
        w.click("reset:global.toggle_reading");
        assert!(w.app.keymap.is_default(Action::ToggleReading));
        assert!(!w.app.keymap.is_default(Action::NewNote));
        assert_eq!(w.app.cfg.keybindings.keys().collect::<Vec<_>>(), ["files.next_file", "global.new_note"]);
        w.app.modal = None;
        w.press(Key::E, CTRL);
        assert!(w.reading(), "Ctrl+E is back");
        w.open_dialog();
        w.click("reset-all");
        assert!(w.app.keymap.overrides().is_empty() && w.app.cfg.keybindings.is_empty());
    }

    fn views(w: &W) -> Vec<String> {
        w.app
            .tabs
            .iter()
            .map(|t| match &t.view {
                View::Note(p) => p.to_string_lossy().into_owned(),
                View::Calendar => "calendar".into(),
                View::Mail => "mail".into(),
                _ => "other".into(),
            })
            .collect()
    }

    // spec: NAV-018
    #[test]
    fn ctrl_alt_h_and_l_switch_tabs_and_wrap() {
        let mut w = W::new("tabs");
        // a single tab: nothing happens
        w.press(Key::L, CTRL_ALT);
        w.press(Key::H, CTRL_ALT);
        assert_eq!((w.app.tabs.len(), w.app.active), (1, 0));
        w.app.open(Path::new("B.md"), true);
        w.app.show_view(View::Calendar, true);
        assert_eq!((views(&w), w.app.active), (vec!["A.md".into(), "B.md".into(), "calendar".into()], 2));
        // Next from the last tab wraps to the first, Previous from the first to the last
        w.press(Key::L, CTRL_ALT);
        assert_eq!(w.app.active, 0);
        w.press(Key::L, CTRL_ALT);
        assert_eq!(w.app.active, 1);
        w.press(Key::H, CTRL_ALT);
        assert_eq!(w.app.active, 0);
        w.press(Key::H, CTRL_ALT);
        assert_eq!(w.app.active, 2, "wraps to the last tab, a sub-app");
        // from a sub-app (calendar is open now) and with the keyboard in the file list
        w.press(Key::H, CTRL_ALT);
        assert_eq!(w.app.active, 1);
        w.press(Key::H, CTRL);
        assert!(w.app.tree_sel.is_some(), "Ctrl+H: the file list has the keyboard");
        w.press(Key::L, CTRL_ALT);
        assert_eq!(w.app.active, 2, "works from the file list too, and Ctrl+L did not take it");
        // the order is the order the tabs are shown in (pinned and dragged tabs sit in `tabs`)
        w.app.tabs.swap(0, 2);
        w.app.active = 0;
        assert_eq!(views(&w)[0], "calendar");
        w.press(Key::L, CTRL_ALT);
        assert_eq!(views(&w)[w.app.active], "B.md");
        // another binding: the old key does nothing
        w.app.keymap.set(Action::NextTab, Binding::parse("Ctrl+Alt+N").unwrap()).unwrap();
        w.press(Key::L, CTRL_ALT);
        assert_eq!(views(&w)[w.app.active], "B.md", "the old key is gone");
        w.press(Key::N, CTRL_ALT);
        assert_eq!(views(&w)[w.app.active], "A.md", "the new one works");
    }

    /// A key that types a character: the key event and the text event, as the window sends them.
    fn typed(w: &mut W, key: Key, text: &str) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        let mut down = vec![ev(true)];
        if !text.is_empty() {
            down.push(Event::Text(text.to_string()));
        }
        w.frame(down);
        w.frame(vec![ev(false)]);
    }

    fn note_text(w: &W) -> String {
        w.app.notes[Path::new("A.md")].text.clone()
    }

    fn settle(w: &mut W) {
        for _ in 0..3 {
            w.frame(vec![]);
        }
    }

    /// Vim-style editing on, the open note's editor has the keyboard (normal mode).
    fn vim_note(tag: &str) -> W {
        let mut w = W::new(tag);
        w.app.cfg.vim_mode = true;
        w.app.focus_editor();
        settle(&mut w);
        assert!(w.app.tabs[0].editor.vim_normal_keys(&w.ctx), "the editor has the keyboard in normal mode");
        w
    }

    // spec: NAV-019
    #[test]
    fn space_tab_toggles_between_the_note_and_the_view_before_it() {
        let mut w = vim_note("alt");
        // nothing before the note yet: nothing happens
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Tab, "");
        assert_eq!(views(&w), ["A.md"]);
        w.app.show_view(View::Calendar, false);
        settle(&mut w);
        assert_eq!(views(&w), ["calendar"]);
        // calendar -> note, note -> calendar, and again
        for expect in ["A.md", "calendar", "A.md", "calendar"] {
            typed(&mut w, Key::Space, " ");
            typed(&mut w, Key::Tab, "");
            settle(&mut w);
            assert_eq!(views(&w), [expect]);
        }
        assert_eq!(note_text(&w), "# A\n", "nothing was typed into the note");
        // another note in the same tab: back and forth between the two
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Tab, "");
        settle(&mut w);
        w.app.open(Path::new("B.md"), false);
        w.app.focus_editor();
        settle(&mut w);
        assert_eq!(views(&w), ["B.md"]);
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Tab, "");
        settle(&mut w);
        assert_eq!(views(&w), ["A.md"]);
        // Back still works, with the swapped history
        w.press(Key::ArrowLeft, Modifiers::ALT);
        assert_eq!(views(&w), ["B.md"]);
        // the key can be changed like any other
        w.app.keymap.set(Action::NormalAlternate, Binding::parse("Space Backspace").unwrap()).unwrap();
        settle(&mut w);
        let before = views(&w);
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Tab, "");
        settle(&mut w);
        assert_eq!(views(&w), before, "the old keys do nothing");
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Backspace, "");
        settle(&mut w);
        assert_ne!(views(&w), before, "the new ones work");
    }

    // spec: EDIT-015
    #[test]
    fn space_tab_in_normal_mode_does_not_type_and_insert_mode_still_types() {
        let mut w = vim_note("leak");
        // a waiting Space types nothing and moves nothing; a key that does not continue it works as usual
        typed(&mut w, Key::Space, " ");
        assert_eq!(note_text(&w), "# A\n");
        typed(&mut w, Key::Z, "z");
        assert_eq!(note_text(&w), "# A\n", "z is no vim key either");
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::I, "i");
        assert!(w.app.tabs[0].editor.vim.mode == crate::vim::Mode::Insert, "i after a cancelled Space starts insert mode");
        // insert mode: Space is typing again
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::X, "x");
        assert!(note_text(&w).contains(" x"), "{:?}", note_text(&w));
        // a vim command line takes every key
        w.press(Key::Escape, Modifiers::NONE);
        typed(&mut w, Key::Colon, ":");
        typed(&mut w, Key::Space, " ");
        assert!(w.app.tabs[0].editor.vim.typing_command());
        assert!(!w.app.tabs[0].editor.vim_normal_keys(&w.ctx));
        // with Vim-style editing off the scope is not read at all
        w.press(Key::Escape, Modifiers::NONE);
        w.app.cfg.vim_mode = false;
        w.app.show_view(View::Calendar, false);
        settle(&mut w);
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Tab, "");
        assert_eq!(views(&w), ["calendar"]);
        // and the mail view keeps Space and f for itself
        w.app.cfg.vim_mode = true;
        w.app.show_view(View::Mail, false);
        settle(&mut w);
        assert!(!w.app.normal_keys_active(&w.ctx));
    }

    fn picker(w: &W) -> &crate::picker::Picker {
        match &w.app.modal {
            Some(Modal::Picker(p)) => p,
            _ => panic!("the picker is not open"),
        }
    }

    fn type_in_picker(w: &mut W, text: &str) {
        w.frame(vec![Event::Text(text.to_string())]);
        settle(w);
    }

    // spec: NAV-020, EDIT-015
    #[test]
    fn space_f_f_finds_a_file_by_name_and_opens_it_in_the_editor() {
        let mut w = vim_note("picker");
        for (key, text) in [(Key::Space, " "), (Key::F, "f")] {
            typed(&mut w, key, text);
            assert!(w.app.modal.is_none(), "half a sequence opens nothing");
        }
        assert_eq!(note_text(&w), "# A\n", "the sequence typed nothing into the note");
        typed(&mut w, Key::F, "f");
        settle(&mut w);
        assert_eq!(picker(&w).query, "", "the window is open with an empty field");
        assert_eq!(note_text(&w), "# A\n");
        // typing filters by file name only: the text of a note and the folder do not count
        type_in_picker(&mut w, "banana");
        assert_eq!(picker(&w).query, "banana");
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.app.modal.is_some() && views(&w) == ["A.md"], "no name matches: Enter does nothing, and no note is created");
        assert!(!w.dir.join("banana.md").exists());
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        settle(&mut w);
        type_in_picker(&mut w, "projects");
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.app.modal.is_some(), "the folder name is not a match");
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        settle(&mut w);
        // j and k are typed (the field has the keyboard); the arrows and Ctrl+J / Ctrl+K select
        type_in_picker(&mut w, "o");
        let names = |w: &W| crate::picker::Source::FileNames.hits(w.app.vault.as_ref().unwrap(), &picker(w).query).into_iter().map(|h| h.title).collect::<Vec<_>>();
        assert_eq!(names(&w), ["Roadmap"], "only the Roadmap has an o in its name");
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        settle(&mut w);
        assert_eq!(names(&w), ["A", "B", "C", "Roadmap"]);
        w.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 1);
        w.press(Key::J, CTRL);
        assert_eq!(picker(&w).sel, 2);
        w.press(Key::K, CTRL);
        w.press(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 0);
        w.press(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 3, "wraps around");
        // Esc closes and changes nothing
        w.press(Key::Escape, Modifiers::NONE);
        assert!(w.app.modal.is_none());
        assert_eq!(views(&w), ["A.md"]);
        // Enter on the selection: opens in the current tab, the keyboard is in the editor
        w.app.focus_editor();
        settle(&mut w);
        for (key, text) in [(Key::Space, " "), (Key::F, "f"), (Key::F, "f")] {
            typed(&mut w, key, text);
        }
        settle(&mut w);
        type_in_picker(&mut w, "rdmp");
        w.press(Key::Enter, Modifiers::NONE);
        for n in 0..4 {
            let ed = &w.app.tabs[0].editor;
            eprintln!("DBG0 {n} {} {} {:?}", ed.request_focus, w.ctx.memory(|m| m.has_focus(ed.id)), w.app.modal.is_some());
            w.frame(vec![]);
        }
        assert!(w.app.modal.is_none());
        assert_eq!((views(&w), w.app.tabs.len()), (vec!["projects/Roadmap.md".to_string()], 1), "opened in the current tab");
        let ed = &w.app.tabs[0].editor;
        eprintln!("DBG {} {:?} {} {}", ed.vim_enabled, ed.vim.mode, ed.vim.typing_command(), w.ctx.memory(|m| m.has_focus(ed.id)));
        assert!(w.app.tabs[0].editor.vim_normal_keys(&w.ctx), "the keyboard is in the editor, in normal mode");
        assert_eq!(w.app.notes[Path::new("A.md")].text, "# A\n");
    }

    // spec: KEY-008, NAV-020
    #[test]
    fn the_find_file_keys_can_be_changed() {
        let mut w = vim_note("picker-keys");
        w.app.keymap.set(Action::NormalFindFile, Binding::parse("Space p").unwrap()).unwrap();
        for (key, text) in [(Key::Space, " "), (Key::F, "f"), (Key::F, "f")] {
            typed(&mut w, key, text);
        }
        assert!(w.app.modal.is_none(), "the old keys do nothing");
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::P, "p");
        assert!(matches!(w.app.modal, Some(Modal::Picker(_))), "the new ones open the picker");
        assert_eq!(note_text(&w), "# A\n");
    }
}
