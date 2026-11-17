//! The formatting toolbar (EDIT-026, EDIT-027, EDIT-028) driven in the whole window with real clicks.

use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{self, text::CCursor, text::CCursorRange, Event, PointerButton};
use std::path::{Path, PathBuf};

const NOTE: &str = "Welcome.md";

struct Fx {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl Fx {
    fn new(tag: &str, text: &str) -> Fx {
        let dir = std::env::temp_dir().join(format!("shale-fbar-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.open(Path::new(NOTE), false);
        let mut hl = Headless::new();
        hl.frames(&mut app, 4);
        app.notes.get_mut(Path::new(NOTE)).unwrap().text = text.to_string();
        hl.frames(&mut app, 3);
        Fx { app, hl, dir }
    }

    fn text(&self) -> String {
        self.app.notes.get(Path::new(NOTE)).unwrap().text.clone()
    }

    fn editor_id(&self) -> egui::Id {
        self.app.tabbar.tabs[self.app.tabbar.active].editor.id
    }

    /// Select the text between two markers in chars: as a mouse selection leaves it; the editor has the keyboard.
    fn select(&mut self, from: usize, to: usize) {
        let id = self.editor_id();
        self.app.tabbar.tabs[self.app.tabbar.active].editor.request_focus = true;
        self.hl.frames(&mut self.app, 3);
        let mut st = egui::text_edit::TextEditState::load(self.hl.ctx(), id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
        st.store(self.hl.ctx(), id);
        self.hl.frames(&mut self.app, 2);
    }

    /// The selected text of the editor.
    fn selected(&self) -> String {
        let id = self.editor_id();
        let r = egui::text_edit::TextEditState::load(self.hl.ctx(), id).and_then(|st| st.cursor.char_range()).unwrap();
        let (a, b) = (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0));
        self.text().chars().skip(a).take(b - a).collect()
    }

    fn click(&mut self, label: &str) {
        let r = self.hl.find_label(&mut self.app, label);
        self.hl.click(&mut self.app, r.center(), PointerButton::Primary);
        self.hl.frames(&mut self.app, 3);
    }

    fn has_focus(&self) -> bool {
        self.hl.ctx().memory(|m| m.has_focus(self.editor_id()))
    }

    fn has(&mut self, label: &str) -> bool {
        self.hl.try_label(&mut self.app, label).is_some()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: EDIT-026
#[test]
fn the_toolbar_is_above_the_text_of_a_note_and_the_setting_and_the_reading_view_hide_it() {
    let mut fx = Fx::new("shown", "one two three\n");
    for tip in [
        "Bold: **text**",
        "Italic: *text*",
        "Strikethrough: ~~text~~",
        "Highlight: ==text==",
        "Inline code: `code`",
        "Highlight colour",
        "Font colour",
        "Heading level",
        "Bullet list: - item",
        "Numbered list: 1. item",
        "Task: - [ ] item",
        "Quote: > text",
        "Code block: ``` fence",
        "Horizontal rule: ---",
        "Table: 2 columns, 2 rows",
        "Link to a note: [[Note]]",
        "Web link: [text](https://…)",
        "Embed a file or picture: opens the file picker (![[file]])",
        "Clear formatting",
    ] {
        assert!(fx.has(tip), "{tip} is on the toolbar");
    }
    let bold = fx.hl.find_label(&mut fx.app, "Bold: **text**");
    let (_, text_at) = fx.hl.text.iter().find(|(t, _)| t.starts_with("one two")).cloned().expect("the note text is painted");
    assert!(bold.center().y < text_at.y, "the toolbar is above the text");
    // the reading view has none
    fx.app.tabbar.tabs[fx.app.tabbar.active].reading = true;
    fx.hl.frames(&mut fx.app, 3);
    assert!(!fx.has("Bold: **text**"), "not in the reading view");
    fx.app.tabbar.tabs[fx.app.tabbar.active].reading = false;
    fx.hl.frames(&mut fx.app, 3);
    assert!(fx.has("Bold: **text**"));
    // the setting
    fx.app.cfg.format_toolbar = false;
    fx.hl.frames(&mut fx.app, 3);
    assert!(!fx.has("Bold: **text**"), "Show formatting toolbar off");
    assert!(!fx.has("Highlight colour"));
}

// spec: EDIT-027
#[test]
fn a_click_wraps_the_selection_is_one_undo_step_and_gives_the_keyboard_back() {
    let mut fx = Fx::new("bold", "one two three\n");
    fx.select(4, 7);
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), "one **two** three\n");
    assert_eq!(fx.selected(), "two", "the word stays selected");
    assert!(fx.has_focus(), "the editor has the keyboard again");
    // typing goes into the note, replacing the selection
    fx.hl.frame(&mut fx.app, vec![Event::Text("2".into())]);
    fx.hl.frames(&mut fx.app, 2);
    assert_eq!(fx.text(), "one **2** three\n");
    // a second click on the same button takes the markers off
    fx.select(6, 7);
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), "one 2 three\n");
    // each click was one step of the note's history (EDIT-021)
    let mut text = fx.text();
    let rel = PathBuf::from(NOTE);
    assert!(fx.app.undo.of(&rel).undo(&mut text).is_some());
    assert_eq!(text, "one **2** three\n", "the unwrap is one step");
    assert!(fx.app.undo.of(&rel).undo(&mut text).is_some() && fx.app.undo.of(&rel).undo(&mut text).is_some());
    assert_eq!(text, "one two three\n", "and the wrap is one step");
}

// spec: EDIT-027
#[test]
fn line_buttons_and_the_heading_menu_work_on_the_lines_of_the_selection() {
    let mut fx = Fx::new("lines", "alpha\nbeta\ngamma\n");
    fx.select(2, 8);
    fx.click("Bullet list: - item");
    assert_eq!(fx.text(), "- alpha\n- beta\ngamma\n");
    fx.click("Bullet list: - item");
    assert_eq!(fx.text(), "alpha\nbeta\ngamma\n", "a second click toggles off");
    fx.select(0, 0);
    fx.click("Heading level");
    fx.click("Heading 2");
    assert_eq!(fx.text(), "## alpha\nbeta\ngamma\n");
    fx.click("Heading level");
    fx.click("Normal text");
    assert_eq!(fx.text(), "alpha\nbeta\ngamma\n");
    fx.select(0, 17);
    fx.click("Task: - [ ] item");
    assert_eq!(fx.text(), "- [ ] alpha\n- [ ] beta\n- [ ] gamma\n");
}

// spec: EDIT-027, EDIT-016
#[test]
fn the_buttons_work_with_the_markdown_preview_on() {
    let mut fx = Fx::new("preview", "one two\n\nlast line\n");
    fx.app.cfg.live_preview = true;
    fx.hl.frames(&mut fx.app, 3);
    fx.select(0, 3);
    fx.click("Italic: *text*");
    assert_eq!(fx.text(), "*one* two\n\nlast line\n");
    assert!(fx.has_focus());
    fx.click("Highlight: ==text==");
    assert_eq!(fx.text(), "*==one==* two\n\nlast line\n");
}

// spec: EDIT-027, EDIT-010
#[test]
fn with_vim_lite_on_a_click_leaves_the_editor_in_a_mode_that_takes_keys() {
    let mut fx = Fx::new("vim", "one two three\n");
    fx.app.cfg.vim_mode = true;
    fx.hl.frames(&mut fx.app, 3);
    fx.select(0, 0);
    // visual mode: the word "two"
    for keys in ["0", "w", "v", "l", "l"] {
        fx.hl.frame(&mut fx.app, vec![Event::Text(keys.into())]);
        fx.hl.frames(&mut fx.app, 2);
    }
    assert_eq!(fx.app.tabbar.tabs[fx.app.tabbar.active].editor.vim_status().as_deref(), Some("VISUAL"));
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), "one **two** three\n");
    fx.hl.frames(&mut fx.app, 3);
    let ed = &fx.app.tabbar.tabs[fx.app.tabbar.active].editor;
    assert_eq!(ed.vim_status().as_deref(), Some("NORMAL"), "normal mode: keys are commands");
    // x in normal mode deletes a character instead of typing an x
    let before = fx.text().len();
    fx.hl.frame(&mut fx.app, vec![Event::Text("x".into())]);
    fx.hl.frames(&mut fx.app, 2);
    assert_eq!(fx.text().len(), before - 1);
    // i starts insert mode, and typing works after a click there too
    fx.hl.frame(&mut fx.app, vec![Event::Text("i".into())]);
    fx.hl.frames(&mut fx.app, 2);
    assert_eq!(fx.app.tabbar.tabs[fx.app.tabbar.active].editor.vim_status().as_deref(), Some("INSERT"));
    fx.click("Italic: *text*");
    assert_eq!(fx.app.tabbar.tabs[fx.app.tabbar.active].editor.vim_status().as_deref(), Some("INSERT"), "insert mode stays");
    let n = fx.text().len();
    fx.hl.frame(&mut fx.app, vec![Event::Text("Z".into())]);
    fx.hl.frames(&mut fx.app, 2);
    assert_eq!(fx.text().len(), n + 1, "typing still goes into the note");
}

