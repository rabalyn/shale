//! Tests of the data of the keymap view (KEY-055).

use super::model::{chord_text, short_of, Category, Filter, Kind, Layer, Mod, Model, System, Where};
use super::tables::{self, Match};
use crate::keys::{Action, Binding, Keymap, DEFS};
use eframe::egui::Key;

fn linux() -> Keymap {
    Keymap::for_os(false)
}

fn mac() -> Keymap {
    Keymap::for_os(true)
}

fn has(model: &Model, layer: Layer, key: Key, a: Action) -> bool {
    model.at(layer, key).iter().any(|e| e.action == Some(a))
}

// spec: KEY-055
#[test]
fn key_055_the_actions_sit_on_the_layer_and_key_of_their_first_chord() {
    let m = Model::new(&linux(), System::Linux);
    assert!(has(&m, Layer::CTRL, Key::H, Action::FocusSidebar));
    assert!(has(&m, Layer::CTRL_SHIFT, Key::F, Action::Search));
    assert!(has(&m, Layer::ALT, Key::B, Action::NoteBold));
    assert!(has(&m, Layer::SHIFT, Key::Tab, Action::HitSourcePrev));
    assert!(has(&m, Layer::NONE, Key::J, Action::TreeNextFile));
    assert!(!has(&m, Layer::NONE, Key::H, Action::FocusSidebar), "not on the bare key");
    assert!(!has(&m, Layer::ALT, Key::H, Action::FocusSidebar), "not on another layer");
    // two keys of one action are two entries, a Ctrl+Shift+Tab as well as a Ctrl+PageUp
    assert!(has(&m, Layer::CTRL, Key::PageUp, Action::PrevTab) && has(&m, Layer::CTRL_SHIFT, Key::Tab, Action::PrevTab));
    // every definable key of the registry is on the keyboard once per binding
    let on_keys = DEFS.iter().map(|d| linux().bindings(d.action).len()).sum::<usize>();
    assert_eq!(m.entries().iter().filter(|e| e.action.is_some()).count(), on_keys);
}

// spec: KEY-055
#[test]
fn key_055_a_sequence_sits_on_its_first_key_in_the_bare_layer_with_its_whole_text() {
    let m = Model::new(&linux(), System::Linux);
    let on_space: Vec<_> = m.at(Layer::NONE, Key::Space).into_iter().filter(|e| e.sequence).collect();
    let find_file = on_space.iter().find(|e| e.action == Some(Action::NormalFindFile)).expect("Space f f is on Space");
    assert_eq!(find_file.text, "Space f f", "the later keys are typed bare");
    assert!(on_space.iter().any(|e| e.action == Some(Action::MainBoards)));
    assert!(m.sequences().iter().all(|e| e.layer == Layer::NONE || e.action.is_some()));
    assert!(!m.at(Layer::NONE, Key::F).iter().any(|e| e.action == Some(Action::NormalFindFile)), "not on the second key");
}

// spec: KEY-055
#[test]
fn key_055_the_live_changes_of_the_user_show_and_the_old_key_is_empty() {
    let mut k = linux();
    k.set(Action::Save, Binding::parse("Ctrl+Alt+Q").unwrap()).unwrap();
    let m = Model::new(&k, System::Linux);
    let e = m.at(Layer::CTRL_ALT, Key::Q).into_iter().find(|e| e.action == Some(Action::Save)).expect("Save moved");
    assert_eq!(e.kind, Kind::Changed);
    assert_eq!(e.text, "Ctrl+Alt+Q");
    assert!(!has(&m, Layer::CTRL, Key::S, Action::Save), "the default key is free now");
    // an action that was cleared is on no key
    k.clear(Action::NewNote);
    let m = Model::new(&k, System::Linux);
    assert!(m.entries().iter().all(|e| e.action != Some(Action::NewNote)));
    // the untouched ones stay defaults
    assert!(m.at(Layer::CTRL, Key::O).iter().any(|e| e.action == Some(Action::QuickSwitcher) && e.kind == Kind::Default));
}

