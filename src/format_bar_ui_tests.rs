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
        // these tests drive the Linux/Windows defaults of the Alt keys (KEY-015), which macOS does not have
        app.keymap = crate::keys::Keymap::from_overrides_for(&app.cfg.keybindings, false);
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
        "Highlight colour: #FFFF00",
        "Highlight colours…",
        "Font colour: #E53E3E",
        "Font colours…",
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
    assert!(!fx.has("Highlight colours…"));
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

// spec: EDIT-109
#[test]
fn the_alignment_buttons_wrap_the_paragraph_under_the_cursor_and_take_the_wrapper_off_again() {
    let mut fx = Fx::new("align", "alpha\nbeta\n\ngamma\n");
    fx.select(2, 2);
    fx.click("Centre the paragraph: <div align=\"center\">");
    assert_eq!(fx.text(), "<div align=\"center\">\n\nalpha\nbeta\n\n</div>\n\ngamma\n");
    fx.click("Align the paragraph right: <div align=\"right\">");
    assert_eq!(fx.text(), "<div align=\"right\">\n\nalpha\nbeta\n\n</div>\n\ngamma\n", "another alignment changes the wrapper");
    fx.click("Align left: no wrapper");
    assert_eq!(fx.text(), "alpha\nbeta\n\ngamma\n", "left takes it off");
    // a selection over two paragraphs is one wrapper
    fx.select(0, 14);
    fx.click("Centre the paragraph: <div align=\"center\">");
    assert_eq!(fx.text(), "<div align=\"center\">\n\nalpha\nbeta\n\ngamma\n\n</div>\n");
    // each click is a step of the note's history (EDIT-021)
    let mut text = fx.text();
    assert!(fx.app.undo.of(Path::new(NOTE)).undo(&mut text).is_some());
    assert_eq!(text, "alpha\nbeta\n\ngamma\n");
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
    fx.click("Highlight colours…");
    fx.click("Highlight colour #39FF14");
    assert_eq!(fx.text(), "one <mark style=\"background: #39FF14;\">two</mark> three\n");
    assert_eq!(fx.selected(), "two");
    // another colour replaces it
    fx.click("Highlight colours…");
    fx.click("Highlight colour #FF00FF");
    assert_eq!(fx.text(), "one <mark style=\"background: #FF00FF;\">two</mark> three\n", "no nesting");
    // the font colour of the same text, from the presets of the settings
    fx.click("Font colours…");
    let first = crate::colour_tags::hex(fx.app.cfg.font_colours[0]);
    fx.click(&format!("Font colour {first}"));
    assert_eq!(fx.text(), format!("one <mark style=\"background: #FF00FF;\"><span style=\"color: {first};\">two</span></mark> three\n"), "the span goes inside the mark");
    // no highlight, and default colour
    fx.click("Highlight colours…");
    fx.click("No highlight");
    assert_eq!(fx.text(), format!("one <span style=\"color: {first};\">two</span> three\n"));
    fx.click("Font colours…");
    fx.click("Default colour");
    assert_eq!(fx.text(), "one two three\n");
    // a changed preset in the settings is what the palette offers
    fx.app.cfg.highlight_colours = vec![[1, 2, 3]];
    fx.click("Highlight colours…");
    fx.click("Highlight colour #010203");
    assert_eq!(fx.text(), "one <mark style=\"background: #010203;\">two</mark> three\n");
}

// spec: EDIT-028
#[test]
fn a_custom_colour_from_the_picker_is_written_in_the_canonical_form() {
    let mut fx = Fx::new("custom", "one two three\n");
    fx.select(4, 7);
    fx.click("Highlight colours…");
    fx.click("Custom colour…");
    assert!(fx.has("Apply"), "the colour picker is open");
    fx.click("Apply");
    assert_eq!(fx.text(), "one <mark style=\"background: #FFA500;\">two</mark> three\n", "the picker's start colour, as #RRGGBB");
    fx.click("Font colours…");
    fx.click("Custom colour…");
    fx.click("Apply");
    assert_eq!(fx.text(), "one <mark style=\"background: #FFA500;\"><span style=\"color: #DC143C;\">two</span></mark> three\n");
    // Esc-free: the palette closed after the pick, and a click on the button opens it again
    fx.click("Highlight colours…");
    assert!(fx.has("No highlight"));
}

