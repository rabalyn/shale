//! The carousel in the reading view (EDIT-143): one image at a time, the buttons, the dots,
//! "2 / 3", the arrow keys while the pointer is over it, and the way round at the ends.

use super::view::{NEXT, PREVIOUS};
use crate::reader::{show, Caches, Extras, SceneCache};
use crate::vault::Vault;
use eframe::egui::{self, epaint::Shape, Event, Key, Modifiers, PointerButton, Pos2, RawInput, Rect};
use std::path::{Path, PathBuf};

/// What a frame painted: the texts, the pictures of embeds (their rectangles), the labelled buttons.
struct Seen {
    texts: Vec<String>,
    pictures: Vec<Rect>,
    buttons: Vec<(String, Rect)>,
}

impl Seen {
    fn shows(&self, text: &str) -> bool {
        self.texts.iter().any(|t| t == text)
    }

    fn button(&self, label: &str) -> Rect {
        self.buttons.iter().find(|(l, _)| l == label).unwrap_or_else(|| panic!("no {label:?}: {:?}", self.buttons)).1
    }

    /// The width of the one picture shown (the images are 40, 60 and 80 points wide).
    fn width(&self) -> f32 {
        assert_eq!(self.pictures.len(), 1, "one image at a time: {:?}", self.pictures);
        self.pictures[0].width().round()
    }
}

/// The reading view of a note in a 700 x 500 window of a vault with `a.png` (40 wide), `b.png` (60) and
/// `c.png` (80), which stays between the frames.
struct Rig {
    dir: PathBuf,
    vault: Vault,
    ctx: egui::Context,
    text: String,
    scenes: SceneCache,
    media: crate::media::MediaPlayer,
    time: f64,
}