// spec: KEY-055
#[test]
fn key_055_the_macos_keyboard_has_the_macos_defaults_with_cmd_and_no_alt_keys() {
    let m = Model::new(&mac(), System::MacOs);
    // Cmd+H hides the app, so the focus keys are Cmd+Shift+H / L (KEY-019)
    assert!(has(&m, Layer::CTRL_SHIFT, Key::H, Action::FocusSidebar));
    assert!(!has(&m, Layer::CTRL, Key::H, Action::FocusSidebar));
    assert!(has(&m, Layer::CTRL, Key::OpenBracket, Action::Back));
    assert!(has(&m, Layer::CTRL_ALT, Key::ArrowLeft, Action::PrevTab));
    // Option+letter types characters, so the buttons of a note have no default there
    assert!(m.entries().iter().all(|e| e.action != Some(Action::NoteBold) && e.action != Some(Action::RibbonSlot1)));
    let e = m.at(Layer::CTRL, Key::S).into_iter().find(|e| e.action == Some(Action::Save)).unwrap();
    assert_eq!(e.text, "Cmd+S");
    assert_eq!(m.at(Layer::CTRL_SHIFT, Key::H)[0].text, "Cmd+Shift+H");
}

// spec: KEY-055
#[test]
fn key_055_another_system_shows_the_changes_of_the_user_on_its_own_defaults() {
    let mut k = linux();
    k.set(Action::Save, Binding::parse("Ctrl+Alt+Q").unwrap()).unwrap();
    let on_mac = Model::new(&k, System::MacOs);
    assert!(has(&on_mac, Layer::CTRL_ALT, Key::Q, Action::Save), "the change comes along");
    assert!(has(&on_mac, Layer::CTRL_SHIFT, Key::H, Action::FocusSidebar), "the rest are the defaults of macOS");
    let on_windows = Model::new(&k, System::Windows);
    assert!(has(&on_windows, Layer::CTRL, Key::H, Action::FocusSidebar));
    assert!(has(&on_windows, Layer::ALT, Key::B, Action::NoteBold));
    assert_eq!(on_windows.at(Layer::CTRL_ALT, Key::Q)[0].text, "Ctrl+Alt+Q");
}

// spec: KEY-055
#[test]
fn key_055_the_fixed_keys_of_the_program_are_on_the_keyboard_per_system() {
    let linux_m = Model::new(&linux(), System::Linux);
    let undo = linux_m.at(Layer::CTRL, Key::Z);
    assert!(undo.iter().any(|e| e.label == "Undo" && e.kind == Kind::Fixed && e.scope == Where::Text && e.cat == Category::Text));
    assert!(linux_m.at(Layer::NONE, Key::H).iter().any(|e| e.scope == Where::EditorNormal && e.cat == Category::Vim), "vim-lite normal mode");
    assert!(linux_m.at(Layer::NONE, Key::R).iter().any(|e| e.scope == Where::Drawing && e.cat == Category::Draw), "the drawing tools");
    assert!(linux_m.at(Layer::CTRL, Key::Q).iter().any(|e| e.label == "Quit"), "egui's quit key");
    assert!(linux_m.at(Layer::CTRL, Key::Num0).iter().any(|e| e.label == "Reset zoom"), "egui's zoom");
    // a fixed key is no definable action
    assert!(linux_m.entries().iter().filter(|e| e.kind == Kind::Fixed).all(|e| e.action.is_none()));
    // the Mac moves in text with Cmd and Option, not with Ctrl
    let mac_m = Model::new(&mac(), System::MacOs);
    assert!(mac_m.at(Layer::ALT, Key::ArrowLeft).iter().any(|e| e.label == "Word left"));
    assert!(!linux_m.at(Layer::ALT, Key::ArrowLeft).iter().any(|e| e.label == "Word left"));
    assert!(linux_m.at(Layer::CTRL, Key::ArrowLeft).iter().any(|e| e.label == "Word left"));
    assert!(mac_m.at(Layer { sup: true, ..Layer::NONE }, Key::A).iter().any(|e| e.label.contains("Emacs")), "Control+A on a Mac");
    // the fixed keys come after the keys of the user on a shared key
    let on_ctrl_shift_z = linux_m.at(Layer::CTRL_SHIFT, Key::Z);
    assert!(on_ctrl_shift_z.iter().all(|e| e.kind == Kind::Fixed));
}