// spec: EDIT-054
#[test]
fn the_main_part_of_a_colour_button_applies_the_current_colour_as_one_undo_step() {
    let mut fx = Fx::new("split-main", "one two three\n");
    fx.select(4, 7);
    fx.click("Highlight colour: #FFFF00");
    assert_eq!(fx.text(), "one <mark style=\"background: #FFFF00;\">two</mark> three\n", "the colour at the start of the run");
    assert!(!fx.has("No highlight"), "a click on the main part opens no palette");
    let rel = PathBuf::from(NOTE);
    let mut text = fx.text();
    assert!(fx.app.undo.of(&rel).undo(&mut text).is_some());
    assert_eq!(text, "one two three\n", "one undo step");
    // a colour picked from the palette is the current colour, in the tooltip and for the main part
    fx.click("Highlight colours…");
    fx.click("Highlight colour #39FF14");
    let at = fx.text().chars().count() - "three\n".len();
    fx.select(at, at + 5);
    assert!(fx.has("Highlight colour: #39FF14"));
    fx.click("Highlight colour: #39FF14");
    assert_eq!(fx.text(), "one <mark style=\"background: #39FF14;\">two</mark> <mark style=\"background: #39FF14;\">three</mark>\n");
    // the font colour is the same
    let at = fx.text().find("two").unwrap();
    fx.select(at, at + 3);
    fx.click("Font colour: #E53E3E");
    assert!(fx.text().contains("<span style=\"color: #E53E3E;\">two</span>"), "{}", fx.text());
}

// spec: EDIT-054
#[test]
fn the_caret_part_of_a_colour_button_opens_the_palette_and_applies_nothing() {
    let mut fx = Fx::new("split-caret", "one two three\n");
    fx.select(4, 7);
    fx.click("Font colours…");
    assert!(fx.has("Default colour"), "the font palette is open");
    assert_eq!(fx.text(), "one two three\n");
}

// spec: EDIT-054
#[test]
fn no_highlight_and_default_colour_leave_the_current_colour_as_it_was() {
    let mut fx = Fx::new("split-none", "one two three\n");
    fx.select(4, 7);
    fx.click("Highlight colours…");
    fx.click("Highlight colour #39FF14");
    fx.click("Highlight colours…");
    fx.click("No highlight");
    assert_eq!(fx.text(), "one two three\n");
    assert!(fx.has("Highlight colour: #39FF14"), "still the current colour");
    fx.click("Font colours…");
    fx.click("Default colour");
    assert!(fx.has("Font colour: #E53E3E"));
}

// spec: EDIT-054, NAV-040
#[test]
fn the_current_colours_of_the_split_buttons_are_saved_with_the_vault_and_come_back() {
    let mut fx = Fx::new("split-saved", "one two three\n");
    fx.select(4, 7);
    fx.click("Highlight colours…");
    fx.click("Highlight colour #39FF14");
    fx.select(8, 13);
    fx.click("Font colours…");
    fx.click("Font colour #2F80ED");
    fx.app.vdata.save().unwrap();
    let saved = crate::fsutil::read_to_string(crate::vault_state::VaultData::state_path(&fx.dir)).unwrap();
    assert!(saved.contains("\"highlight_colour\": \"#39FF14\"") && saved.contains("\"font_colour\": \"#2F80ED\""), "{saved}");
    // a new start of the app on the same vault: the buttons show them and the main part applies them
    let mut again = ShaleApp::build(Options { screenshot: None, vault: Some(fx.dir.clone()), view: None, keys: None }, false);
    again.open_vault(&fx.dir.clone());
    again.open(Path::new(NOTE), false);
    let mut hl = Headless::new();
    hl.frames(&mut again, 4);
    again.notes.get_mut(Path::new(NOTE)).unwrap().text = "one two three\n".into();
    hl.frames(&mut again, 3);
    let id = again.tabbar.tabs[again.tabbar.active].editor.id;
    again.tabbar.tabs[again.tabbar.active].editor.request_focus = true;
    hl.frames(&mut again, 3);
    let mut st = egui::text_edit::TextEditState::load(hl.ctx(), id).unwrap_or_default();
    st.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(4), CCursor::new(7))));
    st.store(hl.ctx(), id);
    hl.frames(&mut again, 2);
    for label in ["Highlight colour: #39FF14", "Font colour: #2F80ED"] {
        assert!(hl.try_label(&mut again, label).is_some(), "{label}");
    }
    let r = hl.find_label(&mut again, "Highlight colour: #39FF14");
    hl.click(&mut again, r.center(), PointerButton::Primary);
    hl.frames(&mut again, 3);
    assert_eq!(again.notes.get(Path::new(NOTE)).unwrap().text, "one <mark style=\"background: #39FF14;\">two</mark> three\n");
    // an unreadable value gives the default again
    again.vdata.state.highlight_colour = Some("pink".into());
    assert_eq!(again.vdata.state.toolbar_colours()[0], crate::vault_state::DEFAULT_HIGHLIGHT);
}

