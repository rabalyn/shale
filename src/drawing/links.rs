//! Links in drawings and the read-only view (a child module of `drawing`):
//! - an element whose text holds a `[[link]]` (or whose Excalidraw `link` field is set) gets a
//!   link mark at its top-right corner; clicking the mark or Ctrl+clicking the element follows it,
//! - Obsidian canvases are shown read-only: pan, zoom and follow links, nothing else.

use super::*;
use crate::editor::EditorAction;
use crate::vault::Vault;

/// What the drawing view asks the app to do.
pub enum DrawingAction {
    /// the same things a note's text box asks for (follow a link, files added, new contact)
    Editor(EditorAction),
    /// make an editable drawing from this canvas
    ConvertCanvas,
}

/// Where an element links to.
#[derive(Clone, Debug, PartialEq)]
pub enum ElementLink {
    Note { target: String, heading: Option<String> },
    Url(String),
}

impl ElementLink {
    fn action(&self, new_tab: bool) -> DrawingAction {
        DrawingAction::Editor(match self {
            ElementLink::Note { target, heading } => EditorAction::OpenLink { target: target.clone(), heading: heading.clone(), new_tab },
            ElementLink::Url(url) => EditorAction::OpenUrl(url.clone()),
        })
    }
}

/// The first `[[link]]` (or markdown link) in a text, else the first web address.
fn link_in_text(text: &str) -> Option<ElementLink> {
    if let Some(l) = crate::markdown::find_links(text).into_iter().find(|l| !l.target.is_empty()) {
        return Some(ElementLink::Note { target: l.target, heading: l.heading });
    }
    crate::lists::find_web_links(text).into_iter().next().map(|w| ElementLink::Url(w.url))
}

/// Where `e` links to: its Excalidraw `link` field, else a link in its text (a shape: its label).
/// Labels themselves have no link of their own; their shape carries it.
pub fn element_link(scene: &Scene, e: &Element) -> Option<ElementLink> {
    if let Some(l) = e.extra.get("link").and_then(Value::as_str).map(str::trim).filter(|l| !l.is_empty()) {
        return link_in_text(l).or_else(|| Some(if l.contains("://") { ElementLink::Url(l.to_string()) } else { ElementLink::Note { target: l.to_string(), heading: None } }));
    }
    if e.kind == "text" {
        return if container_of(e).is_none() { link_in_text(e.text.as_deref()?) } else { None };
    }
    if is_shape(e) {
        let label = scene.live().find(|t| t.kind == "text" && container_of(t).as_deref() == Some(e.id.as_str()))?;
        return link_in_text(label.text.as_deref()?);
    }
    None
}

/// The texts of an element that may hold links, as stored: the plugin's raw text (with link
/// markup) wins over the original text, which wins over the shown text; then the `link` field.
fn link_texts(e: &Element) -> impl Iterator<Item = &str> {
    let field = |k: &str| e.extra.get(k).and_then(Value::as_str);
    let text = field("rawText").or_else(|| field("originalText")).or(e.text.as_deref());
    text.into_iter().chain(field("link"))
}

