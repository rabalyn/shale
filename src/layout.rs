//! Window layout: ribbon, sidebars, tab bar, the note/drawing/tasks/graph views and status bar.

use crate::app::panes::{drag_weights, pane_widths, BORDER_W, MIN_PANE_W};
use crate::app::{graph_click, handle_editor_action, handle_reader_action, muted, Action, LeftPanel, Modal, ShaleApp, TabMarks, View};
use crate::markdown;
use crate::reader;
use crate::sidebar::Item;
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Icon {
    Files,
    Search,
    Graph,
    Tasks,
    Drawing,
    Switcher,
    Settings,
    Tags,
    NewNote,
    NewFolder,
    Collapse,
    Back,
    Forward,
    Book,
    Pencil,
    Preview,
    Close,
    Pin,
    Plus,
    SidebarLeft,
    SidebarRight,
    Sort,
    Calendar,
    Import,
    Mail,
    Social,
    Backlinks,
    Outgoing,
    Outline,
    LocalGraph,
    CalendarPanel,
}

pub fn paint_icon(p: &egui::Painter, r: Rect, icon: Icon, c: Color32) {
    let s = Stroke::new(1.5, c);
    let cx = r.center();
    let w = r.width();
    match icon {
        Icon::Files => {
            let b = Rect::from_center_size(cx + Vec2::new(0.0, 1.0), Vec2::new(w * 0.8, w * 0.6));
            p.add(Shape::closed_line(vec![b.left_top(), b.left_top() + Vec2::new(w * 0.28, 0.0), b.left_top() + Vec2::new(w * 0.36, w * 0.1), b.right_top() + Vec2::new(0.0, w * 0.1), b.right_bottom(), b.left_bottom()], s));
        }
        Icon::Search => {
            p.circle_stroke(cx - Vec2::splat(w * 0.08), w * 0.26, s);
            p.line_segment([cx + Vec2::splat(w * 0.12), cx + Vec2::splat(w * 0.36)], s);
        }
        Icon::Graph => {
            let a = cx + Vec2::new(-w * 0.3, w * 0.2);
            let b = cx + Vec2::new(w * 0.3, w * 0.25);
            let c2 = cx + Vec2::new(w * 0.05, -w * 0.3);
            p.line_segment([a, c2], s);
            p.line_segment([b, c2], s);
            p.line_segment([a, b], s);
            for q in [a, b, c2] {
                p.circle_filled(q, w * 0.1, c);
            }
        }
        Icon::LocalGraph => {
            // the graph around one note: a ring of neighbours round the note in the middle
            p.circle_stroke(cx, w * 0.34, Stroke::new(1.0, c.gamma_multiply(0.55)));
            for k in 0..3 {
                let a = std::f32::consts::TAU * (k as f32 / 3.0 + 0.08);
                let q = cx + Vec2::angled(a) * (w * 0.34);
                p.line_segment([cx, q], s);
                p.circle_filled(q, w * 0.07, c);
            }
            p.circle_filled(cx, w * 0.14, c);
        }
        Icon::CalendarPanel => {
            // a month grid with today's cell filled
            let b = Rect::from_center_size(cx, Vec2::splat(w * 0.66));
            p.rect_stroke(b, CornerRadius::same(2), s, StrokeKind::Middle);
            for k in [1.0f32 / 3.0, 2.0 / 3.0] {
                p.line_segment([Pos2::new(b.min.x + b.width() * k, b.min.y), Pos2::new(b.min.x + b.width() * k, b.max.y)], Stroke::new(1.0, c.gamma_multiply(0.55)));
                p.line_segment([Pos2::new(b.min.x, b.min.y + b.height() * k), Pos2::new(b.max.x, b.min.y + b.height() * k)], Stroke::new(1.0, c.gamma_multiply(0.55)));
            }
            p.rect_filled(Rect::from_min_size(Pos2::new(b.min.x + b.width() / 3.0, b.min.y + b.height() / 3.0), b.size() / 3.0), CornerRadius::ZERO, c);
        }
        Icon::Tasks | Icon::Tags => {
            let b = Rect::from_center_size(cx, Vec2::splat(w * 0.66));
            p.rect_stroke(b, CornerRadius::same(3), s, StrokeKind::Middle);
            if matches!(icon, Icon::Tasks) {
                p.add(Shape::line(vec![b.left_center() + Vec2::new(w * 0.12, 0.0), b.center_bottom() - Vec2::new(w * 0.05, w * 0.16), b.right_top() + Vec2::new(-w * 0.12, w * 0.14)], s));
            } else {
                p.text(cx, Align2::CENTER_CENTER, "#", FontId::proportional(w * 0.6), c);
            }
        }
        Icon::Drawing | Icon::Pencil => {
            let a = cx + Vec2::new(-w * 0.3, w * 0.3);
            let b = cx + Vec2::new(w * 0.25, -w * 0.25);
            let n = Vec2::new(w * 0.09, w * 0.09);
            p.add(Shape::closed_line(
                vec![a, a + Vec2::new(n.x * 1.2, -n.y * 0.2) + Vec2::new(0.0, -n.y * 1.4), b - Vec2::new(n.x, 0.0) + Vec2::new(0.0, -n.y * 0.0), b, b + Vec2::new(n.x * 0.2, n.y * 1.2), a + Vec2::new(n.x * 1.6, 0.0)],
                s,
            ));
            if matches!(icon, Icon::Drawing) {
                p.line_segment([cx + Vec2::new(-w * 0.35, w * 0.42), cx + Vec2::new(w * 0.35, w * 0.42)], Stroke::new(1.0, c.gamma_multiply(0.6)));
            }
        }
        Icon::Switcher | Icon::NewNote => {
            let b = Rect::from_center_size(cx, Vec2::new(w * 0.56, w * 0.72));
            p.add(Shape::closed_line(vec![b.left_top(), b.right_top() - Vec2::new(w * 0.16, 0.0), b.right_top() + Vec2::new(0.0, w * 0.16), b.right_bottom(), b.left_bottom()], s));
            if matches!(icon, Icon::NewNote) {
                p.line_segment([cx - Vec2::new(w * 0.12, -w * 0.08), cx + Vec2::new(w * 0.12, w * 0.08)], s);
                p.line_segment([cx + Vec2::new(0.0, -w * 0.04), cx + Vec2::new(0.0, w * 0.2)], s);
            } else {
                for k in 0..3 {
                    let y = b.min.y + w * 0.25 + k as f32 * w * 0.14;
                    p.line_segment([Pos2::new(b.min.x + w * 0.1, y), Pos2::new(b.max.x - w * 0.1, y)], Stroke::new(1.0, c));
                }
            }
        }
        Icon::NewFolder => {
            paint_icon(p, r, Icon::Files, c);
            p.line_segment([cx + Vec2::new(-w * 0.12, w * 0.08), cx + Vec2::new(w * 0.12, w * 0.08)], s);
            p.line_segment([cx + Vec2::new(0.0, -w * 0.04), cx + Vec2::new(0.0, w * 0.2)], s);
        }
        Icon::Settings => {
            p.circle_stroke(cx, w * 0.16, s);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::TAU / 8.0;
                let d = Vec2::angled(a);
                p.line_segment([cx + d * w * 0.26, cx + d * w * 0.38], Stroke::new(2.2, c));
            }
            p.circle_stroke(cx, w * 0.27, Stroke::new(1.2, c));
        }
        Icon::Collapse => {
            for dy in [-0.15f32, 0.15] {
                let y = cx.y + dy * w;
                p.add(Shape::line(vec![Pos2::new(cx.x - w * 0.25, y + w * 0.12 * dy.signum()), Pos2::new(cx.x, y - w * 0.0), Pos2::new(cx.x + w * 0.25, y + w * 0.12 * dy.signum())], s));
            }
        }
        Icon::Back | Icon::Forward => {
            let d = if matches!(icon, Icon::Back) { -1.0 } else { 1.0 };
            p.add(Shape::line(vec![cx + Vec2::new(-d * w * 0.12, -w * 0.25), cx + Vec2::new(d * w * 0.13, 0.0), cx + Vec2::new(-d * w * 0.12, w * 0.25)], s));
        }
        Icon::Book => {
            let l = Rect::from_min_max(cx + Vec2::new(-w * 0.38, -w * 0.28), cx + Vec2::new(0.0, w * 0.3));
            let rr = Rect::from_min_max(cx + Vec2::new(0.0, -w * 0.28), cx + Vec2::new(w * 0.38, w * 0.3));
            p.rect_stroke(l, CornerRadius::same(1), s, StrokeKind::Middle);
            p.rect_stroke(rr, CornerRadius::same(1), s, StrokeKind::Middle);
        }
        Icon::Preview => {
            // an eye: the markdown preview
            let pts = |sign: f32| {
                (0..=8).map(move |k| {
                    let t = k as f32 / 8.0 * 2.0 - 1.0;
                    cx + Vec2::new(t * w * 0.4, sign * (1.0 - t * t) * w * 0.24)
                })
            };
            p.add(Shape::line(pts(-1.0).collect(), s));
            p.add(Shape::line(pts(1.0).collect(), s));
            p.circle_filled(cx, w * 0.11, c);
        }
        Icon::Close => {
            p.line_segment([cx - Vec2::splat(w * 0.22), cx + Vec2::splat(w * 0.22)], s);
            p.line_segment([cx + Vec2::new(-w * 0.22, w * 0.22), cx + Vec2::new(w * 0.22, -w * 0.22)], s);
        }
        Icon::Pin => {
            // a drawing pin: the head, the body and the needle
            let head = Rect::from_center_size(cx + Vec2::new(0.0, -w * 0.18), Vec2::new(w * 0.5, w * 0.34));
            p.rect_stroke(head, CornerRadius::same(2), s, egui::StrokeKind::Middle);
            p.line_segment([cx + Vec2::new(0.0, w * 0.0), cx + Vec2::new(0.0, w * 0.36)], s);
        }
        Icon::Plus => {
            p.line_segment([cx - Vec2::new(w * 0.28, 0.0), cx + Vec2::new(w * 0.28, 0.0)], s);
            p.line_segment([cx - Vec2::new(0.0, w * 0.28), cx + Vec2::new(0.0, w * 0.28)], s);
        }
        Icon::Calendar => {
            let b = Rect::from_center_size(cx + Vec2::new(0.0, 1.0), Vec2::new(w * 0.72, w * 0.62));
            p.rect_stroke(b, CornerRadius::same(2), s, StrokeKind::Middle);
            p.line_segment([Pos2::new(b.min.x, b.min.y + w * 0.16), Pos2::new(b.max.x, b.min.y + w * 0.16)], s);
            for k in [0.3f32, 0.7] {
                let x = b.min.x + b.width() * k;
                p.line_segment([Pos2::new(x, b.min.y - w * 0.08), Pos2::new(x, b.min.y + w * 0.06)], s);
            }
            for (i, j) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1)] {
                p.circle_filled(Pos2::new(b.min.x + b.width() * (0.25 + 0.25 * i as f32), b.min.y + w * 0.3 + w * 0.15 * j as f32), w * 0.04, c);
            }
        }
        Icon::Import => {
            // an arrow down into a tray
            let top = cx.y - w * 0.32;
            let tip = cx.y + w * 0.08;
            p.line_segment([Pos2::new(cx.x, top), Pos2::new(cx.x, tip)], s);
            p.line_segment([Pos2::new(cx.x - w * 0.16, tip - w * 0.16), Pos2::new(cx.x, tip)], s);
            p.line_segment([Pos2::new(cx.x + w * 0.16, tip - w * 0.16), Pos2::new(cx.x, tip)], s);
            let y = cx.y + w * 0.06;
            let b = cx.y + w * 0.3;
            p.add(Shape::line(vec![Pos2::new(cx.x - w * 0.34, y), Pos2::new(cx.x - w * 0.34, b), Pos2::new(cx.x + w * 0.34, b), Pos2::new(cx.x + w * 0.34, y)], s));
        }
        Icon::Mail => {
            // an envelope
            let b = Rect::from_center_size(cx, Vec2::new(w * 0.72, w * 0.52));
            p.rect_stroke(b, CornerRadius::same(2), s, StrokeKind::Middle);
            p.add(Shape::line(vec![b.left_top(), cx + Vec2::new(0.0, w * 0.06), b.right_top()], s));
        }
        Icon::Social => {
            // a speech bubble with two lines of text
            let b = Rect::from_center_size(cx - Vec2::new(0.0, w * 0.07), Vec2::new(w * 0.8, w * 0.56));
            let tail = b.min.x + w * 0.14;
            p.add(Shape::closed_line(vec![b.left_top(), b.right_top(), b.right_bottom(), Pos2::new(tail + w * 0.26, b.max.y), Pos2::new(tail, b.max.y + w * 0.2), Pos2::new(tail, b.max.y), b.left_bottom()], s));
            for (k, len) in [0.46f32, 0.3].iter().enumerate() {
                let y = b.min.y + w * 0.17 + k as f32 * w * 0.16;
                p.line_segment([Pos2::new(b.min.x + w * 0.14, y), Pos2::new(b.min.x + w * 0.14 + w * len, y)], Stroke::new(1.0, c));
            }
        }
        Icon::Backlinks | Icon::Outgoing => {
            // a line with an arrow head: into the note (backlinks) or out of it (outgoing links)
            let (a, b) = if matches!(icon, Icon::Backlinks) { (cx + Vec2::new(w * 0.34, 0.0), cx + Vec2::new(-w * 0.34, 0.0)) } else { (cx + Vec2::new(-w * 0.34, 0.0), cx + Vec2::new(w * 0.34, 0.0)) };
            p.line_segment([a, b], s);
            let back = if b.x > a.x { -1.0 } else { 1.0 };
            p.line_segment([b, b + Vec2::new(back * w * 0.2, -w * 0.2)], s);
            p.line_segment([b, b + Vec2::new(back * w * 0.2, w * 0.2)], s);
            p.circle_filled(a, w * 0.08, c);
        }
        Icon::Outline => {
            // indented lines
            for (k, (x, len)) in [(0.0f32, 0.62f32), (0.16, 0.46), (0.16, 0.46), (0.0, 0.62)].iter().enumerate() {
                let y = cx.y + (k as f32 - 1.5) * w * 0.22;
                p.line_segment([Pos2::new(cx.x - w * 0.31 + w * x, y), Pos2::new(cx.x - w * 0.31 + w * (x + len), y)], s);
            }
        }
        Icon::Sort => {
            for (k, len) in [0.62f32, 0.44, 0.26].iter().enumerate() {
                let y = cx.y + (k as f32 - 1.0) * w * 0.24;
                p.line_segment([Pos2::new(cx.x - w * 0.31, y), Pos2::new(cx.x - w * 0.31 + w * len, y)], s);
            }
        }
        Icon::SidebarLeft | Icon::SidebarRight => {
            let b = Rect::from_center_size(cx, Vec2::new(w * 0.72, w * 0.6));
            p.rect_stroke(b, CornerRadius::same(2), s, StrokeKind::Middle);
            let x = if matches!(icon, Icon::SidebarLeft) { b.min.x + w * 0.24 } else { b.max.x - w * 0.24 };
            p.line_segment([Pos2::new(x, b.min.y), Pos2::new(x, b.max.y)], s);
        }
    }
}