// spec: EDIT-054, EDIT-030
#[test]
fn in_a_cell_the_main_part_of_a_colour_button_colours_the_cell_text() {
    let mut fx = Fx::table("split-cell");
    fx.edit_cell("one two");
    fx.select_in_cell(4, 7);
    fx.click("Font colour: #E53E3E");
    assert_eq!(fx.text(), "before\n\n| A | B |\n|---|---|\n| one <span style=\"color: #E53E3E;\">two</span> | x |\n");
    assert_eq!(fx.editor().editing_cell(), Some((1, 0)), "the cell is still edited");
}

/// The Bold and the Clear button of the toolbar drawn alone in a window `width` wide (a few frames,
/// as the bar needs one to know its width): the first and the last button.
fn bar_ends(width: f32, scale: f32) -> (egui::Rect, egui::Rect) {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut bar = crate::format_bar::FormatBar::default();
    let palettes = crate::format_bar::Palettes { badges: &Default::default(), highlight: &[[255, 255, 0]], font: &[[255, 0, 0]], scale };
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
        "Centre the paragraph: <div align=\"center\">",
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

const BLOCK: &str = "before\n\n| A | B | C |\n|---|---|---|\n| a1 | b1 | c1 |\n| a2 | b2 | c2 |\n| a3 | b3 | c3 |\n";
const SHIFT: egui::Modifiers = egui::Modifiers::SHIFT;
const NONE: egui::Modifiers = egui::Modifiers::NONE;

impl Fx {
    /// A note with a 3 column, 4 row table drawn as a grid.
    fn block(tag: &str) -> Fx {
        let mut fx = Fx::new(tag, BLOCK);
        fx.app.cfg.live_preview = true;
        fx.hl.frames(&mut fx.app, 4);
        fx
    }

    fn press(&mut self, key: egui::Key, modifiers: egui::Modifiers) {
        self.hl.frame(&mut self.app, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.hl.frames(&mut self.app, 3);
    }

    fn type_text(&mut self, s: &str) {
        self.hl.frame(&mut self.app, vec![Event::Text(s.into())]);
        self.hl.frames(&mut self.app, 3);
    }

    fn selected_cells(&self) -> Vec<(usize, usize)> {
        self.editor().selected_cells()
    }

    /// The cells of the first table: header row first.
    fn cells(&self) -> Vec<Vec<String>> {
        let t = &crate::table::find_tables(&self.text())[0].table;
        std::iter::once(t.header.clone()).chain(t.rows.iter().cloned()).collect()
    }

    /// One undo of the note's history.
    fn undo(&mut self) -> String {
        let mut text = self.text();
        assert!(self.app.undo.of(&PathBuf::from(NOTE)).undo(&mut text).is_some());
        text
    }

    /// Select the 2 by 2 block b1 c1 / b2 c2 with Shift+Right and Shift+Down from b1.
    fn block_b1_c2(&mut self) {
        self.edit_cell("b1");
        self.press(egui::Key::ArrowRight, SHIFT);
        self.press(egui::Key::ArrowDown, SHIFT);
        assert_eq!(self.selected_cells(), [(1, 1), (1, 2), (2, 1), (2, 2)]);
    }
}

// spec: EDIT-030
#[test]
fn shift_arrows_extend_a_block_of_cells_across_rows_and_columns_and_shrink_it_again() {
    let mut fx = Fx::block("block-extend");
    fx.edit_cell("b1");
    assert!(fx.selected_cells().is_empty(), "one cell edited: no block");
    fx.press(egui::Key::ArrowRight, SHIFT);
    assert_eq!(fx.selected_cells(), [(1, 1), (1, 2)], "Shift+Right: two cells");
    fx.press(egui::Key::ArrowUp, SHIFT);
    assert_eq!(fx.selected_cells(), [(0, 1), (0, 2), (1, 1), (1, 2)], "then Shift+Up: a 2 by 2 block spanning rows and columns");
    assert_eq!(fx.editor().editing_cell(), Some((1, 1)), "the anchor stays the edited cell");
    // it shrinks back and the cell is edited as before
    fx.press(egui::Key::ArrowLeft, SHIFT);
    assert_eq!(fx.selected_cells(), [(0, 1), (1, 1)], "Shift+Left: a column of two");
    fx.press(egui::Key::ArrowDown, SHIFT);
    assert!(fx.selected_cells().is_empty(), "back to one cell");
    assert_eq!(fx.editor().editing_cell(), Some((1, 1)));
    fx.type_text("z");
    assert_eq!(fx.cells()[1][1], "b1z", "the cell is edited again");
    // the block can grow down over several rows and does not leave the table
    for _ in 0..3 {
        fx.press(egui::Key::ArrowDown, SHIFT);
    }
    assert_eq!(fx.selected_cells(), [(1, 1), (2, 1), (3, 1)], "stops at the last row");
    assert_eq!(fx.cells()[3][1], "b3", "the keys changed nothing");
}

// spec: EDIT-030
#[test]
fn shift_arrows_select_text_in_the_cell_first_and_extend_to_cells_at_its_edge() {
    let mut fx = Fx::block("block-text");
    fx.edit_cell("b1");
    fx.select_in_cell(0, 0);
    fx.press(egui::Key::ArrowRight, SHIFT);
    assert_eq!((fx.selected_in_cell(), fx.selected_cells().len()), ((0, 1), 0), "inside the text Shift+Right selects text");
    fx.press(egui::Key::ArrowRight, SHIFT);
    assert_eq!((fx.selected_in_cell(), fx.selected_cells().len()), ((0, 2), 0), "up to the end of the text");
    fx.press(egui::Key::ArrowRight, SHIFT);
    assert_eq!(fx.selected_cells(), [(1, 1), (1, 2)], "at the end of the text the cells are selected");
    // at the edge of the table nothing more happens
    fx.press(egui::Key::ArrowRight, SHIFT);
    assert_eq!(fx.selected_cells(), [(1, 1), (1, 2)]);
    assert_eq!(fx.text(), BLOCK, "no key changed the note");
}

// spec: EDIT-030, EDIT-027
#[test]
fn a_toolbar_button_formats_every_cell_of_the_block_as_one_undo_step() {
    let mut fx = Fx::block("block-format");
    fx.block_b1_c2();
    fx.click("Bold: **text**");
    assert_eq!(fx.cells()[1], ["a1", "**b1**", "**c1**"]);
    assert_eq!(fx.cells()[2], ["a2", "**b2**", "**c2**"]);
    assert_eq!(fx.cells()[3], ["a3", "b3", "c3"], "cells outside the block are left alone");
    assert_eq!(fx.selected_cells().len(), 4, "the block stays selected");
    // all are bold: the button takes it off again
    fx.click("Bold: **text**");
    assert_eq!(fx.text(), BLOCK);
    fx.click("Italic: *text*");
    assert_eq!(fx.cells()[2], ["a2", "*b2*", "*c2*"]);
    // line buttons stay greyed out
    let before = fx.text();
    for tip in ["Bullet list: - item", "Quote: > text", "Heading level", "Horizontal rule: ---"] {
        fx.click(tip);
        assert_eq!(fx.text(), before, "{tip} does nothing");
        assert_eq!(fx.selected_cells().len(), 4);
    }
    // Esc ends the block, Esc again the editing; one undo takes all of it back
    fx.escape();
    assert!(fx.selected_cells().is_empty());
    fx.escape();
    assert_eq!(fx.editor().editing_cell(), None);
    assert_eq!(fx.undo(), BLOCK, "all the formatting is one undo step");
}

// spec: EDIT-030, EDIT-027
#[test]
fn formatting_a_block_where_some_cells_have_it_adds_it_to_the_others() {
    let mut fx = Fx::new("block-mixed", "| A | B |\n|---|---|\n| **x** | y |\n| z | w |\n");
    fx.app.cfg.live_preview = true;
    fx.hl.frames(&mut fx.app, 4);
    fx.edit_cell("x");
    fx.press(egui::Key::ArrowRight, SHIFT);
    fx.press(egui::Key::ArrowDown, SHIFT);
    fx.click("Bold: **text**");
    assert_eq!(fx.cells()[1], ["**x**", "**y**"], "the bold cell stays bold, the other gets it");
    assert_eq!(fx.cells()[2], ["**z**", "**w**"], "the second row too");
}

// spec: EDIT-030
#[test]
fn delete_and_backspace_clear_every_cell_of_the_block_as_one_undo_step() {
    let mut fx = Fx::block("block-delete");
    fx.block_b1_c2();
    fx.press(egui::Key::Delete, NONE);
    assert_eq!(fx.cells()[1], ["a1", "", ""]);
    assert_eq!(fx.cells()[2], ["a2", "", ""]);
    assert_eq!(fx.cells()[3], ["a3", "b3", "c3"]);
    assert_eq!(fx.selected_cells().len(), 4, "the block stays selected");
    // Backspace on another block, in the same editing: from b1 (edited again after a plain arrow) down to b3
    fx.press(egui::Key::ArrowLeft, NONE);
    assert_eq!(fx.editor().editing_cell(), Some((1, 0)));
    fx.press(egui::Key::ArrowDown, SHIFT);
    fx.press(egui::Key::ArrowDown, SHIFT);
    fx.press(egui::Key::Backspace, NONE);
    assert_eq!([fx.cells()[1][0].as_str(), fx.cells()[2][0].as_str(), fx.cells()[3][0].as_str()], ["", "", ""], "the column a is cleared");
    assert_eq!(fx.cells()[3], ["", "b3", "c3"]);
    fx.escape();
    fx.escape();
    assert_eq!(fx.editor().editing_cell(), None);
    assert_eq!(fx.undo(), BLOCK, "one undo brings back everything");
}

// spec: EDIT-030
#[test]
fn typing_ends_the_block_and_goes_only_into_its_top_left_cell() {
    let mut fx = Fx::block("block-type");
    fx.block_b1_c2();
    fx.type_text("X");
    assert!(fx.selected_cells().is_empty(), "the block is gone");
    assert_eq!(fx.editor().editing_cell(), Some((1, 1)), "the top left cell is edited");
    assert_eq!(fx.cells()[1], ["a1", "b1X", "c1"]);
    assert_eq!(fx.cells()[2], ["a2", "b2", "c2"], "no other cell got the text");
    fx.type_text("Y");
    assert_eq!(fx.cells()[1][1], "b1XY", "and the cell goes on being edited normally");
    // the anchor is not the top left when the block grows up and to the left
    fx.escape();
    fx.select(0, 0);
    fx.edit_cell("b2");
    fx.press(egui::Key::ArrowUp, SHIFT);
    fx.press(egui::Key::ArrowLeft, SHIFT);
    assert_eq!(fx.editor().editing_cell(), Some((2, 1)));
    fx.type_text("Q");
    assert_eq!(fx.editor().editing_cell(), Some((1, 0)), "the top left cell of the block");
    assert_eq!(fx.cells()[1], ["a1Q", "b1XY", "c1"]);
    assert_eq!(fx.cells()[2], ["a2", "b2", "c2"]);
}

// spec: EDIT-030
#[test]
fn a_plain_arrow_key_esc_and_a_click_end_the_block() {
    let mut fx = Fx::block("block-collapse");
    fx.block_b1_c2();
    fx.press(egui::Key::ArrowRight, NONE);
    assert!(fx.selected_cells().is_empty());
    assert_eq!(fx.editor().editing_cell(), Some((1, 2)), "Right: one cell on from the edited cell, as with one cell");
    fx.press(egui::Key::ArrowLeft, NONE);
    fx.press(egui::Key::ArrowDown, SHIFT);
    assert_eq!(fx.selected_cells().len(), 2);
    fx.press(egui::Key::ArrowDown, NONE);
    assert!(fx.selected_cells().is_empty());
    assert_eq!(fx.editor().editing_cell(), Some((2, 1)), "Down: the cell below the edited one");
    // Esc: the block ends, the cell stays edited; Esc again ends the editing
    fx.press(egui::Key::ArrowUp, SHIFT);
    assert_eq!(fx.selected_cells().len(), 2);
    fx.escape();
    assert!(fx.selected_cells().is_empty());
    assert_eq!(fx.editor().editing_cell(), Some((2, 1)), "Esc keeps the anchor cell edited");
    fx.escape();
    assert_eq!(fx.editor().editing_cell(), None);
    // a click on a cell edits that cell
    fx.block_b1_c2();
    let (_, at) = fx.hl.text.iter().find(|(t, _)| t == "a2").cloned().unwrap();
    fx.hl.click(&mut fx.app, at + egui::Vec2::new(4.0, 6.0), PointerButton::Primary);
    assert_eq!((fx.selected_cells().len(), fx.editor().editing_cell()), (0, Some((2, 0))));
    // a click elsewhere ends the editing
    fx.block_b1_c2();
    let (_, before) = fx.hl.text.iter().find(|(t, _)| t.starts_with("before")).cloned().unwrap();
    fx.hl.click(&mut fx.app, before + egui::Vec2::new(4.0, 6.0), PointerButton::Primary);
    fx.hl.frames(&mut fx.app, 3);
    assert_eq!((fx.selected_cells().len(), fx.editor().editing_cell()), (0, None));
    assert_eq!(fx.text(), BLOCK);
}

// The Alt keys of the buttons of a note (KEY-014 … KEY-016, EDIT-042)

use eframe::egui::{Key, Modifiers, Pos2};

impl Fx {
    /// Press Alt and this key (with the character a system types for it) and let the window settle.
    fn alt_key(&mut self, key: Key, typed: &str) {
        let events = vec![Event::ModifiersChanged(Modifiers::ALT), Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::ALT }, Event::Text(typed.to_string())];
        self.hl.frame(&mut self.app, events);
        self.hl.frame(&mut self.app, vec![Event::ModifiersChanged(Modifiers::NONE)]);
        self.hl.frames(&mut self.app, 3);
    }

    /// The badges one frame draws with these modifiers held: the text and the place of each.
    fn badges(&mut self, mods: Modifiers) -> Vec<(String, Pos2)> {
        fn walk(s: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                egui::Shape::Text(t) if t.galley.job.sections.first().is_some_and(|sec| sec.format.font_id.size == crate::key_badges::FONT_SIZE) => out.push((t.galley.text().to_string(), t.pos)),
                _ => {}
            }
        }
        self.hl.frame(&mut self.app, vec![Event::ModifiersChanged(mods)]);
        let out = self.hl.frame(&mut self.app, vec![]);
        let mut got = vec![];
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut got));
        self.hl.frame(&mut self.app, vec![Event::ModifiersChanged(Modifiers::NONE)]);
        got
    }

    fn reading(&self) -> bool {
        self.app.tabbar.tabs[self.app.tabbar.active].reading
    }
}

