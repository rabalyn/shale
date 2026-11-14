//! Force-directed graph of notes and their links.

use crate::theme;
use crate::vault::{FileKind, GraphNode, Vault};
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct GraphView {
    generation: u64,
    nodes: Vec<GraphNode>,
    edges: Vec<(usize, usize)>,
    pos: Vec<Vec2>,
    vel: Vec<Vec2>,
    degree: Vec<usize>,
    pub offset: Vec2,
    pub zoom: f32,
    dragging: Option<usize>,
    pub show_missing: bool,
    pub show_orphans: bool,
    /// When set, only this note and its neighbours (local graph).
    pub focus: Option<PathBuf>,
    pub depth: usize,
    energy: f32,
    focus_key: Option<PathBuf>,
    /// Zoom to fit until the person pans or zooms themselves.
    auto_fit: bool,
}

impl GraphView {
    pub fn new(local: bool) -> Self {
        GraphView {
            generation: u64::MAX,
            nodes: vec![],
            edges: vec![],
            pos: vec![],
            vel: vec![],
            degree: vec![],
            offset: Vec2::ZERO,
            zoom: if local { 0.8 } else { 1.0 },
            dragging: None,
            show_missing: true,
            show_orphans: true,
            focus: None,
            depth: 1,
            energy: 1.0,
            focus_key: None,
            auto_fit: true,
        }
    }

    fn rebuild(&mut self, vault: &Vault) {
        let (mut nodes, mut edges) = vault.graph(self.show_missing);
        if let Some(f) = &self.focus {
            let key = crate::vault::rel_to_string(f);
            if let Some(center) = nodes.iter().position(|n| n.key == key) {
                let mut keep = vec![false; nodes.len()];
                keep[center] = true;
                for _ in 0..self.depth {
                    let snapshot = keep.clone();
                    for (a, b) in &edges {
                        if snapshot[*a] || snapshot[*b] {
                            keep[*a] = true;
                            keep[*b] = true;
                        }
                    }
                }
                let (n2, e2) = filter(nodes, edges, &keep);
                nodes = n2;
                edges = e2;
            } else {
                nodes.clear();
                edges.clear();
            }
        }
        if !self.show_orphans {
            let mut keep = vec![false; nodes.len()];
            for (a, b) in &edges {
                keep[*a] = true;
                keep[*b] = true;
            }
            let (n2, e2) = filter(nodes, edges, &keep);
            nodes = n2;
            edges = e2;
        }
        // keep old positions for nodes that survived
        let old: HashMap<String, Vec2> = self.nodes.iter().zip(self.pos.iter()).map(|(n, p)| (n.key.clone(), *p)).collect();
        let n = nodes.len();
        self.pos = nodes
            .iter()
            .enumerate()
            .map(|(i, nd)| {
                old.get(&nd.key).copied().unwrap_or_else(|| {
                    // deterministic spiral start
                    let a = i as f32 * 2.399;
                    let r = 30.0 * (i as f32 + 1.0).sqrt();
                    Vec2::new(a.cos() * r, a.sin() * r)
                })
            })
            .collect();
        self.vel = vec![Vec2::ZERO; n];
        self.degree = vec![0; n];
        for (a, b) in &edges {
            self.degree[*a] += 1;
            self.degree[*b] += 1;
        }
        self.nodes = nodes;
        self.edges = edges;
        self.energy = 1.0;
    }

