//! A note, a drawing or a folder dragged from the file list onto the board (BOARD-035 to
//! BOARD-037), with real pointer events: the drag starts on a row of the file list.

use super::*;
use crate::panels::TreeDrag;
use eframe::egui::DragAndDrop;

const FILES: [(&str, &str); 12] = [
    ("Board.md", FLOW),
    ("Alpha.md", "Alpha body #todo\n"),
    ("Beta.md", "---\nstatus: doing\nmodified: 2020-01-01T10:00:00+00:00\n---\nBeta body\n"),
    ("Gamma.md", "---\nstatus: doing\n---\nGamma body\n"),
    ("Pinned One.md", "---\nstatus: doing\n---\nPinned #todo\n"),
    ("Free.md", "---\nmodified: 2020-02-02T10:00:00+00:00\n---\nFree body\n"),
    ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE),
    ("Other.excalidraw", crate::drawing::SAMPLE_SCENE),
    ("Proj/B note.md", "B body\n"),
    ("Proj/A note.md", "A body\n"),
    ("Proj/Sub/C note.md", "C body\n"),
    ("Proj/Done already.md", "Already #todo\n"),
];

const MORE: [(&str, &str); 2] = [("Plain Folder/Inner.md", "Inner body\n"), ("My Folder (x)/Z note.md", "Z body\n")];

fn tree_window(tag: &str) -> W {
    let files: Vec<(&str, &str)> = FILES.iter().chain(MORE.iter()).copied().collect();
    let mut w = W::new(tag, &files);
    w.hl.set_size(vec2(2200.0, 900.0));
    w.settle();
    w
}

impl W {
    /// The row of the file list (left of the main area) that says `text`.
    fn row(&self, text: &str) -> Pos2 {
        let found = self.hl.text.iter().find(|(t, p)| t == text && p.x < MAIN_LEFT);
        let (_, p) = found.unwrap_or_else(|| panic!("no row {text:?}; rows: {:?}", self.hl.text.iter().filter(|(_, p)| p.x < MAIN_LEFT).map(|(t, _)| t.as_str()).collect::<Vec<_>>()));
        *p + vec2(6.0, 6.0)
    }

    /// Press on the row `from` of the file list and drag to `to`, staying there with the button down.
    fn tree_drag(&mut self, from: &str, to: Pos2) {
        let p = self.row(from);
        self.hl.press_and_move(&mut self.app, p, &Headless::steps(p, to, 10));
    }

    fn tree_drag_drop(&mut self, from: &str, to: Pos2) {
        self.tree_drag(from, to);
        self.drop_at(to);
    }

    fn tree_payload(&self) -> bool {
        DragAndDrop::has_payload_of_type::<TreeDrag>(self.hl.ctx())
    }

    /// An empty place right of the last lane.
    fn empty_board(&self) -> Pos2 {
        self.lane_top("Manual") + vec2(330.0, 200.0)
    }
}

// spec: BOARD-035
#[test]
fn board_035_a_note_dropped_on_a_lane_with_a_property_sets_it_and_not_modified() {
    let mut w = tree_window("tree-note-property");
    let to = w.lane_top("Done");
    w.tree_drag("Beta", to);
    assert!(w.tree_payload(), "the drag started in the file list");
    assert!(w.painted("Set status: done in “Beta”"), "the hint says what the drop does: {:?}", w.all_text());
    assert_eq!(w.app.boards.view.over, None, "no card is dragged");
    assert!(w.file("Beta.md").contains("status: doing"), "nothing is written before the drop");
    w.drop_at(to);
    let beta = w.file("Beta.md");
    assert!(beta.contains("status: done") && !beta.contains("status: doing"), "{beta}");
    assert!(beta.contains("modified: 2020-01-01T10:00:00+00:00"), "the drop is no change of the note: {beta}");
    assert_eq!(w.lane("Done"), strs(&["Beta"]));
    assert_eq!(w.lane("Doing"), strs(&["Gamma"]), "a note already on the board moves, it is not shown twice");
    assert_eq!(w.board_text(), FLOW, "a lane with a property pins nothing");
    assert!(!w.tree_payload());
    assert!(w.dir.join("Beta.md").exists(), "the file list did not move the note");
}

