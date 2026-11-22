//! Dialog windows. Every dialog in Shale is shown through [`Dialog`], so they all work alike:
//! drag the title bar to move it, drag any edge or corner to resize it, and the content
//! scrolls when the dialog is made smaller than what it holds.
//!
//! A modal dialog dims the rest of the window and blocks it; Esc, the ✕ in the title bar or a
//! click on the dimmed part asks to close it ([`DialogResponse::should_close`]), as before.
//!
//! Size: a dialog opens at its given width and as tall as its content (at most the window
//! height). Until the user resizes it, it follows its content both ways: it grows when the
//! content grows (a form that gets more fields) and gets shorter when the content does, also
//! when it is opened again with less content, so no empty room is left above the footer or at
//! the bottom. Only the bottom edge moves. Position and size are kept while the app runs, so a
//! dialog opened again comes back where it was left; one the user sized by hand keeps that size.

use eframe::egui::{self, Align2, Color32, Context, Id, Key, Modifiers, Order, Pos2, Rect, ScrollArea, Sense, Ui, Vec2};

/// Space kept free around a dialog that is as large as the window allows.
const MARGIN: f32 = 24.0;
/// Height of the title bar and the window frame, roughly.
const TITLE_ROOM: f32 = 40.0;
/// The smallest a dialog can be made.
const MIN_SIZE: Vec2 = Vec2::new(220.0, 90.0);
/// Room kept below the scrolling content for a footer (its separator and one row of buttons)
/// in the first frame; from the second frame on the height the footer measured is used (NAV-029).
const FOOTER_ROOM: f32 = 40.0;
/// What the room kept for the footer is larger than the footer measured: not to clip it by a
/// rounding error.
const FOOTER_SLACK: f32 = 1.0;
/// How much room below the content a dialog that follows its content must have before it gets
/// shorter (more than the rounding and the footer's slack leave).
const SHRINK_MIN: f32 = 3.0;
/// Frames a new dialog takes to measure its content: egui's invisible sizing pass, one frame
/// to measure, one in which the window takes that size.
const FIT_FRAMES: u8 = 3;

/// Remembered between frames, per dialog.
#[derive(Clone, Copy, Debug)]
enum Sizing {
    /// New: measuring the content. The window follows the content height, centred on the screen.
    Fitting { frames: u8 },
    /// Placed and its content grew past it: one frame of following the content height again,
    /// with the top-left corner kept where it is; then it is set to what it measured
    /// ([`Sizing::Settling`]) like a new dialog, so it ends the same.
    Growing,
    /// Measured, the frame in which the window is set to exactly the measured size: egui keeps
    /// the largest size it was asked for while measuring (a wrapping label is taller in the
    /// first frames, before the window has its width), which left an empty band below the
    /// content.
    /// Also the frame in which a dialog gets shorter because its content did. `floor` is the
    /// height the window turned out not to get below (egui's smallest size), 0 if not known.
    Settling { size: Vec2, floor: f32 },
    /// Placed. `auto` until the user resizes it: then it keeps its size, it neither grows nor
    /// gets shorter with its content.
    Placed { auto: bool, size: Vec2, floor: f32 },
}

pub struct Dialog {
    id: Id,
    title: String,
    width: f32,
    modal: bool,
}

pub struct DialogResponse {
    close: bool,
}

impl DialogResponse {
    /// Esc (on the top dialog, with no popup open), the ✕, or a click outside a modal dialog.
    pub fn should_close(&self) -> bool {
        self.close
    }
}

impl Dialog {
    /// A modal dialog: the rest of the app is dimmed and does not take input.
    pub fn new(id: impl std::hash::Hash + std::fmt::Debug, title: impl Into<String>) -> Self {
        Self { id: Id::unique(("dialog", id)), title: title.into(), width: 440.0, modal: true }
    }

    /// The width it opens with (it can be resized from there).
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Not modal: the app behind stays usable and is not dimmed. Esc is left to the caller.
    pub fn non_modal(mut self) -> Self {
        self.modal = false;
        self
    }

    pub fn show(self, ctx: &Context, content: impl FnOnce(&mut Ui)) -> DialogResponse {
        self.show_inner(ctx, &mut (), None::<fn(&mut Ui, &mut ())>, |ui, _| content(ui), None::<fn(&mut Ui, &mut ())>)
    }

