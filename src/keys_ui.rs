//! The Keybindings tab of the Settings dialog (KEY-006): every definable action by scope with
//! its binding(s), a way to record a new binding, add one, clear, reset one, reset all.

use crate::keys::{Action, Binding, Chord, Keymap, Scope, DEFS};
use crate::settings::anchor;
use crate::settings::registry::{action_anchor, action_matches, bound_keys, terms};
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
    /// A recorded binding that clashes with other actions, waiting for the answer to “Overwrite it?”
    /// (KEY-018).
    pub confirm: Option<Confirm>,
    /// What the filter field above the list says (KEY-006, CFG-072): the actions whose label or
    /// keys fit stay in the list.
    pub filter: String,
    /// The filter field takes the keyboard in the next frame.
    focus_filter: bool,
    /// Where the controls were drawn (`key:<id>`, `add:<id>`, `clear:<id>`, `reset:<id>`,
    /// `reset-all`), for tests.
    pub rects: Vec<(String, Rect)>,
}

/// The question “<keys> is used by “<action>”. Overwrite it?” (KEY-018).
pub struct Confirm {
    action: Action,
    binding: Binding,
    adding: bool,
    /// The actions the binding clashes with.
    clashes: Vec<Action>,
}

/// `“A”`, `“A” and “B”`, `“A”, “B” and “C”`.
fn names(actions: &[Action]) -> String {
    let quoted: Vec<String> = actions.iter().map(|a| format!("“{}”", a.def().label)).collect();
    match quoted.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
        _ => quoted.join(""),
    }
}

impl KeyDialog {
    /// A binding is being recorded or asked about: the keys belong to that, not to a field.
    pub fn capturing(&self) -> bool {
        self.recording.is_some() || self.confirm.is_some()
    }

    /// Make sure the row of `a` is in the list: a filter that hides it is cleared (a result of the
    /// search of the whole dialog goes to the action, CFG-072).
    pub fn reveal(&mut self, a: Action, keymap: &Keymap) {
        if !action_matches(a.def().label, &bound_keys(keymap, a), &terms(&self.filter)) {
            self.filter.clear();
        }
    }

    fn start(&mut self, a: Action, adding: bool) {
        self.confirm = None;
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
            // Ctrl, Alt, Shift and Super pressed on their own are the start of a chord, not a chord
            if crate::keys::is_modifier_key(*key) {
                continue;
            }
            let Some((action, chords)) = &mut self.recording else { break };
            let bare = !modifiers.any();
            if bare && *key == Key::Escape {
                self.recording = None;
            } else if bare && *key == Key::Enter && !chords.is_empty() {
                let binding = Binding(std::mem::take(chords));
                let action = *action;
                self.recording = None;
                match keys.check_rules(action, &binding) {
                    Err(msg) => self.error = Some(msg),
                    Ok(()) => {
                        let clashes = keys.clashes_of(action, &binding);
                        if clashes.is_empty() {
                            saved = self.apply(keys, action, binding, self.adding);
                        } else {
                            self.error = None;
                            self.confirm = Some(Confirm { action, binding, adding: self.adding, clashes });
                        }
                    }
                }
            } else {
                chords.push(Chord::new(*modifiers, *key));
            }
        }
        saved
    }

    /// The answer to the question: Yes overwrites, No changes nothing.
    fn answer(&mut self, keys: &mut Keymap, yes: bool, changed: &mut bool) {
        if let Some(c) = self.confirm.take() {
            if yes {
                *changed |= self.apply(keys, c.action, c.binding, c.adding);
            }
        }
    }

    /// Save the binding, taking it from the actions it clashes with. Returns true when it was saved.
    fn apply(&mut self, keys: &mut Keymap, action: Action, binding: Binding, adding: bool) -> bool {
        match keys.overwrite(action, binding, adding) {
            Ok(()) => {
                self.error = None;
                true
            }
            Err(msg) => {
                self.error = Some(msg);
                false
            }
        }
    }

    /// The keys of one frame while the question is open: Y is Yes; Enter, Esc and N are No (No has the
    /// keyboard focus). Returns `Some(answer)` when it was answered.
    fn answer_keys(events: &[Event]) -> Option<bool> {
        events.iter().find_map(|e| {
            let Event::Key { key, pressed: true, repeat: false, modifiers, .. } = e else { return None };
            if modifiers.any() {
                return None;
            }
            match key {
                Key::Y => Some(true),
                Key::Enter | Key::Escape | Key::N => Some(false),
                _ => None,
            }
        })
    }

    /// Draw the question. Returns `Some(answer)` when a button was clicked.
    fn draw_confirm(&mut self, ctx: &egui::Context) -> Option<bool> {
        let c = self.confirm.as_ref()?;
        let text = format!("{} is used by {}. Overwrite it?", c.binding.display(), names(&c.clashes));
        let mut answer = None;
        let mut rects = vec![];
        egui::Window::new("Overwrite the key?").id(egui::Id::unique(("keys", "overwrite"))).order(egui::Order::Foreground).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.set_max_width(380.0);
            ui.label(text);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let yes = ui.button("Yes");
                rects.push(("overwrite-yes".to_string(), yes.rect));
                let no = ui.button("No");
                rects.push(("overwrite-no".to_string(), no.rect));
                no.request_focus();
                if yes.clicked() {
                    answer = Some(true);
                } else if no.clicked() {
                    answer = Some(false);
                }
            });
        });
        self.rects.extend(rects);
        answer
    }
}

