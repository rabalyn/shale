//! Property tests of the table text (EDIT-024, EDIT-030, EDIT-031, DEV-042): a cell written with
//! `set_cell` reads back, the comment line round-trips, the cell ranges lie in the line, the column
//! widths keep their bounds and the finders never panic or point outside the text. The example
//! tests are in `table.rs`.

use proptest::prelude::*;

use crate::property_test_support::config;
use crate::table::{byte_range, cell_ranges, column_widths, find_tables, meta_line, parse_meta, set_cell, split_cells, stored_widths, Meta, Sizes, MIN_COL};

/// Cell text a person types: awkward characters (`|`, `\`, backticks, blanks at the ends,
/// non-ASCII) mixed with plain words.
fn cell_input() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        3 => Just("|".to_string()),
        3 => Just("\\".to_string()),
        3 => Just("`".to_string()),
        2 => Just("\\|".to_string()),
        2 => Just(" ".to_string()),
        1 => Just("\n".to_string()),
        1 => Just("\t".to_string()),
        2 => Just("é日🙂".to_string()),
        4 => "[a-zA-Z0-9 ]{0,4}",
    ];
    prop::collection::vec(piece, 0..8).prop_map(|p| p.concat())
}

/// A cell already in a table: plain text with escaped pipes, no backtick (a lone backtick would
/// open a code span that runs on into the next cell, which is not what these tests are about).
fn old_cell() -> impl Strategy<Value = String> {
    prop::collection::vec(prop_oneof![3 => "[a-z0-9é日]{1,3}", 1 => Just("\\|".to_string()), 1 => Just(" ".to_string())], 0..4).prop_map(|p| p.concat())
}

/// A table source: a header, a delimiter row and rows with the same or a different number of
/// cells, with or without outer pipes, with `\n` or `\r\n` line ends, with or without a last one.
fn table_source() -> impl Strategy<Value = String> {
    let row = (prop::collection::vec(old_cell(), 1..5), any::<bool>(), any::<bool>()).prop_map(|(cells, lead, trail)| {
        let mut s = String::new();
        if lead {
            s.push('|');
        }
        s.push_str(&cells.join("|"));
        if trail {
            s.push('|');
        }
        // a table line has a pipe
        if !s.contains('|') {
            s.insert(0, '|');
        }
        s
    });
    (prop::collection::vec(row, 1..5), 1usize..5, any::<bool>(), any::<bool>()).prop_map(|(mut rows, n, crlf, last)| {
        let header = rows.remove(0);
        let mut lines = vec![header, format!("|{}", "---|".repeat(n))];
        lines.extend(rows);
        let eol = if crlf { "\r\n" } else { "\n" };
        lines.join(eol) + if last { eol.trim_end_matches('\r') } else { "" }
    })
}

/// The lines of a table source as `set_cell` takes them apart.
fn lines_of(src: &str) -> Vec<String> {
    src.strip_suffix('\n').unwrap_or(src).split('\n').map(str::to_string).collect()
}

proptest! {
    #![proptest_config(config())]

    // spec: EDIT-030
    #[test]
    fn edit_030_a_cell_set_reads_back_and_the_other_cells_stay(src in table_source(), row in 0usize..6, col in 0usize..6, text in cell_input()) {
        let before = lines_of(&src);
        let Some(after_src) = set_cell(&src, row, col, &text) else { return Ok(()) };
        let after = lines_of(&after_src);
        prop_assert_eq!(after.len(), before.len(), "the lines stay");
        prop_assert_eq!(after_src.ends_with('\n'), src.ends_with('\n'), "the last line break stays");
        let at = if row == 0 { 0 } else { row + 1 };
        for (i, (a, b)) in after.iter().zip(&before).enumerate() {
            if i != at {
                prop_assert_eq!(a, b, "line {} is not the cell's", i);
            }
        }
        // the text as written: line breaks are blanks, the ends are trimmed
        let wanted = text.replace(['\n', '\r'], " ").trim().to_string();
        let mut cells = split_cells(&before[at]);
        cells.resize(cells.len().max(col + 1), String::new());
        cells[col] = wanted;
        prop_assert_eq!(split_cells(&after[at]), cells, "line {:?} -> {:?}", before[at], after[at]);
    }
}

/// A finite number a meta can keep: a whole, non-negative one (the line writes whole points).
fn point() -> impl Strategy<Value = f32> {
    (0u32..100_000).prop_map(|n| n as f32)
}

fn meta() -> impl Strategy<Value = Meta> {
    (prop::option::of(any::<bool>()), prop::collection::vec(point(), 0..6), prop::collection::vec(point(), 0..6)).prop_map(|(cells, cols, rows)| Meta { cells, sizes: Sizes { cols, rows } })
}