// spec: EDIT-027
#[test]
fn the_embed_button_opens_the_file_picker() {
    let mut fx = Fx::new("embed", "see \n");
    fx.select(4, 4);
    fx.click("Embed a file or picture: opens the file picker (![[file]])");
    assert_eq!(fx.text(), "see [!\n");
    assert!(fx.app.tabbar.tabs[fx.app.tabbar.active].editor.picker_visible(), "the `[!` file picker is open");
}

// spec: EDIT-028
#[test]
fn the_colour_palettes_write_canonical_tags_and_replace_the_colour_of_a_marked_text() {
    let mut fx = Fx::new("palette", "one two three\n");
    fx.select(4, 7);
    fx.click("Highlight colour");
    fx.click("Highlight colour #39FF14");
    assert_eq!(fx.text(), "one <mark style=\"background: #39FF14;\">two</mark> three\n");
    assert_eq!(fx.selected(), "two");
    // another colour replaces it
    fx.click("Highlight colour");
    fx.click("Highlight colour #FF00FF");
    assert_eq!(fx.text(), "one <mark style=\"background: #FF00FF;\">two</mark> three\n", "no nesting");
    // the font colour of the same text, from the presets of the settings
    fx.click("Font colour");
    let first = crate::colour_tags::hex(fx.app.cfg.font_colours[0]);
    fx.click(&format!("Font colour {first}"));
    assert_eq!(fx.text(), format!("one <mark style=\"background: #FF00FF;\"><span style=\"color: {first};\">two</span></mark> three\n"), "the span goes inside the mark");
    // no highlight, and default colour
    fx.click("Highlight colour");
    fx.click("No highlight");
    assert_eq!(fx.text(), format!("one <span style=\"color: {first};\">two</span> three\n"));
    fx.click("Font colour");
    fx.click("Default colour");
    assert_eq!(fx.text(), "one two three\n");
    // a changed preset in the settings is what the palette offers
    fx.app.cfg.highlight_colours = vec![[1, 2, 3]];
    fx.click("Highlight colour");
    fx.click("Highlight colour #010203");
    assert_eq!(fx.text(), "one <mark style=\"background: #010203;\">two</mark> three\n");
}

