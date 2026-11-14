//! UI tests for every button and control of the drawing editor: each test renders the real
//! editor with egui, moves the pointer onto a control, presses and releases the mouse button,
//! and checks that the drawing changed. (A child module of `drawing::connect`.)

use super::*;
use eframe::egui::{Event, Modifiers, PointerButton, RawInput};

struct H {
    ctx: egui::Context,
    t: f64,
}

impl H {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        H { ctx, t: 0.0 }
    }

    fn frame(&mut self, d: &mut DrawingDoc, events: Vec<Event>) {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1300.0))), time: Some(self.t), events, ..Default::default() };
        let mut out = self.ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| d.ui(ui, true));
        });
        out.textures_delta.clear();
    }

    fn settle(&mut self, d: &mut DrawingDoc) {
        for _ in 0..3 {
            self.frame(d, vec![]);
        }
    }

    fn rect(d: &DrawingDoc, key: &str) -> Rect {
        d.ui_rects.iter().rev().find(|(k, _)| k == key).map(|r| r.1).unwrap_or_else(|| {
            let have: Vec<&String> = d.ui_rects.iter().map(|r| &r.0).collect();
            panic!("no control called {key:?} on screen; controls: {have:?}")
        })
    }

    fn click_at(&mut self, d: &mut DrawingDoc, p: Pos2) {
        let btn = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        self.frame(d, vec![Event::PointerMoved(p)]);
        self.frame(d, vec![btn(true)]);
        self.frame(d, vec![btn(false)]);
        self.frame(d, vec![]);
    }

    /// Click the control named `key` (as recorded in `ui_rects`).
    fn click(&mut self, d: &mut DrawingDoc, key: &str) {
        let r = Self::rect(d, key);
        self.click_at(d, r.center());
    }
}

/// The sample drawing with the rectangle "Idea" (s1, label s2) selected.
fn setup() -> (H, DrawingDoc) {
    let mut h = H::new();
    let mut d = DrawingDoc::new(PathBuf::from("ui.excalidraw"), SAMPLE_SCENE);
    h.settle(&mut d);
    d.selection = vec!["s1".into()];
    h.settle(&mut d);
    (h, d)
}

fn el<'a>(d: &'a DrawingDoc, id: &str) -> &'a Element {
    d.scene.elements.iter().find(|e| e.id == id).unwrap()
}

// spec: DRAW-005
#[test]
fn stroke_color_buttons() {
    let (mut h, mut d) = setup();
    for c in STROKE_COLORS.iter().rev() {
        h.click(&mut d, &format!("stroke:{c}"));
        assert_eq!(el(&d, "s1").stroke_color, *c, "stroke color button {c}");
    }
}

// spec: DRAW-005
#[test]
fn shape_background_buttons() {
    let (mut h, mut d) = setup();
    for c in BG_COLORS.iter().rev() {
        h.click(&mut d, &format!("bg:{c}"));
        assert_eq!(el(&d, "s1").background_color, *c, "background button {c}");
    }
}

// spec: DRAW-005
#[test]
fn fill_buttons() {
    let (mut h, mut d) = setup();
    for (label, value) in [("Solid", "solid"), ("Cross", "cross-hatch"), ("Hatch", "hachure")] {
        h.click(&mut d, &format!("fill:{label}"));
        assert_eq!(el(&d, "s1").fill_style, value, "fill button {label}");
    }
}

// spec: DRAW-005
#[test]
fn stroke_width_buttons_and_slider() {
    let (mut h, mut d) = setup();
    for (label, value) in [("Thin", 1.0), ("Extra", 4.0), ("Bold", 2.0)] {
        h.click(&mut d, &format!("width:{label}"));
        assert_eq!(el(&d, "s1").stroke_width, value, "width button {label}");
    }
    let r = H::rect(&d, "width-slider");
    h.click_at(&mut d, Pos2::new(r.min.x + 4.0, r.center().y));
    assert!(el(&d, "s1").stroke_width < 1.0, "slider at the left end sets a thin line, got {}", el(&d, "s1").stroke_width);
}

// spec: DRAW-005
#[test]
fn outline_pattern_dropdown() {
    let (mut h, mut d) = setup();
    for (value, label) in STROKE_STYLES.iter().rev() {
        h.click(&mut d, "pattern");
        h.click(&mut d, &format!("pattern:{label}"));
        assert_eq!(el(&d, "s1").stroke_style, *value, "pattern {label}");
    }
}

// spec: DRAW-005
#[test]
fn sloppiness_buttons() {
    let (mut h, mut d) = setup();
    for (label, value) in [("Neat", 0.0), ("Wobbly", 2.0), ("Sketchy", 1.0)] {
        h.click(&mut d, &format!("rough:{label}"));
        assert_eq!(el(&d, "s1").roughness, value, "sloppiness {label}");
    }
}

