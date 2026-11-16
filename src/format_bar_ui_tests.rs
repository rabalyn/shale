//! The formatting toolbar (EDIT-025, EDIT-026, EDIT-027) driven in the whole window with real clicks.

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

// spec: EDIT-025
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

// spec: EDIT-026
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

// spec: EDIT-026
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

// spec: EDIT-026, EDIT-016
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

// spec: EDIT-026, EDIT-010
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

// spec: EDIT-026
#[test]
fn the_embed_button_opens_the_file_picker() {
    let mut fx = Fx::new("embed", "see \n");
    fx.select(4, 4);
    fx.click("Embed a file or picture: opens the file picker (![[file]])");
    assert_eq!(fx.text(), "see [!\n");
    assert!(fx.app.tabbar.tabs[fx.app.tabbar.active].editor.picker_visible(), "the `[!` file picker is open");
}

// spec: EDIT-027
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

// spec: EDIT-027
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
