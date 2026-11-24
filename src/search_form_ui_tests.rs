//! The filter form (QUERY-021…QUERY-027) in the two windows that have it, the picker (`Space f g`)
//! and the sidebar search, driven in the whole window with real clicks and key presses.

use crate::app::{LeftPanel, Modal, Options, ShaleApp};
use crate::picker::{Picker, Source};
use crate::search_all::DEBOUNCE;
use crate::ui_test_support::Headless;
use eframe::egui::{accesskit::Role, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::path::PathBuf;

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
    /// A vault: A is doing and has the tag, B is done, C has neither.
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-form-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "---\nstatus: doing\n---\nbanana #todo\n").unwrap();
        std::fs::write(dir.join("B.md"), "---\nstatus: done\n---\nbanana split\n").unwrap();
        std::fs::write(dir.join("C.md"), "cherry\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        // these tests drive the Linux/Windows defaults; macOS has other keys for six actions (KEY-019)
        app.keymap = crate::keys::Keymap::from_overrides_for(&app.cfg.keybindings, false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.settle(&mut w.app);
        w
    }

    fn open_picker(&mut self) {
        self.app.modal = Some(Modal::Picker(Picker::new(Source::Everything)));
        self.hl.frames(&mut self.app, 3);
    }

    fn picker(&self) -> &Picker {
        match &self.app.modal {
            Some(Modal::Picker(p)) => p,
            _ => panic!("the picker is not open"),
        }
    }

    fn open_sidebar(&mut self) {
        self.app.side.left = LeftPanel::Search;
        self.app.search.focus = true;
        self.hl.frames(&mut self.app, 3);
    }

    /// The accessible widgets on screen: role, text, place.
    fn nodes(&mut self) -> Vec<(Role, String, Rect)> {
        self.hl.ctx().enable_accesskit();
        let mut out = self.hl.frame(&mut self.app, vec![]);
        let tree = out.platform_output.accesskit_update.take().expect("accessibility is on");
        tree.nodes
            .iter()
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                Some((n.role(), n.label().or_else(|| n.value()).unwrap_or_default().to_string(), Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32))))
            })
            .collect()
    }

    fn all(&mut self, label: &str) -> Vec<Rect> {
        let mut v: Vec<Rect> = self.nodes().into_iter().filter(|n| n.1 == label).map(|n| n.2).collect();
        v.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
        v
    }

    fn has(&mut self, label: &str) -> bool {
        !self.all(label).is_empty()
    }

    fn says(&mut self, text: &str) -> bool {
        self.nodes().iter().any(|n| n.1.contains(text))
    }

    fn click(&mut self, label: &str) {
        let r = *self.all(label).first().unwrap_or_else(|| panic!("no {label:?} on screen"));
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
    }

    fn pick(&mut self, menu: &str, item: &str) {
        self.click(menu);
        self.click(item);
    }

    /// The text boxes, top to bottom.
    fn edits(&mut self) -> Vec<Rect> {
        let mut v: Vec<Rect> = self.nodes().into_iter().filter(|n| n.0 == Role::TextInput).map(|n| n.2).collect();
        v.sort_by(|a, b| a.min.y.total_cmp(&b.min.y).then(a.min.x.total_cmp(&b.min.x)));
        v
    }

    /// Click into the text box on the row of `label` and type.
    fn type_in_row(&mut self, label: &str, text: &str) {
        let row = self.all(label)[0];
        // (the field nearest to the label, next to it or under it when a narrow window wraps the row)
        let field = self.edits().into_iter().filter(|e| e.min.y > row.min.y - 6.0).min_by(|a, b| a.center().distance(row.center()).total_cmp(&b.center().distance(row.center()))).unwrap_or_else(|| panic!("no field on the row {label:?}"));
        self.hl.click(&mut self.app, field.center(), PointerButton::Primary);
        self.hl.frame(&mut self.app, vec![Event::Text(text.to_string())]);
        self.hl.frames(&mut self.app, 2);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        for pressed in [true, false] {
            self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }]);
        }
        self.hl.frames(&mut self.app, 2);
    }

    /// Wait for the typing to settle (NAV-024) and for the worker.
    fn settle(&mut self) {
        std::thread::sleep(DEBOUNCE + std::time::Duration::from_millis(30));
        self.hl.settle(&mut self.app);
    }

    fn picker_titles(&self) -> Vec<String> {
        let mut t: Vec<String> = self.picker().listed().iter().map(|h| h.title.clone()).collect();
        t.sort();
        t.dedup();
        t
    }
}

