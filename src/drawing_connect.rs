//! Drawing editor extras (a child module of `drawing`, so it can use the editor's internals):
//! - centred labels inside shapes (Excalidraw "bound text": a text element with `containerId`),
//! - connections that snap to a shape's anchor points and stay attached when the shape moves
//!   (`startBinding` / `endBinding` with a `fixedPoint`, plus the shape's `boundElements`),
//! - line endings (arrowheads), extra dash patterns and fonts,
//! - the style panel and the arrange / spread buttons.

use super::*;
use crate::drawing_layout::{self as lay, anchor_point, anchor_ratios, nearest_anchor};
use eframe::egui::text::LayoutJob;
use eframe::egui::{Align, FontFamily};
use std::collections::HashMap;

/// Excalidraw font ids: 1 Virgil (hand-drawn), 5 Excalifont (hand-drawn), 2 normal, 3 code.
pub fn font_id_for(family: u64, size: f32) -> FontId {
    match family {
        2 | 6 => FontId::proportional(size),
        3 | 8 => FontId::monospace(size),
        _ => FontId::new(size, FontFamily::Name(crate::theme::HAND_FONT.into())),
    }
}

pub fn font_family_of(e: &Element) -> u64 {
    e.extra.get("fontFamily").and_then(|v| v.as_u64()).unwrap_or(1)
}

pub fn text_align_of(e: &Element) -> Align {
    match e.extra.get("textAlign").and_then(|v| v.as_str()) {
        Some("center") => Align::Center,
        Some("right") => Align::RIGHT,
        _ => Align::LEFT,
    }
}

pub fn container_of(e: &Element) -> Option<String> {
    e.extra.get("containerId").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Shapes that can hold a label and take connections.
pub fn is_shape(e: &Element) -> bool {
    matches!(e.kind.as_str(), "rectangle" | "diamond" | "ellipse")
}

/// Elements connections can attach to: shapes and free-standing text.
pub fn is_bindable(e: &Element) -> bool {
    is_shape(e) || (e.kind == "text" && container_of(e).is_none())
}

/// Text layout used for both measuring and painting. `wrap` in screen points.
pub fn text_job(e: &Element, zoom: f32, color: Color32, wrap: Option<f32>) -> LayoutJob {
    let size = (e.font_size.unwrap_or(20.0) as f32 * zoom).max(1.0);
    let mut job = LayoutJob::simple(e.text.clone().unwrap_or_default(), font_id_for(font_family_of(e), size), color, wrap.unwrap_or(f32::INFINITY));
    job.halign = text_align_of(e);
    job
}

/// (shape id, anchor fraction) of a connection end.
pub fn binding_of(e: &Element, key: &str) -> Option<(String, Option<[f32; 2]>)> {
    let b = e.extra.get(key)?;
    let id = b.get("elementId")?.as_str()?.to_string();
    let fp = b.get("fixedPoint").and_then(|v| v.as_array()).and_then(|a| Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32]));
    Some((id, fp))
}

/// Paint a line ending. Excalidraw names: arrow, triangle, triangle_outline, bar, dot, circle,
/// circle_outline, diamond, diamond_outline.
#[allow(clippy::too_many_arguments)]
pub fn paint_head(kind: &str, tip: Pos2, from: Pos2, stroke_width: f32, stroke: Stroke, view: &View, out: &mut Vec<Shape>) {
    let d = tip - from;
    let len = d.length();
    if len < 0.1 {
        return;
    }
    let dir = d / len;
    let perp = Vec2::new(-dir.y, dir.x);
    let size = (len * 0.45).min(14.0 + stroke_width * 3.0);
    let s = |p: Pos2| view.to_screen(p);
    let fill = stroke.color;
    match kind {
        "triangle" | "triangle_outline" => {
            let back = tip - dir * size;
            let pts = vec![s(tip), s(back + perp * size * 0.5), s(back - perp * size * 0.5)];
            if kind == "triangle" {
                out.push(Shape::convex_polygon(pts, fill, stroke));
            } else {
                out.push(Shape::closed_line(pts, stroke));
            }
        }
        "bar" => {
            out.push(Shape::line_segment([s(tip + perp * size * 0.5), s(tip - perp * size * 0.5)], stroke));
        }
        "dot" | "circle" | "circle_outline" => {
            let r = (size * 0.3).max(stroke_width * 1.5);
            let c = s(tip - dir * r);
            if kind == "circle_outline" {
                out.push(Shape::circle_stroke(c, r * view.zoom, stroke));
            } else {
                out.push(Shape::circle_filled(c, r * view.zoom, fill));
            }
        }
        "diamond" | "diamond_outline" => {
            let mid = tip - dir * size * 0.5;
            let pts = vec![s(tip), s(mid + perp * size * 0.3), s(tip - dir * size), s(mid - perp * size * 0.3)];
            if kind == "diamond" {
                out.push(Shape::convex_polygon(pts, fill, stroke));
            } else {
                out.push(Shape::closed_line(pts, stroke));
            }
        }
        _ => {
            // "arrow": open arrowhead
            for a in [0.45f32, -0.45] {
                let (sn, cs) = a.sin_cos();
                let back = Vec2::new(-dir.x * cs + dir.y * sn, -dir.x * sn - dir.y * cs) * size;
                out.push(Shape::line_segment([s(tip), s(tip + back)], stroke));
            }
        }
    }
}

/// All line endings offered in the panel: (stored value, label).
pub const HEADS: [(Option<&str>, &str); 9] = [
    (None, "None"),
    (Some("arrow"), "Arrow (open)"),
    (Some("triangle"), "Triangle (filled)"),
    (Some("triangle_outline"), "Triangle (outline)"),
    (Some("bar"), "Bar"),
    (Some("circle"), "Dot (filled)"),
    (Some("circle_outline"), "Circle (outline)"),
    (Some("diamond"), "Diamond (filled)"),
    (Some("diamond_outline"), "Diamond (outline)"),
];

/// Stroke patterns offered in the panel.
pub const STROKE_STYLES: [(&str, &str); 6] = [("solid", "Solid"), ("dashed", "Dashed"), ("dotted", "Dotted"), ("dashdot", "Dash-dot"), ("longdash", "Long dash"), ("sparsedots", "Sparse dots")];

