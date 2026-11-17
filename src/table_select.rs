//! A rectangular selection of table cells in the grid (EDIT-030, raba/shale#243): the cell being
//! edited (the anchor) and the corner that Shift+arrow keys move.

use eframe::egui::Key;

/// The block of cells from `anchor` to `corner`, as (row, column); row 0 is the header.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Block {
    pub anchor: (usize, usize),
    pub corner: (usize, usize),
}

impl Block {
    /// The block of one cell.
    pub fn single(at: (usize, usize)) -> Block {
        Block { anchor: at, corner: at }
    }

    pub fn is_single(&self) -> bool {
        self.anchor == self.corner
    }

    /// The cell at the top left of the block.
    pub fn top_left(&self) -> (usize, usize) {
        (self.anchor.0.min(self.corner.0), self.anchor.1.min(self.corner.1))
    }

    /// The cell at the bottom right of the block.
    pub fn bottom_right(&self) -> (usize, usize) {
        (self.anchor.0.max(self.corner.0), self.anchor.1.max(self.corner.1))
    }

    /// Every cell of the block, row by row.
    pub fn cells(&self) -> Vec<(usize, usize)> {
        let ((r0, c0), (r1, c1)) = (self.top_left(), self.bottom_right());
        (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).collect()
    }

    /// The block with its corner moved one cell by an arrow key; `None` at the edge of the table
    /// (`nrows` and `ncols` count the header row and all columns) or for another key.
    pub fn moved(self, key: Key, nrows: usize, ncols: usize) -> Option<Block> {
        let (r, c) = self.corner;
        let corner = match key {
            Key::ArrowLeft => (r, c.checked_sub(1)?),
            Key::ArrowRight => (r, (c + 1 < ncols).then_some(c + 1)?),
            Key::ArrowUp => (r.checked_sub(1)?, c),
            Key::ArrowDown => ((r + 1 < nrows).then_some(r + 1)?, c),
            _ => return None,
        };
        Some(Block { corner, ..self })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: EDIT-030
    #[test]
    fn the_corner_moves_one_cell_and_stops_at_the_edges_of_the_table() {
        let b = Block::single((1, 1));
        let right = b.moved(Key::ArrowRight, 3, 3).unwrap();
        assert_eq!(right.corner, (1, 2));
        assert_eq!(right.anchor, (1, 1), "the anchor stays");
        assert!(right.moved(Key::ArrowRight, 3, 3).is_none(), "no column right of the last");
        let block = right.moved(Key::ArrowUp, 3, 3).unwrap();
        assert_eq!((block.top_left(), block.bottom_right()), ((0, 1), (1, 2)));
        assert!(block.moved(Key::ArrowUp, 3, 3).is_none(), "no row above the header");
        assert!(Block::single((2, 0)).moved(Key::ArrowDown, 3, 3).is_none());
        assert!(Block::single((0, 0)).moved(Key::ArrowLeft, 3, 3).is_none());
        assert!(b.moved(Key::Tab, 3, 3).is_none());
    }

    // spec: EDIT-030
    #[test]
    fn a_block_goes_back_to_one_cell_and_lists_its_cells_row_by_row() {
        let b = Block::single((1, 1)).moved(Key::ArrowLeft, 3, 3).unwrap().moved(Key::ArrowDown, 3, 3).unwrap();
        assert!(!b.is_single());
        assert_eq!(b.top_left(), (1, 0), "the top left is not the anchor or the corner when they are crossed");
        assert_eq!(b.cells(), [(1, 0), (1, 1), (2, 0), (2, 1)]);
        let back = b.moved(Key::ArrowRight, 3, 3).unwrap().moved(Key::ArrowUp, 3, 3).unwrap();
        assert!(back.is_single());
        assert_eq!(back.cells(), [(1, 1)]);
    }
}
