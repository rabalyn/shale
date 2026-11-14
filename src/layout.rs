//! Window layout: ribbon, sidebars, tab bar, the note/drawing/tasks/graph views and status bar.

use crate::app::{handle_editor_action, handle_reader_action, graph_click, muted, Action, LeftPanel, Modal, ShaleApp, View};
use crate::markdown;
use crate::reader;
use crate::theme;
use crate::vault::rel_to_string;
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};
use std::path::PathBuf;

#[derive(Clone, Copy)]
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
    Close,
    Plus,
    SidebarLeft,
    SidebarRight,
    Sort,
    Calendar,
    Import,
    Social,
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
            p.add(Shape::closed_line(vec![a, a + Vec2::new(n.x * 1.2, -n.y * 0.2) + Vec2::new(0.0, -n.y * 1.4), b - Vec2::new(n.x, 0.0) + Vec2::new(0.0, -n.y * 0.0), b, b + Vec2::new(n.x * 0.2, n.y * 1.2), a + Vec2::new(n.x * 1.6, 0.0)], s));
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
        Icon::Close => {
            p.line_segment([cx - Vec2::splat(w * 0.22), cx + Vec2::splat(w * 0.22)], s);
            p.line_segment([cx + Vec2::new(-w * 0.22, w * 0.22), cx + Vec2::new(w * 0.22, -w * 0.22)], s);
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
    let (r, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hovered = resp.hovered();
    if active {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_ACTIVE);
    } else if hovered {
        ui.painter().rect_filled(r, CornerRadius::same(5), theme::BG_HOVER);
    }
    let c = if active { theme::ACCENT } else if hovered { theme::TEXT_STRONG } else { theme::MUTED };
    paint_icon(ui.painter(), r.shrink(size * 0.22), icon, c);
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
                            let _ = std::fs::create_dir_all(&path);
                            let _ = crate::vault::write_sample_vault(&path);
                            self.actions.push(Action::OpenVault(path.clone()));
                        }
                        if ui.button("Create empty vault").clicked() {
                            let _ = std::fs::create_dir_all(&path);
                            self.actions.push(Action::OpenVault(path.clone()));
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

    pub fn layout(&mut self, ui: &mut Ui) {
        // ribbon
        egui::Panel::left("ribbon").exact_size(44.0).resizable(false).frame(egui::Frame::new().fill(theme::BG_RIBBON).inner_margin(egui::Margin::symmetric(6, 8)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
            self.ribbon(ui);
        });
        // status bar
        egui::Panel::bottom("status").exact_size(24.0).resizable(false).frame(egui::Frame::new().fill(theme::BG_SECONDARY).inner_margin(egui::Margin::symmetric(10, 3)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
            self.status_bar(ui);
        });
        if self.cfg.show_left {
            egui::Panel::left("left-sidebar").default_size(260.0).size_range(170.0..=520.0).frame(egui::Frame::new().fill(theme::BG_SECONDARY).inner_margin(egui::Margin::symmetric(8, 8)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
                self.left_sidebar(ui);
            });
        }
        if self.cfg.show_right {
            egui::Panel::right("right-sidebar").default_size(290.0).size_range(200.0..=560.0).frame(egui::Frame::new().fill(theme::BG_SECONDARY).inner_margin(egui::Margin::symmetric(10, 8)).stroke(Stroke::new(1.0, theme::BORDER))).show(ui, |ui| {
                self.right_sidebar(ui);
            });
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG_PRIMARY)).show(ui, |ui| {
            self.tab_bar(ui);
            self.view_header(ui);
            self.content(ui);
        });
    }

    fn ribbon(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.vertical_centered(|ui| {
            if icon_button(ui, Icon::SidebarLeft, 30.0, false, "Toggle left sidebar").clicked() {
                self.cfg.show_left = !self.cfg.show_left;
            }
            ui.add_space(4.0);
            let files = self.cfg.show_left && self.left == LeftPanel::Files;
            if icon_button(ui, Icon::Files, 30.0, files, "Files").clicked() {
                self.left = LeftPanel::Files;
                self.cfg.show_left = true;
            }
            let search = self.cfg.show_left && self.left == LeftPanel::Search;
            if icon_button(ui, Icon::Search, 30.0, search, "Search all notes (Ctrl+Shift+F)").clicked() {
                self.left = LeftPanel::Search;
                self.cfg.show_left = true;
                self.focus_search = true;
            }
            let tags = self.cfg.show_left && self.left == LeftPanel::Tags;
            if icon_button(ui, Icon::Tags, 30.0, tags, "Tags").clicked() {
                self.left = LeftPanel::Tags;
                self.cfg.show_left = true;
            }
            ui.add_space(8.0);
            let cur = self.tabs.get(self.active).map(|t| t.view.clone());
            if icon_button(ui, Icon::Switcher, 30.0, false, "Quick switcher (Ctrl+O)").clicked() {
                self.modal = Some(Modal::QuickSwitch { query: String::new(), sel: 0 });
            }
            if icon_button(ui, Icon::Graph, 30.0, cur == Some(View::Graph), "Graph of all notes").clicked() {
                self.actions.push(Action::ShowView(View::Graph));
            }
            if icon_button(ui, Icon::Calendar, 30.0, cur == Some(View::Calendar), "Calendar").clicked() {
                self.actions.push(Action::ShowView(View::Calendar));
            }
            if icon_button(ui, Icon::Tasks, 30.0, cur == Some(View::Tasks), "To-do list across all notes").clicked() {
                self.actions.push(Action::ShowView(View::Tasks));
            }
            if icon_button(ui, Icon::Social, 30.0, cur == Some(View::Social), "Feeds: read Mastodon and Bluesky and save posts to the vault").clicked() {
                self.actions.push(Action::ShowView(View::Social));
            }
            if icon_button(ui, Icon::Import, 30.0, matches!(self.modal, Some(Modal::Import(_))), "Import e-mails, events and files (or drop them on the window)").clicked() {
                self.open_import(vec![], vec![], None);
            }
            if icon_button(ui, Icon::Drawing, 30.0, false, "New drawing").clicked() {
                let f = self.current_folder();
                self.actions.push(Action::NewDrawing { folder: f, embed_into: None });
            }
            if icon_button(ui, Icon::NewNote, 30.0, false, "New note (Ctrl+N)").clicked() {
                let f = self.current_folder();
                self.actions.push(Action::NewNote { folder: f });
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                if icon_button(ui, Icon::Settings, 30.0, false, "Settings and vault (Ctrl+,)").clicked() {
                    let vp = self.cfg.vault.as_ref().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
                    self.modal = Some(Modal::Settings { vault_path: vp });
                }
                if icon_button(ui, Icon::SidebarRight, 30.0, false, "Toggle right sidebar").clicked() {
                    self.cfg.show_right = !self.cfg.show_right;
                }
            });
        });
    }

    fn tab_title(&self, view: &View) -> String {
        match view {
            View::Empty => "New tab".into(),
            View::Tasks => "Tasks".into(),
            View::Graph => "Graph".into(),
            View::Calendar => "Calendar".into(),
            View::Social => "Feeds".into(),
            View::Note(p) | View::Drawing(p) => self.vault.as_ref().and_then(|v| v.file(p).map(|f| f.display_name())).unwrap_or_else(|| rel_to_string(p)),
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        let h = 36.0;
        let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, theme::BG_SECONDARY);
        ui.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, theme::BORDER));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(bar.shrink2(Vec2::new(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Max)));
        child.spacing_mut().item_spacing.x = 2.0;
        let n = self.tabs.len().max(1);
        let tab_w = ((bar.width() - 50.0) / n as f32).clamp(60.0, 220.0);
        let mut close = None;
        for i in 0..self.tabs.len() {
            let title = self.tab_title(&self.tabs[i].view);
            let dirty = self.tabs[i].view.path().map(|p| self.notes.get(p).map(|d| d.dirty).unwrap_or(false) || self.drawings.get(p).map(|d| d.dirty).unwrap_or(false)).unwrap_or(false);
            let (r, resp) = child.allocate_exact_size(Vec2::new(tab_w, h - 6.0), Sense::click());
            let active = i == self.active;
            let fill = if active { theme::BG_PRIMARY } else if resp.hovered() { theme::BG_HOVER } else { theme::BG_SECONDARY };
            child.painter().rect_filled(r, CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, fill);
            let text_rect = r.shrink2(Vec2::new(10.0, 0.0));
            let galley = child.painter().layout_no_wrap(title.clone(), FontId::proportional(13.0), if active { theme::TEXT_STRONG } else { theme::MUTED });
            child.painter().with_clip_rect(Rect::from_min_max(text_rect.min, Pos2::new(text_rect.max.x - 18.0, text_rect.max.y))).galley(Pos2::new(text_rect.min.x, r.center().y - galley.size().y / 2.0), galley, theme::TEXT);
            let cr = Rect::from_center_size(Pos2::new(r.max.x - 14.0, r.center().y), Vec2::splat(16.0));
            let close_resp = child.interact(cr, child.scope_id().with(("tab-close", self.tabs[i].id)), Sense::click());
            if dirty && !close_resp.hovered() {
                child.painter().circle_filled(cr.center(), 3.5, theme::MUTED);
            } else if resp.hovered() || active || close_resp.hovered() {
                if close_resp.hovered() {
                    child.painter().rect_filled(cr, CornerRadius::same(4), theme::BG_HOVER);
                }
                paint_icon(child.painter(), cr.shrink(3.0), crate::layout::Icon::Close, theme::MUTED);
            }
            if close_resp.clicked() || resp.middle_clicked() {
                close = Some(i);
            } else if resp.clicked() {
                self.active = i;
            }
            resp.on_hover_text(title);
        }
        if icon_button(&mut child, Icon::Plus, 26.0, false, "New tab").clicked() {
            self.new_tab(View::Empty);
        }
        if let Some(i) = close {
            self.actions.push(Action::CloseTab(i));
        }
    }

    fn view_header(&mut self, ui: &mut Ui) {
        let Some(tab) = self.tabs.get(self.active) else { return };
        let view = tab.view.clone();
        let can_back = !tab.back.is_empty();
        let can_fwd = !tab.fwd.is_empty();
        let reading = tab.reading;
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            if icon_button(ui, Icon::Back, 26.0, false, "Back (Alt+←)").clicked() && can_back {
                self.go_back();
            }
            if icon_button(ui, Icon::Forward, 26.0, false, "Forward (Alt+→)").clicked() && can_fwd {
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
                    if icon_button(ui, icon, 26.0, false, tip).clicked() {
                        if let Some(t) = self.tabs.get_mut(self.active) {
                            t.reading = !t.reading;
                        }
                    }
                    let p2 = p.clone();
                    ui.menu_button(egui::RichText::new("⋯").size(16.0).color(theme::MUTED), |ui| {
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
        let Some(view) = self.tabs.get(self.active).map(|t| t.view.clone()) else {
            self.empty_view(ui);
            return;
        };
        let area = ui.available_rect_before_wrap();
        match &view {
            View::Note(rel) => self.add_drop_zone(area, rel.parent().map(|p| p.to_path_buf()).unwrap_or_default(), Some(rel.clone())),
            View::Calendar => self.add_calendar_area(area),
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
            View::Social => self.social_view(ui),
            View::Calendar => {
                let monday = self.cfg.week_starts_monday;
                let mut act = None;
                egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                    act = self.cal_main.show(ui, &self.calendar, monday, true);
                });
                if let Some(a) = act {
                    self.calendar_action(a);
                }
            }
            View::Graph => {
                let cur = self.tabs.iter().rev().find_map(|t| match &t.view {
                    View::Note(p) => Some(p.clone()),
                    _ => None,
                });
                if let Some(v) = &self.vault {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                        if let Some(c) = self.graph.ui(ui, v, cur.as_deref(), false) {
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
        let base = self.cfg.font_size;
        let readable = self.cfg.readable_width;
        let ctrl = self.cfg.ctrl_to_follow;
        let dark = self.cfg.dark_canvas;
        let active = self.active;
        let display_name = self.vault.as_ref().and_then(|v| v.file(&rel).map(|f| f.display_name())).unwrap_or_default();
        let mut rename_to: Option<String> = None;
        egui::ScrollArea::vertical().id_salt(("note-scroll", self.tabs[active].id)).auto_shrink([false, false]).show(ui, |ui| {
            let full = ui.available_width();
            let col = if readable { full.min(760.0) } else { full - 40.0 };
            let pad = ((full - col) / 2.0).max(20.0);
            ui.add_space(16.0);
            ui.horizontal_top(|ui| {
              ui.add_space(pad);
              ui.vertical(|ui| {
                ui.set_width(col.min(full - pad - 10.0).max(200.0));
                // inline title (rename by editing)
                let tab = &mut self.tabs[active];
                if tab.title_for.as_deref() != Some(rel.as_path()) {
                    tab.title_for = Some(rel.clone());
                    tab.title_buf = display_name.clone();
                }
                let title = egui::TextEdit::singleline(&mut tab.title_buf).font(FontId::proportional(base * 1.9)).text_color(theme::TEXT_STRONG).frame(egui::Frame::NONE).desired_width(f32::INFINITY).id(egui::Id::unique(("title", tab.id)));
                let tr = ui.add(title);
                if tr.lost_focus() && tab.title_buf.trim() != display_name && !tab.title_buf.trim().is_empty() {
                    rename_to = Some(tab.title_buf.trim().to_string());
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
                if reading {
                    let text = doc.text.clone();
                    if let Some(a) = reader::show(ui, &text, vault, &rel, &mut self.scenes, base, dark) {
                        handle_reader_action(&mut self.actions, a, &rel);
                    }
                } else {
                    tab.editor.inline_images = self.cfg.inline_images;
                    tab.editor.vim_enabled = self.cfg.vim_mode;
                    tab.editor.assist.people_dir = self.cfg.people_dir.clone();
                    tab.editor.dark_canvas = self.cfg.dark_canvas;
                    let (changed, action) = tab.editor.show(ui, &mut doc.text, vault, &rel, base, ctrl);
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
        if let Some(name) = rename_to {
            if let Err(e) = self.rename(&rel, &name) {
                self.toast(e);
                if let Some(t) = self.tabs.get_mut(active) {
                    t.title_for = None;
                }
            }
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 14.0;
            let small = |t: String| egui::RichText::new(t).size(11.5).color(theme::MUTED);
            // vim-lite mode (or the command being typed) of the open note
            if let Some(status) = self.tabs.get(self.active).and_then(|t| t.editor.vim_status()) {
                ui.label(egui::RichText::new(status).size(11.5).monospace().color(theme::ACCENT));
            }
            if let (Some(rel), Some(v)) = (self.current_path(), &self.vault) {
                if let Some(doc) = self.notes.get(&rel) {
                    let words = doc.text.split_whitespace().count();
                    let chars = doc.text.chars().count();
                    ui.label(small(format!("{chars} characters")));
                    ui.label(small(format!("{words} words")));
                    let bl = v.backlinks(&rel).len();
                    ui.label(small(format!("{bl} backlink{}", if bl == 1 { "" } else { "s" })));
                    ui.label(small(if doc.dirty { "Saving…".to_string() } else { "Saved".to_string() }));
                } else if let Some(d) = self.drawings.get(&rel) {
                    ui.label(small(format!("{} elements", d.scene.live().count())));
                    ui.label(small(if d.dirty { "Saving…".to_string() } else { "Saved".to_string() }));
                }
            }
            if let Some(v) = &self.vault {
                let open = v.all_tasks().iter().filter(|(_, t)| !t.done).count();
                ui.label(small(format!("{open} open tasks")));
            }
            if self.cfg.input_log {
                ui.label(egui::RichText::new("● recording keys").size(11.5).color(theme::LINK_MISSING)).on_hover_text(crate::config::input_log_path().to_string_lossy());
            }
            ui.label(small(format!("Shale {} · build {}", env!("CARGO_PKG_VERSION"), crate::config::build_stamp())));
        });
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
