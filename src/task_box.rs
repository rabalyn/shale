//! The box of a task and the dot of a list bullet (EDIT-016, NAV-056): the bundled fonts have no ☐ / ☑
//! and `•` is not drawn the same at every size, so both are painted. The editor's Markdown preview and
//! the hits of the search both make room for them in their text (a section with leading space, see
//! [`room`]) and paint them there with [`paint_in_galley`].

use crate::galley_index::GalleyIndex;
use crate::markdown;
use crate::theme;
use eframe::egui::text::{CCursor, LayoutJob};
use eframe::egui::{self, Color32, CornerRadius, Galley, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::ops::Range;

/// The room before the hidden marker of a task, in em of its text; the box is [`SIDE_EM`] wide in it.
const ROOM_EM: f32 = 1.1;
const SIDE_EM: f32 = 0.8;
/// The radius of a dot, in sides of the square it is painted in.
const DOT_RADIUS: f32 = 0.22;

/// The leading space of the hidden section that stands for a task's box in text of size `em`.
pub fn room(em: f32) -> f32 {
    em * ROOM_EM
}

/// What is painted in a gap: the box of a task (ticked or not) or the dot of a list bullet, filled
/// on the top level and hollow on the next, and so on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Box { done: bool },
    Bullet { level: usize },
}

/// A box or a dot in a laid-out text: the hidden marker it stands for (bytes of the job's text), the
/// room it has before it and what is painted.
#[derive(Clone, Debug, PartialEq)]
pub struct Gap {
    pub marker: Range<usize>,
    pub room: f32,
    pub kind: Kind,
}

/// Whether a section is the hidden marker of a task: invisible text of one point with room before it.
fn is_marker(s: &egui::text::LayoutSection) -> bool {
    s.leading_space > 0.0 && s.format.color == Color32::TRANSPARENT && s.format.font_id.size == 1.0
}

/// The boxes and dots of `job`, in the order of its text. A bullet's marker is the space after its
/// hidden `-` (a task's has brackets); its level is its indent as the list code counts it.
pub fn gaps(job: &LayoutJob) -> Vec<Gap> {
    job.sections
        .iter()
        .filter(|s| is_marker(s))
        .map(|s| {
            let marker = s.byte_range.start.0..s.byte_range.end.0;
            let kind = if job.text[marker.clone()].trim().is_empty() {
                let line = job.text[..marker.start].rfind('\n').map_or(0, |n| n + 1);
                let width: usize = job.text[line..marker.start.saturating_sub(1)].chars().map(|c| if c == '\t' { crate::lists::INDENT.len() } else { 1 }).sum();
                Kind::Bullet { level: width / crate::lists::INDENT.len() }
            } else {
                Kind::Box { done: job.text[marker.clone()].contains(['x', 'X']) }
            };
            Gap { marker, room: s.leading_space, kind }
        })
        .collect()
}

#[cfg(test)]
thread_local! {
    static PAINTED: std::cell::RefCell<Vec<(Rect, Kind)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The boxes and dots painted since the last call (tests).
#[cfg(test)]
pub fn take_painted() -> Vec<(Rect, Kind)> {
    PAINTED.with(|p| std::mem::take(&mut *p.borrow_mut()))
}

/// Paint one box (a rounded square with the tick of a ticked task) or dot (filled on even levels,
/// a ring on odd ones) in `rect`.
pub fn paint(painter: &Painter, rect: Rect, kind: Kind) {
    #[cfg(test)]
    PAINTED.with(|p| p.borrow_mut().push((rect, kind)));
    let p = theme::palette();
    match kind {
        Kind::Box { done } => {
            painter.rect(rect, CornerRadius::same(2), p.widget_bg, Stroke::new(1.0, p.faint), StrokeKind::Inside);
            if done {
                let r = rect.shrink(rect.width() * 0.22);
                let tick = [Pos2::new(r.left(), r.center().y), Pos2::new(r.center().x - r.width() * 0.1, r.bottom()), Pos2::new(r.right(), r.top())];
                painter.add(egui::Shape::line(tick.to_vec(), Stroke::new(1.6, p.accent)));
            }
        }
        Kind::Bullet { level } => {
            let radius = rect.width() * DOT_RADIUS;
            if level % 2 == 0 {
                painter.circle_filled(rect.center(), radius, p.text);
            } else {
                painter.circle_stroke(rect.center(), radius - 0.5, Stroke::new(1.0, p.text));
            }
        }
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

/// Paint the box or dot of every gap of `galley` (at `pos`) that is on the screen (the clip rect of
/// `painter`) and say where each box is: its rect and the byte of its check (the `x` or space
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
        if matches!(gap.kind, Kind::Box { .. }) {
            // the marker ends in `x]` / ` ]`
            out.push((rect, gap.marker.end - 2));
        }
        paint(painter, rect, gap.kind);
    }
    out
}

/// The byte of the check of the task whose box has `rect` under the point `p`, if any.
pub fn check_under(boxes: &[(Rect, usize)], p: Pos2) -> Option<usize> {
    boxes.iter().find(|(r, _)| r.expand(2.0).contains(p)).map(|(_, c)| *c)
}

/// The marker of a list line (its bytes) that the preview and the hits hide: a bullet task loses its
/// bullet too, a numbered one keeps its number; a plain bullet is `- ` (a rule, `**bold**` or
/// `*italic*` is not a list item, a numbered item keeps its number).
pub fn marker_range(line: &str) -> Option<Range<usize>> {
    if let Some((check, _, _, indent)) = markdown::parse_task_line(line) {
        let numbered = line[indent..].starts_with(|c: char| c.is_ascii_digit());
        return Some(if numbered { check - 1..check + 2 } else { indent..check + 2 });
    }
    let p = crate::lists::list_prefix(line)?;
    (matches!(p.marker, crate::lists::Marker::Bullet(_)) && line[p.indent + 1..].starts_with(' ')).then_some(p.indent..p.indent + 2)
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
        assert_eq!(marker_range("- plain"), Some(0..2), "a plain bullet is its dash and the space");
        assert_eq!(marker_range("\t+ plain"), Some(1..3));
        for not in ["---", "- - -", "***", "**bold** x", "*italic* x", "1. first", "text - not", "-no space", "-"] {
            assert_eq!(marker_range(not), None, "{not}");
        }
    }
}
