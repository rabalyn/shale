//! Folding in the editor (EDIT-070…EDIT-079, KEY-030…KEY-034) driven in the whole window with real
//! clicks and key presses.

use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::path::{Path, PathBuf};

const NOTE: &str = "A.md";

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

const CTRL_SHIFT: Modifiers = Modifiers { alt: false, ctrl: true, shift: true, mac_cmd: false, command: true };

impl W {
    fn new(tag: &str, note: &str, preview: bool) -> W {
        let dir = std::env::temp_dir().join(format!("shale-fold-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(NOTE), note).unwrap();
        std::fs::write(dir.join("B.md"), "# Other\nx\ny").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.date_prefix_new_notes = false;
        app.cfg.cursor_blink = false;
        app.cfg.line_numbers = true;
        app.cfg.live_preview = preview;
        app.keymap = crate::keys::Keymap::from_overrides_for(&app.cfg.keybindings, false);
        app.open(Path::new(NOTE), false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 4);
        w
    }

    fn editor(&self) -> &crate::editor::NoteEditor {
        &self.app.tabbar.tabs[self.app.tabbar.active].editor
    }

    fn text(&self) -> String {
        self.app.notes.get(Path::new(NOTE)).unwrap().text.clone()
    }

    /// The cursor as a char index.
    fn cursor(&self) -> usize {
        egui::text_edit::TextEditState::load(self.hl.ctx(), self.editor().id).and_then(|s| s.cursor.char_range()).map(|r| r.primary.index.0).unwrap()
    }

    /// The line (from 0) of the cursor.
    fn cursor_line(&self) -> usize {
        let text = self.text();
        crate::editor_preview::line_of(&text, crate::markdown::char_to_byte(&text, self.cursor()))
    }

    /// Put the cursor at the start of the line with `needle`, the keyboard in the editor.
    fn cursor_in(&mut self, needle: &str) {
        let text = self.text();
        let at = crate::markdown::byte_to_char(&text, text.find(needle).unwrap());
        self.app.focus_editor();
        self.app.tabbar.tabs[self.app.tabbar.active].editor.pending_cursor = Some(at);
        self.hl.frames(&mut self.app, 4);
    }

    fn press(&mut self, key: Key, modifiers: Modifiers) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.hl.frames(&mut self.app, 3);
    }

    fn text_x(&self) -> f32 {
        self.editor().last_layout.as_ref().unwrap().1.x
    }

    /// The screen rectangle of the first row of the line that starts with `needle`.
    fn row_of(&self, needle: &str) -> Rect {
        let (galley, pos) = self.editor().last_layout.clone().unwrap();
        let row = galley.rows.iter().find(|r| r.row.glyphs.iter().map(|g| g.chr).collect::<String>().starts_with(needle)).unwrap_or_else(|| panic!("no row starting with {needle:?}"));
        Rect::from_min_size(pos + row.pos.to_vec2(), row.row.size)
    }

    /// The heights of the rows of the laid-out note, one per line (no line of these notes wraps).
    fn heights(&self) -> Vec<f32> {
        self.editor().last_layout.clone().unwrap().0.rows.iter().map(|r| r.row.size.y).collect()
    }

    /// How many lines have rows of no height.
    fn hidden(&self) -> usize {
        self.heights().iter().filter(|h| **h < 1.0).count()
    }

    /// The line numbers painted in the gutter.
    fn numbers(&mut self) -> Vec<usize> {
        self.hl.frames(&mut self.app, 1);
        let x = self.text_x();
        let mut n: Vec<usize> = self.hl.text.iter().filter(|(_, p)| p.x < x && p.x > x - 60.0).filter_map(|(t, _)| t.parse().ok()).collect();
        n.sort_unstable();
        n
    }

    fn hover(&mut self, p: Pos2) {
        self.hl.frame(&mut self.app, vec![Event::PointerMoved(p)]);
        self.hl.frames(&mut self.app, 1);
    }

    fn click(&mut self, p: Pos2) {
        self.hl.click(&mut self.app, p, PointerButton::Primary);
    }

    /// The marker of the header line that starts with `needle`: the pointer goes over its row first, so that it shows.
    fn marker(&mut self, needle: &str) -> Rect {
        let row = self.row_of(needle);
        let x = self.text_x();
        self.hover(Pos2::new(x + 30.0, row.center().y));
        let line = crate::editor_preview::line_of(&self.text(), self.text().find(needle).unwrap());
        self.editor().fold.widgets.iter().find(|w| w.line == line).and_then(|w| w.marker).unwrap_or_else(|| panic!("no marker at {needle:?}: {:?}", self.editor().fold.widgets))
    }

    fn badge(&self, line: usize) -> Option<Rect> {
        self.editor().fold.widgets.iter().find(|w| w.line == line).and_then(|w| w.badge)
    }

    fn painted(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t == text)
    }
}

