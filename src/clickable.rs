//! What a clickable label or row looks like and does (LAY-038, rabalyn/shale#692): the pointing
//! hand, a hover background, a focus frame and no text selection. egui labels are selectable by
//! default and a label with a click sense then shows the text cursor, so every label that is a
//! button goes through [`clickable`] ; a row built with `ui.interact` or a
//! `Frame` goes through [`button_like`]. `clickable_ui_tests.rs` guards both: a scan of the sources
//! for a raw clickable label, and a hover over every click-sensing widget of the window.

use eframe::egui::collapsing_header::CollapsingResponse;
use eframe::egui::{CollapsingHeader, ComboBox, CornerRadius, CursorIcon, InnerResponse, Label, Response, Sense, Shape, StrokeKind, Ui};

/// A label that is a button: it senses clicks, cannot be selected, shows the pointing hand and
/// the hover and focus look of [`button_like`]. Sizing and wrapping stay the caller's
/// (`Label::new(…).truncate()`).
pub fn clickable(ui: &mut Ui, label: Label) -> Response {
    let bg = ui.painter().add(Shape::Noop);
    let resp = ui.add(label.sense(Sense::click()).selectable(false));
    if resp.hovered() || resp.has_focus() {
        let v = ui.visuals();
        ui.painter().set(bg, Shape::rect_filled(resp.rect.expand(1.0), RADIUS, v.widgets.hovered.weak_bg_fill));
    }
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect.expand(1.0), RADIUS, ui.visuals().selection.stroke, StrokeKind::Outside);
    }
    pointer(resp)
}

/// A label that is a button when `on`, plain text when not (a name that is a note is a link, one
/// that is not stays text).
pub fn clickable_if(ui: &mut Ui, label: Label, on: bool) -> Response {
    if on {
        clickable(ui, label)
    } else {
        ui.add(label.sense(Sense::hover()))
    }
}

/// The button look of a widget that is already placed (a framed row, an image, a cell made with
/// `ui.interact(rect, id, Sense::click())`): the pointing hand (the open hand when it can be
/// dragged too), and the outline of a hovered widget, or the frame of the focus, on top of it.
pub fn button_like(ui: &Ui, resp: Response) -> Response {
    hover_ring(ui, &resp);
    pointer(resp)
}

/// The outline of a hovered widget, or the frame of its focus, drawn on top of what it painted.
/// For a widget that paints its own hover background but keeps a fixed one while it is the
/// active (selected) one: call it after the painting, so that the active one shows the hover too.
pub fn hover_ring(ui: &Ui, resp: &Response) {
    let v = ui.visuals();
    let stroke = if resp.has_focus() {
        v.selection.stroke
    } else if resp.hovered() {
        v.widgets.hovered.bg_stroke
    } else {
        return;
    };
    ui.painter().rect_stroke(resp.rect.expand(1.0), RADIUS, stroke, StrokeKind::Outside);
}

const RADIUS: CornerRadius = CornerRadius::same(3);

/// Only the cursor of a button, for a widget that paints its own hover look (an icon button, a
/// row with its own background): the pointing hand (the open hand for a widget that is only dragged).
pub fn pointer(resp: Response) -> Response {
    let cursor = if resp.sense.senses_click() { CursorIcon::PointingHand } else { CursorIcon::Grab };
    if resp.enabled() {
        resp.on_hover_cursor(cursor)
    } else {
        resp
    }
}

/// `ComboBox::show_ui` with the pointing hand over the box.
pub trait ComboBoxPointer {
    fn show_pointer<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>>;
}

impl ComboBoxPointer for ComboBox<'_> {
    fn show_pointer<R>(self, ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<Option<R>> {
        let shown = self.show_ui(ui, add_contents);
        pointer(shown.response.clone());
        shown
    }
}

/// `CollapsingHeader::show` with the pointing hand over the header.
pub trait CollapsingPointer {
    fn show_pointer<R>(self, ui: &mut Ui, add_body: impl FnOnce(&mut Ui) -> R) -> CollapsingResponse<R>;
}

impl CollapsingPointer for CollapsingHeader {
    fn show_pointer<R>(self, ui: &mut Ui, add_body: impl FnOnce(&mut Ui) -> R) -> CollapsingResponse<R> {
        let shown = self.show(ui, add_body);
        pointer(shown.header_response.clone());
        shown
    }
}
