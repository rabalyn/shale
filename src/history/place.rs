//! Where the history is shown (HIST-001, HIST-002): in a column in the margin of the note, or as a
//! panel of the right sidebar. Pure, so the thresholds can be tested.

/// The width of the column in the margin (HIST-001).
pub const COLUMN_W: f32 = 236.0;
/// The width of the text column of a note with a readable line length.
const TEXT_W: f32 = 760.0;
/// The note area must hold the text, the column and a little air.
pub const MIN_AREA_W: f32 = TEXT_W + COLUMN_W + 44.0;

/// Where the history of the focused note is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    /// A column at the right edge of the note area.
    Margin,
    /// The panel of the right sidebar.
    Sidebar,
}

/// The facts the choice depends on.
#[derive(Clone, Copy, Debug)]
pub struct Facts {
    /// How many panes are open side by side.
    pub panes: usize,
    /// The focused pane shows a note (not a drawing, a list or the calendar).
    pub note: bool,
    /// "Readable line length" is on: the text has a column of its own with margins.
    pub readable: bool,
    /// The width of the note area.
    pub area_w: f32,
    /// `history.show_panel`.
    pub wanted: bool,
}

/// Margin or sidebar (HIST-001).
pub fn place(f: Facts) -> Place {
    if f.wanted && f.panes == 1 && f.note && f.readable && f.area_w >= MIN_AREA_W {
        Place::Margin
    } else {
        Place::Sidebar
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDE: Facts = Facts { panes: 1, note: true, readable: true, area_w: 1200.0, wanted: true };

    // spec: HIST-001
    #[test]
    fn hist_001_the_margin_is_for_one_note_with_room_and_everything_else_is_the_sidebar() {
        assert_eq!(place(WIDE), Place::Margin);
        assert_eq!(place(Facts { area_w: MIN_AREA_W, ..WIDE }), Place::Margin, "exactly enough");
        assert_eq!(place(Facts { area_w: MIN_AREA_W - 1.0, ..WIDE }), Place::Sidebar, "a narrow window");
        assert_eq!(place(Facts { panes: 2, ..WIDE }), Place::Sidebar, "several notes side by side");
        assert_eq!(place(Facts { note: false, ..WIDE }), Place::Sidebar, "a drawing or a list has no margin");
        assert_eq!(place(Facts { readable: false, ..WIDE }), Place::Sidebar, "text that uses the whole width has no margin");
        assert_eq!(place(Facts { wanted: false, ..WIDE }), Place::Sidebar, "hidden by the person");
    }
}
