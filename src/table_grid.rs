//! Working in a table drawn as a grid in the editor (EDIT-030, EDIT-031): editing a cell in place
//! and resizing columns, rows and the whole table by their borders. The editor owns one
//! `TableGrid`; what it does to the note comes out as `table::Change`s the editor makes at the
//! start of its next frame.

use crate::markdown;
use crate::table::{Change, Laid, Meta, Sizes, Table, MIN_COL};
use crate::theme;
use eframe::egui::{self, Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use std::ops::Range;

/// Half the width of the strip over a border that resizes (EDIT-031).
const GRAB: f32 = 3.0;

/// The cell being edited (EDIT-030).
pub struct CellEdit {
    /// the table, by its number in the note
    pub table: usize,
    pub row: usize,
    pub col: usize,
    /// the cell's markdown as it is being typed
    pub buf: String,
    /// the note before the first change of this editing (the undo step starts there)
    pub before: Option<String>,
    /// the cell's box had the keyboard (losing it afterwards ends the editing)
    had_focus: bool,
    /// the selection in the box (chars: primary, secondary) when it last had the keyboard: a press on the
    /// toolbar takes the keyboard away, and the box then drops its selection (EDIT-030)
    sel: (usize, usize),
    /// the toolbar had the pointer last frame
    held: bool,
    /// the cursor was on the first / the last line of the box's text last frame (Up and Down leave the cell there)
    first_line: bool,
    last_line: bool,
}

/// Where the cursor goes in a cell that is entered.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Caret {
    Start,
    End,
}

/// Which border is being dragged.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Border {
    /// the border right of a column (not the last)
    Col(usize),
    /// the border under a row (not the last)
    Row(usize),
    /// the right edge, the bottom edge, the bottom right corner: the whole table
    Right,
    Bottom,
    Corner,
}

/// A resize being dragged: the table, the border, the sizes when it began and where the pointer
/// was, and the sizes it has now.
struct Drag {
    table: usize,
    border: Border,
    start: Sizes,
    from: Pos2,
    now: Sizes,
}

/// How a grid is shown this frame.
pub struct Shown<'a> {
    /// the editor's id (the text box); the grid's own ids are made from it
    pub id: egui::Id,
    pub ordinal: usize,
    pub origin: Pos2,
    pub laid: &'a Laid,
    pub table: &'a Table,
    pub meta: &'a Meta,
    /// a click edits a cell (else it goes to the text, EDIT-024)
    pub cells: bool,
}

#[derive(Default)]
pub struct TableGrid {
    pub cell: Option<CellEdit>,
    drag: Option<Drag>,
    /// changes for the start of the next frame: (table, change, a step of its own in the undo history)
    pub pending: Vec<(usize, Change, bool)>,
    /// an editing that ended: the note before it began (for its undo step) and whether the text box gets the keyboard back
    pub ended: Option<(Option<String>, bool)>,
    /// the table of the cell being edited was drawn as a grid this frame
    seen: bool,
    /// the pointer is on the formatting toolbar this frame: losing the keyboard to it does not end the editing (EDIT-030)
    pub hold: bool,
}

impl CellEdit {
    /// The selection of the box as bytes of `buf` (a cursor is an empty range).
    pub fn selection_bytes(&self) -> Range<usize> {
        let (a, b) = (markdown::char_to_byte(&self.buf, self.sel.0), markdown::char_to_byte(&self.buf, self.sel.1));
        a.min(b)..a.max(b)
    }
}

/// The text of a cell of `table` (row 0 is the header).
fn cell_text(table: &Table, row: usize, col: usize) -> Option<String> {
    let r = if row == 0 { Some(&table.header) } else { table.rows.get(row - 1) };
    r.and_then(|r| r.get(col)).cloned()
}

/// The sizes when a border is dragged by `d` from `start`.
fn dragged(start: &Sizes, border: Border, d: Vec2) -> Sizes {
    let mut s = start.clone();
    let scale = |v: &mut Vec<f32>, by: f32, least: f32| {
        let total: f32 = v.iter().sum();
        if total > 0.0 {
            let k = ((total + by) / total).max(0.05);
            v.iter_mut().for_each(|x| *x = (*x * k).max(least));
        }
    };
    match border {
        Border::Col(c) => s.cols[c] = (s.cols[c] + d.x).max(MIN_COL),
        Border::Row(r) => s.rows[r] = (s.rows[r] + d.y).max(1.0),
        Border::Right => scale(&mut s.cols, d.x, MIN_COL),
        Border::Bottom => scale(&mut s.rows, d.y, 1.0),
        Border::Corner => {
            scale(&mut s.cols, d.x, MIN_COL);
            scale(&mut s.rows, d.y, 1.0);
        }
    }
    s
}

