//! UI tests for every button and control of the drawing editor: each test renders the real
//! editor with egui, moves the pointer onto a control, presses and releases the mouse button,
//! and checks that the drawing changed. (A child module of `drawing::connect`.)

use super::*;
use eframe::egui::{Event, Modifiers, PointerButton, RawInput};

struct H {
    ctx: egui::Context,
    t: f64,
    /// the vault the drawing lives in (for links and the `[[` / `[!` helpers)
    vault: crate::vault::Vault,
    /// what the drawing view asked the app to do
    actions: Vec<DrawingAction>,
    /// modifier keys held down
    mods: Modifiers,
}

impl H {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        H { ctx, t: 0.0, vault: crate::vault::Vault::default(), actions: vec![], mods: Modifiers::NONE }
    }

    fn frame(&mut self, d: &mut DrawingDoc, mut events: Vec<Event>) {
        self.t += 0.05;
        events.insert(0, Event::ModifiersChanged(self.mods));
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 1300.0))), time: Some(self.t), events, ..Default::default() };
        let vault = &self.vault;
        let mut asked = None;
        let mut out = self.ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                asked = d.ui(ui, true, vault);
            });
        });
        self.actions.extend(asked);
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

    /// Press a key (with modifiers) for one frame.
    fn key(&mut self, d: &mut DrawingDoc, key: egui::Key, modifiers: Modifiers) {
        self.frame(d, vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }]);
        self.frame(d, vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }]);
    }

    fn type_text(&mut self, d: &mut DrawingDoc, text: &str) {
        self.frame(d, vec![Event::Text(text.into())]);
        self.frame(d, vec![]);
    }

    /// Where the world point `p` is on screen right now.
    fn screen(d: &DrawingDoc, p: Pos2) -> Pos2 {
        View { origin: Self::rect(d, "canvas").min, offset: d.offset, zoom: d.zoom }.to_screen(p)
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

const CANVAS: &str = r#"{
  "nodes":[
    {"id":"a","type":"text","text":"devices","x":0,"y":0,"width":250,"height":60},
    {"id":"b","type":"file","file":"Ideas.md","x":0,"y":200,"width":250,"height":60},
    {"id":"c","type":"text","text":"camera","x":400,"y":200,"width":250,"height":60}
  ],
  "edges":[{"id":"e","fromNode":"a","fromSide":"bottom","toNode":"b","toSide":"top"}]
}"#;

// spec: DRAW-011
#[test]
fn a_canvas_is_shown_read_only() {
    let mut h = H::new();
    let mut d = DrawingDoc::new(PathBuf::from("Map.canvas"), CANVAS);
    assert!(d.error.is_none() && d.read_only.is_some());
    h.settle(&mut d);
    let before = d.scene.elements.clone();
    // no tools, styles or undo: only zoom and the way to an editable copy
    let names: Vec<&str> = d.ui_rects.iter().map(|r| r.0.as_str()).collect();
    assert!(names.contains(&"convert") && names.contains(&"zoom-in"), "{names:?}");
    assert!(!names.iter().any(|n| n.starts_with("tool") || n.starts_with("stroke:") || *n == "undo"), "{names:?}");
    // clicking, double-clicking and keys change nothing
    let card = H::screen(&d, Pos2::new(525.0, 230.0));
    h.click_at(&mut d, card);
    h.click_at(&mut d, card);
    for k in [egui::Key::R, egui::Key::Delete, egui::Key::Enter] {
        h.key(&mut d, k, Modifiers::NONE);
    }
    h.key(&mut d, egui::Key::A, Modifiers::COMMAND);
    h.key(&mut d, egui::Key::Delete, Modifiers::NONE);
    assert!(d.selection.is_empty() && d.editing_text.is_none());
    assert!(h.actions.is_empty(), "a card without a link does nothing");
    // dragging pans the view
    let offset = d.offset;
    let from = H::screen(&d, Pos2::new(300.0, 120.0));
    let btn = |p, pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
    h.frame(&mut d, vec![Event::PointerMoved(from), btn(from, true)]);
    for i in 1..6 {
        h.frame(&mut d, vec![Event::PointerMoved(from + Vec2::new(20.0 * i as f32, 0.0))]);
    }
    h.frame(&mut d, vec![btn(from + Vec2::new(100.0, 0.0), false)]);
    assert!(d.offset.x > offset.x + 20.0, "dragging pans: {:?} -> {:?}", offset, d.offset);
    // nothing changed, nothing is written
    assert_eq!(d.scene.elements.iter().map(|e| (&e.id, e.x, e.y, &e.text)).collect::<Vec<_>>(), before.iter().map(|e| (&e.id, e.x, e.y, &e.text)).collect::<Vec<_>>());
    assert!(!d.dirty && d.file_text().is_err());
    // the file card is a link: a plain click follows it here
    let file_card = H::screen(&d, Pos2::new(125.0, 230.0));
    h.click_at(&mut d, file_card);
    assert!(matches!(h.actions.as_slice(), [DrawingAction::Editor(crate::editor::EditorAction::OpenLink { target, new_tab: false, .. })] if target == "Ideas"), "file card follows its link");
    h.actions.clear();
    h.click(&mut d, "convert");
    assert!(matches!(h.actions.as_slice(), [DrawingAction::ConvertCanvas]));
}