/// A small square icon button. Returns its response.
pub fn icon_button(ui: &mut Ui, icon: Icon, size: f32, active: bool, tip: &str) -> egui::Response {
    icon_button_sense(ui, icon, size, active, tip, Sense::click())
}

/// `icon_button` with another kind of mouse handling (the menubar icons can be dragged).
pub fn icon_button_sense(ui: &mut Ui, icon: Icon, size: f32, active: bool, tip: &str, sense: Sense) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), sense);
    let hovered = resp.hovered();
    if active {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_ACTIVE);
    } else if hovered {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_HOVER);
    }
    let c = if active {
        theme::ACCENT
    } else if hovered {
        theme::TEXT_STRONG
    } else {
        theme::MUTED
    };
    paint_icon(ui.painter(), r.shrink(size * 0.22), icon, c);
    crate::script::hint(tip, r);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip)
}

impl ShaleApp {
    pub fn welcome(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG_PRIMARY)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(ui.available_height() * 0.18);
                ui.label(egui::RichText::new("Shale").size(40.0).color(theme::TEXT_STRONG));
                ui.label(muted("A native notes vault: plain markdown files, linked together."));
                ui.add_space(28.0);
                egui::Frame::new().fill(theme::BG_SECONDARY).corner_radius(CornerRadius::same(10)).inner_margin(20).show(ui, |ui| {
                    ui.set_width(520.0);
                    ui.label(egui::RichText::new("Open a folder as vault").size(16.0).color(theme::TEXT));
                    ui.label(muted("Any folder works, including an existing markdown vault. Type its path:"));
                    ui.add_space(6.0);
                    ui.add(egui::TextEdit::singleline(&mut self.welcome_path).desired_width(f32::INFINITY));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        let path = PathBuf::from(shellexpand(&self.welcome_path));
                        if ui.button("Open folder").clicked() {
                            if path.is_dir() {
                                self.actions.push(Action::OpenVault(path.clone()));
                            } else {
                                self.toast("That folder doesn't exist yet. Use “Create vault”.");
                            }
                        }
                        if ui.button("Create vault with sample notes").clicked() {
                            match std::fs::create_dir_all(&path).and_then(|()| crate::vault::write_sample_vault(&path)) {
                                Ok(()) => self.actions.push(Action::OpenVault(path.clone())),
                                Err(e) => self.toast(format!("Could not create the vault: {e}")),
                            }
                        }
                        if ui.button("Create empty vault").clicked() {
                            match std::fs::create_dir_all(&path) {
                                Ok(()) => self.actions.push(Action::OpenVault(path.clone())),
                                Err(e) => self.toast(format!("Could not create the vault: {e}")),
                            }
                        }
                    });
                    if !self.cfg.recent.is_empty() {
                        ui.add_space(12.0);
                        ui.label(muted("Recent vaults"));
                        for r in self.cfg.recent.clone() {
                            if ui.link(r.to_string_lossy()).clicked() {
                                self.actions.push(Action::OpenVault(r));
                            }
                        }
                    }
                });
            });
        });
    }

    /// Stores the width a sidebar was drawn at when a drag of its border ended (LAY-018).
    fn keep_width(&mut self, ui: &Ui, drawn: f32, left: bool) {
        let (stored, default) = if left { (self.cfg.left_sidebar_width, LEFT_WIDTH) } else { (self.cfg.right_sidebar_width, RIGHT_WIDTH) };
        if ui.ctx().input(|i| i.pointer.any_released()) && (drawn - stored.map_or(default, |w| w as f32)).abs() >= 1.0 {
            let width = Some(drawn.round() as u32);
            if left {
                self.cfg.left_sidebar_width = width;
            } else {
                self.cfg.right_sidebar_width = width;
            }
            self.save_config();
        }
    }

    pub fn layout(&mut self, ui: &mut Ui) {
        self.side.clear_geometry();
        // ribbon
        egui::Panel::left("ribbon").exact_size(44.0).resizable(false).frame(egui::Frame::new().fill(theme::BG_RIBBON).inner_margin(egui::Margin::symmetric(6, 8)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
            self.ribbon(ui);
        });
        // status bar
        egui::Panel::bottom("status").exact_size(24.0).resizable(false).frame(egui::Frame::new().fill(theme::BG_SECONDARY).inner_margin(egui::Margin::symmetric(10, 3)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
            self.status_bar(ui);
        });
        if self.cfg.show_left {
            let left = sidebar_width(self.cfg.left_sidebar_width, LEFT_WIDTH, LEFT_RANGE);
            let panel = egui::Panel::left("left-sidebar").default_size(left).size_range(LEFT_RANGE);
            let panel = if std::mem::take(&mut self.side.reset_widths[0]) { panel.exact_size(LEFT_WIDTH) } else { panel };
            let shown = panel.frame(egui::Frame::new().fill(theme::BG_SECONDARY).inner_margin(egui::Margin::symmetric(8, 8)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
                self.left_sidebar(ui);
            });
            self.keep_width(ui, shown.response.rect.width(), true);
        }
        if self.cfg.show_right {
            let right = sidebar_width(self.cfg.right_sidebar_width, RIGHT_WIDTH, RIGHT_RANGE);
            let panel = egui::Panel::right("right-sidebar").default_size(right).size_range(RIGHT_RANGE);
            let panel = if std::mem::take(&mut self.side.reset_widths[1]) { panel.exact_size(RIGHT_WIDTH) } else { panel };
            let shown = panel.frame(egui::Frame::new().fill(theme::BG_SECONDARY).inner_margin(egui::Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
                self.right_views(ui);
            });
            self.keep_width(ui, shown.response.rect.width(), false);
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG_PRIMARY)).show(ui, |ui| {
            self.panes_ui(ui);
        });
        self.side_drag_tick(ui);
    }

    /// The middle area: one pane, or several side by side with a draggable border between
    /// every two (NAV-032, NAV-034). Each pane is drawn with its tab bar in `self.tabbar`, so
    /// the drawing code does not know about panes; a press inside a pane focuses it.
    fn panes_ui(&mut self, ui: &mut Ui) {
        let n = self.panes.len();
        self.panes.rects.clear();
        self.panes.borders.clear();
        if n == 1 {
            self.tab_bar(ui);
            self.view_header(ui);
            self.content(ui);
            return;
        }
        let area = ui.available_rect_before_wrap();
        let total = area.width() - BORDER_W * (n - 1) as f32;
        let widths = pane_widths(&self.panes.weights, total, MIN_PANE_W);
        let pressed = ui.input(|i| if i.pointer.any_pressed() { i.pointer.interact_pos() } else { None });
        let mut clicked = None;
        let mut x = area.min.x;
        for (i, w) in widths.iter().enumerate() {
            let rect = Rect::from_min_size(Pos2::new(x, area.min.y), Vec2::new(*w, area.height()));
            self.panes.rects.push(rect);
            x += w + BORDER_W;
            self.enter_pane(i);
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("pane", i)).layout(egui::Layout::top_down(egui::Align::Min)));
            child.set_clip_rect(rect.intersect(ui.clip_rect()));
            self.tab_bar(&mut child);
            self.view_header(&mut child);
            self.content(&mut child);
            if pressed.is_some_and(|p| rect.contains(p)) {
                clicked = Some(i);
            }
        }
        self.enter_pane(self.panes.focused);
        ui.allocate_rect(area, Sense::hover());
        // the borders come last, so that they win where a pane reaches them
        for i in 1..n {
            let border = Rect::from_min_max(Pos2::new(self.panes.rects[i].min.x - BORDER_W, area.min.y), Pos2::new(self.panes.rects[i].min.x, area.max.y));
            self.panes.borders.push(border);
            let resp = ui.interact(border, ui.scope_id().with(("pane-border", i)), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            let line = if resp.hovered() || resp.dragged() { theme::ACCENT } else { theme::BORDER };
            ui.painter().rect_filled(Rect::from_center_size(border.center(), Vec2::new(1.0, border.height())), CornerRadius::ZERO, line);
            if resp.dragged() {
                self.panes.weights = drag_weights(&self.panes.weights, i, resp.drag_delta().x, total, MIN_PANE_W);
            }
        }
        if let Some(i) = clicked {
            self.focus_pane(i);
        }
    }

    fn ribbon(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.vertical_centered(|ui| {
            let badges = crate::key_badges::Badges::of_frame(&self.keymap, ui);
            let left = icon_button(ui, Icon::SidebarLeft, 30.0, false, "Toggle left sidebar");
            badges.paint(ui, left.rect, crate::keys::Action::SidebarLeft);
            if left.clicked() {
                self.toggle_left_sidebar();
            }
            ui.add_space(4.0);
            self.ribbon_items(ui);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Settings, 30.0, false, "Settings and vault (Ctrl+,)").clicked() {
                    self.open_settings(crate::settings::last_tab(ui.ctx()));
                }
                let right = icon_button(ui, Icon::SidebarRight, 30.0, false, "Toggle right sidebar");
                badges.paint(ui, right.rect, crate::keys::Action::SidebarRight);
                if right.clicked() {
                    self.toggle_right_sidebar();
                }
            });
        });
    }

    /// What a click on a ribbon icon does.
    pub(crate) fn ribbon_click(&mut self, item: Item) {
        match item {
            Item::Files => {
                self.side.left = LeftPanel::Files;
                self.cfg.show_left = true;
                self.focus_editor_if_note();
            }
            Item::Search => {
                self.side.left = LeftPanel::Search;
                self.cfg.show_left = true;
                self.search.focus = true;
            }
            Item::Tags => {
                self.side.left = LeftPanel::Tags;
                self.cfg.show_left = true;
                self.focus_editor_if_note();
            }
            Item::Switcher => self.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 }),
            Item::Graph => self.actions.push(Action::ShowView(View::Graph)),
            Item::Calendar => self.actions.push(Action::ShowView(View::Calendar)),
            Item::Mail => self.actions.push(Action::ShowView(View::Mail)),
            Item::Tasks => self.actions.push(Action::ShowView(View::Tasks)),
            Item::Social => self.actions.push(Action::ShowView(View::Social(None))),
            Item::Import => self.open_import(vec![], vec![], None),
            Item::Drawing => {
                let f = self.current_folder();
                self.actions.push(Action::NewDrawing { folder: f, embed_into: None });
            }
            Item::NewNote => {
                let f = self.current_folder();
                self.actions.push(Action::NewNote { folder: f });
            }
            Item::Panel(p) => {
                self.toggle_peek(p);
                self.focus_editor_if_note();
            }
        }
    }

    /// The Alt keys to show on the buttons of the focused pane while Alt is held (KEY-016).
    pub(crate) fn key_badges(&self, ui: &Ui) -> crate::key_badges::Badges {
        if self.panes.drawn() == self.panes.focused {
            crate::key_badges::Badges::of_frame(&self.keymap, ui)
        } else {
            Default::default()
        }
    }

    pub(crate) fn tab_title(&self, view: &View) -> String {
        match view {
            View::Empty => "New tab".into(),
            View::Tasks => "Tasks".into(),
            View::Graph => "Graph".into(),
            View::Calendar => "Calendar".into(),
            View::Mail => "Mail".into(),
            View::SettingsFile => "Settings file".into(),
            View::Social(f) => self.social_title(f.as_ref()),
            View::Note(p) | View::Drawing(p) => self.vault.as_ref().and_then(|v| v.file(p).map(|f| f.title())).unwrap_or_else(|| rel_to_string(p)),
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let h = 36.0;
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, theme::BG_SECONDARY);
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, theme::BORDER));
        // the focused one of several panes has an accent line under its tab bar (NAV-032)
        let focus_line = (self.panes.len() > 1 && self.panes.drawn() == self.panes.focused).then(|| Rect::from_min_max(Pos2::new(bar.min.x, bar.max.y - 2.0), bar.max));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Max)));
        child.spacing_mut().item_spacing.x = 2.0;
        let n = self.tabbar.tabs.len().max(1);
        let tab_w = ((bar.width() - 50.0) / n as f32).clamp(60.0, 220.0);
        let mut close = None;
        self.tabbar.tab_rects.clear();
        self.tabbar.tab_marks = TabMarks { focus_line, ..TabMarks::default() };
        let mut dragging = None;
        let mut dropped = None;
        let mut pin_toggle = None;
        let mut split = None;
        for i in 0..self.tabbar.tabs.len() {
            let title = self.tab_title(&self.tabbar.tabs[i].view);
            let dirty = self.tabbar.tabs[i].view.path().map(|p| self.notes.get(p).map(|d| d.dirty).unwrap_or(false) || self.drawings.get(p).map(|d| d.dirty).unwrap_or(false)).unwrap_or(false);
            let (r, resp) = child.allocate_exact_size(Vec2::new(tab_w, h - 6.0), Sense::click_and_drag());
            let active = i == self.tabbar.active;
            let fill = if active {
                theme::BG_PRIMARY
            } else if resp.hovered() {
                theme::BG_HOVER
            } else {
                theme::BG_SECONDARY
            };
            child.painter().rect_filled(r, CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, fill);
            let mut text_rect = r.shrink2(Vec2::new(10.0, 0.0));
            if self.tabbar.tabs[i].pinned {
                let pin = Rect::from_center_size(Pos2::new(text_rect.min.x + 5.0, r.center().y), Vec2::splat(12.0));
                paint_icon(child.painter(), pin, Icon::Pin, if active { theme::TEXT_STRONG } else { theme::MUTED });
                text_rect.min.x += 14.0;
                self.tabbar.tab_marks.pinned.push(i);
            }
            let galley = child.painter().layout_no_wrap(title.clone(), FontId::proportional(13.0), if active { theme::TEXT_STRONG } else { theme::MUTED });
            child.painter().with_clip_rect(Rect::from_min_max(text_rect.min, Pos2::new(text_rect.max.x - 18.0, text_rect.max.y))).galley(Pos2::new(text_rect.min.x, r.center().y - galley.size().y / 2.0), galley, theme::TEXT);
            let cr = Rect::from_center_size(Pos2::new(r.max.x - 14.0, r.center().y), Vec2::splat(16.0));
            let close_resp = child.interact(cr, child.scope_id().with(("tab-close", self.tabbar.tabs[i].id)), Sense::click());
            self.tabbar.tab_rects.push((r, cr));
            crate::script::hint(&format!("Tab {}", i + 1), r);
            let failed = self.tabbar.tabs[i].view.path().map(|p| self.notes.get(p).map(|d| d.save_error.is_some()).unwrap_or(false) || self.drawings.get(p).map(|d| d.save_error.is_some()).unwrap_or(false)).unwrap_or(false);
            if failed && dirty && !close_resp.hovered() {
                // not saved, and the save failed: a red mark instead of the grey dot (DATA-009)
                child.painter().circle_filled(cr.center(), 4.5, theme::LINK_MISSING);
            } else if dirty && !close_resp.hovered() {
                child.painter().circle_filled(cr.center(), 3.5, theme::MUTED);
            } else if resp.hovered() || active || close_resp.hovered() {
                if close_resp.hovered() {
                    child.painter().rect_filled(cr, CornerRadius::same(4), theme::BG_HOVER);
                }
                paint_icon(child.painter(), cr.shrink(3.0), crate::layout::Icon::Close, theme::MUTED);
            }
            if close_resp.clicked() || resp.middle_clicked() {
                close = Some(i);
            } else if resp.clicked() || resp.drag_started() {
                self.tabbar.active = i;
            }
            if resp.dragged() {
                dragging = Some(i);
            }
            if resp.drag_stopped() {
                dropped = Some(i);
            }
            let pinned = self.tabbar.tabs[i].pinned;
            resp.context_menu(|ui| {
                let entry = ui.button(if pinned { "Unpin tab" } else { "Pin tab" });
                self.tabbar.tab_marks.menu_pin = Some(entry.rect);
                if entry.clicked() {
                    pin_toggle = Some((i, !pinned));
                    ui.close();
                }
                let entry = ui.button("Open in split right");
                self.tabbar.tab_marks.menu_split = Some(entry.rect);
                if entry.clicked() {
                    split = Some(i);
                    ui.close();
                }
            });
            resp.on_hover_text(title);
        }
        // the marker of the drag: where the tab lands (NAV-014, NAV-017)
        if let Some(from) = dragging.or(dropped) {
            let mids: Vec<f32> = self.tabbar.tab_rects.iter().map(|t| t.0.center().x).collect();
            if let Some(x) = child.ctx().pointer_interact_pos().map(|p| p.x) {
                let slot = self.tab_slot(from, x, &mids);
                if dropped.is_some() {
                    self.move_tab(from, slot);
                } else {
                    let at = if slot < mids.len() { self.tabbar.tab_rects[slot].0.min.x - 1.0 } else { self.tabbar.tab_rects.last().map_or(bar.min.x, |t| t.0.max.x + 1.0) };
                    let marker = Rect::from_min_max(Pos2::new(at - 1.0, bar.min.y + 6.0), Pos2::new(at + 1.0, bar.max.y - 2.0));
                    child.painter().rect_filled(marker, CornerRadius::same(1), theme::ACCENT);
                    self.tabbar.tab_marks.drop_marker = Some(marker);
                }
            }
        }
        if let Some((i, pinned)) = pin_toggle {
            self.set_tab_pinned(i, pinned);
        }
        let plus = icon_button(&mut child, Icon::Plus, 26.0, false, "New tab");
        // its key works while the focused pane shows a note (KEY-015)
        if self.tabbar.tabs.get(self.tabbar.active).is_some_and(|t| matches!(t.view, View::Note(_))) {
            self.key_badges(ui).paint(ui, plus.rect, crate::keys::Action::NoteNewTab);
        }
        if plus.clicked() {
            self.tabbar.new_tab(View::Empty);
        }
        if let Some(line) = self.tabbar.tab_marks.focus_line {
            ui.painter().rect_filled(line, CornerRadius::ZERO, theme::ACCENT);
        }
        if let Some(i) = close {
            self.actions.push(Action::CloseTab(i));
        }
        if let Some(tab) = split {
            self.actions.push(Action::SplitTabRight { pane: self.panes.drawn(), tab });
        }
    }

    fn view_header(&mut self, ui: &mut Ui) {
        let Some(tab) = self.tabbar.tabs.get(self.tabbar.active) else { return };
        let view = tab.view.clone();
        let can_back = !tab.back.is_empty();
        let can_fwd = !tab.fwd.is_empty();
        let reading = tab.reading;
        let tab_id = tab.id;
        let badges = self.key_badges(ui);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let tip = |name: &str, action| self.keymap.hint(action).map_or_else(|| name.to_string(), |k| format!("{name} ({k})"));
            let (back_tip, fwd_tip) = (tip("Back", crate::keys::Action::Back), tip("Forward", crate::keys::Action::Forward));
            if icon_button(ui, Icon::Back, 26.0, false, &back_tip).clicked() && can_back {
                self.go_back();
            }
            if icon_button(ui, Icon::Forward, 26.0, false, &fwd_tip).clicked() && can_fwd {
                self.go_forward();
            }
            ui.add_space(6.0);
            let crumbs = match &view {
                View::Note(p) | View::Drawing(p) => {
                    let s = rel_to_string(p);
                    let s = s.trim_end_matches(".md").to_string();
                    s.replace('/', "  /  ")
                }
                v => self.tab_title(v),
            };
            ui.label(egui::RichText::new(crumbs).color(theme::MUTED).size(13.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(8.0);
                if let View::Note(p) = &view {
                    let (icon, tip) = if reading { (Icon::Pencil, "Edit (Ctrl+E)") } else { (Icon::Book, "Reading view (Ctrl+E)") };
                    let toggle = icon_button(ui, icon, 26.0, false, tip);
                    badges.paint(ui, toggle.rect, crate::keys::Action::NoteReading);
                    if toggle.clicked() {
                        self.toggle_reading_view();
                    }
                    if !reading {
                        let on = self.cfg.live_preview;
                        let preview = icon_button(ui, Icon::Preview, 26.0, on, if on { "Raw markdown" } else { "Markdown preview" });
                        badges.paint(ui, preview.rect, crate::keys::Action::NotePreview);
                        if preview.clicked() {
                            self.toggle_live_preview();
                        }
                    }
                    let p2 = p.clone();
                    let menu = ui.menu_button(egui::RichText::new("⋯").size(16.0).color(theme::MUTED), |ui| {
                        if ui.button("Export to PDF…").clicked() {
                            self.actions.push(Action::Export(p2.clone()));
                            ui.close();
                        }
                        if ui.button("Insert new drawing here").clicked() {
                            let folder = p2.parent().map(|x| x.to_path_buf()).unwrap_or_default();
                            self.actions.push(Action::NewDrawing { folder, embed_into: Some(p2.clone()) });
                            ui.close();
                        }
                        if ui.button("Rename…").clicked() {
                            self.actions.push(Action::Rename(p2.clone()));
                            ui.close();
                        }
                        if ui.button("Move to trash").clicked() {
                            self.actions.push(Action::Trash(p2.clone()));
                            ui.close();
                        }
                        if ui.button("Open in new tab").clicked() {
                            self.actions.push(Action::Open { rel: p2.clone(), new_tab: true });
                            ui.close();
                        }
                    });
                    badges.paint(ui, menu.response.rect, crate::keys::Action::NoteMenu);
                    // the key of the ⋯ menu opens it (KEY-015)
                    if ui.ctx().data_mut(|d| d.remove_temp::<bool>(egui::Id::unique(("note-menu-key", tab_id)))).is_some() {
                        egui::Popup::toggle_id(ui.ctx(), egui::Popup::default_response_id(&menu.response));
                    }
                }
                if let View::Drawing(p) = &view {
                    let p2 = p.clone();
                    ui.menu_button(egui::RichText::new("⋯").size(16.0).color(theme::MUTED), |ui| {
                        if ui.button("Export to PDF…").clicked() {
                            self.actions.push(Action::Export(p2.clone()));
                            ui.close();
                        }
                        if ui.button("Rename…").clicked() {
                            self.actions.push(Action::Rename(p2.clone()));
                            ui.close();
                        }
                        if ui.button("Move to trash").clicked() {
                            self.actions.push(Action::Trash(p2.clone()));
                            ui.close();
                        }
                        let mut dark = self.cfg.dark_canvas;
                        if ui.checkbox(&mut dark, "Dark canvas").changed() {
                            self.cfg.dark_canvas = dark;
                            self.save_config();
                        }
                    });
                }
            });
        });
        ui.add_space(2.0);
    }

    fn content(&mut self, ui: &mut Ui) {
        let Some(view) = self.tabbar.tabs.get(self.tabbar.active).map(|t| t.view.clone()) else {
            self.empty_view(ui);
            return;
        };
        let area = ui.available_rect_before_wrap();
        match &view {
            View::Note(rel) => self.drag.add_drop_zone(area, rel.parent().map(|p| p.to_path_buf()).unwrap_or_default(), Some(rel.clone())),
            View::Calendar => self.drag.add_calendar_area(area),
            _ => {}
        }
        match view {
            View::Empty => self.empty_view(ui),
            View::Note(rel) => self.note_view(ui, rel),
            View::Drawing(rel) => {
                let dark = self.cfg.dark_canvas;
                match (self.drawings.get_mut(&rel), &self.vault) {
                    (Some(d), Some(v)) => match d.ui(ui, dark, v) {
                        Some(crate::drawing::DrawingAction::Editor(a)) => handle_editor_action(&mut self.actions, a, &rel),
                        Some(crate::drawing::DrawingAction::ConvertCanvas) => self.actions.push(Action::ConvertCanvas { rel, new_tab: false }),
                        None => {}
                    },
                    _ => self.actions.push(Action::Open { rel, new_tab: false }),
                }
            }
            View::Tasks => self.tasks_view(ui),
            View::SettingsFile => self.settings_file_view(ui),
            View::Social(f) => self.social_view(ui, f.clone()),
            View::Calendar => {
                let monday = self.cfg.week_starts_monday;
                let mut act = None;
                let font = self.fonts.area(crate::fonts::Area::Calendar);
                crate::fonts::with_area(ui, &font, |ui| {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                        act = self.cal.main.show(ui, &self.calendar, monday, true);
                    });
                });
                if let Some(a) = act {
                    self.calendar_action(a);
                }
            }
            View::Mail => {
                let modal_open = self.modal.is_some();
                self.mail.away = self.tree.sel.is_some() || self.search.sel.is_some();
                let font = self.fonts.area(crate::fonts::Area::Mail);
                let acts = crate::fonts::with_area(ui, &font, |ui| self.mail.show(ui, &self.cfg, &mut self.keymap, modal_open));
                for a in acts {
                    self.mail_action(a);
                }
            }
            View::Graph => {
                let cur = self.tabbar.tabs.iter().rev().find_map(|t| match &t.view {
                    View::Note(p) => Some(p.clone()),
                    _ => None,
                });
                if let Some(v) = &self.vault {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                        if let Some(c) = self.graphs.global.ui(ui, v, cur.as_deref(), false) {
                            graph_click(&mut self.actions, c);
                        }
                    });
                }
            }
        }
    }

    fn empty_view(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.3);
            ui.label(egui::RichText::new("No file is open").size(20.0).color(theme::TEXT));
            ui.add_space(10.0);
            let folder = self.current_folder();
            if ui.link("Create new note (Ctrl+N)").clicked() {
                self.actions.push(Action::NewNote { folder: folder.clone() });
            }
            if ui.link("Go to file (Ctrl+O)").clicked() {
                self.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 });
            }
            if ui.link("Open the to-do list").clicked() {
                self.actions.push(Action::ShowView(View::Tasks));
            }
            if ui.link("New drawing").clicked() {
                self.actions.push(Action::NewDrawing { folder, embed_into: None });
            }
        });
    }

    fn note_view(&mut self, ui: &mut Ui, rel: PathBuf) {
        if !self.notes.contains_key(&rel) {
            self.actions.push(Action::Open { rel, new_tab: false });
            return;
        }
        let fonts = self.fonts.editor();
        let base = fonts.rendered.size;
        let readable = self.cfg.readable_width;
        let ctrl = self.cfg.ctrl_to_follow;
        let dark = self.cfg.dark_canvas;
        let active = self.tabbar.active;
        // the title line: the name without `.md` and without the date prefix, which a rename keeps (VAULT-025)
        let (display_name, kept_prefix) = self.vault.as_ref().and_then(|v| v.file(&rel)).map(|f| (f.title(), crate::vault::date_prefix_of(&f.display_name()).unwrap_or_default().to_string())).unwrap_or_default();
        let mut rename_to: Option<String> = None;
        // the formatting toolbar sits above the text and stays when the note scrolls (EDIT-026)
        if self.cfg.format_toolbar && !self.tabbar.tabs[active].reading {
            if let Some(doc) = self.notes.get(&rel).filter(|d| d.unreadable.is_none()) {
                let badges = self.key_badges(ui);
                let palettes = crate::format_bar::Palettes { badges: &badges, highlight: &self.cfg.highlight_colours, font: &self.cfg.font_colours, scale: self.cfg.toolbar_size.clamp(80, 200) as f32 / 100.0 };
                ui.add_space(4.0);
                let mut colours = self.vdata.state.toolbar_colours();
                self.tabbar.tabs[active].editor.toolbar(ui, &doc.text, &palettes, &mut colours);
                self.vdata.state.set_toolbar_colours(colours);
                ui.add_space(2.0);
            }
        }
        let mut area = egui::ScrollArea::vertical().id_salt(("note-scroll", self.tabbar.tabs[active].id)).auto_shrink([false, false]);
        // a restored tab is scrolled where it was left, when it is first shown (NAV-038)
        if let Some(y) = self.tabbar.tabs[active].place.restore_scroll.take() {
            area = area.vertical_scroll_offset(y);
        }
        let scrolled = area.show(ui, |ui| {
            let full = ui.available_width();
            let col = if readable { full.min(760.0) } else { full - 40.0 };
            let pad = ((full - col) / 2.0).max(20.0);
            ui.add_space(16.0);
            ui.horizontal_top(|ui| {
                ui.add_space(pad);
                ui.vertical(|ui| {
                    ui.set_width(col.min(full - pad - 10.0).max(200.0));
                    // inline title (rename by editing)
                    let tab = &mut self.tabbar.tabs[active];
                    if tab.title_for.as_deref() != Some(rel.as_path()) {
                        tab.title_for = Some(rel.clone());
                        tab.title_buf = display_name.clone();
                    }
                    let title =
                        egui::TextEdit::singleline(&mut tab.title_buf).font(FontId::proportional(base * 1.9)).text_color(theme::TEXT_STRONG).frame(egui::Frame::NONE).desired_width(f32::INFINITY).id(egui::Id::unique(("title", tab.id)));
                    let tr = ui.add(title);
                    if tr.lost_focus() && tab.title_buf.trim() != display_name && !tab.title_buf.trim().is_empty() {
                        rename_to = Some(format!("{kept_prefix}{}", tab.title_buf.trim()));
                    } else if tr.lost_focus() {
                        tab.title_buf = display_name.clone();
                    }
                    if tr.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        tab.editor.request_focus = true;
                    }
                    ui.add_space(8.0);
                    let reading = tab.reading;
                    let Some(vault) = self.vault.as_mut() else { return };
                    let Some(doc) = self.notes.get_mut(&rel) else { return };
                    if let Some(why) = &doc.unreadable {
                        // never shown as an empty note that the next keystroke would save over the file (DATA-008)
                        ui.label(egui::RichText::new(format!("This note could not be read ({why}). It is shown read-only, and Shale will not change the file.")).color(theme::LINK_MISSING));
                        return;
                    }
                    if reading {
                        let text = doc.text.clone();
                        let extras = reader::Extras {
                            numbers: reader::Extras::numbers_of(&text, self.cfg.numbered_headings),
                            scroll_to: tab.reader_line.take(),
                            properties: self.cfg.properties_in_reader,
                            folds: Some(std::cell::RefCell::new(crate::fold_ui::ReaderFolds { ui: &mut tab.editor.fold, summary: self.cfg.properties_in_editor != crate::config::PropertiesView::Raw })),
                        };
                        let shown = crate::fonts::with_area(ui, &fonts.rendered, |ui| reader::show(ui, &text, vault, &rel, &mut reader::Caches { scenes: &mut self.scenes, media: &self.media }, (base, dark), &extras));
                        if let Some(a) = shown {
                            handle_reader_action(&mut self.actions, a, &rel);
                        }
                    } else {
                        tab.editor.inline_images = self.cfg.inline_images;
                        tab.editor.line_numbers = self.cfg.line_numbers;
                        tab.editor.numbered_headings = self.cfg.numbered_headings;
                        tab.editor.live_preview = self.cfg.live_preview;
                        tab.editor.preview_raw_lines = self.cfg.preview_raw_lines;
                        tab.editor.properties_summary = self.cfg.properties_in_editor != crate::config::PropertiesView::Raw;
                        tab.editor.table_cell_edit = self.cfg.table_cell_edit;
                        tab.editor.vim_enabled = self.cfg.vim_mode;
                        tab.editor.cursor_style = crate::cursor_style::CursorStyle { shape: self.cfg.cursor_shape, normal_shape: self.cfg.normal_cursor_shape, blink: self.cfg.cursor_blink };
                        tab.editor.esc_cursor = self.cfg.esc_cursor;
                        tab.editor.assist.people_dir = self.vdata.settings.people_dir.clone();
                        tab.editor.assist.numbered_headings = self.cfg.numbered_headings;
                        tab.editor.dark_canvas = self.cfg.dark_canvas;
                        tab.editor.media = self.media.clone();
                        if let Some(sel) = tab.place.restore_cursor.take() {
                            tab.editor.restore_selection_chars(ui.ctx(), &doc.text, sel);
                        }
                        let (changed, action) = crate::fonts::with_family(&fonts.rendered, || tab.editor.show(ui, &mut doc.text, vault, crate::editor::Note { rel: &rel, undo: &mut self.undo }, &fonts, ctrl));
                        tab.place.cursor = tab.editor.selection_chars(ui.ctx()).or(tab.place.cursor);
                        if changed {
                            doc.dirty = true;
                            doc.last_edit = std::time::Instant::now();
                            vault.set_note_text(&rel, &doc.text);
                        }
                        if let Some(a) = action {
                            handle_editor_action(&mut self.actions, a, &rel);
                        }
                    }
                    ui.add_space(120.0);
                });
            });
        });
        self.tabbar.tabs[active].place.scroll = Some(scrolled.state.clamped_offset().y.max(0.0).floor());
        if let Some(name) = rename_to {
            if let Err(e) = self.rename(&rel, &name) {
                self.toast(e);
                if let Some(t) = self.tabbar.tabs.get_mut(active) {
                    t.title_for = None;
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let small = |t: String| egui::RichText::new(t).size(11.5).color(theme::MUTED);
            // the keys of a sequence that waits for its next key (KEY-009)
            if let Some((keys, left)) = self.keymap.pending_text(ui.input(|i| i.time)) {
                ui.label(egui::RichText::new(keys).size(11.5).monospace().color(theme::ACCENT));
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(left));
            }
            // vim-lite mode (or the command being typed) of the open note
            if let Some(status) = self.tabbar.tabs.get(self.tabbar.active).and_then(|t| t.editor.vim_status()) {
                ui.label(egui::RichText::new(status).size(11.5).monospace().color(theme::ACCENT));
            }
            if let (Some(rel), Some(v)) = (self.current_path(), &self.vault) {
                if let Some(doc) = self.notes.get(&rel) {
                    let (chars, words) = self.status.counts(&rel, &doc.text);
                    ui.label(small(format!("{chars} characters")));
                    ui.label(small(format!("{words} words")));
                    let bl = self.status.backlinks(v, &rel);
                    ui.label(small(format!("{bl} backlink{}", if bl == 1 { "" } else { "s" })));
                    if doc.unreadable.is_some() {
                        ui.label(egui::RichText::new("Read-only: could not be read").size(11.5).color(theme::LINK_MISSING));
                    } else if let (true, Some(e)) = (doc.dirty, &doc.save_error) {
                        ui.label(egui::RichText::new("Not saved").size(11.5).color(theme::LINK_MISSING)).on_hover_text(e);
                    } else {
                        ui.label(small(if doc.dirty { "Saving…".to_string() } else { "Saved".to_string() }));
                    }
                } else if let Some(d) = self.drawings.get(&rel) {
                    ui.label(small(format!("{} elements", d.scene.live().count())));
                    if let (true, Some(e)) = (d.dirty, &d.save_error) {
                        ui.label(egui::RichText::new("Not saved").size(11.5).color(theme::LINK_MISSING)).on_hover_text(e);
                    } else {
                        ui.label(small(if d.dirty { "Saving…".to_string() } else { "Saved".to_string() }));
                    }
                }
            }
            if let Some(v) = &self.vault {
                let open = self.status.open_tasks(v);
                ui.label(small(format!("{open} open tasks")));
            }
            if let Some(t) = self.mail.status(&self.cfg) {
                ui.label(small(t));
            }
            if self.cfg.input_log {
                ui.label(egui::RichText::new("● recording keys").size(11.5).color(theme::LINK_MISSING)).on_hover_text(crate::config::input_log_path().to_string_lossy());
            }
            ui.label(small(format!("Shale {} · build {}", env!("CARGO_PKG_VERSION"), crate::config::build_stamp())));
        });
    }

    /// Text from an earlier run that could not be saved and was put aside (DATA-011): stays until
    /// it is dismissed.
    pub fn paint_recovery_notice(&mut self, ui: &mut Ui) {
        let Some(first) = self.recovery.notice.last().cloned() else { return };
        let screen = ui.ctx().content_rect();
        let mut dismissed = false;
        egui::Area::new(egui::Id::unique("recovery-notice")).order(egui::Order::Tooltip).fixed_pos(Pos2::new(screen.min.x + 60.0, screen.max.y - 120.0)).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(Color32::from_rgb(64, 44, 44)).stroke(Stroke::new(1.0, theme::LINK_MISSING.gamma_multiply(0.7))).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_max_width(420.0);
                ui.label(egui::RichText::new("Some changes could not be saved when Shale was closed last time. They were put aside so nothing is lost.").color(theme::TEXT));
                ui.horizontal(|ui| {
                    if ui.link("Open the folder").clicked() {
                        if let Err(e) = crate::opener::open_recovery_folder(&first, &self.recovery.root) {
                            self.toasts.push((e, std::time::Instant::now()));
                        }
                    }
                    if ui.link("Dismiss").clicked() {
                        dismissed = true;
                    }
                });
            });
        });
        if dismissed {
            crate::recovery::dismiss(&self.recovery.notice);
            self.recovery.notice.clear();
        }
    }

    pub fn paint_toasts(&mut self, ui: &mut Ui) {
        self.toasts.retain(|(_, t)| t.elapsed().as_secs_f32() < 3.5);
        if self.toasts.is_empty() {
            return;
        }
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        let screen = ui.ctx().content_rect();
        let mut y = screen.max.y - 40.0;
        for (i, (msg, _)) in self.toasts.iter().enumerate().rev() {
            egui::Area::new(egui::Id::unique(("toast", i))).order(egui::Order::Tooltip).fixed_pos(Pos2::new(screen.max.x - 340.0, y - 34.0)).show(ui.ctx(), |ui| {
                egui::Frame::new().fill(Color32::from_rgb(48, 46, 60)).stroke(Stroke::new(1.0, theme::ACCENT.gamma_multiply(0.6))).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.set_width(300.0);
                    ui.label(egui::RichText::new(msg).color(theme::TEXT));
                });
            });
            y -= 46.0;
        }
    }
}

