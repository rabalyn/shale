//! Setting and removing blockers from the board view (BOARD-040 to BOARD-046), driven with real pointer
//! and key events (DEV-041).

use super::{strs, W};
use crate::ui_test_support::Headless;
use eframe::egui::{Key, Modifiers, PointerButton, Pos2};
use std::path::Path;

const PLAN: &str = "---\ntype: board\n---\n# Plan\n\n## Todo\nquery: #t\nsort: dependencies\n\n## Later\nquery: #later\n";

const FILES: [(&str, &str); 8] = [
    ("Board.md", PLAN),
    ("A.md", "---\nmodified: 2020-01-01T10:00:00+00:00\n---\n#t A body\n"),
    ("B.md", "---\nblocked-by: [[A]]\nmodified: 2020-01-01T10:00:00+00:00\n---\n#t B body\n"),
    ("C.md", "#t C body\n"),
    ("D.md", "---\ndue: 2030-01-01\nblocked-by: [[A]], [[B]]\n---\n#t D body\n"),
    ("Odd #1.md", "#later odd\n"),
    ("Z.md", "#later Z\n"),
    ("Unrelated.md", "Nothing here\n"),
];

fn window(tag: &str) -> W {
    W::new(tag, &FILES)
}

impl W {
    /// The ✕ of the lines of the peek, top to bottom, left to right.
    fn crosses(&self) -> Vec<Pos2> {
        let mut v: Vec<Pos2> = self.main().filter(|(t, _)| t == "✕").map(|(_, p)| *p + eframe::egui::vec2(6.0, 6.0)).collect();
        v.sort_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)));
        v
    }

    /// A note on disk without the `created:` line that a save stamps on a note that has none (VAULT-045).
    fn plain(&self, name: &str) -> String {
        self.file(name).lines().filter(|l| !l.starts_with("created:")).map(|l| format!("{l}\n")).collect()
    }

    fn toasted(&self, part: &str) -> bool {
        self.app.toasts.iter().any(|(t, _)| t.contains(part))
    }

    /// Open the field of "+ Add blocker" (or the other one) of the peek of the card `card`, and type `typed`.
    fn add_field(&mut self, card: &str, button: &str, typed: &str) {
        self.click(card);
        self.click(button);
        assert!(self.app.boards.peek.as_ref().is_some_and(|p| p.picker.is_some()), "the field is open");
        self.type_text(typed);
    }

    fn picker_blocked(&self) -> Option<bool> {
        self.app.boards.peek.as_ref().and_then(|p| p.picker.as_ref()).map(|p| p.blocked)
    }
}

// spec: BOARD-040, BOARD-023
#[test]
fn board_040_a_blocker_written_as_a_markdown_link_blocks_like_a_wiki_link() {
    let mut w = W::new("blk-read", &[("Board.md", PLAN), ("Odd #1.md", "#t odd\n"), ("Sub/Dir, two.md", "#t two\n"), ("X.md", "---\nblocked-by: [Odd #1](<Odd%20%231.md>), [two](<Sub/Dir, two.md>)\n---\n#t x\n")]);
    assert_eq!(w.dep("X").blockers, [Path::new("Odd #1.md"), Path::new("Sub/Dir, two.md")], "both forms find the notes, also with a # or a comma in the name");
    assert!(w.dep("X").blocked());
    assert_eq!(w.dep("Odd #1").dependants, [Path::new("X.md")]);
    w.set_note("Odd #1.md", "---\nstatus: done\n---\n#t odd\n");
    assert_eq!(w.dep("X").waiting.len(), 1, "a done blocker no longer holds it");
}

// spec: BOARD-044, BOARD-042
#[test]
fn board_044_the_peek_lists_what_the_card_waits_for_and_what_waits_for_it_and_a_cross_takes_one_away() {
    let mut w = window("blk-lists");
    w.click("D");
    assert!(w.app.boards.peek.is_some());
    assert!(w.painted("Blocked by") && w.painted("Blocks"), "{:?}", w.all_text());
    assert!(w.painted("+ Add blocker") && w.painted("+ Add blocked card"));
    assert_eq!(w.crosses().len(), 2, "A and B: {:?}", w.all_text());
    let a = w.crosses()[0];
    w.click_at(a);
    assert_eq!(w.plain("D.md"), "---\ndue: 2030-01-01\nblocked-by: [[B]]\n---\n#t D body\n", "A is gone, B and the other lines stay as they were");
    assert_eq!(w.dep("D").blockers, [Path::new("B.md")], "the card knows at once");
    // the last one takes the line with it
    let b = w.crosses()[0];
    w.click_at(b);
    assert_eq!(w.plain("D.md"), "---\ndue: 2030-01-01\n---\n#t D body\n");
    assert!(w.dep("D").blockers.is_empty() && !w.dep("D").blocked());
    // the line of the cards that wait for A: its cross takes A out of their `blocked-by:`
    w.click("A");
    assert_eq!(w.crosses().len(), 1, "B waits for A, D does not any more: {:?}", w.all_text());
    let cross = w.crosses()[0];
    w.click_at(cross);
    assert_eq!(w.plain("B.md"), "---\nmodified: 2020-01-01T10:00:00+00:00\n---\n#t B body\n");
}

