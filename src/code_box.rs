//! Fenced code blocks drawn as boxes in the editor's Markdown preview (EDIT-080): a rounded
//! rectangle on the code background with a border and a badge with the language in its top-right
//! corner. The editor hides the fence lines (the opening one keeps its row, the badge sits in it)
//! and gives the lines of the block no background of their own; this paints the box behind them.
//! Also the copy button of a box and the code it copies (EDIT-090…EDIT-094).

use crate::fold::FoldKind;
use crate::fold_ui::{Frame, Painted};
use crate::galley_index::GalleyIndex;
use crate::theme;
use eframe::egui::{self, epaint::StrokeKind, text::CCursor, Galley, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
use std::path::Path;
use std::sync::Arc;

/// How long the copy button says "Copied" (EDIT-091), in seconds.
const FLASH: f64 = 1.5;

/// The shapes of the boxes of the code blocks of `frame` that are on the screen, over the laid-out
/// text at `pos` and up to `right`.
pub fn shapes(ui: &Ui, frame: &Frame, (galley, index): (&Galley, &GalleyIndex), pos: Pos2, right: f32, base: f32) -> Vec<Shape> {
    let mut shapes = Vec::new();
    let blocks: Vec<_> = frame.blocks.iter().filter(|b| b.kind == FoldKind::Code).collect();
    if blocks.is_empty() {
        return shapes;
    }
    let lines = &index.lines;
    let clip = ui.clip_rect();
    let (top, bottom) = (clip.min.y - pos.y, clip.max.y - pos.y);
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
        // far off the screen: no need to ask the galley where the rows are (EDIT-105)
        if index.span_of_char(lines[last].0 + lines[last].1).1 < top || index.span_of_char(lines[b.header].0).0 > bottom {
            continue;
        }
        let head = row_of(b.header, 0);
        let tail = row_of(last, lines[last].1);
        if tail.max.y < clip.min.y || head.min.y > clip.max.y {
            continue;
        }
        let rect = Rect::from_min_max(Pos2::new(pos.x - 4.0, head.min.y), Pos2::new(right - 2.0, tail.max.y.max(head.max.y) + 3.0));
        shapes.push(Shape::rect_filled(rect, 6.0, theme::palette().code_bg));
        shapes.push(Shape::rect_stroke(rect, 6.0, Stroke::new(1.0, theme::palette().border), StrokeKind::Inside));
        let g = badge_galley(ui, &b.title, base);
        let at = Pos2::new(rect.right() - 8.0 - g.size().x, head.center().y - g.size().y / 2.0);
        shapes.push(Shape::galley(at, g, theme::palette().faint));
    }
    shapes
}

/// The badge text of a code block whose opening fence line is `title`: the language, or `code`.
fn badge_galley(ui: &Ui, title: &str, base: f32) -> Arc<Galley> {
    let badge = crate::syntax::badge_language(&crate::syntax::fence_language(title)).unwrap_or_else(|| "code".to_string());
    ui.painter().layout_no_wrap(badge, egui::FontId::monospace((base * 0.75).max(9.0)), theme::palette().faint)
}

/// The code of the fenced block that opens at line `open` of `lines` (EDIT-090): its lines as written,
/// joined with `\n`, without the fences and without a newline at the end, the indentation of the opening
/// fence taken off every line (a block in a list item). `None` when no block opens there.
pub fn code_of(lines: &[&str], open: usize) -> Option<String> {
    let f = crate::syntax::fences(lines).into_iter().find(|f| f.open == open)?;
    let indent = lines[open].chars().take_while(|c| c.is_whitespace()).count();
    let mut body: Vec<&str> = lines[open + 1..if f.closed { f.end - 1 } else { f.end }].iter().map(|l| l.strip_suffix('\r').unwrap_or(l)).collect();
    if !f.closed {
        while body.last().is_some_and(|l| l.trim().is_empty()) {
            body.pop();
        }
    }
    let unindented: Vec<&str> = body
        .into_iter()
        .map(|l| {
            let mut rest = l;
            for _ in 0..indent {
                match rest.chars().next() {
                    Some(c) if c.is_whitespace() => rest = &rest[c.len_utf8()..],
                    _ => break,
                }
            }
            rest
        })
        .collect();
    Some(unindented.join("\n"))
}