enum Change {
    Stroke(String),
    Bg(String),
    Fill(String),
    Width(f64),
    StrokeStyle(String),
    Rough(f64),
    Opacity(f64),
    Round(bool),
    Font(u64),
    FontSize(f64),
    TextColor(String),
    LabelBg(String),
    Align(&'static str),
    StartHead(Option<String>),
    EndHead(Option<String>),
}

impl DrawingDoc {
    /// Short state summary (testing aid).
    pub fn debug_state(&self) -> String {
        let drag = match &self.drag {
            Drag::None => "none",
            Drag::Create { .. } => "create",
            Drag::Endpoint { .. } => "endpoint",
            Drag::Move { .. } => "move",
            Drag::Resize { .. } => "resize",
            Drag::Marquee { .. } => "marquee",
            Drag::Pan => "pan",
            _ => "other",
        };
        let lines: Vec<String> = self
            .scene
            .live()
            .filter(|e| e.is_linear())
            .map(|e| format!("{}->{}", binding_of(e, "startBinding").map(|b| b.0).unwrap_or("-".into()), binding_of(e, "endBinding").map(|b| b.0).unwrap_or("-".into())))
            .collect();
        format!("tool={:?} drag={drag} elements={} selected={} lines={lines:?}", self.tool, self.scene.live().count(), self.selection.len())
    }

    // ------------------------------------------------------------------ labels

    pub(super) fn label_of(&self, container: &str) -> Option<String> {
        self.scene.live().find(|e| e.kind == "text" && container_of(e).as_deref() == Some(container)).map(|e| e.id.clone())
    }

    /// Keep labels centred in their shapes (and remove labels whose shape is gone).
    pub(super) fn sync_labels(&mut self, ctx: &egui::Context) {
        let shapes: HashMap<String, Rect> = self.scene.live().filter(|e| is_shape(e)).map(|e| (e.id.clone(), e.bounds())).collect();
        let editing = self.editing_text.clone();
        for e in self.scene.elements.iter_mut().filter(|e| !e.is_deleted && e.kind == "text") {
            let Some(cid) = container_of(e) else { continue };
            let Some(r) = shapes.get(&cid) else {
                if editing.as_deref() != Some(e.id.as_str()) {
                    e.is_deleted = true;
                }
                continue;
            };
            let wrap = (r.width() - 16.0).max(20.0);
            let job = text_job(e, 1.0, Color32::WHITE, Some(wrap));
            let size = ctx.fonts_mut(|f| f.layout_job(job)).size();
            let (w, h) = (size.x as f64, size.y as f64);
            let x = r.center().x as f64 - w / 2.0;
            let y = r.center().y as f64 - h / 2.0;
            if (e.x - x).abs() > 0.01 || (e.y - y).abs() > 0.01 || (e.width - w).abs() > 0.01 || (e.height - h).abs() > 0.01 {
                e.x = x;
                e.y = y;
                e.width = w;
                e.height = h;
            }
        }
    }

    /// Start editing the label of a shape (creating it if needed).
    pub(super) fn edit_label(&mut self, shape_id: &str) {
        self.snapshot();
        let id = match self.label_of(shape_id) {
            Some(id) => id,
            None => {
                let c = self.scene.elements.iter().find(|e| e.id == shape_id).map(|e| e.bounds().center()).unwrap_or(Pos2::ZERO);
                let mut t = self.new_element("text", c);
                t.text = Some(String::new());
                t.stroke_color = self.style.text_color.clone();
                t.background_color = self.style.label_bg.clone();
                t.extra.insert("containerId".into(), json!(shape_id));
                t.extra.insert("textAlign".into(), json!("center"));
                t.extra.insert("verticalAlign".into(), json!("middle"));
                let tid = t.id.clone();
                self.scene.elements.push(t);
                if let Some(s) = self.scene.elements.iter_mut().find(|e| e.id == shape_id) {
                    push_bound(s, &tid, "text");
                }
                tid
            }
        };
        self.text_buf = self.scene.elements.iter().find(|e| e.id == id).and_then(|e| e.text.clone()).unwrap_or_default();
        self.editing_text = Some(id);
        self.selection = vec![shape_id.to_string()];
    }

    // ------------------------------------------------------------------ connections

    /// The anchor of a shape near `p` that a connection end would snap to.
    pub(super) fn find_snap(&self, p: Pos2, exclude: Option<&str>) -> Option<(String, [f32; 2], Pos2)> {
        let reach = 22.0 / self.zoom;
        let mut best: Option<(String, [f32; 2], Pos2, f32, bool)> = None;
        for e in self.scene.live().filter(|e| is_bindable(e)) {
            if Some(e.id.as_str()) == exclude {
                continue;
            }
            let r = e.bounds();
            if !r.expand(reach).contains(p) {
                continue;
            }
            let inside = hit(e, p, 0.0) || r.contains(p);
            let (i, q, d) = nearest_anchor(&e.kind, r, p);
            if !inside && d > reach * 1.5 {
                continue;
            }
            let better = match &best {
                None => true,
                // the topmost shape the pointer is inside wins; otherwise the closest anchor
                Some(b) => (inside && !b.4) || (inside == b.4 && d < b.3),
            };
            if better {
                best = Some((e.id.clone(), anchor_ratios(&e.kind)[i], q, d, inside));
            }
        }
        best.map(|b| (b.0, b.1, b.2))
    }

