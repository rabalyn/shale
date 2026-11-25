//! The keymap as a web page (KEY-060, rabalyn/shale#712): `docs/keymap/index.html` is the template
//! `docs/keymap/template.html` with the data of the program put in: the definable actions with their
//! default keys on Linux and Windows and on macOS (`keys::DEFS`), the categories, the places, and the
//! tables of `tables.rs` (fixed keys, keys the systems reserve). The page has no other source of keys, so it
//! cannot drift from the program; a test builds it and compares it with the checked-in file (run it with
//! `SHALE_UPDATE_KEYMAP=1` to write the file instead).

use super::model::{short_of, Category, Where};
use super::tables::{self, Match, Reserved};
use crate::keys::{Keymap, DEFS};
use serde_json::{json, Value};

const TEMPLATE: &str = include_str!("../../docs/keymap/template.html");

fn texts(km: &Keymap, d: &crate::keys::Def) -> Vec<String> {
    km.bindings(d.action).iter().map(|b| b.to_text()).collect()
}

fn actions() -> Vec<Value> {
    let (lin, mac) = (Keymap::for_os(false), Keymap::for_os(true));
    DEFS.iter().map(|d| json!({ "id": d.id, "label": d.label, "scope": format!("{:?}", Where::from(d.scope)), "cat": format!("{:?}", Category::of_def(d)), "short": short_of(d), "lin": texts(&lin, d), "mac": texts(&mac, d) })).collect()
}

fn builtin() -> Vec<Value> {
    tables::BUILTIN.iter().map(|b| json!({ "c": b.keys, "p": b.systems, "label": b.label, "scope": format!("{:?}", b.scope), "cat": format!("{:?}", b.cat), "note": b.note })).collect()
}

fn reserved(table: &[Reserved]) -> Vec<Value> {
    table
        .iter()
        .map(|r| {
            let kind = match r.kind {
                Match::Exact => "exact",
                Match::AnyKeyWith => "any",
                Match::AnyCharWith => "char",
            };
            json!({ "keys": r.keys, "label": r.label, "kind": kind })
        })
        .collect()
}

/// `"name": [` and one item per line, so a change of one key is one line of the diff.
fn array(name: &str, items: &[Value]) -> String {
    let lines: Vec<String> = items.iter().map(|v| format!("  {v}")).collect();
    format!(" \"{name}\": [\n{}\n ]", lines.join(",\n"))
}

/// The `const DATA = …;` of the page.
fn data() -> String {
    let cats: Vec<Value> = Category::ALL.iter().map(|c| json!({ "id": format!("{c:?}"), "name": c.name() })).collect();
    let scopes: Vec<Value> = Where::ALL.iter().map(|w| json!({ "id": format!("{w:?}"), "label": w.label() })).collect();
    let parts = [
        array("actions", &actions()),
        array("cats", &cats),
        array("scopes", &scopes),
        array("builtin", &builtin()),
        array("reservedLinux", &reserved(tables::RESERVED_LINUX)),
        array("reservedWindows", &reserved(tables::RESERVED_WINDOWS)),
        array("reservedMacos", &reserved(tables::RESERVED_MACOS)),
    ];
    // no `</script>` can end the script early
    format!("const DATA = {{\n{}\n}};", parts.join(",\n")).replace("</", "<\\/")
}

