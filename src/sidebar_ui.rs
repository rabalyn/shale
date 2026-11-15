//! Drawing and dragging of the menubars (LAY-001…LAY-011). The model is in `sidebar.rs`, the
//! decision in ADR-0014.

use crate::app::{muted, LeftPanel, Modal, RightPanel, ShaleApp, View};
use crate::layout::{icon_button_sense, paint_icon};
use crate::sidebar::{Dest, Item, Layout};
use crate::theme;
use eframe::egui::{self, Align, CornerRadius, FontId, Id, LayerId, Layout as UiLayout, Order, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2};

/// The height of a menubar of the right sidebar, and its icons.
const BAR_H: f32 = 30.0;
const ICON: f32 = 26.0;

/// What the window knows about the icons: the layout, a drag in progress, where things were drawn.
#[derive(Default)]
pub struct SideState {
    pub layout: Layout,
    /// the icon being dragged with the mouse
    pub drag: Option<Item>,
    /// a panel shown by a click on its shortcut in the ribbon (LAY-011), below the views
    pub peek: Option<RightPanel>,
    /// where things were drawn this frame (and the frame before the next one)
    pub(crate) ribbon_rect: Option<Rect>,
    pub(crate) ribbon_icons: Vec<(Item, Rect)>,
    pub(crate) bars: Vec<Bar>,
    /// the content of each view and the place a new view would get if an icon is dropped there
    pub(crate) bodies: Vec<(usize, Rect)>,
    pub(crate) right_rect: Option<Rect>,
    /// what dropping now would do (for the marker, and for tests)
    pub(crate) cue: Cue,
}

/// The menubar of one view of the right sidebar, as drawn.
pub(crate) struct Bar {
    pub view: usize,
    pub rect: Rect,
    pub icons: Vec<(Item, Rect)>,
}

/// What the pointer shows while an icon is dragged.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub enum Cue {
    #[default]
    None,
    /// the icon would land in the ribbon with this many icons above it; the marker line is `line`
    Ribbon { at: usize, line: [Pos2; 2] },
    /// the icon would land in the menubar of this view with this many icons before it
    Bar { view: usize, at: usize, line: [Pos2; 2] },
    /// the icon would make a new view at this place of the stack; the zone is highlighted (LAY-009)
    Zone { at: usize, rect: Rect },
    /// the icon cannot live here (LAY-006)
    Refused,
}

impl SideState {
    pub fn new(layout: Layout) -> SideState {
        SideState { layout, ..SideState::default() }
    }

    /// Forget where things were drawn (before a frame draws them again).
    pub(crate) fn clear_geometry(&mut self) {
        self.ribbon_rect = None;
        self.ribbon_icons.clear();
        self.bars.clear();
        self.bodies.clear();
        self.right_rect = None;
    }

    /// What dropping `item` at `pos` would do.
    fn cue_at(&self, item: Item, pos: Pos2) -> Cue {
        if let Some(r) = self.ribbon_rect.filter(|r| r.contains(pos)) {
            let at = self.ribbon_icons.iter().filter(|(_, ir)| ir.center().y < pos.y).count();
            let y = match (at.checked_sub(1).and_then(|i| self.ribbon_icons.get(i)), self.ribbon_icons.get(at)) {
                (Some((_, a)), Some((_, b))) => (a.max.y + b.min.y) / 2.0,
                (None, Some((_, b))) => b.min.y - 2.0,
                (Some((_, a)), None) => a.max.y + 2.0,
                (None, None) => r.min.y,
            };
            return Cue::Ribbon { at, line: [Pos2::new(r.min.x + 6.0, y), Pos2::new(r.max.x - 6.0, y)] };
        }
        if let Some(b) = self.bars.iter().find(|b| b.rect.contains(pos)) {
            if !item.is_panel() {
                return Cue::Refused;
            }
            let at = b.icons.iter().filter(|(_, ir)| ir.center().x < pos.x).count();
            let x = match (at.checked_sub(1).and_then(|i| b.icons.get(i)), b.icons.get(at)) {
                (Some((_, l)), Some((_, r))) => (l.max.x + r.min.x) / 2.0,
                (None, Some((_, r))) => r.min.x - 1.0,
                (Some((_, l)), None) => l.max.x + 1.0,
                (None, None) => b.rect.min.x,
            };
            return Cue::Bar { view: b.view, at, line: [Pos2::new(x, b.rect.min.y + 3.0), Pos2::new(x, b.rect.max.y - 3.0)] };
        }
        let zone = self.bodies.iter().find(|(_, r)| r.contains(pos)).copied().or_else(|| self.right_rect.filter(|r| self.layout.views.is_empty() && r.contains(pos)).map(|r| (0, r)));
        match zone {
            Some(_) if !item.is_panel() => Cue::Refused,
            Some((at, rect)) => Cue::Zone { at, rect },
            None => Cue::None,
        }
    }
}

impl ShaleApp {
    /// Show or close a panel from its shortcut in the ribbon (LAY-011).
    pub(crate) fn toggle_peek(&mut self, panel: RightPanel) {
        if self.side.peek == Some(panel) && self.cfg.show_right {
            self.side.peek = None;
        } else {
            self.side.peek = Some(panel);
            self.cfg.show_right = true;
        }
    }

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
                Item::Import => matches!(self.modal, Some(Modal::Import(_))),
                Item::Panel(p) => self.cfg.show_right && self.side.peek == Some(p),
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