proptest! {
    #![proptest_config(config())]

    // spec: EDIT-030
    #[test]
    fn edit_030_a_meta_line_reads_back_as_the_meta(m in meta()) {
        match meta_line(&m) {
            Some(line) => {
                prop_assert!(!line.contains('\n'), "{:?}", line);
                prop_assert_eq!(parse_meta(&line), Some(m));
            }
            None => prop_assert_eq!(m, Meta::default(), "only nothing writes no line"),
        }
    }

    // spec: EDIT-030
    #[test]
    fn edit_030_a_meta_line_written_from_read_numbers_does_not_change_again(
        cells in prop::option::of(any::<bool>()),
        cols in prop::collection::vec(0.0f32..1.0e9, 0..6),
        rows in prop::collection::vec(0.0f32..1.0e9, 0..6),
    ) {
        let m = Meta { cells, sizes: Sizes { cols, rows } };
        if let Some(line) = meta_line(&m) {
            let read = parse_meta(&line).unwrap();
            prop_assert_eq!(meta_line(&read), Some(line), "rounded once, then stable");
        }
    }

    // spec: EDIT-030
    #[test]
    fn edit_030_reading_any_text_as_a_meta_never_panics(text in "(<!-- )?(shale-table)?[ a-z=,.0-9eE+-]{0,30}( -->)?|\\PC{0,20}") {
        let _ = parse_meta(&text);
    }
}

/// A line of a table, made of the characters that matter to it.
fn table_line() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        4 => Just("|".to_string()),
        2 => Just("\\".to_string()),
        2 => Just("\\|".to_string()),
        3 => Just("`".to_string()),
        2 => Just(" ".to_string()),
        1 => Just("\t".to_string()),
        1 => Just("\r".to_string()),
        1 => Just("é日🙂".to_string()),
        1 => Just("---".to_string()),
        1 => Just(":".to_string()),
        4 => "[a-z0-9]{0,3}",
    ];
    prop::collection::vec(piece, 0..14).prop_map(|p| p.concat())
}

proptest! {
    #![proptest_config(config())]

    // spec: EDIT-024
    #[test]
    fn edit_024_the_cell_ranges_of_a_line_lie_inside_it_in_order_without_overlap(line in table_line()) {
        let ranges = cell_ranges(&line);
        let mut at = 0;
        for r in &ranges {
            prop_assert!(r.start <= r.end && r.end <= line.len(), "{:?} outside {:?}", r, line);
            prop_assert!(line.is_char_boundary(r.start) && line.is_char_boundary(r.end), "{:?} cuts a character of {:?}", r, line);
            prop_assert!(r.start >= at, "{:?} overlaps or comes before {}", r, at);
            // the pipe between two cells is not in either
            at = r.end;
        }
        prop_assert_eq!(split_cells(&line).len(), ranges.len());
    }

    // spec: EDIT-024
    #[test]
    fn edit_024_the_ranges_between_two_cells_are_the_pipes_that_separate_them(line in table_line()) {
        let ranges = cell_ranges(&line);
        for w in ranges.windows(2) {
            prop_assert_eq!(&line[w[0].end..w[1].start], "|", "in {:?}", line);
        }
    }
}

/// Widths in points: zero, small, large.
fn widths(n: usize) -> impl Strategy<Value = Vec<f32>> {
    prop::collection::vec(prop_oneof![1 => Just(0.0f32), 6 => 0.0f32..400.0, 1 => 400.0f32..5000.0], n)
}

fn column_input() -> impl Strategy<Value = (Vec<f32>, Vec<f32>)> {
    (0usize..8).prop_flat_map(|n| (widths(n), widths(n)))
}

/// Room for the columns: nothing, little, much.
fn room() -> impl Strategy<Value = f32> {
    prop_oneof![1 => Just(0.0f32), 1 => -50.0f32..0.0, 6 => 0.0f32..3000.0, 1 => 3000.0f32..20000.0]
}

