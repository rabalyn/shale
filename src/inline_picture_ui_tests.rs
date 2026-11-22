//! A picture inside a line in the reading view (EDIT-114): drawn in the line at 0.9 of its height
//! and lowered by 0.2 of it, also in table cells; what cannot be shown stays link text.

use crate::reader::{show, Caches, Extras, SceneCache};
use crate::vault::Vault;
use eframe::egui::{self, epaint::Shape, Color32, Event, Pos2, RawInput, Rect};
use std::path::Path;
use std::sync::Arc;

/// What a frame painted: the text of every galley with its position and the galley, and the
/// rectangles of the pictures (the font atlas is texture 0).
struct Frame {
    texts: Vec<(String, Pos2, Arc<egui::Galley>)>,
    pictures: Vec<Rect>,
    /// the pictures of embeds on lines of their own (drawn as textured rectangles)
    blocks: Vec<Rect>,
}

impl Frame {
    fn text(&self, start: &str) -> &(String, Pos2, Arc<egui::Galley>) {
        self.texts.iter().find(|(t, ..)| t.starts_with(start)).unwrap_or_else(|| panic!("{start:?} is not painted: {:?}", self.texts.iter().map(|t| &t.0).collect::<Vec<_>>()))
    }

    /// Whether any text of the galley that starts with `start` is invisible.
    fn has_hidden_text(&self, start: &str) -> bool {
        self.text(start).2.job.sections.iter().any(|s| s.format.color == Color32::TRANSPARENT)
    }
}

/// The reading view of `text` in a 700 x 500 window of a vault with `icon.png` (40 x 30),
/// `wide.png` (3000 x 30), a damaged `broken.png`, a drawing and a PDF; the events are for the
/// frames after the first ones, which wait for the pictures.
fn read(text: &str, frames: Vec<Vec<Event>>) -> Frame {
    static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("shale-inline-picture-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let _ = std::fs::remove_dir_all(&d);
    crate::vault::write_sample_vault(&d).unwrap();
    image::RgbaImage::from_pixel(40, 30, image::Rgba([230, 20, 20, 255])).save(d.join("icon.png")).unwrap();
    image::RgbaImage::from_pixel(3000, 30, image::Rgba([20, 20, 230, 255])).save(d.join("wide.png")).unwrap();
    std::fs::write(d.join("broken.png"), b"no picture").unwrap();
    std::fs::write(d.join("Sketch.excalidraw"), crate::drawing::SAMPLE_SCENE).unwrap();
    std::fs::write(d.join("doc.pdf"), crate::pdf::view::sample_pdf()).unwrap();
    let v = Vault::open(&d).unwrap();
    let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
    let ctx = egui::Context::default();
    let ctx2 = ctx.clone();
    crate::theme::apply(&ctx);
    let mut time = 0.0;
    let mut last = Frame { texts: vec![], pictures: vec![], blocks: vec![] };
    let mut run = |events: Vec<Event>| {
        time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(time), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                show(ui, text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras::default());
            });
        });
        out.textures_delta.clear();
        let mut frame = Frame { texts: vec![], pictures: vec![], blocks: vec![] };
        fn walk(s: &Shape, frame: &mut Frame) {
            match s {
                Shape::Text(t) => frame.texts.push((t.galley.job.text.clone(), t.pos, t.galley.clone())),
                Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => frame.pictures.push(m.calc_bounds()),
                Shape::Rect(r) if r.brush.is_some() => frame.blocks.push(r.rect),
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, frame)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut frame));
        frame
    };
    // the pictures are looked up and decoded on workers (MEDIA-024): the first frames wait for them
    let looks = crate::media::look::Looks::of(&ctx2);
    let mut idle = 0;
    while idle < 3 {
        last = run(Vec::new());
        idle = if looks.busy() { 0 } else { idle + 1 };
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    for events in frames.into_iter().chain(std::iter::repeat_with(Vec::new).take(10)) {
        last = run(events);
    }
    let _ = std::fs::remove_dir_all(&d);
    last
}

/// The baseline of the first row of a painted galley, in the space of the screen.
fn baseline(t: &(String, Pos2, Arc<egui::Galley>)) -> f32 {
    let row = &t.2.rows[0];
    t.1.y + row.pos.y + row.row.glyphs.iter().filter(|g| g.font_height > 2.0).map(|g| g.pos.y).fold(0.0, f32::max)
}