const CTRL_ALT: Modifiers = Modifiers { alt: true, ctrl: true, shift: false, mac_cmd: false, command: true };

// spec: KEY-015, KEY-014
#[test]
fn alt_b_bolds_the_selection_with_the_keyboard_in_the_text_and_types_nothing() {
    let mut fx = Fx::new("alt-b", "one two three\n");
    fx.select(4, 7);
    fx.alt_key(Key::B, "b");
    assert_eq!(fx.text(), "one **two** three\n", "what a click on Bold does, and no b typed");
    assert_eq!(fx.selected(), "two");
    assert!(fx.has_focus());
    // a line button
    fx.alt_key(Key::U, "u");
    assert_eq!(fx.text(), "- one **two** three\n");
    // a rebound key works, and the old one is free (the character it types goes into the text)
    fx.app.keymap.set(crate::keys::Action::NoteBold, crate::keys::Binding::parse("Alt+Z").unwrap()).unwrap();
    fx.select(8, 11);
    fx.alt_key(Key::Z, "z");
    assert_eq!(fx.text(), "- one **two** three\n".replacen("**two**", "two", 1), "Alt+Z takes the markers off again");
}

// spec: KEY-015, KEY-016
#[test]
fn on_macos_the_alt_keys_do_nothing_and_show_no_badge_until_one_is_bound() {
    use crate::keys::{Action, Binding, Keymap};
    let mut fx = Fx::new("alt-mac", "one two three\n");
    fx.app.keymap = Keymap::from_overrides_for(&fx.app.cfg.keybindings, true);
    fx.select(4, 7);
    assert!(fx.badges(Modifiers::ALT).is_empty(), "no key, no badge");
    fx.alt_key(Key::B, "b");
    assert_eq!(fx.text(), "one b three\n", "Alt+B is no bold key: the b it types replaces the selection");
    fx.alt_key(Key::V, "v");
    assert!(!fx.reading(), "Alt+V does not switch the view");
    // bound by hand, the key works and its badge shows
    fx.app.keymap.set(Action::NoteBold, Binding::parse("Alt+B").unwrap()).unwrap();
    let text = fx.text();
    let start = text.find("one").unwrap_or(0);
    fx.select(start, start + 3);
    fx.alt_key(Key::B, "b");
    assert!(fx.text().contains("**"), "Alt+B bolds now: {:?}", fx.text());
    let badges = fx.badges(Modifiers::ALT);
    assert!(badges.iter().any(|(t, _)| t == "B"), "the badge of Bold shows: {badges:?}");
    assert!(!badges.iter().any(|(t, _)| t == "I"), "no other toolbar letter: {badges:?}");
}