// spec: EDIT-028
#[test]
fn a_custom_colour_from_the_picker_is_written_in_the_canonical_form() {
    let mut fx = Fx::new("custom", "one two three\n");
    fx.select(4, 7);
    fx.click("Highlight colour");
    fx.click("Custom colour…");
    assert!(fx.has("Apply"), "the colour picker is open");
    fx.click("Apply");
    assert_eq!(fx.text(), "one <mark style=\"background: #FFA500;\">two</mark> three\n", "the picker's start colour, as #RRGGBB");
    fx.click("Font colour");
    fx.click("Custom colour…");
    fx.click("Apply");
    assert_eq!(fx.text(), "one <mark style=\"background: #FFA500;\"><span style=\"color: #DC143C;\">two</span></mark> three\n");
    // Esc-free: the palette closed after the pick, and a click on the button opens it again
    fx.click("Highlight colour");
    assert!(fx.has("No highlight"));
}

/// The Bold and the Clear button of the toolbar drawn alone in a window `width` wide (a few frames,
/// as the bar needs one to know its width): the first and the last button.
fn bar_ends(width: f32, scale: f32) -> (egui::Rect, egui::Rect) {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut bar = crate::format_bar::FormatBar::default();
    let palettes = crate::format_bar::Palettes { highlight: &[[255, 255, 0]], font: &[[255, 0, 0]], scale };
    let mut nodes = vec![];
    for _ in 0..3 {
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 400.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
                bar.show(ui, "text", 0..0, false, &palettes);
            });
        });
        out.textures_delta.clear();
        nodes = out.platform_output.accesskit_update.take().expect("accessibility is on").nodes;
    }
    let find = |label: &str| {
        let b = nodes.iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.bounds()).unwrap_or_else(|| panic!("no {label:?}"));
        egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32))
    };
    (find("Bold: **text**"), find("Clear formatting"))
}

