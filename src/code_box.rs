//! Fenced code blocks drawn as boxes in the editor's Markdown preview (EDIT-080): a rounded
//! rectangle on the code background with a border and a badge with the language in its top-right
//! corner. The editor hides the fence lines (the opening one keeps its row, the badge sits in it)
//! and gives the lines of the block no background of their own; this paints the box behind them.

use crate::fold::FoldKind;
use crate::fold_ui::{line_chars, Frame};
use crate::theme;
use eframe::egui::{self, epaint::StrokeKind, text::CCursor, Galley, Pos2, Rect, Shape, Stroke, Ui};

/// The shapes of the boxes of the code blocks of `frame` that are on the screen, over the laid-out
/// text at `pos` and up to `right`.
pub fn shapes(ui: &Ui, frame: &Frame, galley: &Galley, pos: Pos2, right: f32, base: f32) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let blocks: Vec<_> = frame.blocks.iter().filter(|b| b.kind == FoldKind::Code).collect();
    if blocks.is_empty() {
        return shapes;
    }
    let text = galley.job.text.as_str();
    let lines = line_chars(text);
    let clip = ui.clip_rect();
    let row_of = |line: usize, offset: usize| {
        let r = galley.pos_from_cursor(CCursor::new(lines[line].0 + offset));
        Rect::from_min_max(pos + r.min.to_vec2(), pos + r.max.to_vec2())
    };
    for b in blocks {
        // a block inside a folded block is not drawn; a folded one is the row of its opening fence
        if b.header >= lines.len() || frame.view.is_hidden(b.header) {
            continue;
        }
        let last = if frame.view.is_hidden(b.header + 1) { b.header } else { (b.end - 1).min(lines.len() - 1) };
        let head = row_of(b.header, 0);
        let tail = row_of(last, lines[last].1);
        if tail.max.y < clip.min.y || head.min.y > clip.max.y {
            continue;
        }
        let rect = Rect::from_min_max(Pos2::new(pos.x - 4.0, head.min.y), Pos2::new(right - 2.0, tail.max.y.max(head.max.y) + 3.0));
        shapes.push(Shape::rect_filled(rect, 6.0, theme::CODE_BG));
        shapes.push(Shape::rect_stroke(rect, 6.0, Stroke::new(1.0, theme::BORDER), StrokeKind::Inside));
        let badge = crate::syntax::badge_language(&crate::syntax::fence_language(&b.title)).unwrap_or_else(|| "code".to_string());
        let g = ui.painter().layout_no_wrap(badge, egui::FontId::monospace((base * 0.75).max(9.0)), theme::FAINT);
        let at = Pos2::new(rect.right() - 8.0 - g.size().x, head.center().y - g.size().y / 2.0);
        shapes.push(Shape::galley(at, g, theme::FAINT));
    }
    shapes
}
