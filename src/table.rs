//! Markdown pipe tables (EDIT-024): finding and parsing them in a note, working out the column
//! widths, and laying out and painting them as a real grid. The editor's preview and the reading
//! view both use this module.

use crate::markdown::{self, LinkSpan};
use crate::theme;
use eframe::egui::{self, text::LayoutJob, Align, Color32, Galley, Pos2, Rect, Stroke, Vec2};
use std::ops::Range;
use std::sync::Arc;

/// How the text of a column sits in its cells (`:---`, `:---:`, `---:`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Col {
    Left,
    Center,
    Right,
}

/// A parsed table: the cells are the markdown text of each cell (inline formatting still in it,
/// `\|` already turned into `|`). Every row has exactly one cell per column.
#[derive(Clone, PartialEq, Debug)]
pub struct Table {
    pub aligns: Vec<Col>,
    pub header: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

/// A table in a note: the lines it occupies (numbers from 0, end excluded; its comment line
/// included), its content and what its comment line says (EDIT-030).
#[derive(Clone, PartialEq, Debug)]
pub struct Block {
    pub lines: Range<usize>,
    pub table: Table,
    /// the first of `lines` is the table's comment line
    pub has_meta: bool,
    pub meta: Meta,
}

impl Block {
    /// The lines of the table itself: the header, the delimiter row and the data rows.
    pub fn table_lines(&self) -> Range<usize> {
        self.lines.start + usize::from(self.has_meta)..self.lines.end
    }
}

/// The sizes a person gave a table's grid (EDIT-031), in points with the padding: the width of
/// each column and the least height of each row (the header first). Empty: worked out.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Sizes {
    pub cols: Vec<f32>,
    pub rows: Vec<f32>,
}

/// What a table keeps in the comment line above it (EDIT-030): `Some(true)` edit the cells in the
/// grid, `Some(false)` edit the raw markdown, `None` as the setting says; and its sizes.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Meta {
    pub cells: Option<bool>,
    pub sizes: Sizes,
}

const META_TAG: &str = "shale-table";

/// The comment line of a table, `<!-- shale-table edit=cells cols=120,80 rows=30,30 -->`, read;
/// unknown words and broken numbers are ignored. `None` when the line is no such comment.
pub fn parse_meta(line: &str) -> Option<Meta> {
    let inner = line.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let rest = inner.strip_prefix(META_TAG)?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let mut m = Meta::default();
    for word in rest.split_whitespace() {
        let Some((k, v)) = word.split_once('=') else { continue };
        let nums = || v.split(',').map(|x| x.trim().parse::<f32>().ok().filter(|f| f.is_finite() && *f >= 0.0)).collect::<Option<Vec<f32>>>().unwrap_or_default();
        match k {
            "edit" => {
                m.cells = match v {
                    "cells" => Some(true),
                    "raw" => Some(false),
                    _ => None,
                }
            }
            "cols" => m.sizes.cols = nums(),
            "rows" => m.sizes.rows = nums(),
            _ => {}
        }
    }
    Some(m)
}

/// The comment line for `m` (without a line break); `None` when there is nothing to keep.
pub fn meta_line(m: &Meta) -> Option<String> {
    let list = |v: &[f32]| v.iter().map(|f| (f.round() as i64).to_string()).collect::<Vec<_>>().join(",");
    let mut parts = Vec::new();
    if let Some(c) = m.cells {
        parts.push(format!("edit={}", if c { "cells" } else { "raw" }));
    }
    if !m.sizes.cols.is_empty() {
        parts.push(format!("cols={}", list(&m.sizes.cols)));
    }
    if !m.sizes.rows.is_empty() {
        parts.push(format!("rows={}", list(&m.sizes.rows)));
    }
    (!parts.is_empty()).then(|| format!("<!-- {META_TAG} {} -->", parts.join(" ")))
}

/// The cells of one table line as they are written: byte ranges between the separating pipes
/// (spaces included), and whether the line has an outer pipe at the start and at the end. The
/// ranges are in `line`; `end` is where the line's text ends (before trailing blanks and `\r`).
/// `\|` and pipes inside code spans do not separate.
struct RawLine {
    cells: Vec<Range<usize>>,
    trailing: bool,
    end: usize,
}

fn raw_line(line: &str) -> RawLine {
    let end = line.trim_end().len();
    let start = (line.len() - line.trim_start().len()).min(end);
    let leading = line[start..end].starts_with('|');
    let begin = start + usize::from(leading);
    let body = &line[begin..end];
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let mut cells = Vec::new();
    let mut from = begin;
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if c == '\\' && chars.get(i + 1).is_some_and(|x| x.1 == '|') {
            i += 2;
        } else if c == '`' {
            // a code span runs to the next backtick on the line; without one it is a plain character
            match chars[i + 1..].iter().position(|x| x.1 == '`') {
                Some(e) => i += e + 2,
                None => i += 1,
            }
        } else if c == '|' {
            cells.push(from..begin + at);
            from = begin + at + 1;
            i += 1;
        } else {
            i += 1;
        }
    }
    // text after the last pipe is a cell; nothing after it means the line ended with a pipe
    let trailing = from == end;
    if !trailing {
        cells.push(from..end);
    }
    RawLine { cells, trailing, end }
}

/// The byte ranges of the cells of one table line, between its pipes (blanks included).
pub fn cell_ranges(line: &str) -> Vec<Range<usize>> {
    raw_line(line).cells
}

/// The text of a cell as it is shown: `\|` is a pipe (but not inside a code span), trimmed.
fn cell_text(raw: &str) -> String {
    let chars: Vec<char> = raw.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'|') {
            out.push('|');
            i += 2;
        } else if chars[i] == '`' {
            let n = chars[i + 1..].iter().position(|&x| x == '`').map_or(1, |e| e + 2);
            out.extend(&chars[i..i + n]);
            i += n;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out.trim().to_string()
}

/// Split one table line into its cells: the outer pipes are optional, `\|` is a pipe inside a
/// cell, and a pipe inside a code span does not split. Cells are trimmed.
pub fn split_cells(line: &str) -> Vec<String> {
    raw_line(line).cells.iter().map(|r| cell_text(&line[r.clone()])).collect()
}

/// The alignments of a delimiter row (`|---|:---:|--:|`), or none when the line is not one.
pub fn delimiter_row(line: &str) -> Option<Vec<Col>> {
    if !line.contains('|') {
        return None;
    }
    let cells = split_cells(line);
    if cells.is_empty() {
        return None;
    }
    cells
        .iter()
        .map(|c| {
            let left = c.starts_with(':');
            let right = c.len() > 1 && c.ends_with(':');
            let dashes = c.trim_start_matches(':').trim_end_matches(':');
            (!dashes.is_empty() && dashes.bytes().all(|b| b == b'-')).then_some(match (left, right) {
                (true, true) => Col::Center,
                (false, true) => Col::Right,
                _ => Col::Left,
            })
        })
        .collect()
}