impl Rig {
    fn new(text: &str) -> Rig {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!("shale-carousel-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&dir);
        crate::vault::write_sample_vault(&dir).unwrap();
        for (name, w, r) in [("a.png", 40, 230), ("b.png", 60, 20), ("c.png", 80, 90)] {
            image::RgbaImage::from_pixel(w, 30, image::Rgba([r, 20, 20, 255])).save(dir.join(name)).unwrap();
        }
        let vault = Vault::open(&dir).unwrap();
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        ctx.enable_accesskit();
        let mut rig = Rig { dir, vault, ctx, text: text.to_string(), scenes: SceneCache::default(), media: crate::media::MediaPlayer::default(), time: 0.0 };
        // the pictures are looked up and decoded on workers (MEDIA-024): the first frames wait for them
        let looks = crate::media::look::Looks::of(&rig.ctx.clone());
        looks.settle_with(|| {
            rig.frame(vec![]);
        });
        rig.frames(3);
        rig
    }

    fn frame(&mut self, events: Vec<Event>) -> Seen {
        self.time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 500.0))), time: Some(self.time), events, ..Default::default() };
        let (text, vault) = (self.text.clone(), &self.vault);
        let (scenes, media) = (&mut self.scenes, &self.media);
        let mut out = self.ctx.run_ui(raw, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                show(ui, &text, vault, Path::new("Welcome.md"), &mut Caches { scenes, media }, (15.0, true), &Extras::default());
            });
        });
        out.textures_delta.clear();
        let mut seen = Seen { texts: vec![], pictures: vec![], buttons: vec![] };
        fn walk(s: &Shape, seen: &mut Seen) {
            match s {
                Shape::Text(t) => seen.texts.push(t.galley.job.text.clone()),
                Shape::Rect(r) if r.brush.is_some() => seen.pictures.push(r.rect),
                Shape::Vec(v) => v.iter().for_each(|s| walk(s, seen)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut seen));
        if let Some(tree) = out.platform_output.accesskit_update.take() {
            for (_, n) in tree.nodes.iter() {
                if let (Some(label), Some(b)) = (n.label(), n.bounds()) {
                    seen.buttons.push((label.to_string(), Rect::from_min_max(Pos2::new(b.x0 as f32, b.y0 as f32), Pos2::new(b.x1 as f32, b.y1 as f32))));
                }
            }
        }
        seen
    }

    /// Frames until the screen is still, the last one's picture.
    fn frames(&mut self, n: usize) -> Seen {
        let mut last = self.frame(vec![]);
        for _ in 1..n {
            last = self.frame(vec![]);
        }
        last
    }

    fn click(&mut self, at: Pos2) -> Seen {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![button(true)]);
        self.frame(vec![button(false)]);
        self.frames(3)
    }

    /// The pointer moved to `at` and the key pressed there.
    fn key(&mut self, at: Pos2, key: Key) -> Seen {
        self.frame(vec![Event::PointerMoved(at)]);
        self.frame(vec![Event::Key { key, physical_key: Some(key), pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.frames(3)
    }

    fn at(&mut self) -> Seen {
        self.frames(2)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

const THREE: &str = "Before\n\n> [!carousel] Layers\n> ![[a.png|First]]\n> ![[b.png|Second]]\n> ![[c.png|Third]]\n\nAfter\n";

// spec: EDIT-143
#[test]
fn edit_143_a_carousel_shows_one_image_at_its_size_with_title_caption_and_position() {
    let mut rig = Rig::new(THREE);
    let s = rig.at();
    assert_eq!(s.width(), 40.0, "the first image at its own size");
    for t in ["Layers", "First", "1 / 3", "Before", "After"] {
        assert!(s.shows(t), "{t:?} in {:?}", s.texts);
    }
    assert!(!s.shows("Second") && !s.shows("[!carousel] Layers"), "the quote's own text is not drawn: {:?}", s.texts);
    assert!(s.buttons.iter().any(|(l, _)| l == PREVIOUS) && s.buttons.iter().any(|(l, _)| l == NEXT));
}

// spec: EDIT-143
#[test]
fn edit_143_the_buttons_and_the_dots_go_through_the_images() {
    let mut rig = Rig::new(THREE);
    let s = rig.at();
    let s = rig.click(s.button(NEXT).center());
    assert_eq!((s.width(), s.shows("2 / 3"), s.shows("Second"), s.shows("First")), (60.0, true, true, false));
    let s = rig.click(s.button(NEXT).center());
    assert_eq!((s.width(), s.shows("3 / 3"), s.shows("Third")), (80.0, true, true));
    let s = rig.click(s.button(PREVIOUS).center());
    assert_eq!((s.width(), s.shows("2 / 3")), (60.0, true));
    let s = rig.click(s.button("Image 1").center());
    assert_eq!((s.width(), s.shows("1 / 3")), (40.0, true), "a dot goes to its image");
    let s = rig.click(s.button("Image 3").center());
    assert_eq!((s.width(), s.shows("3 / 3")), (80.0, true));
}

// spec: EDIT-143
#[test]
fn edit_143_the_images_go_round_at_the_ends() {
    let mut rig = Rig::new(THREE);
    let s = rig.at();
    let s = rig.click(s.button(PREVIOUS).center());
    assert!(s.shows("3 / 3"), "before the first is the last: {:?}", s.texts);
    let s = rig.click(s.button(NEXT).center());
    assert!(s.shows("1 / 3"), "after the last is the first: {:?}", s.texts);
}

// spec: EDIT-143
#[test]
fn edit_143_the_arrow_keys_work_only_while_the_pointer_is_over_the_carousel() {
    let mut rig = Rig::new(THREE);
    let s = rig.at();
    let over = s.pictures[0].center();
    let away = Pos2::new(650.0, 480.0);
    assert!(rig.key(away, Key::ArrowRight).shows("1 / 3"), "the pointer is elsewhere");
    assert!(rig.key(over, Key::ArrowRight).shows("2 / 3"));
    assert!(rig.key(over, Key::ArrowRight).shows("3 / 3"));
    assert!(rig.key(over, Key::ArrowRight).shows("1 / 3"), "round");
    assert!(rig.key(over, Key::ArrowLeft).shows("3 / 3"), "left goes back, round");
    assert!(rig.key(away, Key::ArrowLeft).shows("3 / 3"));
}

// spec: EDIT-143
#[test]
fn edit_143_each_carousel_has_its_own_image() {
    let two = "> [!carousel] One\n> ![[a.png]]\n> ![[b.png]]\n\n> [!carousel] Two\n> ![[a.png]]\n> ![[c.png]]\n";
    let mut rig = Rig::new(two);
    let s = rig.at();
    let first = s.buttons.iter().filter(|(l, _)| l == NEXT).map(|(_, r)| *r).min_by(|a, b| a.min.y.total_cmp(&b.min.y)).unwrap();
    let s = rig.click(first.center());
    let sizes: Vec<f32> = s.pictures.iter().map(|r| r.width().round()).collect();
    assert_eq!(sizes, [60.0, 40.0], "the first carousel went on, the second did not: {:?}", s.texts);
}

// spec: EDIT-143
#[test]
fn edit_143_a_missing_image_is_its_link_in_the_slide_and_the_carousel_still_goes_on() {
    let mut rig = Rig::new("> [!carousel]\n> ![[a.png]]\n> ![[nothing.png|Gone]]\n");
    let s = rig.at();
    let s = rig.click(s.button(NEXT).center());
    assert!(s.shows("![[nothing.png]]") && s.shows("Gone") && s.shows("2 / 2"), "{:?}", s.texts);
    assert!(s.pictures.is_empty());
}

// spec: EDIT-143
#[test]
fn edit_143_any_other_quote_is_drawn_as_a_quote_without_a_carousel() {
    let mut rig = Rig::new("> [!note] Layers\n> ![[a.png]]\n");
    let s = rig.at();
    assert!(!s.buttons.iter().any(|(l, _)| l == NEXT) && !s.texts.iter().any(|t| t.contains(" / ")), "{:?}", s.texts);
}
