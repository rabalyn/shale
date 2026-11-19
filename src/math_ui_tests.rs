//! Math on the screen (EDIT-047, EDIT-048, EDIT-049): in the editor's preview, the reading view,
//! table cells and the outline, driven in the whole window or in a view of its own.

use crate::app::{Options, ShaleApp};
use crate::reader::{show, Caches, Extras, SceneCache};
use crate::ui_test_support::Headless;
use crate::vault::Vault;
use eframe::egui::{self, epaint::Shape, Event, Pos2, RawInput, Rect};
use std::path::{Path, PathBuf};

/// The pictures that were painted (formulas): their rectangles. The font atlas is texture 0.
fn pictures(shapes: &[egui::epaint::ClippedShape]) -> Vec<Rect> {
    fn walk(s: &Shape, out: &mut Vec<Rect>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => out.push(m.calc_bounds()),
            _ => {}
        }
    }
    let mut out = Vec::new();
    shapes.iter().for_each(|c| walk(&c.shape, &mut out));
    out.sort_by(|a, b| (a.min.y, a.min.x).partial_cmp(&(b.min.y, b.min.x)).unwrap());
    out
}

/// The texts that were painted and where.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, Rect)> {
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

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    fn new(tag: &str, note: &str) -> W {
        let dir = std::env::temp_dir().join(format!("shale-math-{tag}-{}", std::process::id()));
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

    /// One frame: the pictures it painted.
    fn look(&mut self) -> Vec<Rect> {
        let out = self.hl.frame(&mut self.app, vec![]);
        pictures(&out.shapes)
    }

    fn editor(&self) -> &crate::editor::NoteEditor {
        &self.app.tabbar.tabs[self.app.tabbar.active].editor
    }

    /// The laid-out text of the editor and its place on the screen.
    fn galley(&self) -> (std::sync::Arc<egui::Galley>, Pos2) {
        self.editor().last_layout.clone().expect("the editor was laid out")
    }

    /// The screen rectangle of the row that holds the text `needle`.
    fn row_of(&self, needle: &str) -> Rect {
        let (galley, pos) = self.galley();
        let row = galley.rows.iter().find(|r| r.row.glyphs.iter().map(|g| g.chr).collect::<String>().contains(needle)).unwrap_or_else(|| panic!("no row with {needle:?}"));
        Rect::from_min_size(pos + row.pos.to_vec2(), row.row.size)
    }

    /// Put the cursor in the line with `needle`, as a click would, and let the editor settle.
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

/// The glyphs of the row that holds `needle`, with their screen x: (char, x, font height).
fn glyph_xs(w: &W, needle: &str) -> Vec<(char, f32, f32)> {
    let (galley, pos) = w.galley();
    let row = galley.rows.iter().find(|r| r.row.glyphs.iter().map(|g| g.chr).collect::<String>().contains(needle)).unwrap();
    row.row.glyphs.iter().map(|g| (g.chr, pos.x + row.pos.x + g.pos.x, g.font_height)).collect()
}

// spec: EDIT-048
#[test]
fn edit_048_the_preview_draws_formulas_in_the_room_of_their_hidden_tex() {
    let note = "intro\n\nLet $x^2+1$ be real and more text.\n\n$$\\frac{a}{b}$$\n\nTall $\\frac{\\frac{a}{b}}{\\frac{c}{d}}$ inline\n\nlast line\n";
    let mut w = W::new("preview", note);
    w.app.cfg.live_preview = true;
    w.hl.frames(&mut w.app, 3);
    let pics = w.look();
    assert_eq!(pics.len(), 3, "an inline formula, a display one and a tall inline one: {pics:?}");
    // the TeX is not painted: its glyphs are hidden
    let (galley, _) = w.galley();
    assert!(galley.rows.iter().all(|r| r.row.glyphs.iter().filter(|g| g.font_height > 2.0).all(|g| g.chr != '^' && g.chr != '$')), "no visible TeX");

    // the inline formula is between "Let " and " be", on the row of the text
    let row = w.row_of("Let ");
    let xs = glyph_xs(&w, "Let ");
    let tex_start = xs.iter().find(|(c, _, h)| *c == '$' && *h < 2.0).unwrap().1;
    let be = xs.iter().find(|(c, _, h)| *c == 'b' && *h > 2.0).unwrap().1;
    let inline = pics[0];
    assert!(inline.min.x >= tex_start - 3.0 && inline.max.x <= be, "{inline:?} between {tex_start} and {be}");
    assert!(inline.min.y >= row.min.y - 1.0 && inline.max.y <= row.max.y + 1.0, "inside its row: {inline:?} {row:?}");
    // a tall formula makes its row taller and still fits it
    let tall_row = w.row_of("Tall ");
    assert!(tall_row.height() > row.height() + 8.0, "the row of a tall formula grows: {} {}", tall_row.height(), row.height());
    let tall = pics[2];
    assert!(tall.min.y >= tall_row.min.y - 1.0 && tall.max.y <= tall_row.max.y + 1.0, "the tall formula fits its row: {tall:?} {tall_row:?}");

    // the display formula is centred in a row of its own
    let block = pics[1];
    let (galley, pos) = w.galley();
    let mid = pos.x + galley.job.wrap.max_width / 2.0;
    assert!((block.center().x - mid).abs() < 14.0, "centred: {} {mid}", block.center().x);
    let rows: Vec<Rect> = galley.rows.iter().map(|r| Rect::from_min_size(pos + r.pos.to_vec2(), r.row.size)).collect();
    assert!(rows.iter().any(|r| r.min.y <= block.min.y && block.max.y <= r.max.y), "inside a row of its own height");

    // the line with the cursor shows its TeX instead (EDIT-016: the markup of the cursor line)
    w.cursor_in("x^2+1");
    let pics = w.look();
    assert_eq!(pics.len(), 2, "the formula of the cursor line is not drawn: {pics:?}");
    let xs = glyph_xs(&w, "Let ");
    assert!(xs.iter().any(|(c, _, h)| *c == '^' && *h > 2.0), "the TeX is shown as text");
    // a cursor in a display formula shows its TeX
    w.cursor_in("frac{a}{b}");
    assert_eq!(w.look().len(), 2, "the display formula is TeX now, the others are drawn");
    // and back
    w.cursor_in("intro");
    assert_eq!(w.look().len(), 3);
}

// spec: EDIT-048, EDIT-046
#[test]
fn edit_048_a_display_formula_over_several_lines_is_one_picture_and_its_lines_are_hidden() {
    let note = "before\n$$\na = b\nc = d\n$$\nafter\n";
    let mut w = W::new("multi", note);
    w.app.cfg.live_preview = true;
    w.hl.frames(&mut w.app, 3);
    let pics = w.look();
    assert_eq!(pics.len(), 1);
    let before = w.row_of("before");
    let after = w.row_of("after");
    // the rows between the two texts take the formula's height, not that of three lines of text
    let gap = after.min.y - before.max.y;
    assert!(gap < 80.0 && gap > 25.0, "{gap}");
    assert!(pics[0].min.y > before.max.y - 2.0 && pics[0].max.y < after.min.y + 2.0, "between the texts: {:?}", pics[0]);
    // a ```math fence is the same
    let mut w = W::new("fence", "before\n```math\n\\frac{a}{b}\n```\nafter\n");
    w.app.cfg.live_preview = true;
    w.hl.frames(&mut w.app, 3);
    assert_eq!(w.look().len(), 1, "the fence is a display formula");
}

/// Draw `text` in the reading view of a 700 x 500 window for some frames (the events are for the
/// frames after the first three); returns the texts and pictures of the last frame and the context.
fn read(text: &str, frames: Vec<Vec<Event>>) -> (Vec<(String, Rect)>, Vec<Rect>, egui::Context) {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("shale-math-reader-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    crate::vault::write_sample_vault(&d).unwrap();
    let v = Vault::open(&d).unwrap();
    let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut time = 0.0;
    let mut last = (vec![], vec![]);
    for events in std::iter::repeat_with(Vec::new).take(3).chain(frames).chain(std::iter::repeat_with(Vec::new).take(15)) {
        time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(time), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                show(ui, text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras::default());
            });
        });
        out.textures_delta.clear();
        last = (texts(&out.shapes), pictures(&out.shapes));
    }
    let _ = std::fs::remove_dir_all(&d);
    (last.0, last.1, ctx)
}

