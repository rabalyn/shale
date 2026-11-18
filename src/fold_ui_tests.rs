//! Folding in the editor (EDIT-070…EDIT-086, KEY-030…KEY-034) driven in the whole window with real
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
        W::new_with(tag, note, preview, |_| {})
    }

    /// Like `new`, with a look at the app's settings before the note is opened.
    fn new_with(tag: &str, note: &str, preview: bool, setup: impl FnOnce(&mut ShaleApp)) -> W {
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
        setup(&mut app);
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
    w.press(Key::O, CTRL_SHIFT);
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
    w.press(Key::O, CTRL_SHIFT);
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

/// A note with a code block: lines 0 to 6.
const CODE: &str = "intro\n\n```rust\nfn main() {}\nlet x = 1;\n```\n\nafter";

impl W {
    /// The rectangles painted with the code background (the boxes of the code blocks).
    fn boxes(&self) -> Vec<Rect> {
        self.hl.rects.iter().filter(|(_, c)| *c == crate::theme::CODE_BG).map(|(r, _)| *r).collect()
    }
}

// spec: EDIT-080
#[test]
fn edit_080_a_code_block_in_the_preview_is_a_box_with_a_language_badge_and_without_its_fences() {
    let mut w = W::new("code-box", CODE, true);
    w.cursor_in("after");
    // the closing fence has no row, the opening one keeps its row for the badge but shows no text
    let h = w.heights();
    assert!(h[5] < 1.0, "the closing fence is hidden: {h:?}");
    assert!(h[2] > 1.0 && h[3] > 1.0, "the opening fence keeps its row: {h:?}");
    let boxes = w.boxes();
    assert_eq!(boxes.len(), 1, "one box: {boxes:?}");
    let (top, last) = (w.row_of("```rust").min.y, w.row_of("let x").max.y);
    assert!(boxes[0].min.y <= top + 0.5 && boxes[0].max.y >= last, "the box spans the block: {:?} {top} {last}", boxes[0]);
    assert!(boxes[0].min.x < w.text_x() && boxes[0].max.x > w.text_x() + 200.0, "and the text width: {:?}", boxes[0]);
    assert!(w.painted("rust"), "the badge names the language: {:?}", w.hl.text);
    // the opening fence's text is not drawn: its glyphs are invisible
    let (galley, _) = w.editor().last_layout.clone().unwrap();
    let fence_row = &galley.rows[2];
    assert!(fence_row.row.glyphs.iter().all(|g| g.size().x < 2.0), "hidden like other markup: {:?}", fence_row.row.glyphs.iter().map(|g| g.size().x).collect::<Vec<_>>());
    // the cursor in the block shows the fences raw again, the box stays
    w.cursor_in("let x");
    assert_eq!(w.hidden(), 0, "both fences are there: {:?}", w.heights());
    assert_eq!(w.boxes().len(), 1);
    assert!(w.painted("rust"));
    assert_eq!(w.text(), CODE, "the text is never touched");
}

// spec: EDIT-080
#[test]
fn edit_080_a_block_without_a_language_says_code_and_with_the_preview_off_there_is_no_box() {
    let note = "```\nplain\n```\n\n~~~python\nx = 1\n~~~\n\nend";
    let mut w = W::new("code-plain", note, true);
    w.cursor_in("end");
    assert_eq!(w.boxes().len(), 2);
    assert!(w.painted("code") && w.painted("python"), "{:?}", w.hl.text);
    let mut w = W::new("code-raw", note, false);
    w.cursor_in("end");
    assert!(w.boxes().is_empty(), "the raw drawing has no box");
    assert_eq!(w.hidden(), 0, "and shows the fences");
    assert!(!w.painted("code"));
}

// spec: EDIT-080
#[test]
fn edit_080_a_code_block_folds_to_its_opening_row_with_and_without_the_preview() {
    for preview in [true, false] {
        let mut w = W::new(&format!("code-fold-{preview}"), CODE, preview);
        w.cursor_in("let x");
        // Fold from a line inside the block: the block folds and the cursor goes to its header
        w.press(Key::K, CTRL_SHIFT);
        assert_eq!(w.cursor_line(), 2, "preview {preview}");
        assert_eq!(w.numbers(), vec![1, 2, 3, 7, 8], "preview {preview}: the three lines under the fence have no number");
        assert!(w.painted("\u{2026} 3 lines"), "preview {preview}: {:?}", w.hl.text);
        if preview {
            // the box is the opening row alone, with its badge
            let b = w.boxes();
            assert_eq!(b.len(), 1);
            assert!(b[0].height() < 40.0, "{:?}", b[0]);
            assert!(w.painted("rust"));
        }
        // the key and the badge unfold it again
        w.press(Key::J, CTRL_SHIFT);
        assert_eq!(w.hidden(), 0, "preview {preview}");
        assert_eq!(w.numbers(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

/// A mail note: the front matter is lines 0 to 3, a quote is lines 6 and 7, the signature lines 10 to 12.
const MAIL: &str = "---\ntype: email\nfrom: Anna <anna@example.org>\nsubject: Hi\n---\nHello\n\n> q1\n> q2\n\nThanks\n-- \nAnna\nAcme";

// spec: EDIT-081
#[test]
fn edit_081_a_note_opens_with_its_front_matter_folded_to_a_summary_line() {
    let mut w = W::new("front", MAIL, true);
    let n = w.numbers();
    assert!(n.contains(&1) && n.contains(&6) && !n.contains(&2) && !n.contains(&5), "only the first line of the front matter stays: {n:?}");
    assert!(w.painted("Properties \u{b7} email \u{b7} Anna"), "the summary line: {:?}", w.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.editor().fold.widgets.iter().any(|x| x.line == 0 && x.marker.is_some()), "the ▸ marker shows");
    assert_eq!(w.text(), MAIL, "the text is as it was");
    // a click on the summary line opens the raw front matter, with the cursor in it
    let row = w.row_of("---");
    let x = w.text_x();
    w.click(Pos2::new(x + 40.0, row.center().y));
    assert!(w.painted("type: email") || w.numbers().contains(&2), "the lines are open: {:?}", w.numbers());
    assert!(w.cursor_line() >= 1 && w.cursor_line() <= 4, "{}", w.cursor_line());
    assert_eq!(std::fs::read_to_string(w.dir.join(NOTE)).unwrap(), MAIL);
}

// spec: EDIT-081
#[test]
fn edit_081_the_setting_raw_leaves_the_front_matter_open_and_a_note_without_one_is_not_affected() {
    let mut w = W::new_with("front-raw", MAIL, true, |app| app.cfg.properties_in_editor = crate::config::PropertiesView::Raw);
    assert!(w.numbers().contains(&3), "all the lines of the front matter show: {:?}", w.numbers());
    assert!(!w.painted("Properties \u{b7} email"));
    // it can still be folded by hand: a badge with the count, no summary
    let m = w.marker("---");
    w.click(m.center());
    assert!(w.painted("\u{2026} 4 lines"), "{:?}", w.hl.text);
    assert!(!w.painted("Properties \u{b7} email"));
    let w = W::new("front-none", SECTIONS, true);
    assert_eq!(w.hidden(), 0);
}

// spec: EDIT-084
#[test]
fn edit_084_the_cursor_entering_the_front_matter_opens_it_and_leaving_folds_it_again() {
    let mut w = W::new("front-cursor", MAIL, false);
    w.cursor_in("Hello");
    assert!(!w.numbers().contains(&2), "folded: {:?}", w.numbers());
    // Ctrl+Home goes to the first line: the raw front matter opens
    w.press(Key::Home, Modifiers::CTRL);
    assert_eq!(w.cursor_line(), 0);
    assert!(w.numbers().contains(&2) && w.numbers().contains(&4), "opened: {:?}", w.numbers());
    // moving on to the body folds it back
    w.press(Key::End, Modifiers::CTRL);
    assert!(!w.numbers().contains(&2), "folded again: {:?}", w.numbers());
    // an Up arrow from the first line of the body goes into the hidden lines: they open and take the cursor
    w.cursor_in("Hello");
    w.press(Key::ArrowUp, Modifiers::NONE);
    assert_eq!(w.cursor_line(), 4);
    assert!(w.numbers().contains(&3), "{:?}", w.numbers());
    w.press(Key::ArrowDown, Modifiers::NONE);
    assert!(!w.numbers().contains(&3), "folded again after the cursor left: {:?}", w.numbers());
}

// spec: EDIT-084
#[test]
fn edit_084_what_the_user_opened_by_hand_stays_open_and_typing_on_the_folded_line_opens_it() {
    let mut w = W::new("front-hand", MAIL, false);
    let m = w.marker("---");
    w.click(m.center());
    assert!(w.numbers().contains(&3), "opened with the marker: {:?}", w.numbers());
    w.cursor_in("subject");
    w.cursor_in("Hello");
    assert!(w.numbers().contains(&3), "the cursor leaving does not fold what the user opened: {:?}", w.numbers());
    // fold by key, put the cursor on the folded line and type: the text goes into the front matter, which opens
    w.cursor_in("---");
    w.press(Key::K, CTRL_SHIFT);
    assert!(!w.numbers().contains(&3));
    w.hl.frame(&mut w.app, vec![Event::Text("x".into())]);
    w.hl.frames(&mut w.app, 3);
    assert!(w.numbers().contains(&3), "typing opens it: {:?}", w.numbers());
}

// spec: EDIT-082, EDIT-083
#[test]
fn edit_082_a_mail_note_opens_with_its_quotes_and_signature_folded_and_other_notes_do_not() {
    let mut w = W::new("mail", MAIL, true);
    // front matter 4 lines, the quote's second line, the signature's two lines
    assert_eq!(w.hidden(), 4 + 1 + 2, "{:?}", w.heights());
    assert!(w.painted("\u{2026} 1 quoted line"), "{:?}", w.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.painted("\u{2026} 2 lines"), "the signature");
    // the folds are not in the file, and a click on the quote's badge opens it
    assert_eq!(std::fs::read_to_string(w.dir.join(NOTE)).unwrap(), MAIL);
    let badge = w.badge(7).expect("a badge on the quote's first line");
    w.click(badge.center());
    assert_eq!(w.hidden(), 4 + 2);
    // a note that is no mail keeps its quotes and signature open
    let plain = MAIL.replace("type: email", "type: note");
    let w = W::new("not-mail", &plain, true);
    assert_eq!(w.hidden(), 4, "only the front matter: {:?}", w.heights());
}

// spec: EDIT-085
#[test]
fn edit_085_the_folds_a_note_starts_with_are_set_once_per_session_and_never_written() {
    let mut w = W::new("start-once", MAIL, false);
    let m = w.marker("---");
    w.click(m.center());
    assert!(w.numbers().contains(&3));
    // another note and back: it is not folded by default a second time
    w.app.open(Path::new("B.md"), false);
    w.hl.frames(&mut w.app, 4);
    w.app.open(Path::new(NOTE), false);
    w.hl.frames(&mut w.app, 4);
    assert!(w.numbers().contains(&3), "the user's unfold is remembered: {:?}", w.numbers());
    assert_eq!(std::fs::read_to_string(w.dir.join(NOTE)).unwrap(), MAIL);
}

impl W {
    /// The texts that the frames of these events put on the clipboard, and three frames after them.
    fn copied_by(&mut self, steps: Vec<Vec<Event>>) -> Vec<String> {
        let mut out = vec![];
        let mut take = |o: egui::FullOutput| {
            out.extend(o.platform_output.commands.iter().filter_map(|c| match c {
                egui::OutputCommand::CopyText(s) => Some(s.clone()),
                _ => None,
            }))
        };
        for events in steps {
            take(self.hl.frame(&mut self.app, events));
        }
        for _ in 0..3 {
            take(self.hl.frame(&mut self.app, vec![]));
        }
        out
    }

    fn click_copied(&mut self, p: Pos2) -> Vec<String> {
        self.copied_by(vec![vec![Event::PointerMoved(p)], vec![crate::ui_test_support::Headless::button(p, PointerButton::Primary, true)], vec![crate::ui_test_support::Headless::button(p, PointerButton::Primary, false)]])
    }

    fn key_copied(&mut self, key: Key) -> Vec<String> {
        self.copied_by(vec![vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: CTRL_SHIFT }]])
    }

    /// Where the copy button of the code block that opens at `line` is, if it is shown.
    fn copy_button(&self, line: usize) -> Option<Rect> {
        self.editor().fold.widgets.iter().find(|w| w.line == line).and_then(|w| w.copy)
    }
}

// spec: EDIT-090, EDIT-091, EDIT-094
#[test]
fn edit_090_the_copy_button_shows_on_hover_and_a_click_copies_the_code_without_touching_cursor_or_text() {
    let mut w = W::new("copy-click", CODE, true);
    w.cursor_in("after");
    w.hover(Pos2::new(700.0, 600.0));
    assert!(w.copy_button(2).is_none(), "no button while the pointer and the cursor are elsewhere");
    let row = w.row_of("let x");
    let x = w.text_x();
    w.hover(Pos2::new(x + 30.0, row.center().y));
    let button = w.copy_button(2).expect("the pointer over the box shows the button");
    let box_ = w.boxes()[0];
    assert!(button.max.x < box_.max.x - 20.0 && button.center().y < row.min.y, "top right of the box, left of the badge: {button:?} {box_:?}");
    let cursor = w.cursor();
    let copied = w.click_copied(button.center());
    assert_eq!(copied, vec!["fn main() {}\nlet x = 1;".to_string()], "the code only, no fences, no newline at the end");
    assert_eq!(w.cursor(), cursor, "the click did not move the cursor into the block");
    assert_eq!(w.hidden(), 1, "the closing fence is still hidden: the cursor is not in the block");
    assert_eq!(w.text(), CODE);
    assert!(w.painted("Copied"), "the button says so: {:?}", w.hl.text);
    // after a moment it is the icon again
    w.hl.frames(&mut w.app, 40);
    assert!(!w.painted("Copied"));
    // with the preview off there is no box and no button
    let mut raw = W::new("copy-raw", CODE, false);
    raw.cursor_in("let x");
    assert!(raw.copy_button(2).is_none());
}

// spec: EDIT-090
#[test]
fn edit_090_the_button_shows_while_the_cursor_is_in_the_block_and_on_a_folded_block_and_does_not_unfold_it() {
    let mut w = W::new("copy-cursor", CODE, true);
    w.cursor_in("let x");
    w.hover(Pos2::new(700.0, 600.0));
    assert!(w.copy_button(2).is_some(), "the cursor is in the block");
    w.press(Key::K, CTRL_SHIFT);
    w.cursor_in("after");
    w.hover(Pos2::new(700.0, 600.0));
    let numbers = w.numbers();
    assert_eq!(numbers, vec![1, 2, 3, 7, 8], "folded");
    let button = w.copy_button(2).expect("a folded block shows it always");
    let cursor = w.cursor();
    assert_eq!(w.click_copied(button.center()), vec!["fn main() {}\nlet x = 1;".to_string()]);
    assert_eq!(w.numbers(), numbers, "still folded");
    assert_eq!(w.cursor(), cursor);
}

// spec: EDIT-092, KEY-035
#[test]
fn key_035_the_key_copies_the_block_at_the_cursor_and_does_nothing_outside_one() {
    let mut w = W::new("copy-key", CODE, true);
    w.cursor_in("let x");
    assert_eq!(w.key_copied(Key::C), vec!["fn main() {}\nlet x = 1;".to_string()]);
    assert!(w.painted("Copied"));
    // on the opening fence too, and on a folded block
    w.cursor_in("```rust");
    assert_eq!(w.key_copied(Key::C).len(), 1);
    w.press(Key::K, CTRL_SHIFT);
    assert_eq!(w.key_copied(Key::C), vec!["fn main() {}\nlet x = 1;".to_string()]);
    // outside a block: nothing, and the text stays
    w.cursor_in("after");
    assert!(w.key_copied(Key::C).is_empty());
    assert_eq!(w.text(), CODE);
}

// spec: KEY-037
#[test]
fn key_037_the_copy_key_does_nothing_in_the_reading_view() {
    let mut w = W::new("copy-reading", CODE, true);
    w.cursor_in("let x");
    w.app.toggle_reading_view();
    w.hl.frames(&mut w.app, 4);
    assert!(w.key_copied(Key::C).is_empty());
}
