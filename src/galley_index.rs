//! A lookup over a laid-out note for what the editor paints on top of its galley (EDIT-105): which
//! char starts which line and row, so that the markers, badges, boxes and heading numbers of
//! thousands of lines are placed without asking egui for the position of each (its
//! `pos_from_cursor` walks the rows) and the ones far off the screen are skipped.

use crate::fold_ui::line_chars;
use eframe::egui::Galley;

/// The lines and rows of one galley, found once per frame.
pub struct GalleyIndex {
    /// for each line of the text: the char index where it starts and how many chars it shows
    pub lines: Vec<(usize, usize)>,
    /// for each row of the galley: the char index where it starts
    starts: Vec<usize>,
    /// for each row: its top and bottom, in the galley's own coordinates
    spans: Vec<(f32, f32)>,
}

impl GalleyIndex {
    pub fn new(galley: &Galley) -> GalleyIndex {
        let mut starts = Vec::with_capacity(galley.rows.len());
        let mut spans = Vec::with_capacity(galley.rows.len());
        let mut at = 0;
        for row in &galley.rows {
            starts.push(at);
            at += row.char_count_including_newline().0;
            let r = row.rect();
            spans.push((r.min.y, r.max.y));
        }
        GalleyIndex { lines: line_chars(&galley.job.text), starts, spans }
    }

    /// The index of the row that holds char `c` (the last row for one past the end).
    pub fn row_of_char(&self, c: usize) -> Option<usize> {
        self.starts.partition_point(|s| *s <= c).checked_sub(1)
    }

    /// The top and bottom (in the galley's coordinates) of the rows that can hold char `c`: its own
    /// and the ones next to it, since a cursor at the end of a wrapped row may stand on either.
    pub fn span_of_char(&self, c: usize) -> (f32, f32) {
        let Some(row) = self.row_of_char(c) else { return (0.0, 0.0) };
        (self.spans[row.saturating_sub(1)].0, self.spans[(row + 1).min(self.spans.len() - 1)].1)
    }

    /// Whether char `c` is surely not between the heights `top` and `bottom` of the galley's
    /// coordinates.
    pub fn off_screen(&self, c: usize, top: f32, bottom: f32) -> bool {
        let (a, b) = self.span_of_char(c);
        b < top || a > bottom
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, text::CCursor, Color32, FontId};

    fn galley(text: &str, width: f32) -> std::sync::Arc<Galley> {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(900.0, 700.0))), ..Default::default() };
        let mut out = None;
        let mut done = ctx.run_ui(raw, |ui| out = Some(ui.painter().layout(text.to_string(), FontId::monospace(14.0), Color32::WHITE, width)));
        done.textures_delta.clear();
        out.unwrap()
    }

    // spec: EDIT-105
    #[test]
    fn the_index_finds_the_row_of_every_char_and_skips_only_what_is_off_screen() {
        let text = "one\ntwo two two two two two two two two two\nthree\n\nfive";
        for width in [60.0, 10_000.0] {
            let g = galley(text, width);
            let ix = GalleyIndex::new(&g);
            assert_eq!(ix.lines.len(), 5);
            for (c, _) in text.chars().enumerate() {
                let y = g.pos_from_cursor(CCursor::new(c));
                let (a, b) = ix.span_of_char(c);
                assert!(a <= y.min.y + 0.01 && y.max.y <= b + 0.01, "char {c} at {y:?} is in its span {a}..{b} (width {width})");
                assert!(!ix.off_screen(c, y.min.y, y.max.y), "a char on the screen is not skipped");
            }
            let last = text.chars().count();
            assert!(ix.off_screen(0, 10_000.0, 10_100.0) && ix.off_screen(last, -500.0, -400.0));
            assert_eq!(ix.row_of_char(0), Some(0));
        }
    }
}
