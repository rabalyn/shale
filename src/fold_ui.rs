//! The fold state of the open notes and what the editor draws for it (EDIT-070…EDIT-089): the
//! ▸ / ▾ marker left of a header line, the badge after a folded header, the summary of a folded
//! front matter, the folds a note starts with and the commands. The blocks and the commands
//! themselves are in `fold.rs`.

use crate::fold::{find_blocks, Block, FoldCmd, FoldKind, Key, NoteFolds, View};
use crate::theme;
use eframe::egui::{self, text::CCursor, Galley, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The folds of one frame: the blocks of the text, which of them are folded, and what is hidden.
#[derive(Clone)]
pub struct Frame {
    hash: u64,
    /// shared with the cache of [`FoldUi`] (EDIT-100): an unchanged frame does not copy them
    pub blocks: Arc<Vec<Block>>,
    folds: Arc<NoteFolds>,
    pub folded: Vec<usize>,
    pub view: View,
    /// a mail note (EDIT-082): its quotes are "quoted lines"
    pub mail: bool,
    /// a folded front matter shows its summary line instead of `---` (EDIT-081)
    pub summary: bool,
}

/// A fast hash of a text, to tell whether it is the one the folds were found in (EDIT-100): eight bytes
/// at a time, the length in it; a megabyte takes about 0.2 ms.
pub(crate) fn hash_of(text: &str) -> u64 {
    const K: u64 = 0x517c_c1b7_2722_0a95;
    let bytes = text.as_bytes();
    let mut h = bytes.len() as u64;
    let (chunks, rest) = bytes.as_chunks::<8>();
    for c in chunks {
        h = (h.rotate_left(5) ^ u64::from_le_bytes(*c)).wrapping_mul(K);
    }
    for &b in rest {
        h = (h.rotate_left(5) ^ u64::from(b)).wrapping_mul(K);
    }
    h
}

impl Frame {
    fn new(hash: u64, blocks: Arc<Vec<Block>>, mail: bool, folds: Arc<NoteFolds>, summary: bool) -> Frame {
        let folded = folds.folded(&blocks);
        let view = View::of(&blocks, &folded);
        Frame { hash, blocks, folds, folded, view, mail, summary }
    }

    /// Is `text` a mail note whose quotes say "quoted lines" (EDIT-082)?
    fn mail_of(text: &str, blocks: &[Block]) -> bool {
        blocks.iter().any(|b| b.kind == FoldKind::Quote) && crate::fold::is_mail(text)
    }

    /// The folded front matter's block, when its first line is shown as a summary (EDIT-081).
    fn front_summary(&self) -> Option<&Block> {
        let b = self.blocks.iter().find(|b| b.kind == FoldKind::Front)?;
        (self.summary && self.folded.iter().any(|&i| self.blocks[i].header == b.header && self.blocks[i].kind == FoldKind::Front)).then_some(b)
    }

    /// The bytes of the first line of `text` while a summary is painted over it: its own text is
    /// hidden but keeps its row (EDIT-081).
    pub fn veiled(&self, text: &str) -> Option<Range<usize>> {
        self.front_summary().map(|_| 0..text.find('\n').map_or(text.len(), |p| p + 1))
    }

    /// The same folds on `text`: this frame when the text is the one it was made for, else the folds
    /// found again in it (the text box changed it in the middle of the frame).
    pub fn at(&self, text: &str) -> Cow<'_, Frame> {
        let hash = hash_of(text);
        if hash == self.hash {
            Cow::Borrowed(self)
        } else {
            let blocks = find_blocks(text);
            let mail = Frame::mail_of(text, &blocks);
            Cow::Owned(Frame::new(hash, Arc::new(blocks), mail, self.folds.clone(), self.summary))
        }
    }
}

/// Where the editor drew its text: for [`FoldUi::paint`].
#[derive(Clone, Copy)]
pub struct Painted<'a> {
    pub id: egui::Id,
    /// the lines and rows of the galley (EDIT-105)
    pub index: &'a crate::galley_index::GalleyIndex,
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
    /// the copy button of a code box (EDIT-090)
    pub copy: Option<Rect>,
}