/// Expand a leading `~` to the home folder.
pub fn shellexpand(p: &str) -> String {
    let p = p.trim();
    if let Some(rest) = p.strip_prefix('~') {
        if let Some(h) = dirs::home_dir() {
            return format!("{}{}", h.to_string_lossy(), rest);
        }
    }
    p.to_string()
}

pub fn _unused(_: &markdown::LinkSpan) {}

/// The first width and the limits of the left and of the right sidebar, in points (LAY-018).
const LEFT_WIDTH: f32 = 260.0;
const LEFT_RANGE: std::ops::RangeInclusive<f32> = 170.0..=520.0;
const RIGHT_WIDTH: f32 = 290.0;
const RIGHT_RANGE: std::ops::RangeInclusive<f32> = 200.0..=560.0;

/// The width a sidebar starts with: the stored one, within its limits, else the default (LAY-018).
fn sidebar_width(stored: Option<u32>, default: f32, range: std::ops::RangeInclusive<f32>) -> f32 {
    stored.map_or(default, |w| w as f32).clamp(*range.start(), *range.end())
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: LAY-018
    #[test]
    fn lay_018_a_stored_width_is_clamped_to_the_range_of_its_sidebar() {
        assert_eq!(sidebar_width(None, LEFT_WIDTH, LEFT_RANGE), 260.0);
        assert_eq!(sidebar_width(Some(333), LEFT_WIDTH, LEFT_RANGE), 333.0);
        assert_eq!(sidebar_width(Some(5), LEFT_WIDTH, LEFT_RANGE), 170.0);
        assert_eq!(sidebar_width(Some(4000), LEFT_WIDTH, LEFT_RANGE), 520.0);
        assert_eq!(sidebar_width(Some(5), RIGHT_WIDTH, RIGHT_RANGE), 200.0);
        assert_eq!(sidebar_width(Some(4000), RIGHT_WIDTH, RIGHT_RANGE), 560.0);
    }
}