// spec: KEY-015
#[test]
fn alt_v_and_alt_p_change_the_view_and_the_toolbar_keys_do_nothing_in_the_reading_view() {
    let mut fx = Fx::new("alt-v", "one two three\n");
    fx.select(4, 7);
    fx.alt_key(Key::V, "v");
    assert!(fx.reading(), "Alt+V: the reading view");
    assert_eq!(fx.text(), "one two three\n");
    // the toolbar and the preview button are not there: their keys do nothing
    let preview = fx.app.cfg.live_preview;
    for (key, typed) in [(Key::B, "b"), (Key::P, "p"), (Key::E, "e"), (Key::Y, "y")] {
        fx.alt_key(key, typed);
    }
    assert_eq!(fx.text(), "one two three\n");
    assert_eq!(fx.app.cfg.live_preview, preview, "Alt+P does nothing in the reading view");
    assert!(!egui::Popup::is_any_open(fx.hl.ctx()), "no menu opened");
    fx.alt_key(Key::V, "v");
    assert!(!fx.reading(), "Alt+V again: back to editing");
    assert!(fx.has_focus() && fx.selected() == "two", "EDIT-042: the keyboard is back, the selection is where it was");
    // Alt+P switches the preview
    fx.alt_key(Key::P, "p");
    assert_eq!(fx.app.cfg.live_preview, !preview);
    assert_eq!(fx.text(), "one two three\n", "no p typed");
}