/// The fold state of the notes an editor has shown, and what it is asked to do with it.
#[derive(Default)]
pub struct FoldUi {
    notes: HashMap<PathBuf, NoteFolds>,
    /// the last frame of each note: while the text and the folds are the same, `begin` hands it out
    /// again and parses nothing (EDIT-100)
    last: HashMap<PathBuf, Frame>,
    /// a command (a key, vim's `za`…) to do at the start of the next frame
    pub request: Option<FoldCmd>,
    pub widgets: Vec<Widget>,
    /// the key "copy the code block at the cursor" (KEY-035) asks for it at the next frame
    pub copy_code: bool,
    /// the notes whose folds were set when they were first shown (EDIT-081, EDIT-082)
    seen: HashSet<PathBuf>,
    /// the folds a note started with, to tell what the user changed (EDIT-079)
    initial: HashMap<PathBuf, NoteFolds>,
    /// the notes whose folds came from the vault state (EDIT-079): they are written back as they are
    restored: HashSet<PathBuf>,
    /// the notes whose front matter the cursor opened and that fold it again when it leaves (EDIT-081)
    auto: HashSet<PathBuf>,
    /// per note: was the cursor in the front matter in the last frame that had the keyboard
    in_front: HashMap<PathBuf, bool>,
    /// a click on the summary line asks the editor for the cursor in this line
    pub goto: Option<usize>,
}

/// How many notes keep their last frame (EDIT-100).
const MAX_CACHED: usize = 16;

/// Where the markers sit left of the text, and their size.
pub const MARKER: f32 = 12.0;

/// What the reading view gets of a note's folds (EDIT-095): the state it shares with the editor, and the
/// setting that folds a note's front matter when the note is first shown (EDIT-081).
pub struct ReaderFolds<'a> {
    pub ui: &'a mut FoldUi,
    pub summary: bool,
}

/// The ▸ / ▾ marker in `rect`: drawn, with its hand cursor and its hint; true when it was clicked.
pub fn marker_button(ui: &mut Ui, id: egui::Id, rect: Rect, folded: bool) -> bool {
    let painter = ui.painter().with_clip_rect(ui.clip_rect());
    // it takes drags too: a button that only senses clicks leaves the drag to the text box under it, which then
    // moves its cursor to the pointer for as long as the button is held (EDIT-112)
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let colour = if resp.hovered() { theme::palette().text } else { theme::palette().muted };
    let c = rect.center();
    let tri = if folded { vec![c + Vec2::new(-2.5, -4.0), c + Vec2::new(-2.5, 4.0), c + Vec2::new(3.5, 0.0)] } else { vec![c + Vec2::new(-4.0, -2.5), c + Vec2::new(4.0, -2.5), c + Vec2::new(0.0, 3.5)] };
    painter.add(Shape::convex_polygon(tri, colour, Stroke::NONE));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.on_hover_text(if folded { "Unfold" } else { "Fold" }).clicked()
}

impl FoldUi {
    /// The folds of `text` of the note `rel` for this frame; forgets the folds whose block is gone.
    /// The first time a note is shown it gets the folds it starts with (EDIT-081, EDIT-082); `summary`
    /// is the setting that folds the front matter (Folded and Card; Raw leaves it open).
    pub fn begin(&mut self, rel: &Path, text: &str, summary: bool) -> Frame {
        let hash = hash_of(text);
        // the blocks of the last frame while the text is the same (EDIT-100)
        let same = self.last.get(rel).filter(|f| f.hash == hash && f.summary == summary);
        let (blocks, mail) = match same {
            Some(f) => (f.blocks.clone(), f.mail),
            None => {
                let blocks = find_blocks(text);
                let mail = Frame::mail_of(text, &blocks);
                (Arc::new(blocks), mail)
            }
        };
        if !self.seen.contains(rel) {
            self.seen.insert(rel.to_path_buf());
            let start = NoteFolds::defaults(&blocks, crate::fold::is_mail(text), summary);
            if !start.is_empty() {
                self.initial.insert(rel.to_path_buf(), start.clone());
                self.notes.insert(rel.to_path_buf(), start);
            }
        }
        // the folds are the ones of that frame as well: nothing to follow
        if let Some(f) = same {
            let unchanged = match self.notes.get(rel) {
                Some(n) => *f.folds == *n,
                None => f.folds.is_empty(),
            };
            if unchanged {
                return f.clone();
            }
        }
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
        let frame = Frame::new(hash, blocks, mail, Arc::new(folds), summary);
        if self.last.len() >= MAX_CACHED {
            self.last.clear();
        }
        self.last.insert(rel.to_path_buf(), frame.clone());
        frame
    }