// spec: EDIT-114
#[test]
fn edit_114_a_picture_in_a_line_is_drawn_in_it_at_09_of_the_line_and_lowered_by_02() {
    let frame = read("an icon ![[icon.png]] here\n", vec![]);
    assert_eq!(frame.pictures.len(), 1, "{:?}", frame.pictures);
    let (pic, line) = (frame.pictures[0], frame.text("an icon"));
    // 15 points of text: the line is 22.5, the picture 0.9 of it high and 40 : 30 wide
    assert!((pic.height() - 20.25).abs() < 0.6 && (pic.width() - 27.0).abs() < 0.8, "size {:?}", pic.size());
    assert!((pic.max.y - (baseline(line) + 0.2 * 22.5)).abs() < 1.0, "bottom {} lowered from the baseline {}", pic.max.y, baseline(line));
    // in the room of its hidden link text, between the words around it
    let x = |i: usize| line.1.x + line.2.pos_from_cursor(egui::text::CCursor::new(i)).min.x;
    assert!((pic.min.x - x(8)).abs() < 1.0, "at the start of the link text: {} {}", pic.min.x, x(8));
    assert!(x(16) >= pic.max.x - 1.0 && x(16) - pic.max.x < 6.0, "the text goes on right after it: {} {}", pic.max.x, x(16));
    assert!(frame.has_hidden_text("an icon"), "the link text is invisible");
    // the line is no taller than a line of text of the same size
    let plain = read("an icon here\n", vec![]);
    assert_eq!(line.2.size().y, plain.text("an icon").2.size().y);
}

// spec: EDIT-114
#[test]
fn edit_114_a_wide_picture_is_not_wider_than_the_column_and_the_link_still_answers_the_pointer() {
    let frame = read("see ![[wide.png]] there\n", vec![]);
    assert_eq!(frame.pictures.len(), 1);
    let pic = frame.pictures[0];
    assert!(pic.width() <= 700.0 && pic.width() > 400.0, "scaled to the column: {:?}", pic.size());
    assert!((pic.width() / pic.height() - 2048.0 / 20.0).abs() < 4.0, "with its proportions: {:?}", pic.size());
    // the pointer over a picture gets what it gets over the link text
    let frame = read("an icon ![[icon.png]] here\n", vec![]);
    let over = frame.pictures[0].center();
    let still = std::iter::repeat_with(Vec::new).take(11);
    let hovered = read("an icon ![[icon.png]] here\n", std::iter::once(vec![Event::PointerMoved(over)]).chain(still).collect());
    assert!(hovered.texts.iter().any(|(t, ..)| t.contains("Open") && t.contains("icon.png")), "the tooltip of the link: {:?}", hovered.texts.iter().map(|t| &t.0).collect::<Vec<_>>());
}

// spec: EDIT-114
#[test]
fn edit_114_what_cannot_be_drawn_in_a_line_stays_its_link_text() {
    for link in ["![[nothing.png]]", "![[broken.png]]", "![[Sketch.excalidraw]]", "![[doc.pdf]]", "[[icon.png]]"] {
        let frame = read(&format!("left {link} right\n"), vec![]);
        assert!(frame.pictures.is_empty(), "{link}: nothing is drawn: {:?}", frame.pictures);
        assert!(!frame.has_hidden_text("left"), "{link}: the link text is shown");
    }
    // an embed on a line of its own is as before: the picture is a block, not in a line
    let frame = read("![[icon.png]]\n", vec![]);
    assert!(frame.pictures.is_empty() && frame.blocks.len() == 1, "a block, not in a line: {:?} {:?}", frame.pictures, frame.blocks);
    assert!(frame.blocks[0].height() > 25.0, "its own size, not the line's: {:?}", frame.blocks[0].size());
}

// spec: EDIT-114
#[test]
fn edit_114_a_picture_in_a_table_cell_is_drawn_in_the_cell() {
    let frame = read("| A | B |\n|---|---|\n| one | go ![[icon.png]] on |\n", vec![]);
    assert_eq!(frame.pictures.len(), 1, "{:?}", frame.pictures);
    let (pic, label) = (frame.pictures[0], frame.text("one"));
    let cell = frame.text("go ");
    let one = Rect::from_min_size(label.1, label.2.size());
    assert!(pic.min.x > one.max.x && (pic.center().y - one.center().y).abs() < 10.0, "in the next column on the row of 'one': {pic:?} {one:?}");
    assert!(pic.height() > 18.0 && pic.height() < 21.5, "0.9 of the line of the cell: {:?}", pic.size());
    assert!(frame.has_hidden_text("go ") && cell.0.contains("icon.png"));
    let broken = read("| A | B |\n|---|---|\n| one | go ![[broken.png]] on |\n", vec![]);
    assert!(broken.pictures.is_empty() && !broken.has_hidden_text("go "), "an unreadable picture is link text in a cell too");
}