    /// Like [`Dialog::show`], with a header above the content (a search field and its key hint,
    /// NAV-049). Only the content scrolls; the header stays in view, also in a window shorter than
    /// the content. The header and the content are closures over the same `state`; the header runs
    /// first, in the same frame.
    pub fn show_with_header<S>(self, ctx: &Context, state: &mut S, header: impl FnOnce(&mut Ui, &mut S), content: impl FnOnce(&mut Ui, &mut S)) -> DialogResponse {
        self.show_inner(ctx, state, Some(header), content, None::<fn(&mut Ui, &mut S)>)
    }

    /// Like [`Dialog::show`], with a footer (the buttons) below the content. Only the content
    /// scrolls; the footer stays in view, also in a window shorter than the content. The content
    /// and the footer are closures over the same `state` (they could not both borrow what the
    /// dialog edits); the footer runs right after the content, in the same frame.
    pub fn show_with_footer<S>(self, ctx: &Context, state: &mut S, content: impl FnOnce(&mut Ui, &mut S), footer: impl FnOnce(&mut Ui, &mut S)) -> DialogResponse {
        self.show_inner(ctx, state, None::<fn(&mut Ui, &mut S)>, content, Some(footer))
    }

    fn show_inner<S>(self, ctx: &Context, state: &mut S, header: Option<impl FnOnce(&mut Ui, &mut S)>, content: impl FnOnce(&mut Ui, &mut S), footer: Option<impl FnOnce(&mut Ui, &mut S)>) -> DialogResponse {
        let Self { id, title, width, modal } = self;
        let has_footer = footer.is_some();
        let screen = ctx.content_rect();
        let max = (screen.size() - Vec2::splat(2.0 * MARGIN)).max(MIN_SIZE);
        let sizing_key = id.with("sizing");
        let sizing = ctx.data(|d| d.get_temp::<Sizing>(sizing_key)).unwrap_or(Sizing::Fitting { frames: 0 });
        let any_popup_open = egui::Popup::is_any_open(ctx);

        // ---- the dimmed backdrop: takes the clicks outside the dialog and blocks the app behind it
        let mut backdrop_clicked = false;
        let mut is_top = true;
        if modal {
            let backdrop = egui::Area::new(id.with("backdrop")).order(Order::Foreground).fixed_pos(screen.min).interactable(true).sense(Sense::click_and_drag()).show(ctx, |ui| {
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(100));
                ui.allocate_space(screen.size());
            });
            let layer = backdrop.response.layer_id;
            backdrop_clicked = backdrop.response.clicked();
            is_top = ctx.memory(|m| m.top_modal_layer()).is_none_or(|top| top == layer);
            ctx.memory_mut(|m| m.set_modal_layer(layer));
            // the dialog is drawn right above its backdrop, also when the backdrop is clicked
            ctx.set_sublayer(layer, egui::LayerId::new(Order::Foreground, id));
        }