    /// Give the note `rel` the folds the vault state remembers (EDIT-079), instead of the ones it would
    /// start with. Does nothing for a note that was shown or restored already.
    pub fn restore(&mut self, rel: &Path, saved: &[Key]) {
        if !self.seen.insert(rel.to_path_buf()) {
            return;
        }
        self.restored.insert(rel.to_path_buf());
        if !saved.is_empty() {
            self.notes.insert(rel.to_path_buf(), NoteFolds::from_saved(saved));
        }
    }

    /// What the vault state should hold for each note this editor has shown (EDIT-079): its folds, or
    /// `None` when they are the ones it starts with and nothing needs to be remembered.
    pub fn snapshot(&self) -> Vec<(PathBuf, Option<Vec<Key>>)> {
        let empty = NoteFolds::default();
        self.seen
            .iter()
            .map(|rel| {
                let now = self.notes.get(rel).unwrap_or(&empty);
                let start = self.initial.get(rel).unwrap_or(&empty);
                let keep = self.restored.contains(rel) || now != start;
                (rel.clone(), keep.then(|| now.to_saved()))
            })
            .collect()
    }

    /// A note or folder was moved: what is kept for `from` follows to the new path `remap` gives.
    pub fn moved(&mut self, remap: impl Fn(&Path) -> Option<PathBuf>) {
        fn rekey<V>(m: &mut HashMap<PathBuf, V>, remap: &impl Fn(&Path) -> Option<PathBuf>) {
            for k in m.keys().cloned().collect::<Vec<_>>() {
                if let (Some(nk), Some(v)) = (remap(&k), m.remove(&k)) {
                    m.insert(nk, v);
                }
            }
        }
        fn rekey_set(m: &mut HashSet<PathBuf>, remap: &impl Fn(&Path) -> Option<PathBuf>) {
            for k in m.iter().cloned().collect::<Vec<_>>() {
                if let Some(nk) = remap(&k) {
                    m.remove(&k);
                    m.insert(nk);
                }
            }
        }
        rekey(&mut self.notes, &remap);
        rekey(&mut self.initial, &remap);
        rekey(&mut self.in_front, &remap);
        rekey_set(&mut self.seen, &remap);
        rekey_set(&mut self.restored, &remap);
        rekey_set(&mut self.auto, &remap);
        // (the cached frames are made again from the text)
        self.last.clear();
    }

    /// Do a command with the cursor in `line`; the answer is the line the cursor has to go to when
    /// the command hid the one it was in.
    pub fn apply(&mut self, rel: &Path, frame: &Frame, cmd: FoldCmd, line: usize) -> Option<usize> {
        // what the user asks for is theirs: the cursor leaving the front matter does not fold it again
        self.auto.remove(rel);
        let folds = self.notes.entry(rel.to_path_buf()).or_default();
        let moved = folds.apply(&frame.blocks, cmd, line);
        if folds.is_empty() {
            self.notes.remove(rel);
        }
        moved
    }

