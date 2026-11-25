//! The colours that are really drawn (CFG-082, #691): a dropdown with a selected row, a hovered
//! row, a hovered button and a pressed one are run through egui in every built-in theme, and the
//! colour of each text is checked against the fill of the rectangle painted right behind it. The
//! palette tests (`contrast_tests`) check the pairs the palette names; this one checks what egui
//! makes of them, so a colour that egui picks by itself (the text of a selected row) is seen too.

use super::contrast_tests::themes;
use super::mapping::contrast;
use crate::theme::{self, Palette};
use eframe::egui::{self, epaint::ClippedShape, epaint::Shape, Color32, Event, PointerButton, Pos2, RawInput, Rect, Vec2};

/// The contrast text needs (WCAG AA).
const TEXT: f32 = 4.5;

/// One text of a frame with the colour it is drawn in, and the fill of the rectangle painted
/// right behind it (the last opaque one before the text that has its middle).
struct Drawn {
    text: String,
    colour: Color32,
    behind: Color32,
}

/// The texts of `shapes` in the order they are painted.
fn drawn(shapes: &[ClippedShape]) -> Vec<Drawn> {
    fn flatten<'a>(s: &'a Shape, out: &mut Vec<&'a Shape>) {
        match s {
            Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            other => out.push(other),
        }
    }
    let mut flat = vec![];
    shapes.iter().for_each(|c| flatten(&c.shape, &mut flat));
    let mut out = vec![];
    let mut rects: Vec<(Rect, Color32)> = vec![];
    for s in flat {
        match s {
            Shape::Rect(r) if r.fill.a() == 255 => rects.push((r.rect, r.fill)),
            Shape::Text(t) => {
                let middle = t.visual_bounding_rect().center();
                let Some((_, behind)) = rects.iter().rev().find(|(r, _)| r.contains(middle)) else { continue };
                for section in &t.galley.job.sections {
                    let colour = t.override_text_color.unwrap_or(if section.format.color == Color32::PLACEHOLDER { t.fallback_color } else { section.format.color });
                    out.push(Drawn { text: t.galley.text().to_string(), colour, behind: *behind });
                }
            }
            _ => {}
        }
    }
    out
}

/// What the pointer does in the last frame of a scene.
#[derive(Clone, Copy, PartialEq)]
enum Scene {
    /// A dropdown, open, with the first row selected and the pointer over the second.
    Dropdown,
    /// A button with the pointer over it.
    Hovered,
    /// A button with the mouse button held down on it.
    Pressed,
}

type Places = Vec<(&'static str, Rect)>;

/// Run the frames of `scene` in a window with the palette `p`; the texts of the last one with the
/// rectangles behind them.
fn render(p: &Palette, scene: Scene) -> Vec<Drawn> {
    let ctx = egui::Context::default();
    theme::use_palette(&ctx, *p);
    let mut picked = 1;
    let mut t = 0.0;
    let mut places: Places = vec![];
    let mut step = |events: Vec<Event>, places: &mut Places| {
        t += 0.1;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))), time: Some(t), events, ..Default::default() };
        places.clear();
        let mut out = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("A label in the panel");
                let combo = egui::ComboBox::from_id_salt("pick").selected_text("Ablage").show_ui(ui, |ui| {
                    for (n, name) in [(1, "Ablage"), (2, "Projekte"), (3, "Archiv")] {
                        let r = ui.selectable_value(&mut picked, n, name);
                        places.push((name, r.rect));
                    }
                });
                places.push(("combo", combo.response.rect));
                let b = ui.button("A button");
                places.push(("button", b.rect));
            });
        });
        out.textures_delta.clear();
        out
    };
    let centre = |places: &Places, name: &str| places.iter().find(|(n, _)| *n == name).map(|(_, r)| r.center()).unwrap_or_else(|| panic!("{name} was not laid out"));
    let click = |pos: Pos2, pressed: bool| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Default::default() };
    step(vec![], &mut places);
    let last = match scene {
        Scene::Dropdown => {
            let combo = centre(&places, "combo");
            step(vec![Event::PointerMoved(combo)], &mut places);
            step(vec![click(combo, true)], &mut places);
            step(vec![click(combo, false)], &mut places);
            step(vec![], &mut places);
            let row = centre(&places, "Projekte");
            step(vec![Event::PointerMoved(row)], &mut places);
            step(vec![], &mut places)
        }
        Scene::Hovered | Scene::Pressed => {
            let button = centre(&places, "button");
            step(vec![Event::PointerMoved(button)], &mut places);
            if scene == Scene::Pressed {
                step(vec![click(button, true)], &mut places);
            }
            step(vec![], &mut places)
        }
    };
    drawn(&last.shapes)
}

/// The texts of `scene` that do not read, and the texts of `wanted` that were not drawn on their
/// fill (the scene is not what the test thinks it is), as lines.
fn scene_failures(name: &str, p: &Palette, scene: Scene, wanted: &[(&str, Color32)]) -> Vec<String> {
    let texts = render(p, scene);
    let mut bad = vec![];
    for d in &texts {
        if contrast(d.colour, d.behind) < TEXT {
            bad.push(format!("{name}: {:?} is {:?} on {:?}: {:.2}", d.text, d.colour, d.behind, contrast(d.colour, d.behind)));
        }
    }
    for (text, fill) in wanted {
        if !texts.iter().any(|d| d.text == *text && d.behind == *fill) {
            bad.push(format!("{name}: {text:?} was not drawn on {fill:?}: {:?}", texts.iter().map(|d| (&d.text, d.behind)).collect::<Vec<_>>()));
        }
    }
    bad
}

// spec: CFG-082
#[test]
fn cfg_082_the_text_drawn_in_a_dropdown_and_on_a_button_reads_on_the_fill_behind_it_in_every_theme() {
    let mut bad = vec![];
    for (name, p) in themes() {
        // the selected row, a hovered row, the text of the panel and of the closed dropdown
        bad.extend(scene_failures(&name, &p, Scene::Dropdown, &[("Ablage", p.selection_bg), ("Projekte", p.bg_hover)]));
        bad.extend(scene_failures(&name, &p, Scene::Hovered, &[("A button", p.bg_hover)]));
        bad.extend(scene_failures(&name, &p, Scene::Pressed, &[("A button", p.bg_active)]));
    }
    assert!(bad.is_empty(), "{} texts do not read:\n{}", bad.len(), bad.join("\n"));
}

// spec: CFG-082
#[test]
fn cfg_082_the_scene_shows_what_the_issue_reported_with_the_old_selection_colour() {
    // before #691 egui drew the text of the selected row in the accent: the same scene with that
    // colour put back must fail, so the check above would have caught the bug
    let p = themes().into_iter().find(|(n, _)| n == "Catppuccin Latte").expect("the theme").1;
    let ctx_pair = |colour: Color32| contrast(colour, p.selection_bg);
    assert!(ctx_pair(p.accent) < TEXT, "the accent of Latte on its selection is the pink on salmon: {:.2}", ctx_pair(p.accent));
    let texts = render(&p, Scene::Dropdown);
    let row = texts.iter().find(|d| d.text == "Ablage" && d.behind == p.selection_bg).expect("the selected row");
    assert_ne!(row.colour, p.accent);
    assert!(contrast(row.colour, row.behind) >= TEXT);
}