// spec: EDIT-048
#[test]
fn edit_048_the_reading_view_draws_inline_formulas_in_the_text_and_display_ones_centred() {
    let text = "Before $x^2 + 1$ after.\n\n$$\\sum_{k=1}^n k$$\n\n$$\na = b\n$$\n\n```math\n\\int_0^1 x\\,dx\n```\n\nEnd.\n";
    let (painted, pics, _) = read(text, vec![]);
    assert_eq!(pics.len(), 4, "one inline and three display formulas: {pics:?}");
    let line = painted.iter().find(|(t, _)| t.starts_with("Before")).unwrap().1;
    let inline = pics[0];
    assert!(inline.min.y >= line.min.y - 3.0 && inline.max.y <= line.max.y + 3.0, "on the line: {inline:?} {line:?}");
    assert!(inline.min.x > line.min.x + 30.0 && inline.max.x < line.max.x, "in the middle of the line: {inline:?} {line:?}");
    // the display formulas are centred in the column and one under the other
    let centre = pics[1].center().x;
    assert!(pics[1..].iter().all(|p| (p.center().x - centre).abs() < 6.0), "{pics:?}");
    assert!(pics[1].max.y < pics[2].min.y && pics[2].max.y < pics[3].min.y, "in the order of the note");
    let end = painted.iter().find(|(t, _)| t == "End.").unwrap().1;
    assert!(pics[3].max.y < end.min.y, "before the text that follows");
    // the column is the width of the window less the scroll bar: the centre is near its middle
    assert!((centre - 350.0).abs() < 20.0, "{centre}");
}