// spec: KEY-055
#[test]
fn key_055_the_keys_the_system_keeps_are_marked_per_system() {
    let linux_m = Model::new(&linux(), System::Linux);
    let windows = Model::new(&linux(), System::Windows);
    let mac_m = Model::new(&mac(), System::MacOs);
    assert_eq!(linux_m.reserved(Layer::ALT, Key::Tab), Some("Switch windows"));
    assert!(windows.reserved(Layer::ALT, Key::Tab).is_some());
    assert_eq!(mac_m.reserved(Layer::CTRL, Key::H), Some("Hide Shale"), "Cmd+H hides the app (KEY-019)");
    assert_eq!(linux_m.reserved(Layer::CTRL, Key::H), None);
    assert_eq!(mac_m.reserved(Layer::CTRL_SHIFT, Key::Num3), Some("Screenshot"));
    // the Super key takes every key with it, on every system
    let sup = Layer { sup: true, ..Layer::NONE };
    for m in [&linux_m, &windows, &mac_m] {
        assert!(m.reserved(Layer { sup: true, shift: true, ..Layer::NONE }, Key::K).is_some() == (m.system != System::MacOs));
    }
    assert!(linux_m.reserved(sup, Key::G).is_some() && windows.reserved(sup, Key::G).is_some());
    // AltGr is Ctrl+Alt on Windows: every key that types is taken, an arrow is not
    assert!(windows.reserved(Layer::CTRL_ALT, Key::Q).is_some());
    assert_eq!(windows.reserved(Layer::CTRL_ALT, Key::ArrowLeft), None);
    assert_eq!(windows.reserved(Layer::NONE, Key::Q), None);
    // a layer of a key lists the system's chord too, so a key that only the system uses is listed
    let layers = mac_m.layers_of(Key::H);
    assert!(layers.iter().any(|l| l.layer == Layer::CTRL && l.reserved == Some("Hide Shale")));
}

// spec: KEY-055
#[test]
fn key_055_every_row_of_the_two_tables_is_a_valid_chord_and_every_short_name_is_short() {
    for b in tables::BUILTIN {
        let (_, key) = Layer::parse(b.keys).unwrap_or_else(|| panic!("{} is no chord", b.keys));
        assert!(key.is_some(), "{} has no key", b.keys);
        assert!(!b.systems.is_empty() && b.systems.chars().all(|c| "lwm".contains(c)), "{}", b.keys);
    }
    for table in [tables::RESERVED_LINUX, tables::RESERVED_WINDOWS, tables::RESERVED_MACOS] {
        for r in table {
            let (_, key) = Layer::parse(r.keys).unwrap_or_else(|| panic!("{} is no chord", r.keys));
            assert_eq!(key.is_some(), r.kind == Match::Exact, "{}", r.keys);
        }
    }
    for d in DEFS {
        let short = short_of(d);
        assert!(!short.is_empty() && short.chars().count() <= 26, "{}: {short}", d.id);
    }
    for (id, _) in tables::SHORT {
        assert!(DEFS.iter().any(|d| d.id == *id), "{id} is no action");
    }
    assert_eq!(Layer::parse("Ctrl+Shift+K"), Some((Layer::CTRL_SHIFT, Some(Key::K))));
    assert_eq!(Layer::parse("Super+Alt"), Some((Layer { sup: true, alt: true, ..Layer::NONE }, None)));
    assert_eq!(Layer::parse("Nope+K"), None);
}

// spec: KEY-055
#[test]
fn key_055_every_action_has_a_category_and_the_ids_decide_it() {
    let cat = |id: &str| Category::of_def(DEFS.iter().find(|d| d.id == id).unwrap());
    assert_eq!(cat("note.bold"), Category::Format);
    assert_eq!(cat("note.fold"), Category::Format);
    assert_eq!(cat("mail.reply"), Category::Mail);
    assert_eq!(cat("global.ribbon_3"), Category::Ribbon);
    assert_eq!(cat("global.sidebar_left"), Category::Ribbon);
    assert_eq!(cat("global.back"), Category::Moving);
    assert_eq!(cat("files.next_file"), Category::Moving);
    assert_eq!(cat("list.open"), Category::Moving);
    assert_eq!(cat("global.save"), Category::App);
    assert_eq!(cat("global.keymap"), Category::App);
    // the colours are different in the light and the dark theme and no two kinds share one
    for dark in [false, true] {
        let colours: std::collections::HashSet<_> = Category::ALL.iter().map(|c| c.colour(dark)).collect();
        assert_eq!(colours.len(), Category::ALL.len());
    }
    assert!(Category::ALL.iter().all(|c| c.colour(false) != c.colour(true)));
}