// spec: BOARD-035
#[test]
fn board_035_a_note_dropped_on_a_manual_lane_is_pinned_where_it_is_dropped() {
    let mut w = tree_window("tree-note-pin");
    let before = w.file("Free.md");
    let to = w.at("Pinned One");
    w.tree_drag("Free", to);
    assert!(w.painted("Pin “Free” in Manual"), "{:?}", w.all_text());
    w.drop_at(to);
    assert!(w.board_text().contains("## Manual\n- [[Free]]\n- [[Pinned One]]\n"), "{}", w.board_text());
    assert_eq!(w.lane("Manual"), strs(&["Free", "Pinned One", "Sketch"]));
    assert_eq!(w.file("Free.md"), before, "the note is not changed");
    // a lane with a query and no property takes the pin on top as well
    let to = w.lane_top("Todo");
    w.tree_drag_drop("Gamma", to);
    assert_eq!(w.lane("Todo")[0], "Gamma", "{:?}", w.lanes());
}

// spec: BOARD-035
#[test]
fn board_035_a_pinned_note_dropped_on_another_lane_is_unpinned_in_its_old_one() {
    let mut w = tree_window("tree-note-move");
    let to = w.lane_top("Done");
    w.tree_drag_drop("Pinned One", to);
    assert!(w.file("Pinned One.md").contains("status: done"));
    assert!(!w.board_text().contains("[[Pinned One]]"), "{}", w.board_text());
    assert!(w.lane("Todo").contains(&"Pinned One".to_string()), "its #todo shows it in the first lane that matches: {:?}", w.lanes());
    assert_eq!(w.lane("Manual"), strs(&["Sketch"]));
}

// spec: BOARD-035
#[test]
fn board_035_a_note_dropped_where_it_already_is_does_nothing_and_says_so() {
    let mut w = tree_window("tree-note-here");
    let to = w.lane_top("Manual") + vec2(0.0, 30.0);
    w.tree_drag("Pinned One", to);
    assert!(w.painted("already here"), "{:?}", w.all_text());
    w.drop_at(to);
    assert_eq!(w.board_text(), FLOW);
}

// spec: BOARD-035
#[test]
fn board_035_a_drawing_is_pinned_and_its_lane_property_is_not_written() {
    let mut w = tree_window("tree-drawing");
    let to = w.lane_top("Doing");
    let before = w.file("Other.excalidraw");
    w.tree_drag("Other", to);
    assert!(w.painted("Pin “Other” in Doing (a drawing has no properties)"), "{:?}", w.all_text());
    w.drop_at(to);
    assert!(w.board_text().contains("## Doing\nquery: status:doing\n- [[Other.excalidraw]]\n"), "{}", w.board_text());
    assert_eq!(w.file("Other.excalidraw"), before);
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("a drawing and has no properties")), "the user is told");
    assert_eq!(w.lane("Doing")[0], "Other");
    // on a lane without a property nothing needs saying
    let to = w.lane_top("Manual");
    w.tree_drag("Other", to);
    assert!(w.painted("Pin “Other” in Manual") && !w.painted("(a drawing has no properties)"), "{:?}", w.all_text());
}

// spec: BOARD-035
#[test]
fn board_035_the_note_of_the_board_and_the_empty_board_are_refused_with_a_hint() {
    let mut w = tree_window("tree-refused");
    let to = w.lane_top("Done");
    w.tree_drag("Board", to);
    assert!(w.painted("is this board: drop another note"), "{:?}", w.all_text());
    w.drop_at(to);
    assert_eq!(w.board_text(), FLOW);
    // a note on the empty board has no lane to go to
    let at = w.empty_board();
    w.tree_drag("Free", at);
    assert!(w.painted("Drop “Free” on a lane"), "{:?}", w.all_text());
    w.drop_at(at);
    assert_eq!(w.board_text(), FLOW);
}

// spec: BOARD-035
#[test]
fn board_035_the_drag_marks_the_lane_and_esc_cancels_it() {
    let mut w = tree_window("tree-esc");
    let to = w.at("Beta");
    w.tree_drag("Free", to);
    assert!(w.painted("Pin “Free” in Doing") || w.painted("Set status"), "{:?}", w.all_text());
    assert!(!w.painted("Move “Free”"), "the file list's hint is not shown over the board");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!w.tree_payload(), "Esc clears the drag");
    w.drop_at(to);
    assert_eq!(w.board_text(), FLOW);
    assert!(w.dir.join("Free.md").exists());
}

// spec: BOARD-035
#[test]
fn board_035_a_drop_on_a_folder_of_the_file_list_still_moves_the_file() {
    let mut w = tree_window("tree-move");
    let to = w.row("Plain Folder");
    w.tree_drag("Free", to);
    assert!(w.painted("Move “Free”") || w.hl.text.iter().any(|(t, _)| t.contains("Move “Free” into")), "the file list's own hint shows");
    w.drop_at(to);
    assert!(w.dir.join("Plain Folder/Free.md").exists() && !w.dir.join("Free.md").exists());
    assert_eq!(w.board_text(), FLOW);
}

