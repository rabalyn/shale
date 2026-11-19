//! The page-break marker on the screen (EDIT-065): the reading view and the editor's Markdown
//! preview draw a dashed rule with the label "Page break" where the HTML line is; the line with the
//! cursor and the editor without the preview show the HTML as written, and the text never changes.

use crate::app::{Options, ShaleApp};
use crate::reader::{show, Caches, Extras, SceneCache};
use crate::ui_test_support::Headless;
use crate::vault::Vault;
use eframe::egui::{self, epaint::Shape, Pos2, RawInput, Rect};
use std::path::{Path, PathBuf};

const MARK: &str = "<div style=\"page-break-after: always;\"></div>";

/// The texts that were painted and where.
pub(crate) fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Rect)> {
    fn walk(s: &Shape, out: &mut Vec<(String, Rect)>) {
        match s {
            Shape::Text(t) => out.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out
}

/// The line segments painted (the dashes of the rule).
fn dashes(shapes: &[egui::epaint::ClippedShape]) -> usize {
    fn walk(s: &Shape) -> usize {
        match s {
            Shape::LineSegment { .. } => 1,
            Shape::Vec(v) => v.iter().map(walk).sum(),
            _ => 0,
        }
    }
    shapes.iter().map(|c| walk(&c.shape)).sum()
}

fn label_of(painted: &[(String, Rect)]) -> Option<Rect> {
    painted.iter().find(|(t, _)| t == "Page break").map(|(_, r)| *r)
}

/// The reading view of `text` in a 700 x 500 window: what the last frame painted.
pub(crate) fn read(text: &str) -> (Vec<(String, Rect)>, usize) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("shale-pagebreak-reader-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    crate::vault::write_sample_vault(&d).unwrap();
    let v = Vault::open(&d).unwrap();
    let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut time = 0.0;
    let mut last = (vec![], 0);
    for _ in 0..6 {
        time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(time), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                show(ui, text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras::default());
            });
        });
        out.textures_delta.clear();
        last = (texts(&out.shapes), dashes(&out.shapes));
    }
    let _ = std::fs::remove_dir_all(&d);
    last
}

// spec: EDIT-065
#[test]
fn the_reading_view_draws_a_dashed_rule_with_its_label_instead_of_the_html() {
    let (painted, dashes) = read(&format!("before\n\n{MARK}\n\nafter\n\n```\n{MARK}\n```\n"));
    let before = painted.iter().find(|(t, _)| t == "before").unwrap().1;
    let after = painted.iter().find(|(t, _)| t == "after").unwrap().1;
    let label = label_of(&painted).expect("the label is drawn");
    assert!(before.max.y < label.min.y && label.max.y < after.min.y, "between the two texts: {before:?} {label:?} {after:?}");
    assert!(painted.iter().filter(|(t, _)| t == "Page break").count() == 1, "one marker; the one in the code is code");
    assert!(painted.iter().filter(|(t, _)| t.contains("<div")).count() == 1, "the raw HTML is the code block's only: {painted:?}");
    assert!(dashes >= 2, "a dashed rule on both sides of the label: {dashes}");
    // the rule runs over the width of the column, the label in the middle
    assert!((label.center().x - 350.0).abs() < 20.0, "{label:?}");
}

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    fn new(tag: &str, note: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-pagebreak-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("A.md"), note).unwrap();
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.date_prefix_new_notes = false;
        app.cfg.cursor_blink = false;
        app.open(Path::new("A.md"), false);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.frames(&mut w.app, 4);
        w
    }

    /// One frame: the texts and dashes it painted.
    fn look(&mut self) -> (Vec<(String, Rect)>, usize) {
        let out = self.hl.frame(&mut self.app, vec![]);
        (texts(&out.shapes), dashes(&out.shapes))
    }

    /// Whether the editor's layout holds the HTML as visible glyphs.
    fn shows_html(&self) -> bool {
        let (galley, _) = self.app.tabbar.tabs[self.app.tabbar.active].editor.last_layout.clone().expect("laid out");
        galley.rows.iter().any(|r| r.row.glyphs.iter().any(|g| g.chr == '<' && g.font_height > 2.0))
    }

    fn cursor_in(&mut self, needle: &str) {
        let text = self.app.notes.get(Path::new("A.md")).unwrap().text.clone();
        let at = text.find(needle).unwrap();
        self.app.focus_editor();
        self.app.tabbar.tabs[self.app.tabbar.active].editor.pending_cursor = Some(crate::markdown::byte_to_char(&text, at));
        self.hl.frames(&mut self.app, 4);
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// spec: EDIT-065
#[test]
fn the_preview_draws_the_rule_away_from_the_cursor_and_shows_the_html_on_the_cursor_line() {
    let note = format!("intro\n\n{MARK}\n\nlast line\n");
    let mut w = W::new("preview", &note);
    // without the preview the HTML is shown as written, like `---`
    let (painted, _) = w.look();
    assert!(label_of(&painted).is_none() && w.shows_html(), "raw editor");
    w.app.cfg.live_preview = true;
    w.hl.frames(&mut w.app, 3);
    let (painted, dashes) = w.look();
    assert!(!w.shows_html(), "the HTML is hidden");
    let label = label_of(&painted).expect("the label is drawn");
    assert!(dashes >= 2);
    // the cursor in the line shows the HTML again
    w.cursor_in("page-break");
    let (painted, _) = w.look();
    assert!(w.shows_html() && label_of(&painted).is_none(), "the cursor line is raw");
    // and the cursor away from it draws the rule again, in the same place
    w.cursor_in("intro");
    let (painted, _) = w.look();
    let again = label_of(&painted).expect("drawn again");
    assert!((again.center().y - label.center().y).abs() < 1.0 && !w.shows_html());
    // the text of the note is what it was
    assert_eq!(w.app.notes.get(Path::new("A.md")).unwrap().text, note);
}