// spec: KEY-015
#[test]
fn the_keys_of_the_menu_buttons_open_their_popups_and_new_tab_and_the_menu_key_work() {
    let mut fx = Fx::new("alt-menus", "one two three\n");
    fx.select(4, 7);
    fx.alt_key(Key::E, "e");
    assert!(fx.has("Heading 2"), "Alt+E opens the heading menu");
    fx.click("Heading 2");
    assert_eq!(fx.text(), "## one two three\n");
    fx.alt_key(Key::Y, "y");
    assert!(fx.has("No highlight"), "Alt+Y opens the highlight palette");
    fx.alt_key(Key::F, "f");
    assert!(fx.has("Default colour") && !fx.has("No highlight"), "Alt+F opens the font palette instead");
    egui::Popup::close_all(fx.hl.ctx());
    fx.hl.frames(&mut fx.app, 2);
    fx.alt_key(Key::D, "d");
    assert!(fx.has("Export to PDF…"), "Alt+D opens the ⋯ menu");
    assert_eq!(fx.text(), "## one two three\n", "nothing was typed");
    egui::Popup::close_all(fx.hl.ctx());
    fx.hl.frames(&mut fx.app, 2);
    let tabs = fx.app.tabbar.tabs.len();
    fx.alt_key(Key::N, "n");
    assert_eq!(fx.app.tabbar.tabs.len(), tabs + 1, "Alt+N: a new tab");
}