// spec: QUERY-027
#[test]
fn query_027_the_picker_is_one_text_box_and_the_form_is_behind_a_gear_button() {
    let mut w = W::new("picker-gear");
    let others = w.edits().len();
    w.open_picker();
    assert_eq!(w.edits().len(), others + 1, "one text box in the window");
    assert!(w.has("⚙") && !w.has("All of") && !w.has("Advanced"), "the form is behind the gear");
    w.click("⚙");
    assert!(w.picker().form_open);
    assert!(w.has("All of") && w.has("Any of") && w.has("+ Add filter") && w.has("Advanced"));
    assert_eq!(w.edits().len(), others + 1, "still one text box: the form has rows, not boxes, until a filter is added");
    w.click("⚙");
    assert!(!w.picker().form_open && !w.has("All of"), "the gear closes it again");
    // the file-name picker has no query language, so no gear
    w.app.modal = Some(Modal::Picker(Picker::new(Source::FileNames)));
    w.hl.frames(&mut w.app, 3);
    assert!(!w.has("⚙"));
}

// spec: QUERY-027, QUERY-021
#[test]
fn query_027_the_rows_of_the_form_write_the_query_of_the_picker_and_search_at_once() {
    let mut w = W::new("picker-rows");
    w.open_picker();
    w.click("⚙");
    w.pick("+ Add filter", "tag is");
    w.type_in_row("tag is", "todo");
    assert_eq!(w.picker().query, "#todo");
    w.hl.settle(&mut w.app);
    assert_eq!(w.picker_titles(), ["A"], "the search ran for the text the form wrote, without waiting for the typing");
    // not: every note but A
    w.click("not");
    assert_eq!(w.picker().query, "NOT #todo");
    w.hl.settle(&mut w.app);
    assert_eq!(w.picker_titles(), ["B", "C"]);
    // a second row, All of: notes without the tag that are done
    w.pick("+ Add filter", "property is");
    w.type_in_row("property is", "status");
    let value = w.all("property is")[0];
    let field = w.edits().into_iter().filter(|e| (e.center().y - value.center().y).abs() < 12.0).nth(1).expect("the value field");
    w.hl.click(&mut w.app, field.center(), PointerButton::Primary);
    w.hl.frame(&mut w.app, vec![Event::Text("done".into())]);
    w.hl.frames(&mut w.app, 2);
    assert_eq!(w.picker().query, "NOT #todo AND status:done");
    w.hl.settle(&mut w.app);
    assert_eq!(w.picker_titles(), ["B"]);
    // Any of: one of them
    w.click("Any of");
    assert_eq!(w.picker().query, "NOT #todo OR status:done");
    w.hl.settle(&mut w.app);
    assert_eq!(w.picker_titles(), ["B", "C"]);
}

// spec: QUERY-027, QUERY-025
#[test]
fn query_027_what_is_typed_in_the_box_fills_the_form() {
    let mut w = W::new("picker-typed");
    w.open_picker();
    w.click("⚙");
    w.app.modal = match w.app.modal.take() {
        Some(Modal::Picker(mut p)) => {
            p.query = "#todo OR status:done".into();
            Some(Modal::Picker(p))
        }
        m => m,
    };
    w.hl.frames(&mut w.app, 3);
    let (mode, rows) = w.picker().form.rows();
    assert_eq!((mode, rows.len()), (crate::search_form::Mode::Any, 2));
    assert!(w.has("tag is") && w.has("property is"));
    // a query that is more than one level is shown as it is: too complex, nothing written
    w.app.modal = match w.app.modal.take() {
        Some(Modal::Picker(mut p)) => {
            p.query = "(#todo OR status:done) AND banana".into();
            Some(Modal::Picker(p))
        }
        m => m,
    };
    w.hl.frames(&mut w.app, 3);
    assert!(w.says("This query is too complex for the form"));
    assert!(!w.has("All of") && !w.has("+ Add filter"));
    assert_eq!(w.picker().query, "(#todo OR status:done) AND banana", "the form left the query alone");
    assert!(!w.app.cfg.search.advanced, "the setting is not changed by it");
}