    fn step(&mut self) {
        let n = self.pos.len();
        if n == 0 {
            return;
        }
        let mut force = vec![Vec2::ZERO; n];
        for i in 0..n {
            for j in (i + 1)..n {
                let d = self.pos[i] - self.pos[j];
                let dist2 = d.length_sq().max(25.0);
                let f = d / dist2.sqrt() * (2600.0 / dist2);
                force[i] += f;
                force[j] -= f;
            }
        }
        for (a, b) in &self.edges {
            let d = self.pos[*b] - self.pos[*a];
            let len = d.length().max(0.01);
            let f = d / len * (len - 90.0) * 0.04;
            force[*a] += f;
            force[*b] -= f;
        }
        let mut total = 0.0;
        #[allow(clippy::needless_range_loop)] // force, pos and vel are indexed together
        for i in 0..n {
            force[i] -= self.pos[i] * 0.012;
            if Some(i) == self.dragging {
                self.vel[i] = Vec2::ZERO;
                continue;
            }
            self.vel[i] = (self.vel[i] + force[i]) * 0.6;
            let v = if self.vel[i].length() > 30.0 { self.vel[i].normalized() * 30.0 } else { self.vel[i] };
            self.pos[i] += v;
            total += v.length();
        }
        self.energy = total / n as f32;
    }

    /// Returns a note to open when a node is clicked.
    pub fn ui(&mut self, ui: &mut Ui, vault: &Vault, current: Option<&Path>, compact: bool) -> Option<GraphClick> {
        if self.focus_key.as_deref() != self.focus.as_deref() {
            self.focus_key = self.focus.clone();
            self.generation = u64::MAX;
            self.nodes.clear();
            self.pos.clear();
        }
        if self.generation != vault.generation {
            self.generation = vault.generation;
            self.rebuild(vault);
        }
        let mut result = None;
        if !compact {
            ui.horizontal(|ui| {
                let mut changed = false;
                changed |= ui.checkbox(&mut self.show_missing, "Show missing notes").changed();
                changed |= ui.checkbox(&mut self.show_orphans, "Show unlinked notes").changed();
                if changed {
                    self.generation = u64::MAX;
                }
                if ui.button("Fit").clicked() {
                    self.auto_fit = true;
                }
                ui.label(egui::RichText::new(format!("{} notes · {} links", self.nodes.len(), self.edges.len())).color(theme::MUTED));
            });
        }
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::BG_PRIMARY);

        for _ in 0..3 {
            if self.energy > 0.05 || self.dragging.is_some() {
                self.step();
            }
        }
        if self.energy > 0.05 || self.dragging.is_some() {
            ui.ctx().request_repaint();
        }

        if self.auto_fit && !self.pos.is_empty() {
            let mut lo = Vec2::splat(f32::MAX);
            let mut hi = Vec2::splat(f32::MIN);
            for p in &self.pos {
                lo = lo.min(*p);
                hi = hi.max(*p);
            }
            let ext = (hi - lo).max(Vec2::splat(60.0)) + Vec2::splat(80.0);
            let target = ((rect.width() / ext.x).min(rect.height() / ext.y) * 0.85).clamp(0.3, if compact { 1.6 } else { 2.2 });
            self.zoom += (target - self.zoom) * 0.2;
            self.offset = -((lo + hi) / 2.0) * self.zoom;
        }
        let center = rect.center() + self.offset;
        let zoom = self.zoom;
        let to_world = |s: Pos2| (s - center) / zoom;

        // zoom with wheel
        if resp.contains_pointer() {
            let (scroll, zd, hover) = ui.input(|i| (i.smooth_scroll_delta().y, i.zoom_delta(), i.pointer.hover_pos()));
            let factor = if zd != 1.0 { zd } else { (scroll * 0.002).exp() };
            if factor != 1.0 {
                if let Some(h) = hover {
                    self.auto_fit = false;
                    let before = to_world(h);
                    self.zoom = (self.zoom * factor).clamp(0.15, 5.0);
                    let after = (h - center) / self.zoom;
                    self.offset += (after - before) * self.zoom;
                }
            }
        }
        let zoom = self.zoom;
        let center = rect.center() + self.offset;
        let to_screen = move |p: Vec2| center + p * zoom;