// spec: KEY-015
#[test]
fn alt_1_does_what_a_click_on_the_first_ribbon_item_does_and_alt_comma_toggles_the_sidebar() {
    let mut fx = Fx::new("alt-ribbon", "one two three\n");
    assert_eq!(fx.app.side.layout.ribbon[0], crate::sidebar::Item::Files);
    fx.app.side.left = crate::app::LeftPanel::Tags;
    fx.app.cfg.show_left = false;
    fx.select(4, 7);
    fx.alt_key(Key::Num1, "1");
    assert!(fx.app.side.left == crate::app::LeftPanel::Files && fx.app.cfg.show_left, "the first item: Files");
    assert_eq!(fx.text(), "one two three\n", "no 1 typed");
    assert!(fx.has_focus() && fx.selected() == "two", "EDIT-042");
    fx.alt_key(Key::Comma, ",");
    assert!(!fx.app.cfg.show_left, "Alt+, hides the left sidebar");
    fx.alt_key(Key::Comma, ",");
    assert!(fx.app.cfg.show_left);
    let right = fx.app.cfg.show_right;
    fx.alt_key(Key::Period, ".");
    assert_eq!(fx.app.cfg.show_right, !right, "Alt+. toggles the right sidebar");
    assert!(fx.has_focus() && fx.selected() == "two");
    assert_eq!(fx.text(), "one two three\n");
    // the second item is Search, which takes the keyboard (it is not a change of the view only)
    fx.alt_key(Key::Num2, "2");
    assert!(fx.app.side.left == crate::app::LeftPanel::Search);
}

// spec: KEY-015, EDIT-030
#[test]
fn in_a_cell_the_keys_of_the_greyed_out_buttons_do_nothing_and_the_inline_ones_work() {
    let mut fx = Fx::table("alt-cell");
    fx.edit_cell("one two");
    fx.select_in_cell(0, 3);
    for (key, typed) in [(Key::K, "k"), (Key::O, "o"), (Key::T, "t"), (Key::Q, "q"), (Key::R, "r"), (Key::A, "a"), (Key::M, "m"), (Key::E, "e"), (Key::U, "u")] {
        fx.alt_key(key, typed);
        assert_eq!(fx.text(), CELLS, "Alt+{key:?} does nothing in a cell");
        assert_eq!(fx.editor().editing_cell(), Some((1, 0)));
    }
    assert!(!egui::Popup::is_any_open(fx.hl.ctx()), "the heading menu is greyed out too");
    assert!(!fx.editor().picker_visible());
    fx.alt_key(Key::I, "i");
    assert!(fx.text().contains("| *one* two |"), "{}", fx.text());
}

