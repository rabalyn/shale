//! Drawing and dragging of the menubars (LAY-001…). The model is in `sidebar.rs`.

use crate::app::{LeftPanel, ShaleApp, View};
use crate::layout::{icon_button_sense, paint_icon};
use crate::sidebar::{Item, Layout};
use crate::theme;
use eframe::egui::{self, CornerRadius, Id, LayerId, Order, Pos2, Rect, Sense, Stroke, Ui, Vec2};

/// What the window knows about the icons: the layout, a drag in progress, where things were drawn.
#[derive(Default)]
pub struct SideState {
    pub layout: Layout,
    /// the icon being dragged with the mouse
    pub drag: Option<Item>,
    /// where the ribbon and its icons were drawn this frame (and the frame before the next one)
    pub(crate) ribbon_rect: Option<Rect>,
    pub(crate) ribbon_icons: Vec<(Item, Rect)>,
    /// what dropping now would do (for the marker, and for tests)
    pub(crate) cue: Cue,
}

/// What the pointer shows while an icon is dragged.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub enum Cue {
    #[default]
    None,
    /// the icon would land with this many icons above it; the marker line is `line`
    Ribbon { at: usize, line: [Pos2; 2] },
}

impl SideState {
    pub fn new(layout: Layout) -> SideState {
        SideState { layout, ..SideState::default() }
    }

    /// Forget where things were drawn (before a frame draws them again).
    pub(crate) fn clear_geometry(&mut self) {
        self.ribbon_rect = None;
        self.ribbon_icons.clear();
    }

    /// What dropping at `pos` would do.
    fn cue_at(&self, pos: Pos2) -> Cue {
        let Some(r) = self.ribbon_rect else { return Cue::None };
        if !r.contains(pos) {
            return Cue::None;
        }
        let at = self.ribbon_icons.iter().filter(|(_, ir)| ir.center().y < pos.y).count();
        let y = match (at.checked_sub(1).and_then(|i| self.ribbon_icons.get(i)), self.ribbon_icons.get(at)) {
            (Some((_, a)), Some((_, b))) => (a.max.y + b.min.y) / 2.0,
            (None, Some((_, b))) => b.min.y - 2.0,
            (Some((_, a)), None) => a.max.y + 2.0,
            (None, None) => r.min.y,
        };
        Cue::Ribbon { at, line: [Pos2::new(r.min.x + 6.0, y), Pos2::new(r.max.x - 6.0, y)] }
    }
}

impl ShaleApp {
    /// The icons of the ribbon between the two sidebar toggles and Settings (LAY-001).
    pub(crate) fn ribbon_items(&mut self, ui: &mut Ui) {
        self.side.ribbon_rect = Some(ui.max_rect().expand2(Vec2::new(6.0, 8.0)));
        let cur = self.tabs.get(self.active).map(|t| t.view.clone());
        for item in self.side.layout.ribbon.clone() {
            let active = match item {
                Item::Files => self.cfg.show_left && self.left == LeftPanel::Files,
                Item::Search => self.cfg.show_left && self.left == LeftPanel::Search,
                Item::Tags => self.cfg.show_left && self.left == LeftPanel::Tags,
                Item::Graph => cur == Some(View::Graph),
                Item::Calendar => cur == Some(View::Calendar),
                Item::Mail => cur == Some(View::Mail),
                Item::Tasks => cur == Some(View::Tasks),
                Item::Social => matches!(cur, Some(View::Social(_))),
                Item::Import => matches!(self.modal, Some(crate::app::Modal::Import(_))),
                Item::Switcher | Item::Drawing | Item::NewNote => false,
            };
            let resp = icon_button_sense(ui, item.icon(), 30.0, active, item.tip(), Sense::click_and_drag());
            self.side.ribbon_icons.push((item, resp.rect));
            if resp.drag_started() {
                self.side.drag = Some(item);
            } else if resp.clicked() {
                self.ribbon_click(item);
            }
        }
    }

    /// While an icon is dragged: the marker and the dragged icon, Esc, and the drop. Runs after
    /// everything is drawn, so the places are known (LAY-001).
    pub(crate) fn side_drag_tick(&mut self, ui: &mut Ui) {
        let Some(item) = self.side.drag else { return };
        let ctx = ui.ctx().clone();
        let (released, down, esc) = ctx.input_mut(|i| (i.pointer.any_released(), i.pointer.any_down(), i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)));
        if esc || (!released && !down) {
            self.side.drag = None;
            self.side.cue = Cue::None;
            return;
        }
        let pos = ctx.pointer_latest_pos();
        self.side.cue = pos.map(|p| self.side.cue_at(p)).unwrap_or_default();
        if released {
            self.side.drag = None;
            if let Cue::Ribbon { at, .. } = self.side.cue {
                self.side.layout.move_in_ribbon(item, at);
                self.side.layout.store(&mut self.cfg);
                self.save_config();
            }
            self.side.cue = Cue::None;
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("side-drag")));
        if let Cue::Ribbon { line, .. } = self.side.cue {
            painter.line_segment(line, Stroke::new(2.0, theme::ACCENT));
        }
        if let Some(p) = pos {
            let r = Rect::from_center_size(p + Vec2::new(16.0, 16.0), Vec2::splat(26.0));
            painter.rect_filled(r, CornerRadius::same(5), theme::BG_ACTIVE);
            paint_icon(&painter, r.shrink(5.0), item.icon(), theme::ACCENT);
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        }
    }
}