/// The tables of a note, in order. Fenced code and front matter are skipped. A table is a header
/// line, a delimiter row with as many cells, and the lines after it that hold a pipe, up to a
/// blank line. Rows with too few cells are filled up, extra cells are dropped.
pub fn find_tables(text: &str) -> Vec<Block> {
    let lines = markdown::lines_with_offsets(text);
    let fenced = markdown::fenced_code_lines(text);
    let mut i = 0;
    if lines.first().is_some_and(|(_, l)| l.trim() == "---") {
        if let Some(end) = lines.iter().skip(1).position(|(_, l)| l.trim() == "---") {
            i = end + 2;
        }
    }
    let body = i;
    let mut out: Vec<Block> = Vec::new();
    while i < lines.len() {
        let header = lines[i].1;
        let is_table = !fenced[i] && header.contains('|') && i + 1 < lines.len() && !fenced[i + 1];
        let aligns = if is_table { delimiter_row(lines[i + 1].1) } else { None };
        let head = split_cells(header);
        let Some(aligns) = aligns.filter(|a| a.len() == head.len()) else {
            i += 1;
            continue;
        };
        let mut j = i + 2;
        let mut rows = Vec::new();
        while j < lines.len() && !fenced[j] && !lines[j].1.trim().is_empty() && lines[j].1.contains('|') {
            let mut cells = split_cells(lines[j].1);
            cells.resize(head.len(), String::new());
            rows.push(cells);
            j += 1;
        }
        // the comment line right above it belongs to the table (EDIT-030)
        let free = i > body && !fenced[i - 1] && out.last().is_none_or(|b| b.lines.end < i);
        let meta = free.then(|| parse_meta(lines[i - 1].1)).flatten();
        let has_meta = meta.is_some();
        out.push(Block { lines: i - usize::from(has_meta)..j, table: Table { aligns, header: head, rows }, has_meta, meta: meta.unwrap_or_default() });
        i = j;
    }
    out
}

/// The byte range of the lines `lines` of `text`, each with its line break (the last line of the
/// note has none).
pub fn byte_range(text: &str, lines: &Range<usize>) -> Range<usize> {
    let all = markdown::lines_with_offsets(text);
    let start = all.get(lines.start).map_or(text.len(), |(o, _)| *o);
    let end = all.get(lines.end).map_or(text.len(), |(o, _)| *o);
    start..end
}

/// A change of a table's source text made by the buttons of the editor's grid (EDIT-025).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edit {
    /// a new empty column after column `n` (from 0), with `---` in the delimiter row
    InsertColumn(usize),
    DeleteColumn(usize),
    /// a new empty row after data row `n` (from 0), or `None`: right below the header
    InsertRow(Option<usize>),
    /// delete data row `n` (from 0); the header cannot be deleted
    DeleteRow(usize),
}

/// A cell to put after cell `col` of a line: the pipe and the text, written like its neighbours.
fn with_cell_after(line: &str, col: usize, text: &str) -> String {
    let raw = raw_line(line);
    let mut out = line.to_string();
    // a short row is filled up to the cell first
    if col + 1 > raw.cells.len() {
        let mut fill = String::new();
        if !raw.trailing {
            fill.push_str(" |");
        }
        fill.push_str(&"  |".repeat(col + 1 - raw.cells.len()));
        fill.push_str(&format!(" {text} |"));
        out.insert_str(raw.end, &fill);
        return out;
    }
    let cell = &raw.cells[col];
    if cell.end < raw.end || raw.trailing {
        // a pipe follows the cell: the new cell goes behind it
        out.insert_str(cell.end + 1, &format!(" {text} |"));
    } else {
        // the last cell has no pipe behind it: a pipe, the new cell and a closing pipe
        out.insert_str(raw.end, &format!(" | {text} |"));
    }
    out
}

/// `line` without cell `col`, the rest as written.
fn without_cell(line: &str, col: usize) -> String {
    let raw = raw_line(line);
    let Some(cell) = raw.cells.get(col) else { return line.to_string() };
    let last_unclosed = col + 1 == raw.cells.len() && !raw.trailing;
    let range = if last_unclosed { cell.start.saturating_sub(1)..cell.end } else { cell.start..cell.end + 1 };
    let mut out = line.to_string();
    out.replace_range(range, "");
    out
}

/// An empty row of `n` cells, written like `like` (its indentation and line end are kept).
fn empty_row(like: &str, n: usize) -> String {
    let indent = &like[..like.len() - like.trim_start().len()];
    format!("{indent}|{}{}", "   |".repeat(n), if like.ends_with('\r') { "\r" } else { "" })
}

/// The table in `src` (the text of its lines, the last with or without its line break) after
/// `edit`: only the cells or lines concerned change, everything else stays as written. `None`
/// when the edit is not allowed or does not fit the table: the header row, the last column and
/// the last data row cannot be deleted, a column or row that is not there.
pub fn edit_source(src: &str, edit: Edit) -> Option<String> {
    let (body, tail) = src.strip_suffix('\n').map_or((src, ""), |b| (b, "\n"));
    let mut lines: Vec<String> = body.split('\n').map(str::to_string).collect();
    if lines.len() < 2 {
        return None;
    }
    let ncols = split_cells(&lines[0]).len();
    let data = lines.len() - 2;
    match edit {
        Edit::InsertColumn(after) if after < ncols => {
            for (i, l) in lines.iter_mut().enumerate() {
                let text = match i {
                    0 => "",
                    1 => "---",
                    _ => "",
                };
                *l = with_cell_after(l, after, text);
            }
        }
        Edit::DeleteColumn(col) if col < ncols && ncols > 1 => {
            for l in &mut lines {
                *l = without_cell(l, col);
            }
        }
        Edit::InsertRow(after) if after.is_none_or(|r| r < data) => {
            let at = after.map_or(2, |r| r + 3);
            let new = empty_row(&lines[at - 1], ncols);
            lines.insert(at, new);
        }
        Edit::DeleteRow(row) if row < data && data > 1 => {
            lines.remove(row + 2);
        }
        _ => return None,
    }
    Some(lines.join("\n") + tail)
}