// spec: BOARD-041, BOARD-044
#[test]
fn board_041_a_blocker_found_by_typing_is_added_the_way_a_drop_writes_and_is_one_undo_step() {
    let mut w = window("blk-add");
    w.add_field("B", "+ Add blocker", "c");
    assert_eq!(w.picker_blocked(), Some(false));
    assert!(w.painted("C"), "the list offers the note");
    w.key(Key::Enter, Modifiers::NONE, "");
    assert_eq!(w.plain("B.md"), "---\nblocked-by: [[A]], [[C]]\nmodified: 2020-01-01T10:00:00+00:00\n---\n#t B body\n", "the other blocker and `modified:` stay");
    assert!(w.app.boards.peek.as_ref().is_some_and(|p| p.picker.is_none()), "the field closes");
    let doc = &w.app.notes[Path::new("B.md")];
    assert!(!doc.dirty, "saved the checked way");
    let mut t = doc.text.clone();
    assert!(w.app.undo.of(Path::new("B.md")).undo(&mut t).is_some());
    assert!(t.contains("blocked-by: [[A]]\n") && !t.contains("[[C]]"), "one undo step of the note: {t:?}");
    assert_eq!(w.dep("B").blockers.len(), 2);
}

// spec: BOARD-041, BOARD-044
#[test]
fn board_041_a_card_that_should_wait_for_this_one_is_added_from_its_line_and_any_note_is_found_by_the_worker() {
    let mut w = window("blk-add-blocked");
    w.add_field("C", "+ Add blocked card", "unrel");
    assert_eq!(w.picker_blocked(), Some(true));
    assert!(w.painted("Unrelated"), "a note that is no card of the board is found too: {:?}", w.all_text());
    w.click("Unrelated");
    assert_eq!(w.plain("Unrelated.md"), "---\nblocked-by: [[C]]\n---\nNothing here\n", "the note gets the line");
    assert_eq!(w.plain("C.md"), "#t C body\n", "the blocker is not changed");
}

// spec: BOARD-044
#[test]
fn board_044_the_field_offers_the_cards_first_leaves_out_what_is_there_and_esc_closes_only_the_field() {
    let mut w = window("blk-field");
    w.add_field("D", "+ Add blocker", "");
    // A and B are blockers already, D is the card itself: C is the one card left in its lane
    assert!(w.painted("C") && w.painted("Odd #1"), "{:?}", w.all_text());
    w.key(Key::Escape, Modifiers::NONE, "");
    let peek = w.app.boards.peek.as_ref();
    assert!(peek.is_some_and(|p| p.picker.is_none()), "Esc closed the field");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(w.app.boards.peek.is_none(), "the next Esc closes the peek");
    assert_eq!(w.plain("D.md"), FILES[4].1, "nothing was written");
}

// spec: BOARD-041, BOARD-042, BOARD-004
#[test]
fn board_041_a_name_that_wiki_links_cannot_hold_is_written_as_a_markdown_link_that_blocks() {
    let mut w = window("blk-md");
    w.app.block(Path::new("Odd #1.md"), Path::new("C.md"));
    w.settle();
    assert_eq!(w.plain("C.md"), "---\nblocked-by: [Odd #1](<Odd %231.md>)\n---\n#t C body\n");
    assert_eq!(w.dep("C").blockers, [Path::new("Odd #1.md")], "it reads back as that note");
    assert!(w.dep("C").blocked());
    w.app.unblock(Path::new("Odd #1.md"), Path::new("C.md"));
    w.settle();
    assert_eq!(w.plain("C.md"), "---\n---\n#t C body\n", "the line is gone with its last blocker");
    assert!(w.dep("C").blockers.is_empty());
}

// spec: BOARD-043
#[test]
fn board_043_a_card_cannot_block_itself_and_a_blocker_is_not_added_twice() {
    let mut w = window("blk-rules");
    w.app.block(Path::new("A.md"), Path::new("A.md"));
    assert!(w.toasted("cannot block itself"), "{:?}", w.app.toasts);
    assert_eq!(w.plain("A.md"), FILES[1].1);
    w.app.block(Path::new("A.md"), Path::new("B.md"));
    assert!(w.toasted("is blocked by “A” already"), "{:?}", w.app.toasts);
    assert_eq!(w.plain("B.md"), FILES[2].1);
    // a file that is no note neither blocks nor waits
    w.app.block(Path::new("Z.md"), Path::new("Missing.md"));
    assert!(w.toasted("must both be notes"));
}

// spec: BOARD-043, BOARD-028
#[test]
fn board_043_a_blocker_that_closes_a_cycle_is_added_with_a_hint_and_the_cards_are_circular() {
    let mut w = window("blk-cycle");
    assert!(!w.painted("circular"));
    // D waits for B, which waits for A: A waiting for D closes the circle
    w.app.block(Path::new("D.md"), Path::new("A.md"));
    w.settle();
    assert!(w.plain("A.md").contains("blocked-by: [[D]]"), "{}", w.plain("A.md"));
    assert!(w.toasted("a cycle"), "{:?}", w.app.toasts);
    assert!(w.painted("circular"), "{:?}", w.all_text());
}