    /// Unfold what hides `line` (a footnote jump or the outline goes there, EDIT-097); true when something
    /// was unfolded.
    pub fn reveal_line(&mut self, rel: &Path, frame: &Frame, line: usize) -> bool {
        if !frame.view.is_hidden(line) {
            return false;
        }
        let Some(folds) = self.notes.get_mut(rel) else { return false };
        let changed = folds.reveal(&frame.blocks, line);
        if folds.is_empty() {
            self.notes.remove(rel);
        }
        changed
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
        let opens_front = frame.front_summary().is_some_and(|b| b.body().contains(&line));
        let changed = folds.reveal(&frame.blocks, line);
        if folds.is_empty() {
            self.notes.remove(rel);
        }
        if changed && opens_front {
            self.auto.insert(rel.to_path_buf());
        }
        changed
    }

    /// The cursor and the folded front matter (EDIT-081): the cursor entering it (from outside, or a
    /// key pressed with the cursor in it) opens it, and the cursor leaving it folds it again when
    /// the cursor opened it. `line` is the cursor's line while the editor has the keyboard.
    pub fn follow_cursor(&mut self, rel: &Path, frame: &Frame, line: Option<usize>, typed: bool) {
        let Some(fi) = frame.blocks.iter().position(|b| b.kind == FoldKind::Front).filter(|_| frame.summary) else { return };
        let Some(line) = line else {
            self.in_front.remove(rel);
            return;
        };
        let inside = line < frame.blocks[fi].end;
        let was = self.in_front.insert(rel.to_path_buf(), inside);
        let folded = frame.folded.contains(&fi);
        if inside && folded && (was == Some(false) || typed) {
            self.auto.insert(rel.to_path_buf());
            self.set(rel, frame, fi, false);
        } else if !inside && !folded && was == Some(true) && self.auto.remove(rel) {
            self.set(rel, frame, fi, true);
        }
    }

    fn set(&mut self, rel: &Path, frame: &Frame, i: usize, fold: bool) {
        let folds = self.notes.entry(rel.to_path_buf()).or_default();
        folds.sync(&frame.blocks);
        folds.set(&frame.blocks, i, fold);
        if folds.is_empty() {
            self.notes.remove(rel);
        }
    }