/// The custom properties of the colours of the categories, for a light or a dark theme.
fn colours(dark: bool) -> String {
    Category::ALL
        .iter()
        .map(|c| {
            let [r, g, b] = c.colour(dark);
            format!("--c-{}: #{r:02x}{g:02x}{b:02x};", format!("{c:?}").to_lowercase())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The whole page.
pub fn page() -> String {
    TEMPLATE.replace("/*DATA*/", &data()).replace("/*CAT_LIGHT*/", &colours(false)).replace("/*CAT_DARK*/", &colours(true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    }

    // spec: KEY-060
    #[test]
    fn key_060_the_page_is_made_from_the_tables_and_is_checked_in() {
        let page = page();
        let path = root().join("docs/keymap/index.html");
        if std::env::var("SHALE_UPDATE_KEYMAP").is_ok_and(|v| v == "1") {
            std::fs::write(&path, &page).unwrap();
        }
        let checked_in = std::fs::read_to_string(&path).unwrap_or_default();
        assert!(checked_in == page, "docs/keymap/index.html is out of date: run this test again with SHALE_UPDATE_KEYMAP=1 to write it, and commit the result");
    }

    // spec: KEY-060
    #[test]
    fn key_060_the_page_names_every_action_every_fixed_key_and_every_reserved_key() {
        let page = page();
        for d in DEFS {
            assert!(page.contains(&format!("\"id\":\"{}\"", d.id)), "{} is not on the page", d.id);
        }
        // the keys of macOS are its own: the focus keys have Shift there (KEY-019)
        assert!(page.contains(r#"{"cat":"Moving","id":"global.focus_sidebar","label":"Keyboard one place to the left (pane, then left sidebar)","lin":["Ctrl+H"],"mac":["Ctrl+Shift+H"],"scope":"Global","short":"Focus ← left"}"#));
        assert!(page.contains(r#""id":"global.keymap","label":"Keymap (the keyboard with every shortcut, in a tab)","lin":["Ctrl+Shift+Y"],"mac":["Ctrl+Shift+Y"]"#));
        assert_eq!(page.matches("{\"c\":\"").count(), tables::BUILTIN.len());
        let reserved = tables::RESERVED_LINUX.len() + tables::RESERVED_WINDOWS.len() + tables::RESERVED_MACOS.len();
        assert_eq!(page.matches("\"kind\":\"").count(), reserved);
        for c in Category::ALL {
            assert!(page.contains(&format!("\"id\":\"{c:?}\",\"name\":\"{}\"", c.name())), "{c:?}");
            let [r, g, b] = c.colour(true);
            assert!(page.contains(&format!("--c-{}: #{r:02x}{g:02x}{b:02x};", format!("{c:?}").to_lowercase())), "the dark colour of {c:?}");
        }
        for w in Where::ALL {
            assert!(page.contains(&format!("\"id\":\"{w:?}\",\"label\":\"{}\"", w.label())), "{w:?}");
        }
    }

    // spec: KEY-060
    #[test]
    fn key_060_the_page_is_one_file_that_loads_nothing_and_leaves_no_placeholder() {
        let page = page();
        for outside in ["http://", "https://", "src=\"//", "@import"] {
            assert!(!page.contains(outside), "the page refers to {outside}");
        }
        assert!(!page.contains("/*DATA*/") && !page.contains("/*CAT_"), "a placeholder is left");
        assert_eq!(page.matches("</script>").count(), 1, "the data must not end the script");
        assert!(page.starts_with("<!doctype html>") && page.trim_end().ends_with("</html>"));
    }

    // spec: KEY-061
    #[test]
    fn key_061_the_keymap_note_and_the_guide_show_every_layer_of_every_system() {
        let keymap = std::fs::read_to_string(root().join("docs/KEYMAP.md")).unwrap();
        let guide = std::fs::read_to_string(root().join("docs/userguide/Keyboard shortcuts.md")).unwrap();
        for (system, label, layers) in [
            ("linux", "Linux", ["none", "shift", "ctrl", "alt", "ctrl-shift", "alt-shift"]),
            ("windows", "Windows", ["none", "shift", "ctrl", "alt", "ctrl-shift", "alt-shift"]),
            ("macos", "macOS", ["none", "shift", "cmd", "option", "cmd-shift", "option-shift"]),
        ] {
            for layer in layers {
                let picture = format!("keymap-{system}-{layer}.png");
                assert!(keymap.contains(&format!("](userguide/z_media/screenshots/{picture})")), "KEYMAP.md lacks {picture}");
                assert!(guide.contains(&format!("![[{picture}|")), "the guide lacks {picture}");
                assert!(root().join("docs/userguide/z_media/screenshots").join(&picture).is_file(), "{picture} does not exist");
            }
            assert!(keymap.contains(&format!("<details><summary>{label}: ")), "KEYMAP.md has no block for {label}");
            assert!(guide.contains(&format!("> [!carousel] Keyboard layers: {label}")), "the guide has no carousel for {label}");
        }
        // a blank line after the summary, so the Markdown inside is rendered on GitHub and Forgejo
        assert_eq!(keymap.matches("</summary>\n\n![").count(), keymap.matches("<summary>").count());
        // the page and the file are linked
        for doc in ["README.md", "docs/FEATURES.md"] {
            let text = std::fs::read_to_string(root().join(doc)).unwrap();
            assert!(text.contains("https://rabalyn.github.io/shale/") && text.contains("KEYMAP.md"), "{doc} does not link the keymap");
        }
    }
}