// spec: DRAW-005
#[test]
fn corner_buttons() {
    let (mut h, mut d) = setup();
    h.click(&mut d, "corners:Sharp");
    assert!(el(&d, "s1").extra["roundness"].is_null());
    h.click(&mut d, "corners:Round");
    assert_eq!(el(&d, "s1").extra["roundness"]["type"], 3);
}

// spec: DRAW-005
#[test]
fn label_font_buttons() {
    let (mut h, mut d) = setup();
    for (label, value) in [("Normal", 2), ("Code", 3), ("Hand-drawn", 1)] {
        h.click(&mut d, &format!("font:{label}"));
        assert_eq!(el(&d, "s2").extra["fontFamily"], value, "font {label}");
    }
}

// spec: DRAW-005
#[test]
fn label_size_buttons() {
    let (mut h, mut d) = setup();
    for (label, value) in [("S", 16.0), ("L", 28.0), ("XL", 36.0), ("M", 20.0)] {
        h.click(&mut d, &format!("size:{label}"));
        assert_eq!(el(&d, "s2").font_size, Some(value), "size {label}");
    }
}

// spec: DRAW-005
#[test]
fn label_text_color_buttons() {
    let (mut h, mut d) = setup();
    for c in STROKE_COLORS.iter().rev() {
        h.click(&mut d, &format!("textcolor:{c}"));
        assert_eq!(el(&d, "s2").stroke_color, *c, "text color {c}");
    }
}

// spec: DRAW-005
#[test]
fn label_background_buttons() {
    let (mut h, mut d) = setup();
    for c in BG_COLORS.iter().rev() {
        h.click(&mut d, &format!("labelbg:{c}"));
        assert_eq!(el(&d, "s2").background_color, *c, "label background {c}");
    }
}

// spec: DRAW-005
#[test]
fn label_align_buttons() {
    let (mut h, mut d) = setup();
    for (label, value) in [("Left", "left"), ("Right", "right"), ("Center", "center")] {
        h.click(&mut d, &format!("align:{label}"));
        assert_eq!(el(&d, "s2").extra["textAlign"], value, "align {label}");
    }
}

// spec: DRAW-005
#[test]
fn opacity_slider() {
    let (mut h, mut d) = setup();
    let r = H::rect(&d, "opacity-slider");
    h.click_at(&mut d, Pos2::new(r.min.x + 4.0, r.center().y));
    assert!(el(&d, "s1").opacity < 50.0, "opacity slider moved to the left end, got {}", el(&d, "s1").opacity);
}

// spec: DRAW-005
#[test]
fn action_to_front_and_to_back() {
    let (mut h, mut d) = setup();
    let pos = |d: &DrawingDoc, id: &str| d.scene.elements.iter().position(|e| e.id == id).unwrap();
    h.click(&mut d, "action:To front");
    assert!(pos(&d, "s1") > pos(&d, "s4"), "rectangle in front of the ellipse");
    assert!(pos(&d, "s2") > pos(&d, "s1"), "label stays on top of its shape");
    h.click(&mut d, "action:To back");
    assert_eq!(pos(&d, "s1"), 0, "rectangle at the back");
}

// spec: DRAW-005
#[test]
fn action_duplicate() {
    let (mut h, mut d) = setup();
    let before = d.scene.live().count();
    h.click(&mut d, "action:Duplicate");
    assert_eq!(d.scene.live().count(), before + 2, "shape and its label are copied");
    assert_ne!(d.selection, vec!["s1".to_string()], "the copy is selected");
}

// spec: DRAW-005
#[test]
fn action_remove() {
    let (mut h, mut d) = setup();
    h.click(&mut d, "action:Remove");
    assert!(el(&d, "s1").is_deleted);
    assert!(el(&d, "s2").is_deleted, "its label goes too");
}

// spec: DRAW-005
#[test]
fn action_edit_label() {
    let (mut h, mut d) = setup();
    h.click(&mut d, "action:Edit label");
    assert_eq!(d.editing_text.as_deref(), Some("s2"));
}

// spec: DRAW-005
#[test]
fn line_end_dropdowns() {
    let (mut h, mut d) = setup();
    d.selection = vec!["s3".into()];
    h.settle(&mut d);
    for (value, label) in HEADS {
        h.click(&mut d, "start-head");
        h.click(&mut d, &format!("start-head:{label}"));
        assert_eq!(el(&d, "s3").arrowhead("startArrowhead").as_deref(), value, "line start {label}");
        h.click(&mut d, "end-head");
        h.click(&mut d, &format!("end-head:{label}"));
        assert_eq!(el(&d, "s3").arrowhead("endArrowhead").as_deref(), value, "line end {label}");
    }
}