// spec: EDIT-049
#[test]
fn edit_049_a_formula_that_cannot_be_typeset_is_shown_as_code_with_the_reason_on_hover() {
    let text = "Try $\\unknown{x}$ here.\n\n$$\n\\frac{a}\n$$\n";
    let (painted, pics, _) = read(text, vec![]);
    assert!(pics.is_empty(), "nothing is drawn typeset: {pics:?}");
    let line = painted.iter().find(|(t, _)| t.starts_with("Try")).unwrap().1;
    assert!(painted.iter().any(|(t, _)| t.contains("$\\unknown{x}$")), "the TeX is shown: {painted:?}");
    assert!(painted.iter().any(|(t, _)| t == "\\frac{a}"), "a display formula as a code block: {painted:?}");
    // hovering over it says why
    let over = Pos2::new(line.min.x + 60.0, line.center().y);
    let still = std::iter::repeat_with(Vec::new).take(11);
    let (painted, _, _) = read(text, std::iter::once(vec![Event::PointerMoved(over)]).chain(still).collect());
    assert!(painted.iter().any(|(t, _)| t.contains("cannot be typeset") && t.contains("\\unknown")), "the reason: {:?}", painted.iter().map(|p| &p.0).collect::<Vec<_>>());
    // in a table cell, too
    let table = "| A | B |\n|---|---|\n| one | $\\unknown$ |\n";
    let (cells, pics, _) = read(table, vec![]);
    assert!(pics.is_empty() && cells.iter().any(|(t, _)| t.contains("$\\unknown$")));
    let cell = cells.iter().find(|(t, _)| t.contains("$\\unknown$")).unwrap().1;
    let still = std::iter::repeat_with(Vec::new).take(11);
    let (cells, _, _) = read(table, std::iter::once(vec![Event::PointerMoved(cell.center())]).chain(still).collect());
    assert!(cells.iter().any(|(t, _)| t.contains("cannot be typeset")), "the reason in a cell: {:?}", cells.iter().map(|p| &p.0).collect::<Vec<_>>());
    // the font is the code font
    let (job, spans, maths) = crate::reader::inline_job("Try $\\unknown{x}$ here.", &[], 15.0, crate::theme::palette().text, false);
    assert!(spans.is_empty() && maths.len() == 1 && maths[0].formula.is_err());
    let s = job.sections.iter().find(|s| job.text[s.byte_range.start.0..s.byte_range.end.0].contains("unknown")).unwrap();
    assert_eq!(s.format.font_id.family, egui::FontFamily::Monospace);
    assert_eq!(s.format.color, crate::theme::palette().code);
}

