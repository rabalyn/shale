//! The text cursor of the note editor (EDIT-039…EDIT-041), drawn in the whole window: exactly one
//! cursor in every mode, in the shape of the settings.

use crate::app::{Options, ShaleApp};
use crate::cursor_style::CursorShape;
use crate::theme;
use crate::ui_test_support::Headless;
use eframe::egui::{epaint::Shape, Event, Key, Modifiers, Rect};
use std::path::{Path, PathBuf};

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    fn new(tag: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-cursor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), "plain words here\n").unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.vim_mode = true;
        app.cfg.cursor_blink = false;
        app.cfg.date_prefix_new_notes = false;
        app.open(Path::new("A.md"), false);
        app.focus_editor();
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 4);
        w
    }

    fn vim_cursor_rect(&self) -> Option<Rect> {
        self.app.tabbar.tabs[self.app.tabbar.active].editor.vim_cursor_rect
    }

    /// Run a frame and say what it painted of the cursor: the accent-coloured filled rectangles
    /// (the shapes) and the vertical accent-coloured lines of 2 points (egui's pipe).
    fn cursor_paint(&mut self) -> (Vec<Rect>, usize) {
        fn walk(s: &Shape, rects: &mut Vec<Rect>, pipes: &mut usize) {
            match s {
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, rects, pipes)),
                Shape::Rect(r) if r.fill == theme::palette().accent || r.fill == theme::palette().accent.gamma_multiply(0.45) => rects.push(r.rect),
                Shape::LineSegment { points, stroke } if stroke.color == theme::palette().accent && stroke.width == 2.0 && points[0].x == points[1].x => *pipes += 1,
                _ => {}
            }
        }
        let out = self.hl.frame(&mut self.app, vec![]);
        let (mut rects, mut pipes) = (vec![], 0);
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut rects, &mut pipes));
        (rects, pipes)
    }

    fn type_key(&mut self, key: Key, text: &str) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        self.hl.frame(&mut self.app, vec![ev(true), Event::Text(text.to_string())]);
        self.hl.frame(&mut self.app, vec![ev(false)]);
        self.hl.frames(&mut self.app, 3);
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: EDIT-039, EDIT-040
#[test]
fn edit_039_one_cursor_is_painted_in_normal_and_insert_mode() {
    let mut w = W::new("one");
    // normal mode: the block, and not egui's pipe
    let (rects, pipes) = w.cursor_paint();
    let block = w.vim_cursor_rect().expect("normal mode has a character rect");
    assert_eq!((rects.as_slice(), pipes), ([block].as_slice(), 0), "one block, no pipe");
    assert!(block.width() > 4.0);
    // another shape for normal mode
    w.app.cfg.normal_cursor_shape = CursorShape::Underline;
    let (rects, pipes) = w.cursor_paint();
    let ch = w.vim_cursor_rect().unwrap();
    assert_eq!((rects.as_slice(), pipes), ([CursorShape::Underline.rect(ch)].as_slice(), 0));
    // insert mode: one bar at the left edge of the character at the cursor, no pipe
    w.type_key(Key::I, "i");
    assert!(w.vim_cursor_rect().is_none(), "the character rect is only kept in normal mode");
    let (rects, pipes) = w.cursor_paint();
    assert_eq!((rects.len(), pipes), (1, 0), "{rects:?}");
    assert_eq!(rects[0].width(), 2.0);
    // the bar after the character is at its right edge
    let left = rects[0];
    w.app.cfg.cursor_shape = CursorShape::BarAfter;
    let (rects, _) = w.cursor_paint();
    assert_eq!(rects.len(), 1);
    assert!(rects[0].min.x > left.min.x && rects[0].height() == left.height(), "{left:?} {rects:?}");
}

// spec: EDIT-041
#[test]
fn edit_041_a_steady_cursor_is_painted_in_every_frame_and_a_blinking_one_goes_out() {
    let mut w = W::new("blink");
    for _ in 0..40 {
        assert_eq!(w.cursor_paint().0.len(), 1);
    }
    // blinking: within a few seconds of frames the cursor is off in some frames and on in others
    w.app.cfg.cursor_blink = true;
    let shown: Vec<bool> = (0..60).map(|_| !w.cursor_paint().0.is_empty()).collect();
    assert!(shown.contains(&true) && shown.contains(&false), "{shown:?}");
}

// spec: EDIT-043
#[test]
fn edit_043_esc_leaves_the_cursor_where_the_typing_stopped_unless_step_back_is_chosen() {
    use crate::vim::EscCursor;
    // the default: i, type, Esc: the block is over the character after the typing
    let mut w = W::new("esc-stay");
    assert_eq!(w.app.cfg.esc_cursor, EscCursor::Stay);
    w.type_key(Key::G, "g");
    w.type_key(Key::G, "g");
    w.type_key(Key::I, "i");
    let (bar, _) = (w.cursor_paint().0[0], ());
    w.type_key(Key::X, "x");
    let (bar_after_typing, _) = (w.cursor_paint().0[0], ());
    assert!(bar_after_typing.min.x > bar.min.x, "the bar moved on with the typing");
    w.type_key(Key::Escape, "");
    let block = w.vim_cursor_rect().expect("normal mode");
    assert_eq!(block.min.x, bar_after_typing.min.x, "the block is where the bar was: {block:?} {bar_after_typing:?}");
    // step back: the block is over the typed character
    let mut w = W::new("esc-back");
    w.app.cfg.esc_cursor = EscCursor::StepBack;
    w.type_key(Key::G, "g");
    w.type_key(Key::G, "g");
    w.type_key(Key::I, "i");
    w.type_key(Key::X, "x");
    let bar = w.cursor_paint().0[0];
    w.type_key(Key::Escape, "");
    let block = w.vim_cursor_rect().expect("normal mode");
    assert!(block.min.x < bar.min.x, "one character back: {block:?} {bar:?}");
}