/// Seven lines: # One (with ## Sub in it) and # Two, no line break at the end.
const SECTIONS: &str = "# One\nfirst\nsecond\n## Sub\nthird\n# Two\nfourth";

// spec: EDIT-071, EDIT-074, EDIT-018
#[test]
fn edit_071_a_folded_block_shows_its_header_and_a_badge_and_hides_its_lines_with_and_without_the_preview() {
    for preview in [false, true] {
        let mut w = W::new(&format!("hidden-{preview}"), SECTIONS, preview);
        w.cursor_in("# Two");
        assert_eq!(w.hidden(), 0, "nothing is hidden before a fold");
        assert_eq!(w.numbers(), vec![1, 2, 3, 4, 5, 6, 7]);
        // a click on the marker of "# One" folds it
        let m = w.marker("# One");
        w.click(m.center());
        let h = w.heights();
        assert_eq!(w.hidden(), 4, "preview {preview}: the four lines under # One have no height: {h:?}");
        assert!(h[0] > 1.0 && h[5] > 1.0, "the header and what follows stay");
        assert_eq!(w.numbers(), vec![1, 6, 7], "preview {preview}: no number for a hidden line (EDIT-018)");
        assert!(w.painted("\u{2026} 4 lines"), "the badge: {:?}", w.hl.text);
        assert!(w.badge(0).is_some());
        // the folded text is still in the note, and the marker of a folded block stays without hovering
        assert_eq!(w.text(), SECTIONS);
        w.hover(Pos2::new(5.0, 5.0));
        assert!(w.editor().fold.widgets.iter().any(|x| x.line == 0 && x.marker.is_some()));
        // the badge unfolds it
        let badge = w.badge(0).unwrap();
        w.click(badge.center());
        assert_eq!(w.hidden(), 0, "preview {preview}: the badge unfolds");
        assert_eq!(w.numbers(), vec![1, 2, 3, 4, 5, 6, 7]);
    }
}

// spec: EDIT-074
#[test]
fn edit_074_the_marker_shows_on_hover_and_a_click_on_it_toggles_the_fold() {
    let mut w = W::new("marker", SECTIONS, false);
    w.cursor_in("# Two");
    // no marker while the pointer is elsewhere
    w.hover(Pos2::new(700.0, 600.0));
    assert!(w.editor().fold.widgets.iter().all(|x| x.marker.is_none()), "{:?}", w.editor().fold.widgets);
    // on a line that is no header there is none either
    let row = w.row_of("first");
    let x = w.text_x();
    w.hover(Pos2::new(x + 30.0, row.center().y));
    assert!(w.editor().fold.widgets.iter().all(|x| x.marker.is_none()));
    // over a header there is one, left of the text
    let m = w.marker("## Sub");
    assert!(m.max.x < x, "left of the text: {m:?} {x}");
    // "## Sub" has only "third" under it
    w.click(m.center());
    assert!(w.painted("\u{2026} 1 line"), "{:?}", w.hl.text);
    assert_eq!(w.numbers(), vec![1, 2, 3, 4, 6, 7]);
    // the marker of the folded block unfolds it
    let m = w.marker("## Sub");
    w.click(m.center());
    assert_eq!(w.hidden(), 0);
    // the folds are not written anywhere
    assert_eq!(std::fs::read_to_string(w.dir.join(NOTE)).unwrap(), SECTIONS);
}

// spec: EDIT-074
#[test]
fn edit_074_without_the_gutter_the_marker_is_left_of_the_text() {
    let mut w = W::new("no-gutter", SECTIONS, false);
    w.app.cfg.line_numbers = false;
    w.cursor_in("# Two");
    let x = w.text_x();
    let m = w.marker("# One");
    assert!(m.max.x < x && m.min.x > x - 20.0, "{m:?} {x}");
    w.click(m.center());
    assert_eq!(w.hidden(), 4);
}