    /// Attach (or detach with None) one end of a line/arrow to a shape anchor.
    pub(super) fn set_binding(&mut self, line_id: &str, start: bool, target: Option<(String, [f32; 2])>) {
        let key = if start { "startBinding" } else { "endBinding" };
        let other_key = if start { "endBinding" } else { "startBinding" };
        let (old, other) = match self.scene.elements.iter().find(|e| e.id == line_id) {
            Some(e) => (binding_of(e, key).map(|b| b.0), binding_of(e, other_key).map(|b| b.0)),
            None => return,
        };
        if let Some(old) = old {
            if other.as_deref() != Some(old.as_str()) && target.as_ref().map(|t| t.0 != old).unwrap_or(true) {
                if let Some(s) = self.scene.elements.iter_mut().find(|e| e.id == old) {
                    remove_bound(s, line_id);
                }
            }
        }
        let value = match &target {
            Some((sid, fp)) => json!({"elementId": sid, "focus": 0.0, "gap": 0.0, "fixedPoint": [fp[0], fp[1]]}),
            None => Value::Null,
        };
        if let Some(l) = self.scene.elements.iter_mut().find(|e| e.id == line_id) {
            l.extra.insert(key.into(), value);
        }
        if let Some((sid, _)) = target {
            if let Some(s) = self.scene.elements.iter_mut().find(|e| e.id == sid) {
                push_bound(s, line_id, "arrow");
            }
        }
    }

    /// Move connection ends to their shapes' anchors. Returns true if anything moved.
    pub(super) fn update_bindings(&mut self) -> bool {
        let shapes: HashMap<String, (String, Rect)> = self.scene.live().filter(|e| is_bindable(e)).map(|e| (e.id.clone(), (e.kind.clone(), e.bounds()))).collect();
        let mut any = false;
        for e in self.scene.elements.iter_mut().filter(|e| !e.is_deleted && matches!(e.kind.as_str(), "line" | "arrow")) {
            if e.points.len() < 2 {
                continue;
            }
            let mut abs: Vec<Pos2> = e.points.iter().map(|p| Pos2::new((e.x + p[0]) as f32, (e.y + p[1]) as f32)).collect();
            let last = abs.len() - 1;
            let mut moved = false;
            for (key, idx) in [("startBinding", 0usize), ("endBinding", last)] {
                let Some((sid, fp)) = binding_of(e, key) else { continue };
                match (shapes.get(&sid), fp) {
                    (Some((_, r)), Some(fp)) => {
                        let p = anchor_point(*r, fp);
                        if (abs[idx] - p).length() > 0.01 {
                            abs[idx] = p;
                            moved = true;
                        }
                    }
                    (None, _) => {
                        // the shape was removed: the end stays where it is, unattached
                        e.extra.insert(key.into(), Value::Null);
                        moved = true;
                    }
                    _ => {}
                }
            }
            if moved {
                set_abs_points(e, &abs);
                any = true;
            }
        }
        any
    }

    /// After shapes moved: a connection that now runs through one of its own shapes switches to
    /// the pair of anchors that face each other. Connections that still look fine keep the
    /// anchors the person picked.
    pub(super) fn tidy_connections(&mut self, moved: &[String]) {
        let shapes: HashMap<String, (String, Rect)> = self.scene.live().filter(|e| is_bindable(e)).map(|e| (e.id.clone(), (e.kind.clone(), e.bounds()))).collect();
        let mut fixes: Vec<(String, String, [f32; 2], String, [f32; 2])> = Vec::new();
        for e in self.scene.live().filter(|e| e.is_linear() && e.points.len() == 2) {
            let (Some((a, Some(fa))), Some((b, Some(fb)))) = (binding_of(e, "startBinding"), binding_of(e, "endBinding")) else { continue };
            if !moved.contains(&a) && !moved.contains(&b) {
                continue;
            }
            let (Some((ka, ra)), Some((kb, rb))) = (shapes.get(&a), shapes.get(&b)) else { continue };
            let (p, q) = (anchor_point(*ra, fa), anchor_point(*rb, fb));
            let through = (1..10).map(|i| p + (q - p) * (i as f32 / 10.0)).any(|m| ra.shrink(3.0).contains(m) || rb.shrink(3.0).contains(m));
            if through {
                let (i, j) = lay::best_anchor_pair(ka, *ra, kb, *rb);
                fixes.push((e.id.clone(), a.clone(), anchor_ratios(ka)[i], b.clone(), anchor_ratios(kb)[j]));
            }
        }
        for (line, a, fa, b, fb) in fixes {
            self.set_binding(&line, true, Some((a, fa)));
            self.set_binding(&line, false, Some((b, fb)));
        }
        self.update_bindings();
    }

    /// When lines are dragged on their own, their ends let go of shapes that stay put.
    pub(super) fn detach_unselected(&mut self) {
        let sel = self.selection.clone();
        let lines: Vec<(String, Option<String>, Option<String>)> = self
            .scene
            .live()
            .filter(|e| sel.contains(&e.id) && e.is_linear())
            .map(|e| (e.id.clone(), binding_of(e, "startBinding").map(|b| b.0), binding_of(e, "endBinding").map(|b| b.0)))
            .collect();
        for (id, s, en) in lines {
            if s.map(|s| !sel.contains(&s)).unwrap_or(false) {
                self.set_binding(&id, true, None);
            }
            if en.map(|s| !sel.contains(&s)).unwrap_or(false) {
                self.set_binding(&id, false, None);
            }
        }
    }

    // ------------------------------------------------------------------ arrange / spread

