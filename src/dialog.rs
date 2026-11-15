//! Dialog windows. Every dialog in Shale is shown through [`Dialog`], so they all work alike:
//! drag the title bar to move it, drag any edge or corner to resize it, and the content
//! scrolls when the dialog is made smaller than what it holds.
//!
//! A modal dialog dims the rest of the window and blocks it; Esc, the ✕ in the title bar or a
//! click on the dimmed part asks to close it ([`DialogResponse::should_close`]), as before.
//!
//! Size: a dialog opens at its given width and as tall as its content (at most the window
//! height). Until the user resizes it, it grows when its content grows (a form that gets more
//! fields). Position and size are kept while the app runs, so a dialog opened again comes back
//! where it was left.

use eframe::egui::{self, Align2, Color32, Context, Id, Key, Modifiers, Order, Pos2, Rect, ScrollArea, Sense, Ui, Vec2};

/// Space kept free around a dialog that is as large as the window allows.
const MARGIN: f32 = 24.0;
/// Height of the title bar and the window frame, roughly.
const TITLE_ROOM: f32 = 40.0;
/// The smallest a dialog can be made.
const MIN_SIZE: Vec2 = Vec2::new(220.0, 90.0);
/// Frames a new dialog takes to measure its content: egui's invisible sizing pass, one frame
/// to measure, one in which the window takes that size.
const FIT_FRAMES: u8 = 3;

/// Remembered between frames, per dialog.
#[derive(Clone, Copy, Debug)]
enum Sizing {
    /// New: measuring the content. The window follows the content height, centred on the screen.
    Fitting { frames: u8 },
    /// Placed and its content grew past it: one frame of following the content height again,
    /// with the top-left corner kept where it is.
    Growing,
    /// Placed. `auto` until the user resizes it: then it no longer grows by itself.
    Placed { auto: bool, size: Vec2 },
}

pub struct Dialog {
    id: Id,
    title: String,
    width: f32,
    modal: bool,
}

pub struct DialogResponse<T> {
    /// What the content returned; `None` when the window was not shown (only on a hidden frame).
    pub inner: Option<T>,
    close: bool,
}

impl<T> DialogResponse<T> {
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

    pub fn show<T>(self, ctx: &Context, content: impl FnOnce(&mut Ui) -> T) -> DialogResponse<T> {
        let Self { id, title, width, modal } = self;
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
        let measuring = !matches!(sizing, Sizing::Placed { .. });
        if measuring {
            window = window.max_height(max.y);
        }
        if matches!(sizing, Sizing::Fitting { .. }) {
            // centred while it finds its height; from then on it stays where it is put
            window = window.pivot(Align2::CENTER_CENTER).default_pos(screen.center());
        }
        let mut overflow = false;
        let shown = window.show(ctx, |ui| {
            // The content always sits in the same scroll area (so its widgets keep their state).
            // While measuring, the scroll area may be as tall as the screen allows and shrinks to
            // the content (an inner scroll area takes what it is given), so the window takes the
            // content's height; otherwise it fills the window and scrolls.
            let (room, shrink) = if measuring { (Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), (max.y - TITLE_ROOM).max(MIN_SIZE.y))), [false, true]) } else { (ui.available_rect_before_wrap(), [false, false]) };
            ui.scope_builder(egui::UiBuilder::new().max_rect(room), |ui| {
                let out = ScrollArea::vertical().id_salt("dialog-body").auto_shrink(shrink).show(ui, content);
                overflow = out.content_size.y > out.inner_rect.height() + 1.0;
                out.inner
            })
            .inner
        });

        let Some(shown) = shown else {
            return DialogResponse { inner: None, close: !open };
        };
        let rect = shown.response.rect;

        // ---- what to do next frame
        let next = match sizing {
            Sizing::Fitting { frames } if frames + 1 < FIT_FRAMES => Sizing::Fitting { frames: frames + 1 },
            Sizing::Fitting { .. } => {
                // done measuring: pin the top-left corner so a resize does not move the dialog
                pin_left_top(ctx, id, rect.min);
                Sizing::Placed { auto: true, size: rect.size() }
            }
            Sizing::Growing => Sizing::Placed { auto: true, size: rect.size() },
            Sizing::Placed { auto, size } => {
                let resized = (rect.size() - size).length() > 0.5 && ctx.input(|i| i.pointer.any_down());
                let auto = auto && !resized;
                if auto && overflow && rect.height() < max.y - 1.0 {
                    // the content grew and the user has not sized the dialog: grow with it
                    Sizing::Growing
                } else {
                    Sizing::Placed { auto, size: rect.size() }
                }
            }
        };
        ctx.data_mut(|d| d.insert_temp(sizing_key, next));
        if !matches!(next, Sizing::Placed { .. }) {
            ctx.request_repaint();
        }
        let esc = modal && is_top && !any_popup_open && ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape));
        DialogResponse { inner: shown.inner, close: !open || backdrop_clicked || esc }
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
