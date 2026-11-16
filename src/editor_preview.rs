//! Markdown preview in the editor (EDIT-016, EDIT-017): which lines stay raw.

use std::ops::Range;

/// The line numbers shown as raw markdown: the lines the cursor or selection (byte offsets
/// `sel`) touches, plus `around` lines before and after. Without a selection (the editor does
/// not have the keyboard) no line is raw.
pub fn raw_lines(text: &str, sel: Option<Range<usize>>, around: usize) -> Range<usize> {
    let Some(sel) = sel else { return 0..0 };
    let line_of = |byte: usize| text.as_bytes()[..byte.min(text.len())].iter().filter(|b| **b == b'\n').count();
    let (first, last) = (line_of(sel.start), line_of(sel.end));
    first.saturating_sub(around)..last + around + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: EDIT-017
    #[test]
    fn raw_lines_follow_the_cursor_selection_and_setting() {
        let t = "a\nb\nc\nd\ne";
        assert_eq!(raw_lines(t, Some(4..4), 0), 2..3, "only the cursor line");
        assert_eq!(raw_lines(t, Some(4..4), 1), 1..4, "one line before and after");
        assert_eq!(raw_lines(t, Some(0..0), 2), 0..3, "the start of the note");
        assert_eq!(raw_lines(t, Some(2..6), 0), 1..4, "every line of a selection");
        assert_eq!(raw_lines(t, Some(2..6), 1), 0..5, "and the lines around it");
        assert!(raw_lines(t, None, 3).is_empty(), "without the keyboard everything is rendered");
    }
}
