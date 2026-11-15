//! The Keybindings tab of the Settings dialog (KEY-006): every definable action by scope with
//! its binding(s), a way to record a new binding, add one, clear, reset one, reset all.

use crate::keys::{Action, Binding, Chord, Keymap, Scope, DEFS};
use crate::theme;
use eframe::egui::{self, Event, Key, Modifiers, Rect, RichText, Ui};

#[derive(Default)]
pub struct KeyDialog {
    /// The action whose binding is being recorded, and the chords pressed so far.
    pub recording: Option<(Action, Vec<Chord>)>,
    /// The recording adds a binding (keeping the others) instead of replacing them.
    adding: bool,
    /// Why the last binding was refused.
    pub error: Option<String>,
    /// Where the controls were drawn (`key:<id>`, `add:<id>`, `clear:<id>`, `reset:<id>`,
    /// `reset-all`), for tests.
    pub rects: Vec<(String, Rect)>,
}

impl KeyDialog {
    fn start(&mut self, a: Action, adding: bool) {
        self.recording = Some((a, vec![]));
        self.adding = adding;
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
                let binding = Binding(std::mem::take(chords));
                match if self.adding { keys.add(*action, binding) } else { keys.set(*action, binding) } {
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

/// Draw the tab's content. Returns true when a binding changed (the caller saves).
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
    ui.label(muted("Press Change (replaces the keys) or Add (one more), then the keys: one key or chord, or several in a row (a sequence). Enter saves, Esc cancels."));
    if let Some(e) = &d.error {
        ui.add_space(4.0);
        ui.label(RichText::new(e.as_str()).color(theme::LINK_MISSING));
    }
    ui.add_space(6.0);
    let r = ui.button("Reset all");
    d.rects.push(("reset-all".into(), r.rect));
    if r.clicked() {
        keys.reset_all();
        d.recording = None;
        d.error = None;
        changed = true;
    }
    let mut start = None;
    let mut clear = None;
    let mut reset = None;
    // one grid for all groups, so that the columns line up from group to group
    egui::Grid::new("keys-grid").num_columns(3).spacing([14.0, 4.0]).show(ui, |ui| {
        for scope in Scope::ALL {
            ui.allocate_space(egui::vec2(0.0, 8.0));
            ui.end_row();
            ui.label(RichText::new(scope.label()).strong());
            ui.end_row();
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
                        start = Some((def.action, false));
                    }
                    let add = ui.button("Add").on_hover_text("One more key for this action, the others stay");
                    d.rects.push((format!("add:{}", def.id), add.rect));
                    if add.clicked() {
                        start = Some((def.action, true));
                    }
                    let c = ui.add_enabled(!keys.bindings(def.action).is_empty(), egui::Button::new("Clear")).on_hover_text("No key for this action");
                    d.rects.push((format!("clear:{}", def.id), c.rect));
                    if c.clicked() {
                        clear = Some(def.action);
                    }
                    let r = ui.add_enabled(!keys.is_default(def.action), egui::Button::new("Reset"));
                    d.rects.push((format!("reset:{}", def.id), r.rect));
                    if r.clicked() {
                        reset = Some(def.action);
                    }
                });
                ui.end_row();
            }
        }
    });
    if let Some((a, adding)) = start {
        d.start(a, adding);
    }
    if let Some(a) = clear {
        keys.clear(a);
        d.error = None;
        changed = true;
    }
    if let Some(a) = reset {
        keys.reset(a);
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
    use crate::app::{Modal, Options, SettingsTab, ShaleApp, View};
    use eframe::egui::{Pos2, RawInput, Vec2};
    use std::path::Path;

    struct W {
        app: ShaleApp,
        ctx: egui::Context,
        t: f64,
        dir: std::path::PathBuf,
        /// What the last frame painted.
        texts: Vec<String>,
        /// The same with the top-left corner of each text.
        text_pos: Vec<(String, Pos2)>,
        /// The size of the window.
        size: Vec2,
    }

    impl W {
        fn new(tag: &str) -> W {
            let dir = std::env::temp_dir().join(format!("shale-keysui-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for n in ["A", "B", "C"] {
                std::fs::write(dir.join(format!("{n}.md")), format!("# {n}\n")).unwrap();
            }
            std::fs::write(dir.join("Map.excalidraw"), r#"{"type":"excalidraw","version":2,"elements":[],"appState":{},"files":{}}"#).unwrap();
            std::fs::create_dir_all(dir.join("projects")).unwrap();
            std::fs::write(dir.join("projects/Roadmap.md"), "# Roadmap\nbanana\n").unwrap();
            let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
            app.open(Path::new("A.md"), false);
            let mut w = W { app, ctx: egui::Context::default(), t: 0.0, dir, texts: vec![], text_pos: vec![], size: Vec2::new(900.0, 1600.0) };
            w.frame(vec![]);
            w
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, self.size)), time: Some(self.t), events, ..Default::default() };
            let app = &mut self.app;
            let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
            out.textures_delta.clear();
            fn walk(s: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
                match s {
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.pos)),
                    _ => {}
                }
            }
            self.text_pos.clear();
            for c in &out.shapes {
                walk(&c.shape, &mut self.text_pos);
            }
            self.texts = self.text_pos.iter().map(|t| t.0.clone()).collect();
        }

        fn press(&mut self, key: Key, modifiers: Modifiers) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
            self.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
        }

        fn open_dialog(&mut self) {
            self.app.open_settings(SettingsTab::Keybindings);
            for _ in 0..4 {
                self.frame(vec![]);
            }
        }

        fn dialog(&self) -> &KeyDialog {
            match &self.app.modal {
                Some(Modal::Settings { keys, tab: SettingsTab::Keybindings, .. }) => keys,
                _ => panic!("the Keybindings tab of the settings is not open"),
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

        /// The window rect of the Settings dialog.
        fn dialog_rect(&self) -> Rect {
            self.ctx.memory(|m| m.area_rect(egui::Id::unique(("dialog", "settings")))).expect("settings dialog shown")
        }

        fn done_at(&self) -> Pos2 {
            self.text_pos.iter().find(|t| t.0 == "Done").expect("a Done button").1
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
        // a pinned tab sits first, and so it comes first in the order
        let a = w.app.tabs.iter().position(|t| t.view == View::Note("A.md".into())).unwrap();
        w.app.set_tab_pinned(a, true);
        assert_eq!(views(&w), ["A.md", "calendar", "B.md"]);
        w.app.active = 0;
        w.press(Key::H, CTRL_ALT);
        assert_eq!(views(&w)[w.app.active], "B.md", "Previous from the pinned first tab wraps to the last");
        w.press(Key::L, CTRL_ALT);
        w.press(Key::L, CTRL_ALT);
        assert_eq!(views(&w)[w.app.active], "calendar");
        w.app.active = views(&w).iter().position(|v| v == "B.md").unwrap();
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

    // spec: KEY-006, KEY-007
    #[test]
    fn add_keeps_the_other_bindings_and_clear_unbinds() {
        let mut w = W::new("add-clear");
        w.open_dialog();
        // the Keybindings tab sits in the settings next to the General one, which keeps the vault folder
        assert!(w.texts.iter().any(|t| t == "General") && w.texts.iter().any(|t| t == "Keybindings"), "{:?}", w.texts);
        assert!(w.texts.iter().any(|t| t.contains("Press Change")));
        assert!(!w.texts.iter().any(|t| t == "Vault folder"));
        // Add: j and Down stay, the new key joins them
        w.click("add:files.next_file");
        w.press(Key::J, CTRL_ALT);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error, None);
        assert_eq!(w.app.cfg.keybindings.get("files.next_file"), Some(&vec!["J".to_string(), "Down".to_string(), "Ctrl+Alt+J".to_string()]));
        // an added binding that clashes is refused with the name of the other action and changes nothing
        w.click("add:files.next_file");
        w.press(Key::K, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error.as_deref(), Some("k is already used by “Previous file”"));
        assert_eq!(w.app.keymap.bindings(Action::TreeNextFile).len(), 3);
        // Change replaces all of them
        w.click("key:files.next_file");
        w.press(Key::N, CTRL_ALT);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.app.cfg.keybindings.get("files.next_file"), Some(&vec!["Ctrl+Alt+N".to_string()]));
        // Clear: stored as an empty list, shown as not set, and the key does nothing any more
        w.click("clear:global.toggle_reading");
        assert_eq!(w.app.cfg.keybindings.get("global.toggle_reading"), Some(&vec![]));
        assert!(w.texts.iter().any(|t| t.contains("not set")), "{:?}", w.texts);
        w.app.modal = None;
        w.press(Key::E, CTRL);
        assert!(!w.reading(), "Ctrl+E does nothing after Clear");
        // Reset gives the default back
        w.open_dialog();
        w.click("reset:global.toggle_reading");
        assert!(w.app.keymap.is_default(Action::ToggleReading));
        w.app.modal = None;
        w.press(Key::E, CTRL);
        assert!(w.reading());
    }

    // spec: KEY-006
    #[test]
    fn the_columns_of_the_keybindings_line_up_across_the_groups() {
        let mut w = W::new("columns");
        w.open_dialog();
        let x = |id: &str| w.dialog().rects.iter().find(|r| r.0 == id).unwrap().1.min.x;
        let first = x("key:global.save");
        for d in DEFS {
            assert_eq!(x(&format!("key:{}", d.id)), first, "{}", d.id);
        }
    }

    // spec: KEY-009
    #[test]
    fn a_waiting_sequence_shows_in_the_status_bar() {
        let mut w = vim_note("pending");
        assert!(!w.texts.iter().any(|t| t.starts_with("Space")));
        typed(&mut w, Key::Space, " ");
        w.frame(vec![]);
        assert!(w.texts.iter().any(|t| t == "Space"), "{:?}", w.texts);
        typed(&mut w, Key::F, "f");
        w.frame(vec![]);
        assert!(w.texts.iter().any(|t| t == "Space f"), "{:?}", w.texts);
        // after the time-out it is gone, and the late key starts nothing
        for _ in 0..40 {
            w.frame(vec![]);
        }
        assert!(!w.texts.iter().any(|t| t.starts_with("Space")), "{:?}", w.texts);
        typed(&mut w, Key::F, "f");
        assert!(w.app.modal.is_none());
        // a finished sequence shows nothing
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Tab, "");
        w.frame(vec![]);
        assert!(!w.texts.iter().any(|t| t.starts_with("Space")), "{:?}", w.texts);
    }

    fn kind(w: &W) -> &'static str {
        match w.app.tabs[w.app.active].view {
            View::Note(_) => "note",
            View::Mail => "mail",
            View::Drawing(_) => "drawing",
            _ => "other",
        }
    }

    // spec: NAV-019, KEY-011
    #[test]
    fn ctrl_6_swaps_note_and_mail_and_note_and_drawing_by_keys_only() {
        let mut w = W::new("ctrl6");
        // nothing before the note: nothing happens
        w.press(Key::Num6, CTRL);
        assert_eq!(kind(&w), "note");
        // note -> mail (Ctrl+Shift+M) -> note -> mail -> note, from the mail view too
        w.press(Key::M, Modifiers::COMMAND | Modifiers::SHIFT);
        settle(&mut w);
        assert_eq!(kind(&w), "mail");
        for expect in ["note", "mail", "note"] {
            w.press(Key::Num6, CTRL);
            settle(&mut w);
            assert_eq!(kind(&w), expect);
        }
        // note -> drawing -> note -> drawing -> note -> drawing: Space and the tool keys stay the drawing's own
        w.app.open(Path::new("Map.excalidraw"), false);
        settle(&mut w);
        assert_eq!(kind(&w), "drawing");
        for expect in ["note", "drawing", "note", "drawing"] {
            w.press(Key::Num6, CTRL);
            settle(&mut w);
            assert_eq!(kind(&w), expect);
        }
        assert_eq!(note_text(&w), "# A\n", "nothing was typed");
        // the key is a definable action: the old one does nothing, the new one swaps
        w.app.keymap.set(Action::AlternateView, Binding::parse("Ctrl+Alt+6").unwrap()).unwrap();
        w.press(Key::Num6, CTRL);
        assert_eq!(kind(&w), "drawing");
        w.press(Key::Num6, CTRL_ALT);
        assert_eq!(kind(&w), "note");
        // not while a dialog is open
        w.app.open_settings(SettingsTab::General);
        settle(&mut w);
        w.press(Key::Num6, CTRL_ALT);
        assert_eq!(kind(&w), "note");
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
        let names = |w: &W| picker(w).listed().iter().map(|h| h.title.clone()).collect::<Vec<_>>();
        assert_eq!(names(&w), ["Roadmap"], "only the Roadmap has an o in its name");
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        settle(&mut w);
        assert_eq!(names(&w), ["A", "B", "C", "Map", "Roadmap"]);
        w.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 1);
        w.press(Key::J, CTRL);
        assert_eq!(picker(&w).sel, 2);
        w.press(Key::K, CTRL);
        w.press(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 0);
        w.press(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 4, "wraps around");
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
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert_eq!((views(&w), w.app.tabs.len()), (vec!["projects/Roadmap.md".to_string()], 1), "opened in the current tab");
        assert!(w.app.tabs[0].editor.vim_normal_keys(&w.ctx), "the keyboard is in the editor, in normal mode");
        assert_eq!(w.app.notes[Path::new("A.md")].text, "# A\n");
    }

    /// A mail index with two mails ("Banana invoice" and "Lunch") that the app has open.
    fn with_mail(w: &mut W) {
        let root = w.dir.join("mail-store");
        {
            let store = crate::mail_store::MailStore::open(&root).unwrap();
            let folder = store.ensure_folder("acct", "Acct", "INBOX", Some("/"), None, true).unwrap();
            store.commit(vec![crate::mail_db::Op::FolderState { folder: folder.id, uidvalidity: Some(7), uidnext: Some(10), highestmodseq: None, last_sync: 1 }]).unwrap();
            let folder = crate::mail_db::FolderRow { uidvalidity: Some(7), ..folder };
            let mails = [(1u32, "Banana invoice"), (2, "Lunch")];
            let ops = mails.iter().map(|(uid, subject)| {
                let headers = format!("From: Ann <ann@x.de>\r\nSubject: {subject}\r\n\r\n");
                crate::mail_db::Op::message(store.header_only(&folder, 7, *uid, headers.as_bytes(), "", Some(1_700_000_000 + i64::from(*uid)), 100))
            });
            store.commit(ops.collect()).unwrap();
        }
        w.app.cfg.mail_sync_minutes = 0;
        w.app.cfg.mail_accounts = vec![crate::config::MailAccount { id: "acct".into(), name: "Acct".into(), host: "localhost".into(), user: "u".into(), ..Default::default() }];
        w.app.mail.root = root;
        w.app.show_view(View::Mail, false);
        for _ in 0..200 {
            w.frame(vec![]);
            if w.app.mail.index().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(w.app.mail.index().is_some(), "the mail store opened");
        w.app.open(Path::new("A.md"), false);
        w.app.focus_editor();
        settle(w);
    }

    /// One post about bananas in the post index, of the demo account.
    fn with_post(w: &mut W) {
        let entry = crate::social::demo::account();
        w.app.cfg.social_accounts = vec![entry.clone()];
        w.app.social.load_demo(&entry);
        let mut p = crate::social::tests::post("b1", "bob", crate::cal_time::now() - 600, None);
        p.text_markdown = "banana republic".into();
        w.app.social.index.add(&entry.account.id, "home", &[p]);
    }

    /// Type into the picker of `Space f g` and let the debounce pass (NAV-024).
    fn search_for(w: &mut W, text: &str) {
        w.frame(vec![Event::Text(text.to_string())]);
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle(w);
    }

    fn space_f_g(w: &mut W) {
        for (key, text) in [(Key::Space, " "), (Key::F, "f"), (Key::G, "g")] {
            typed(w, key, text);
        }
        settle(w);
    }

    fn picker_rows(w: &W) -> Vec<(&'static str, String)> {
        picker(w).listed().iter().map(|h| (h.group, h.title.clone())).collect()
    }

    // spec: NAV-022, NAV-024, EDIT-015
    #[test]
    fn space_f_g_lists_notes_mail_and_posts_in_groups_with_counts() {
        let mut w = vim_note("search-all");
        with_mail(&mut w);
        with_post(&mut w);
        space_f_g(&mut w);
        assert_eq!(picker(&w).source, crate::picker::Source::Everything);
        assert_eq!(picker(&w).query, "");
        assert!(picker(&w).listed().is_empty(), "nothing typed, nothing listed");
        assert_eq!(note_text(&w), "# A\n", "the sequence typed nothing into the note");
        search_for(&mut w, "banana");
        assert_eq!(note_text(&w), "# A\n", "typing goes into the field");
        assert_eq!(picker_rows(&w), [("Notes", "banana".to_string()), ("Mail", "Banana invoice".to_string()), ("Posts", "banana republic".to_string())], "grouped in this order");
        // the wildcards reach all three
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything)));
        settle(&mut w);
        search_for(&mut w, "ban*");
        assert_eq!(picker_rows(&w).iter().map(|r| r.0).collect::<Vec<_>>(), ["Notes", "Mail", "Posts"]);
        // ↓ ↑ Ctrl+J Ctrl+K select across the groups, wrapping
        assert_eq!(picker(&w).sel, 0);
        w.press(Key::ArrowDown, Modifiers::NONE);
        w.press(Key::J, CTRL);
        assert_eq!(picker(&w).sel, 2);
        w.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 0, "wraps");
        w.press(Key::K, CTRL);
        assert_eq!(picker(&w).sel, 2);
        w.press(Key::ArrowUp, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 1);
        // Esc closes and changes nothing
        w.press(Key::Escape, Modifiers::NONE);
        assert!(w.app.modal.is_none());
        assert_eq!((views(&w), note_text(&w)), (vec!["A.md".to_string()], "# A\n".to_string()));
    }

    // spec: NAV-025, NAV-009
    #[test]
    fn space_f_g_opens_a_note_line_a_mail_and_a_post() {
        let mut w = vim_note("search-open");
        with_mail(&mut w);
        with_post(&mut w);
        // a note: the found line, the keyboard in the editor
        space_f_g(&mut w);
        search_for(&mut w, "ban?na");
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert_eq!((views(&w), w.app.tabs.len()), (vec!["projects/Roadmap.md".to_string()], 1), "opened in this tab");
        assert!(w.app.tabs[0].editor.vim_normal_keys(&w.ctx), "the keyboard is in the editor");
        let cursor = egui::text_edit::TextEditState::load(&w.ctx, w.app.tabs[0].editor.id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0);
        let text = &w.app.notes[Path::new("projects/Roadmap.md")].text;
        let at = text.find("banana").unwrap();
        assert_eq!(cursor, Some(text[..at].chars().count()), "the cursor is at the match on line 2");
        // a mail: the mail view with the message open
        w.app.open(Path::new("A.md"), false);
        w.app.focus_editor();
        settle(&mut w);
        space_f_g(&mut w);
        search_for(&mut w, "invoice");
        assert_eq!(picker_rows(&w), [("Mail", "Banana invoice".to_string())]);
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert_eq!(views(&w), ["mail"]);
        assert_eq!(w.app.mail.open.as_ref().map(|o| o.msg.subject.as_str()), Some("Banana invoice"), "the message is open");
        assert_eq!(w.app.mail.cursor, Some(1), "the list has the cursor on it (newest first: it is the second row)");
        // a post: its feed in a new Feeds tab
        w.app.open(Path::new("A.md"), false);
        w.app.focus_editor();
        settle(&mut w);
        let tabs = w.app.tabs.len();
        space_f_g(&mut w);
        search_for(&mut w, "republic");
        assert_eq!(picker_rows(&w), [("Posts", "banana republic".to_string())]);
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert_eq!(w.app.tabs.len(), tabs + 1, "a new tab");
        let feed = crate::social::FeedRef { account: crate::social::demo::account().account.id, feed: "home".into() };
        assert_eq!(w.app.tabs[w.app.active].view, View::Social(Some(feed)));
        assert_eq!(w.app.notes[Path::new("A.md")].text, "# A\n");
    }

    // spec: KEY-008, NAV-022
    #[test]
    fn the_search_all_keys_can_be_changed() {
        let mut w = vim_note("search-keys");
        w.app.keymap.set(Action::NormalSearchAll, Binding::parse("Space s").unwrap()).unwrap();
        for (key, text) in [(Key::Space, " "), (Key::F, "f"), (Key::G, "g")] {
            typed(&mut w, key, text);
        }
        assert!(w.app.modal.is_none(), "the old keys do nothing");
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::S, "s");
        assert!(matches!(&w.app.modal, Some(Modal::Picker(p)) if p.source == crate::picker::Source::Everything));
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

    /// A real click on a point.
    fn click_at(w: &mut W, p: Pos2) {
        let b = |pressed| Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        w.frame(vec![Event::PointerMoved(p)]);
        w.frame(vec![b(true)]);
        w.frame(vec![b(false)]);
        w.frame(vec![]);
    }

    // spec: NAV-027
    #[test]
    fn nav_027_the_done_button_stays_in_view_on_every_tab_of_a_short_window() {
        for size in [Vec2::new(1000.0, 600.0), Vec2::new(800.0, 500.0)] {
            for tab in [SettingsTab::Keybindings, SettingsTab::General] {
                let mut w = W::new("short");
                w.size = size;
                w.app.open_settings(tab);
                for _ in 0..8 {
                    w.frame(vec![]);
                }
                let screen = Rect::from_min_size(Pos2::ZERO, size);
                let dialog = w.dialog_rect();
                assert!(screen.contains_rect(dialog), "{size:?} {tab:?}: {dialog:?}");
                let done = w.done_at();
                assert!(dialog.contains(done + Vec2::new(10.0, 8.0)), "{size:?} {tab:?}: Done at {done:?} is outside the dialog {dialog:?}");
                // a real click on it closes the dialog
                click_at(&mut w, done + Vec2::new(10.0, 8.0));
                assert!(w.app.modal.is_none(), "{size:?} {tab:?}: Done did not close the settings");
            }
        }
    }

    // spec: NAV-027
    #[test]
    fn nav_027_the_key_list_scrolls_to_its_last_row_and_rebinding_still_works() {
        let mut w = W::new("short-scroll");
        w.size = Vec2::new(1000.0, 600.0);
        w.open_dialog();
        let last = DEFS.last().expect("actions").id;
        let key = format!("key:{last}");
        let shown = |w: &W| {
            let r = w.dialog().rects.iter().find(|r| r.0 == key).expect("control").1;
            w.dialog_rect().contains_rect(r)
        };
        assert!(!shown(&w), "the last row is below the short window at first");
        // the mouse wheel over the list scrolls it to the end
        let over = w.dialog_rect().center();
        w.frame(vec![Event::PointerMoved(over)]);
        for _ in 0..40 {
            w.frame(vec![Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, -400.0), modifiers: Modifiers::NONE, phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel }]);
        }
        assert!(shown(&w), "the last row came into view");
        // its button works with a real click, and the key is recorded
        w.click(&key);
        assert!(w.dialog().recording.is_some());
        w.press(Key::Y, CTRL_ALT);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error, None);
        assert_eq!(w.app.cfg.keybindings.get(last), Some(&vec!["Ctrl+Alt+Y".to_string()]));
        // the Done button is still in view after scrolling
        let done = w.done_at() + Vec2::new(10.0, 8.0);
        assert!(w.dialog_rect().contains(done));
    }
}
