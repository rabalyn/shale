//! The text cursor of the note editor (EDIT-039…EDIT-041): its shape, the rectangle a shape
//! paints for the character at the cursor, and the blink phase.

use crate::theme;
use eframe::egui::{Painter, Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};

/// How the cursor looks. Written in the settings file as `bar`, `bar_after`, `block`, `underline`.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    /// A thin bar at the left edge of the character at the cursor.
    #[default]
    Bar,
    /// A thin bar at the right edge of the character at the cursor.
    BarAfter,
    /// A translucent block over the character at the cursor.
    Block,
    /// A line under the character at the cursor.
    Underline,
}

/// The thickness of a bar or an underline in points.
const THICKNESS: f32 = 2.0;

impl CursorShape {
    pub const ALL: [CursorShape; 4] = [CursorShape::Bar, CursorShape::BarAfter, CursorShape::Block, CursorShape::Underline];

    pub fn label(self) -> &'static str {
        match self {
            CursorShape::Bar => "Bar before the character",
            CursorShape::BarAfter => "Bar after the character",
            CursorShape::Block => "Block",
            CursorShape::Underline => "Underline",
        }
    }

    /// The rectangle this shape paints for the character whose box is `ch`.
    pub fn rect(self, ch: Rect) -> Rect {
        match self {
            CursorShape::Bar => Rect::from_min_size(ch.min, Vec2::new(THICKNESS, ch.height())),
            CursorShape::BarAfter => Rect::from_min_size(Pos2::new(ch.max.x - THICKNESS, ch.min.y), Vec2::new(THICKNESS, ch.height())),
            CursorShape::Block => ch,
            CursorShape::Underline => Rect::from_min_size(Pos2::new(ch.min.x, ch.max.y - THICKNESS), Vec2::new(ch.width(), THICKNESS)),
        }
    }

    /// Paint the cursor over the character whose box is `ch`.
    pub fn paint(self, painter: &Painter, ch: Rect) {
        let colour = if self == CursorShape::Block { theme::ACCENT.gamma_multiply(0.45) } else { theme::ACCENT };
        painter.rect_filled(self.rect(ch), 1.0, colour);
    }
}

/// The cursor settings the editor draws with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorStyle {
    /// typing, vim insert and visual mode
    pub shape: CursorShape,
    /// vim normal mode
    pub normal_shape: CursorShape,
    pub blink: bool,
}

impl Default for CursorStyle {
    fn default() -> Self {
        CursorStyle { shape: CursorShape::Bar, normal_shape: CursorShape::Block, blink: true }
    }
}

/// Whether a blinking cursor is shown `since` seconds after it last moved (shown at once after
/// a move), and how many seconds until that changes. A steady cursor is always shown.
pub fn blink_phase(blink: bool, since: f64, on: f32, off: f32) -> (bool, Option<f64>) {
    let (on, off) = (f64::from(on), f64::from(off));
    if !blink || on <= 0.0 || off <= 0.0 {
        return (true, None);
    }
    let t = since.max(0.0) % (on + off);
    if t < on {
        (true, Some(on - t))
    } else {
        (false, Some(on + off - t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(8.0, 16.0))
    }

    // spec: EDIT-039
    #[test]
    fn edit_039_each_shape_paints_its_part_of_the_character_box() {
        assert_eq!(CursorShape::Bar.rect(ch()), Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(2.0, 16.0)));
        assert_eq!(CursorShape::BarAfter.rect(ch()), Rect::from_min_size(Pos2::new(16.0, 20.0), Vec2::new(2.0, 16.0)));
        assert_eq!(CursorShape::Block.rect(ch()), ch());
        assert_eq!(CursorShape::Underline.rect(ch()), Rect::from_min_size(Pos2::new(10.0, 34.0), Vec2::new(8.0, 2.0)));
    }

    // spec: EDIT-039
    #[test]
    fn edit_039_the_shapes_are_written_as_lowercase_words_and_default_to_bar_and_block() {
        let words: Vec<String> = CursorShape::ALL.iter().map(|s| serde_json::to_string(s).unwrap()).collect();
        assert_eq!(words, ["\"bar\"", "\"bar_after\"", "\"block\"", "\"underline\""]);
        let d = CursorStyle::default();
        assert_eq!((d.shape, d.normal_shape, d.blink), (CursorShape::Bar, CursorShape::Block, true));
    }

    // spec: EDIT-041
    #[test]
    fn edit_041_a_blinking_cursor_is_on_after_a_move_then_off_then_on() {
        assert_eq!(blink_phase(true, 0.0, 0.5, 0.5), (true, Some(0.5)));
        assert_eq!(blink_phase(true, 0.25, 0.5, 0.5), (true, Some(0.25)));
        assert_eq!(blink_phase(true, 0.75, 0.5, 0.5), (false, Some(0.25)));
        assert_eq!(blink_phase(true, 1.25, 0.5, 0.5), (true, Some(0.25)));
    }

    // spec: EDIT-041
    #[test]
    fn edit_041_a_steady_cursor_is_always_shown() {
        assert_eq!(blink_phase(false, 0.75, 0.5, 0.5), (true, None));
    }
}
