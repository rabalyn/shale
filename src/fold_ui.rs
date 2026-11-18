//! The fold state of the open notes and what the editor draws for it (EDIT-070…EDIT-079): the
//! ▸ / ▾ marker left of a header line, the badge after a folded header, and the commands. The
//! blocks and the commands themselves are in `fold.rs`.

use crate::fold::{find_blocks, Block, FoldCmd, NoteFolds, View};
use crate::theme;
use eframe::egui::{self, text::CCursor, Galley, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::borrow::Cow;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::path::{Path, PathBuf};

/// The folds of one frame: the blocks of the text, which of them are folded, and what is hidden.
#[derive(Clone)]
pub struct Frame {
    hash: u64,
    pub blocks: Vec<Block>,
    folds: NoteFolds,
    pub folded: Vec<usize>,
    pub view: View,
}

fn hash_of(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}

impl Frame {
    fn new(text: &str, blocks: Vec<Block>, folds: NoteFolds) -> Frame {
        let folded = folds.folded(&blocks);
        let view = View::of(&blocks, &folded);
        Frame { hash: hash_of(text), blocks, folds, folded, view }
    }

    /// The same folds on `text`: this frame when the text is the one it was made for, else the folds
    /// found again in it (the text box changed it in the middle of the frame).
    pub fn at(&self, text: &str) -> Cow<'_, Frame> {
        if hash_of(text) == self.hash {
            Cow::Borrowed(self)
        } else {
            Cow::Owned(Frame::new(text, find_blocks(text), self.folds.clone()))
        }
    }
}

/// Where the editor drew its text: for [`FoldUi::paint`].
#[derive(Clone, Copy)]
pub struct Painted<'a> {
    pub id: egui::Id,
    pub rel: &'a Path,
    pub galley: &'a Galley,
    /// where the galley is on the screen
    pub pos: Pos2,
    /// the rectangle of the text box
    pub area: Rect,
    /// the size of the text
    pub base: f32,
}

/// What the last frame drew for one header line (for the tests).
#[derive(Clone, Debug)]
pub struct Widget {
    pub line: usize,
    pub marker: Option<Rect>,
    pub badge: Option<Rect>,
}

/// The fold state of the notes an editor has shown, and what it is asked to do with it.
#[derive(Default)]
pub struct FoldUi {
    notes: HashMap<PathBuf, NoteFolds>,
    /// a command (a key, vim's `za`…) to do at the start of the next frame
    pub request: Option<FoldCmd>,
    pub widgets: Vec<Widget>,
}

/// Where the markers sit left of the text, and their size.
const MARKER: f32 = 12.0;

impl FoldUi {
    /// The folds of `text` of the note `rel` for this frame; forgets the folds whose block is gone.
    pub fn begin(&mut self, rel: &Path, text: &str) -> Frame {
        let blocks = find_blocks(text);
        let folds = match self.notes.get_mut(rel) {
            Some(f) => {
                f.sync(&blocks);
                f.clone()
            }
            None => NoteFolds::default(),
        };
        if folds.is_empty() {
            self.notes.remove(rel);
        }
        Frame::new(text, blocks, folds)
    }

    /// Do a command with the cursor in `line`; the answer is the line the cursor has to go to when
    /// the command hid the one it was in.
    pub fn apply(&mut self, rel: &Path, frame: &Frame, cmd: FoldCmd, line: usize) -> Option<usize> {
        let folds = self.notes.entry(rel.to_path_buf()).or_default();
        let moved = folds.apply(&frame.blocks, cmd, line);
        if folds.is_empty() {
            self.notes.remove(rel);
        }
        moved
    }

    /// The cursor (byte `primary`, in the selection `sel`) is in text that a fold hides: unfold what
    /// hides it. A selection that takes in the whole hidden text (Ctrl+A) leaves it folded.
    pub fn reveal(&mut self, rel: &Path, frame: &Frame, text: &str, primary: usize, sel: Range<usize>) -> bool {
        let frame = frame.at(text);
        let line = crate::editor_preview::line_of(text, primary);
        let Some(i) = frame.view.hidden.iter().position(|h| h.contains(&line)) else { return false };
        let bytes = &frame.view.byte_ranges(text)[i];
        if !sel.is_empty() && sel.start <= bytes.start && sel.end + 1 >= bytes.end {
            return false;
        }
        let Some(folds) = self.notes.get_mut(rel) else { return false };
        let changed = folds.reveal(&frame.blocks, line);
        if folds.is_empty() {
            self.notes.remove(rel);
        }
        changed
    }