// spec: KEY-055
#[test]
fn key_055_the_layers_have_names_and_ids_on_every_system_and_a_key_lists_its_layers() {
    assert_eq!(Layer::CTRL_SHIFT.name(System::Linux), "Ctrl+Shift");
    assert_eq!(Layer::CTRL_SHIFT.name(System::MacOs), "Cmd+Shift");
    assert_eq!(Layer::ALT.name(System::MacOs), "Option");
    assert_eq!(Layer::NONE.name(System::Windows), "no modifier");
    assert_eq!(Layer { sup: true, ..Layer::NONE }.name(System::Windows), "Win");
    assert_eq!(Layer::CTRL_SHIFT.id(System::MacOs), "cmd-shift");
    for system in System::ALL {
        for bits in 0..16u8 {
            let l = Layer { ctrl: bits & 1 != 0, alt: bits & 2 != 0, shift: bits & 4 != 0, sup: bits & 8 != 0 };
            assert_eq!(Layer::from_id(system, &l.id(system)), Some(l), "{system:?} {l:?}");
        }
        assert_eq!(System::from_id(system.id()), Some(system));
    }
    assert_eq!(chord_text(System::MacOs, Layer::CTRL_ALT, Key::ArrowLeft), "Cmd+Option+←");
    assert!(Layer::PRESETS.iter().all(|l| l.has(Mod::Ctrl) as u8 + l.has(Mod::Alt) as u8 + l.has(Mod::Shift) as u8 <= 2));
    // K is fold on Ctrl+Shift, the link preview on Shift, vim up on the bare key
    let m = Model::new(&linux(), System::Linux);
    let layers: Vec<Layer> = m.layers_of(Key::K).iter().map(|l| l.layer).collect();
    assert!(layers.contains(&Layer::NONE) && layers.contains(&Layer::SHIFT) && layers.contains(&Layer::CTRL_SHIFT));
    assert!(layers.windows(2).all(|w| w[0].name(System::Linux).len() <= w[1].name(System::Linux).len() || w[0] < w[1]), "fewest modifiers first");
}

// spec: KEY-055
#[test]
fn key_055_the_filter_hides_a_kind_and_a_place() {
    let m = Model::new(&linux(), System::Linux);
    let mut f = Filter::default();
    let all = m.entries().len();
    assert_eq!(m.entries().iter().filter(|e| f.shows(e)).count(), all);
    f.hidden_cats.push(Category::Mail);
    let no_mail = m.entries().iter().filter(|e| f.shows(e)).count();
    assert!(no_mail < all && m.entries().iter().filter(|e| f.shows(e)).all(|e| e.cat != Category::Mail));
    f.hidden_scopes.push(Where::Global);
    assert!(m.entries().iter().filter(|e| f.shows(e)).all(|e| e.scope != Where::Global && e.cat != Category::Mail));
}

// spec: KEY-055, KEY-058, KEY-019
#[test]
fn key_058_the_default_key_of_the_keymap_action_is_free_on_every_system_and_nothing_fixed_uses_it() {
    for (system, k) in [(System::Linux, linux()), (System::Windows, linux()), (System::MacOs, mac())] {
        let m = Model::new(&k, system);
        assert_eq!(m.reserved(Layer::CTRL_SHIFT, Key::Y), None, "{system:?}");
        let on = m.at(Layer::CTRL_SHIFT, Key::Y);
        assert_eq!(on.len(), 1, "{system:?}: only the keymap action is on Ctrl+Shift+Y");
        assert_eq!(on[0].action, Some(Action::KeymapView));
        assert_eq!(on[0].cat, Category::App);
    }
}