    /// The right sidebar: the views stacked, each with its menubar and its panel (LAY-003), and
    /// the panel of a ribbon shortcut below them (LAY-011).
    pub(crate) fn right_views(&mut self, ui: &mut Ui) {
        let full = ui.available_rect_before_wrap();
        self.side.right_rect = Some(full);
        let views = self.side.layout.views.len();
        let n = views + usize::from(self.side.peek.is_some());
        if n == 0 {
            ui.label(muted("No panel is shown here. Drag a panel icon here, for example from the ribbon."));
            return;
        }
        let h = full.height() / n as f32;
        for i in 0..n {
            let rect = Rect::from_min_size(Pos2::new(full.min.x, full.min.y + h * i as f32), Vec2::new(full.width(), h));
            if i > 0 {
                ui.painter().line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, theme::BORDER));
            }
            let top = if i > 0 { rect.min.y + 3.0 } else { rect.min.y };
            let bar = Rect::from_min_max(Pos2::new(rect.min.x, top), Pos2::new(rect.max.x, top + BAR_H));
            let body = Rect::from_min_max(Pos2::new(rect.min.x, bar.max.y + 2.0), rect.max);
            let mut bar_ui = ui.new_child(UiBuilder::new().max_rect(bar).layout(UiLayout::left_to_right(Align::Center)));
            bar_ui.spacing_mut().item_spacing.x = 2.0;
            let panel = if i < views {
                let (icons, active) = (self.side.layout.views[i].icons.clone(), self.side.layout.views[i].active);
                let mut drawn = vec![];
                for item in icons {
                    let resp = icon_button_sense(&mut bar_ui, item.icon(), ICON, item == active, item.tip(), Sense::click_and_drag());
                    drawn.push((item, resp.rect));
                    if resp.drag_started() {
                        self.side.drag = Some(item);
                    } else if resp.clicked() {
                        self.side.layout.views[i].active = item;
                        self.layout_changed();
                    }
                }
                self.side.bars.push(Bar { view: i, rect: bar, icons: drawn });
                self.side.bodies.push((i + 1, body));
                match active {
                    Item::Panel(p) => p,
                    _ => continue,
                }
            } else {
                // the shortcut's panel: its icon closes it again
                let p = self.side.peek.unwrap_or(RightPanel::Backlinks);
                if icon_button_sense(&mut bar_ui, Item::Panel(p).icon(), ICON, true, "Close this panel", Sense::click()).clicked() {
                    self.side.peek = None;
                }
                bar_ui.label(muted(Item::Panel(p).tip()));
                self.side.bodies.push((n, body));
                p
            };
            let mut body_ui = ui.new_child(UiBuilder::new().max_rect(body));
            body_ui.set_clip_rect(body.intersect(ui.clip_rect()));
            self.right_panel_body(&mut body_ui, panel);
        }
    }

    /// The layout changed: keep it in the settings.
    fn layout_changed(&mut self) {
        self.side.layout.store(&mut self.cfg);
        self.save_config();
    }

    /// While an icon is dragged: the marker, the zone and the dragged icon, Esc, and the drop.
    /// Runs after everything is drawn, so the places are known (LAY-001, LAY-005, LAY-009,
    /// LAY-010).
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
        self.side.cue = pos.map(|p| self.side.cue_at(item, p)).unwrap_or_default();
        if released {
            self.side.drag = None;
            let dest = match self.side.cue {
                Cue::Ribbon { at, .. } => Some(Dest::Ribbon(at)),
                Cue::Bar { view, at, .. } => Some(Dest::Bar { view, at }),
                Cue::Zone { at, .. } => Some(Dest::NewView { at }),
                Cue::Refused | Cue::None => None,
            };
            self.side.cue = Cue::None;
            if let Some(d) = dest {
                if self.side.layout.apply(item, d).is_ok() {
                    if let Item::Panel(p) = item {
                        if self.side.peek == Some(p) {
                            self.side.peek = None;
                        }
                    }
                    self.layout_changed();
                }
            }
            return;
        }
        let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::unique("side-drag")));
        match self.side.cue {
            Cue::Ribbon { line, .. } | Cue::Bar { line, .. } => {
                painter.line_segment(line, Stroke::new(2.0, theme::ACCENT));
            }
            Cue::Zone { rect, .. } => {
                painter.rect_filled(rect, CornerRadius::same(6), theme::ACCENT.gamma_multiply(0.22));
                painter.rect_stroke(rect.shrink(2.0), CornerRadius::same(6), Stroke::new(2.0, theme::ACCENT), StrokeKind::Inside);
                let galley = painter.layout("Drop to open this icon in a new view".into(), FontId::proportional(13.0), theme::TEXT_STRONG, rect.width() - 24.0);
                painter.galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT_STRONG);
            }
            Cue::Refused | Cue::None => {}
        }
        if let Some(p) = pos {
            let refused = self.side.cue == Cue::Refused;
            let r = Rect::from_center_size(p + Vec2::new(16.0, 16.0), Vec2::splat(26.0));
            painter.rect_filled(r, CornerRadius::same(5), theme::BG_ACTIVE);
            paint_icon(&painter, r.shrink(5.0), item.icon(), if refused { theme::LINK_MISSING } else { theme::ACCENT });
            if refused {
                painter.circle_stroke(r.center(), 13.0, Stroke::new(2.0, theme::LINK_MISSING));
                painter.line_segment([r.center() + Vec2::new(-9.0, 9.0), r.center() + Vec2::new(9.0, -9.0)], Stroke::new(2.0, theme::LINK_MISSING));
            }
            ctx.set_cursor_icon(if refused { egui::CursorIcon::NoDrop } else { egui::CursorIcon::Grabbing });
        }
    }
}