// spec: EDIT-026
#[test]
fn the_toolbar_is_centred_over_the_editor_and_a_wrapping_one_starts_at_the_left() {
    let (bold, clear) = bar_ends(1400.0, 1.0);
    let (left, right) = (bold.left(), 1400.0 - clear.right());
    assert!(left > 100.0, "space left of a bar that fits: {left}");
    assert!((left - right).abs() <= 4.0, "as much space on both sides: {left} / {right}");
    let (bold, clear) = bar_ends(300.0, 1.0);
    assert!(bold.left() < 20.0, "a bar that wraps starts at the left: {bold:?}");
    assert!(clear.top() > bold.bottom(), "and has more than one row: {clear:?}");
}

// spec: EDIT-026
#[test]
fn the_toolbar_button_size_setting_scales_the_buttons() {
    let (bold, clear) = bar_ends(1400.0, 1.0);
    let (big_bold, big_clear) = bar_ends(1400.0, 1.5);
    assert!((bold.height() - 26.0).abs() < 0.5, "base size: {bold:?}");
    assert!((big_bold.height() - 39.0).abs() < 0.5, "150 %: {big_bold:?}");
    assert!((big_clear.width() - clear.width() * 1.5).abs() < 0.5, "drawn buttons grow too");
    let (w, big_w) = (clear.right() - bold.left(), big_clear.right() - big_bold.left());
    assert!(big_w > w * 1.4, "the whole bar grows: {w} -> {big_w}");
    assert!(((1400.0 - big_clear.right()) - big_bold.left()).abs() <= 4.0, "and stays centred");
}

// spec: EDIT-026
#[test]
fn the_app_draws_the_toolbar_at_the_size_of_the_setting() {
    let mut fx = Fx::new("size", "one two");
    let base = fx.hl.find_label(&mut fx.app, "Bold: **text**");
    assert!((base.height() - 26.0 * 1.1).abs() < 0.5, "110 % by default: {base:?}");
    fx.app.cfg.toolbar_size = 200;
    fx.hl.frames(&mut fx.app, 3);
    assert!((fx.hl.find_label(&mut fx.app, "Bold: **text**").height() - 52.0).abs() < 0.5);
    fx.app.cfg.toolbar_size = 5;
    fx.hl.frames(&mut fx.app, 3);
    assert!((fx.hl.find_label(&mut fx.app, "Bold: **text**").height() - 26.0 * 0.8).abs() < 0.5, "a value out of range is clamped to 80 %");
}

const CELLS: &str = "before\n\n| A | B |\n|---|---|\n| one two | x |\n";
const ESC: Event = Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };

impl Fx {
    /// A note with a table drawn as a grid (the markdown preview is on).
    fn table(tag: &str) -> Fx {
        let mut fx = Fx::new(tag, CELLS);
        fx.app.cfg.live_preview = true;
        fx.hl.frames(&mut fx.app, 4);
        fx
    }

    /// Click into the cell with this text (it is edited in the grid).
    fn edit_cell(&mut self, text: &str) {
        let (_, at) = self.hl.text.iter().find(|(t, _)| t == text).cloned().unwrap_or_else(|| panic!("{text} is not painted"));
        self.hl.click(&mut self.app, at + egui::Vec2::new(4.0, 6.0), PointerButton::Primary);
        assert!(self.editor().editing_cell().is_some(), "the cell of {text} is edited");
    }

    fn editor(&self) -> &crate::editor::NoteEditor {
        &self.app.tabbar.tabs[self.app.tabbar.active].editor
    }