proptest! {
    #![proptest_config(config())]

    // spec: EDIT-024
    #[test]
    fn edit_024_column_widths_are_whole_numbers_that_add_up_to_at_most_the_room((natural, min) in column_input(), avail in room()) {
        let w = column_widths(&natural, &min, avail);
        prop_assert_eq!(w.len(), natural.len());
        prop_assert!(w.iter().all(|x| x.is_finite() && *x >= 0.0), "{:?}", w);
        let sum: f32 = w.iter().sum();
        let nat: f32 = natural.iter().sum();
        // only the natural widths may be wider than the room, and only when they all fit it
        prop_assert!(sum <= avail.max(0.0) * 1.0001 + 0.01 || nat <= avail, "{} of {}: {:?}", sum, avail, w);
        for ((x, n), m) in w.iter().zip(&natural).zip(&min) {
            prop_assert!(*x <= *n * 1.0001 + 0.01, "{} is wider than the natural {}", x, n);
            let floor_sum: f32 = natural.iter().zip(&min).map(|(n, m)| m.min(*n)).sum();
            if floor_sum <= avail {
                prop_assert!(*x + 0.01 >= m.min(*n) * 0.9999, "{} is narrower than the minimum {} (natural {})", x, m, n);
            }
        }
    }

    // spec: EDIT-024
    #[test]
    fn edit_024_column_widths_are_the_natural_ones_when_they_fit((natural, min) in column_input(), extra in 0.0f32..100.0) {
        let avail = natural.iter().sum::<f32>() + extra;
        prop_assert_eq!(column_widths(&natural, &min, avail), natural);
    }

    // spec: EDIT-031
    #[test]
    fn edit_031_stored_widths_keep_the_room_the_minimum_and_are_checked(cols in prop::collection::vec(prop_oneof![8 => 0.0f32..1000.0, 1 => Just(-1.0f32), 1 => Just(f32::NAN), 1 => Just(f32::INFINITY)], 0..7), ncols in 0usize..7, avail in room()) {
        let valid = ncols != 0 && cols.len() == ncols && cols.iter().all(|c| c.is_finite() && *c > 0.0);
        let Some(w) = stored_widths(&cols, ncols, avail) else {
            prop_assert!(!valid, "valid sizes are used");
            return Ok(());
        };
        prop_assert!(valid, "invalid sizes are ignored");
        prop_assert_eq!(w.len(), ncols);
        prop_assert!(w.iter().all(|x| x.is_finite() && *x >= 0.0), "{:?}", w);
        let clamped: f32 = cols.iter().map(|c| c.max(MIN_COL)).sum();
        let sum: f32 = w.iter().sum();
        if clamped <= avail - 1.0 {
            let want: Vec<f32> = cols.iter().map(|c| c.max(MIN_COL)).collect();
            prop_assert_eq!(w, want, "as stored when they fit, at least MIN_COL");
        } else {
            prop_assert!(sum <= (avail - 1.0).max(0.0) * 1.0001 + 0.01, "{} of {}", sum, avail);
        }
    }
}

/// A note made of lines that make and break tables: pipes, delimiter rows, fences, front matter,
/// comment lines, blank lines, non-ASCII, `\r`.
fn note() -> impl Strategy<Value = String> {
    let line = prop_oneof![
        3 => Just("| a | b |".to_string()),
        3 => Just("|---|:---:|".to_string()),
        2 => Just("a | b".to_string()),
        2 => Just("--|--".to_string()),
        2 => Just("| x |".to_string()),
        2 => Just("```".to_string()),
        2 => Just("---".to_string()),
        2 => Just("<!-- shale-table edit=cells cols=10,20 -->".to_string()),
        2 => Just("".to_string()),
        1 => Just("\r".to_string()),
        1 => Just("é | 日🙂 |".to_string()),
        3 => table_line(),
    ];
    prop::collection::vec(line, 0..14).prop_map(|l| l.join("\n"))
}

proptest! {
    #![proptest_config(config())]

    // spec: EDIT-024
    #[test]
    fn edit_024_the_tables_found_lie_in_the_note_in_order_with_even_rows(text in note()) {
        let n = text.split('\n').count();
        let mut at = 0;
        for b in find_tables(&text) {
            prop_assert!(b.lines.start >= at && b.lines.start < b.lines.end && b.lines.end <= n, "{:?} of {} lines", b.lines, n);
            let t = b.table_lines();
            prop_assert!(t.start + 2 <= t.end, "a table has a header and a delimiter row: {:?}", b.lines);
            prop_assert_eq!(b.table.aligns.len(), b.table.header.len());
            prop_assert_eq!(t.end - t.start - 2, b.table.rows.len());
            prop_assert!(b.table.rows.iter().all(|r| r.len() == b.table.header.len()), "{:?}", b.table);
            prop_assert_eq!(b.has_meta, b.lines.start != t.start);
            let bytes = byte_range(&text, &b.lines);
            prop_assert!(bytes.start <= bytes.end && bytes.end <= text.len(), "{:?} of {}", bytes, text.len());
            prop_assert!(text.is_char_boundary(bytes.start) && text.is_char_boundary(bytes.end));
            at = b.lines.end;
        }
    }

    // spec: EDIT-024
    #[test]
    fn edit_024_the_bytes_of_any_lines_lie_in_the_text(text in "\\PC{0,40}(\n\\PC{0,10}){0,6}", a in 0usize..12, b in 0usize..12) {
        let lines = a.min(b)..a.max(b);
        let r = byte_range(&text, &lines);
        prop_assert!(r.start <= r.end && r.end <= text.len(), "{:?} of {}", r, text.len());
        prop_assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        let _ = find_tables(&text);
    }
}