/// The sizes after a double click on a border: every column (row) the same share of the whole.
fn equalised(start: &Sizes, border: Border) -> Sizes {
    let mut s = start.clone();
    let even = |v: &mut Vec<f32>| {
        let share = v.iter().sum::<f32>() / v.len().max(1) as f32;
        v.iter_mut().for_each(|x| *x = share);
    };
    match border {
        Border::Col(_) | Border::Right => even(&mut s.cols),
        Border::Row(_) | Border::Bottom => even(&mut s.rows),
        Border::Corner => {
            even(&mut s.cols);
            even(&mut s.rows);
        }
    }
    s
}

impl TableGrid {
    /// The sizes a table has while one of its borders is dragged (else those it stores).
    pub fn sizes<'a>(&'a self, table: usize, stored: &'a Sizes) -> &'a Sizes {
        self.drag.as_ref().filter(|d| d.table == table).map_or(stored, |d| &d.now)
    }

    /// Whether a cell of this table is being edited (the editor then keeps it a grid).
    pub fn editing(&self, table: usize) -> bool {
        self.cell.as_ref().is_some_and(|c| c.table == table)
    }

    /// Call before the grids are drawn.
    pub fn begin_frame(&mut self) {
        self.seen = false;
    }

    /// Do a toolbar command to the text of the cell being edited (EDIT-030, EDIT-027): the same text
    /// transform as for the note, the result selected, the keyboard back in the cell's box and the
    /// change part of the editing's undo step. False when no cell is edited, the command makes no
    /// sense in a cell or changes nothing.
    pub fn format(&mut self, ctx: &egui::Context, id: egui::Id, table: usize, cmd: crate::format::Cmd) -> bool {
        let Some(c) = self.cell.as_mut().filter(|c| c.table == table && crate::format::works_in_a_cell(cmd)) else { return false };
        let box_id = Self::cell_id(id);
        ctx.memory_mut(|m| m.request_focus(box_id));
        let Some((new, r)) = crate::format::apply(cmd, &c.buf, c.selection_bytes()) else { return false };
        let sel = (markdown::byte_to_char(&new, r.start), markdown::byte_to_char(&new, r.end));
        c.buf = new;
        c.sel = sel;
        let mut state = egui::text_edit::TextEditState::load(ctx, box_id).unwrap_or_default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(sel.0), egui::text::CCursor::new(sel.1))));
        state.store(ctx, box_id);
        self.pending.push((table, Change::Cell { row: c.row, col: c.col, text: c.buf.clone() }, false));
        ctx.request_repaint();
        true
    }

    /// Call after the grids were drawn: an editing whose table is no longer a grid ends.
    pub fn end_frame(&mut self) {
        self.hold = false;
        if self.cell.is_some() && !self.seen {
            self.finish(false);
        }
    }

    fn finish(&mut self, give_back: bool) {
        if let Some(c) = self.cell.take() {
            self.ended = Some((c.before, give_back));
        }
    }

    fn start(&mut self, ui: &Ui, s: &Shown, row: usize, col: usize, caret: Caret) {
        let Some(buf) = cell_text(s.table, row, col) else { return };
        // a cell of the same editing keeps the undo step going
        let before = self.cell.take().and_then(|c| c.before);
        let id = Self::cell_id(s.id);
        let at = if caret == Caret::Start { 0 } else { buf.chars().count() };
        let mut state = egui::text_edit::TextEditState::default();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
        state.store(ui.ctx(), id);
        self.cell = Some(CellEdit { table: s.ordinal, row, col, buf, before, had_focus: false, held: false, sel: (at, at), first_line: true, last_line: true });
        ui.ctx().memory_mut(|m| m.request_focus(id));
        ui.ctx().request_repaint();
    }

    pub fn cell_id(id: egui::Id) -> egui::Id {
        id.with("table-cell-edit")
    }

    /// Draw what belongs to the grid over it: the box of the cell being edited, the strip that
    /// takes the clicks on the cells (cells mode) and the resize borders.
    pub fn show(&mut self, ui: &mut Ui, s: &Shown) {
        let grid = Rect::from_min_size(s.origin, s.laid.size());
        if s.cells {
            // the clicks on the cells (and the drags: else the text box under them takes them)
            let resp = ui.interact(grid, s.id.with(("table-cells", s.ordinal)), Sense::click_and_drag());
            if resp.clicked() {
                if let Some((row, col, _)) = resp.interact_pointer_pos().and_then(|p| s.laid.cell_at(s.origin, p)) {
                    self.start(ui, s, row, col, Caret::End);
                }
            }
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Text);
            }
        }
        if self.editing(s.ordinal) {
            self.seen = true;
            self.cell_box(ui, s);
        }
        self.borders(ui, s, grid);
    }

    /// The box over the cell being edited, and its keys (EDIT-030). The text wraps in the
    /// column's width; the box grows down over the rows below when it needs more room.
    fn cell_box(&mut self, ui: &mut Ui, s: &Shown) {
        let id = Self::cell_id(s.id);
        let Some(c) = self.cell.as_mut() else { return };
        let (nrows, ncols) = (s.laid.heights.len(), s.laid.widths.len());
        if c.row >= nrows || c.col >= ncols {
            self.finish(true);
            return;
        }
        // the keys that move go before the box sees them (a line break never goes into a cell)
        let focused = ui.ctx().memory(|m| m.has_focus(id));
        let take = |ui: &mut Ui, key: Key, m: Modifiers| focused && ui.input_mut(|i| i.consume_key(m, key));
        let flat = c.row * ncols + c.col;
        let cells = nrows * ncols;
        // the arrows leave the cell at the edge of its text (no modifiers, nothing selected)
        let caret = egui::text_edit::TextEditState::load(ui.ctx(), id).and_then(|st| st.cursor.char_range());
        let len = c.buf.chars().count();
        let (at_start, at_end) = caret.filter(|r| r.primary == r.secondary).map_or((false, false), |r| (r.primary.index.0 == 0, r.primary.index.0 >= len));
        let plain = focused && ui.input(|i| i.modifiers.is_none());
        let arrow = |ui: &mut Ui, key: Key, ok: bool, to: Option<usize>| match to.filter(|t| plain && ok && *t < cells) {
            Some(t) if ui.input_mut(|i| i.consume_key(Modifiers::NONE, key)) => Some(t),
            _ => None,
        };
        let moved = arrow(ui, Key::ArrowRight, at_end, Some(flat + 1))
            .map(|t| (t, Caret::Start))
            .or_else(|| arrow(ui, Key::ArrowLeft, at_start, flat.checked_sub(1)).map(|t| (t, Caret::End)))
            .or_else(|| arrow(ui, Key::ArrowUp, c.first_line, flat.checked_sub(ncols)).map(|t| (t, Caret::End)))
            .or_else(|| arrow(ui, Key::ArrowDown, c.last_line, Some(flat + ncols)).map(|t| (t, Caret::End)));
        if let Some((t, caret)) = moved {
            return self.start(ui, s, t / ncols, t % ncols, caret);
        }
        let target = if take(ui, Key::Escape, Modifiers::NONE) {
            Some(None)
        // with Shift first: a key without modifiers would take the Shift one as well
        } else if take(ui, Key::Tab, Modifiers::SHIFT) {
            Some(flat.checked_sub(1))
        } else if take(ui, Key::Tab, Modifiers::NONE) {
            Some(Some(flat + 1))
        } else if take(ui, Key::Enter, Modifiers::SHIFT) {
            Some(flat.checked_sub(ncols))
        } else if take(ui, Key::Enter, Modifiers::NONE) {
            Some(Some(flat + ncols))
        } else {
            None
        };
        let rect = s.laid.cell_rect(s.origin, c.row, c.col);
        // the size of the cell's own text
        let size = s.laid.cells[c.row][c.col].galley.job.sections.iter().map(|x| x.format.font_id.size).fold(0.0, f32::max);
        let font = egui::FontId::proportional(if size > 2.0 { size } else { 14.0 });
        // the background goes under the box, whose height is known once it is drawn
        let back = ui.painter().add(egui::Shape::Noop);
        if self.hold || c.held {
            // the toolbar had the pointer (this frame or the last): the box drops its selection when it loses the keyboard to a press there
            let mut state = egui::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(c.sel.0), egui::text::CCursor::new(c.sel.1))));
            state.store(ui.ctx(), id);
        }
        c.held = self.hold;
        let inner = Rect::from_min_size(rect.min + Vec2::new(6.0, 3.0), Vec2::new(rect.width() - 12.0, f32::INFINITY));
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                egui::TextEdit::multiline(&mut c.buf)
                    .id(id)
                    .frame(egui::Frame::NONE)
                    .desired_width(inner.width())
                    .desired_rows(1)
                    .font(font)
                    .event_filter(egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true })
                    .show(ui)
            })
            .inner;
        // which line of the box the cursor is on, for Up and Down next frame
        if let Some(r) = out.cursor_range {
            if !self.hold {
                c.sel = (r.primary.index.0, r.secondary.index.0);
            }
            let at = out.galley.pos_from_cursor(r.primary);
            let rows = &out.galley.rows;
            c.first_line = rows.first().is_none_or(|row| at.center().y < row.rect().max.y);
            c.last_line = rows.last().is_none_or(|row| at.center().y > row.rect().min.y);
        }
        let resp = out.response.response;
        let shown = Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.max.y.max(resp.rect.bottom() + 3.0))).shrink(1.0);
        ui.painter().set(back, egui::Shape::rect_filled(shown, 0.0, theme::BG_PRIMARY));
        ui.painter().rect_stroke(shown, 0.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
        if resp.changed() {
            // a pasted line break becomes a blank (set_cell does it in the note as well)
            if c.buf.contains(['\n', '\r']) {
                c.buf = c.buf.replace(['\n', '\r'], " ");
            }
            self.pending.push((s.ordinal, Change::Cell { row: c.row, col: c.col, text: c.buf.clone() }, false));
        }
        match target {
            Some(Some(t)) if t < cells => return self.start(ui, s, t / ncols, t % ncols, Caret::End),
            // Esc, or past the first or the last cell: the editing ends
            Some(_) => return self.finish(true),
            None => {}
        }
        if resp.has_focus() {
            c.had_focus = true;
        } else if c.had_focus && !self.hold {
            // a click elsewhere took the keyboard
            self.finish(false);
        } else if c.had_focus {
            // the toolbar took it: the cell keeps the editing
            ui.ctx().memory_mut(|m| m.request_focus(id));
        } else {
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
    }

    /// The resize strips over the borders (EDIT-031).
    fn borders(&mut self, ui: &mut Ui, s: &Shown, grid: Rect) {
        let laid = s.laid;
        let (ncols, nrows) = (laid.widths.len(), laid.heights.len());
        let mut strips: Vec<(Border, Rect)> = Vec::new();
        let mut x = grid.left();
        for (c, w) in laid.widths.iter().enumerate().take(ncols - 1) {
            x += w;
            strips.push((Border::Col(c), Rect::from_x_y_ranges(x - GRAB..=x + GRAB, grid.y_range())));
        }
        let mut y = grid.top();
        for (r, h) in laid.heights.iter().enumerate().take(nrows - 1) {
            y += h;
            strips.push((Border::Row(r), Rect::from_x_y_ranges(grid.x_range(), y - GRAB..=y + GRAB)));
        }
        strips.push((Border::Right, Rect::from_x_y_ranges(grid.right() - GRAB..=grid.right() + GRAB, grid.y_range())));
        strips.push((Border::Bottom, Rect::from_x_y_ranges(grid.x_range(), grid.bottom() - GRAB..=grid.bottom() + GRAB - 1.0)));
        strips.push((Border::Corner, Rect::from_center_size(grid.max, Vec2::splat(4.0 * GRAB))));
        let actual = Sizes { cols: laid.widths.clone(), rows: laid.heights.clone() };
        let mut placed = Vec::new();
        for (border, rect) in strips {
            let resp = ui.interact(rect, s.id.with(("table-border", s.ordinal, format!("{border:?}"))), Sense::click_and_drag());
            if resp.hovered() || resp.dragged() {
                ui.ctx().set_cursor_icon(match border {
                    Border::Col(_) | Border::Right => egui::CursorIcon::ResizeHorizontal,
                    Border::Row(_) | Border::Bottom => egui::CursorIcon::ResizeVertical,
                    Border::Corner => egui::CursorIcon::ResizeNwSe,
                });
            }
            if resp.double_clicked() {
                self.pending.push((s.ordinal, Change::Meta(Meta { cells: s.meta.cells, sizes: equalised(&actual, border) }), true));
                ui.ctx().request_repaint();
            } else if resp.drag_started() {
                let from = resp.interact_pointer_pos().unwrap_or(rect.center());
                self.drag = Some(Drag { table: s.ordinal, border, start: actual.clone(), from, now: actual.clone() });
            } else if resp.dragged() {
                if let (Some(d), Some(p)) = (self.drag.as_mut().filter(|d| d.table == s.ordinal && d.border == border), ui.input(|i| i.pointer.interact_pos())) {
                    d.now = dragged(&d.start, border, p - d.from);
                    ui.ctx().request_repaint();
                }
            } else if resp.drag_stopped() {
                if let Some(d) = self.drag.take().filter(|d| d.table == s.ordinal && d.border == border) {
                    if d.now != d.start {
                        self.pending.push((s.ordinal, Change::Meta(Meta { cells: s.meta.cells, sizes: d.now }), true));
                    }
                }
            }
            placed.push((format!("{border:?}"), rect));
        }
        ui.ctx().data_mut(|d| d.insert_temp(s.id.with(("table-borders", s.ordinal)), placed));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes(cols: &[f32], rows: &[f32]) -> Sizes {
        Sizes { cols: cols.to_vec(), rows: rows.to_vec() }
    }

    // spec: EDIT-031
    #[test]
    fn a_border_resizes_its_column_or_row_and_the_edges_the_whole_table() {
        let s = sizes(&[100.0, 50.0], &[30.0, 30.0, 40.0]);
        assert_eq!(dragged(&s, Border::Col(0), Vec2::new(20.0, 9.0)), sizes(&[120.0, 50.0], &[30.0, 30.0, 40.0]));
        assert_eq!(dragged(&s, Border::Col(1), Vec2::new(-200.0, 0.0)).cols, [100.0, MIN_COL], "never narrower than MIN_COL");
        assert_eq!(dragged(&s, Border::Row(1), Vec2::new(0.0, 10.0)).rows, [30.0, 40.0, 40.0]);
        assert_eq!(dragged(&s, Border::Right, Vec2::new(150.0, 0.0)).cols, [200.0, 100.0], "in proportion");
        assert_eq!(dragged(&s, Border::Bottom, Vec2::new(0.0, -50.0)).rows, [15.0, 15.0, 20.0]);
        assert_eq!(dragged(&s, Border::Corner, Vec2::new(150.0, 100.0)), sizes(&[200.0, 100.0], &[60.0, 60.0, 80.0]));
    }

    // spec: EDIT-031
    #[test]
    fn a_double_click_shares_the_width_or_the_height_equally() {
        let s = sizes(&[100.0, 50.0, 30.0], &[30.0, 60.0]);
        assert_eq!(equalised(&s, Border::Col(1)), sizes(&[60.0, 60.0, 60.0], &[30.0, 60.0]));
        assert_eq!(equalised(&s, Border::Right).cols, [60.0, 60.0, 60.0]);
        assert_eq!(equalised(&s, Border::Row(0)), sizes(&[100.0, 50.0, 30.0], &[45.0, 45.0]));
        assert_eq!(equalised(&s, Border::Bottom).rows, [45.0, 45.0]);
        assert_eq!(equalised(&s, Border::Corner), sizes(&[60.0, 60.0, 60.0], &[45.0, 45.0]));
    }

    // spec: EDIT-030
    #[test]
    fn the_text_of_a_cell_comes_from_the_header_or_its_row() {
        let t = Table { aligns: vec![crate::table::Col::Left; 2], header: vec!["A".into(), "B".into()], rows: vec![vec!["1".into(), "2".into()]] };
        assert_eq!(cell_text(&t, 0, 1).as_deref(), Some("B"));
        assert_eq!(cell_text(&t, 1, 0).as_deref(), Some("1"));
        assert_eq!(cell_text(&t, 2, 0), None);
    }
}