    /// Shapes to arrange (the selected ones, or all) and the connections between them.
    fn layout_input(&self) -> (Vec<String>, Vec<(String, Rect)>, Vec<(usize, usize, String)>) {
        let sel_shapes: Vec<&Element> = self.scene.live().filter(|e| is_bindable(e) && self.selection.contains(&e.id)).collect();
        let pool: Vec<&Element> = if sel_shapes.len() >= 2 { sel_shapes } else { self.scene.live().filter(|e| is_bindable(e)).collect() };
        let ids: Vec<String> = pool.iter().map(|e| e.id.clone()).collect();
        let nodes: Vec<(String, Rect)> = pool.iter().map(|e| (e.kind.clone(), e.bounds())).collect();
        let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, s)| (s.as_str(), i)).collect();
        let mut edges = Vec::new();
        for e in self.scene.live().filter(|e| e.is_linear()) {
            if let (Some((a, _)), Some((b, _))) = (binding_of(e, "startBinding"), binding_of(e, "endBinding")) {
                if let (Some(&ia), Some(&ib)) = (index.get(a.as_str()), index.get(b.as_str())) {
                    if ia != ib {
                        edges.push((ia, ib, e.id.clone()));
                    }
                }
            }
        }
        (ids, nodes, edges)
    }

    /// Move shapes to new rects and re-pick the best anchors for their connections.
    fn apply_rects(&mut self, ids: &[String], nodes: &[(String, Rect)], rects: &[Rect], edges: &[(usize, usize, String)]) {
        for (id, r) in ids.iter().zip(rects) {
            if let Some(e) = self.scene.elements.iter_mut().find(|e| e.id == *id) {
                e.x = r.min.x as f64;
                e.y = r.min.y as f64;
            }
        }
        for (a, b, line) in edges {
            let (i, j) = lay::best_anchor_pair(&nodes[*a].0, rects[*a], &nodes[*b].0, rects[*b]);
            self.set_binding(line, true, Some((ids[*a].clone(), anchor_ratios(&nodes[*a].0)[i])));
            self.set_binding(line, false, Some((ids[*b].clone(), anchor_ratios(&nodes[*b].0)[j])));
        }
        self.update_bindings();
        let all: Vec<String> = ids.to_vec();
        self.touch_ids(&all);
        self.changed();
    }

    pub(super) fn auto_arrange(&mut self) {
        let (ids, nodes, edges) = self.layout_input();
        let pairs: Vec<(usize, usize)> = edges.iter().map(|e| (e.0, e.1)).collect();
        match lay::auto_layout(&nodes, &pairs) {
            None => self.layout_msg = Some("Nothing to arrange: add at least two shapes".into()),
            Some(res) if res.crossings_after > res.crossings_before => {
                self.layout_msg = Some(format!("Kept the current layout: it has fewer crossings ({}) than any arrangement found", res.crossings_before));
            }
            Some(res) => {
                self.snapshot();
                self.apply_rects(&ids, &nodes, &res.rects, &edges);
                self.layout_msg = Some(format!(
                    "Arranged {} shapes · crossing connections {} → {} · stopped after {} step{}",
                    ids.len(),
                    res.crossings_before,
                    res.crossings_after,
                    res.sweeps,
                    if res.sweeps == 1 { "" } else { "s" }
                ));
            }
        }
    }

    pub(super) fn spread_shapes(&mut self, factor: f32) {
        let (ids, nodes, edges) = self.layout_input();
        let pairs: Vec<(usize, usize)> = edges.iter().map(|e| (e.0, e.1)).collect();
        match lay::spread(&nodes, &pairs, factor) {
            lay::SpreadOutcome::Moved(rects, f) => {
                self.snapshot();
                self.apply_rects(&ids, &nodes, &rects, &edges);
                let pct = ((f - 1.0) * 100.0).round();
                self.layout_msg = Some(if (f - factor).abs() < 1e-3 {
                    format!("{} {} shapes by {:.0}%", if factor > 1.0 { "Spread" } else { "Pulled together" }, ids.len(), pct.abs())
                } else {
                    format!("Moved only {:.0}% so shapes don't overlap and connections don't cross", pct.abs())
                });
            }
            lay::SpreadOutcome::Blocked(msg) => self.layout_msg = Some(msg.into()),
        }
    }

    pub(super) fn arrange_bar(&mut self, ui: &mut Ui, canvas: Rect, dark: bool) {
        let w = 430.0;
        let r = Rect::from_min_size(Pos2::new(canvas.max.x - w - 10.0, canvas.max.y - 54.0), Vec2::new(w, 46.0));
        Self::blocker(ui, r, "arrange-block");
        let mut child = ui.new_child(UiBuilder::new().max_rect(r).layout(egui::Layout::right_to_left(egui::Align::Center)));
        let mut act: Option<u8> = None;
        let scope = if self.scene.live().filter(|e| is_bindable(e) && self.selection.contains(&e.id)).count() >= 2 { "the selected shapes" } else { "all shapes" };
        let mut rects: Vec<(String, Rect)> = Vec::new();
        Self::panel_frame(dark).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                let b = ui.button("Auto arrange");
                rects.push(("arrange".into(), b.rect));
                if b.on_hover_text(format!("Arrange {scope} along their connections with as few crossing connections as possible")).clicked() {
                    act = Some(0);
                }
                let b = ui.button("Spread out");
                rects.push(("spread".into(), b.rect));
                if b.on_hover_text(format!("Push {scope} further apart (connections stay attached)")).clicked() {
                    act = Some(1);
                }
                let b = ui.button("Pull together");
                rects.push(("pull".into(), b.rect));
                if b.on_hover_text(format!("Pull {scope} closer, without overlapping shapes or crossing connections")).clicked() {
                    act = Some(2);
                }
            });
        });
        self.ui_rects.extend(rects);
        match act {
            Some(0) => self.auto_arrange(),
            Some(1) => self.spread_shapes(1.25),
            Some(2) => self.spread_shapes(0.8),
            _ => {}
        }
        if let Some(msg) = &self.layout_msg {
            let fg = if dark { Color32::from_gray(200) } else { Color32::from_gray(70) };
            ui.painter().text(Pos2::new(canvas.max.x - 14.0, canvas.max.y - 62.0), Align2::RIGHT_BOTTOM, msg, FontId::proportional(12.0), fg);
        }
    }

    // ------------------------------------------------------------------ style panel

    pub(super) fn properties(&mut self, ui: &mut Ui, canvas: Rect, dark: bool) {
        let tool_kind = matches!(self.tool, Tool::Rectangle | Tool::Diamond | Tool::Ellipse | Tool::Arrow | Tool::Line | Tool::Draw | Tool::Text);
        if (self.selection.is_empty() && !tool_kind) || self.editing_text.is_some() {
            self.props_rect = None;
            return;
        }
        let first = self.scene.live().find(|e| self.selection.contains(&e.id)).cloned();
        let kind = match &first {
            Some(e) => e.kind.clone(),
            None => match self.tool {
                Tool::Text => "text".into(),
                t => t.element_kind().into(),
            },
        };
        let shape = matches!(kind.as_str(), "rectangle" | "diamond" | "ellipse");
        let linear = matches!(kind.as_str(), "line" | "arrow");
        let text_el = kind == "text";
        let label: Option<Element> = match &first {
            Some(e) if text_el => Some(e.clone()),
            Some(e) => self.label_of(&e.id).and_then(|id| self.scene.elements.iter().find(|x| x.id == id).cloned()),
            None => None,
        };
        let st = self.style.clone();
        let stroke = first.as_ref().map(|e| e.stroke_color.clone()).unwrap_or(st.stroke.clone());
        let bg = first.as_ref().map(|e| e.background_color.clone()).unwrap_or(st.background.clone());
        let fill = first.as_ref().map(|e| e.fill_style.clone()).unwrap_or(st.fill.clone());
        let width = first.as_ref().map(|e| e.stroke_width).unwrap_or(st.width);
        let sstyle = first.as_ref().map(|e| e.stroke_style.clone()).unwrap_or(st.stroke_style.clone());
        let rough = first.as_ref().map(|e| e.roughness).unwrap_or(st.roughness);
        let opacity = first.as_ref().map(|e| e.opacity).unwrap_or(st.opacity);
        let round = first.as_ref().map(|e| e.has_roundness()).unwrap_or(st.round);
        let font = label.as_ref().map(font_family_of).unwrap_or(st.font_family);
        let fsize = label.as_ref().and_then(|e| e.font_size).unwrap_or(st.font_size);
        let tcolor = label.as_ref().map(|e| e.stroke_color.clone()).unwrap_or(st.text_color.clone());
        let lbg = label.as_ref().map(|e| e.background_color.clone()).unwrap_or(st.label_bg.clone());
        let align = label.as_ref().and_then(|e| e.extra.get("textAlign").and_then(|v| v.as_str()).map(|s| s.to_string())).unwrap_or(if text_el && first.as_ref().map(|e| container_of(e).is_none()).unwrap_or(true) { "left".into() } else { "center".into() });
        let start_head = first.as_ref().map(|e| e.arrowhead("startArrowhead")).unwrap_or(st.start_head.clone());
        let end_head = first.as_ref().map(|e| e.arrowhead("endArrowhead")).unwrap_or(if kind == "arrow" { st.end_head.clone() } else { None });

        let fg = if dark { Color32::from_gray(200) } else { Color32::from_gray(60) };
        let mut changes: Vec<Change> = Vec::new();
        let mut action: Option<&str> = None;
        let r = Rect::from_min_size(canvas.min + Vec2::new(10.0, 72.0), Vec2::new(236.0, (canvas.height() - 150.0).max(120.0)));
        // Stop clicks on the panel from reaching the canvas. This has to be registered BEFORE the
        // panel's own buttons (egui gives a click to the widget added last), so it uses the
        // panel's size from the previous frame.
        if let Some(prev) = self.props_rect {
            Self::blocker(ui, prev, "props-block");
        }
        let rects: std::cell::RefCell<Vec<(String, Rect)>> = Default::default();
        let rec = |key: String, resp: egui::Response| -> egui::Response {
            rects.borrow_mut().push((key, resp.rect));
            resp
        };
        let mut child = ui.new_child(UiBuilder::new().max_rect(r));
        let inner = Self::panel_frame(dark).show(&mut child, |ui| {
            ui.set_width(220.0);
            egui::ScrollArea::vertical().max_height(r.height() - 16.0).auto_shrink([false, true]).show(ui, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                let label_ui = |ui: &mut Ui, t: &str| {
                    ui.label(egui::RichText::new(t).size(11.0).color(fg));
                };
                let swatches = |ui: &mut Ui, colors: &[&str], current: &str, key: &str| -> Option<String> {
                    let mut picked = None;
                    ui.horizontal(|ui| {
                        for c in colors {
                            let (sr, sresp) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                            match parse_color(c).map(|c| if dark { dark_variant(c) } else { c }) {
                                Some(col) => {
                                    ui.painter().rect_filled(sr.shrink(3.0), CornerRadius::same(4), col);
                                }
                                None => {
                                    ui.painter().rect_stroke(sr.shrink(3.0), CornerRadius::same(4), Stroke::new(1.0, fg.gamma_multiply(0.5)), StrokeKind::Inside);
                                    ui.painter().line_segment([sr.shrink(5.0).left_bottom(), sr.shrink(5.0).right_top()], Stroke::new(1.0, Color32::from_rgb(220, 80, 80)));
                                }
                            }
                            if current.eq_ignore_ascii_case(c) {
                                ui.painter().rect_stroke(sr.shrink(0.5), CornerRadius::same(5), Stroke::new(1.5, ACCENT), StrokeKind::Inside);
                            }
                            if rec(format!("{key}:{c}"), sresp.on_hover_text(*c)).clicked() {
                                picked = Some(c.to_string());
                            }
                        }
                    });
                    picked
                };
                if !text_el {
                    label_ui(ui, "Stroke color");
                    if let Some(c) = swatches(ui, &STROKE_COLORS, &stroke, "stroke") {
                        changes.push(Change::Stroke(c));
                    }
                }
                if shape {
                    label_ui(ui, "Shape background");
                    if let Some(c) = swatches(ui, &BG_COLORS, &bg, "bg") {
                        changes.push(Change::Bg(c));
                    }
                    label_ui(ui, "Fill");
                    ui.horizontal(|ui| {
                        for (v, t) in [("hachure", "Hatch"), ("cross-hatch", "Cross"), ("solid", "Solid")] {
                            if rec(format!("fill:{t}"), ui.selectable_label(fill == v, t)).clicked() {
                                changes.push(Change::Fill(v.into()));
                            }
                        }
                    });
                }
                if !text_el {
                    label_ui(ui, "Stroke width");
                    ui.horizontal(|ui| {
                        for (v, t) in [(1.0, "Thin"), (2.0, "Bold"), (4.0, "Extra")] {
                            if rec(format!("width:{t}"), ui.selectable_label((width - v).abs() < 0.01, t)).clicked() {
                                changes.push(Change::Width(v));
                            }
                        }
                    });
                    let mut w = width;
                    if rec("width-slider".into(), ui.add(egui::Slider::new(&mut w, 0.5..=12.0).step_by(0.5).suffix(" px"))).changed() {
                        changes.push(Change::Width(w));
                    }
                    label_ui(ui, "Outline pattern");
                    let combo = egui::ComboBox::from_id_salt("stroke-style").width(200.0).selected_text(STROKE_STYLES.iter().find(|s| s.0 == sstyle).map(|s| s.1).unwrap_or("Solid")).show_ui(ui, |ui| {
                        for (v, t) in STROKE_STYLES {
                            if rec(format!("pattern:{t}"), ui.selectable_label(sstyle == v, t)).clicked() {
                                changes.push(Change::StrokeStyle(v.into()));
                            }
                        }
                    });
                    rec("pattern".into(), combo.response);
                    label_ui(ui, "Sloppiness");
                    ui.horizontal(|ui| {
                        for (v, t) in [(0.0, "Neat"), (1.0, "Sketchy"), (2.0, "Wobbly")] {
                            if rec(format!("rough:{t}"), ui.selectable_label((rough - v).abs() < 0.1, t)).clicked() {
                                changes.push(Change::Rough(v));
                            }
                        }
                    });
                }
                if matches!(kind.as_str(), "rectangle" | "diamond") {
                    label_ui(ui, "Corners");
                    ui.horizontal(|ui| {
                        if rec("corners:Sharp".into(), ui.selectable_label(!round, "Sharp")).clicked() {
                            changes.push(Change::Round(false));
                        }
                        if rec("corners:Round".into(), ui.selectable_label(round, "Round")).clicked() {
                            changes.push(Change::Round(true));
                        }
                    });
                }
                if linear {
                    label_ui(ui, "Line start");
                    head_combo(ui, "start-head", &start_head, &rec, |v| changes.push(Change::StartHead(v)));
                    label_ui(ui, "Line end");
                    head_combo(ui, "end-head", &end_head, &rec, |v| changes.push(Change::EndHead(v)));
                    if first.as_ref().map(|e| binding_of(e, "startBinding").is_some() || binding_of(e, "endBinding").is_some()).unwrap_or(false) {
                        ui.label(egui::RichText::new("Attached to shapes: moves with them. Drag an end to re-attach it.").size(11.0).color(fg));
                    }
                }
                if shape || text_el {
                    ui.separator();
                    label_ui(ui, if text_el { "Text" } else { "Label" });
                    if shape && label.is_none() && first.is_some() {
                        ui.label(egui::RichText::new("Double-click the shape (or press Enter) to type a label").size(11.0).color(fg));
                    }
                    label_ui(ui, "Font");
                    ui.horizontal(|ui| {
                        for (v, t) in [(1u64, "Hand-drawn"), (2, "Normal"), (3, "Code")] {
                            let rt = egui::RichText::new(t).font(font_id_for(v, 13.0));
                            if rec(format!("font:{t}"), ui.selectable_label(font == v || (v == 1 && font == 5), rt)).clicked() {
                                changes.push(Change::Font(v));
                            }
                        }
                    });
                    label_ui(ui, "Font size");
                    ui.horizontal(|ui| {
                        for (v, t) in [(16.0, "S"), (20.0, "M"), (28.0, "L"), (36.0, "XL")] {
                            if rec(format!("size:{t}"), ui.selectable_label((fsize - v).abs() < 0.5, t)).clicked() {
                                changes.push(Change::FontSize(v));
                            }
                        }
                    });
                    label_ui(ui, "Text color");
                    if let Some(c) = swatches(ui, &STROKE_COLORS, &tcolor, "textcolor") {
                        changes.push(Change::TextColor(c));
                    }
                    label_ui(ui, if text_el { "Text background" } else { "Label background" });
                    if let Some(c) = swatches(ui, &BG_COLORS, &lbg, "labelbg") {
                        changes.push(Change::LabelBg(c));
                    }
                    label_ui(ui, "Align");
                    ui.horizontal(|ui| {
                        for (v, t) in [("left", "Left"), ("center", "Center"), ("right", "Right")] {
                            if rec(format!("align:{t}"), ui.selectable_label(align == v, t)).clicked() {
                                changes.push(Change::Align(v));
                            }
                        }
                    });
                }
                ui.separator();
                label_ui(ui, "Opacity");
                let mut o = opacity;
                if rec("opacity-slider".into(), ui.add(egui::Slider::new(&mut o, 0.0..=100.0).step_by(10.0))).changed() {
                    changes.push(Change::Opacity(o));
                }
                if !self.selection.is_empty() {
                    label_ui(ui, "Actions");
                    ui.horizontal_wrapped(|ui| {
                        if rec("action:To back".into(), ui.small_button("To back")).clicked() {
                            action = Some("back");
                        }
                        if rec("action:To front".into(), ui.small_button("To front")).clicked() {
                            action = Some("front");
                        }
                        if rec("action:Duplicate".into(), ui.small_button("Duplicate")).on_hover_text("Ctrl+D").clicked() {
                            action = Some("dup");
                        }
                        if rec("action:Remove".into(), ui.small_button("Remove")).on_hover_text("Delete key").clicked() {
                            action = Some("del");
                        }
                        if shape && first.is_some() && rec("action:Edit label".into(), ui.small_button("Edit label")).on_hover_text("Double-click the shape").clicked() {
                            action = Some("label");
                        }
                    });
                }
            });
        });
        self.props_rect = Some(inner.response.rect);
        self.ui_rects.extend(rects.into_inner());
        if !changes.is_empty() {
            self.apply_changes(changes);
        }
        match action {
            Some("back") => self.reorder_selection(false),
            Some("front") => self.reorder_selection(true),
            Some("dup") => self.duplicate_selection(),
            Some("del") => self.delete_selection(),
            Some("label") => {
                if let Some(e) = &first {
                    let id = e.id.clone();
                    self.edit_label(&id);
                }
            }
            _ => {}
        }
    }

    fn apply_changes(&mut self, changes: Vec<Change>) {
        // remember as defaults for the next shapes
        for c in &changes {
            let s = &mut self.style;
            match c {
                Change::Stroke(v) => s.stroke = v.clone(),
                Change::Bg(v) => s.background = v.clone(),
                Change::Fill(v) => s.fill = v.clone(),
                Change::Width(v) => s.width = *v,
                Change::StrokeStyle(v) => s.stroke_style = v.clone(),
                Change::Rough(v) => s.roughness = *v,
                Change::Opacity(v) => s.opacity = *v,
                Change::Round(v) => s.round = *v,
                Change::Font(v) => s.font_family = *v,
                Change::FontSize(v) => s.font_size = *v,
                Change::TextColor(v) => s.text_color = v.clone(),
                Change::LabelBg(v) => s.label_bg = v.clone(),
                Change::Align(_) => {}
                Change::StartHead(v) => s.start_head = v.clone(),
                Change::EndHead(v) => s.end_head = v.clone(),
            }
        }
        if self.selection.is_empty() {
            return;
        }
        self.snapshot();
        let sel = self.selection.clone();
        // text changes go to text elements, and to the labels of selected shapes
        let mut text_targets: Vec<String> = sel.clone();
        for id in &sel {
            if let Some(l) = self.label_of(id) {
                text_targets.push(l);
            }
        }
        for e in self.scene.elements.iter_mut().filter(|e| !e.is_deleted) {
            let is_sel = sel.contains(&e.id);
            let is_text_target = e.kind == "text" && text_targets.contains(&e.id);
            if !is_sel && !is_text_target {
                continue;
            }
            for c in &changes {
                match c {
                    Change::Stroke(v) if is_sel && e.kind != "text" => e.stroke_color = v.clone(),
                    Change::Bg(v) if is_sel && is_shape(e) => e.background_color = v.clone(),
                    Change::Fill(v) if is_sel && is_shape(e) => e.fill_style = v.clone(),
                    Change::Width(v) if is_sel && e.kind != "text" => e.stroke_width = *v,
                    Change::StrokeStyle(v) if is_sel && e.kind != "text" => e.stroke_style = v.clone(),
                    Change::Rough(v) if is_sel && e.kind != "text" => e.roughness = *v,
                    Change::Opacity(v) => e.opacity = *v,
                    Change::Round(v) if is_sel && matches!(e.kind.as_str(), "rectangle" | "diamond") => {
                        e.extra.insert("roundness".into(), if *v { json!({"type": 3}) } else { Value::Null });
                    }
                    Change::Font(v) if is_text_target => {
                        e.extra.insert("fontFamily".into(), json!(v));
                    }
                    Change::FontSize(v) if is_text_target => e.font_size = Some(*v),
                    Change::TextColor(v) if is_text_target => e.stroke_color = v.clone(),
                    Change::LabelBg(v) if is_text_target => e.background_color = v.clone(),
                    Change::Align(v) if is_text_target => {
                        e.extra.insert("textAlign".into(), json!(v));
                    }
                    Change::StartHead(v) if is_sel && e.is_linear() => {
                        e.extra.insert("startArrowhead".into(), v.clone().map(Value::from).unwrap_or(Value::Null));
                    }
                    Change::EndHead(v) if is_sel && e.is_linear() => {
                        e.extra.insert("endArrowhead".into(), v.clone().map(Value::from).unwrap_or(Value::Null));
                    }
                    _ => {}
                }
            }
        }
        // free text: keep its size in step with font changes
        let ids: Vec<String> = text_targets;
        self.touch_ids(&ids);
        self.changed();
        self.remeasure_free_text = true;
    }
}