// spec: BOARD-036
#[test]
fn board_036_a_folder_dropped_on_a_lane_pins_its_notes_in_path_order_and_writes_nothing_into_them() {
    let mut w = tree_window("tree-folder-pin");
    // the note of the board is open, so the rewrite is a step of its undo history
    w.app.open(Path::new("Board.md"), true);
    w.app.show_view(View::Boards, false);
    w.settle();
    let notes = ["Proj/A note.md", "Proj/B note.md", "Proj/Sub/C note.md", "Proj/Done already.md"].map(|n| (n, w.file(n)));
    let to = w.at("Pinned One");
    w.tree_drag("Proj", to);
    assert!(w.painted("Pin 3 notes of “Proj” in Manual"), "the notes on the board are not counted: {:?}", w.all_text());
    w.drop_at(to);
    assert!(w.board_text().contains("## Manual\n- [[A note]]\n- [[B note]]\n- [[C note]]\n- [[Pinned One]]\n"), "{}", w.board_text());
    assert_eq!(w.lane("Manual"), strs(&["A note", "B note", "C note", "Pinned One", "Sketch"]));
    assert!(!w.board_text().contains("Done already"), "a note that is a card already is skipped");
    for (n, text) in notes {
        assert_eq!(w.file(n), text, "{n} is not changed");
    }
    // one undo step for the board file
    let mut t = w.board_text();
    assert!(w.app.undo.of(Path::new("Board.md")).undo(&mut t).is_some());
    assert!(!t.contains("[[A note]]") && t.contains("[[Pinned One]]"), "the drop was one step: {t}");
}

// spec: BOARD-036
#[test]
fn board_036_a_folder_with_nothing_left_to_pin_is_refused() {
    let mut w = tree_window("tree-folder-none");
    let to = w.lane_top("Manual");
    w.tree_drag_drop("Plain Folder", to);
    assert!(w.board_text().contains("[[Inner]]"));
    let after = w.board_text();
    let to = w.lane_top("Done");
    w.tree_drag("Plain Folder", to);
    assert!(w.painted("has no note or drawing that is not on the board"), "{:?}", w.all_text());
    w.drop_at(to);
    assert_eq!(w.board_text(), after);
}

// spec: BOARD-036
#[test]
fn board_036_a_folder_dropped_on_a_lane_with_a_property_pins_and_leaves_the_lane_query_alone() {
    let mut w = tree_window("tree-folder-prop");
    let to = w.lane_top("Done");
    w.tree_drag_drop("Plain Folder", to);
    assert!(w.board_text().contains("## Done\nquery: status:done\n- [[Inner]]\n"), "{}", w.board_text());
    assert_eq!(w.lane("Done"), strs(&["Inner"]));
    assert_eq!(w.file("Plain Folder/Inner.md"), "Inner body\n");
}

// spec: BOARD-037
#[test]
fn board_037_a_folder_dropped_on_the_empty_board_makes_a_lane_that_follows_it() {
    let mut w = tree_window("tree-folder-lane");
    let at = w.empty_board();
    w.tree_drag("Proj", at);
    assert!(w.painted("New lane “Proj” (path:Proj/)"), "{:?}", w.all_text());
    w.drop_at(at);
    assert!(w.board_text().ends_with("## Proj\nquery: path:Proj/\n"), "{}", w.board_text());
    assert_eq!(w.lane("Proj"), strs(&["A note", "B note", "C note"]), "{:?}", w.lanes());
    // it stays in sync: a note that comes into the folder shows
    crate::fsutil::write_atomic(&w.dir.join("Proj/D note.md"), b"D\n").unwrap();
    w.app.vault.as_mut().unwrap().rescan();
    w.settle();
    assert!(w.lane("Proj").contains(&"D note".to_string()), "{:?}", w.lanes());
}

// spec: BOARD-037
#[test]
fn board_037_a_folder_with_spaces_and_parentheses_is_a_quoted_path_and_the_add_lane_button_takes_it() {
    let mut w = tree_window("tree-folder-quote");
    let at = w.at("+ Add lane");
    w.tree_drag("My Folder (x)", at);
    assert!(w.painted("New lane “My Folder (x)” (path:\"My Folder (x)/\")"), "{:?}", w.all_text());
    w.drop_at(at);
    assert!(w.board_text().ends_with("## My Folder (x)\nquery: path:\"My Folder (x)/\"\n"), "{}", w.board_text());
    assert_eq!(w.lane("My Folder (x)"), strs(&["Z note"]), "the query is valid and finds the notes: {:?}", w.lanes());
}