/// The links of a drawing for the vault's link index: one line per element text or `link` field
/// that holds a link. A bare note name in a `link` field becomes `[[name]]`; web addresses are left
/// out (they are not vault links).
pub fn link_lines(scene: &Scene) -> String {
    let mut lines: Vec<String> = Vec::new();
    for e in scene.live() {
        for t in link_texts(e) {
            let t = t.trim().replace('\n', " ");
            let line = if crate::markdown::find_links(&t).iter().any(|l| !l.target.is_empty()) {
                t
            } else if !t.is_empty() && !t.contains("://") && e.extra.get("link").and_then(Value::as_str).map(str::trim) == Some(t.as_str()) {
                format!("[[{t}]]")
            } else {
                continue;
            };
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    lines.join("\n")
}

/// Apply `edit` to every text of a drawing file that can hold a link (shown, original and raw
/// text, `link` field). Returns the new file text in the file's own format, or None if nothing
/// changed. Canvases are never rewritten (an error).
pub fn rewrite_link_texts(rel: &std::path::Path, file_text: &str, edit: &dyn Fn(&str) -> String) -> Result<Option<String>, String> {
    if crate::drawing::canvas::is_canvas_path(rel) {
        return Err("canvases are never written".into());
    }
    let mut scene = Scene::from_file_text(rel, file_text)?;
    let mut any = false;
    for e in scene.elements.iter_mut() {
        if let Some(t) = &e.text {
            let new = edit(t);
            if new != *t {
                e.text = Some(new);
                any = true;
            }
        }
        for key in ["originalText", "rawText", "link"] {
            if let Some(t) = e.extra.get(key).and_then(Value::as_str) {
                let new = edit(t);
                if new != t {
                    e.extra.insert(key.into(), json!(new));
                    any = true;
                }
            }
        }
    }
    if !any {
        return Ok(None);
    }
    if crate::drawing::excalidraw_md::is_plugin_file(rel) {
        crate::drawing::excalidraw_md::write(file_text, &scene).map(Some)
    } else {
        Ok(Some(scene.to_json()))
    }
}

/// Screen rect of the link mark of an element with world bounds `b`.
fn mark_rect(view: &View, b: Rect) -> Rect {
    Rect::from_center_size(view.to_screen(Pos2::new(b.max.x, b.min.y)), Vec2::splat(18.0))
}

impl DrawingDoc {
    /// Every element with a link: (id, link, mark rect on screen), topmost last.
    pub(super) fn links_on_screen(&self, view: &View) -> Vec<(String, ElementLink, Rect)> {
        self.scene.live().filter(|e| self.editing_text.as_deref() != Some(e.id.as_str())).filter_map(|e| element_link(&self.scene, e).map(|l| (e.id.clone(), l, mark_rect(view, e.bounds())))).collect()
    }

    /// The link whose mark is under `p` (screen), topmost first.
    pub(super) fn mark_at(&self, view: &View, p: Pos2) -> Option<ElementLink> {
        self.links_on_screen(view).into_iter().rev().find(|(_, _, r)| r.contains(p)).map(|x| x.1)
    }

    /// The link of the topmost element under `wp` (world). Read-only, a card counts anywhere
    /// inside its box (there is nothing to select, and canvas cards are often unfilled).
    pub(super) fn link_at(&self, wp: Pos2, tol: f32) -> Option<ElementLink> {
        let e = if self.read_only.is_some() {
            let live: Vec<&Element> = self.scene.live().collect();
            live.into_iter().rev().filter(|e| container_of(e).is_none() && !e.is_linear()).find(|e| e.bounds().expand(tol).contains(wp))?
        } else {
            let id = self.topmost_hit(wp, tol)?;
            self.scene.live().find(|e| e.id == id)?
        };
        element_link(&self.scene, e)
    }

    /// Paint the link marks; hovering one shows where it goes. Returns true if the pointer is on one.
    pub(super) fn paint_links(&self, ui: &Ui, painter: &egui::Painter, view: &View, resp: &egui::Response, vault: &Vault) -> bool {
        let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| resp.rect.contains(*p));
        let mut on_mark = false;
        for (_, link, r) in self.links_on_screen(view) {
            let (colour, tip) = match &link {
                ElementLink::Note { target, .. } if vault.exists(target, Some(&self.rel)) => (crate::theme::LINK, format!("Open “{target}”")),
                ElementLink::Note { target, .. } => (crate::theme::LINK_MISSING, format!("Create note “{target}”")),
                ElementLink::Url(url) => (crate::theme::LINK, url.clone()),
            };
            let hot = hover.is_some_and(|p| r.contains(p));
            painter.circle_filled(r.center(), r.width() / 2.0, if hot { colour } else { colour.gamma_multiply(0.85) });
            painter.text(r.center(), Align2::CENTER_CENTER, "↗", FontId::proportional(12.0), Color32::WHITE);
            if hot {
                on_mark = true;
                let hint = if self.read_only.is_some() { "Click to follow · Shift opens a new tab" } else { "Click to follow · Shift opens a new tab · Ctrl+click on the shape works too" };
                resp.clone().on_hover_ui_at_pointer(|ui| {
                    ui.label(egui::RichText::new(&tip).color(colour));
                    ui.label(egui::RichText::new(hint).small().color(crate::theme::MUTED));
                });
            }
        }
        on_mark
    }

    /// A click at `sp` (screen) that follows a link: on a mark, or Ctrl+click (any click when
    /// read-only) on an element with a link.
    pub(super) fn follow_click(&self, ui: &Ui, view: &View, sp: Pos2, tol: f32) -> Option<DrawingAction> {
        let m = ui.input(|i| i.modifiers);
        let link = self.mark_at(view, sp).or_else(|| (m.command || self.read_only.is_some()).then(|| self.link_at(view.to_world(sp), tol)).flatten())?;
        Some(link.action(m.shift))
    }

    /// The view of a drawing that can't be edited (an Obsidian canvas): pan, zoom, follow links.
    pub(super) fn ui_read_only(&mut self, ui: &mut Ui, dark: bool, vault: &Vault) -> Option<DrawingAction> {
        let mut action = None;
        self.ui_rects.clear();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.ui_rects.push(("canvas".into(), rect));
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::ZERO, if dark { Color32::from_rgb(18, 18, 18) } else { Color32::WHITE });
        self.fit_once(rect);
        self.wheel(ui, rect, &resp);
        if resp.dragged() {
            self.offset += resp.drag_delta() / self.zoom;
        }
        let view = View { origin: rect.min, offset: self.offset, zoom: self.zoom };
        paint_grid(&painter, rect, &view, dark);
        // tidy labels and connections in memory only: the file is never written
        self.sync_labels(ui.ctx());
        self.update_bindings();
        let pics = crate::drawing::pictures::textures(ui.ctx(), &self.scene, vault, &self.rel);
        paint_scene(&painter, &self.scene, &view, dark, None, &pics);
        let on_mark = self.paint_links(ui, &painter, &view, &resp, vault);
        let tol = 6.0 / self.zoom;
        if resp.clicked() {
            if let Some(sp) = resp.interact_pointer_pos() {
                action = self.follow_click(ui, &view, sp, tol);
            }
        }
        if resp.hovered() {
            let over_link = on_mark || ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| self.link_at(view.to_world(p), tol).is_some());
            ui.ctx().set_cursor_icon(if resp.dragged() {
                egui::CursorIcon::Grabbing
            } else if over_link {
                egui::CursorIcon::PointingHand
            } else {
                egui::CursorIcon::Grab
            });
        }

        // what this is, and the way to an editable copy
        let w = 520.0_f32.min(rect.width() - 20.0);
        let r = Rect::from_min_size(Pos2::new(rect.center().x - w / 2.0, rect.min.y + 10.0), Vec2::new(w, 44.0));
        Self::blocker(ui, r, "read-only-block");
        let mut child = ui.new_child(UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Center)));
        Self::panel_frame(dark).show(&mut child, |ui| {
            ui.horizontal(|ui| {
                let why = self.read_only.clone().unwrap_or_default();
                ui.label(egui::RichText::new(why).color(if dark { crate::theme::TEXT } else { Color32::from_gray(60) }));
                let b = ui.button("Convert to drawing…");
                self.ui_rects.push(("convert".into(), b.rect));
                if b.on_hover_text("Make an editable Excalidraw drawing next to it (the canvas file is not changed)").clicked() {
                    action = Some(DrawingAction::ConvertCanvas);
                }
            });
        });
        self.zoom_bar(ui, rect, dark);
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene_with(elements: &str) -> Scene {
        Scene::parse(&format!(r#"{{"type":"excalidraw","version":2,"elements":[{elements}]}}"#)).unwrap()
    }

    // spec: DRAW-015
    #[test]
    fn links_of_elements() {
        let s = scene_with(
            r#"{"id":"r","type":"rectangle","x":0,"y":0,"width":100,"height":50},
               {"id":"l","type":"text","x":10,"y":10,"text":"see [[Plan#Goals]] now","containerId":"r"},
               {"id":"t","type":"text","x":200,"y":0,"text":"docs at https://example.org/x"},
               {"id":"f","type":"ellipse","x":0,"y":100,"width":10,"height":10,"link":"[[Other]]"},
               {"id":"u","type":"diamond","x":0,"y":200,"width":10,"height":10,"link":"https://example.com"},
               {"id":"n","type":"rectangle","x":0,"y":300,"width":10,"height":10,"link":"Bare name"},
               {"id":"p","type":"text","x":0,"y":400,"text":"plain"}"#,
        );
        let link = |id: &str| element_link(&s, s.elements.iter().find(|e| e.id == id).unwrap());
        assert_eq!(link("r"), Some(ElementLink::Note { target: "Plan".into(), heading: Some("Goals".into()) }), "a shape links through its label");
        assert_eq!(link("l"), None, "the label itself carries no mark");
        assert_eq!(link("t"), Some(ElementLink::Url("https://example.org/x".into())));
        assert_eq!(link("f"), Some(ElementLink::Note { target: "Other".into(), heading: None }));
        assert_eq!(link("u"), Some(ElementLink::Url("https://example.com".into())));
        assert_eq!(link("n"), Some(ElementLink::Note { target: "Bare name".into(), heading: None }));
        assert_eq!(link("p"), None);
    }
}
