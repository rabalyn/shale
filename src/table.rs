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

/// A table in a note: the lines it occupies (numbers from 0, end excluded) and its content.
#[derive(Clone, PartialEq, Debug)]
pub struct Block {
    pub lines: Range<usize>,
    pub table: Table,
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
    let mut out = Vec::new();
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
        out.push(Block { lines: i..j, table: Table { aligns, header: head, rows } });
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
        return floor.iter().map(|f| f * avail.max(0.0) / floor_sum).collect();
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
fn cell_job(text: &str, links: &[(LinkSpan, bool)], base: f32, header: bool) -> (LayoutJob, Spans) {
    let (mut job, spans) = crate::reader::inline_job(text, links, base, theme::TEXT, false);
    if header {
        for s in &mut job.sections {
            if s.format.color == theme::TEXT {
                s.format.color = theme::TEXT_STRONG;
                s.format.extra_letter_spacing = 0.3;
            }
        }
    }
    (job, spans)
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

/// Lay a table out for `avail` points of width. `exists` says whether a link target is a note.
pub fn layout(ctx: &egui::Context, table: &Table, base: f32, avail: f32, exists: &dyn Fn(&LinkSpan) -> bool) -> Laid {
    let ncols = table.header.len();
    let mut jobs: Vec<Vec<(LayoutJob, Spans)>> = Vec::new();
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
        for (c, (job, _)) in row.iter().enumerate() {
            natural[c] = natural[c].max(measure(job));
            min[c] = min[c].max(words(job).iter().map(measure).fold(0.0, f32::max));
        }
    }
    let text_avail = avail - 2.0 * PAD_X * ncols as f32 - 1.0;
    let text_w = column_widths(&natural, &min, text_avail);
    let mut cells = Vec::new();
    let mut heights = Vec::new();
    for row in jobs {
        let mut laid_row = Vec::new();
        let mut h = base * 1.5;
        for (c, (mut job, spans)) in row.into_iter().enumerate() {
            job.wrap.max_width = text_w[c].max(1.0);
            job.halign = halign(table.aligns[c]);
            let galley = ctx.fonts_mut(|f| f.layout_job(job));
            h = h.max(galley.size().y);
            laid_row.push(LaidCell { galley, spans });
        }
        heights.push(h + 2.0 * PAD_Y);
        cells.push(laid_row);
    }
    Laid { aligns: table.aligns.clone(), widths: text_w.iter().map(|w| w + 2.0 * PAD_X).collect(), heights, cells }
}

/// Paint a laid-out table with its top left corner at `origin`: a filled header, lines between
/// all cells, the text clipped to its cell.
pub fn paint(painter: &egui::Painter, origin: Pos2, laid: &Laid) {
    let size = laid.size();
    let line = Stroke::new(1.0, theme::FAINT.gamma_multiply(0.6));
    let head = Rect::from_min_size(origin, Vec2::new(size.x, laid.heights[0]));
    painter.rect_filled(head, 0.0, theme::BG_ACTIVE.gamma_multiply(0.6));
    let mut y = origin.y;
    for (r, row) in laid.cells.iter().enumerate() {
        let mut x = origin.x;
        for (c, cell) in row.iter().enumerate() {
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(laid.widths[c], laid.heights[r]));
            painter.with_clip_rect(rect).galley(laid.anchor(origin, r, c), cell.galley.clone(), Color32::PLACEHOLDER);
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
        let mut full = ctx.run_ui(egui::RawInput::default(), |ui| out = Some(layout(ui.ctx(), &t, 15.0, width, &|_| true)));
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
        assert_eq!(head.color, theme::TEXT_STRONG, "the header is strong");
        assert_eq!(cell.galley.job.sections[0].format.color, theme::CODE);
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
}