    /// Draw the markers and the badges over the laid-out text and do what a click on one asks.
    pub fn paint(&mut self, ui: &mut Ui, frame: &Frame, at: &Painted) {
        let Painted { id, index, rel, galley, pos, area, base } = *at;
        self.widgets.clear();
        let text = galley.job.text.as_str();
        let frame = frame.at(text);
        let painter = ui.painter().with_clip_rect(ui.clip_rect());
        let clip = ui.clip_rect();
        let hover = ui.input(|i| i.pointer.hover_pos());
        let lines = &index.lines;
        // what is far off the screen is skipped without asking the galley where it is (EDIT-105)
        let (top, bottom) = (clip.min.y - pos.y, clip.max.y - pos.y);
        let row_of = |line: usize, offset: usize| {
            let r = galley.pos_from_cursor(CCursor::new(lines[line].0 + offset));
            Rect::from_min_max(pos + r.min.to_vec2(), pos + r.max.to_vec2())
        };
        let mut clicked: Option<(usize, FoldCmd)> = None;
        let folded_lines: HashSet<usize> = frame.folded.iter().map(|&i| frame.blocks[i].header).collect();
        let mut last = None;
        for b in frame.blocks.iter() {
            if last == Some(b.header) || frame.view.is_hidden(b.header) || b.header >= lines.len() {
                continue;
            }
            last = Some(b.header);
            if index.off_screen(lines[b.header].0, top, bottom) {
                continue;
            }
            let row = row_of(b.header, 0);
            if row.max.y < clip.min.y || row.min.y > clip.max.y {
                continue;
            }
            let folded = folded_lines.contains(&b.header);
            let hot = hover.is_some_and(|p| p.y >= row.min.y && p.y <= row.max.y && p.x >= pos.x - MARKER - 6.0 && p.x <= area.right());
            let mut w = Widget { line: b.header, marker: None, badge: None, copy: None };
            if folded || hot {
                let rect = Rect::from_center_size(Pos2::new(pos.x - MARKER / 2.0 - 2.0, row.center().y), Vec2::splat(MARKER));
                if marker_button(ui, id.with(("fold-marker", b.header)), rect, folded) {
                    clicked = Some((b.header, FoldCmd::Toggle));
                }
                w.marker = Some(rect);
            }
            self.widgets.push(w);
        }
        let summary_header = frame.front_summary().map(|b| b.header);
        for &(header, n) in &frame.view.badges {
            if header >= lines.len() || summary_header == Some(header) {
                continue;
            }
            if index.off_screen(lines[header].0 + lines[header].1, top, bottom) {
                continue;
            }
            let end = row_of(header, lines[header].1);
            if end.max.y < clip.min.y || end.min.y > clip.max.y {
                continue;
            }
            let kind = frame.folded.iter().map(|&i| &frame.blocks[i]).filter(|b| b.header == header).max_by_key(|b| b.end).map_or(FoldKind::Heading, |b| b.kind);
            let label = crate::fold::badge_label(kind, frame.mail, n);
            let g = painter.layout_no_wrap(label, egui::FontId::proportional((base * 0.8).max(9.0)), theme::palette().muted);
            let rect = Rect::from_center_size(Pos2::new(end.min.x + 10.0 + (g.size().x + 10.0) / 2.0, end.center().y), g.size() + Vec2::new(10.0, 2.0));
            // takes drags too, so the text box under it does not get the press (EDIT-112)
            let resp = ui.interact(rect, id.with(("fold-badge", header)), Sense::click_and_drag());
            painter.rect_filled(rect, 4.0, if resp.hovered() { theme::palette().bg_active } else { theme::palette().bg_secondary });
            painter.galley(rect.center() - g.size() / 2.0, g, theme::palette().muted);
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.on_hover_text("Unfold").clicked() {
                clicked = Some((header, FoldCmd::Unfold));
            }
            // the widgets are in the order of their lines, like the badges
            match self.widgets.binary_search_by_key(&header, |w| w.line) {
                Ok(i) => self.widgets[i].badge = Some(rect),
                Err(i) => self.widgets.insert(i, Widget { line: header, marker: None, badge: Some(rect), copy: None }),
            }
        }
        // the folded front matter: its summary line in place of `---`, a click opens it (EDIT-081)
        if let Some(b) = frame.front_summary().filter(|b| b.header < lines.len()) {
            let row = row_of(b.header, 0);
            if row.max.y >= clip.min.y && row.min.y <= clip.max.y {
                let label = match crate::properties::summary(text) {
                    Some(s) => format!("Properties \u{b7} {s}"),
                    None => "Properties".to_string(),
                };
                let hit = Rect::from_min_max(Pos2::new(pos.x, row.min.y), Pos2::new(area.right(), row.max.y));
                // takes drags too, so the text box under it does not get the press (EDIT-112)
                let resp = ui.interact(hit, id.with("fold-summary"), Sense::click_and_drag());
                let colour = if resp.hovered() { theme::palette().text } else { theme::palette().muted };
                let g = painter.layout_no_wrap(label, egui::FontId::proportional((base * 0.9).max(9.0)), colour);
                painter.with_clip_rect(hit.intersect(clip)).galley(Pos2::new(pos.x, row.center().y - g.size().y / 2.0), g, colour);
                if resp.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text("Open the properties").clicked() {
                    self.auto.insert(rel.to_path_buf());
                    if let Some(i) = frame.blocks.iter().position(|x| x.kind == FoldKind::Front) {
                        self.set(rel, &frame, i, false);
                    }
                    self.goto = Some(b.header + 1);
                    ui.ctx().request_repaint();
                }
            }
        }
        if let Some((line, cmd)) = clicked {
            self.apply(rel, &frame, cmd, line);
            ui.ctx().request_repaint();
        }
    }
}

/// For each line of `text`: the char index where it starts and how many chars it has (without a `\r`).
pub(crate) fn line_chars(text: &str) -> Vec<(usize, usize)> {
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