/// The block that line `line` is in (its fences included): its opening line and its code (EDIT-092).
pub fn code_at(lines: &[&str], line: usize) -> Option<(usize, String)> {
    let f = crate::syntax::fences(lines).into_iter().find(|f| (f.open..f.end).contains(&line))?;
    code_of(lines, f.open).map(|c| (f.open, c))
}

/// The identity of the copy button of the block that opens at line `open` of the note `rel`.
pub fn copy_id(id: Id, rel: &Path, open: usize) -> Id {
    id.with(("code-copy", rel, open))
}

/// Put `code` on the clipboard and let the button `id` say "Copied" for a moment (EDIT-091).
pub fn copy(ui: &Ui, id: Id, code: String) {
    ui.ctx().copy_text(code);
    let now = ui.input(|i| i.time);
    ui.ctx().data_mut(|d| d.insert_temp(id, now));
    ui.ctx().request_repaint_after_secs(FLASH as f32 + 0.05);
}

/// Does the button `id` say "Copied" now? Asks for the frame that takes it back.
fn flashing(ui: &Ui, id: Id) -> bool {
    let now = ui.input(|i| i.time);
    let Some(at) = ui.ctx().data(|d| d.get_temp::<f64>(id)) else { return false };
    let left = FLASH - (now - at);
    if left > 0.0 {
        ui.ctx().request_repaint_after_secs(left as f32 + 0.05);
    }
    left > 0.0
}

