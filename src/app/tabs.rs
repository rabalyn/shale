//! The open tabs of the window.

use super::{NoteEditor, Tab, TabMarks, View};
use eframe::egui;

/// The tabs of the window: the open ones in order, the active one, and where the tab bar drew them
/// last frame (for tests).
pub struct TabBar {
    pub tabs: Vec<Tab>,
    pub active: usize,
    pub(crate) next_tab_id: u64,
    /// where the tabs of the tab bar are, and their close marks (for tests)
    pub(crate) tab_rects: Vec<(egui::Rect, egui::Rect)>,
    /// what the tab bar showed last frame besides the tabs (for tests)
    pub(crate) tab_marks: TabMarks,
}

impl TabBar {
    pub fn new() -> Self {
        Self { tabs: vec![], active: 0, next_tab_id: 1, tab_rects: vec![], tab_marks: TabMarks::default() }
    }

    /// Open a tab on `view` and make it the active one.
    pub fn new_tab(&mut self, view: View) {
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        self.tabs.push(Tab { id, view, back: vec![], fwd: vec![], reading: false, reader_line: None, pinned: false, editor: NoteEditor::new(egui::Id::unique(("note-editor", id))), title_buf: String::new(), title_for: None });
        self.active = self.tabs.len() - 1;
    }
}