/// A cell's text as it is written between its pipes: a pipe outside a code span becomes `\\|`, a
/// line break a blank (EDIT-030).
fn escape_cell(text: &str) -> String {
    let chars: Vec<char> = text.chars().map(|c| if c == '\n' || c == '\r' { ' ' } else { c }).collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '`' => {
                let n = chars[i + 1..].iter().position(|&x| x == '`').map_or(1, |e| e + 2);
                out.extend(&chars[i..i + n]);
                i += n;
            }
            '|' => {
                out.push_str("\\|");
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.trim().to_string()
}

/// The table in `src` (its lines as in `edit_source`) with the cell in `row` (0 is the header, the
/// delimiter row is not counted) and column `col` set to `text`; the other cells stay as written,
/// a short row is filled up. `None` when there is no such cell.
pub fn set_cell(src: &str, row: usize, col: usize, text: &str) -> Option<String> {
    let (body, tail) = src.strip_suffix('\n').map_or((src, ""), |b| (b, "\n"));
    let mut lines: Vec<String> = body.split('\n').map(str::to_string).collect();
    if lines.len() < 2 || col >= split_cells(&lines[0]).len() {
        return None;
    }
    let at = if row == 0 { 0 } else { row + 1 };
    let line = lines.get(at)?.clone();
    let raw = raw_line(&line);
    let text = escape_cell(text);
    lines[at] = match raw.cells.get(col) {
        Some(cell) => {
            // an empty cell with no pipe on its open side would not be read as a cell: blanks
            // before the first cell's text are not a cell, and nor are blanks after the last pipe
            let open = col == 0 && !line.trim_start().starts_with('|');
            let close = cell.end == raw.end && !raw.trailing;
            let blank = text.is_empty();
            let (before, after) = (if open && blank { "|" } else { "" }, if close && blank { "|" } else { "" });
            let mut l = line.clone();
            l.replace_range(cell.clone(), &format!("{before} {text} {after}"));
            l
        }
        None if col == 0 => format!("{}| {text} |{}", &line[..line.len() - line.trim_start().len()], if line.ends_with('\r') { "\r" } else { "" }),
        None => with_cell_after(&line, col - 1, &text),
    };
    Some(lines.join("\n") + tail)
}

/// A change of a table made in the editor's grid: a button (EDIT-025), its comment line (the mode
/// and the sizes, EDIT-030, EDIT-031) or the text of a cell (EDIT-030).
#[derive(Clone, PartialEq, Debug)]
pub enum Change {
    Edit(Edit),
    Meta(Meta),
    Cell { row: usize, col: usize, text: String },
}

/// The sizes after a button's change: a new column or row gets a size, a deleted one loses its
/// own. Sizes that did not match the table stay as they were (they are ignored anyway).
fn sizes_after(sizes: &Sizes, ncols: usize, nrows: usize, edit: Edit) -> Sizes {
    let mut s = sizes.clone();
    let cols_fit = s.cols.len() == ncols;
    let rows_fit = s.rows.len() == nrows;
    match edit {
        Edit::InsertColumn(after) if cols_fit => {
            let w = s.cols[after];
            s.cols.insert(after + 1, w);
        }
        Edit::DeleteColumn(col) if cols_fit => {
            s.cols.remove(col);
        }
        // a new row is as tall as its text
        Edit::InsertRow(after) if rows_fit => s.rows.insert(after.map_or(1, |r| r + 2), 0.0),
        Edit::DeleteRow(row) if rows_fit => {
            s.rows.remove(row + 1);
        }
        _ => {}
    }
    s
}

/// The text of a block (its comment line, if it has one, and its table lines, as `byte_range`
/// gives them) after `change`; the comment line is written, changed or removed as its content
/// asks. `None` when the change does not fit the table.
pub fn change_block(src: &str, has_meta: bool, meta: &Meta, change: &Change) -> Option<String> {
    let (old_meta, table) = if has_meta {
        let cut = src.find('\n').map_or(src.len(), |p| p + 1);
        (Some(&src[..cut]), &src[cut..])
    } else {
        (None, src)
    };
    let (new_table, new_meta) = match change {
        Change::Edit(e) => {
            let t = edit_source(table, *e)?;
            let ncols = split_cells(table.lines().next()?).len();
            let nrows = table.lines().count().saturating_sub(1);
            (t, Meta { cells: meta.cells, sizes: sizes_after(&meta.sizes, ncols, nrows, *e) })
        }
        Change::Meta(m) => (table.to_string(), m.clone()),
        Change::Cell { row, col, text } => (set_cell(table, *row, *col, text)?, meta.clone()),
    };
    let crlf = old_meta.map_or(table.split('\n').next().is_some_and(|l| l.ends_with('\r')), |m| m.ends_with("\r\n"));
    let mut out = String::new();
    if let Some(line) = meta_line(&new_meta) {
        out.push_str(&line);
        out.push_str(if crlf { "\r\n" } else { "\n" });
    }
    out.push_str(&new_table);
    Some(out)
}

/// The narrowest a column can be made (EDIT-031).
pub const MIN_COL: f32 = 24.0;

/// Column widths for the text of the columns: the natural width (the text on one line) of each
/// column when it all fits into `avail`; otherwise every column keeps at least its `min` (its
/// longest word, never more than the natural width) and the rest is shared in proportion to how
/// much more each column wants. When even the minimums do not fit they are scaled down together.
pub fn column_widths(natural: &[f32], min: &[f32], avail: f32) -> Vec<f32> {
    if natural.iter().sum::<f32>() <= avail {
        return natural.to_vec();
    }
    let floor: Vec<f32> = natural.iter().zip(min).map(|(n, m)| m.min(*n)).collect();
    let floor_sum: f32 = floor.iter().sum();
    if floor_sum >= avail {
        // no room (or no minimum at all): nothing is left to scale
        let scale = if floor_sum > 0.0 { avail.max(0.0) / floor_sum } else { 0.0 };
        return floor.iter().map(|f| f * scale).collect();
    }
    let extra: Vec<f32> = natural.iter().zip(&floor).map(|(n, f)| n - f).collect();
    let share = (avail - floor_sum) / extra.iter().sum::<f32>();
    floor.iter().zip(&extra).map(|(f, e)| f + e * share).collect()
}

/// Space between a cell's border and its text.
const PAD_X: f32 = 8.0;
const PAD_Y: f32 = 4.0;

/// The links of a cell: the range in the shown text, the link and whether its note exists.
pub type Spans = Vec<(Range<usize>, LinkSpan, bool)>;

/// One cell, laid out: its text and where the links in it are (byte ranges of the shown text).
pub struct LaidCell {
    pub galley: Arc<Galley>,
    pub spans: Spans,
    /// the formulas in it (EDIT-048)
    pub maths: Vec<crate::reader::MathAt>,
}

/// A table laid out for a given width: row 0 is the header.
pub struct Laid {
    pub aligns: Vec<Col>,
    /// width of each column including its padding
    pub widths: Vec<f32>,
    /// height of each row (header first) including its padding
    pub heights: Vec<f32>,
    pub cells: Vec<Vec<LaidCell>>,
}

impl Laid {
    pub fn size(&self) -> Vec2 {
        Vec2::new(self.widths.iter().sum(), self.heights.iter().sum())
    }

    /// Where the galley of the cell is painted: the anchor that goes with the cell's alignment.
    fn anchor(&self, origin: Pos2, row: usize, col: usize) -> Pos2 {
        let x = origin.x + self.widths[..col].iter().sum::<f32>();
        let y = origin.y + self.heights[..row].iter().sum::<f32>() + PAD_Y;
        let w = self.widths[col];
        let x = match self.aligns[col] {
            Col::Left => x + PAD_X,
            Col::Center => x + w / 2.0,
            Col::Right => x + w - PAD_X,
        };
        Pos2::new(x, y)
    }

    /// The rectangle of a cell of a table whose top left corner is `origin`.
    pub fn cell_rect(&self, origin: Pos2, row: usize, col: usize) -> Rect {
        let min = origin + Vec2::new(self.widths[..col].iter().sum(), self.heights[..row].iter().sum());
        Rect::from_min_size(min, Vec2::new(self.widths[col], self.heights[row]))
    }

    /// The cell under `p` for a table whose top left corner is `origin`: (row, column, the
    /// cell's galley anchor).
    pub fn cell_at(&self, origin: Pos2, p: Pos2) -> Option<(usize, usize, Pos2)> {
        let (dx, dy) = (p.x - origin.x, p.y - origin.y);
        if dx < 0.0 || dy < 0.0 {
            return None;
        }
        let col = (0..self.widths.len()).find(|c| dx < self.widths[..=*c].iter().sum::<f32>())?;
        let row = (0..self.heights.len()).find(|r| dy < self.heights[..=*r].iter().sum::<f32>())?;
        Some((row, col, self.anchor(origin, row, col)))
    }
}

fn halign(c: Col) -> Align {
    match c {
        Col::Left => Align::LEFT,
        Col::Center => Align::Center,
        Col::Right => Align::RIGHT,
    }
}

/// One cell as a layout job (the inline formatting of the reading view); the header is bold.
fn cell_job(text: &str, links: &[(LinkSpan, bool)], base: f32, header: bool) -> (LayoutJob, Spans, Vec<crate::reader::MathAt>) {
    let (mut job, spans, _, mut maths) = crate::reader::cell_painted(text, links, base, theme::palette().text);
    if header {
        for s in &mut job.sections {
            if s.format.color == theme::palette().text {
                s.format.color = theme::palette().text_strong;
                s.format.extra_letter_spacing = 0.3;
            }
        }
        for m in maths.iter_mut().filter(|m| m.color == theme::palette().text) {
            m.color = theme::palette().text_strong;
        }
    }
    (job, spans, maths)
}

/// The words of a job (split at blanks), each as a job of its own with the formats it had: the
/// widest of them is the least width a column can have.
fn words(job: &LayoutJob) -> Vec<LayoutJob> {
    let text = &job.text;
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        match (start, c.is_whitespace()) {
            (None, false) => start = Some(i),
            (Some(s), true) => {
                let mut w = LayoutJob { text: text[s..i].to_string(), ..Default::default() };
                for sec in &job.sections {
                    let (a, b) = (sec.byte_range.start.0.max(s), sec.byte_range.end.0.min(i));
                    if a < b {
                        let mut sec = sec.clone();
                        sec.byte_range = egui::text::ByteIndex(a - s)..egui::text::ByteIndex(b - s);
                        w.sections.push(sec);
                    }
                }
                out.push(w);
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The widths (with padding) of a table's columns from the sizes stored with it (EDIT-031), used
/// by the grid, the reading view and the PDF export: each at least `MIN_COL`, scaled down
/// together when they do not fit `avail`, else as stored. `None` when the sizes do not match the
/// `ncols` columns or are no positive numbers: the widths are then worked out.
pub fn stored_widths(cols: &[f32], ncols: usize, avail: f32) -> Option<Vec<f32>> {
    if ncols == 0 || cols.len() != ncols || cols.iter().any(|c| !c.is_finite() || *c <= 0.0) {
        return None;
    }
    let mut w: Vec<f32> = cols.iter().map(|c| c.max(MIN_COL)).collect();
    let (sum, room) = (w.iter().sum::<f32>(), avail - 1.0);
    if sum > room {
        w.iter_mut().for_each(|x| *x *= room.max(0.0) / sum);
    }
    Some(w)
}

/// Lay a table out for `avail` points of width. `exists` says whether a link target is a note.
/// Sizes that match the table's columns (rows) are used (EDIT-031): the widths scaled down
/// together when they do not fit, every column at least `MIN_COL`, a row never shorter than its
/// text.
pub fn layout(ctx: &egui::Context, table: &Table, sizes: &Sizes, base: f32, avail: f32, exists: &dyn Fn(&LinkSpan) -> bool) -> Laid {
    let ncols = table.header.len();
    let mut jobs: Vec<Vec<(LayoutJob, Spans, Vec<crate::reader::MathAt>)>> = Vec::new();
    for (r, row) in std::iter::once(&table.header).chain(&table.rows).enumerate() {
        jobs.push(
            row.iter()
                .map(|cell| {
                    let links: Vec<(LinkSpan, bool)> = markdown::find_links(cell).into_iter().map(|l| (l.clone(), exists(&l))).collect();
                    cell_job(cell, &links, base, r == 0)
                })
                .collect(),
        );
    }
    let measure = |job: &LayoutJob| ctx.fonts_mut(|f| f.layout_job(job.clone())).size().x;
    let mut natural = vec![0.0f32; ncols];
    let mut min = vec![0.0f32; ncols];
    for row in &jobs {
        for (c, (job, ..)) in row.iter().enumerate() {
            natural[c] = natural[c].max(measure(job));
            min[c] = min[c].max(words(job).iter().map(measure).fold(0.0, f32::max));
        }
    }
    let text_avail = avail - 2.0 * PAD_X * ncols as f32 - 1.0;
    let text_w = match stored_widths(&sizes.cols, ncols, avail) {
        Some(w) => w.iter().map(|x| (x - 2.0 * PAD_X).max(1.0)).collect(),
        None => column_widths(&natural, &min, text_avail),
    };
    let rows_fit = sizes.rows.len() == jobs.len();
    let mut cells = Vec::new();
    let mut heights = Vec::new();
    for (r, row) in jobs.into_iter().enumerate() {
        let mut laid_row = Vec::new();
        let mut h = base * 1.5;
        for (c, (mut job, spans, maths)) in row.into_iter().enumerate() {
            job.wrap.max_width = text_w[c].max(1.0);
            job.halign = halign(table.aligns[c]);
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            h = h.max(galley.size().y);
            laid_row.push(LaidCell { galley, spans, maths });
        }
        let least = if rows_fit { sizes.rows[r] } else { 0.0 };
        heights.push((h + 2.0 * PAD_Y).max(least));
        cells.push(laid_row);
    }
    Laid { aligns: table.aligns.clone(), widths: text_w.iter().map(|w| w + 2.0 * PAD_X).collect(), heights, cells }
}

/// Paint a laid-out table with its top left corner at `origin`: a filled header, lines between
/// all cells, the text clipped to its cell.
pub fn paint(painter: &egui::Painter, origin: Pos2, laid: &Laid) {
    let size = laid.size();
    let line = Stroke::new(1.0, theme::palette().faint.gamma_multiply(0.6));
    let head = Rect::from_min_size(origin, Vec2::new(size.x, laid.heights[0]));
    painter.rect_filled(head, 0.0, theme::palette().bg_active.gamma_multiply(0.6));
    let mut y = origin.y;
    for (r, row) in laid.cells.iter().enumerate() {
        let mut x = origin.x;
        for (c, cell) in row.iter().enumerate() {
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(laid.widths[c], laid.heights[r]));
            let cell_painter = painter.with_clip_rect(rect);
            let anchor = laid.anchor(origin, r, c);
            cell_painter.galley(anchor, cell.galley.clone(), Color32::PLACEHOLDER);
            // the formulas and the pictures of the cell, in the room their hidden text has (EDIT-048, EDIT-114)
            for m in &cell.maths {
                if let Ok(f) = &m.formula {
                    let at = markdown::byte_to_char(&cell.galley.job.text, m.range.start.min(cell.galley.job.text.len()));
                    crate::math::paint::paint_in_galley(&cell_painter, &cell.galley, anchor, at, f, m.color);
                }
            }
            crate::reader::paint_pictures(&cell_painter, &cell.galley, anchor, &cell.spans);
            x += laid.widths[c];
            if c + 1 < row.len() {
                painter.line_segment([Pos2::new(x, origin.y), Pos2::new(x, origin.y + size.y)], line);
            }
        }
        y += laid.heights[r];
        let stroke = if r == 0 { Stroke::new(1.5, line.color) } else { line };
        painter.line_segment([Pos2::new(origin.x, y), Pos2::new(origin.x + size.x, y)], stroke);
    }
    painter.line_segment([origin, Pos2::new(origin.x + size.x, origin.y)], line);
    painter.line_segment([origin, Pos2::new(origin.x, origin.y + size.y)], line);
    painter.line_segment([Pos2::new(origin.x + size.x, origin.y), origin + size], line);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cells(line: &str) -> Vec<String> {
        split_cells(line)
    }

    // spec: EDIT-024
    #[test]
    fn column_widths_with_no_room_and_no_minimum_are_zero_not_nan() {
        // found by a property test (#535): 0 / 0
        assert_eq!(column_widths(&[0.0], &[0.0], -24.0), [0.0]);
        assert_eq!(column_widths(&[50.0, 20.0], &[0.0, 0.0], 0.0), [0.0, 0.0]);
    }

    // spec: EDIT-024
    // spec: EDIT-031
    #[test]
    fn stored_widths_fit_scale_down_clamp_and_are_checked() {
        assert_eq!(stored_widths(&[100.0, 200.0], 2, 500.0).unwrap(), [100.0, 200.0], "as stored when they fit");
        let w = stored_widths(&[1.0, 100.0], 2, 500.0).unwrap();
        assert_eq!(w, [MIN_COL, 100.0], "at least MIN_COL");
        let w = stored_widths(&[300.0, 100.0], 2, 201.0).unwrap();
        assert_eq!(w, [150.0, 50.0], "scaled down together to avail - 1");
        assert!(stored_widths(&[], 0, 400.0).is_none() && stored_widths(&[1.0], 2, 400.0).is_none());
        assert!(stored_widths(&[1.0, f32::NAN], 2, 400.0).is_none() && stored_widths(&[1.0, -2.0], 2, 400.0).is_none());
    }

    #[test]
    fn cells_split_on_pipes_with_or_without_the_outer_ones() {
        assert_eq!(cells("| a | b | c |"), ["a", "b", "c"]);
        assert_eq!(cells("a | b | c"), ["a", "b", "c"]);
        assert_eq!(cells("|a|b|"), ["a", "b"]);
        assert_eq!(cells("| a | | c |"), ["a", "", "c"], "an empty cell stays");
        assert_eq!(cells("| a | b"), ["a", "b"], "no closing pipe");
        assert_eq!(cells("| a | b |  "), ["a", "b"], "trailing blanks after the last pipe");
        assert!(cells("   ").is_empty() && cells("").is_empty(), "a blank line has no cells");
    }

    // spec: EDIT-024
    #[test]
    fn an_escaped_pipe_and_a_pipe_in_a_code_span_stay_in_their_cell() {
        assert_eq!(cells(r"| a \| b | c |"), ["a | b", "c"]);
        assert_eq!(cells("| `a | b` | c |"), ["`a | b`", "c"]);
        assert_eq!(cells(r"| [[Note\|alias]] | c |"), ["[[Note|alias]]", "c"], "a wikilink alias");
        assert_eq!(cells("| a ` b | c |"), ["a ` b", "c"], "a lone backtick is no code span");
        assert_eq!(cells("| ünï | 日本 |"), ["ünï", "日本"]);
    }

    // spec: EDIT-024
    #[test]
    fn the_delimiter_row_gives_the_alignment() {
        use Col::*;
        assert_eq!(delimiter_row("|---|:---|:---:|---:|"), Some(vec![Left, Left, Center, Right]));
        assert_eq!(delimiter_row("--- | ---"), Some(vec![Left, Left]));
        assert_eq!(delimiter_row("| - | :-: |"), Some(vec![Left, Center]));
        assert_eq!(delimiter_row("---"), None, "a rule is no delimiter row");
        assert_eq!(delimiter_row("| a | b |"), None);
        assert_eq!(delimiter_row("|---|:|"), None);
        assert_eq!(delimiter_row("| --- | -x- |"), None);
    }

    // spec: EDIT-024
    #[test]
    fn tables_are_found_with_their_lines_and_rows_are_made_even() {
        let text = "intro\n\n| A | B |\n|---|--:|\n| 1 | 2 |\n| only |\n| x | y | extra |\n\nafter | not a table\n";
        let blocks = find_tables(text);
        assert_eq!(blocks.len(), 1);
        let b = &blocks[0];
        assert_eq!(b.lines, 2..7, "header, delimiter and three rows");
        assert_eq!(b.table.header, ["A", "B"]);
        assert_eq!(b.table.aligns, [Col::Left, Col::Right]);
        assert_eq!(b.table.rows, [["1", "2"], ["only", ""], ["x", "y"]], "missing cells filled, extra dropped");
        assert_eq!(&text[byte_range(text, &b.lines)], "| A | B |\n|---|--:|\n| 1 | 2 |\n| only |\n| x | y | extra |\n");
    }

    // spec: EDIT-024
    #[test]
    fn code_blocks_front_matter_and_look_alikes_are_no_tables() {
        assert!(find_tables("```\n| a | b |\n|---|---|\n```").is_empty(), "fenced code");
        assert!(find_tables("---\n| a | b |\n|---|---|\n---\n").is_empty(), "front matter");
        assert!(find_tables("| a | b |\n|---|").is_empty(), "the delimiter must have as many cells");
        assert!(find_tables("| a | b |\n| c | d |").is_empty(), "no delimiter row");
        assert!(find_tables("a | b").is_empty());
        let two = find_tables("| a |\n|---|\n| 1 |\n\n| b |\n|---|\n");
        assert_eq!(two.iter().map(|b| b.lines.clone()).collect::<Vec<_>>(), [0..3, 4..6], "a blank line ends a table; a header alone is a table");
        assert_eq!(find_tables("| a |\r\n|---|\r\n| 1 |\r\n")[0].table.rows, [["1"]], "Windows line ends");
    }

    // spec: EDIT-024
    #[test]
    fn column_widths_use_the_text_width_when_it_fits_else_share_the_rest() {
        assert_eq!(column_widths(&[100.0, 50.0], &[20.0, 20.0], 400.0), [100.0, 50.0], "fits: natural widths, not stretched");
        // too wide: the minimums stay, the rest is shared by how much each wants
        let w = column_widths(&[300.0, 100.0], &[40.0, 40.0], 200.0);
        assert!((w.iter().sum::<f32>() - 200.0).abs() < 0.01, "{w:?}");
        assert!(w[0] > w[1] && w[0] >= 40.0 && w[1] >= 40.0, "{w:?}");
        // a column never gets more than it wants, nor less than its longest word
        let w = column_widths(&[30.0, 500.0], &[80.0, 60.0], 200.0);
        assert_eq!(w[0], 30.0, "{w:?}");
        assert!((w[1] - 170.0).abs() < 0.01, "{w:?}");
        // not even the longest words fit: scaled down together
        let w = column_widths(&[200.0, 200.0], &[150.0, 50.0], 100.0);
        assert_eq!(w, [75.0, 25.0]);
        assert_eq!(column_widths(&[200.0], &[50.0], -5.0), [0.0], "no room at all");
    }

    fn laid(width: f32, aligns: Vec<Col>, header: &[&str], rows: &[&[&str]]) -> Laid {
        let ctx = egui::Context::default();
        let t = Table { aligns, header: header.iter().map(|s| s.to_string()).collect(), rows: rows.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect() };
        let mut out = None;
        let mut full = ctx.run_ui(egui::RawInput::default(), |ui| out = Some(layout(ui.ctx(), &t, &Sizes::default(), 15.0, width, &|_| true)));
        full.textures_delta.clear();
        out.unwrap()
    }

    // spec: EDIT-024
    #[test]
    fn long_cell_text_wraps_in_its_own_column() {
        let long = "word ".repeat(40);
        let narrow = laid(400.0, vec![Col::Left, Col::Left], &["A", "B"], &[&["short", &long]]);
        let wide = laid(4000.0, vec![Col::Left, Col::Left], &["A", "B"], &[&["short", &long]]);
        assert!(narrow.size().x <= 400.5, "fits the width: {}", narrow.size().x);
        assert!(narrow.heights[1] > wide.heights[1] * 3.0, "the long cell wraps over several rows: {:?} / {:?}", narrow.heights, wide.heights);
        assert!(narrow.widths[1] > narrow.widths[0] * 3.0, "the long column gets the room: {:?}", narrow.widths);
        assert_eq!(narrow.cells[1][0].galley.rows.len(), 1, "the short cell stays on one row {:?}", narrow.widths);
        assert!(wide.size().x > 400.0, "unconstrained, the table is as wide as its text");
    }

    // spec: EDIT-024
    #[test]
    fn a_cell_is_formatted_like_the_reading_view_and_keeps_its_links() {
        let l = laid(600.0, vec![Col::Left], &["Head"], &[&["`code` and **bold** and [[Target]]"]]);
        let cell = &l.cells[1][0];
        assert_eq!(cell.galley.job.text, "code and bold and Target");
        assert_eq!(cell.spans.len(), 1);
        assert_eq!(cell.spans[0].1.target, "Target");
        let head = &l.cells[0][0].galley.job.sections[0].format;
        assert_eq!(head.color, theme::palette().text_strong, "the header is strong");
        assert_eq!(cell.galley.job.sections[0].format.color, theme::palette().code);
    }

    // spec: EDIT-045
    #[test]
    fn br_in_a_cell_starts_a_new_line_in_the_grid() {
        let l = laid(600.0, vec![Col::Left, Col::Left], &["A", "B"], &[&["\u{2022} a<br>\u{2022} b<br/>c<BR />d", "x"]]);
        let cell = &l.cells[1][0];
        assert_eq!(cell.galley.job.text, "\u{2022} a\n\u{2022} b\nc\nd");
        assert_eq!(cell.galley.rows.len(), 4, "one row for each line");
        assert!(l.heights[1] > l.heights[0] * 2.5, "the row is as tall as its lines: {:?}", l.heights);
        assert_eq!(l.cells[1][1].galley.rows.len(), 1);
        // the width of the column is that of its longest line, not of all of them
        let one = laid(600.0, vec![Col::Left], &["A"], &[&["\u{2022} a \u{2022} b c d"]]);
        assert!(l.widths[0] < one.widths[0], "{:?} / {:?}", l.widths, one.widths);
        // a cell without the tag stays on one line; the tag in a code span is text
        let code = laid(600.0, vec![Col::Left], &["A"], &[&["`a<br>b`"]]);
        assert_eq!(code.cells[1][0].galley.job.text, "a<br>b");
    }

    // spec: EDIT-045
    #[test]
    fn the_cell_editor_keeps_the_br_tag_in_the_text_of_the_cell() {
        let src = "| A | B |\n|---|---|\n| \u{2022} a<br>\u{2022} b | x |\n";
        let t = find_tables(src).remove(0).table;
        assert_eq!(t.rows[0][0], "\u{2022} a<br>\u{2022} b", "the cell text has the tag, the editor gets it as it is");
        let edited = set_cell(src, 1, 0, "\u{2022} a<br />\u{2022} b<br>c").unwrap();
        assert_eq!(find_tables(&edited).remove(0).table.rows[0][0], "\u{2022} a<br />\u{2022} b<br>c");
        assert!(edited.contains("| \u{2022} a<br />\u{2022} b<br>c |"), "{edited}");
    }

    // spec: EDIT-024
    #[test]
    fn alignment_places_the_text_in_the_cell() {
        let l = laid(600.0, vec![Col::Left, Col::Center, Col::Right], &["a", "b", "c"], &[&["x", "y", "z"]]);
        let o = Pos2::ZERO;
        let x = |c: usize| l.cell_at(o, Pos2::new(l.widths[..c].iter().sum::<f32>() + 1.0, 1.0)).unwrap().2.x;
        assert!((x(0) - PAD_X).abs() < 0.01);
        assert!((x(1) - (l.widths[0] + l.widths[1] / 2.0)).abs() < 0.01, "centre of the column");
        assert!((x(2) - (l.size().x - PAD_X)).abs() < 0.01, "right edge of the column");
        assert!(l.cell_at(o, Pos2::new(-1.0, 0.0)).is_none() && l.cell_at(o, Pos2::new(1.0, l.size().y + 1.0)).is_none());
        let (r, c, _) = l.cell_at(o, Pos2::new(l.size().x - 1.0, l.heights[0] + 1.0)).unwrap();
        assert_eq!((r, c), (1, 2));
    }

    // spec: EDIT-029
    #[test]
    fn a_cell_shows_colours_without_their_tags() {
        let l = laid(600.0, vec![Col::Left], &["Head"], &[&["<mark style=\"background: #39FF14;\">neon</mark> and <span style=\"color: #FF0000;\">**red**</span>"]]);
        let job = &l.cells[1][0].galley.job;
        assert_eq!(job.text, "neon and red");
        assert_eq!(job.sections[0].format.background, Color32::from_rgb(0x39, 0xff, 0x14));
        let red = job.sections.iter().find(|s| job.text[s.byte_range.start.0..].starts_with("red")).unwrap();
        assert_eq!(red.format.color, Color32::from_rgb(255, 0, 0));
    }

    const SRC: &str = "| A | B |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |\n";

    // spec: EDIT-025
    #[test]
    fn a_column_is_inserted_after_a_column_with_an_empty_header_and_a_delimiter_cell() {
        assert_eq!(edit_source(SRC, Edit::InsertColumn(0)).unwrap(), "| A |  | B |\n|:--| --- |--:|\n| 1 |  | 2 |\n| 3 |  | 4 |\n");
        let last = edit_source(SRC, Edit::InsertColumn(1)).unwrap();
        assert_eq!(last, "| A | B |  |\n|:--|--:| --- |\n| 1 | 2 |  |\n| 3 | 4 |  |\n");
        let blocks = find_tables(&last);
        assert_eq!(blocks[0].table.header, ["A", "B", ""]);
        assert_eq!(blocks[0].table.aligns, [Col::Left, Col::Right, Col::Left]);
        assert_eq!(edit_source(SRC, Edit::InsertColumn(2)), None, "no such column");
    }

    // spec: EDIT-025
    #[test]
    fn deleting_a_column_leaves_the_other_cells_as_written() {
        assert_eq!(edit_source(SRC, Edit::DeleteColumn(0)).unwrap(), "| B |\n|--:|\n| 2 |\n| 4 |\n");
        assert_eq!(edit_source(SRC, Edit::DeleteColumn(1)).unwrap(), "| A |\n|:--|\n| 1 |\n| 3 |\n");
        let three = "|  a  |b|  c  |\n|---|:-:|---:|\n| x | y | z |\n";
        assert_eq!(edit_source(three, Edit::DeleteColumn(1)).unwrap(), "|  a  |  c  |\n|---|---:|\n| x | z |\n", "spacing and alignment of the rest untouched");
        assert_eq!(edit_source("| A |\n|---|\n| 1 |", Edit::DeleteColumn(0)), None, "the last column stays");
        assert_eq!(edit_source(SRC, Edit::DeleteColumn(5)), None);
    }

    // spec: EDIT-025
    #[test]
    fn rows_are_inserted_and_deleted_but_the_header_and_the_last_row_stay() {
        assert_eq!(edit_source(SRC, Edit::InsertRow(None)).unwrap(), "| A | B |\n|:--|--:|\n|   |   |\n| 1 | 2 |\n| 3 | 4 |\n");
        assert_eq!(edit_source(SRC, Edit::InsertRow(Some(0))).unwrap(), "| A | B |\n|:--|--:|\n| 1 | 2 |\n|   |   |\n| 3 | 4 |\n");
        let end = edit_source(SRC, Edit::InsertRow(Some(1))).unwrap();
        assert_eq!(end, "| A | B |\n|:--|--:|\n| 1 | 2 |\n| 3 | 4 |\n|   |   |\n");
        assert_eq!(find_tables(&end)[0].table.rows.len(), 3, "the empty row is a row of the table");
        assert_eq!(edit_source(SRC, Edit::DeleteRow(0)).unwrap(), "| A | B |\n|:--|--:|\n| 3 | 4 |\n");
        assert_eq!(edit_source("| A |\n|---|\n| 1 |\n", Edit::DeleteRow(0)), None, "the last row stays");
        assert_eq!(edit_source(SRC, Edit::DeleteRow(2)), None);
        assert_eq!(edit_source(SRC, Edit::InsertRow(Some(2))), None);
    }

    // spec: EDIT-025
    #[test]
    fn missing_outer_pipes_uneven_rows_escapes_and_line_ends_are_handled() {
        // no outer pipes, no final line break
        let bare = "A | B\n--|--\n1 | 2";
        assert_eq!(edit_source(bare, Edit::InsertColumn(1)).unwrap(), "A | B |  |\n--|-- | --- |\n1 | 2 |  |");
        assert_eq!(edit_source(bare, Edit::DeleteColumn(1)).unwrap(), "A \n--\n1 ");
        assert_eq!(edit_source(bare, Edit::DeleteColumn(0)).unwrap(), " B\n--\n 2");
        // a short row is filled up to the new column; an extra cell is kept as written
        let uneven = "| A | B | C |\n|---|---|---|\n| 1 |\n| 1 | 2 | 3 | 4 |\n";
        let ins = edit_source(uneven, Edit::InsertColumn(2)).unwrap();
        let t = &find_tables(&ins)[0].table;
        assert_eq!(t.header, ["A", "B", "C", ""]);
        assert_eq!(t.rows, [["1", "", "", ""], ["1", "2", "3", ""]]);
        assert_eq!(edit_source(uneven, Edit::DeleteColumn(1)).unwrap(), "| A | C |\n|---|---|\n| 1 |\n| 1 | 3 | 4 |\n");
        // an escaped pipe and a pipe in a code span stay inside their cell
        let esc = "| a \\| b | `x | y` | c |\n|---|---|---|\n| [[N\\|n]] | 2 | 3 |\n";
        assert_eq!(edit_source(esc, Edit::DeleteColumn(0)).unwrap(), "| `x | y` | c |\n|---|---|\n| 2 | 3 |\n");
        assert_eq!(edit_source(esc, Edit::DeleteColumn(1)).unwrap(), "| a \\| b | c |\n|---|---|\n| [[N\\|n]] | 3 |\n");
        // Windows line ends and an indented table
        let crlf = "  | A |\r\n  |---|\r\n  | 1 |\r\n";
        assert_eq!(edit_source(crlf, Edit::InsertRow(Some(0))).unwrap(), "  | A |\r\n  |---|\r\n  | 1 |\r\n  |   |\r\n");
        assert_eq!(edit_source("| A |\n|---|", Edit::InsertColumn(0)), Some("| A |  |\n|---| --- |".to_string()));
    }

    // spec: EDIT-025
    #[test]
    fn edits_of_short_rows_header_only_tables_and_non_tables() {
        assert_eq!(edit_source("| A |", Edit::InsertRow(None)), None, "a single line is no table");
        assert_eq!(edit_source("", Edit::InsertColumn(0)), None);
        assert_eq!(edit_source("| A |\n|---|", Edit::InsertRow(None)).unwrap(), "| A |\n|---|\n|   |", "the first row of a header-only table");
        assert_eq!(edit_source("| A |\n|---|", Edit::DeleteRow(0)), None, "there is no row to delete");
        // a short row without outer pipes is closed and filled up to the new column
        let short = "A | B | C\n--|--|--\n1\n";
        let t = &find_tables(&edit_source(short, Edit::InsertColumn(2)).unwrap())[0].table;
        assert_eq!(t.header, ["A", "B", "C", ""]);
        assert_eq!(t.rows, [["1", "", "", ""]]);
        // and a column behind its end is just not there
        assert_eq!(edit_source(short, Edit::DeleteColumn(2)).unwrap(), "A | B \n--|--\n1\n");
        assert_eq!(edit_source("| A | B |\n|---|---|\n| 1 |\n", Edit::DeleteColumn(1)).unwrap(), "| A |\n|---|\n| 1 |\n", "a short row without that cell stays");
    }

    // spec: EDIT-030
    #[test]
    fn the_comment_line_above_a_table_is_read_written_and_belongs_to_it() {
        let m = parse_meta("<!-- shale-table edit=raw cols=120,80 rows=30,0,44 future=1 -->").unwrap();
        assert_eq!(m, Meta { cells: Some(false), sizes: Sizes { cols: vec![120.0, 80.0], rows: vec![30.0, 0.0, 44.0] } });
        assert_eq!(meta_line(&m).unwrap(), "<!-- shale-table edit=raw cols=120,80 rows=30,0,44 -->");
        assert_eq!(parse_meta("<!--shale-table edit=cells-->").unwrap().cells, Some(true));
        assert_eq!(parse_meta("<!-- shale-table cols=1,x -->").unwrap().sizes.cols, Vec::<f32>::new(), "broken numbers are ignored");
        assert_eq!(parse_meta("<!-- shale-tables -->"), None);
        assert_eq!(parse_meta("<!-- a comment -->"), None);
        assert_eq!(meta_line(&Meta::default()), None, "nothing to keep: no line");
        let text = "x\n<!-- shale-table edit=cells -->\n| A |\n|---|\n| 1 |\n\n<!-- shale-table edit=raw -->\n\n| B |\n|---|\n";
        let t = find_tables(text);
        assert_eq!((t[0].lines.clone(), t[0].table_lines(), t[0].has_meta, t[0].meta.cells), (1..5, 2..5, true, Some(true)));
        assert_eq!((t[1].lines.clone(), t[1].has_meta), (8..10, false), "a comment with a blank line under it is not the table's");
        // in fenced code it is not the table's either
        let t = find_tables("```\n<!-- shale-table edit=raw -->\n```\n| A |\n|---|\n");
        assert_eq!((t[0].lines.clone(), t[0].has_meta), (3..5, false));
    }

    // spec: EDIT-030
    #[test]
    fn a_cell_is_written_with_its_pipes_escaped_and_the_rest_as_it_was() {
        let src = "| A |  B |\n|:--|--:|\n|1|2|\n| 3 |\n";
        assert_eq!(set_cell(src, 0, 1, "Be").unwrap(), "| A | Be |\n|:--|--:|\n|1|2|\n| 3 |\n");
        assert_eq!(set_cell(src, 1, 0, "a|b `c|d` e\nf").unwrap(), "| A |  B |\n|:--|--:|\n| a\\|b `c|d` e f |2|\n| 3 |\n");
        assert_eq!(set_cell(src, 2, 1, "4").unwrap(), "| A |  B |\n|:--|--:|\n|1|2|\n| 3 | 4 |\n", "a short row is filled up");
        assert_eq!(set_cell(src, 3, 0, "x"), None, "no such row");
        assert_eq!(set_cell(src, 0, 2, "x"), None, "no such column");
        assert_eq!(set_cell("A | B\r\n--|--\r\n1 | 2\r\n", 1, 1, ""), Some("A | B\r\n--|--\r\n1 |  |\r\n".to_string()), "no outer pipes, Windows line ends, an empty last cell keeps a closing pipe");
        // the empty first cell of a row without outer pipes keeps an opening pipe (found by a property test, #535)
        let emptied = set_cell("日|a\n|---|", 0, 0, "").unwrap();
        assert_eq!(emptied, "|  |a\n|---|");
        assert_eq!(split_cells(emptied.lines().next().unwrap()), ["", "a"]);
        let back = find_tables(&set_cell(src, 1, 0, "a|b `c|d`").unwrap())[0].table.rows[0][0].clone();
        assert_eq!(back, "a|b `c|d`", "what was typed is what the cell shows");
    }

    // spec: EDIT-030, EDIT-031
    #[test]
    fn changes_rewrite_the_comment_line_and_keep_the_sizes_with_their_columns() {
        let table = "| A | B |\n|---|---|\n| 1 | 2 |\n";
        let on = Meta { cells: Some(false), sizes: Sizes::default() };
        let with = change_block(table, false, &Meta::default(), &Change::Meta(on.clone())).unwrap();
        assert_eq!(with, format!("<!-- shale-table edit=raw -->\n{table}"));
        // and away again when nothing is left in it
        assert_eq!(change_block(&with, true, &on, &Change::Meta(Meta::default())).unwrap(), table);
        let sized = Meta { cells: None, sizes: Sizes { cols: vec![100.0, 60.0], rows: vec![30.0, 40.0] } };
        let src = format!("<!-- shale-table cols=100,60 rows=30,40 -->\n{table}");
        let out = change_block(&src, true, &sized, &Change::Edit(Edit::InsertColumn(0))).unwrap();
        assert!(out.starts_with("<!-- shale-table cols=100,100,60 rows=30,40 -->\n| A |  | B |"), "{out}");
        let out = change_block(&src, true, &sized, &Change::Edit(Edit::DeleteColumn(0))).unwrap();
        assert!(out.starts_with("<!-- shale-table cols=60 rows=30,40 -->\n"), "{out}");
        let out = change_block(&src, true, &sized, &Change::Edit(Edit::InsertRow(Some(0)))).unwrap();
        assert!(out.starts_with("<!-- shale-table cols=100,60 rows=30,40,0 -->\n"), "a new row is as tall as its text: {out}");
        let out = change_block(&src, true, &sized, &Change::Cell { row: 1, col: 1, text: "two".into() }).unwrap();
        assert_eq!(out, "<!-- shale-table cols=100,60 rows=30,40 -->\n| A | B |\n|---|---|\n| 1 | two |\n");
        assert_eq!(change_block(&src, true, &sized, &Change::Cell { row: 5, col: 0, text: "x".into() }), None);
        // a Windows note gets a Windows line end
        let crlf = "| A |\r\n|---|\r\n";
        assert_eq!(change_block(crlf, false, &Meta::default(), &Change::Meta(on)).unwrap(), format!("<!-- shale-table edit=raw -->\r\n{crlf}"));
    }

    // spec: EDIT-031
    #[test]
    fn stored_sizes_set_the_columns_and_the_least_row_heights() {
        let ctx = egui::Context::default();
        let t = Table { aligns: vec![Col::Left; 2], header: vec!["A".into(), "B".into()], rows: vec![vec!["1".into(), "2".into()]] };
        let run = |sizes: Sizes, width: f32| {
            let mut out = None;
            let mut full = ctx.run_ui(egui::RawInput::default(), |ui| out = Some(layout(ui.ctx(), &t, &sizes, 15.0, width, &|_| true)));
            full.textures_delta.clear();
            out.unwrap()
        };
        let auto = run(Sizes::default(), 800.0);
        let l = run(Sizes { cols: vec![200.0, 10.0], rows: vec![0.0, 90.0] }, 800.0);
        assert_eq!(l.widths, [200.0, MIN_COL], "a column is at least MIN_COL wide");
        assert_eq!(l.heights[0], auto.heights[0], "a row is never shorter than its text");
        assert_eq!(l.heights[1], 90.0);
        let narrow = run(Sizes { cols: vec![600.0, 200.0], rows: vec![] }, 401.0);
        assert!((narrow.widths[0] - 300.0).abs() < 0.01 && (narrow.widths[1] - 100.0).abs() < 0.01, "too wide: scaled down together {:?}", narrow.widths);
        assert_eq!(run(Sizes { cols: vec![100.0], rows: vec![1.0] }, 800.0).widths, auto.widths, "sizes that do not match are ignored");
    }
}
