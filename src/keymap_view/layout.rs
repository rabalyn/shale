//! The rows of keys of the keyboard that is drawn (KEY-056): an ANSI (US) and an ISO keyboard, the
//! bottom row of a PC and of a Mac, and the block of the navigation keys. Widths are in key units.

use super::model::{Mod, System};
use eframe::egui::Key;

/// One cap of a row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cap {
    /// A key that can have actions.
    Key { key: Key, legend: &'static str, w: f32 },
    /// The lower part of the tall ISO Enter: the same key, drawn without its chips.
    Cont { key: Key, w: f32 },
    /// A key the keymap cannot bind (Caps Lock, AltGr, fn, …).
    Dead { legend: &'static str, w: f32 },
    /// A modifier key: it switches the layer.
    Mod { m: Mod, legend: &'static str, w: f32 },
    /// Space between caps.
    Gap(f32),
}

impl Cap {
    pub fn width(&self) -> f32 {
        match *self {
            Cap::Key { w, .. } | Cap::Cont { w, .. } | Cap::Dead { w, .. } | Cap::Mod { w, .. } | Cap::Gap(w) => w,
        }
    }

    /// The key of the cap, for the ones that have actions.
    pub fn key(&self) -> Option<Key> {
        match *self {
            Cap::Key { key, .. } | Cap::Cont { key, .. } => Some(key),
            _ => None,
        }
    }
}

fn key(key: Key, legend: &'static str, w: f32) -> Cap {
    Cap::Key { key, legend, w }
}

/// The keys named by the characters of `s` (letters or digits), one unit wide.
fn chars(s: &'static str) -> Vec<Cap> {
    s.char_indices().map(|(i, _)| key(Key::from_name(&s[i..=i]).expect("a letter or digit is a key"), &s[i..=i], 1.0)).collect()
}

fn row(parts: Vec<Vec<Cap>>) -> Vec<Cap> {
    parts.into_iter().flatten().collect()
}

/// The function row and the five rows of the main block of the keyboard.
pub fn main_rows(iso: bool, system: System) -> Vec<Vec<Cap>> {
    let f = |keys: [(Key, &'static str); 4]| keys.into_iter().map(|(k, l)| key(k, l, 1.0)).collect::<Vec<_>>();
    let frow = row(vec![
        vec![key(Key::Escape, "Esc", 1.0), Cap::Gap(1.0)],
        f([(Key::F1, "F1"), (Key::F2, "F2"), (Key::F3, "F3"), (Key::F4, "F4")]),
        vec![Cap::Gap(0.5)],
        f([(Key::F5, "F5"), (Key::F6, "F6"), (Key::F7, "F7"), (Key::F8, "F8")]),
        vec![Cap::Gap(0.5)],
        f([(Key::F9, "F9"), (Key::F10, "F10"), (Key::F11, "F11"), (Key::F12, "F12")]),
    ]);
    let numbers = row(vec![vec![key(Key::Backtick, "`", 1.0)], chars("1234567890"), vec![key(Key::Minus, "-", 1.0), key(Key::Equals, "=", 1.0), key(Key::Backspace, "Backspace", 2.0)]]);
    let (tab, caps, shift) = if iso {
        let top = row(vec![vec![key(Key::Tab, "Tab", 1.5)], chars("QWERTYUIOP"), vec![key(Key::OpenBracket, "[", 1.0), key(Key::CloseBracket, "]", 1.0), key(Key::Enter, "Enter", 1.5)]]);
        let home = row(vec![vec![Cap::Dead { legend: "Caps", w: 1.75 }], chars("ASDFGHJKL"), vec![key(Key::Semicolon, ";", 1.0), key(Key::Quote, "'", 1.0), key(Key::Backslash, "#", 1.0), Cap::Cont { key: Key::Enter, w: 1.25 }]]);
        let bottom = row(vec![
            vec![Cap::Mod { m: Mod::Shift, legend: "Shift", w: 1.25 }, Cap::Dead { legend: "<", w: 1.0 }],
            chars("ZXCVBNM"),
            vec![key(Key::Comma, ",", 1.0), key(Key::Period, ".", 1.0), key(Key::Slash, "/", 1.0), Cap::Mod { m: Mod::Shift, legend: "Shift", w: 2.75 }],
        ]);
        (top, home, bottom)
    } else {
        let top = row(vec![vec![key(Key::Tab, "Tab", 1.5)], chars("QWERTYUIOP"), vec![key(Key::OpenBracket, "[", 1.0), key(Key::CloseBracket, "]", 1.0), key(Key::Backslash, "\\", 1.5)]]);
        let home = row(vec![vec![Cap::Dead { legend: "Caps", w: 1.75 }], chars("ASDFGHJKL"), vec![key(Key::Semicolon, ";", 1.0), key(Key::Quote, "'", 1.0), key(Key::Enter, "Enter", 2.25)]]);
        let bottom = row(vec![
            vec![Cap::Mod { m: Mod::Shift, legend: "Shift", w: 2.25 }],
            chars("ZXCVBNM"),
            vec![key(Key::Comma, ",", 1.0), key(Key::Period, ".", 1.0), key(Key::Slash, "/", 1.0), Cap::Mod { m: Mod::Shift, legend: "Shift", w: 2.75 }],
        ]);
        (top, home, bottom)
    };
    let space = match system {
        System::MacOs => vec![
            Cap::Dead { legend: "fn", w: 1.0 },
            Cap::Mod { m: Mod::Super, legend: "control", w: 1.25 },
            Cap::Mod { m: Mod::Alt, legend: "option", w: 1.25 },
            Cap::Mod { m: Mod::Ctrl, legend: "cmd", w: 1.5 },
            key(Key::Space, "Space", 6.0),
            Cap::Mod { m: Mod::Ctrl, legend: "cmd", w: 1.5 },
            Cap::Mod { m: Mod::Alt, legend: "option", w: 1.25 },
            Cap::Mod { m: Mod::Super, legend: "control", w: 1.25 },
        ],
        _ => {
            let sup = if system == System::Windows { "Win" } else { "Super" };
            vec![
                Cap::Mod { m: Mod::Ctrl, legend: "Ctrl", w: 1.25 },
                Cap::Mod { m: Mod::Super, legend: sup, w: 1.25 },
                Cap::Mod { m: Mod::Alt, legend: "Alt", w: 1.25 },
                key(Key::Space, "Space", 6.25),
                Cap::Dead { legend: "AltGr", w: 1.25 },
                Cap::Mod { m: Mod::Super, legend: sup, w: 1.25 },
                key(Key::Menu, "Menu", 1.25),
                Cap::Mod { m: Mod::Ctrl, legend: "Ctrl", w: 1.25 },
            ]
        }
    };
    vec![frow, numbers, tab, caps, shift, space]
}

/// The six rows of the block right of the main one: the rows line up with those of `main_rows`.
pub fn nav_rows() -> Vec<Vec<Cap>> {
    let dead = |legend| Cap::Dead { legend, w: 1.0 };
    vec![
        vec![key(Key::PrintScreen, "PrtSc", 1.0), dead("ScrLk"), dead("Pause")],
        vec![key(Key::Insert, "Ins", 1.0), key(Key::Home, "Home", 1.0), key(Key::PageUp, "PgUp", 1.0)],
        vec![key(Key::Delete, "Del", 1.0), key(Key::End, "End", 1.0), key(Key::PageDown, "PgDn", 1.0)],
        vec![],
        vec![Cap::Gap(1.0), key(Key::ArrowUp, "↑", 1.0), Cap::Gap(1.0)],
        vec![key(Key::ArrowLeft, "←", 1.0), key(Key::ArrowDown, "↓", 1.0), key(Key::ArrowRight, "→", 1.0)],
    ]
}

/// The width of a row in key units, with `gap` between caps (a cap of `w` units is `w` keys and the
/// gaps between them wide).
pub fn row_width(row: &[Cap], gap: f32) -> f32 {
    row.iter().map(|c| c.width() + (c.width() - 1.0) * gap).sum::<f32>() + gap * row.len().saturating_sub(1) as f32
}

/// Every key the drawn keyboard has.
pub fn keys_on(iso: bool, system: System) -> Vec<Key> {
    main_rows(iso, system).into_iter().chain(nav_rows()).flatten().filter_map(|c| c.key()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: KEY-056
    #[test]
    fn key_056_every_row_of_the_main_block_is_fifteen_units_wide_on_every_keyboard() {
        for iso in [false, true] {
            for system in System::ALL {
                for (i, r) in main_rows(iso, system).iter().enumerate() {
                    let units: f32 = r.iter().map(Cap::width).sum();
                    assert!((units - 15.0).abs() < 0.001, "row {i} of iso={iso} on {system:?} is {units} wide");
                }
            }
        }
        assert_eq!(nav_rows().len(), main_rows(false, System::Linux).len(), "the navigation block lines up with the main one");
    }

    // spec: KEY-056
    #[test]
    fn key_056_the_iso_keyboard_has_the_extra_key_and_the_tall_enter_and_a_mac_has_no_menu_key() {
        let iso = main_rows(true, System::Linux);
        assert!(iso[4].iter().any(|c| matches!(c, Cap::Dead { legend: "<", .. })), "the key left of Z");
        assert!(iso[3].iter().any(|c| matches!(c, Cap::Cont { key: Key::Enter, .. })), "the lower part of Enter");
        assert!(!main_rows(false, System::Linux)[3].iter().any(|c| matches!(c, Cap::Cont { .. })));
        assert!(keys_on(false, System::Linux).contains(&Key::Menu));
        assert!(!keys_on(false, System::MacOs).contains(&Key::Menu));
    }
}