/// Draw the tab's content. Returns true when a binding changed (the caller saves).
pub fn show(ui: &mut Ui, d: &mut KeyDialog, keys: &mut Keymap) -> bool {
    let mut changed = false;
    d.rects.clear();
    if d.confirm.is_some() {
        // the question takes every key: nothing else may trigger and Esc must not close the dialog
        let events = ui.input(|i| i.events.clone());
        if let Some(yes) = KeyDialog::answer_keys(&events) {
            d.answer(keys, yes, &mut changed);
        }
        ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
    }
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
    // the clash note, once all keys are released (KEY-018): informs only
    let note = d.recording.as_ref().filter(|(_, chords)| !chords.is_empty() && ui.input(|i| i.keys_down.is_empty())).and_then(|(a, chords)| {
        let b = Binding(chords.clone());
        let clashes = if keys.check_rules(*a, &b).is_ok() { keys.clashes_of(*a, &b) } else { vec![] };
        (!clashes.is_empty()).then(|| format!("{} would clash with {}. Enter then asks whether to overwrite it.", b.display(), names(&clashes)))
    });
    crate::theme::prose(ui, muted("Press Change (replaces the keys) or Add (one more), then the keys: one key or chord, or several in a row (a sequence). Enter saves, Esc cancels."));
    if let Some(n) = note {
        crate::theme::prose(ui, muted(n));
    }
    if let Some(e) = &d.error {
        ui.label(RichText::new(e.as_str()).color(theme::palette().link_missing));
    }
    let capturing = d.capturing();
    if !capturing && crate::settings::find::take_search_keys(ui) {
        d.focus_filter = true;
    }
    let filter_id = egui::Id::unique("keys-filter-field");
    // Esc in the field with text in it clears the text; the dialog stays
    if ui.ctx().memory(|m| m.has_focus(filter_id)) && !d.filter.is_empty() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
        d.filter.clear();
    }
    let r = ui
        .horizontal(|ui| {
            let field = ui.add_enabled(!capturing, egui::TextEdit::singleline(&mut d.filter).id(filter_id).hint_text("Filter by name or key, such as ctrl+h or F6  (Ctrl+F)").desired_width(260.0));
            crate::settings::find::keep_focus_on_esc(ui.ctx(), filter_id);
            if std::mem::take(&mut d.focus_filter) {
                field.request_focus();
            }
            ui.button("Reset all").on_hover_text("Puts every action back to its keys, also those the filter does not show")
        })
        .inner;
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
    // the actions that fit the filter, by scope; a group with none is not shown (KEY-006)
    let wanted = terms(&d.filter);
    let shown: Vec<_> = DEFS.iter().filter(|def| action_matches(def.label, &bound_keys(keys, def.action), &wanted)).collect();
    if shown.is_empty() {
        crate::theme::prose(ui, muted("No action matches"));
    }
    egui::Grid::new("keys-grid").num_columns(3).spacing([14.0, 4.0]).show(ui, |ui| {
        for scope in Scope::ALL.into_iter().filter(|s| shown.iter().any(|d| d.scope == *s)) {
            ui.allocate_space(egui::vec2(0.0, 8.0));
            ui.end_row();
            ui.label(RichText::new(scope.label()).strong());
            ui.end_row();
            for def in shown.iter().filter(|d| d.scope == scope) {
                // long names wrap: the page sits next to the tab list of the settings
                let name = ui.scope(|ui| {
                    ui.set_max_width(180.0);
                    ui.add(egui::Label::new(def.label).wrap());
                });
                match &d.recording {
                    Some((a, chords)) if *a == def.action => {
                        let typed = Binding(chords.clone()).display();
                        ui.label(RichText::new(if chords.is_empty() { "press the keys…".to_string() } else { typed }).color(theme::palette().accent));
                    }
                    _ => {
                        let text = keys.display(def.action);
                        // several keys wrap, so that the buttons stay inside the dialog (KEY-039)
                        ui.scope(|ui| {
                            ui.set_max_width(140.0);
                            ui.add(egui::Label::new(if keys.is_default(def.action) { RichText::new(text) } else { RichText::new(format!("{text}  (changed)")).color(theme::palette().accent) }).wrap());
                        });
                    }
                }
                let buttons = ui.horizontal(|ui| {
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
                anchor::mark(ui, &action_anchor(def), name.response.rect.union(buttons.response.rect));
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
    if let Some(yes) = d.draw_confirm(ui.ctx()) {
        d.answer(keys, yes, &mut changed);
    }
    keys.recording = d.recording.is_some() || d.confirm.is_some();
    changed
}

fn muted(t: impl Into<String>) -> RichText {
    RichText::new(t).color(theme::palette().muted)
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
            // these tests drive the Linux/Windows defaults; macOS has other keys for six actions (KEY-019)
            app.keymap = crate::keys::Keymap::from_overrides_for(&app.cfg.keybindings, false);
            app.open(Path::new("A.md"), false);
            let mut w = W { app, ctx: egui::Context::default(), t: 0.0, dir, texts: vec![], text_pos: vec![], size: Vec2::new(900.0, 2000.0) };
            // the style of the app: the rows of the dialog are a little taller than egui's own
            crate::theme::apply(&w.ctx);
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
            self.app.tabbar.tabs[self.app.tabbar.active].reading
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

    // spec: KEY-006, KEY-007
    #[test]
    fn key_006_the_modifier_keys_of_a_chord_are_not_recorded() {
        let mut w = W::new("ctrl-alt-1");
        w.open_dialog();
        w.click("key:global.ribbon_1");
        // a real keyboard (X11, Wayland) reports Ctrl and Alt as keys of their own before the digit
        let key = |key, modifiers| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers };
        w.frame(vec![key(Key::ControlLeft, Modifiers::CTRL)]);
        w.frame(vec![key(Key::AltLeft, CTRL_ALT)]);
        w.frame(vec![key(Key::Num1, CTRL_ALT)]);
        assert_eq!(w.dialog().recording.as_ref().map(|r| Binding(r.1.clone()).display()), Some("Ctrl+Alt+1".to_string()), "only the chord is recorded");
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error, None);
        assert!(w.dialog().confirm.is_none());
        assert_eq!(w.app.cfg.keybindings.get("global.ribbon_1"), Some(&vec!["Ctrl+Alt+1".to_string()]));
        assert_eq!(w.app.keymap.display(Action::RibbonSlot1), "Ctrl+Alt+1");
    }

    fn shows(w: &W, needle: &str) -> bool {
        w.texts.iter().any(|t| t.contains(needle))
    }

    // spec: KEY-004, KEY-005, KEY-018
    #[test]
    fn a_binding_that_breaks_a_rule_is_refused_with_its_message_and_no_dialog() {
        let mut w = W::new("double");
        w.open_dialog();
        // a plain key is no global shortcut
        w.click("key:global.back");
        w.press(Key::G, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().error.as_deref().unwrap().contains("needs Ctrl or Alt"));
        assert!(w.dialog().confirm.is_none(), "no dialog for a refusal that is no clash");
        // reserved for text editing
        w.click("key:global.back");
        w.press(Key::C, CTRL);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().error.as_deref().unwrap().contains("reserved"));
        assert!(w.dialog().confirm.is_none());
        // a fixed meaning
        w.click("key:files.toggle_folder");
        w.press(Key::Enter, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().error.as_deref().unwrap().contains("fixed meaning"));
        assert!(w.dialog().confirm.is_none());
        assert!(w.app.cfg.keybindings.is_empty());
        // a sequence can be recorded in a list scope
        w.click("key:files.toggle_folder");
        w.press(Key::Space, Modifiers::NONE);
        w.press(Key::F, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error, None);
        assert_eq!(w.app.cfg.keybindings.get("files.toggle_folder"), Some(&vec!["Space F".to_string()]));
    }

    // spec: KEY-018, KEY-004
    #[test]
    fn a_clashing_key_asks_to_overwrite_and_no_changes_nothing() {
        let mut w = W::new("ask-no");
        w.open_dialog();
        w.click("key:global.focus_editor");
        w.press(Key::H, CTRL);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().confirm.is_some(), "the question is open");
        assert_eq!(w.dialog().error, None);
        assert!(shows(&w, "Ctrl+H is used by “Keyboard one place to the left (pane, then left sidebar)”. Overwrite it?"), "{:?}", w.texts);
        assert!(w.dialog().rects.iter().any(|r| r.0 == "overwrite-yes") && w.dialog().rects.iter().any(|r| r.0 == "overwrite-no"));
        assert!(w.app.keymap.recording, "the keys trigger nothing meanwhile");
        // keys do nothing else while it is open: Ctrl+E would switch the view
        w.press(Key::E, CTRL);
        assert!(!w.reading());
        assert!(w.dialog().confirm.is_some());
        // Enter is No
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().confirm.is_none());
        assert!(w.app.keymap.is_default(Action::FocusEditor) && w.app.keymap.is_default(Action::FocusSidebar), "nothing changed");
        assert!(w.app.cfg.keybindings.is_empty());
        assert!(!w.app.keymap.recording);
        // a click on No, and N
        for answer in ["click", "n"] {
            w.click("key:global.focus_editor");
            w.press(Key::H, CTRL);
            w.press(Key::Enter, Modifiers::NONE);
            assert!(w.dialog().confirm.is_some());
            if answer == "n" {
                w.press(Key::N, Modifiers::NONE);
            } else {
                w.click("overwrite-no");
            }
            assert!(w.dialog().confirm.is_none(), "{answer}");
            assert!(w.app.cfg.keybindings.is_empty(), "{answer}");
        }
    }

    // spec: KEY-018, KEY-008, KEY-007
    #[test]
    fn yes_takes_the_key_from_the_other_action_and_the_new_one_works_at_once() {
        for answer in ["y", "click"] {
            let mut w = W::new(&format!("ask-yes-{answer}"));
            w.open_dialog();
            // Ctrl+N is “New note”; give it to the reading view action
            w.click("key:global.toggle_reading");
            w.press(Key::N, CTRL);
            w.press(Key::Enter, Modifiers::NONE);
            assert!(shows(&w, "Ctrl+N is used by “New note”. Overwrite it?"), "{:?}", w.texts);
            if answer == "y" {
                w.press(Key::Y, Modifiers::NONE);
            } else {
                w.click("overwrite-yes");
            }
            assert!(w.dialog().confirm.is_none());
            assert_eq!(w.dialog().error, None);
            assert_eq!(w.app.keymap.display(Action::NewNote), "not set");
            assert_eq!(w.app.keymap.display(Action::ToggleReading), "Ctrl+N");
            assert_eq!(w.app.cfg.keybindings.get("global.new_note"), Some(&vec![]), "stored as cleared");
            assert_eq!(w.app.cfg.keybindings.get("global.toggle_reading"), Some(&vec!["Ctrl+N".to_string()]));
            assert!(shows(&w, "not set"), "the tab shows it");
            w.app.modal = None;
            w.frame(vec![]);
            let tabs = w.app.tabbar.tabs.len();
            w.press(Key::N, CTRL);
            assert!(w.reading(), "the edited action fires on the key");
            assert_eq!(w.app.tabbar.tabs.len(), tabs, "and New note no longer does");
            assert!(w.app.notes.len() <= 1 + tabs);
        }
    }

    // spec: KEY-018
    #[test]
    fn esc_on_the_question_is_no_and_the_settings_stay_open() {
        let mut w = W::new("ask-esc");
        w.open_dialog();
        w.click("key:global.focus_editor");
        w.press(Key::H, CTRL);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(w.dialog().confirm.is_some());
        w.press(Key::Escape, Modifiers::NONE);
        assert!(w.dialog().confirm.is_none(), "Esc is No");
        assert!(matches!(w.app.modal, Some(Modal::Settings { .. })), "the settings window stays open");
        assert!(w.app.cfg.keybindings.is_empty());
    }

    // spec: KEY-018
    #[test]
    fn a_prefix_clash_names_every_action_and_yes_takes_them_all() {
        let mut w = W::new("ask-prefix");
        w.app.keymap.set(Action::TreeToggleFolder, Binding::parse("Space f f").unwrap()).unwrap();
        w.app.keymap.set(Action::TreePrevFile, Binding::parse("Space f g").unwrap()).unwrap();
        w.open_dialog();
        w.click("key:files.next_file");
        w.press(Key::Space, Modifiers::NONE);
        w.press(Key::F, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(shows(&w, "Space f is used by “Previous file” and “Open or close the folder”. Overwrite it?"), "{:?}", w.texts);
        w.press(Key::Y, Modifiers::NONE);
        assert_eq!(w.app.keymap.display(Action::TreeToggleFolder), "not set");
        assert_eq!(w.app.keymap.display(Action::TreePrevFile), "not set");
        assert_eq!(w.app.keymap.display(Action::TreeNextFile), "Space f");
        // Add: the edited action keeps its other keys, the other one loses only the clashing one
        w.click("add:files.next_file");
        w.press(Key::J, CTRL);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(shows(&w, "Ctrl+J is used by “Next folder”. Overwrite it?"), "{:?}", w.texts);
        w.press(Key::Y, Modifiers::NONE);
        assert_eq!(w.app.keymap.display(Action::TreeNextFolder), "not set");
        assert_eq!(w.app.keymap.display(Action::TreeNextFile), "Space f / Ctrl+J");
    }

    // spec: KEY-018
    #[test]
    fn the_clash_note_shows_once_all_keys_are_released_and_not_while_one_is_held() {
        let mut w = W::new("note");
        w.open_dialog();
        w.click("key:global.focus_editor");
        let ev = |pressed| Event::Key { key: Key::H, physical_key: None, pressed, repeat: false, modifiers: CTRL };
        w.frame(vec![ev(true)]);
        w.frame(vec![]);
        assert!(!shows(&w, "would clash"), "H is still held: {:?}", w.texts);
        w.frame(vec![ev(false)]);
        assert!(shows(&w, "Ctrl+H would clash with “Keyboard one place to the left (pane, then left sidebar)”"), "{:?}", w.texts);
        assert!(w.dialog().confirm.is_none(), "the note only informs");
        // a key that clashes with nothing has no note
        w.click("key:global.focus_editor");
        w.press(Key::E, CTRL_ALT);
        assert!(!shows(&w, "would clash"), "{:?}", w.texts);
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
            .tabbar
            .tabs
            .iter()
            .map(|t| match &t.view {
                View::Note(p) => p.to_string_lossy().replace('\\', "/"),
                View::Calendar => "calendar".into(),
                View::Mail => "mail".into(),
                _ => "other".into(),
            })
            .collect()
    }

    // spec: NAV-018
    #[test]
    fn ctrl_page_up_and_down_switch_tabs_and_wrap() {
        let mut w = W::new("tabs");
        // a single tab: nothing happens
        w.press(Key::PageDown, CTRL);
        w.press(Key::PageUp, CTRL);
        assert_eq!((w.app.tabbar.tabs.len(), w.app.tabbar.active), (1, 0));
        w.app.open(Path::new("B.md"), true);
        w.app.show_view(View::Calendar, true);
        assert_eq!((views(&w), w.app.tabbar.active), (vec!["A.md".into(), "B.md".into(), "calendar".into()], 2));
        // Next from the last tab wraps to the first, Previous from the first to the last
        w.press(Key::PageDown, CTRL);
        assert_eq!(w.app.tabbar.active, 0);
        w.press(Key::PageDown, CTRL);
        assert_eq!(w.app.tabbar.active, 1);
        w.press(Key::PageUp, CTRL);
        assert_eq!(w.app.tabbar.active, 0);
        w.press(Key::PageUp, CTRL);
        assert_eq!(w.app.tabbar.active, 2, "wraps to the last tab, a sub-app");
        // from a sub-app (calendar is open now) and with the keyboard in the file list
        w.press(Key::PageUp, CTRL);
        assert_eq!(w.app.tabbar.active, 1);
        w.press(Key::H, CTRL);
        assert!(w.app.tree.sel.is_some(), "Ctrl+H: the file list has the keyboard");
        w.press(Key::PageDown, CTRL);
        assert_eq!(w.app.tabbar.active, 2, "works from the file list too, and Ctrl+L did not take it");
        // the order is the order the tabs are shown in (pinned and dragged tabs sit in `tabs`)
        w.app.tabbar.tabs.swap(0, 2);
        w.app.tabbar.active = 0;
        assert_eq!(views(&w)[0], "calendar");
        w.press(Key::PageDown, CTRL);
        assert_eq!(views(&w)[w.app.tabbar.active], "B.md");
        // a pinned tab sits first, and so it comes first in the order
        let a = w.app.tabbar.tabs.iter().position(|t| t.view == View::Note("A.md".into())).unwrap();
        w.app.set_tab_pinned(a, true);
        assert_eq!(views(&w), ["A.md", "calendar", "B.md"]);
        w.app.tabbar.active = 0;
        w.press(Key::PageUp, CTRL);
        assert_eq!(views(&w)[w.app.tabbar.active], "B.md", "Previous from the pinned first tab wraps to the last");
        w.press(Key::PageDown, CTRL);
        w.press(Key::PageDown, CTRL);
        assert_eq!(views(&w)[w.app.tabbar.active], "calendar");
        w.app.tabbar.active = views(&w).iter().position(|v| v == "B.md").unwrap();
        // another binding: the old key does nothing
        w.app.keymap.set(Action::NextTab, Binding::parse("Ctrl+Alt+N").unwrap()).unwrap();
        w.press(Key::PageDown, CTRL);
        assert_eq!(views(&w)[w.app.tabbar.active], "B.md", "the old key is gone");
        w.press(Key::N, CTRL_ALT);
        assert_eq!(views(&w)[w.app.tabbar.active], "A.md", "the new one works");
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
        crate::note_props::unstamped(&w.app.notes[Path::new("A.md")].text)
    }

    fn settle(w: &mut W) {
        for _ in 0..3 {
            w.frame(vec![]);
        }
        // a search on the worker (QUERY-014) is waited for
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while w.app.search_worker.busy() {
            assert!(std::time::Instant::now() < end, "the search worker never answered");
            std::thread::sleep(std::time::Duration::from_millis(2));
            w.frame(vec![]);
        }
    }

    /// `settle`, then more frames until `done` holds (at most 10 s). `settle` alone is not enough
    /// for a search: the worker only gets the query after the debounce, measured from the last
    /// list made, and on a slow or loaded machine (CI, #659) the answer can still be on its way
    /// when `settle` sees the worker idle.
    fn settle_until(w: &mut W, what: &str, done: impl Fn(&W) -> bool) {
        settle(w);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !done(w) {
            assert!(std::time::Instant::now() < end, "never reached: {what}");
            std::thread::sleep(std::time::Duration::from_millis(5));
            w.frame(vec![]);
        }
        settle(w);
    }

    /// Vim-style editing on, the open note's editor has the keyboard (normal mode).
    fn vim_note(tag: &str) -> W {
        let mut w = W::new(tag);
        w.app.cfg.vim_mode = true;
        w.app.focus_editor();
        settle(&mut w);
        assert!(w.app.tabbar.tabs[0].editor.vim_normal_keys(&w.ctx), "the editor has the keyboard in normal mode");
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
        typed(&mut w, Key::Q, "q");
        assert_eq!(note_text(&w), "# A\n", "q is no vim key either");
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::I, "i");
        assert!(w.app.tabbar.tabs[0].editor.vim.mode == crate::vim::Mode::Insert, "i after a cancelled Space starts insert mode");
        // insert mode: Space is typing again
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::X, "x");
        assert!(note_text(&w).contains(" x"), "{:?}", note_text(&w));
        // a vim command line takes every key
        w.press(Key::Escape, Modifiers::NONE);
        typed(&mut w, Key::Colon, ":");
        typed(&mut w, Key::Space, " ");
        assert!(w.app.tabbar.tabs[0].editor.vim.typing_command());
        assert!(!w.app.tabbar.tabs[0].editor.vim_normal_keys(&w.ctx));
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
        // the Keybindings tab sits in the settings next to the Vault one, which keeps the vault folder
        assert!(w.texts.iter().any(|t| t == "Vault") && w.texts.iter().any(|t| t == "Keybindings"), "{:?}", w.texts);
        assert!(w.texts.iter().any(|t| t.contains("Press Change")));
        assert!(!w.texts.iter().any(|t| t == "Vault folder"));
        // Add: j and Down stay, the new key joins them
        w.click("add:files.next_file");
        w.press(Key::J, CTRL_ALT);
        w.press(Key::Enter, Modifiers::NONE);
        assert_eq!(w.dialog().error, None);
        assert_eq!(w.app.cfg.keybindings.get("files.next_file"), Some(&vec!["J".to_string(), "Down".to_string(), "Ctrl+Alt+J".to_string()]));
        // an added binding that clashes asks first (KEY-018); No changes nothing
        w.click("add:files.next_file");
        w.press(Key::K, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        assert!(shows(&w, "k is used by “Previous file”. Overwrite it?"), "{:?}", w.texts);
        w.press(Key::N, Modifiers::NONE);
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

    /// Is the search of everything (Space f g) open, or the file finder (Space f f)?
    fn open_source(w: &W) -> Option<crate::picker::Source> {
        match &w.app.modal {
            Some(Modal::Picker(p)) => Some(p.source),
            _ => None,
        }
    }

    fn space_f(w: &mut W, second: (Key, &str)) {
        for (key, text) in [(Key::Space, " "), (Key::F, "f"), second] {
            typed(w, key, text);
        }
        settle(w);
    }

    // spec: KEY-041, KEY-040
    #[test]
    fn key_041_space_f_g_and_space_f_f_work_in_every_main_view_without_vim_mode() {
        let mut w = W::new("main-views");
        assert!(!w.app.cfg.vim_mode);
        // a note in the reading view, then the other views
        w.app.toggle_reading_view();
        settle(&mut w);
        let views = [None, Some(View::Calendar), Some(View::Mail), Some(View::Social(None)), Some(View::Graph), Some(View::Tasks), Some(View::Empty)];
        for view in views {
            if let Some(v) = view.clone() {
                w.app.show_view(v, false);
                settle(&mut w);
            }
            for (second, source) in [((Key::G, "g"), crate::picker::Source::Everything), ((Key::F, "f"), crate::picker::Source::FileNames)] {
                space_f(&mut w, second);
                assert_eq!(open_source(&w), Some(source), "{view:?}");
                w.app.modal = None;
                settle(&mut w);
            }
        }
        // the editor: vim normal mode only (the text box has the keyboard otherwise)
        let mut w = vim_note("main-views-vim");
        space_f(&mut w, (Key::G, "g"));
        assert_eq!(open_source(&w), Some(crate::picker::Source::Everything));
        assert_eq!(note_text(&w), "# A\n", "nothing was typed");
        // a drawing keeps Space for panning
        let mut w = W::new("main-views-drawing");
        w.app.open(Path::new("Map.excalidraw"), false);
        settle(&mut w);
        space_f(&mut w, (Key::G, "g"));
        assert_eq!(open_source(&w), None);
    }

    // spec: KEY-042, KEY-038
    #[test]
    fn key_042_not_while_a_text_field_has_the_keyboard_and_not_under_a_dialog() {
        let mut w = W::new("main-views-blocked");
        w.app.show_view(View::Mail, false);
        settle(&mut w);
        // the search field of the mail view has the keyboard: the keys are typed
        let search = egui::Id::unique("mail-search");
        w.ctx.memory_mut(|m| m.request_focus(search));
        settle(&mut w);
        assert!(w.ctx.text_edit_focused(), "the mail search field has the keyboard");
        space_f(&mut w, (Key::G, "g"));
        assert_eq!(open_source(&w), None);
        // and the editor of a note without vim mode: typing
        let mut w = W::new("main-views-typing");
        w.app.focus_editor();
        settle(&mut w);
        assert!(w.ctx.text_edit_focused());
        space_f(&mut w, (Key::G, "g"));
        assert_eq!(open_source(&w), None);
        assert!(note_text(&w).contains(" fg"), "the keys were typed: {:?}", note_text(&w));
        // under a modal dialog nothing fires, a half typed sequence is forgotten
        let mut w = W::new("main-views-modal");
        w.app.show_view(View::Calendar, false);
        settle(&mut w);
        typed(&mut w, Key::Space, " ");
        w.app.open_settings(SettingsTab::Vault);
        settle(&mut w);
        let is_settings = |w: &W| matches!(w.app.modal, Some(Modal::Settings { .. }));
        for second in [(Key::G, "g"), (Key::F, "f")] {
            space_f(&mut w, second);
            assert!(is_settings(&w), "the dialog stays: no picker over it");
        }
        w.app.modal = None;
        settle(&mut w);
        typed(&mut w, Key::F, "g");
        assert_eq!(open_source(&w), None, "the Space before the dialog was forgotten");
        space_f(&mut w, (Key::G, "g"));
        assert_eq!(open_source(&w), Some(crate::picker::Source::Everything), "works again with the dialog closed");
    }

    // spec: KEY-043, MAIL-061
    #[test]
    fn key_043_in_the_mail_view_space_is_a_leader_and_its_single_keys_still_work() {
        let mut w = W::new("main-views-mail");
        w.app.show_view(View::Mail, false);
        settle(&mut w);
        space_f(&mut w, (Key::G, "g"));
        assert_eq!(open_source(&w), Some(crate::picker::Source::Everything));
        w.app.modal = None;
        settle(&mut w);
        // Space F is not the flag key f: the f after the Space belongs to the sequence
        // a single key of the mail view still reaches it: `/` focuses the search, no sequence is pending
        typed(&mut w, Key::Slash, "/");
        settle(&mut w);
        assert!(w.ctx.text_edit_focused(), "the mail key `/` still opens the search field");
        assert_eq!(w.app.keymap.pending_text(w.t), None);
        // Space then another key: the sequence ends, the key works as a mail key afterwards
        let mut w = W::new("main-views-mail2");
        w.app.show_view(View::Mail, false);
        settle(&mut w);
        typed(&mut w, Key::Space, " ");
        assert!(w.app.keymap.pending_text(w.t).is_some(), "Space started a sequence");
        typed(&mut w, Key::Slash, "/");
        settle(&mut w);
        assert_eq!(open_source(&w), None);
        assert!(w.ctx.text_edit_focused(), "the key that did not continue the sequence worked as a mail key");
    }

    // spec: KEY-044, KEY-001
    #[test]
    fn key_044_the_keybindings_tab_shows_the_main_views_scope_with_its_keys() {
        let mut w = W::new("main-views-tab");
        w.open_dialog();
        // the filter of the list leaves the two actions and the heading of their scope
        if let Some(Modal::Settings { keys, .. }) = &mut w.app.modal {
            keys.filter = "Space f".to_string();
        }
        settle(&mut w);
        assert!(shows(&w, Scope::MainViews.label()), "{:?}", w.texts);
        assert!(shows(&w, "Search notes, mail and posts") && shows(&w, "Find a file by name"), "{:?}", w.texts);
        assert!(w.dialog().rects.iter().any(|r| r.0 == "key:normal.search_all"));
        // they are rebindable there, and a clash with a mail key is refused with its name
        w.app.keymap.set(Action::NormalSearchAll, Binding::parse("Space s").unwrap()).unwrap();
        assert_eq!(w.app.keymap.display(Action::NormalSearchAll), "Space s");
        assert!(w.app.keymap.set(Action::MailMark, Binding::parse("Space s").unwrap()).unwrap_err().contains("already used by “Search notes, mail and posts”"));
    }

    fn set_note(w: &mut W, text: &str) {
        w.app.notes.get_mut(Path::new("A.md")).unwrap().text = text.to_string();
        settle(w);
    }

    fn space_space(w: &mut W) {
        typed(w, Key::Space, " ");
        typed(w, Key::Space, " ");
        settle(w);
    }

    fn toasted(w: &W, text: &str) -> bool {
        w.app.toasts.iter().any(|t| t.0.contains(text))
    }

    // spec: KEY-045, TASK-003
    #[test]
    fn key_045_space_space_ticks_the_task_on_the_cursor_line_and_undoes() {
        let mut w = vim_note("toggle-task");
        set_note(&mut w, "# A\n- [ ] one\n- [x] two\nplain\n");
        typed(&mut w, Key::G, "g");
        typed(&mut w, Key::G, "g");
        typed(&mut w, Key::J, "j");
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n- [x] one\n- [x] two\nplain\n", "ticked");
        assert!(w.app.notes[Path::new("A.md")].dirty || std::fs::read_to_string(w.dir.join("A.md")).unwrap().contains("[x] one"), "saved with the note");
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n- [ ] one\n- [x] two\nplain\n", "unticked again");
        // the next line: a task that is done is unticked
        typed(&mut w, Key::J, "j");
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n- [ ] one\n- [ ] two\nplain\n");
        // u undoes the last toggle as one step
        typed(&mut w, Key::U, "u");
        settle(&mut w);
        assert_eq!(note_text(&w), "# A\n- [ ] one\n- [x] two\nplain\n", "undone");
    }

    // spec: KEY-045
    #[test]
    fn key_045_space_space_without_a_task_or_while_typing_changes_nothing() {
        let mut w = vim_note("toggle-task-none");
        set_note(&mut w, "# A\n- [ ] one\nplain\n");
        typed(&mut w, Key::G, "g");
        typed(&mut w, Key::G, "g");
        // line 0 is a heading: nothing changes and the status bar says so
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n- [ ] one\nplain\n");
        assert_eq!(w.app.tabbar.tabs[0].editor.vim.message.as_deref(), Some("No task on this line"));
        // insert mode: Space types a space, the task stays
        typed(&mut w, Key::J, "j");
        typed(&mut w, Key::I, "i");
        typed(&mut w, Key::Space, " ");
        typed(&mut w, Key::Space, " ");
        settle(&mut w);
        assert!(note_text(&w).contains("- [ ] one"), "not ticked: {:?}", note_text(&w));
        assert!(note_text(&w).contains("  "), "the spaces were typed: {:?}", note_text(&w));
    }

    // spec: KEY-045, TASK-003
    #[test]
    fn key_045_space_space_in_the_reading_view_and_the_task_list_takes_the_task_under_the_pointer() {
        let mut w = W::new("toggle-task-views");
        set_note(&mut w, "# A\n\n- [ ] buy milk\n\nplain\n");
        w.app.toggle_reading_view();
        settle(&mut w);
        let row = |w: &W, name: &str| w.text_pos.iter().find(|t| t.0.contains(name)).map(|t| t.1 + Vec2::new(4.0, 6.0)).unwrap_or_else(|| panic!("no {name}: {:?}", w.texts));
        // the pointer is not on a task: nothing
        w.frame(vec![Event::PointerMoved(row(&w, "plain"))]);
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n\n- [ ] buy milk\n\nplain\n");
        assert!(toasted(&w, "No task here"));
        // on the task row
        w.frame(vec![Event::PointerMoved(row(&w, "buy milk"))]);
        w.frame(vec![]);
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n\n- [x] buy milk\n\nplain\n");
        // the save of the tick added the properties above the task, which the reading view shows: the task moved down
        settle(&mut w);
        w.frame(vec![Event::PointerMoved(row(&w, "buy milk"))]);
        w.frame(vec![]);
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n\n- [ ] buy milk\n\nplain\n", "again unticks");
        // the task list: the task row under the pointer, in the buffer of its note
        w.app.show_view(View::Tasks, false);
        settle(&mut w);
        w.frame(vec![Event::PointerMoved(row(&w, "buy milk"))]);
        w.frame(vec![]);
        space_space(&mut w);
        assert_eq!(note_text(&w), "# A\n\n- [x] buy milk\n\nplain\n");
    }

    fn kind(w: &W) -> &'static str {
        match w.app.tabbar.tabs[w.app.tabbar.active].view {
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
        w.app.open_settings(SettingsTab::Vault);
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
        assert_eq!((views(&w), w.app.tabbar.tabs.len()), (vec!["projects/Roadmap.md".to_string()], 1), "opened in the current tab");
        assert!(w.app.tabbar.tabs[0].editor.vim_normal_keys(&w.ctx), "the keyboard is in the editor, in normal mode");
        assert_eq!(w.app.notes[Path::new("A.md")].text, "# A\n");
    }

    // spec: DATA-001
    #[test]
    fn data_001_ctrl_s_saves_a_note_that_was_edited_just_now() {
        let mut w = W::new("ctrl-s");
        let doc = w.app.notes.get_mut(Path::new("A.md")).unwrap();
        doc.text = "# A\nedited\n".to_string();
        doc.dirty = true;
        doc.last_edit = std::time::Instant::now();
        w.frame(vec![]);
        assert_eq!(std::fs::read_to_string(w.dir.join("A.md")).unwrap(), "# A\n", "the autosave waits 0.8 s");
        w.press(Key::S, CTRL);
        assert_eq!(crate::note_props::unstamped(&std::fs::read_to_string(w.dir.join("A.md")).unwrap()), "# A\nedited\n", "Ctrl+S saves at once");
        assert!(!w.app.notes[Path::new("A.md")].dirty);
    }

    // spec: USE-001
    #[test]
    fn use_001_ctrl_shift_d_opens_the_data_usage_dialog() {
        let mut w = W::new("data-usage-key");
        settle(&mut w);
        assert!(w.app.modal.is_none());
        w.press(Key::D, Modifiers { shift: true, ..CTRL });
        assert!(matches!(w.app.modal, Some(Modal::DataUsage)), "the command global.data_usage has Ctrl+Shift+D");
    }

    // spec: NAV-001
    #[test]
    fn nav_001_every_tab_has_its_own_history_that_alt_arrows_and_the_mouse_side_buttons_walk() {
        let mut w = W::new("tab-history");
        settle(&mut w);
        let first = w.app.current_path().expect("a note is open");
        w.app.open_link("Roadmap", None, Some(first.clone()), false);
        let second = w.app.current_path().expect("the link opened a note");
        assert_ne!(second, first);
        // a second tab has a history of its own: it is empty, so back does nothing there
        w.app.tabbar.new_tab(View::Calendar);
        settle(&mut w);
        let alt = Modifiers { alt: true, ..Modifiers::NONE };
        w.press(Key::ArrowLeft, alt);
        assert_eq!(w.app.tabbar.tabs[1].view, View::Calendar, "the history of the first tab is not the second tab's");
        assert_eq!(w.app.tabbar.tabs[0].view, View::Note(second.clone()));
        // back and forward in the first tab, by the keys
        w.app.tabbar.active = 0;
        settle(&mut w);
        w.press(Key::ArrowLeft, alt);
        assert_eq!(w.app.current_path(), Some(first.clone()), "Alt+Left goes back");
        w.press(Key::ArrowRight, alt);
        assert_eq!(w.app.current_path(), Some(second.clone()), "Alt+Right goes forward");
        // and by the mouse side buttons
        let side = |button, pressed| Event::PointerButton { pos: Pos2::new(300.0, 300.0), button, pressed, modifiers: Modifiers::NONE };
        w.frame(vec![side(egui::PointerButton::Extra1, true)]);
        w.frame(vec![side(egui::PointerButton::Extra1, false)]);
        assert_eq!(w.app.current_path(), Some(first), "the back button goes back");
        w.frame(vec![side(egui::PointerButton::Extra2, true)]);
        w.frame(vec![side(egui::PointerButton::Extra2, false)]);
        assert_eq!(w.app.current_path(), Some(second), "the forward button goes forward");
    }

    // spec: NAV-005
    #[test]
    fn nav_005_ctrl_o_opens_the_quick_switcher_that_finds_or_creates_a_note() {
        let mut w = W::new("quick-switcher");
        w.app.cfg.date_prefix_new_notes = false;
        assert!(w.app.modal.is_none());
        w.press(Key::O, CTRL);
        settle(&mut w);
        assert!(matches!(w.app.modal, Some(Modal::QuickSwitch { .. })), "Ctrl+O opens the quick switcher");
        // a name that is a note: Enter opens it in the current tab
        w.frame(vec![Event::Text("road".to_string())]);
        settle(&mut w);
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert_eq!((views(&w), w.app.tabbar.tabs.len()), (vec!["projects/Roadmap.md".to_string()], 1));
        // a name that is no note: the last entry creates it
        assert!(!w.dir.join("Zebra.md").exists());
        w.press(Key::O, CTRL);
        settle(&mut w);
        w.frame(vec![Event::Text("Zebra".to_string())]);
        settle(&mut w);
        assert!(w.texts.iter().any(|t| t.contains("Create “Zebra”")), "a name that is no note offers to create it");
        w.press(Key::ArrowDown, Modifiers::NONE);
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert!(w.dir.join("Zebra.md").exists(), "the note is created");
        assert_eq!(views(&w), ["Zebra.md"]);
    }

    /// Open the file picker over a vault of 40 files in a window of 900 x 420 points, so the list is taller than the window.
    fn long_picker(tag: &str) -> W {
        let mut w = vim_note(tag);
        for i in 0..40 {
            std::fs::write(w.dir.join(format!("Note {i:02}.md")), "x\n").unwrap();
        }
        w.app.vault.as_mut().unwrap().rescan();
        w.size = Vec2::new(900.0, 420.0);
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        settle(&mut w);
        w
    }

    /// Where the hint of the picker is painted, and where the first and the last of the listed lines are.
    fn hint_and_rows(w: &W) -> (Pos2, Pos2) {
        let hint = w.text_pos.iter().find(|(t, _)| t.contains(" next · ")).unwrap_or_else(|| panic!("no hint painted: {:?}", w.texts)).1;
        let first = w.text_pos.iter().find(|(t, p)| t.starts_with("Note 0") && p.x > 150.0).expect("a listed line (the file list at the left has the same names)").1;
        (hint, first)
    }

    // spec: NAV-049, NAV-021
    #[test]
    fn nav_049_the_picker_hint_stays_in_view_above_a_list_that_is_longer_than_the_window() {
        let mut w = long_picker("picker-hint");
        assert_eq!(picker(&w).listed().len(), 14, "the list is cut at 14 lines, and still longer than the window");
        let screen = Rect::from_min_size(Pos2::ZERO, w.size);
        let (hint, first) = hint_and_rows(&w);
        assert!(screen.contains(hint), "the hint is inside the window: {hint:?}");
        assert!(hint.y < first.y, "the hint {hint:?} is above the list {first:?}");
        let field = w.text_pos.iter().find(|(t, _)| t.contains("File name")).expect("the field").1;
        assert!(field.y < hint.y, "the field {field:?} is above the hint {hint:?}");
        // moving to the last line scrolls the list, the hint and the field stay where they are
        w.press(Key::ArrowUp, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(picker(&w).sel, 13);
        let (hint2, first2) = hint_and_rows(&w);
        assert_eq!(hint2, hint, "the hint does not move when the list scrolls");
        assert!(first2.y < first.y, "the list scrolled inside the window: {first:?} then {first2:?}");
        // the same window for the other source (NAV-021)
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything)));
        settle(&mut w);
        let hint3 = w.text_pos.iter().find(|(t, _)| t.contains(" next · ")).expect("the hint of the search").1;
        assert!(screen.contains(hint3));
    }

    // spec: NAV-049, KEY-001
    #[test]
    fn nav_049_the_picker_hint_shows_the_keys_as_they_are_bound_now_and_the_rebound_keys_work() {
        let mut w = long_picker("picker-hint-keys");
        let hint = |w: &W| w.texts.iter().find(|t| t.contains(" next · ")).cloned().expect("the hint");
        assert_eq!(hint(&w), "↓ / Ctrl+J next · ↑ / Ctrl+K previous · Enter open · Esc close");
        let key = |t: &str| crate::keys::Binding::parse(t).unwrap();
        w.app.keymap.set(crate::keys::Action::ListNext, key("Ctrl+Alt+N")).unwrap();
        w.app.keymap.clear(crate::keys::Action::ListClose);
        settle(&mut w);
        assert_eq!(hint(&w), "Ctrl+Alt+N next · ↑ / Ctrl+K previous · Enter open");
        w.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(picker(&w).sel, 0, "the old key no longer moves");
        w.press(Key::N, CTRL_ALT);
        assert_eq!(picker(&w).sel, 1, "the new key moves");
        // a key of the field is refused, and the window is closed with the key of the new binding
        assert!(w.app.keymap.check(crate::keys::Action::ListNext, &key("J")).is_err(), "a bare letter is typing");
        w.app.keymap.set(crate::keys::Action::ListClose, key("Ctrl+Q")).unwrap();
        w.press(Key::Q, CTRL);
        assert!(w.app.modal.is_none());
    }

    /// A mail index with two mails ("Banana invoice" and "Lunch") that the app has open.
    fn with_mail(w: &mut W) {
        let root = w.dir.join("mail-store");
        {
            let store = crate::mail::store::MailStore::open(&root).unwrap();
            let folder = store.ensure_folder("acct", "Acct", "INBOX", Some("/"), None, true).unwrap();
            store.commit(vec![crate::mail::db::Op::FolderState { folder: folder.id, uidvalidity: Some(7), uidnext: Some(10), highestmodseq: None, last_sync: 1 }]).unwrap();
            let folder = crate::mail::db::FolderRow { uidvalidity: Some(7), ..folder };
            let mails = [(1u32, "Banana invoice"), (2, "Lunch")];
            let ops = mails.iter().map(|(uid, subject)| {
                let headers = format!("From: Ann <ann@x.de>\r\nSubject: {subject}\r\n\r\n");
                crate::mail::db::Op::message(store.header_only(&folder, 7, *uid, headers.as_bytes(), crate::mail::store::ServerMeta { flags: "", internal_date: Some(1_700_000_000 + i64::from(*uid)), size: 100 }))
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
        let mut p = crate::social::tests::post("b1", "bob", crate::calendar::time::now() - 600, None);
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
        assert_eq!((views(&w), w.app.tabbar.tabs.len()), (vec!["projects/Roadmap.md".to_string()], 1), "opened in this tab");
        assert!(w.app.tabbar.tabs[0].editor.vim_normal_keys(&w.ctx), "the keyboard is in the editor");
        let cursor = egui::text_edit::TextEditState::load(&w.ctx, w.app.tabbar.tabs[0].editor.id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0);
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
        let tabs = w.app.tabbar.tabs.len();
        space_f_g(&mut w);
        search_for(&mut w, "republic");
        assert_eq!(picker_rows(&w), [("Posts", "banana republic".to_string())]);
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert!(w.app.modal.is_none());
        assert_eq!(w.app.tabbar.tabs.len(), tabs + 1, "a new tab");
        let feed = crate::social::FeedRef::new(crate::social::demo::account().account.id, "home");
        assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Social(Some(feed)));
        assert_eq!(w.app.notes[Path::new("A.md")].text, "# A\n");
    }

    /// A note with a hit that has markup in it: a task, bold, a wiki link with an alias and a tag.
    fn with_rich_note(w: &mut W) {
        w.app.vault.as_mut().unwrap().create_file(Path::new("Rich.md"), "- [ ] **zebra** with [[Some note|the alias]] and #idea\n").unwrap();
        w.app.vault.as_mut().unwrap().rescan();
    }

    const RAW_HIT: &str = "- [ ] **zebra** with [[Some note|the alias]] and #idea";
    const RENDERED_HIT: &str = "[ ] zebra with the alias and #idea";

    // spec: NAV-056, NAV-057, NAV-058
    #[test]
    fn nav_056_the_picker_shows_a_note_hit_rendered_or_raw_as_the_editor_does_or_the_setting_says() {
        let mut w = vim_note("hit-display-picker");
        with_rich_note(&mut w);
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything)));
        settle(&mut w);
        search_for(&mut w, "zebra");
        assert_eq!(picker_rows(&w), [("Notes", RAW_HIT.to_string())], "the hit keeps the line as written (the click opens it)");
        // the editor shows the markdown (no preview, edit view): raw
        assert!(!w.app.search_rendered());
        assert!(shows(&w, RAW_HIT), "{:?}", w.texts);
        // the Markdown preview is on: rendered
        w.app.cfg.live_preview = true;
        settle(&mut w);
        assert!(shows(&w, RENDERED_HIT), "{:?}", w.texts);
        assert!(!shows(&w, RAW_HIT));
        // the setting says raw / rendered whatever the editor does
        w.app.cfg.search.hit_display = "raw".into();
        settle(&mut w);
        assert!(shows(&w, RAW_HIT), "{:?}", w.texts);
        w.app.cfg.live_preview = false;
        w.app.cfg.search.hit_display = "rendered".into();
        settle(&mut w);
        assert!(shows(&w, RENDERED_HIT), "{:?}", w.texts);
        // follow the editor again: the reading view of the active note counts as rendered
        w.app.cfg.search.hit_display = "follow".into();
        settle(&mut w);
        assert!(shows(&w, RAW_HIT));
        w.app.tabbar.tabs[0].reading = true;
        settle(&mut w);
        assert!(w.app.search_rendered());
        assert!(shows(&w, RENDERED_HIT), "{:?}", w.texts);
        // a view that is not a note is not a reading view
        w.app.tabbar.tabs[0].reading = false;
        w.app.show_view(View::Calendar, false);
        w.app.tabbar.tabs[w.app.tabbar.active].reading = true;
        assert!(!w.app.search_rendered());
    }

    // spec: NAV-056, NAV-057, NAV-058
    #[test]
    fn nav_056_the_sidebar_shows_a_note_hit_rendered_or_raw_as_the_editor_does() {
        let mut w = W::new("hit-display-sidebar");
        with_rich_note(&mut w);
        w.app.side.left = crate::app::LeftPanel::Search;
        w.app.search.query = "zebra".into();
        w.app.search.focus = true;
        settle(&mut w);
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle(&mut w);
        assert_eq!(sidebar_rows(&w), [("Notes", RAW_HIT.to_string())]);
        assert!(shows(&w, RAW_HIT), "raw while the editor shows the markdown: {:?}", w.texts);
        w.app.cfg.live_preview = true;
        settle(&mut w);
        assert!(shows(&w, RENDERED_HIT), "rendered while the preview is on: {:?}", w.texts);
        assert!(!shows(&w, RAW_HIT));
    }

    /// The groups and titles the sidebar search lists.
    fn sidebar_rows(w: &W) -> Vec<(&'static str, String)> {
        w.app.search.listed().iter().map(|h| (h.group, h.title.clone())).collect()
    }

    /// Put the keyboard into the sidebar search's list: show the panel, type, wait for the list, ↓.
    fn browse_sidebar(w: &mut W, text: &str) {
        w.app.side.left = crate::app::LeftPanel::Search;
        w.app.search.query.clear();
        w.app.search.focus = true;
        settle(w);
        w.frame(vec![Event::Text(text.to_string())]);
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle(w);
        w.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(w.app.search.sel, Some(0), "↓ goes into the list");
    }

    // spec: NAV-007, NAV-008, NAV-025, NAV-023, SOC-036, NAV-009
    #[test]
    fn the_sidebar_search_lists_notes_mail_and_posts_and_the_keyboard_opens_each() {
        let mut w = W::new("sidebar-all");
        with_mail(&mut w);
        with_post(&mut w);
        browse_sidebar(&mut w, "ban*");
        let rows = sidebar_rows(&w);
        let groups: Vec<&str> = rows.iter().map(|r| r.0).collect();
        let (mail, post) = (groups.iter().position(|g| *g == "Mail").expect("the mail group"), groups.iter().position(|g| *g == "Posts").expect("the posts group"));
        assert_eq!(groups[0], "Notes");
        assert_eq!((&rows[mail].1[..], &rows[post].1[..]), ("Banana invoice", "banana republic"), "{rows:?}");
        assert!(post == mail + 1 && groups.len() == post + 1, "notes, then mail, then posts: {groups:?}");
        // Ctrl+J / Ctrl+K go from group to group: the first hit of the next / previous
        w.press(Key::J, CTRL);
        assert_eq!(w.app.search.sel, Some(mail));
        w.press(Key::J, CTRL);
        assert_eq!(w.app.search.sel, Some(post));
        w.press(Key::J, CTRL);
        assert_eq!(w.app.search.sel, Some(post), "no group after the last");
        w.press(Key::K, CTRL);
        assert_eq!(w.app.search.sel, Some(mail));
        w.press(Key::K, Modifiers::NONE);
        assert_eq!(w.app.search.sel, Some(mail - 1), "k goes to the hit before, across the groups");
        w.press(Key::J, Modifiers::NONE);
        // Enter on the mail hit opens it in the mail view
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(views(&w), ["mail"]);
        assert_eq!(w.app.mail.open.as_ref().map(|o| o.msg.subject.as_str()), Some("Banana invoice"));
        assert_eq!(w.app.search.sel, None, "the hit was opened");
        // Enter on the post hit opens its feed in a new tab
        browse_sidebar(&mut w, "ban*");
        w.press(Key::J, CTRL);
        w.press(Key::J, CTRL);
        let tabs = w.app.tabbar.tabs.len();
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(w.app.tabbar.tabs.len(), tabs + 1);
        let feed = crate::social::FeedRef::new(crate::social::demo::account().account.id, "home");
        assert_eq!(w.app.tabbar.tabs[w.app.tabbar.active].view, View::Social(Some(feed)));
        // Enter on a note line puts the cursor on the found text
        browse_sidebar(&mut w, "ban*");
        let (crate::app::Action::GotoMatch { rel, .. } | crate::app::Action::Open { rel, .. }) = &w.app.search.listed()[0].action else { panic!("the first hit is a note") };
        let rel = rel.clone();
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(w.app.current_path(), Some(rel), "the note of the first hit is open");
    }

    /// Where the count button of a source is drawn ("Mail 1": its name, a blank and the count; the
    /// headings of the groups have two blanks).
    fn source_button(w: &W, name: &str) -> Pos2 {
        let one = format!("{name} ");
        let two = format!("{name}  ");
        w.text_pos.iter().find(|(t, _)| t.starts_with(&one) && !t.starts_with(&two)).unwrap_or_else(|| panic!("no button {name}: {:?}", w.texts)).1 + Vec2::new(4.0, 4.0)
    }

    fn button_texts(w: &W) -> Vec<String> {
        ["Notes", "Mail", "Posts"].iter().filter_map(|n| w.texts.iter().find(|t| t.starts_with(&format!("{n} ")) && !t.starts_with(&format!("{n}  "))).cloned()).collect()
    }

    fn click_source(w: &mut W, name: &str) {
        let p = source_button(w, name);
        click_at(w, p);
    }

    /// A click on the button of a source with Shift held.
    fn shift_click_source(w: &mut W, name: &str) {
        let p = source_button(w, name);
        let b = |pressed| Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::SHIFT };
        w.frame(vec![Event::ModifiersChanged(Modifiers::SHIFT), Event::PointerMoved(p)]);
        w.frame(vec![b(true)]);
        w.frame(vec![b(false)]);
        w.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    }

    fn groups_of(rows: Vec<(&'static str, String)>) -> Vec<&'static str> {
        let mut g: Vec<&str> = rows.into_iter().map(|r| r.0).collect();
        g.dedup();
        g
    }

    // spec: NAV-052, NAV-050
    #[test]
    fn nav_052_the_buttons_of_the_picker_solo_a_source_and_count_the_others() {
        let mut w = vim_note("buttons-picker");
        with_mail(&mut w);
        with_post(&mut w);
        space_f_g(&mut w);
        search_for(&mut w, "banana");
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"], "a button per source with its count");
        assert_eq!(groups_of(picker_rows(&w)), ["Notes", "Mail", "Posts"]);
        // a click: only that source is listed, the others are still counted, the query is as typed
        click_source(&mut w, "Mail");
        assert_eq!(groups_of(picker_rows(&w)), ["Mail"]);
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"], "a switched-off source is counted");
        assert_eq!(picker(&w).query, "banana");
        // another one while soloed: solo that one; the same again: back
        click_source(&mut w, "Posts");
        assert_eq!(groups_of(picker_rows(&w)), ["Posts"]);
        click_source(&mut w, "Posts");
        assert_eq!(groups_of(picker_rows(&w)), ["Notes", "Mail", "Posts"], "back to the default selection");
        assert_eq!(w.app.cfg.search, crate::search_sources::SearchConfig::default(), "a solo is not saved");
        // a prefix wins over the buttons, and the buttons show it
        click_source(&mut w, "Notes");
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything)));
        settle(&mut w);
        search_for(&mut w, "m:banana");
        assert_eq!(groups_of(picker_rows(&w)), ["Mail"], "the prefix chooses");
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"], "the others are counted all the same");
        // the window of file names has no buttons
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::FileNames)));
        settle(&mut w);
        assert!(button_texts(&w).is_empty());
    }

    // spec: NAV-053, NAV-054
    #[test]
    fn nav_053_shift_click_changes_the_saved_default_selection_for_both_views() {
        let mut w = vim_note("buttons-shift");
        with_mail(&mut w);
        with_post(&mut w);
        space_f_g(&mut w);
        search_for(&mut w, "banana");
        shift_click_source(&mut w, "Mail");
        assert_eq!(w.app.cfg.search.sources, ["notes", "posts"], "Mail is out of the default selection");
        assert_eq!(groups_of(picker_rows(&w)), ["Notes", "Posts"]);
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"], "a switched-off source is counted");
        shift_click_source(&mut w, "Notes");
        assert_eq!(w.app.cfg.search.sources, ["posts"]);
        shift_click_source(&mut w, "Posts");
        assert_eq!(w.app.cfg.search.sources, ["posts"], "the last one cannot be switched off");
        assert_eq!(groups_of(picker_rows(&w)), ["Posts"]);
        // the sidebar search uses the same selection
        w.press(Key::Escape, Modifiers::NONE);
        w.app.side.left = crate::app::LeftPanel::Search;
        w.app.search.query = "banana".into();
        w.app.search.focus = true;
        settle(&mut w);
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle_until(&mut w, "the sidebar lists Posts only", |w| groups_of(sidebar_rows(w)) == ["Posts"]);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Posts"], "the same default selection");
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"]);
        // …and Shift+click there changes it for the picker too
        shift_click_source(&mut w, "Notes");
        assert_eq!(w.app.cfg.search.sources, ["notes", "posts"]);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Notes", "Posts"]);
        w.app.side.left = crate::app::LeftPanel::Files;
        w.app.focus_editor();
        settle(&mut w);
        assert!(w.app.tabbar.tabs[0].editor.vim_normal_keys(&w.ctx), "the keyboard is back in the editor");
        space_f_g(&mut w);
        search_for(&mut w, "banana");
        assert_eq!(groups_of(picker_rows(&w)), ["Notes", "Posts"]);
        // a solo is on top of the selection, and back goes to the selection
        click_source(&mut w, "Mail");
        assert_eq!(groups_of(picker_rows(&w)), ["Mail"]);
        click_source(&mut w, "Mail");
        assert_eq!(groups_of(picker_rows(&w)), ["Notes", "Posts"]);
    }

    // spec: NAV-052
    #[test]
    fn nav_052_the_buttons_of_the_sidebar_search_solo_a_source_and_count_the_others() {
        let mut w = W::new("buttons-sidebar");
        with_mail(&mut w);
        with_post(&mut w);
        w.app.side.left = crate::app::LeftPanel::Search;
        w.app.search.focus = true;
        settle(&mut w);
        w.frame(vec![Event::Text("banana".to_string())]);
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle_until(&mut w, "the sidebar lists the three sources", |w| groups_of(sidebar_rows(w)) == ["Notes", "Mail", "Posts"]);
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"]);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Notes", "Mail", "Posts"]);
        click_source(&mut w, "Posts");
        assert_eq!(groups_of(sidebar_rows(&w)), ["Posts"]);
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"], "counted, not listed");
        assert_eq!(w.app.search.query, "banana", "the buttons leave the query alone");
        click_source(&mut w, "Notes");
        assert_eq!(groups_of(sidebar_rows(&w)), ["Notes"], "another one while soloed");
        click_source(&mut w, "Notes");
        assert_eq!(groups_of(sidebar_rows(&w)), ["Notes", "Mail", "Posts"], "back");
        // a prefix in the box
        w.app.search.query = "s: banana".into();
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle_until(&mut w, "the sidebar lists Posts only", |w| groups_of(sidebar_rows(w)) == ["Posts"]);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Posts"]);
        assert_eq!(button_texts(&w), ["Notes 1", "Mail 1", "Posts 1"]);
    }

    // spec: NAV-055
    #[test]
    fn nav_055_tab_and_shift_tab_solo_the_sources_in_the_picker() {
        let mut w = vim_note("tab-picker");
        with_mail(&mut w);
        with_post(&mut w);
        space_f_g(&mut w);
        assert!(w.texts.iter().any(|t| t.contains("Tab next source · Shift+Tab previous source")), "the hint has the keys: {:?}", w.texts);
        search_for(&mut w, "banana");
        let solo = |w: &W| picker(w).filter.solo;
        let mut seen = vec![];
        for _ in 0..4 {
            w.press(Key::Tab, Modifiers::NONE);
            seen.push(groups_of(picker_rows(&w)));
        }
        assert_eq!(seen, [vec!["Notes"], vec!["Mail"], vec!["Posts"], vec!["Notes", "Mail", "Posts"]], "Notes, Mail, Posts, then all again");
        w.press(Key::Tab, Modifiers::SHIFT);
        assert_eq!(groups_of(picker_rows(&w)), ["Posts"], "Shift+Tab goes backwards");
        w.press(Key::Tab, Modifiers::SHIFT);
        assert_eq!(groups_of(picker_rows(&w)), ["Mail"]);
        assert_eq!(picker(&w).query, "banana", "Tab typed nothing");
        assert_eq!(note_text(&w), "# A\n");
        // with a prefix the keys do nothing
        search_for(&mut w, "x");
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything)));
        settle(&mut w);
        search_for(&mut w, "m:banana");
        w.press(Key::Tab, Modifiers::NONE);
        assert_eq!(solo(&w), None);
        // the keys are definable, and the hint follows
        w.app.modal = Some(Modal::Picker(crate::picker::Picker::new(crate::picker::Source::Everything)));
        w.app.keymap.set(Action::ListSourceNext, Binding::parse("Ctrl+Alt+S").unwrap()).unwrap();
        settle(&mut w);
        assert!(w.texts.iter().any(|t| t.contains("Ctrl+Alt+S next source · Shift+Tab previous source")), "{:?}", w.texts);
        search_for(&mut w, "banana");
        w.press(Key::Tab, Modifiers::NONE);
        assert_eq!(solo(&w), None, "the old key does nothing");
        w.press(Key::S, CTRL_ALT);
        assert_eq!(solo(&w), Some(crate::search_sources::Kind::Notes));
    }

    // spec: NAV-055
    #[test]
    fn nav_055_tab_and_shift_tab_solo_the_sources_in_the_sidebar_search_also_while_browsing() {
        let mut w = W::new("tab-sidebar");
        with_mail(&mut w);
        with_post(&mut w);
        w.app.side.left = crate::app::LeftPanel::Search;
        w.app.search.focus = true;
        settle(&mut w);
        w.frame(vec![Event::Text("banana".to_string())]);
        std::thread::sleep(std::time::Duration::from_millis(200));
        settle(&mut w);
        // in the box
        w.press(Key::Tab, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Notes"], "Tab in the box solos the next source");
        assert!(w.ctx.memory(|m| m.has_focus(egui::Id::unique("search-input"))), "and the box keeps the keyboard");
        w.press(Key::Tab, Modifiers::SHIFT);
        w.press(Key::Tab, Modifiers::SHIFT);
        settle(&mut w);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Posts"], "Shift+Tab goes back through the default selection to the last source");
        // while the hits are browsed
        w.app.search.filter.solo = None;
        settle(&mut w);
        w.press(Key::ArrowDown, Modifiers::NONE);
        assert_eq!(w.app.search.sel, Some(0));
        assert!(w.texts.iter().any(|t| t.contains("Tab next source, Shift+Tab previous source")), "the hint has the keys: {:?}", w.texts);
        w.press(Key::Tab, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Notes"]);
        assert_eq!(w.app.search.sel, Some(0), "still browsing, at the first hit of the new list");
        assert!(w.ctx.memory(|m| m.focused().is_none()), "Tab did not move the focus");
        w.press(Key::Tab, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(groups_of(sidebar_rows(&w)), ["Mail"]);
        assert_eq!(w.app.search.sel, Some(0));
        w.press(Key::Enter, Modifiers::NONE);
        settle(&mut w);
        assert_eq!(views(&w), ["mail"], "Enter opens the hit of the soloed source");
    }

    // spec: NAV-051, CFG-067
    #[test]
    fn nav_051_the_search_settings_refuse_clashing_letters_and_keep_the_old_ones() {
        let mut w = W::new("search-settings");
        w.app.open_settings(SettingsTab::Search);
        settle(&mut w);
        let field = |w: &W, text: &str| w.text_pos.iter().find(|(t, _)| t == text).unwrap_or_else(|| panic!("no field {text}: {:?}", w.texts)).1 + Vec2::new(3.0, 5.0);
        let type_into = |w: &mut W, old: &str, new: &str| {
            let at = field(w, old);
            click_at(w, at);
            w.press(Key::A, CTRL);
            w.frame(vec![Event::Text(new.to_string())]);
            settle(w);
        };
        type_into(&mut w, "f", "i");
        assert_eq!(w.app.cfg.search.prefix_notes, "i", "a letter that is fine is taken");
        type_into(&mut w, "m", "n");
        assert_eq!(w.app.cfg.search.prefix_mail, "m", "i and n would spell in:");
        assert!(w.texts.iter().any(|t| t.contains("“in:”")), "the reason is shown: {:?}", w.texts);
        type_into(&mut w, "n", "i");
        assert_eq!(w.app.cfg.search.prefix_mail, "m", "the same letter as notes");
        assert!(w.texts.iter().any(|t| t.contains("prefix of notes and of mail")), "{:?}", w.texts);
        // the sources: the last one cannot be unchecked
        let boxes = |w: &W| ["Notes", "Mail", "Posts"].map(|n| w.text_pos.iter().filter(|(t, _)| t == n).map(|(_, p)| *p).next().unwrap_or_else(|| panic!("no {n}")));
        let [notes, mail, _] = boxes(&w);
        click_at(&mut w, notes + Vec2::new(3.0, 5.0));
        click_at(&mut w, mail + Vec2::new(3.0, 5.0));
        assert!(w.app.cfg.search.sources.len() <= 2);
    }

    // spec: NAV-022, NAV-007
    #[test]
    fn the_sidebar_search_has_no_mail_group_while_the_mail_store_is_closed() {
        let mut w = W::new("sidebar-nomail");
        with_post(&mut w);
        browse_sidebar(&mut w, "banana");
        let groups: Vec<&str> = sidebar_rows(&w).iter().map(|r| r.0).collect();
        assert!(!groups.contains(&"Mail") && groups.contains(&"Posts"), "{groups:?}");
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

    // spec: KEY-008
    #[test]
    fn key_008_a_rebound_key_of_the_file_list_works_and_the_old_one_does_not() {
        let mut w = W::new("tree-rebind");
        w.app.keymap.set(Action::TreeNextFile, Binding::parse("Ctrl+Alt+J").unwrap()).unwrap();
        // Ctrl+H gives the file list the keyboard, with the open note A selected
        w.press(Key::H, CTRL);
        assert_eq!(w.app.tree.sel.as_deref(), Some(Path::new("A.md")));
        w.press(Key::J, Modifiers::NONE);
        assert_eq!(w.app.tree.sel.as_deref(), Some(Path::new("A.md")), "the old key does nothing now");
        w.press(Key::J, CTRL_ALT);
        assert_eq!(w.app.tree.sel.as_deref(), Some(Path::new("B.md")), "the new key moves to the next file");
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
            for tab in SettingsTab::ALL {
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
        // the last row of the list is the last action of the last scope it shows
        let last = DEFS.iter().rev().find(|d| Scope::ALL.last() == Some(&d.scope)).expect("actions").id;
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
