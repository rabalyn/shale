//! Tests of the keymap view drawn by the app (KEY-056…KEY-059): the List | Keyboard switch of the
//! Keybindings tab, the layer and system buttons, the keys, the Change button and the Keymap tab.

use super::model::{Category, Layer, System};
use super::State;
use crate::app::{Modal, Options, SettingsTab, ShaleApp, View};
use crate::keys::{Action, Binding, Keymap};
use crate::sidebar::Item;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};

struct W {
    app: ShaleApp,
    ctx: egui::Context,
    t: f64,
    dir: std::path::PathBuf,
    texts: Vec<String>,
}

impl W {
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-keymapui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "# A\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        // the tests drive the keys of Linux and Windows, and the keyboard of Linux
        app.keymap = Keymap::from_overrides_for(&app.cfg.keybindings, false);
        app.keymap_view = State::from_view("keymap:linux");
        let mut w = W { app, ctx: egui::Context::default(), t: 0.0, dir, texts: vec![] };
        crate::theme::apply(&w.ctx);
        w.frame(vec![]);
        w
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, 1400.0))), time: Some(self.t), events, ..Default::default() };
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.run_frame(ui));
        out.textures_delta.clear();
        fn walk(s: &egui::Shape, out: &mut Vec<String>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
                _ => {}
            }
        }
        self.texts.clear();
        for c in &out.shapes {
            walk(&c.shape, &mut self.texts);
        }
    }

    fn shows(&self, needle: &str) -> bool {
        self.texts.iter().any(|t| t == needle || t.contains(needle))
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
    }

    fn click_at(&mut self, p: Pos2) {
        let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(p)]);
        self.frame(vec![b(true)]);
        self.frame(vec![b(false)]);
        self.frame(vec![]);
    }

    fn open_settings_keys(&mut self) {
        self.app.open_settings(SettingsTab::Keybindings);
        if let Some(Modal::Settings { keys, .. }) = &mut self.app.modal {
            keys.board = State::from_view("keymap:linux");
        }
        for _ in 0..4 {
            self.frame(vec![]);
        }
    }

    /// The keyboard view in use: the dialog's, or the tab's.
    fn board(&self) -> &State {
        match &self.app.modal {
            Some(Modal::Settings { keys, .. }) if keys.mode == crate::keys_ui::Mode::Keyboard => &keys.board,
            _ => &self.app.keymap_view,
        }
    }

    fn rect(&self, id: &str) -> Rect {
        self.board().rects.iter().find(|r| r.0 == id).unwrap_or_else(|| panic!("no control {id} in {:?}", self.board().rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1
    }

    fn click(&mut self, id: &str) {
        let p = self.rect(id).center();
        self.click_at(p);
    }

    fn dialog(&self) -> &crate::keys_ui::KeyDialog {
        match &self.app.modal {
            Some(Modal::Settings { keys, tab: SettingsTab::Keybindings, .. }) => keys,
            _ => panic!("the Keybindings tab of the settings is not open"),
        }
    }

    fn switch(&mut self, mode: &str) {
        let r = self.dialog().rects.iter().find(|r| r.0 == format!("mode:{mode}")).expect("the switch").1;
        self.click_at(r.center());
    }

    fn open_keyboard_in_settings(&mut self) {
        self.open_settings_keys();
        self.switch("keyboard");
        self.frame(vec![]);
    }

    fn active_view(&self) -> View {
        self.app.tabbar.tabs[self.app.tabbar.active].view.clone()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };

// spec: KEY-057
#[test]
fn key_057_the_keybindings_tab_switches_between_the_list_and_the_keyboard() {
    let mut w = W::new("switch");
    w.open_settings_keys();
    assert_eq!(w.dialog().mode, crate::keys_ui::Mode::List, "the list is the first thing");
    assert!(w.shows("Reset all") && !w.shows("ANSI (US)"));
    w.switch("keyboard");
    w.frame(vec![]);
    assert_eq!(w.dialog().mode, crate::keys_ui::Mode::Keyboard);
    assert!(w.shows("ANSI (US)") && w.shows("Click a key to see what is on it"), "the keyboard and the hint of its panel");
    assert!(!w.shows("Reset all"), "the list is hidden");
    w.switch("list");
    w.frame(vec![]);
    assert!(w.shows("Reset all") && !w.shows("ANSI (US)"));
    // the settings keep the keyboard they had
    w.switch("keyboard");
    w.frame(vec![]);
    w.click("keyboard:iso");
    assert!(w.board().iso);
    w.switch("list");
    w.switch("keyboard");
    assert!(w.board().iso, "the choice stays while the dialog is open");
}

// spec: KEY-056
#[test]
fn key_056_the_keys_show_the_actions_of_the_layer_as_chips_and_the_layer_buttons_change_them() {
    let mut w = W::new("layers");
    w.open_keyboard_in_settings();
    assert_eq!(w.board().layer, Layer::NONE);
    assert!(w.shows("showing no modifier"));
    assert!(w.shows("Next file"), "J on the bare layer: the next file of the file list");
    assert!(!w.shows("Quick switcher"), "Ctrl+O is on the Ctrl layer");
    w.click("layer:ctrl");
    assert_eq!(w.board().layer, Layer::CTRL);
    assert!(w.shows("showing Ctrl") && w.shows("Quick switcher") && !w.shows("Next file"));
    w.click("layer:ctrl-shift");
    assert!(w.shows("Search all") && w.shows("Keymap"), "Ctrl+Shift+F and Ctrl+Shift+Y");
    w.click("layer:alt");
    assert!(w.shows("Bold") && w.shows("Ribbon 1"));
    w.click("layer:shift");
    assert!(w.shows("Reply all"), "Shift+R in the mail view");
    // the modifier keys of the drawn keyboard switch a modifier on and off
    w.click("layer:none");
    w.click("mod:Ctrl");
    assert_eq!(w.board().layer, Layer::CTRL);
    w.click("mod:Shift");
    assert_eq!(w.board().layer, Layer::CTRL_SHIFT);
    w.click("mod:Ctrl");
    assert_eq!(w.board().layer, Layer::SHIFT);
}

// spec: KEY-056
#[test]
fn key_056_holding_modifiers_peeks_at_their_layer_and_letting_go_goes_back() {
    let mut w = W::new("peek");
    w.open_keyboard_in_settings();
    w.click("layer:alt");
    w.frame(vec![Event::ModifiersChanged(Modifiers { ctrl: true, command: true, ..Modifiers::NONE })]);
    assert!(w.shows("showing Ctrl (keys held)") && w.shows("Quick switcher"), "{:?}", w.texts);
    assert_eq!(w.board().layer, Layer::ALT, "the chosen layer is not changed by it");
    w.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
    assert!(w.shows("showing Alt") && !w.shows("(keys held)") && !w.shows("Quick switcher"));
}

// spec: KEY-056
#[test]
fn key_056_the_system_and_the_keyboard_are_chosen_and_the_default_is_the_running_system() {
    assert_eq!(State::default().system, System::running());
    let mut w = W::new("system");
    w.open_keyboard_in_settings();
    w.click("layer:ctrl-shift");
    w.click("key:H");
    assert!(w.shows("Ctrl+Shift+H"), "the heading of the key panel");
    assert!(w.shows("Free: nothing in Shale uses this key on this layer."), "Linux has Ctrl+H for it, not Ctrl+Shift+H");
    w.click("system:macos");
    assert_eq!(w.board().system, System::MacOs);
    assert!(w.shows("Cmd+Shift+H") && w.shows("Focus ← left"), "macOS has Cmd+Shift+H: {:?}", w.texts);
    assert!(w.shows("cmd") && w.shows("option"), "the keycaps of a Mac");
    w.click("system:windows");
    assert!(w.shows("Win") && w.shows("AltGr"));
    w.click("keyboard:iso");
    assert!(w.shows("<"), "the ISO key left of Z");
}

// spec: KEY-056
#[test]
fn key_056_a_key_the_system_keeps_is_marked_and_says_so_in_its_panel() {
    let mut w = W::new("reserved");
    w.open_keyboard_in_settings();
    w.click("system:macos");
    w.click("layer:ctrl");
    w.click("key:H");
    assert!(w.shows("Hide Shale"), "Cmd+H on the key and in the panel: {:?}", w.texts);
    assert!(w.shows("The system or the desktop takes this key"));
    w.click("key:K");
    assert!(!w.shows("The system or the desktop takes this key"));
}

// spec: KEY-056
#[test]
fn key_056_the_legend_hides_a_kind_of_action_and_the_scope_filter_a_place() {
    let mut w = W::new("filter");
    w.open_keyboard_in_settings();
    w.click("layer:ctrl");
    assert!(w.shows("Quick switcher") && w.shows("Undo"), "an action of the app and a text key");
    w.click(&format!("cat:{}", Category::App.name()));
    assert!(w.board().filter.hidden_cats.contains(&Category::App));
    assert!(!w.shows("Quick switcher") && w.shows("Undo"), "{:?}", w.texts);
    w.click(&format!("cat:{}", Category::App.name()));
    assert!(w.shows("Quick switcher"));
    w.click("scope:Text fields");
    assert!(!w.shows("Undo") && w.shows("Quick switcher"));
}

// spec: KEY-056
#[test]
fn key_056_clicking_a_key_lists_its_actions_with_their_scope_binding_and_kind() {
    let mut w = W::new("detail");
    w.open_keyboard_in_settings();
    w.click("layer:ctrl-shift");
    w.click("key:K");
    assert!(w.shows("Ctrl+Shift+K"));
    assert!(w.shows("Fold the block at the cursor"), "the whole label of the action");
    assert!(w.shows("Formatting & folding · Note · Ctrl+Shift+K"), "its kind of action, scope and binding: {:?}", w.texts);
    assert!(w.shows("default"));
    // a key of the user is called so
    w.app.keymap.set(Action::NoteUnfold, Binding::parse("Ctrl+Alt+K").unwrap()).unwrap();
    w.frame(vec![]);
    w.click("layer:ctrl-alt");
    w.click("key:K");
    assert!(w.shows("your key"), "{:?}", w.texts);
    // the layers of a key
    w.click("key:K");
    assert!(w.shows("This key on every layer"));
    let n = w.board().rects.iter().filter(|r| r.0.starts_with("layer-of:")).count();
    assert!(n >= 4, "bare, Shift, Ctrl+Shift and Ctrl+Alt have something on K");
    w.click("layer-of:0");
    assert_eq!(w.board().layer, Layer::NONE, "a layer of the list is chosen by a click");
    // a key with nothing on it
    w.click("layer:alt-shift");
    w.click("key:Z");
    assert!(w.shows("Free: nothing in Shale uses this key on this layer."));
}

// spec: KEY-057, KEY-058
#[test]
fn key_057_change_goes_to_the_row_of_the_action_in_the_list_and_starts_the_new_key() {
    let mut w = W::new("change");
    w.open_keyboard_in_settings();
    w.click("layer:ctrl");
    w.click("key:S");
    assert!(w.shows("Save now"));
    w.click("change:global.save");
    let d = w.dialog();
    assert_eq!(d.mode, crate::keys_ui::Mode::List, "the Keybindings tab is the list again");
    assert!(matches!(&d.recording, Some((Action::Save, chords)) if chords.is_empty()), "the change of Save has started");
    assert!(w.app.keymap.recording, "the keys belong to the recording now");
    assert!(w.shows("press the keys…"));
    // a key pressed now is the new key of the action, and the keyboard shows it
    w.press(Key::Q, Modifiers { alt: true, ctrl: true, command: true, ..Modifiers::NONE });
    w.press(Key::Enter, Modifiers::NONE);
    assert_eq!(w.app.keymap.display(Action::Save), "Ctrl+Alt+Q");
    w.switch("keyboard");
    w.frame(vec![]);
    w.click("layer:ctrl-alt");
    assert!(w.shows("Save"), "the new key is on the keyboard: {:?}", w.texts);
}

// spec: KEY-057
#[test]
fn key_057_change_clears_a_filter_that_hides_the_row() {
    let mut w = W::new("change-filter");
    w.open_settings_keys();
    if let Some(Modal::Settings { keys, .. }) = &mut w.app.modal {
        keys.filter = "zzzz".into();
    }
    w.switch("keyboard");
    w.frame(vec![]);
    w.click("layer:ctrl");
    w.click("key:S");
    w.click("change:global.save");
    assert!(w.dialog().filter.is_empty(), "the row of the action must be in the list");
    assert!(w.dialog().recording.is_some());
}

// spec: KEY-058
#[test]
fn key_058_the_keymap_tab_opens_by_its_key_by_its_ribbon_item_and_shows_the_keyboard() {
    let mut w = W::new("tab");
    for _ in 0..3 {
        w.frame(vec![]);
    }
    assert_ne!(w.active_view(), View::Keymap);
    assert_eq!(Action::KeymapView.def().id, "global.keymap");
    assert_eq!(w.app.keymap.display(Action::KeymapView), "Ctrl+Shift+Y");
    w.press(Key::Y, CTRL_SHIFT);
    w.frame(vec![]);
    assert_eq!(w.active_view(), View::Keymap, "Ctrl+Shift+Y");
    assert_eq!(w.app.tab_title(&View::Keymap), "Keymap");
    assert!(w.shows("ANSI (US)") && w.shows("Click a key to see what is on it"), "{:?}", w.texts);
    assert!(w.shows("Linux") && w.shows("macOS"));
    // another tab, then the ribbon item
    w.app.show_view(View::Tasks, false);
    w.frame(vec![]);
    w.frame(vec![]);
    let rect = w.app.side.ribbon_icons.iter().find(|(i, _)| *i == Item::Keymap).expect("the ribbon has the Keymap item").1;
    w.click_at(rect.center());
    w.frame(vec![]);
    assert_eq!(w.active_view(), View::Keymap, "the ribbon item");
    // the keys of the tab work like those of the settings
    w.click("layer:ctrl");
    assert!(w.shows("Quick switcher"));
}

// spec: KEY-058
#[test]
fn key_058_change_in_the_tab_opens_settings_keybindings_at_the_row_and_records() {
    let mut w = W::new("tab-change");
    w.app.show_view(View::Keymap, false);
    for _ in 0..3 {
        w.frame(vec![]);
    }
    w.click("layer:ctrl");
    w.click("key:O");
    assert!(w.shows("Quick switcher"));
    assert!(w.app.modal.is_none());
    w.click("change:global.quick_switcher");
    w.frame(vec![]);
    assert!(matches!(&w.app.modal, Some(Modal::Settings { tab: SettingsTab::Keybindings, .. })), "the Keybindings tab of the settings is open");
    let d = w.dialog();
    assert_eq!(d.mode, crate::keys_ui::Mode::List);
    assert!(matches!(&d.recording, Some((Action::QuickSwitcher, _))), "the change has started");
    assert!(w.shows("press the keys…"));
}

// spec: KEY-058
#[test]
fn key_058_the_keymap_tab_shows_changes_made_while_it_is_open() {
    let mut w = W::new("tab-live");
    w.app.show_view(View::Keymap, false);
    w.frame(vec![]);
    w.click("layer:ctrl-alt");
    assert!(!w.shows("Quick switcher"));
    w.app.keymap.set(Action::QuickSwitcher, Binding::parse("Ctrl+Alt+Q").unwrap()).unwrap();
    w.frame(vec![]);
    assert!(w.shows("Quick switcher"), "the live keymap, not a copy");
}

// spec: KEY-059
#[test]
fn key_059_a_screenshot_view_names_the_system_and_the_layer() {
    let s = State::from_view("keymap:macos:cmd-shift");
    assert_eq!((s.system, s.layer), (System::MacOs, Layer::CTRL_SHIFT));
    let s = State::from_view("keymap:windows:alt");
    assert_eq!((s.system, s.layer), (System::Windows, Layer::ALT));
    let s = State::from_view("keymap:linux:none");
    assert_eq!((s.system, s.layer), (System::Linux, Layer::NONE));
    let s = State::from_view("keymap");
    assert_eq!((s.system, s.layer), (System::running(), Layer::NONE));
    // what it does not know is the default
    let s = State::from_view("keymap:amiga:hyper");
    assert_eq!((s.system, s.layer), (System::running(), Layer::NONE));
    // a Mac has no Ctrl layer under that name: it is Cmd
    assert_eq!(State::from_view("keymap:macos:ctrl").layer, Layer::NONE);
}

// spec: KEY-059
#[test]
fn key_059_the_recipes_make_a_picture_per_system_and_main_layer() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/docs/screenshots/recipes.conf")).unwrap();
    for system in System::ALL {
        for layer in [Layer::NONE, Layer::SHIFT, Layer::CTRL, Layer::ALT, Layer::CTRL_SHIFT, Layer::ALT_SHIFT] {
            let (id, name) = (layer.id(system), system.id());
            let section = format!("[keymap-{name}-{id}]\n");
            let at = text.find(&section).unwrap_or_else(|| panic!("no recipe {section}"));
            let body = text[at..].lines().skip(1).take_while(|l| !l.starts_with('[')).collect::<Vec<_>>().join("\n");
            assert!(body.contains(&format!("view = keymap:{name}:{id}")), "{section}{body}");
            let view = State::from_view(&format!("keymap:{name}:{id}"));
            assert_eq!((view.system, view.layer), (system, layer), "the recipe's view means what its name says");
        }
    }
    assert!(text.contains("[keymap-linux-none-light]") && text.contains("[keymap-macos-cmd-light]"), "and a light picture of two");
}
