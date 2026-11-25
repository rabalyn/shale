//! An image carousel in the editor's rendered state (EDIT-144): one image of the quote at a time
//! with the row of buttons below it, the other slides hidden, the lines raw while the cursor is in
//! the carousel, and ← and → under the pointer while the text has no keyboard.

use super::tests::{vault_for_tests, Harness};
use super::*;
use egui::{Event, PointerButton};

const NOTE: &str = "intro\n\n> [!carousel] Layers\n> ![[a.png|First]]\n> ![[b.png|Second]]\n> ![[c.png|Third]]\n\nafter";

fn setup(tag: &str) -> (NoteEditor, Harness, Vault, std::path::PathBuf, String) {
    let (v, d) = vault_for_tests(tag);
    for (name, c) in [("a.png", 200u8), ("b.png", 120), ("c.png", 40)] {
        image::RgbaImage::from_pixel(200, 120, image::Rgba([c, 30, 30, 255])).save(d.join(name)).unwrap();
    }
    let mut v = v;
    v.rescan();
    let mut ed = NoteEditor::new(egui::Id::unique(tag.to_string()));
    ed.inline_images = true;
    let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
    let mut text = NOTE.to_string();
    h.settle(&mut ed, &mut text, &v, Path::new("Welcome.md"));
    (ed, h, v, d, text)
}

fn rect_of(h: &Harness, t: &str) -> Option<Rect> {
    h.rects.iter().find(|(s, _)| s == t).map(|(_, r)| *r)
}

fn shows(h: &Harness, t: &str) -> bool {
    rect_of(h, t).is_some()
}

fn press(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }
}

fn click(h: &mut Harness, ed: &mut NoteEditor, text: &mut String, v: &Vault, at: Pos2) {
    let rel = Path::new("Welcome.md");
    h.frame(ed, text, v, rel, vec![Event::PointerMoved(at)]);
    h.frame(ed, text, v, rel, vec![press(at, true)]);
    h.frame(ed, text, v, rel, vec![press(at, false)]);
    for _ in 0..3 {
        h.frame(ed, text, v, rel, vec![]);
    }
}

/// The centres of the buttons "Previous image" and "Next image" and of the first dot, from the place of the position label.
fn buttons(h: &Harness, position: &str) -> (Pos2, Pos2, Pos2) {
    let label = rect_of(h, position).unwrap_or_else(|| panic!("{position:?} is not painted: {:?}", h.texts));
    let y = label.center().y;
    let next = Pos2::new(label.left() - 8.0 - 12.0, y);
    (Pos2::new(next.x - 32.0, y), next, Pos2::new(label.right() + 8.0 + 7.0, y))
}

// spec: EDIT-144
#[test]
fn edit_144_the_editor_shows_one_image_of_a_carousel_with_its_buttons_and_hides_the_other_slides() {
    let (ed, h, _, d, _) = setup("car-one");
    let _ = ed;
    for t in ["1 / 3", "First"] {
        assert!(shows(&h, t), "{t:?} in {:?}", h.texts);
    }
    // the room of one image, not three
    let note = h.rects.iter().find(|(t, _)| t.starts_with("intro")).expect("the text of the note").1;
    assert!(note.height() > 120.0 && note.height() < 120.0 * 2.0 + 120.0, "one image of 120 points, not three: {note:?}");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-144
#[test]
fn edit_144_the_buttons_and_the_dots_go_through_the_images_and_round_at_the_ends() {
    let (mut ed, mut h, v, d, mut text) = setup("car-buttons");
    let rel = Path::new("Welcome.md");
    let (prev, next, dot) = buttons(&h, "1 / 3");
    click(&mut h, &mut ed, &mut text, &v, next);
    assert!(shows(&h, "2 / 3") && shows(&h, "Second") && !shows(&h, "First"), "next: {:?}", h.texts);
    let (prev2, _, _) = buttons(&h, "2 / 3");
    click(&mut h, &mut ed, &mut text, &v, prev2);
    assert!(shows(&h, "1 / 3") && shows(&h, "First"), "previous: {:?}", h.texts);
    click(&mut h, &mut ed, &mut text, &v, prev);
    assert!(shows(&h, "3 / 3") && shows(&h, "Third"), "before the first is the last: {:?}", h.texts);
    let (_, next3, _) = buttons(&h, "3 / 3");
    click(&mut h, &mut ed, &mut text, &v, next3);
    assert!(shows(&h, "1 / 3"), "after the last is the first: {:?}", h.texts);
    // the third dot shows the third image; the note is as written
    click(&mut h, &mut ed, &mut text, &v, Pos2::new(dot.x + 2.0 * 22.0, dot.y));
    assert!(shows(&h, "3 / 3"), "{:?}", h.texts);
    assert_eq!(text, NOTE);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-144
#[test]
fn edit_144_the_arrow_keys_work_under_the_pointer_while_the_text_has_no_keyboard() {
    let (mut ed, mut h, v, d, mut text) = setup("car-keys");
    let rel = Path::new("Welcome.md");
    let key = |k: Key| Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: Modifiers::NONE };
    let over = rect_of(&h, "First").unwrap().center() - Vec2::new(0.0, 60.0);
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(Pos2::new(850.0, 650.0))]);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowRight)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(shows(&h, "1 / 3"), "the pointer is elsewhere");
    h.frame(&mut ed, &mut text, &v, rel, vec![Event::PointerMoved(over)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowRight)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(shows(&h, "2 / 3"), "{:?}", h.texts);
    h.frame(&mut ed, &mut text, &v, rel, vec![key(Key::ArrowLeft)]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    h.frame(&mut ed, &mut text, &v, rel, vec![]);
    assert!(shows(&h, "1 / 3"), "{:?}", h.texts);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-144
#[test]
fn edit_144_with_the_cursor_in_the_carousel_all_its_lines_are_raw() {
    let (mut ed, mut h, v, d, mut text) = setup("car-raw");
    ed.pending_cursor = Some(NOTE.find("Second").unwrap());
    h.settle(&mut ed, &mut text, &v, Path::new("Welcome.md"));
    assert!(!shows(&h, "1 / 3"), "no buttons while it is edited: {:?}", h.texts);
    for link in ["![[a.png|First]]", "![[b.png|Second]]", "![[c.png|Third]]"] {
        assert!(h.texts.iter().any(|t| t.contains(link)), "{link} is shown as written: {:?}", h.texts);
    }
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-144
#[test]
fn edit_144_any_other_quote_keeps_its_images_and_the_slides_are_slides_again_without_the_syntax() {
    let (v, d) = vault_for_tests("car-other");
    image::RgbaImage::from_pixel(200, 120, image::Rgba([200, 30, 30, 255])).save(d.join("a.png")).unwrap();
    image::RgbaImage::from_pixel(200, 120, image::Rgba([30, 30, 200, 255])).save(d.join("b.png")).unwrap();
    let mut v = v;
    v.rescan();
    let mut ed = NoteEditor::new(egui::Id::unique("car-other"));
    ed.inline_images = true;
    let mut h = Harness { ctx: egui::Context::default(), time: 0.0, texts: vec![], rects: vec![] };
    let mut text = "> [!note] Layers\n> ![[a.png|First]]\n> ![[b.png|Second]]\nafter".to_string();
    h.settle(&mut ed, &mut text, &v, Path::new("Welcome.md"));
    assert!(shows(&h, "First") && shows(&h, "Second") && !h.texts.iter().any(|t| t.contains(" / ")), "{:?}", h.texts);
    let _ = std::fs::remove_dir_all(&d);
}