    /// Draw the markers and the badges over the laid-out text and do what a click on one asks.
    pub fn paint(&mut self, ui: &mut Ui, frame: &Frame, at: &Painted) {
        let Painted { id, rel, galley, pos, area, base } = *at;
        self.widgets.clear();
        let text = galley.job.text.as_str();
        let frame = frame.at(text);
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        let clip = ui.clip_rect();
        let hover = ui.input(|i| i.pointer.hover_pos());
        let lines = line_chars(text);
        let row_of = |line: usize, offset: usize| {
            let r = galley.pos_from_cursor(CCursor::new(lines[line].0 + offset));
            Rect::from_min_max(pos + r.min.to_vec2(), pos + r.max.to_vec2())
        };
        let mut clicked: Option<(usize, FoldCmd)> = None;
        let folded_lines: Vec<usize> = frame.folded.iter().map(|&i| frame.blocks[i].header).collect();
        let mut last = None;
        for b in &frame.blocks {
            if last == Some(b.header) || frame.view.is_hidden(b.header) || b.header >= lines.len() {
                continue;
            }
            last = Some(b.header);
            let row = row_of(b.header, 0);
            if row.max.y < clip.min.y || row.min.y > clip.max.y {
                continue;
            }
            let folded = folded_lines.contains(&b.header);
            let hot = hover.is_some_and(|p| p.y >= row.min.y && p.y <= row.max.y && p.x >= pos.x - MARKER - 6.0 && p.x <= area.right());
            let mut w = Widget { line: b.header, marker: None, badge: None };
            if folded || hot {
                let rect = Rect::from_center_size(Pos2::new(pos.x - MARKER / 2.0 - 2.0, row.center().y), Vec2::splat(MARKER));
                let resp = ui.interact(rect, id.with(("fold-marker", b.header)), Sense::click());
                let colour = if resp.hovered() { theme::TEXT } else { theme::MUTED };
                let c = rect.center();
                let tri = if folded { vec![c + Vec2::new(-2.5, -4.0), c + Vec2::new(-2.5, 4.0), c + Vec2::new(3.5, 0.0)] } else { vec![c + Vec2::new(-4.0, -2.5), c + Vec2::new(4.0, -2.5), c + Vec2::new(0.0, 3.5)] };
                painter.add(Shape::convex_polygon(tri, colour, Stroke::NONE));
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                let resp = resp.on_hover_text(if folded { "Unfold" } else { "Fold" });
                if resp.clicked() {
                    clicked = Some((b.header, FoldCmd::Toggle));
                }
                w.marker = Some(rect);
            }
            self.widgets.push(w);
        }
        for &(header, n) in &frame.view.badges {
            if header >= lines.len() {
                continue;
            }
            let end = row_of(header, lines[header].1);
            if end.max.y < clip.min.y || end.min.y > clip.max.y {
                continue;
            }
            let label = format!("\u{2026} {n} line{}", if n == 1 { "" } else { "s" });
            let g = painter.layout_no_wrap(label, egui::FontId::proportional((base * 0.8).max(9.0)), theme::MUTED);
            let rect = Rect::from_center_size(Pos2::new(end.min.x + 10.0 + (g.size().x + 10.0) / 2.0, end.center().y), g.size() + Vec2::new(10.0, 2.0));
            let resp = ui.interact(rect, id.with(("fold-badge", header)), Sense::click());
            painter.rect_filled(rect, 4.0, if resp.hovered() { theme::BG_ACTIVE } else { theme::BG_SECONDARY });
            painter.galley(rect.center() - g.size() / 2.0, g, theme::MUTED);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text("Unfold").clicked() {
                clicked = Some((header, FoldCmd::Unfold));
            }
            match self.widgets.iter_mut().find(|w| w.line == header) {
                Some(w) => w.badge = Some(rect),
                None => self.widgets.push(Widget { line: header, marker: None, badge: Some(rect) }),
            }
        }
        if let Some((line, cmd)) = clicked {
            self.apply(rel, &frame, cmd, line);
            ui.ctx().request_repaint();
        }
    }
}

/// For each line of `text`: the char index where it starts and how many chars it has (without a `\r`).
fn line_chars(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let (mut start, mut total, mut shown) = (0, 0, 0);
    for c in text.chars() {
        if c == '\n' {
            out.push((start, shown));
            start += total + 1;
            (total, shown) = (0, 0);
        } else {
            total += 1;
            shown += usize::from(c != '\r');
        }
    }
    out.push((start, shown));
    out
}