// spec: QUERY-024, QUERY-026
#[test]
fn query_026_the_last_advanced_choice_is_remembered_and_the_count_is_live() {
    let mut w = W::new("picker-advanced");
    assert!(!w.app.cfg.search.advanced, "the form to start with");
    w.open_picker();
    w.click("⚙");
    w.click("Advanced");
    assert!(w.app.cfg.search.advanced, "the choice is a setting");
    assert!(w.says("Edit the query in the box above."));
    assert!(!w.has("+ Add filter"));
    // the live count is the count of the search that ran
    w.app.modal = match w.app.modal.take() {
        Some(Modal::Picker(mut p)) => {
            p.query = "banana".into();
            Some(Modal::Picker(p))
        }
        m => m,
    };
    w.settle();
    w.hl.frames(&mut w.app, 2);
    assert!(w.says("2 matches"), "banana is in A and B");
    // another picker, later: the form comes up in Advanced
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_none());
    w.open_picker();
    w.click("⚙");
    assert!(w.says("Edit the query in the box above.") && !w.has("All of"));
    w.click("Advanced");
    assert!(!w.app.cfg.search.advanced);
    assert!(w.has("All of"));
}

// spec: QUERY-027
#[test]
fn query_027_the_keyboard_opens_the_form_walks_it_and_esc_still_closes_the_picker() {
    let mut w = W::new("picker-keys");
    w.open_picker();
    w.press(Key::G, CTRL_SHIFT);
    assert!(w.picker().form_open, "the key of the form opens it");
    assert!(w.hl.ctx().memory(|m| m.focused()).is_some(), "the form has the keyboard");
    // typing now goes to the form, not to the box
    let before = w.hl.ctx().memory(|m| m.focused());
    w.press(Key::Tab, Modifiers::NONE);
    assert_ne!(w.hl.ctx().memory(|m| m.focused()), before, "Tab moves inside the form, it does not step through the sources");
    assert_eq!(w.picker().filter, crate::search_sources::Filter::default(), "no source was soloed");
    w.press(Key::Enter, Modifiers::NONE);
    assert!(w.app.modal.is_some(), "Enter on a button of the form does not pick a hit");
    // the key again closes it, and the box has the keyboard back
    w.press(Key::G, CTRL_SHIFT);
    assert!(!w.picker().form_open);
    w.hl.frames(&mut w.app, 2);
    w.hl.frame(&mut w.app, vec![Event::Text("ban".into())]);
    assert_eq!(w.picker().query, "ban", "typing goes to the box");
    // Esc closes the picker, also with the form open and a widget of it focused
    w.press(Key::G, CTRL_SHIFT);
    assert!(w.picker().form_open);
    w.press(Key::Escape, Modifiers::NONE);
    assert!(w.app.modal.is_none());
}

// spec: QUERY-027
#[test]
fn query_027_the_sidebar_search_has_the_same_form_next_to_its_box() {
    let mut w = W::new("sidebar");
    w.open_sidebar();
    assert!(w.has("⚙") && !w.has("All of"));
    w.click("⚙");
    assert!(w.app.search.form_open && w.has("All of") && w.has("Advanced"));
    w.pick("+ Add filter", "tag is");
    w.type_in_row("tag is", "todo");
    assert_eq!(w.app.search.query, "#todo");
    w.settle();
    let notes: Vec<String> = w.app.search.listed().iter().map(|h| h.title.clone()).collect();
    assert!(
        !notes.is_empty() && w.app.search.listed().iter().all(|h| matches!(&h.action, crate::app::Action::GotoMatch { rel, .. } | crate::app::Action::Open { rel, .. } if rel.to_string_lossy() == "A.md")),
        "only A has the tag: {notes:?}"
    );
    // the same Advanced setting as the picker's
    w.click("Advanced");
    assert!(w.app.cfg.search.advanced);
    assert!(w.says("Edit the query in the box above."));
    w.settle();
    w.hl.frames(&mut w.app, 2);
    assert!(w.says("1 match") || w.says("matches"), "the count of the sidebar's search");
    w.click("Advanced");
    assert!(!w.app.cfg.search.advanced && w.has("+ Add filter"));
    // the key opens and closes it from the box
    w.click("⚙");
    assert!(!w.app.search.form_open);
    w.app.search.focus = true;
    w.hl.frames(&mut w.app, 2);
    w.press(Key::G, CTRL_SHIFT);
    assert!(w.app.search.form_open);
    w.press(Key::G, CTRL_SHIFT);
    assert!(!w.app.search.form_open);
}

// spec: QUERY-025, QUERY-027
#[test]
fn query_025_a_prefix_of_the_sidebar_query_stays_when_the_form_writes() {
    let mut w = W::new("sidebar-prefix");
    w.open_sidebar();
    w.app.search.query = "f: #todo".into();
    w.click("⚙");
    w.hl.frames(&mut w.app, 2);
    w.pick("+ Add filter", "is blocked");
    assert_eq!(w.app.search.query, "f: #todo AND blocked");
}