fn head_combo(ui: &mut Ui, salt: &str, cur: &Option<String>, rec: &dyn Fn(String, egui::Response) -> egui::Response, mut set: impl FnMut(Option<String>)) {
    let shown = HEADS.iter().find(|h| h.0 == cur.as_deref()).map(|h| h.1).unwrap_or("None");
    let combo = egui::ComboBox::from_id_salt(salt).width(200.0).selected_text(shown).show_ui(ui, |ui| {
        for (v, t) in HEADS {
            if rec(format!("{salt}:{t}"), ui.selectable_label(cur.as_deref() == v, t)).clicked() {
                set(v.map(|s| s.to_string()));
            }
        }
    });
    rec(salt.to_string(), combo.response);
}

fn push_bound(shape: &mut Element, id: &str, kind: &str) {
    let mut list = shape.extra.get("boundElements").and_then(|v| v.as_array()).cloned().unwrap_or_default();
    if !list.iter().any(|b| b.get("id").and_then(|v| v.as_str()) == Some(id)) {
        list.push(json!({"id": id, "type": kind}));
    }
    shape.extra.insert("boundElements".into(), Value::Array(list));
}

fn remove_bound(shape: &mut Element, id: &str) {
    if let Some(list) = shape.extra.get("boundElements").and_then(|v| v.as_array()).cloned() {
        let list: Vec<Value> = list.into_iter().filter(|b| b.get("id").and_then(|v| v.as_str()) != Some(id)).collect();
        shape.extra.insert("boundElements".into(), if list.is_empty() { Value::Null } else { Value::Array(list) });
    }
}