        // ---- the window
        let mut open = true;
        let mut window = egui::Window::new(title)
            .id(id)
            .order(Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .movable(true)
            .constrain(true)
            .min_size(MIN_SIZE.min(max))
            .max_size(screen.size())
            .default_width(width.min(max.x))
            .default_height(MIN_SIZE.y);
        let measuring = matches!(sizing, Sizing::Fitting { .. } | Sizing::Growing);
        if measuring {
            window = window.max_height(max.y);
        }
        if let Sizing::Settling { size, .. } = sizing {
            window = window.fixed_size(size);
        }
        if matches!(sizing, Sizing::Fitting { .. }) {
            // centred while it finds its height; from then on it stays where it is put
            window = window.pivot(Align2::CENTER_CENTER).default_pos(screen.center());
        }
        let mut overflow = false;
        let mut spare = 0.0;
        let footer_key = id.with("footer-height");
        let footer_room = ctx.data(|d| d.get_temp::<f32>(footer_key)).unwrap_or(FOOTER_ROOM);
        let shown = window.show(ctx, |ui| {
            // the spacing raster (NAV-042): the content and the footer both inherit it
            crate::theme::dialog_spacing(ui);
            // The content always sits in the same scroll area (so its widgets keep their state).
            // While measuring, the scroll area may be as tall as the screen allows and shrinks to
            // the content (an inner scroll area takes what it is given), so the window takes the
            // content's height; otherwise it fills the window and scrolls.
            let top = ui.cursor().min.y;
            if let Some(header) = header {
                header(ui, state);
            }
            let header_room = ui.cursor().min.y - top;
            let reserve = if footer.is_some() { footer_room } else { 0.0 };
            let (room, shrink) = if measuring {
                (Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), (max.y - TITLE_ROOM - header_room - reserve).max(MIN_SIZE.y))), [false, true])
            } else {
                let mut room = ui.available_rect_before_wrap();
                room.max.y = (room.max.y - reserve).max(room.min.y);
                (room, [false, false])
            };
            let scope = ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
                let out = ScrollArea::vertical().id_salt("dialog-body").auto_shrink(shrink).show(ui, |ui| content(ui, &mut *state));
                overflow = out.content_size.y > out.inner_rect.height() + 1.0;
                spare = out.inner_rect.height() - out.content_size.y;
            });
            if let Some(footer) = footer {
                ui.separator();
                footer(ui, state);
                // what the footer took, measured from the end of the content: the room kept for
                // it in the next frame (a bigger font makes a taller footer)
                let measured = ui.min_rect().bottom() - scope.response.rect.bottom() + FOOTER_SLACK;
                if (measured - footer_room).abs() > 0.5 {
                    ctx.data_mut(|d| d.insert_temp(footer_key, measured));
                    ctx.request_repaint();
                }
            }
        });

        let Some(shown) = shown else {
            return DialogResponse { close: !open };
        };
        let rect = shown.response.rect;

        // ---- what to do next frame
        let next = match sizing {
            Sizing::Fitting { frames } if frames + 1 < FIT_FRAMES => Sizing::Fitting { frames: frames + 1 },
            Sizing::Fitting { .. } => {
                // done measuring: pin the top-left corner so a resize does not move the dialog
                pin_left_top(ctx, id, rect.min);
                // the window gets the slack of the footer's room too, or the content (as tall as
                // it is) would be a hair too tall for the scroll area and get a scroll bar
                Sizing::Settling { size: rect.size() + Vec2::new(0.0, if has_footer { FOOTER_SLACK } else { 0.0 }), floor: 0.0 }
            }
            Sizing::Settling { size, floor } => {
                // asked for less than egui lets a window be: that is the lowest it gets
                let floor = if rect.height() > size.y + 1.0 { rect.height() } else { floor };
                Sizing::Placed { auto: true, size: rect.size(), floor }
            }
            // measured again: set to that size with the footer's slack, as a new dialog is
            Sizing::Growing => Sizing::Settling { size: rect.size() + Vec2::new(0.0, if has_footer { FOOTER_SLACK } else { 0.0 }), floor: 0.0 },
            Sizing::Placed { auto, size, floor } => {
                let resized = (rect.size() - size).length() > 0.5 && ctx.input(|i| i.pointer.any_down());
                let auto = auto && !resized;
                if auto && overflow && rect.height() < max.y - 1.0 {
                    // the content grew and the user has not sized the dialog: grow with it
                    Sizing::Growing
                } else if auto && spare > SHRINK_MIN && !ctx.input(|i| i.pointer.any_down()) && rect.height() - spare > floor + 0.5 {
                    // the content got shorter and the user has not sized the dialog: follow it
                    // (the footer's slack too, as when the dialog was new)
                    Sizing::Settling { size: rect.size() - Vec2::new(0.0, spare - if has_footer { FOOTER_SLACK } else { 0.0 }), floor }
                } else {
                    Sizing::Placed { auto, size: rect.size(), floor }
                }
            }
        };
        ctx.data_mut(|d| d.insert_temp(sizing_key, next));
        if !matches!(next, Sizing::Placed { .. }) {
            ctx.request_repaint();
        }
        let esc = modal && is_top && !any_popup_open && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        DialogResponse { close: !open || backdrop_clicked || esc }
    }
}

