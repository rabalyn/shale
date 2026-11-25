//! A theme switch in the whole window (CFG-062, #689): every view follows it in the next frame,
//! also the editor, whose laid-out galley and formatted blocks are kept between frames and bake
//! the colours of the palette in.

use crate::app::{Options, ShaleApp};
use crate::theme::{self, Palette};
use crate::ui_test_support::Headless;
use eframe::egui::{self, epaint::Shape, Color32};
use std::path::{Path, PathBuf};

const NOTE: &str = "plainwords here and a [[A]] link\n\n```rust\nfn main() { let x = 1; }\n```\n";

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-theme-switch-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), NOTE).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.date_prefix_new_notes = false;
        app.cfg.cursor_blink = false;
        app.open(Path::new("A.md"), false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.settle(&mut w.app);
        w
    }

    /// The colour the editor's layout of the last frame gives the first `word`.
    fn editor_colour(&self, word: &str) -> Color32 {
        let ed = &self.app.tabbar.tabs[self.app.tabbar.active].editor;
        let (galley, _) = ed.last_layout.clone().expect("the editor was laid out");
        colour_in(&galley, word)
    }

    /// The colour the reading view gives the first `word`, from the shapes of one frame.
    fn reader_colour(&mut self, word: &str) -> Color32 {
        let out = self.hl.frame(&mut self.app, vec![]);
        let mut found = None;
        out.shapes.iter().for_each(|c| find_text(&c.shape, word, &mut found));
        found.unwrap_or_else(|| panic!("{word:?} was not painted in the reading view"))
    }

    /// Switch the theme the way the settings do, and run the next frame.
    fn switch(&mut self, palette: Palette) {
        theme::switch_palette(self.hl.ctx(), palette);
        self.hl.frame(&mut self.app, vec![]);
    }
}

impl Drop for W {
    fn drop(&mut self) {
        theme::set_palette(Palette::shale());
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn colour_in(galley: &egui::Galley, word: &str) -> Color32 {
    let at = galley.job.text.find(word).unwrap_or_else(|| panic!("{word:?} is not in the text"));
    galley.job.sections.iter().find(|s| s.byte_range.contains(&egui::text::ByteIndex(at))).expect("a section").format.color
}

fn find_text(shape: &Shape, word: &str, found: &mut Option<Color32>) {
    match shape {
        Shape::Text(t) if found.is_none() && t.galley.job.text.contains(word) => *found = Some(colour_in(&t.galley, word)),
        Shape::Vec(v) => v.iter().for_each(|s| find_text(s, word, found)),
        _ => {}
    }
}

/// The dark palette the window starts with, and a light one.
fn palettes() -> (Palette, Palette) {
    let themes = crate::themes::Themes::load(None);
    (Palette::shale(), themes.get("Catppuccin Latte").expect("a built-in light theme").palette)
}

// spec: CFG-062
#[test]
fn cfg_062_the_editor_follows_a_theme_switch_in_the_next_frame_text_and_code_block() {
    let (dark, light) = palettes();
    assert_ne!(dark.text, light.text);
    assert_ne!(dark.syntax_keyword, light.syntax_keyword);
    let mut w = W::new("editor");
    assert_eq!(w.editor_colour("plainwords"), dark.text, "laid out with the dark palette");
    assert_eq!(w.editor_colour("fn main"), dark.syntax_keyword, "the code block is coloured by the dark palette");
    w.switch(light);
    assert_eq!(w.editor_colour("plainwords"), light.text, "the next frame has the text colour of the light palette");
    assert_eq!(w.editor_colour("fn main"), light.syntax_keyword, "and its code block the light palette's colours");
    w.switch(dark);
    assert_eq!(w.editor_colour("plainwords"), dark.text, "and back");
    assert_eq!(w.editor_colour("fn main"), dark.syntax_keyword);
}

// spec: CFG-062
#[test]
fn cfg_062_the_reading_view_follows_a_theme_switch_in_the_next_frame_text_and_code_block() {
    let (dark, light) = palettes();
    let mut w = W::new("reader");
    let tab = w.app.tabbar.active;
    w.app.tabbar.tabs[tab].reading = true;
    w.hl.settle(&mut w.app);
    assert_eq!(w.reader_colour("plainwords"), dark.text);
    assert_eq!(w.reader_colour("fn main"), dark.syntax_keyword);
    w.switch(light);
    assert_eq!(w.reader_colour("plainwords"), light.text);
    assert_eq!(w.reader_colour("fn main"), light.syntax_keyword);
}