// spec: KEY-016
#[test]
fn holding_alt_shows_the_key_of_each_button_at_its_corner_and_a_rebound_key_moves_the_badge() {
    let mut fx = Fx::new("badges", "one two three\n");
    assert!(fx.badges(Modifiers::NONE).is_empty(), "no badge without Alt");
    assert!(fx.badges(CTRL_ALT).is_empty(), "none with Ctrl+Alt (AltGr)");
    let shown = fx.badges(Modifiers::ALT);
    let texts: Vec<&str> = shown.iter().map(|(t, _)| t.as_str()).collect();
    for want in ["B", "I", "S", "H", "C", "Y", "F", "E", "U", "O", "T", "Q", "K", "R", "A", "L", "W", "M", "X", "V", "P", "D", "N", "1", "2", ",", "."] {
        assert!(texts.contains(&want), "no badge {want}: {texts:?}");
    }
    // the badge of Bold sits at the corner of its button
    let bold = fx.hl.find_label(&mut fx.app, "Bold: **text**");
    let at = shown.iter().find(|(t, _)| t == "B").unwrap().1;
    assert!(bold.expand(8.0).contains(at), "{at:?} is not at the Bold button {bold:?}");
    // the ribbon: one per item, the tenth shows 0
    let items = fx.app.side.layout.ribbon.len();
    assert!(items >= 5);
    assert_eq!(texts.iter().filter(|t| t.len() == 1 && t.chars().all(|c| c.is_ascii_digit())).count(), items.min(10));
    // a rebound key: the new text, at the same button
    fx.app.keymap.set(crate::keys::Action::NoteBold, crate::keys::Binding::parse("Alt+Z").unwrap()).unwrap();
    let shown = fx.badges(Modifiers::ALT);
    assert!(!shown.iter().any(|(t, _)| t == "B"));
    let moved = shown.iter().find(|(t, _)| t == "Z").unwrap().1;
    assert!(bold.expand(8.0).contains(moved), "the badge Z is at the Bold button too");
    // an action without an Alt key has no badge
    fx.app.keymap.clear(crate::keys::Action::NoteItalic);
    assert!(!fx.badges(Modifiers::ALT).iter().any(|(t, _)| t == "I"));
    // the reading view has no toolbar and no preview button; the view button is still there
    fx.alt_key(Key::V, "v");
    let texts: Vec<String> = fx.badges(Modifiers::ALT).into_iter().map(|(t, _)| t).collect();
    assert!(!texts.contains(&"Z".to_string()) && !texts.contains(&"P".to_string()), "{texts:?}");
    assert!(texts.contains(&"V".to_string()) && texts.contains(&"N".to_string()) && texts.contains(&"1".to_string()));
}

// spec: KEY-016, EDIT-030
#[test]
fn a_greyed_out_button_of_a_cell_has_no_badge() {
    let mut fx = Fx::table("badges-cell");
    fx.edit_cell("one two");
    let texts: Vec<String> = fx.badges(Modifiers::ALT).into_iter().map(|(t, _)| t).collect();
    assert!(texts.contains(&"B".to_string()) && texts.contains(&"Y".to_string()), "{texts:?}");
    for greyed in ["K", "O", "E", "T", "Q", "R", "A", "M", "U"] {
        assert!(!texts.contains(&greyed.to_string()), "{greyed} is greyed out: {texts:?}");
    }
}

// spec: EDIT-042
#[test]
fn after_a_view_button_or_a_toolbar_button_the_editor_has_the_keyboard_and_the_selection_is_unchanged() {
    let mut fx = Fx::new("focus-back", "one two three\n");
    // a click on the preview button, the edit / reading button, the sidebar toggle, a ribbon item
    for tip in ["Markdown preview", "Raw markdown", "Toggle left sidebar", "Toggle left sidebar", "Files"] {
        fx.select(4, 7);
        fx.click(tip);
        assert!(fx.has_focus(), "after a click on {tip} the editor has the keyboard");
        assert_eq!(fx.selected(), "two", "…and the selection is unchanged after {tip}");
    }
    // the reading view has no editor; the button back to editing gives it the keyboard
    fx.select(4, 7);
    fx.click("Reading view (Ctrl+E)");
    assert!(fx.reading());
    fx.click("Edit (Ctrl+E)");
    assert!(!fx.reading(), "editing again");
    assert!(fx.has_focus(), "the editor has the keyboard");
    assert_eq!(fx.selected(), "two");
    // the same through the keys
    fx.select(4, 7);
    for (key, typed) in [(Key::P, "p"), (Key::Comma, ","), (Key::Comma, ","), (Key::Num1, "1")] {
        fx.alt_key(key, typed);
        assert!(fx.has_focus(), "after Alt+{key:?} the editor has the keyboard");
        assert_eq!(fx.selected(), "two", "after Alt+{key:?}");
    }
    // the reading view and back
    fx.alt_key(Key::V, "v");
    fx.alt_key(Key::V, "v");
    assert!(fx.has_focus() && fx.selected() == "two", "after Alt+V twice");
    assert_eq!(fx.text(), "one two three\n");
    // a toolbar button, by click and by key
    fx.click("Bold: **text**");
    assert!(fx.has_focus() && fx.selected() == "two");
    fx.alt_key(Key::I, "i");
    assert!(fx.has_focus() && fx.selected() == "two");
}