// spec: DRAW-005
#[test]
fn clicking_the_panel_background_keeps_the_selection() {
    let (mut h, mut d) = setup();
    let panel = d.props_rect.expect("panel on screen");
    h.click_at(&mut d, Pos2::new(panel.max.x - 6.0, panel.min.y + 6.0));
    assert_eq!(d.selection, vec!["s1".to_string()], "a click on the panel must not reach the canvas");
}

// spec: DRAW-005
#[test]
fn toolbar_buttons() {
    let (mut h, mut d) = setup();
    for t in Tool::ALL.iter().rev() {
        h.click(&mut d, &format!("tool:{t:?}"));
        assert_eq!(d.tool, *t, "toolbar button {t:?}");
    }
    assert!(!d.tool_locked);
    h.click(&mut d, "lock");
    assert!(d.tool_locked, "lock button");
    h.click(&mut d, "lock");
    assert!(!d.tool_locked);
}

// spec: DRAW-005
#[test]
fn zoom_buttons() {
    let (mut h, mut d) = setup();
    h.click(&mut d, "zoom-reset");
    assert!((d.zoom - 1.0).abs() < 1e-3, "reset to 100%");
    h.click(&mut d, "zoom-in");
    assert!(d.zoom > 1.1, "zoom in");
    h.click(&mut d, "zoom-out");
    h.click(&mut d, "zoom-out");
    assert!(d.zoom < 1.0, "zoom out");
    d.offset += Vec2::new(5000.0, 0.0);
    h.click(&mut d, "fit");
    let b = d.scene.bounds().unwrap();
    let view = View { origin: Pos2::ZERO, offset: d.offset, zoom: d.zoom };
    assert!(view.to_screen(b.center()).x < 1400.0, "fit brings the drawing back into view");
}

// spec: DRAW-005
#[test]
fn undo_and_redo_buttons() {
    let (mut h, mut d) = setup();
    h.click(&mut d, "stroke:#e03131");
    assert_eq!(el(&d, "s1").stroke_color, "#e03131");
    h.click(&mut d, "undo");
    assert_ne!(el(&d, "s1").stroke_color, "#e03131", "undo button");
    h.click(&mut d, "redo");
    assert_eq!(el(&d, "s1").stroke_color, "#e03131", "redo button");
}

// spec: DRAW-005
#[test]
fn arrange_buttons() {
    let (mut h, mut d) = setup();
    d.selection.clear();
    h.settle(&mut d);
    let centre_gap = |d: &DrawingDoc| el(d, "s4").bounds().center().x - el(d, "s1").bounds().center().x;
    let before = centre_gap(&d);
    h.click(&mut d, "spread");
    assert!(centre_gap(&d) > before + 10.0, "spread out");
    let spread = centre_gap(&d);
    h.click(&mut d, "pull");
    assert!(centre_gap(&d) < spread - 10.0, "pull together");
    h.click(&mut d, "arrange");
    assert!(d.layout_msg.as_deref().unwrap_or("").starts_with("Arranged"), "auto arrange: {:?}", d.layout_msg);
}

// spec: DRAW-008, DRAW-009
#[test]
fn opening_a_drawing_changes_nothing_until_it_is_edited() {
    let mut h = H::new();
    // Shale's own format and an Obsidian plugin drawing (with an arrow bound to two shapes)
    for (name, text) in [("ui.excalidraw", SAMPLE_SCENE), ("Plan.excalidraw.md", include_str!("testdata/plugin-drawing.excalidraw.md"))] {
        let mut d = DrawingDoc::new(PathBuf::from(name), text);
        assert!(d.error.is_none(), "{name}: {:?}", d.error);
        h.settle(&mut d);
        assert!(!d.dirty, "{name}: just looking at it must not count as a change");
        // a real edit does, and the plugin drawing is saved in the plugin's layout
        let shape = d.scene.live().find(|e| e.kind == "rectangle").unwrap().id.clone();
        d.selection = vec![shape.clone()];
        h.settle(&mut d);
        h.click(&mut d, "stroke:#e03131");
        assert!(d.dirty, "{name}: an edit is a change");
        let saved = d.file_text().unwrap();
        let back = Scene::from_file_text(Path::new(name), &saved).unwrap();
        assert_eq!(back.live().find(|e| e.id == shape).unwrap().stroke_color, "#e03131");
        if name.ends_with(".md") {
            assert!(saved.starts_with("---\n\nexcalidraw-plugin: parsed") && saved.contains("## Text Elements\nrectangle ^"));
        }
    }
}
