//! The box of a task (EDIT-016, NAV-056): the bundled fonts have no ☐ / ☑, so the box is painted.
//! The editor's Markdown preview and the hits of the search both make room for it in their text
//! (a section with leading space, see [`room`]) and paint it there with [`paint_in_galley`].

use crate::galley_index::GalleyIndex;
use crate::markdown;
use crate::theme;
use eframe::egui::text::{CCursor, LayoutJob};
use eframe::egui::{self, Color32, CornerRadius, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::ops::Range;

/// The room before the hidden marker of a task, in em of its text; the box is [`SIDE_EM`] wide in it.
const ROOM_EM: f32 = 1.1;
const SIDE_EM: f32 = 0.8;

/// The leading space of the hidden section that stands for a task's box in text of size `em`.
pub fn room(em: f32) -> f32 {
    em * ROOM_EM
}

/// A box in a laid-out text: the hidden marker it stands for (bytes of the job's text), the room
/// it has before it and whether it is ticked.
#[derive(Clone, Debug, PartialEq)]
pub struct Gap {
    pub marker: Range<usize>,
    pub room: f32,
    pub done: bool,
}

/// Whether a section is the hidden marker of a task: invisible text of one point with room before it.
fn is_marker(s: &egui::text::LayoutSection) -> bool {
    s.leading_space > 0.0 && s.format.color == Color32::TRANSPARENT && s.format.font_id.size == 1.0
}

/// The boxes of `job`, in the order of its text.
pub fn gaps(job: &LayoutJob) -> Vec<Gap> {
    job.sections
        .iter()
        .filter(|s| is_marker(s))
        .map(|s| {
            let marker = s.byte_range.start.0..s.byte_range.end.0;
            let done = job.text[marker.clone()].contains(['x', 'X']);
            Gap { marker, room: s.leading_space, done }
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    static PAINTED: std::cell::RefCell<Vec<(Rect, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The boxes painted since the last call, with whether each is ticked (tests).
#[cfg(test)]
pub fn take_painted() -> Vec<(Rect, bool)> {
    PAINTED.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

/// Paint one box: a rounded square with the tick of a ticked task.
pub fn paint(painter: &Painter, rect: Rect, done: bool) {
    #[cfg(test)]
    PAINTED.with(|p| p.borrow_mut().push((rect, done)));
    let p = theme::palette();
    painter.rect(rect, CornerRadius::same(2), p.widget_bg, Stroke::new(1.0, p.faint), StrokeKind::Inside);
    if done {
        let r = rect.shrink(rect.width() * 0.22);
        let tick = [Pos2::new(r.left(), r.center().y), Pos2::new(r.center().x - r.width() * 0.1, r.bottom()), Pos2::new(r.right(), r.top())];
        painter.add(egui::Shape::line(tick.to_vec(), Stroke::new(1.6, p.accent)));
    }
}

/// Where the box of `gap` is in `galley` laid out at `pos`: left in its room, sitting on the baseline
/// of the text of its row. `None` when the marker is not in the galley.
pub fn rect_in_galley(galley: &Galley, index: &GalleyIndex, pos: Pos2, at_char: usize, room: f32) -> Option<Rect> {
    let at = galley.pos_from_cursor(CCursor::new(at_char));
    let row = galley.rows.get(index.row_of_char(at_char)?)?;
    let baseline = row.row.glyphs.iter().filter(|g| g.font_height > 2.0).map(|g| g.pos.y).reduce(f32::max).unwrap_or(row.rect().height() * 0.78);
    let side = room / ROOM_EM * SIDE_EM;
    // (a point in: the layout rounds the room to pixels, and the box must not reach into the text before it)
    let min = Pos2::new(at.min.x - room + 1.0, row.pos.y + baseline - side);
    Some(Rect::from_min_size(pos + min.to_vec2(), Vec2::splat(side)))
}

/// Paint the box of every gap of `galley` (at `pos`) that is on the screen (the clip rect of
/// `painter`) and say where each is: its rect and the byte of its check (the `x` or space
/// between the brackets).
pub fn paint_in_galley(painter: &Painter, (galley, index): (&Galley, &GalleyIndex), pos: Pos2) -> Vec<(Rect, usize)> {
    let text = &galley.job.text;
    let mut out = Vec::new();
    let clip = painter.clip_rect();
    let (top, bottom) = (clip.min.y - pos.y, clip.max.y - pos.y);
    // the gaps are in the order of the text: the char index of each is found going on from the last (EDIT-105)
    let (mut chars, mut byte) = (0, 0);
    for gap in gaps(&galley.job) {
        chars += text[byte..gap.marker.start].chars().count();
        byte = gap.marker.start;
        if index.off_screen(chars, top, bottom) {
            continue;
        }
        let Some(rect) = rect_in_galley(galley, index, pos, chars, gap.room) else { continue };
        // the marker ends in `x]` / ` ]`
        out.push((rect, gap.marker.end - 2));
        paint(painter, rect, gap.done);
    }
    out
}

/// The byte of the check of the task whose box has `rect` under the point `p`, if any.
pub fn check_under(boxes: &[(Rect, usize)], p: Pos2) -> Option<usize> {
    boxes.iter().find(|(r, _)| r.expand(2.0).contains(p)).map(|(_, c)| *c)
}

/// The marker of a task line (its bytes) that the preview and the hits hide: a bullet task
/// loses its bullet too, a numbered one keeps its number.
pub fn marker_range(line: &str) -> Option<Range<usize>> {
    let (check, _, _, indent) = markdown::parse_task_line(line)?;
    let numbered = line[indent..].starts_with(|c: char| c.is_ascii_digit());
    Some(if numbered { check - 1..check + 2 } else { indent..check + 2 })
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: EDIT-016
    #[test]
    fn a_bullet_task_hides_its_bullet_and_a_numbered_one_keeps_its_number() {
        assert_eq!(marker_range("- [ ] a"), Some(0..5));
        assert_eq!(marker_range("  * [x] a"), Some(2..7));
        assert_eq!(marker_range("1. [ ] a"), Some(3..6));
        assert_eq!(marker_range("- plain"), None);
    }
}