/// Switch the window's area from "centred" to "top-left corner here".
fn pin_left_top(ctx: &Context, id: Id, left_top: Pos2) {
    if let Some(mut st) = egui::AreaState::load(ctx, id) {
        st.pivot = Align2::LEFT_TOP;
        st.pivot_pos = Some(left_top);
        ctx.memory_mut(|m| m.areas_mut().set_state(egui::LayerId::new(Order::Foreground, id), st));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Event, PointerButton, RawInput, Rect};

    /// A dialog with `rows` lines of text, run headless; remembers the window rect each frame.
    struct W {
        ctx: Context,
        t: f64,
        rows: usize,
        open: bool,
        modal: bool,
    }

    const SCREEN: Vec2 = Vec2::new(1200.0, 800.0);

    impl W {
        fn new(rows: usize, modal: bool) -> Self {
            let mut w = W { ctx: Context::default(), t: 0.0, rows, open: true, modal };
            w.frames(4);
            w
        }
        fn frame(&mut self, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.t), events, ..Default::default() };
            let (rows, modal) = (self.rows, self.modal);
            let mut close = false;
            let mut out = self.ctx.run_ui(raw, |ui| {
                let d = Dialog::new("test", "Test dialog").width(400.0);
                let d = if modal { d } else { d.non_modal() };
                let r = d.show(ui.ctx(), |ui| {
                    for i in 0..rows {
                        ui.label(format!("line {i}"));
                    }
                });
                close = r.should_close();
            });
            out.textures_delta.clear();
            if close {
                self.open = false;
            }
        }
        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(vec![]);
            }
        }
        fn rect(&self) -> Rect {
            self.ctx.memory(|m| m.area_rect(Id::unique(("dialog", "test")))).expect("dialog shown")
        }
        fn drag(&mut self, from: Pos2, to: Pos2) {
            let b = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![b(from, true)]);
            for k in 1..=5 {
                self.frame(vec![Event::PointerMoved(from + (to - from) * (k as f32 / 5.0))]);
            }
            self.frame(vec![b(to, false)]);
            self.frames(3);
        }
    }

    #[test]
    fn opens_centred_at_its_width_and_as_tall_as_its_content() {
        let w = W::new(5, true);
        let r = w.rect();
        assert!((r.center().x - SCREEN.x / 2.0).abs() < 2.0 && (r.center().y - SCREEN.y / 2.0).abs() < 2.0, "{r:?}");
        assert!((r.width() - 400.0).abs() < 2.0, "{r:?}");
        assert!(r.height() > 5.0 * 14.0 && r.height() < 300.0, "{r:?}");
    }

    /// A dialog with 200 rows of content and a footer of an error line and a button, in a
    /// short window; every text style scaled by `scale`. Returns the dialog and the button.
    fn footer_at(scale: f32, screen: Vec2) -> (Rect, Rect) {
        let ctx = Context::default();
        ctx.all_styles_mut(|s| s.text_styles.values_mut().for_each(|f| f.size *= scale));
        let mut button = Rect::NOTHING;
        for i in 0..12 {
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, screen)), time: Some(f64::from(i) * 0.05), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                Dialog::new("footer", "Footer").width(400.0).show_with_footer(
                    ui.ctx(),
                    &mut button,
                    |ui, _| {
                        for i in 0..200 {
                            ui.label(format!("line {i}"));
                        }
                    },
                    |ui, button| {
                        ui.label("An error that takes a line");
                        *button = ui.button("Done").rect;
                    },
                );
            });
            out.textures_delta.clear();
        }
        (ctx.memory(|m| m.area_rect(Id::unique(("dialog", "footer")))).expect("dialog shown"), button)
    }

    /// What a dialog's content and footer see of the spacing, and what the ui around it sees.
    // spec: NAV-042
    #[test]
    fn nav_042_a_dialog_has_the_raster_in_its_content_and_its_footer_and_the_window_around_it_keeps_its_spacing() {
        use crate::theme::{ITEM_GAP, LINE_GAP};
        let ctx = Context::default();
        let (mut outside, mut content, mut footer) = ((0.0, 0.0), (0.0, 0.0), (0.0, 0.0));
        let before = ctx.global_style().spacing.item_spacing.y;
        for i in 0..4 {
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(f64::from(i) * 0.05), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                outside = (ui.spacing().item_spacing.y, ui.spacing().extra_text_line_spacing);
                Dialog::new("raster", "Raster").show_with_footer(
                    ui.ctx(),
                    &mut (),
                    |ui, _| content = (ui.spacing().item_spacing.y, ui.spacing().extra_text_line_spacing),
                    |ui, _| footer = (ui.spacing().item_spacing.y, ui.spacing().extra_text_line_spacing),
                );
            });
            out.textures_delta.clear();
        }
        assert_eq!(content, (ITEM_GAP, LINE_GAP));
        assert_eq!(footer, (ITEM_GAP, LINE_GAP));
        assert_eq!(outside, (before, 0.0), "the window outside the dialog is untouched");
    }

    /// Two wrapped paragraphs one below the other in a narrow dialog: the pitch of the rows of
    /// one, and the gap between the two.
    // spec: NAV-042
    #[test]
    fn nav_042_in_a_dialog_the_gap_between_two_paragraphs_is_bigger_than_the_gap_between_the_lines_of_one() {
        use crate::theme::{ITEM_GAP, LINE_GAP};
        const TEXT: &str = "A paragraph that is long enough to wrap over several lines in a narrow dialog.";
        let ctx = Context::default();
        let mut measured = None;
        for i in 0..6 {
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(f64::from(i) * 0.05), ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| {
                Dialog::new("paragraphs", "Paragraphs").width(220.0).show(ui.ctx(), |ui| {
                    let rich = crate::theme::spaced(ui, egui::RichText::new(TEXT));
                    let (_, first, r1) = egui::Label::new(rich).layout_in_ui(ui);
                    let r2 = crate::theme::paragraph(ui, TEXT);
                    let row = ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Body.resolve(ui.style())));
                    measured = Some((first.rows[1].pos.y - first.rows[0].pos.y - row, r2.rect.top() - r1.rect.bottom(), first.rows.len()));
                });
            });
            out.textures_delta.clear();
        }
        let (line_gap, item_gap, rows) = measured.expect("shown");
        assert!(rows >= 3, "the paragraph wraps: {rows} rows");
        assert!((line_gap - LINE_GAP).abs() < 0.6, "between the lines of one paragraph: {line_gap}");
        assert!((item_gap - ITEM_GAP).abs() < 0.6, "between two paragraphs: {item_gap}");
        assert!(item_gap > line_gap + 2.0, "clearly bigger: {item_gap} against {line_gap}");
    }

    // spec: NAV-029
    #[test]
    fn nav_029_the_footer_is_not_clipped_at_the_biggest_font_and_stays_inside_the_dialog() {
        let screen = Vec2::new(900.0, 420.0);
        // the biggest the settings allow for the text is 24 against the default 15.5
        for scale in [1.0, 24.0 / 15.5, 2.5] {
            let (dialog, button) = footer_at(scale, screen);
            assert!(Rect::from_min_size(Pos2::ZERO, screen).contains_rect(dialog), "scale {scale}: {dialog:?}");
            assert!(dialog.contains_rect(button), "scale {scale}: the button {button:?} is not inside the dialog {dialog:?}");
            assert!(button.height() > 14.0, "scale {scale}: the button is not clipped to nothing: {button:?}");
        }
    }

    // spec: NAV-013
    #[test]
    fn nav_013_a_dialog_opens_centred_in_a_window_of_any_height_and_a_tall_one_fills_it() {
        for height in [300.0, 700.0, 900.0, 1100.0, 1300.0, 2400.0] {
            for rows in [5, 400] {
                let ctx = Context::default();
                for i in 0..8 {
                    let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500.0, height))), time: Some(f64::from(i) * 0.05), ..Default::default() };
                    let mut out = ctx.run_ui(raw, |ui| {
                        Dialog::new("centred", "Centred").width(400.0).show(ui.ctx(), |ui| {
                            for i in 0..rows {
                                ui.label(format!("line {i}"));
                            }
                        });
                    });
                    out.textures_delta.clear();
                }
                let r = ctx.memory(|m| m.area_rect(Id::unique(("dialog", "centred")))).expect("dialog shown");
                assert!((r.center().y - height / 2.0).abs() < 2.0 && (r.center().x - 750.0).abs() < 2.0, "window {height} high, {rows} rows: {r:?}");
                assert!(r.top() >= 0.0 && r.bottom() <= height, "window {height} high, {rows} rows: {r:?}");
            }
        }
    }

    #[test]
    fn a_tall_dialog_fits_on_the_screen() {
        let w = W::new(200, true);
        let r = w.rect();
        assert!(r.top() >= 0.0 && r.bottom() <= SCREEN.y, "{r:?}");
    }

    #[test]
    fn moves_by_its_title_bar() {
        let mut w = W::new(5, true);
        let r = w.rect();
        let grab = Pos2::new(r.center().x, r.top() + 10.0);
        w.drag(grab, grab + Vec2::new(-200.0, -100.0));
        let r2 = w.rect();
        assert!((r2.min - r.min - Vec2::new(-200.0, -100.0)).length() < 3.0, "{r:?} -> {r2:?}");
        assert_eq!(r2.size(), r.size());
        assert!(w.open, "dragging is not a click outside");
    }

    #[test]
    fn resizes_from_its_corner_and_keeps_its_place() {
        let mut w = W::new(5, true);
        let r = w.rect();
        let corner = r.max - Vec2::splat(2.0);
        w.drag(corner, corner + Vec2::new(150.0, 120.0));
        let r2 = w.rect();
        assert!((r2.min - r.min).length() < 2.0, "the top-left corner stays: {r:?} -> {r2:?}");
        assert!((r2.width() - r.width() - 150.0).abs() < 3.0 && (r2.height() - r.height() - 120.0).abs() < 3.0, "{r:?} -> {r2:?}");
        // and smaller than its content: it scrolls instead of growing back
        let corner = r2.max - Vec2::splat(2.0);
        w.drag(corner, Pos2::new(r2.min.x + 260.0, r2.min.y + 100.0));
        w.frames(5);
        let r3 = w.rect();
        assert!(r3.height() < r.height() - 15.0 && r3.width() < 270.0, "smaller than the content: {r:?}, {r2:?} -> {r3:?}");
    }

    #[test]
    fn resizes_from_its_left_edge() {
        let mut w = W::new(5, true);
        let r = w.rect();
        let edge = Pos2::new(r.left() + 1.0, r.center().y);
        w.drag(edge, edge - Vec2::new(100.0, 0.0));
        let r2 = w.rect();
        assert!((r2.left() - (r.left() - 100.0)).abs() < 3.0 && (r2.right() - r.right()).abs() < 3.0, "{r:?} -> {r2:?}");
    }

    #[test]
    fn grows_with_its_content_until_resized() {
        let mut w = W::new(3, true);
        let h = w.rect().height();
        w.rows = 12;
        w.frames(4);
        let r = w.rect();
        assert!(r.height() > h + 100.0, "{h} -> {r:?}");
    }

    /// What a [`B`] dialog drew in its last frame.
    struct Seen {
        rows: usize,
        wrap: bool,
        last: Rect,
        visible: Rect,
        button: Rect,
    }

    /// A dialog with a footer (one button) and `rows` lines of content, optionally followed by a
    /// long wrapping label; the content can change between frames, and the dialog can be left
    /// out of some frames (closed).
    struct B {
        ctx: Context,
        t: f64,
        rows: usize,
        wrap: bool,
        footer: bool,
        shown: bool,
        seen: Seen,
        repaints: usize,
    }

    const LONG: &str = "A long line of text that wraps at the width of the dialog, so that it is measured taller while the window has no width yet and shorter after it, the way the lines of the Import dialog do.";

    impl B {
        fn new(rows: usize, footer: bool) -> Self {
            let mut b = B { ctx: Context::default(), t: 0.0, rows, wrap: false, footer, shown: true, seen: Seen { rows: 0, wrap: false, last: Rect::NOTHING, visible: Rect::NOTHING, button: Rect::NOTHING }, repaints: 0 };
            b.frames(8);
            b
        }
        fn frame(&mut self, events: Vec<Event>) {
            self.t += 0.05;
            let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN)), time: Some(self.t), events, ..Default::default() };
            self.seen.rows = self.rows;
            self.seen.wrap = self.wrap;
            let (shown, has_footer) = (self.shown, self.footer);
            let seen = &mut self.seen;
            let mut out = self.ctx.run_ui(raw, |ui| {
                if !shown {
                    return;
                }
                let content = |ui: &mut Ui, seen: &mut Seen| {
                    for i in 0..seen.rows {
                        seen.last = ui.label(format!("line {i}")).rect;
                    }
                    if seen.wrap {
                        seen.last = ui.label(LONG).rect;
                    }
                    seen.visible = ui.clip_rect();
                };
                let d = Dialog::new("b", "B").width(400.0);
                if has_footer {
                    d.show_with_footer(ui.ctx(), seen, content, |ui, seen| seen.button = ui.button("Done").rect);
                } else {
                    d.show(ui.ctx(), |ui| content(ui, &mut *seen));
                }
            });
            if out.viewport_output.values().any(|v| v.repaint_delay.is_zero()) {
                self.repaints += 1;
            }
            out.textures_delta.clear();
        }
        fn frames(&mut self, n: usize) {
            for _ in 0..n {
                self.frame(vec![]);
            }
        }
        fn rect(&self) -> Rect {
            self.ctx.memory(|m| m.area_rect(Id::unique(("dialog", "b")))).expect("dialog shown")
        }
        /// Between the last line and the footer's button (a footer dialog), or the dialog's bottom
        /// edge (without a footer).
        fn gap(&self) -> f32 {
            if self.footer {
                self.seen.button.top() - self.seen.last.bottom()
            } else {
                self.rect().bottom() - self.seen.last.bottom()
            }
        }
        /// How far the last line is below what the scroll area shows (it scrolls).
        fn hidden(&self) -> f32 {
            self.seen.last.bottom() - self.seen.visible.bottom()
        }
        fn drag(&mut self, from: Pos2, to: Pos2) {
            let b = |pos, pressed| Event::PointerButton { pos, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            self.frame(vec![Event::PointerMoved(from)]);
            self.frame(vec![b(from, true)]);
            for k in 1..=5 {
                self.frame(vec![Event::PointerMoved(from + (to - from) * (k as f32 / 5.0))]);
            }
            self.frame(vec![b(to, false)]);
            self.frames(3);
        }
    }

    impl B {
        /// A new dialog with `rows` lines, run until it has settled.
        fn fresh(rows: usize, footer: bool, wrap: bool) -> Self {
            let mut b = B::new(rows, footer);
            b.wrap = wrap;
            b.frames(8);
            b
        }
        fn placed(&self) -> bool {
            let key = Id::unique(("dialog", "b")).with("sizing");
            self.ctx.data(|d| d.get_temp::<Sizing>(key)).is_some_and(|s| matches!(s, Sizing::Placed { .. }))
        }
    }

    /// Every case: with and without a footer, with and without a wrapping line after the rows.
    const CASES: [(bool, bool); 4] = [(true, false), (true, true), (false, false), (false, true)];

    fn same_as_fresh(b: &B, fresh: &B, what: &str) {
        let (r, f) = (b.rect(), fresh.rect());
        assert!((r.height() - f.height()).abs() <= 1.5, "{what}: {r:?} against a new dialog {f:?}");
        assert!((b.gap() - fresh.gap()).abs() <= 1.5, "{what}: {} px of room against {} in a new dialog", b.gap(), fresh.gap());
        assert!(b.hidden() <= fresh.hidden() + 0.5, "{what}: the content scrolls by {} px, a new dialog by {}", b.hidden(), fresh.hidden());
        assert!(b.placed(), "{what}: not settled");
        if b.footer {
            assert!(r.contains_rect(b.seen.button), "{what}: the button {:?} is not inside the dialog {r:?}", b.seen.button);
        }
    }

    // spec: NAV-028
    #[test]
    fn nav_028_a_dialog_that_grew_ends_like_a_new_one_with_that_content() {
        for (footer, wrap) in CASES {
            let mut b = B::fresh(3, footer, wrap);
            let before = b.rect();
            b.rows = 12;
            b.frames(10);
            let r = b.rect();
            assert_eq!(r.min, before.min, "footer {footer}, wrap {wrap}: the top-left corner stays");
            assert_eq!(r.width(), before.width());
            same_as_fresh(&b, &B::fresh(12, footer, wrap), &format!("footer {footer}, wrap {wrap}, grown"));
        }
    }

    // spec: NAV-028
    #[test]
    fn nav_028_a_dialog_gets_shorter_with_its_content_and_keeps_its_corner_and_width() {
        for (footer, wrap) in CASES {
            for rows in [1, 3, 6] {
                let mut b = B::fresh(12, footer, wrap);
                let before = b.rect();
                b.rows = rows;
                b.frames(10);
                let r = b.rect();
                assert_eq!(r.min, before.min, "footer {footer}, wrap {wrap}, {rows} rows: the top-left corner stays");
                assert_eq!(r.width(), before.width());
                assert!(r.height() <= before.height(), "{r:?}");
                same_as_fresh(&b, &B::fresh(rows, footer, wrap), &format!("footer {footer}, wrap {wrap}, 12 -> {rows} rows"));
            }
        }
    }

    // spec: NAV-028
    #[test]
    fn nav_028_a_dialog_opened_again_with_less_content_is_as_short_as_a_new_one() {
        for (footer, wrap) in CASES {
            let mut b = B::fresh(12, footer, wrap);
            let before = b.rect();
            b.shown = false;
            b.frames(5);
            b.shown = true;
            b.rows = 3;
            b.frames(10);
            assert_eq!(b.rect().min, before.min, "footer {footer}, wrap {wrap}: it comes back where it was");
            same_as_fresh(&b, &B::fresh(3, footer, wrap), &format!("footer {footer}, wrap {wrap}, opened again with 3 rows"));
        }
    }

    // spec: NAV-028
    #[test]
    fn nav_028_a_new_dialog_keeps_its_size_and_asks_for_no_more_frames_once_placed() {
        for (footer, wrap) in CASES {
            // one row and no row are shorter than egui lets a window be: there is room left that
            // the dialog must not try to get rid of again and again
            for rows in [0, 1, 3, 12] {
                let mut b = B::fresh(rows, footer, wrap);
                assert!(b.placed(), "footer {footer}, wrap {wrap}, {rows} rows");
                let r = b.rect();
                b.repaints = 0;
                for n in 0..20 {
                    b.frame(vec![]);
                    assert_eq!(b.rect(), r, "footer {footer}, wrap {wrap}, {rows} rows: frame {n}");
                }
                assert!(b.placed(), "footer {footer}, wrap {wrap}, {rows} rows");
                assert_eq!(b.repaints, 0, "footer {footer}, wrap {wrap}, {rows} rows: the dialog keeps asking for frames");
            }
        }
    }

    // spec: NAV-013
    #[test]
    fn nav_013_a_dialog_sized_by_hand_keeps_its_size_when_the_content_gets_shorter() {
        for footer in [true, false] {
            let mut b = B::fresh(3, footer, false);
            let r = b.rect();
            let corner = r.max - Vec2::splat(2.0);
            b.drag(corner, corner + Vec2::new(100.0, 200.0));
            let sized = b.rect();
            assert!(sized.height() > r.height() + 150.0, "{r:?} -> {sized:?}");
            // the content is shorter than the dialog all along: nothing moves
            b.frames(10);
            assert_eq!(b.rect(), sized, "footer {footer}");
            // grows and gets shorter again: the size is the person's
            b.rows = 40;
            b.frames(10);
            assert_eq!(b.rect(), sized, "footer {footer}: it does not grow");
            b.rows = 1;
            b.frames(10);
            assert_eq!(b.rect(), sized, "footer {footer}: it does not get shorter");
            // also not after it was closed and opened again
            b.shown = false;
            b.frames(5);
            b.shown = true;
            b.frames(10);
            assert_eq!(b.rect(), sized, "footer {footer}: opened again");
        }
    }

    // spec: NAV-013
    #[test]
    fn nav_013_a_dialog_does_not_get_shorter_while_the_pointer_is_down_on_it() {
        let mut b = B::fresh(12, true, false);
        let h = b.rect().height();
        let p = Pos2::new(b.rect().center().x, b.rect().top() + 10.0);
        let btn = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        b.frame(vec![Event::PointerMoved(p)]);
        b.frame(vec![btn(true)]);
        b.rows = 3;
        b.frames(6);
        assert_eq!(b.rect().height(), h, "it waits for the button to go up");
        b.frame(vec![btn(false)]);
        b.frames(6);
        assert!(b.rect().height() < h - 100.0, "{h} -> {:?}", b.rect());
    }

    #[test]
    fn esc_and_a_click_outside_close_a_modal_dialog() {
        let mut w = W::new(3, true);
        w.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(!w.open);
        let mut w = W::new(3, true);
        let p = Pos2::new(20.0, 20.0);
        let b = |pressed| Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        w.frame(vec![Event::PointerMoved(p)]);
        w.frame(vec![b(true)]);
        w.frame(vec![b(false)]);
        assert!(!w.open);
    }

    #[test]
    fn a_non_modal_dialog_leaves_esc_and_outside_clicks_alone() {
        let mut w = W::new(3, false);
        w.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(w.open);
        let r = w.rect();
        let grab = Pos2::new(r.center().x, r.top() + 10.0);
        w.drag(grab, grab + Vec2::new(50.0, 50.0));
        assert!((w.rect().min - r.min - Vec2::new(50.0, 50.0)).length() < 3.0);
    }
}
