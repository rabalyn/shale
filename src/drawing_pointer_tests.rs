//! The pointer on the drawing canvas (DRAW-019): real press, move and release events drive
//! `begin_drag`, `continue_drag`, `end_drag` and `click`; the tests look at the scene.
//! (A child module of `drawing_ui_tests`, which has the harness.)

use super::*;

impl H {
    fn button(p: Pos2, b: PointerButton, pressed: bool, modifiers: Modifiers) -> Event {
        Event::PointerButton { pos: p, button: b, pressed, modifiers }
    }

    /// Press the primary button at `from`, move in steps to `to` and let go there.
    fn drag(&mut self, d: &mut DrawingDoc, from: Pos2, to: Pos2) {
        let m = self.mods;
        self.frame(d, vec![Event::PointerMoved(from)]);
        self.frame(d, vec![Self::button(from, PointerButton::Primary, true, m)]);
        for k in 1..=6 {
            self.frame(d, vec![Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
        }
        self.frame(d, vec![Self::button(to, PointerButton::Primary, false, m)]);
        self.settle(d);
    }

    /// A double click at `p`: two quick clicks.
    fn double_click_at(&mut self, d: &mut DrawingDoc, p: Pos2) {
        for _ in 0..8 {
            self.frame(d, vec![]);
        }
        self.frame(d, vec![Event::PointerMoved(p)]);
        for _ in 0..2 {
            self.frame(d, vec![Self::button(p, PointerButton::Primary, true, self.mods)]);
            self.frame(d, vec![Self::button(p, PointerButton::Primary, false, self.mods)]);
        }
        self.settle(d);
    }

    /// The same by world points `(x, y)`, worked out where the drawing is now.
    fn drag_w(&mut self, d: &mut DrawingDoc, from: (f32, f32), to: (f32, f32)) {
        let (a, b) = (Self::at(d, from.0, from.1), Self::at(d, to.0, to.1));
        self.drag(d, a, b);
    }

    fn click_at_w(&mut self, d: &mut DrawingDoc, p: (f32, f32)) {
        let p = Self::at(d, p.0, p.1);
        self.click_at(d, p);
    }

    fn double_click_at_w(&mut self, d: &mut DrawingDoc, p: (f32, f32)) {
        let p = Self::at(d, p.0, p.1);
        self.double_click_at(d, p);
    }

    /// Where the world point (x, y) is on screen.
    fn at(d: &DrawingDoc, x: f32, y: f32) -> Pos2 {
        Self::screen(d, Pos2::new(x, y))
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 0.6
}

fn count(d: &DrawingDoc) -> usize {
    d.scene.live().count()
}

/// The sample drawing (rectangle s1 "Idea", arrow s3, ellipse s4 "Note") with nothing selected.
fn plain() -> (H, DrawingDoc) {
    let (mut h, mut d) = setup();
    d.selection.clear();
    h.settle(&mut d);
    (h, d)
}

/// A drawing of one scribble "f" and one free text "t", nothing selected.
fn scribble_and_text() -> (H, DrawingDoc) {
    let scene = r##"{"type":"excalidraw","version":2,"source":"shale","elements":[
      {"id":"f","type":"freedraw","x":100,"y":100,"width":100,"height":50,"points":[[0,0],[50,50],[100,0]],"strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":2,"strokeStyle":"solid","roughness":0,"opacity":100,"seed":1,"version":1,"versionNonce":1,"isDeleted":false,"angle":0},
      {"id":"t","type":"text","x":400,"y":100,"width":80,"height":25,"text":"Hello","originalText":"Hello","fontSize":20,"fontFamily":1,"textAlign":"left","verticalAlign":"top","strokeColor":"#1e1e1e","backgroundColor":"transparent","fillStyle":"solid","strokeWidth":1,"strokeStyle":"solid","roughness":0,"opacity":100,"seed":2,"version":1,"versionNonce":1,"isDeleted":false,"angle":0,"lineHeight":1.25,"containerId":null}
    ],"appState":{"viewBackgroundColor":"#ffffff"},"files":{}}"##;
    let mut h = H::new();
    let mut d = DrawingDoc::new(PathBuf::from("ui.excalidraw"), scene);
    h.settle(&mut d);
    (h, d)
}

fn tool(h: &mut H, d: &mut DrawingDoc, t: Tool) {
    d.tool = t;
    h.settle(d);
}

// spec: DRAW-019
#[test]
fn pointer_click_selects_toggles_and_clears() {
    let (mut h, mut d) = plain();
    let s1 = H::at(&d, 45.0, 90.0);
    let s4 = H::at(&d, 425.0, 95.0);
    let nothing = H::at(&d, 300.0, 300.0);
    h.click_at(&mut d, s1);
    assert_eq!(d.selection, ["s1"]);
    h.mods = Modifiers::SHIFT;
    h.click_at(&mut d, s4);
    assert_eq!(d.selection, ["s1", "s4"], "Shift adds");
    h.settle(&mut d);
    h.click_at(&mut d, nothing);
    assert_eq!(d.selection, ["s1", "s4"], "Shift on empty canvas keeps the selection");
    h.click_at(&mut d, s4);
    assert_eq!(d.selection, ["s1"], "Shift on a selected shape removes it");
    h.mods = Modifiers::NONE;
    h.click_at(&mut d, s4);
    assert_eq!(d.selection, ["s4"], "a plain click replaces the selection");
    h.click_at_w(&mut d, (340.0, 95.0));
    assert_eq!(d.selection, ["s3"], "the arrow is picked on its line");
    h.click_at(&mut d, nothing);
    assert!(d.selection.is_empty(), "empty canvas clears");
    assert!(!d.dirty, "selecting changes nothing");
}

// spec: DRAW-019
#[test]
fn pointer_drag_moves_the_selection_with_labels_and_connections() {
    let (mut h, mut d) = setup();
    let from = H::at(&d, 45.0, 90.0);
    h.drag(&mut d, from, from + Vec2::new(100.0, 50.0));
    assert!(near(el(&d, "s1").x, 140.0) && near(el(&d, "s1").y, 90.0), "moved by the drag: {} {}", el(&d, "s1").x, el(&d, "s1").y);
    let (l, s) = (el(&d, "s2").bounds().center().x, el(&d, "s1").bounds().center().x);
    assert!(near(l as f64, s as f64), "the label stays centred in its rectangle: {l} {s}");
    let arrow = el(&d, "s3");
    assert!(arrow.x > 350.0 && arrow.x < 380.0, "the arrow stays on the rectangle's edge: {}", arrow.x);
    assert!(d.dirty);
    d.undo();
    assert!(near(el(&d, "s1").x, 40.0), "one undo takes the whole drag back");
}

// spec: DRAW-019
#[test]
fn pointer_drag_after_zoom_moves_by_world_distance() {
    let (mut h, mut d) = setup();
    d.zoom = 2.0;
    h.settle(&mut d);
    let from = H::at(&d, 45.0, 90.0);
    h.drag(&mut d, from, from + Vec2::new(100.0, 0.0));
    assert!(near(el(&d, "s1").x, 90.0), "100 screen pixels at zoom 2 are 50 world units: {}", el(&d, "s1").x);
    // and a shape is still picked where it is on screen
    d.selection.clear();
    h.click_at_w(&mut d, (100.0, 95.0));
    assert_eq!(d.selection, ["s1"]);
}

// spec: DRAW-019
#[test]
fn pointer_drag_on_a_shape_selects_it_first_and_shift_extends() {
    let (mut h, mut d) = plain();
    let from = H::at(&d, 425.0, 95.0);
    h.drag(&mut d, from, from + Vec2::new(0.0, 60.0));
    assert_eq!(d.selection, ["s4"], "dragging an unselected shape selects it");
    assert!(near(el(&d, "s4").y, 100.0));
    assert!(near(el(&d, "s1").y, 40.0));
    h.mods = Modifiers::SHIFT;
    let a = H::at(&d, 45.0, 90.0);
    h.drag(&mut d, a, a + Vec2::new(0.0, 20.0));
    assert_eq!(d.selection, ["s4", "s1"], "Shift-drag adds to the selection");
    assert!(near(el(&d, "s4").y, 120.0) && near(el(&d, "s1").y, 60.0), "both move together");
}

// spec: DRAW-019
#[test]
fn pointer_drag_on_empty_canvas_selects_with_a_frame() {
    let (mut h, mut d) = setup();
    let from = H::at(&d, -10.0, -10.0);
    let to = H::at(&d, 275.0, 165.0);
    h.drag(&mut d, from, to);
    assert_eq!(d.selection, ["s1", "s2"], "the frame takes what it fully contains, not the older selection");
    assert!(d.marquee.is_none());
    h.mods = Modifiers::SHIFT;
    h.drag_w(&mut d, (400.0, 20.0), (600.0, 170.0));
    assert!(["s1", "s2", "s4", "s5"].iter().all(|id| d.selection.iter().any(|s| s == id)), "Shift keeps the old selection: {:?}", d.selection);
    h.mods = Modifiers::NONE;
    h.drag_w(&mut d, (300.0, 300.0), (350.0, 350.0));
    assert!(d.selection.is_empty(), "a frame around nothing clears the selection");
}

// spec: DRAW-019
#[test]
fn pointer_corner_handles_resize_a_shape() {
    let (mut h, mut d) = setup();
    // bottom right corner, 4 outside the shape
    h.drag_w(&mut d, (264.0, 154.0), (364.0, 204.0));
    let e = el(&d, "s1");
    assert!(near(e.x, 40.0) && near(e.y, 40.0) && near(e.width, 324.0) && near(e.height, 164.0), "{} {} {} {}", e.x, e.y, e.width, e.height);
    assert!(el(&d, "s2").x > 100.0, "the label is centred again");
    // the top left corner moves the origin and keeps the far corner
    let be = el(&d, "s1").bounds();
    h.drag_w(&mut d, (be.min.x - 4.0, be.min.y - 4.0), (0.0, 0.0));
    let e = el(&d, "s1");
    assert!(near(e.x, 0.0) && near(e.y, 0.0) && near(e.width + e.x, 364.0) && near(e.height + e.y, 204.0), "{} {} {} {}", e.x, e.y, e.width, e.height);
    // past the opposite side: the shape turns over and keeps a positive size
    let be = el(&d, "s1").bounds();
    h.drag_w(&mut d, (be.max.x + 4.0, be.max.y + 4.0), (-50.0, -30.0));
    let e = el(&d, "s1");
    assert!(e.width > 0.0 && e.height > 0.0 && near(e.x, -50.0) && near(e.y, -30.0), "{} {} {} {}", e.x, e.y, e.width, e.height);
    d.undo();
    assert!(near(el(&d, "s1").width, 364.0), "one undo takes back one resize");
}

// spec: DRAW-019
#[test]
fn pointer_corner_handles_resize_text_and_scribbles() {
    let (mut h, mut d) = scribble_and_text();
    // free text: the font grows with the height
    d.selection = vec!["t".into()];
    h.settle(&mut d);
    let b = el(&d, "t").bounds();
    let before = el(&d, "t").font_size.unwrap();
    h.drag_w(&mut d, (b.max.x + 4.0, b.max.y + 4.0), (b.max.x + 4.0, b.min.y + (b.height() * 2.0)));
    assert!(el(&d, "t").font_size.unwrap() > before * 1.5, "font {} -> {:?}", before, el(&d, "t").font_size);
    // a scribble: its points are scaled
    d.selection = vec!["f".into()];
    h.settle(&mut d);
    let b = el(&d, "f").bounds();
    h.drag_w(&mut d, (b.max.x + 4.0, b.max.y + 4.0), (b.max.x + 100.0, b.max.y + 4.0));
    let f = el(&d, "f");
    assert!(near(f.width, 200.0), "{}", f.width);
    assert!(near(f.points[2][0], 200.0), "the far point moved with the corner: {:?}", f.points);
    // and turned over by dragging the corner across
    let b = el(&d, "f").bounds();
    h.drag_w(&mut d, (b.max.x + 4.0, b.max.y + 4.0), (b.min.x - 40.0, b.min.y - 40.0));
    let f = el(&d, "f");
    assert!(f.width > 0.0 && f.points.iter().all(|p| p[0] >= -0.01 && p[1] >= -0.01), "{:?}", f.points);
}

// spec: DRAW-019
#[test]
fn pointer_ends_of_an_arrow_move_and_attach() {
    let (mut h, mut d) = plain();
    h.click_at_w(&mut d, (340.0, 95.0));
    assert_eq!(d.selection, ["s3"]);
    h.settle(&mut d);
    // the head leaves the ellipse: free end, no binding
    h.drag_w(&mut d, (420.0, 95.0), (500.0, 300.0));
    assert!(binding_of(el(&d, "s3"), "endBinding").is_none(), "{:?}", el(&d, "s3").extra.get("endBinding"));
    let a = el(&d, "s3");
    assert!(near(a.x + a.points[1][0], 500.0) && near(a.y + a.points[1][1], 300.0), "{} {} {:?}", a.x, a.y, a.points);
    assert!(binding_of(a, "startBinding").is_some(), "the other end stays attached");
    // and back onto the ellipse's left side
    h.drag_w(&mut d, (500.0, 300.0), (424.0, 97.0));
    assert_eq!(binding_of(el(&d, "s3"), "endBinding").map(|b| b.0), Some("s4".to_string()));
    // the tail with Shift is locked to 45 degree steps
    h.mods = Modifiers::SHIFT;
    h.drag_w(&mut d, (260.0, 95.0), (330.0, 190.0));
    let a = el(&d, "s3");
    let (s, e) = (Pos2::new((a.x + a.points[0][0]) as f32, (a.y + a.points[0][1]) as f32), Pos2::new((a.x + a.points[1][0]) as f32, (a.y + a.points[1][1]) as f32));
    let v = e - s;
    assert!(binding_of(a, "startBinding").is_none());
    assert!((v.x.abs() - v.y.abs()).abs() < 1.0 || v.x.abs() < 1.0 || v.y.abs() < 1.0, "a multiple of 45 degrees: {v:?}");
}

// spec: DRAW-019
#[test]
fn pointer_shape_tools_draw_by_dragging() {
    let (mut h, mut d) = plain();
    let n = count(&d);
    tool(&mut h, &mut d, Tool::Rectangle);
    h.drag_w(&mut d, (300.0, 200.0), (400.0, 260.0));
    assert_eq!(count(&d), n + 1);
    let r = d.scene.elements.last().unwrap();
    assert_eq!(r.kind, "rectangle");
    assert!(near(r.x, 300.0) && near(r.y, 200.0) && near(r.width, 100.0) && near(r.height, 60.0), "{} {} {} {}", r.x, r.y, r.width, r.height);
    assert_eq!(d.selection, [r.id.clone()]);
    assert_eq!(d.tool, Tool::Select, "the tool goes back to Select");
    // dragging up and to the left still gives a positive size, Shift makes it square
    tool(&mut h, &mut d, Tool::Ellipse);
    h.mods = Modifiers::SHIFT;
    h.drag_w(&mut d, (600.0, 400.0), (500.0, 370.0));
    let e = d.scene.elements.last().unwrap();
    assert_eq!(e.kind, "ellipse");
    assert!(near(e.width, 100.0) && near(e.height, 100.0) && near(e.x, 500.0) && near(e.y, 300.0), "{} {} {} {}", e.x, e.y, e.width, e.height);
    h.mods = Modifiers::NONE;
    // a locked tool stays
    h.click(&mut d, "lock");
    tool(&mut h, &mut d, Tool::Diamond);
    h.drag_w(&mut d, (300.0, 400.0), (340.0, 440.0));
    assert_eq!(d.scene.elements.last().unwrap().kind, "diamond");
    assert_eq!(d.tool, Tool::Diamond, "locked");
    h.drag_w(&mut d, (300.0, 500.0), (340.0, 540.0));
    assert_eq!(count(&d), n + 4);
}

// spec: DRAW-019
#[test]
fn pointer_a_click_or_tiny_drag_with_a_tool_drops_a_default_shape() {
    let (mut h, mut d) = plain();
    let n = count(&d);
    tool(&mut h, &mut d, Tool::Rectangle);
    h.click_at_w(&mut d, (300.0, 300.0));
    let r = d.scene.elements.last().unwrap();
    assert!(near(r.width, 100.0) && near(r.height, 70.0) && near(r.x, 250.0) && near(r.y, 265.0), "{} {} {} {}", r.x, r.y, r.width, r.height);
    assert_eq!(d.tool, Tool::Select);
    tool(&mut h, &mut d, Tool::Arrow);
    h.click_at_w(&mut d, (300.0, 500.0));
    let a = d.scene.elements.last().unwrap();
    assert!(a.kind == "arrow" && near(a.width, 100.0) && a.points.len() == 2, "{:?}", a.points);
    tool(&mut h, &mut d, Tool::Draw);
    h.click_at_w(&mut d, (600.0, 500.0));
    assert_eq!(d.scene.elements.last().unwrap().kind, "freedraw");
    assert_eq!(d.tool, Tool::Draw, "the pencil stays");
    assert_eq!(count(&d), n + 3);
    // a drag of a pixel or two is a click too
    tool(&mut h, &mut d, Tool::Ellipse);
    let p = H::at(&d, 700.0, 300.0);
    h.drag(&mut d, p, p + Vec2::new(1.0, 1.0));
    let e = d.scene.elements.last().unwrap();
    assert!(near(e.width, 100.0) && near(e.height, 70.0), "{} {}", e.width, e.height);
    tool(&mut h, &mut d, Tool::Line);
    let p = H::at(&d, 700.0, 600.0);
    h.drag(&mut d, p, p + Vec2::new(1.0, 1.0));
    let l = d.scene.elements.last().unwrap();
    assert!(l.kind == "line" && near(l.points[1][0], 100.0), "{:?}", l.points);
}

// spec: DRAW-019
#[test]
fn pointer_arrows_and_lines_snap_to_shapes_and_lock_to_45_degrees() {
    let (mut h, mut d) = plain();
    tool(&mut h, &mut d, Tool::Arrow);
    // from the middle of the rectangle's lower edge to the ellipse's lower edge
    h.drag_w(&mut d, (150.0, 152.0), (505.0, 152.0));
    let a = d.scene.elements.last().unwrap();
    assert_eq!(a.kind, "arrow");
    assert_eq!(binding_of(a, "startBinding").map(|b| b.0), Some("s1".to_string()));
    assert_eq!(binding_of(a, "endBinding").map(|b| b.0), Some("s4".to_string()));
    // a line with Shift: 11 degrees becomes 0
    tool(&mut h, &mut d, Tool::Line);
    h.mods = Modifiers::SHIFT;
    h.drag_w(&mut d, (300.0, 300.0), (400.0, 320.0));
    let l = d.scene.elements.last().unwrap();
    assert!(l.kind == "line" && l.height < 1.0 && l.width > 95.0, "{} {}", l.width, l.height);
    assert!(binding_of(l, "startBinding").is_none());
}

// spec: DRAW-019
#[test]
fn pointer_the_pencil_draws_a_scribble() {
    let (mut h, mut d) = plain();
    tool(&mut h, &mut d, Tool::Draw);
    let n = count(&d);
    h.drag_w(&mut d, (300.0, 300.0), (400.0, 360.0));
    assert_eq!(count(&d), n + 1);
    let f = d.scene.elements.last().unwrap();
    assert_eq!(f.kind, "freedraw");
    assert!(f.points.len() >= 4, "a point per step: {:?}", f.points);
    assert!(f.width > 90.0 && f.height > 50.0, "{} {}", f.width, f.height);
}

// spec: DRAW-019
#[test]
fn pointer_the_eraser_removes_what_it_passes_and_what_it_clicks() {
    let (mut h, mut d) = plain();
    tool(&mut h, &mut d, Tool::Eraser);
    h.drag_w(&mut d, (480.0, 95.0), (560.0, 95.0));
    assert!(!d.scene.live().any(|e| e.id == "s4"), "the ellipse is gone");
    assert!(d.scene.live().any(|e| e.id == "s1"));
    assert_eq!(d.tool, Tool::Eraser, "the eraser stays");
    h.click_at_w(&mut d, (45.0, 90.0));
    assert!(!d.scene.live().any(|e| e.id == "s1"), "a click erases too");
    d.undo();
    assert!(d.scene.live().any(|e| e.id == "s1"), "undo brings it back");
}

// spec: DRAW-019
#[test]
fn pointer_the_hand_and_the_space_bar_pan_the_view() {
    let (mut h, mut d) = plain();
    let before = d.scene.elements.iter().map(|e| (e.x, e.y)).collect::<Vec<_>>();
    tool(&mut h, &mut d, Tool::Hand);
    let o = d.offset;
    h.drag_w(&mut d, (300.0, 300.0), (400.0, 340.0));
    assert!(near(d.offset.x as f64, (o.x + 100.0) as f64) && near(d.offset.y as f64, (o.y + 40.0) as f64), "{:?} -> {:?}", o, d.offset);
    tool(&mut h, &mut d, Tool::Select);
    let o = d.offset;
    let key = |pressed| Event::Key { key: egui::Key::Space, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    h.frame(&mut d, vec![key(true)]);
    h.drag_w(&mut d, (45.0, 90.0), (25.0, 90.0));
    h.frame(&mut d, vec![key(false)]);
    assert!(d.offset.x < o.x - 10.0, "Space and drag pan: {:?} -> {:?}", o, d.offset);
    assert_eq!(before, d.scene.elements.iter().map(|e| (e.x, e.y)).collect::<Vec<_>>(), "panning moves nothing");
    assert!(!d.dirty);
}

// spec: DRAW-019
#[test]
fn pointer_double_clicks_edit_labels_and_texts() {
    let (mut h, mut d) = plain();
    h.double_click_at_w(&mut d, (45.0, 90.0));
    assert_eq!(d.editing_text.as_deref(), Some("s2"), "a double click on a shape edits its label");
    // a click elsewhere ends the editing
    h.click_at_w(&mut d, (300.0, 400.0));
    assert!(d.editing_text.is_none());
    // empty canvas: new text
    let n = count(&d);
    h.double_click_at_w(&mut d, (300.0, 250.0));
    assert_eq!(count(&d), n + 1);
    let id = d.editing_text.clone().expect("editing the new text");
    assert_eq!(d.selection, [id.clone()]);
    // dragging elsewhere ends it as well
    h.drag_w(&mut d, (300.0, 500.0), (350.0, 520.0));
    assert!(d.editing_text.is_none());
    // the text tool: a click makes text
    tool(&mut h, &mut d, Tool::Text);
    h.click_at_w(&mut d, (700.0, 500.0));
    assert!(d.editing_text.is_some());
    assert_eq!(d.tool, Tool::Select);
    // a double click on free text edits it, and so does the text tool
    let (mut h2, mut d2) = scribble_and_text();
    h2.double_click_at_w(&mut d2, (410.0, 112.0));
    assert_eq!(d2.editing_text.as_deref(), Some("t"), "a double click on free text edits it");
    h2.click_at_w(&mut d2, (300.0, 400.0));
    tool(&mut h2, &mut d2, Tool::Text);
    h2.click_at_w(&mut d2, (410.0, 112.0));
    assert_eq!(d2.editing_text.as_deref(), Some("t"), "the text tool on text edits it");
}