/// A vault on disk (the sample vault) for the `[[` helpers, and the sample drawing in edit mode
/// on the label of the rectangle "Idea", with the cursor at the end.
fn editing_label(tag: &str) -> (H, DrawingDoc, PathBuf) {
    let dir = std::env::temp_dir().join(format!("shale-draw-links-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::vault::write_sample_vault(&dir).unwrap();
    let (mut h, mut d) = setup();
    h.vault = crate::vault::Vault::open(&dir).unwrap();
    // keys reach the drawing while the pointer is over it
    let over = H::rect(&d, "canvas").center();
    h.frame(&mut d, vec![Event::PointerMoved(over)]);
    h.key(&mut d, egui::Key::Enter, Modifiers::NONE);
    assert_eq!(d.editing_text.as_deref(), Some("s2"), "Enter edits the label");
    h.key(&mut d, egui::Key::End, Modifiers::COMMAND);
    (h, d, dir)
}

// spec: DRAW-014, DRAW-015
#[test]
fn double_brackets_in_a_drawing_suggest_notes_and_the_link_can_be_followed() {
    let (mut h, mut d, dir) = editing_label("notes");
    h.type_text(&mut d, " [[Ide");
    assert!(d.assist.ac_active, "typing [[ in a label opens the suggestions");
    h.key(&mut d, egui::Key::Enter, Modifiers::NONE);
    assert_eq!(d.text_buf, "Idea [[Ideas]]", "Enter picks the note and closes the link");
    assert!(d.editing_text.is_some(), "picking does not end the editing");
    h.key(&mut d, egui::Key::Escape, Modifiers::NONE);
    assert!(d.editing_text.is_none());
    assert_eq!(el(&d, "s2").text.as_deref(), Some("Idea [[Ideas]]"));
    h.settle(&mut d);
    // the rectangle now carries a link mark; clicking it follows the link (Shift: new tab)
    let mark = d.links_on_screen(&View { origin: H::rect(&d, "canvas").min, offset: d.offset, zoom: d.zoom }).into_iter().find(|l| l.0 == "s1").expect("a link mark on the shape").2;
    h.mods = Modifiers::SHIFT;
    h.click_at(&mut d, mark.center());
    h.mods = Modifiers::NONE;
    assert!(matches!(h.actions.as_slice(), [DrawingAction::Editor(crate::editor::EditorAction::OpenLink { target, new_tab: true, .. })] if target == "Ideas"), "the mark follows the link");
    // Ctrl+click on the shape follows it too; a plain click only selects
    h.actions.clear();
    let centre = H::screen(&d, el(&d, "s1").bounds().center());
    h.click_at(&mut d, centre);
    assert!(h.actions.is_empty() && d.selection == vec!["s1".to_string()]);
    h.mods = Modifiers::COMMAND;
    h.click_at(&mut d, centre);
    h.mods = Modifiers::NONE;
    assert!(matches!(h.actions.as_slice(), [DrawingAction::Editor(crate::editor::EditorAction::OpenLink { target, .. })] if target == "Ideas"));
    let _ = std::fs::remove_dir_all(dir);
}

// spec: DRAW-014
#[test]
fn people_and_the_file_picker_in_a_drawing() {
    let (mut h, mut d, dir) = editing_label("people");
    h.type_text(&mut d, " [[p Anna");
    assert!(d.assist.ac_labels.iter().any(|l| l.contains("New contact “Anna”")), "{:?}", d.assist.ac_labels);
    h.key(&mut d, egui::Key::Enter, Modifiers::NONE);
    assert_eq!(d.text_buf, "Idea [[Anna]]");
    assert!(matches!(h.actions.as_slice(), [DrawingAction::Editor(crate::editor::EditorAction::CreatePerson(n))] if n == "Anna"));
    // `[!` opens the file picker; Esc closes it and keeps editing
    h.type_text(&mut d, " [!");
    assert!(d.assist.picker_visible(), "[! opens the file picker");
    h.key(&mut d, egui::Key::Escape, Modifiers::NONE);
    assert!(!d.assist.picker_visible());
    assert!(d.editing_text.is_some(), "the first Esc only closes the picker");
    let _ = std::fs::remove_dir_all(dir);
}