// spec: BOARD-045
#[test]
fn board_045_the_handle_of_the_selected_card_dragged_onto_another_makes_it_wait_with_a_hint() {
    let mut w = window("blk-connect");
    assert!(w.app.boards.view.handle.is_none(), "no handle without a selected card");
    w.click("C");
    w.key(Key::Escape, Modifiers::NONE, "");
    let handle = w.app.boards.view.handle.expect("the selected card has a handle").center();
    let d = w.at("D");
    w.hl.press_and_move(&mut w.app, handle, &Headless::steps(handle, d, 8));
    assert!(w.app.boards.view.dragging());
    assert_eq!(w.app.boards.view.connect_over.as_deref(), Some(Path::new("D.md")));
    assert!(w.painted("C blocks D"), "the hint names both: {:?}", w.all_text());
    assert_eq!(w.plain("D.md"), FILES[4].1, "nothing is written before the drop");
    w.drop_at(d);
    assert!(!w.app.boards.view.dragging());
    assert_eq!(w.plain("D.md"), "---\ndue: 2030-01-01\nblocked-by: [[A]], [[B]], [[C]]\n---\n#t D body\n");
    assert_eq!(w.plain("C.md"), "#t C body\n", "the blocker is not changed");
    assert!(w.dep("D").blockers.contains(&Path::new("C.md").to_path_buf()), "arrows and badges follow at once");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("C.md")), "C stays selected");
    assert!(w.app.boards.view.arrows.len() == 1, "{:?}", w.app.boards.view.arrows);
}

// spec: BOARD-045
#[test]
fn board_045_esc_cancels_the_link_and_a_drop_on_itself_or_on_nothing_writes_nothing() {
    let mut w = window("blk-connect-cancel");
    w.click("C");
    w.key(Key::Escape, Modifiers::NONE, "");
    let handle = w.app.boards.view.handle.unwrap().center();
    let d = w.at("D");
    w.hl.press_and_move(&mut w.app, handle, &Headless::steps(handle, d, 8));
    assert!(w.app.boards.view.dragging());
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!w.app.boards.view.dragging() && w.app.boards.view.connect_over.is_none());
    w.drop_at(d);
    assert_eq!(w.plain("D.md"), FILES[4].1, "Esc cancelled it");
    // on itself: the hint says so
    let own = w.at("C");
    w.hl.press_and_move(&mut w.app, handle, &Headless::steps(handle, own, 8));
    assert!(w.painted("C cannot block itself"), "{:?}", w.all_text());
    w.drop_at(own);
    assert_eq!(w.plain("C.md"), "#t C body\n");
    // in the strip above the lanes
    let strip = w.at("+ New board");
    w.hl.press_and_move(&mut w.app, handle, &Headless::steps(handle, strip, 8));
    w.drop_at(strip);
    for (name, text) in FILES.iter().skip(1) {
        assert_eq!(&w.plain(name), text, "{name} is as it was");
    }
    assert!(w.app.boards.peek.is_none(), "a link drag is no click");
}

// spec: BOARD-046
#[test]
fn board_046_the_menu_of_a_card_removes_a_blocker_and_opens_the_fields() {
    let mut w = window("blk-menu");
    let b = w.at("B");
    w.hl.click(&mut w.app, b, PointerButton::Secondary);
    w.settle();
    for entry in ["Add blocker…", "Remove blocker", "Add blocked card…", "Remove blocked card"] {
        assert!(w.painted(entry), "{entry}: {:?}", w.all_text());
    }
    w.click("Remove blocker");
    w.click("A");
    assert_eq!(w.plain("B.md"), "---\nmodified: 2020-01-01T10:00:00+00:00\n---\n#t B body\n", "A left the line of B; `modified:` stays");
    // a card without blockers has no such entry
    let b = w.at("B");
    w.hl.click(&mut w.app, b, PointerButton::Secondary);
    w.settle();
    assert!(w.painted("Add blocker…") && !w.painted("Remove blocker"), "{:?}", w.all_text());
    w.click("Add blocker…");
    assert_eq!(w.picker_blocked(), Some(false), "the peek of the card opens with the field");
    assert_eq!(w.app.boards.peek.as_ref().map(|p| p.rel.clone()).as_deref(), Some(Path::new("B.md")));
    // (the peek covers the next lane: Esc closes the field and the peek)
    w.key(Key::Escape, Modifiers::NONE, "");
    w.key(Key::Escape, Modifiers::NONE, "");
    let z = w.at("Z");
    w.hl.click(&mut w.app, z, PointerButton::Secondary);
    w.settle();
    w.click("Add blocked card…");
    assert_eq!(w.picker_blocked(), Some(true));
    assert_eq!(w.app.boards.peek.as_ref().map(|p| p.rel.clone()).as_deref(), Some(Path::new("Z.md")));
    assert_eq!(w.lane("Later"), strs(&["Odd #1", "Z"]));
}