        let radius = |deg: usize| (3.5 + (deg as f32).sqrt() * 2.2).min(14.0);
        let hover_pos = ui.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
        let hovered = hover_pos.and_then(|hp| {
            (0..self.nodes.len()).filter(|i| (to_screen(self.pos[*i]) - hp).length() < radius(self.degree[*i]) * zoom.max(0.6) + 4.0).min_by(|a, b| {
                let da = (to_screen(self.pos[*a]) - hp).length();
                let db = (to_screen(self.pos[*b]) - hp).length();
                da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
            })
        });
        let highlight = self.dragging.or(hovered);
        let neighbours: Vec<usize> = match highlight {
            Some(h) => self
                .edges
                .iter()
                .filter_map(|(a, b)| {
                    if *a == h {
                        Some(*b)
                    } else if *b == h {
                        Some(*a)
                    } else {
                        None
                    }
                })
                .collect(),
            None => vec![],
        };
        let current_key = current.map(crate::vault::rel_to_string);

        // edges
        for (a, b) in &self.edges {
            let lit = highlight.map(|h| h == *a || h == *b).unwrap_or(false);
            let col = if lit {
                theme::ACCENT
            } else if highlight.is_some() {
                Color32::from_gray(50)
            } else {
                Color32::from_gray(78)
            };
            painter.line_segment([to_screen(self.pos[*a]), to_screen(self.pos[*b])], Stroke::new(if lit { 1.6 } else { 1.0 }, col));
        }
        // nodes
        for (i, n) in self.nodes.iter().enumerate() {
            let p = to_screen(self.pos[i]);
            let r = radius(self.degree[i]) * zoom.clamp(0.5, 2.0);
            let is_current = current_key.as_deref() == Some(n.key.as_str());
            let dim = highlight.is_some() && highlight != Some(i) && !neighbours.contains(&i);
            let mut col = match (n.rel.is_some(), n.kind) {
                (false, _) => theme::LINK_MISSING.gamma_multiply(0.7),
                (true, FileKind::Drawing) => Color32::from_rgb(120, 190, 140),
                _ => Color32::from_gray(170),
            };
            if is_current || highlight == Some(i) {
                col = theme::ACCENT;
            }
            if dim {
                col = col.gamma_multiply(0.35);
            }
            painter.circle_filled(p, r, col);
            let show_label = zoom > 0.55 || highlight == Some(i) || neighbours.contains(&i) || is_current;
            if show_label && !(compact && dim) {
                let tc = if dim { theme::MUTED.gamma_multiply(0.5) } else { theme::TEXT };
                painter.text(p + Vec2::new(0.0, r + 3.0), Align2::CENTER_TOP, &n.label, FontId::proportional(if compact { 11.0 } else { 12.5 }), tc);
            }
        }
        if self.nodes.is_empty() {
            painter.text(rect.center(), Align2::CENTER_CENTER, "No linked notes yet", FontId::proportional(13.0), theme::MUTED);
        }

        // interaction
        if resp.drag_started() {
            self.dragging = hovered;
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match self.dragging {
                Some(i) => {
                    self.pos[i] += d / zoom;
                    self.energy = 1.0;
                }
                None => {
                    self.auto_fit = false;
                    self.offset += d;
                }
            }
        }
        if resp.drag_stopped() {
            self.dragging = None;
        }
        if hovered.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.clicked() {
            if let Some(i) = hovered {
                let n = &self.nodes[i];
                result = Some(match &n.rel {
                    Some(r) => GraphClick::Open(r.clone()),
                    None => GraphClick::Create(n.label.clone()),
                });
            }
        }
        let _ = Rect::NOTHING;
        result
    }
}

pub enum GraphClick {
    Open(PathBuf),
    Create(String),
}

fn filter(nodes: Vec<GraphNode>, edges: Vec<(usize, usize)>, keep: &[bool]) -> (Vec<GraphNode>, Vec<(usize, usize)>) {
    let mut map = vec![usize::MAX; nodes.len()];
    let mut out = Vec::new();
    for (i, n) in nodes.into_iter().enumerate() {
        if keep[i] {
            map[i] = out.len();
            out.push(n);
        }
    }
    let e = edges.into_iter().filter(|(a, b)| keep[*a] && keep[*b]).map(|(a, b)| (map[a], map[b])).collect();
    (out, e)
}