/// Set a line's points from absolute positions (the first point becomes x, y).
pub fn set_abs_points(e: &mut Element, abs: &[Pos2]) {
    if abs.is_empty() {
        return;
    }
    let o = abs[0];
    e.x = o.x as f64;
    e.y = o.y as f64;
    e.points = abs.iter().map(|p| [(p.x - o.x) as f64, (p.y - o.y) as f64]).collect();
    let b = e.bounds();
    e.width = b.width() as f64;
    e.height = b.height() as f64;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc_with_two_boxes() -> (DrawingDoc, String, String, String) {
        let mut d = DrawingDoc::new(PathBuf::from("t.excalidraw"), "");
        let mut a = d.new_element("rectangle", Pos2::new(0.0, 0.0));
        a.width = 100.0;
        a.height = 60.0;
        let mut b = d.new_element("rectangle", Pos2::new(300.0, 0.0));
        b.width = 100.0;
        b.height = 60.0;
        let line = d.new_element("arrow", Pos2::new(100.0, 30.0));
        let (ia, ib, il) = (a.id.clone(), b.id.clone(), line.id.clone());
        d.scene.elements.extend([a, b, line]);
        (d, ia, ib, il)
    }

    #[test]
    fn snapping_finds_edge_middles() {
        let (d, a, _, _) = doc_with_two_boxes();
        let (id, ratio, p) = d.find_snap(Pos2::new(103.0, 33.0), None).unwrap();
        assert_eq!(id, a);
        assert_eq!(ratio, [1.0, 0.5]);
        assert_eq!(p, Pos2::new(100.0, 30.0));
        assert!(d.find_snap(Pos2::new(200.0, 200.0), None).is_none());
    }

    #[test]
    fn connections_follow_moved_shapes() {
        let (mut d, a, b, l) = doc_with_two_boxes();
        d.set_binding(&l, true, Some((a.clone(), [1.0, 0.5])));
        d.set_binding(&l, false, Some((b.clone(), [0.0, 0.5])));
        d.update_bindings();
        let line = d.scene.elements.iter().find(|e| e.id == l).unwrap().clone();
        assert_eq!((line.x, line.y), (100.0, 30.0));
        assert_eq!(line.points.last().unwrap(), &[200.0, 0.0]);
        // shapes know about the connection (Excalidraw boundElements)
        let sa = d.scene.elements.iter().find(|e| e.id == a).unwrap();
        assert!(sa.extra["boundElements"].as_array().unwrap().iter().any(|v| v["id"] == l.as_str()));
        // move shape b down: the end follows
        d.scene.elements.iter_mut().find(|e| e.id == b).unwrap().y += 100.0;
        assert!(d.update_bindings());
        let line = d.scene.elements.iter().find(|e| e.id == l).unwrap();
        let end = Pos2::new((line.x + line.points[1][0]) as f32, (line.y + line.points[1][1]) as f32);
        assert_eq!(end, Pos2::new(300.0, 130.0));
        // deleting a shape lets go of the line
        d.scene.elements.iter_mut().find(|e| e.id == a).unwrap().is_deleted = true;
        d.update_bindings();
        let line = d.scene.elements.iter().find(|e| e.id == l).unwrap();
        assert!(line.extra["startBinding"].is_null());
        // saved file keeps the Excalidraw binding fields
        let json = d.scene.to_json();
        assert!(json.contains("\"endBinding\"") && json.contains("\"fixedPoint\""));
    }

    #[test]
    fn labels_are_centred_and_follow() {
        let (mut d, a, _, _) = doc_with_two_boxes();
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut out = ctx.run_ui(Default::default(), |_| {});
        out.textures_delta.clear();
        d.edit_label(&a);
        d.text_buf = "Hello".into();
        let id = d.editing_text.clone().unwrap();
        if let Some(t) = d.scene.elements.iter_mut().find(|e| e.id == id) {
            t.text = Some("Hello".into());
        }
        d.editing_text = None;
        d.sync_labels(&ctx);
        let t = d.scene.elements.iter().find(|e| e.id == id).unwrap().clone();
        assert_eq!(container_of(&t).as_deref(), Some(a.as_str()));
        assert!(((t.x + t.width / 2.0) - 50.0).abs() < 0.5, "label centred horizontally");
        assert!(((t.y + t.height / 2.0) - 30.0).abs() < 0.5, "label centred vertically");
        d.scene.elements.iter_mut().find(|e| e.id == a).unwrap().x += 40.0;
        d.sync_labels(&ctx);
        let t2 = d.scene.elements.iter().find(|e| e.id == id).unwrap();
        assert!((t2.x - t.x - 40.0).abs() < 0.01);
        // removing the shape removes its label
        d.scene.elements.iter_mut().find(|e| e.id == a).unwrap().is_deleted = true;
        d.sync_labels(&ctx);
        assert!(d.scene.elements.iter().find(|e| e.id == id).unwrap().is_deleted);
    }

    #[test]
    fn arrange_and_spread_move_shapes_and_keep_connections() {
        let (mut d, a, b, l) = doc_with_two_boxes();
        d.set_binding(&l, true, Some((a.clone(), [1.0, 0.5])));
        d.set_binding(&l, false, Some((b.clone(), [0.0, 0.5])));
        d.update_bindings();
        d.spread_shapes(1.25);
        let ra = d.scene.elements.iter().find(|e| e.id == a).unwrap().bounds();
        let rb = d.scene.elements.iter().find(|e| e.id == b).unwrap().bounds();
        assert!(rb.center().x - ra.center().x > 360.0);
        let line = d.scene.elements.iter().find(|e| e.id == l).unwrap();
        let end = Pos2::new((line.x + line.points[1][0]) as f32, (line.y + line.points[1][1]) as f32);
        assert!((end - Pos2::new(rb.min.x, rb.center().y)).length() < 0.5, "connection end stays on the shape");
        d.auto_arrange();
        assert!(d.layout_msg.as_deref().unwrap_or("").starts_with("Arranged"));
        // undo brings the old positions back
        d.undo();
        d.undo();
        let ra2 = d.scene.elements.iter().find(|e| e.id == a).unwrap().bounds();
        assert_eq!(ra2.min, Pos2::new(0.0, 0.0));
    }

    #[test]
    fn line_endings_and_patterns_are_saved() {
        let (mut d, _, _, l) = doc_with_two_boxes();
        d.selection = vec![l.clone()];
        d.apply_changes(vec![Change::StartHead(Some("diamond".into())), Change::EndHead(Some("triangle".into())), Change::StrokeStyle("dashdot".into())]);
        let e = d.scene.elements.iter().find(|e| e.id == l).unwrap();
        assert_eq!(e.extra["startArrowhead"], "diamond");
        assert_eq!(e.extra["endArrowhead"], "triangle");
        assert_eq!(e.stroke_style, "dashdot");
        d.apply_changes(vec![Change::EndHead(None)]);
        assert!(d.scene.elements.iter().find(|e| e.id == l).unwrap().extra["endArrowhead"].is_null());
    }
}

#[cfg(test)]
#[path = "drawing_ui_tests.rs"]
mod ui_tests;