/// The copy button of a code box (EDIT-090, EDIT-091), its right edge at `right`, centred on `y`. It is
/// there when `shown` (the pointer or the cursor is in the box) and while it says "Copied"; a click
/// puts what `code` makes on the clipboard. Where the button is, when it is there.
pub fn copy_button(ui: &Ui, id: Id, right: f32, y: f32, shown: bool, code: impl FnOnce() -> String) -> Option<Rect> {
    let copied = flashing(ui, id);
    if !shown && !copied {
        return None;
    }
    let label = copied.then(|| ui.painter().layout_no_wrap("Copied".to_string(), egui::FontId::proportional(11.0), theme::palette().muted));
    let width = 16.0 + label.as_ref().map_or(0.0, |g| g.size().x + 4.0);
    let rect = Rect::from_min_max(Pos2::new(right - width, y - 9.0), Pos2::new(right, y + 9.0));
    // it takes drags too: a button that only senses clicks leaves the drag to the text box under it, which then
    // moves its cursor to the pointer for as long as the button is held (EDIT-094)
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let colour = if resp.hovered() || copied { theme::palette().text } else { theme::palette().muted };
    let painter = ui.painter();
    let c = Pos2::new(rect.right() - 8.0, y);
    if copied {
        painter.add(Shape::line(vec![c + Vec2::new(-4.5, 0.5), c + Vec2::new(-1.5, 3.5), c + Vec2::new(4.5, -3.5)], Stroke::new(1.6, colour)));
    } else {
        let stroke = Stroke::new(1.2, colour);
        // the sheet behind (its top and right edges) and the sheet in front
        let back = Rect::from_center_size(c + Vec2::new(1.5, -1.5), Vec2::new(8.0, 9.0));
        painter.add(Shape::line(vec![back.left_top(), back.right_top(), back.right_bottom()], stroke));
        let front = Rect::from_center_size(c + Vec2::new(-1.5, 1.5), Vec2::new(8.0, 9.0));
        painter.rect_stroke(front, 1.5, stroke, StrokeKind::Inside);
    }
    if let Some(g) = label {
        painter.galley(Pos2::new(rect.left(), y - g.size().y / 2.0), g, colour);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.on_hover_text("Copy the code").clicked() {
        copy(ui, id, code());
    }
    Some(rect)
}

/// The copy buttons of the code boxes of the editor (EDIT-090): at the top right of a box, left of its
/// badge, while the pointer is over the box or the cursor (in line `cursor`) is in it, and always on the
/// row of a folded block. The opening line of each block that has a button, and where the button is.
pub fn buttons(ui: &Ui, frame: &Frame, at: &Painted, cursor: Option<usize>) -> Vec<(usize, Rect)> {
    let mut out = Vec::new();
    let text = at.galley.job.text.as_str();
    let lines = &at.index.lines;
    let hover = ui.input(|i| i.pointer.hover_pos());
    let clip = ui.clip_rect();
    let (top, bottom) = (clip.min.y - at.pos.y, clip.max.y - at.pos.y);
    let row_of = |line: usize, offset: usize| {
        let r = at.galley.pos_from_cursor(CCursor::new(lines[line].0 + offset));
        Rect::from_min_max(at.pos + r.min.to_vec2(), at.pos + r.max.to_vec2())
    };
    for b in frame.blocks.iter().filter(|b| b.kind == FoldKind::Code) {
        if b.header >= lines.len() || frame.view.is_hidden(b.header) {
            continue;
        }
        let folded = frame.view.is_hidden(b.header + 1);
        let last = if folded { b.header } else { (b.end - 1).min(lines.len() - 1) };
        if at.index.span_of_char(lines[last].0 + lines[last].1).1 < top || at.index.span_of_char(lines[b.header].0).0 > bottom {
            continue;
        }
        let head = row_of(b.header, 0);
        let tail = row_of(last, lines[last].1);
        if tail.max.y < clip.min.y || head.min.y > clip.max.y {
            continue;
        }
        let bounds = Rect::from_min_max(Pos2::new(at.pos.x - 4.0, head.min.y), Pos2::new(at.area.right() - 2.0, tail.max.y.max(head.max.y) + 3.0));
        let shown = folded || hover.is_some_and(|p| bounds.contains(p)) || cursor.is_some_and(|l| (b.header..b.end).contains(&l));
        let right = bounds.right() - 8.0 - badge_galley(ui, &b.title, at.base).size().x - 6.0;
        let code = || code_of(&text.split('\n').collect::<Vec<_>>(), b.header).unwrap_or_default();
        if let Some(r) = copy_button(ui, copy_id(at.id, at.rel, b.header), right, head.center().y, shown, code) {
            out.push((b.header, r));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(t: &str) -> Vec<&str> {
        t.split('\n').collect()
    }

    // spec: EDIT-090
    #[test]
    fn the_code_of_a_block_is_its_lines_as_written_without_the_fences_and_the_last_newline() {
        let t = lines("a\n```sh\ncargo build\n  --release\n\n```\nb");
        assert_eq!(code_of(&t, 1).as_deref(), Some("cargo build\n  --release\n"));
        assert_eq!(code_of(&t, 0), None, "no block opens there");
        assert_eq!(code_of(&lines("~~~\nx\n~~~"), 0).as_deref(), Some("x"));
        assert_eq!(code_of(&lines("```rust\nfn a() {}\r\n```"), 0).as_deref(), Some("fn a() {}"), "no carriage return");
    }

    // spec: EDIT-090
    #[test]
    fn an_empty_block_has_empty_code_and_an_unclosed_one_runs_to_the_end_without_its_blank_lines() {
        assert_eq!(code_of(&lines("```\n```"), 0).as_deref(), Some(""));
        assert_eq!(code_of(&lines("```\nx\n\n"), 0).as_deref(), Some("x"));
        assert_eq!(code_of(&lines("```\nx\ny"), 0).as_deref(), Some("x\ny"));
    }

    // spec: EDIT-090
    #[test]
    fn the_indentation_of_the_opening_fence_is_taken_off_every_line() {
        let t = lines("- item\n    ```sh\n    ls\n      -la\n  odd\n    ```\n- next");
        assert_eq!(code_of(&t, 1).as_deref(), Some("ls\n  -la\nodd"), "a line indented less keeps what is left");
    }

    // spec: EDIT-092
    #[test]
    fn the_block_at_a_line_is_found_from_its_fences_and_everything_between() {
        let t = lines("x\n```\none\n```\ny\n~~~py\ntwo\n~~~");
        assert_eq!(code_at(&t, 1), Some((1, "one".to_string())), "the opening fence");
        assert_eq!(code_at(&t, 2), Some((1, "one".to_string())));
        assert_eq!(code_at(&t, 3), Some((1, "one".to_string())), "the closing fence");
        assert_eq!(code_at(&t, 6), Some((5, "two".to_string())));
        assert_eq!(code_at(&t, 0), None);
        assert_eq!(code_at(&t, 4), None);
    }
}