// spec: EDIT-073, KEY-030, KEY-031, KEY-032, KEY-033
#[test]
fn key_030_the_fold_keys_fold_and_unfold_at_the_cursor_and_all() {
    let mut w = W::new("keys", SECTIONS, true);
    w.cursor_in("## Sub");
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.hidden(), 1, "## Sub is folded");
    assert_eq!(w.numbers(), vec![1, 2, 3, 4, 6, 7]);
    w.press(Key::J, CTRL_SHIFT);
    assert_eq!(w.hidden(), 0, "unfold");
    // fold all, from a line inside a block: the cursor goes to the header that stays
    w.cursor_in("second");
    w.press(Key::U, CTRL_SHIFT);
    assert_eq!(w.numbers(), vec![1, 6], "only the two # headings stay");
    assert_eq!(w.cursor_line(), 0, "the cursor is on the header of the fold that hid its line");
    assert_eq!(w.cursor(), "# One".len());
    w.press(Key::I, CTRL_SHIFT);
    assert_eq!(w.hidden(), 0);
    // the keys do nothing to the text
    assert_eq!(w.text(), SECTIONS);
}

// spec: EDIT-075
#[test]
fn edit_075_the_cursor_moving_into_a_folded_block_unfolds_it() {
    let mut w = W::new("cursor", SECTIONS, false);
    w.cursor_in("## Sub");
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.hidden(), 1);
    // at the end of the header the cursor is still outside
    w.press(Key::End, Modifiers::NONE);
    assert_eq!(w.hidden(), 1, "still folded");
    // Down goes into the hidden line: the block opens, and the cursor is in it
    w.press(Key::ArrowDown, Modifiers::NONE);
    assert_eq!(w.hidden(), 0, "Down into the block unfolds it");
    assert_eq!(w.cursor_line(), 4, "the cursor is on the line that was hidden");
    // a jump to a line in a fold (a link, a search) unfolds what hides it, and only that
    w.cursor_in("# One");
    w.press(Key::U, CTRL_SHIFT);
    assert_eq!(w.numbers(), vec![1, 6]);
    let text = w.text();
    w.app.tabbar.tabs[w.app.tabbar.active].editor.pending_cursor = Some(crate::markdown::byte_to_char(&text, text.find("third").unwrap()));
    w.hl.frames(&mut w.app, 5);
    assert_eq!(w.numbers(), vec![1, 2, 3, 4, 5, 6], "# One and ## Sub are open, # Two stays folded");
}

// spec: EDIT-076
#[test]
fn edit_076_selecting_everything_leaves_folds_alone_and_typing_and_undo_work_around_them() {
    let mut w = W::new("select", SECTIONS, false);
    w.cursor_in("# Two");
    w.press(Key::K, CTRL_SHIFT);
    w.cursor_in("# One");
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.numbers(), vec![1, 6]);
    // Ctrl+A selects the hidden text too, and nothing unfolds
    w.press(Key::A, Modifiers::COMMAND);
    assert_eq!(w.numbers(), vec![1, 6], "still folded with everything selected");
    // typing at the end of a header goes into the note, and the fold stays with its block
    w.cursor_in("# One");
    w.press(Key::End, Modifiers::NONE);
    w.hl.frame(&mut w.app, vec![Event::Text("!".into())]);
    w.hl.frames(&mut w.app, 3);
    assert_eq!(w.text(), SECTIONS.replace("# One", "# One!"));
    assert_eq!(w.numbers(), vec![1, 6], "editing the header keeps it folded");
    // Ctrl+Z takes the typing back and leaves the fold
    w.press(Key::Z, Modifiers::COMMAND);
    assert_eq!(w.text(), SECTIONS);
    assert_eq!(w.numbers(), vec![1, 6]);
    // lines added above keep the fold on its block
    w.app.tabbar.tabs[w.app.tabbar.active].editor.pending_cursor = Some(0);
    w.hl.frames(&mut w.app, 3);
    w.app.notes.get_mut(Path::new(NOTE)).unwrap().text = format!("intro\n\n{SECTIONS}");
    w.hl.frames(&mut w.app, 4);
    assert_eq!(w.numbers(), vec![1, 2, 3, 8], "intro, a blank line, # One and # Two");
}

