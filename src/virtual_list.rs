//! A long list of rows of different heights that draws only the rows in view (PERF-001): the rows
//! above and below are one empty space each, as high as the rows measured so far (and an estimate
//! for the rows never drawn), so the scroll bar keeps its length and a row measured later only
//! moves the rows after it.

use eframe::egui::Ui;

/// How far above and below the visible part rows are drawn too, so a scroll step finds them ready.
const MARGIN: f32 = 150.0;

#[derive(Default)]
pub struct RowHeights {
    /// per row: its height with the space after it, 0 while it was never drawn
    heights: Vec<f32>,
    /// how many rows the last call drew
    drawn: usize,
}

impl RowHeights {
    /// Draw the rows `0..n` that are in view with `row` (called as `row(ui, index)`). `estimate`
    /// is the height of a row that was never drawn. Call it inside a scroll area, on the ui the
    /// list goes in.
    pub fn show(&mut self, ui: &mut Ui, n: usize, estimate: f32, mut row: impl FnMut(&mut Ui, usize)) {
        self.heights.resize(n, 0.0);
        let gap = ui.spacing().item_spacing.y;
        let top = ui.cursor().min.y;
        let clip = ui.clip_rect();
        let (from, to) = (clip.min.y - MARGIN, clip.max.y + MARGIN);
        let height = |heights: &[f32], i: usize| if heights[i] > 0.0 { heights[i] } else { estimate + gap };
        // the first row that ends below `from`, the first that starts below `to`
        let (mut first, mut first_y, mut end, mut end_y, mut y) = (n, 0.0, n, 0.0, 0.0);
        for i in 0..n {
            let h = height(&self.heights, i);
            if first == n && top + y + h >= from {
                (first, first_y) = (i, y);
            }
            if first != n && end == n && top + y > to {
                (end, end_y) = (i, y);
            }
            y += h;
        }
        let total = y;
        if first == n {
            // everything is above the view (the list got shorter than the scroll position): the end
            (first, first_y) = (n.saturating_sub(1), total - n.checked_sub(1).map_or(0.0, |i| height(&self.heights, i)));
        }
        if end == n {
            end_y = total;
        }
        // (`add_space` adds no spacing after itself, and each row's height has its gap in it)
        if first_y > 0.0 {
            ui.add_space(first_y);
        }
        self.drawn = 0;
        for i in first..end.max(first) {
            let shown = ui.scope(|ui| row(ui, i));
            self.heights[i] = shown.response.rect.height() + gap;
            self.drawn += 1;
        }
        if end < n && total - end_y > 0.0 {
            ui.add_space(total - end_y);
        }
    }

    /// How many rows the last call drew (tests).
    #[cfg(test)]
    pub fn drawn(&self) -> usize {
        self.drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{self, Pos2, RawInput, Rect, Vec2};

    fn run(rows: &mut RowHeights, ctx: &egui::Context, n: usize, scroll: f32, seen: &mut Vec<usize>) {
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::ScrollArea::vertical().vertical_scroll_offset(scroll).auto_shrink([false, false]).show(ui, |ui| {
                rows.show(ui, n, 20.0, |ui, i| {
                    seen.push(i);
                    // rows of two heights
                    ui.label(if i % 3 == 0 { "one\ntwo" } else { "one" });
                });
            });
        });
        out.textures_delta.clear();
    }

    // spec: PERF-001
    #[test]
    fn perf_001_a_long_list_draws_only_the_rows_in_view_and_finds_the_others_when_scrolled_to() {
        let ctx = egui::Context::default();
        let mut rows = RowHeights::default();
        let mut seen = vec![];
        run(&mut rows, &ctx, 5000, 0.0, &mut seen);
        assert!(seen.first() == Some(&0) && rows.drawn() < 60 && rows.drawn() > 5, "{} rows drawn", rows.drawn());
        // a second frame knows the heights of the rows drawn and draws the same rows
        let before = rows.drawn();
        seen.clear();
        run(&mut rows, &ctx, 5000, 0.0, &mut seen);
        assert!(rows.drawn() <= before + 2);
        // scrolled far down: other rows, still few
        seen.clear();
        run(&mut rows, &ctx, 5000, 60_000.0, &mut seen);
        run(&mut rows, &ctx, 5000, 60_000.0, &mut seen);
        assert!(seen.iter().any(|&i| i > 2000) && rows.drawn() < 60, "drew {:?}", seen.iter().copied().max());
        // a list that got short while scrolled far down still draws its last rows
        seen.clear();
        run(&mut rows, &ctx, 10, 60_000.0, &mut seen);
        run(&mut rows, &ctx, 10, 60_000.0, &mut seen);
        assert!(rows.drawn() > 0 && seen.contains(&9), "{seen:?}");
        // an empty list draws nothing and does not fail
        run(&mut rows, &ctx, 0, 0.0, &mut seen);
        assert_eq!(rows.drawn(), 0);
    }
}