// spec: EDIT-048, EDIT-024
#[test]
fn edit_048_formulas_in_table_cells_are_drawn_in_their_cells() {
    let text = "| Name | Formula |\n|:---|:---:|\n| Circle | $A = \\pi r^2$ |\n| Euler | $e^{i\\pi} + 1 = 0$ |\n";
    let (painted, pics, _) = read(text, vec![]);
    assert_eq!(pics.len(), 2, "{pics:?}");
    let head = painted.iter().find(|(t, _)| t == "Formula").unwrap().1;
    for (pic, name) in pics.iter().zip(["Circle", "Euler"]) {
        let label = painted.iter().find(|(t, _)| t == name).unwrap().1;
        assert!((pic.center().y - label.center().y).abs() < 9.0, "{name}: on its row: {pic:?} {label:?}");
        assert!(pic.min.x > label.max.x, "{name}: in the next column");
        // the column is centred: the formula is under the middle of its header
        assert!((pic.center().x - head.center().x).abs() < 4.0, "{name}: centred under {head:?}: {pic:?}");
    }
}

// spec: EDIT-048, LINK-013
#[test]
fn edit_048_the_outline_draws_formulas_in_the_headings() {
    let ctx = egui::Context::default();
    crate::theme::apply(&ctx);
    let mut last = (vec![], vec![]);
    for n in 0..4 {
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(400.0, 200.0))), time: Some(n as f64 * 0.05), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            let job = crate::editor::outline_job("Energy $E = mc^2$ and `code`", &[], 14.0);
            assert!(job.text.contains("$E = mc^2$") && !job.text.contains('`'), "the formula's TeX stays (hidden), the markup is gone: {:?}", job.text);
            crate::editor::outline_label(ui, job, 14.0);
        });
        out.textures_delta.clear();
        last = (texts(&out.shapes), pictures(&out.shapes));
    }
    let (painted, pics) = last;
    assert_eq!(pics.len(), 1, "{pics:?}");
    let label = painted.iter().find(|(t, _)| t.starts_with("Energy")).unwrap().1;
    assert!(pics[0].min.x > label.min.x + 30.0 && pics[0].max.x < label.max.x + 2.0, "inside the label after the word: {:?} {label:?}", pics[0]);
    assert!(pics[0].min.y >= label.min.y - 2.0 && pics[0].max.y <= label.max.y + 2.0, "on its row: {:?} {label:?}", pics[0]);
}

// spec: EDIT-048
#[test]
fn edit_048_a_formula_is_laid_out_and_made_into_a_picture_once() {
    let text = "A $x^2$ and again $x^2$ and $y$.\n";
    let (_, pics, ctx) = read(text, vec![]);
    assert_eq!(pics.len(), 3);
    // the same formula in the same size and scale is one picture, however often it is drawn
    assert_eq!(crate::math::paint::pictures_kept(&ctx), 2, "x^2 and y: two pictures for three drawings in many frames");
    // the layout is shared, too
    let a = crate::math::typeset("x^2", 15.0, false).unwrap();
    let b = crate::math::typeset("x^2", 15.0, false).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a, &b));
}