    /// Select chars in the box of the cell being edited (it has the keyboard).
    fn select_in_cell(&mut self, from: usize, to: usize) {
        let id = self.editor().cell_box_id();
        let mut st = egui::text_edit::TextEditState::load(self.hl.ctx(), id).unwrap_or_default();
        st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(from), CCursor::new(to))));
        st.store(self.hl.ctx(), id);
        self.hl.frames(&mut self.app, 2);
    }

    fn selected_in_cell(&self) -> (usize, usize) {
        let r = egui::text_edit::TextEditState::load(self.hl.ctx(), self.editor().cell_box_id()).and_then(|st| st.cursor.char_range()).unwrap();
        (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0))
    }

    fn escape(&mut self) {
        self.hl.frame(&mut self.app, vec![ESC]);
        self.hl.frames(&mut self.app, 3);
    }
}

// spec: EDIT-030, EDIT-027
#[test]
fn in_a_cell_the_inline_buttons_format_the_cell_selection_and_the_cursor_and_undo_takes_it_back_with_the_edit() {
    let mut fx = Fx::table("cell-inline");
    fx.edit_cell("one two");
    fx.select_in_cell(4, 7);
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), "before\n\n| A | B |\n|---|---|\n| one **two** | x |\n", "the selection in the cell is wrapped, the note around it is not touched");
    assert_eq!(fx.editor().editing_cell(), Some((1, 0)), "the cell is still edited");
    assert_eq!(fx.selected_in_cell(), (6, 9), "the word stays selected, inside the markers");
    // a cursor with no selection: the pair goes in with the cursor between, and typing goes in there
    fx.select_in_cell(3, 3);
    fx.click("Inline code: `code`");
    fx.hl.frame(&mut fx.app, vec![Event::Text("y".into())]);
    fx.hl.frames(&mut fx.app, 2);
    assert_eq!(fx.text(), "before\n\n| A | B |\n|---|---|\n| one`y` **two** | x |\n");
    // the same transforms toggle off again
    fx.select_in_cell(9, 12);
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), "before\n\n| A | B |\n|---|---|\n| one`y` two | x |\n");
    // leaving the cell: everything is one step of the note's undo history (EDIT-021)
    fx.escape();
    assert_eq!(fx.editor().editing_cell(), None);
    let mut text = fx.text();
    assert!(fx.app.undo.of(&PathBuf::from(NOTE)).undo(&mut text).is_some());
    assert_eq!(text, CELLS, "one undo takes the formatting back together with the typing of the cell");
}

// spec: EDIT-030, EDIT-026
#[test]
fn while_a_cell_is_edited_the_line_buttons_are_greyed_out_and_do_nothing() {
    let mut fx = Fx::table("cell-lines");
    fx.edit_cell("one two");
    fx.select_in_cell(0, 3);
    for tip in [
        "Bullet list: - item",
        "Numbered list: 1. item",
        "Task: - [ ] item",
        "Quote: > text",
        "Code block: ``` fence",
        "Horizontal rule: ---",
        "Table: 2 columns, 2 rows",
        "Heading level",
        "Embed a file or picture: opens the file picker (![[file]])",
    ] {
        fx.click(tip);
        assert_eq!(fx.text(), CELLS, "{tip} does nothing in a cell");
        assert_eq!(fx.editor().editing_cell(), Some((1, 0)), "and the editing goes on");
    }
    assert!(!fx.editor().picker_visible(), "the embed did not start the file picker");
    fx.click("Italic: *text*");
    assert!(fx.text().contains("| *one* two |"), "the inline buttons are not greyed out: {}", fx.text());
}

// spec: EDIT-030, EDIT-027
#[test]
fn with_no_cell_edited_the_toolbar_acts_on_the_note_text_even_in_a_note_with_a_table() {
    let mut fx = Fx::table("cell-none");
    fx.select(0, 6);
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), "**before**\n\n| A | B |\n|---|---|\n| one two | x |\n");
    fx.click("Bullet list: - item");
    assert!(fx.text().starts_with("- **before**"), "the line buttons work: {}", fx.text());
    // after a cell was edited and left, the bar is back on the note
    fx.edit_cell("one two");
    fx.escape();
    fx.select(4, 10);
    fx.click("Italic: *text*");
    assert!(fx.text().starts_with("- ***before***"), "{}", fx.text());
}