// spec: EDIT-072
#[test]
fn edit_072_folds_are_kept_per_note_for_the_session_and_not_in_the_file() {
    let mut w = W::new("per-note", SECTIONS, false);
    w.cursor_in("# Two");
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.numbers(), vec![1, 2, 3, 4, 5, 6]);
    // another note, and back
    w.app.open(Path::new("B.md"), false);
    w.hl.frames(&mut w.app, 4);
    assert_eq!(w.numbers(), vec![1, 2, 3], "B has no folds");
    w.app.open(Path::new(NOTE), false);
    w.hl.frames(&mut w.app, 4);
    assert_eq!(w.numbers(), vec![1, 2, 3, 4, 5, 6], "A is folded again where it was");
    assert_eq!(std::fs::read_to_string(w.dir.join(NOTE)).unwrap(), SECTIONS);
}

// spec: EDIT-070, EDIT-071
#[test]
fn edit_070_quotes_and_lists_fold_too_and_a_table_inside_a_fold_is_not_drawn() {
    let note = "> q1\n> q2\n> q3\n\n- item\n    - child\n    - child 2\n- next\n\n# H\n\n| Alpha | Beta |\n|---|---|\n| one | two |\n\nafter";
    let mut w = W::new("kinds", note, true);
    w.cursor_in("after");
    assert!(w.painted("Alpha") && w.painted("Beta"), "the table is a grid: {:?}", w.hl.text);
    w.cursor_in("> q1");
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.heights().iter().take(3).filter(|h| **h < 1.0).count(), 2, "the quote's two last lines");
    w.cursor_in("- item");
    w.press(Key::K, CTRL_SHIFT);
    let n = w.numbers();
    assert!(n.contains(&5) && !n.contains(&6) && !n.contains(&7) && n.contains(&8), "{n:?}");
    // a heading section with a table: folded, the grid is not drawn over the hidden text
    w.cursor_in("# H");
    w.press(Key::K, CTRL_SHIFT);
    w.hl.frames(&mut w.app, 2);
    assert!(!w.painted("Alpha") && !w.painted("Beta"), "no grid cells: {:?}", w.hl.text);
    assert!(w.painted("\u{2026} 6 lines"), "{:?}", w.hl.text);
}

// spec: EDIT-073
#[test]
fn edit_073_unfolding_an_outer_block_leaves_the_folds_inside_it() {
    let mut w = W::new("nested", SECTIONS, false);
    w.cursor_in("## Sub");
    w.press(Key::K, CTRL_SHIFT);
    w.cursor_in("# One");
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.numbers(), vec![1, 6, 7]);
    w.press(Key::J, CTRL_SHIFT);
    assert_eq!(w.numbers(), vec![1, 2, 3, 4, 6, 7], "## Sub stays folded");
}

// spec: EDIT-078
#[test]
fn edit_078_vim_za_zc_zo_zm_zr_fold_and_unfold() {
    let mut w = W::new("vim", SECTIONS, false);
    w.app.cfg.vim_mode = true;
    w.hl.frames(&mut w.app, 3);
    w.cursor_in("## Sub");
    let type_keys = |w: &mut W, keys: &str| {
        w.hl.frame(&mut w.app, vec![Event::Text(keys.into())]);
        w.hl.frames(&mut w.app, 3);
    };
    type_keys(&mut w, "zc");
    assert_eq!(w.hidden(), 1, "zc folds ## Sub");
    type_keys(&mut w, "zo");
    assert_eq!(w.hidden(), 0, "zo opens it");
    type_keys(&mut w, "za");
    assert_eq!(w.hidden(), 1, "za folds");
    type_keys(&mut w, "za");
    assert_eq!(w.hidden(), 0, "and unfolds");
    type_keys(&mut w, "zM");
    assert_eq!(w.numbers(), vec![1, 6], "zM folds all");
    type_keys(&mut w, "zR");
    assert_eq!(w.hidden(), 0, "zR opens all");
    assert_eq!(w.text(), SECTIONS, "the text is not touched");
}
