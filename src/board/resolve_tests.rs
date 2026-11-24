//! The board as the exports read it (BOARD-030): lanes and cards resolved without the window.

use super::card::Card;
use super::resolve::resolve;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

fn vault(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
    let d = std::env::temp_dir().join(format!("shale-board-resolve-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (name, text) in files {
        std::fs::write(d.join(name), text).unwrap();
    }
    (Vault::open(&d).unwrap(), d)
}

fn titles<'a>(cards: impl IntoIterator<Item = &'a Card>) -> Vec<String> {
    cards.into_iter().map(|c| c.title.clone()).collect()
}

const NOTES: &[(&str, &str)] = &[("A.md", "---\nstatus: todo\n---\n"), ("B.md", "---\nstatus: doing\n---\n"), ("C.md", "---\nstatus: doing\npriority: high\n---\n"), ("D.md", "---\nstatus: done\n---\n")];

fn with_board(tag: &str, board: &str) -> (Vault, PathBuf) {
    let mut files = NOTES.to_vec();
    files.push(("Board.md", board));
    vault(tag, &files)
}

// spec: BOARD-030
#[test]
fn board_030_a_board_resolves_to_its_lanes_and_cards_in_the_order_of_the_view() {
    let (v, d) = with_board("order", "---\ntype: board\n---\n# Board\n\n## Later\nquery: status:todo OR status:done\nsort: title desc\n\n## Now\nquery: status:doing\n\n## Pinned\n- [[A]]\n");
    let b = resolve(&v, Path::new("Board.md")).unwrap();
    assert_eq!(b.title, "Board", "the name of the note");
    let names: Vec<&str> = b.lanes.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Later", "Now", "Pinned"]);
    // A is pinned: only in the lane that pins it; the sort of a lane is applied
    assert_eq!(titles(&b.lanes[0].cards), ["D"]);
    assert_eq!(titles(&b.lanes[1].cards), ["B", "C"]);
    assert_eq!(titles(&b.lanes[2].cards), ["A"]);
    let order: Vec<String> = b.chapters().iter().flat_map(|(_, c)| titles(c.iter().copied())).collect();
    assert_eq!(order, ["D", "B", "C", "A"], "lane order, then card order");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-030, BOARD-009
#[test]
fn board_030_a_card_of_several_lanes_is_one_chapter_and_a_lane_without_cards_has_none() {
    let (v, d) = with_board("every", "---\ntype: board\ncards: every-lane\n---\n## All\nquery: status:doing OR status:todo\n\n## Doing\nquery: status:doing\n\n## Nothing\nquery: status:nothing\n");
    let b = resolve(&v, Path::new("Board.md")).unwrap();
    assert_eq!(b.lanes[1].cards.len(), 2, "the view has the cards in both lanes");
    let chapters = b.chapters();
    assert_eq!(chapters.len(), 1, "the other lanes have nothing left or nothing at all");
    assert_eq!((chapters[0].0.name.as_str(), titles(chapters[0].1.iter().copied())), ("All", vec!["A".to_string(), "B".into(), "C".into()]));
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-030
#[test]
fn board_030_a_lane_whose_query_fails_has_the_reason_and_no_cards_and_a_blank_one_has_only_its_pins() {
    let (v, d) = with_board("invalid", "---\ntype: board\n---\n## Broken\nquery: (status:doing\n\n## Prefixed\nquery: m: x\n\n## Manual\n- [[D]]\n\n## Good\nquery: status:todo\n");
    let b = resolve(&v, Path::new("Board.md")).unwrap();
    assert!(b.lanes[0].error.is_some() && b.lanes[0].cards.is_empty());
    assert!(b.lanes[1].error.is_some() && b.lanes[1].cards.is_empty());
    assert_eq!((b.lanes[2].error.clone(), titles(&b.lanes[2].cards)), (None, vec!["D".to_string()]));
    assert_eq!(titles(&b.lanes[3].cards), ["A"]);
    let order: Vec<String> = b.chapters().iter().map(|(l, _)| l.name.clone()).collect();
    assert_eq!(order, ["Manual", "Good"]);
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-030
#[test]
fn board_030_only_a_board_note_resolves() {
    let (v, d) = with_board("not", "---\ntype: board\n---\n");
    assert!(resolve(&v, Path::new("A.md")).unwrap_err().contains("not a board"));
    assert!(resolve(&v, Path::new("Nothing.md")).unwrap_err().contains("not a note"));
    assert!(resolve(&v, Path::new("Board.md")).unwrap().lanes.is_empty());
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-026, BOARD-027, BOARD-024
#[test]
fn board_026_a_resolved_board_has_the_dependency_order_the_steps_and_the_done_lane_as_the_view() {
    let board = "---\ntype: board\n---\n## Chain\nquery: #t\nsort: dependencies\n\n## Done\nquery: #fin\ndone: true\n";
    let files = [("Board.md", board), ("A.md", "---\nblocked-by: [[Fin]]\ndue: 2030-01-03\n---\n#t\n"), ("B.md", "---\nblocked-by: [[A]]\n---\n#t\n"), ("C.md", "---\nblocked-by: [[A]]\ndue: 2030-01-01\n---\n#t\n"), ("Fin.md", "#fin\n")];
    let (v, d) = vault("deps", &files);
    let b = resolve(&v, Path::new("Board.md")).unwrap();
    assert_eq!(titles(&b.lanes[0].cards), ["A", "C", "B"], "blockers first, C is due before B");
    let cards = &b.lanes[0].cards;
    let words: Vec<Vec<String>> = cards.iter().map(|c| c.chip_texts(&[])).collect();
    // A waits for Fin, which is in the done lane: not blocked; B and C wait for A
    assert_eq!(words, [vec!["step 1"], vec!["blocked", "step 2a"], vec!["blocked", "step 2b"]]);
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-004
#[test]
fn board_004_a_pin_that_points_to_no_note_is_listed_as_missing_in_its_lane() {
    let (v, d) = with_board("missing", "---\ntype: board\n---\n# Board\n\n## Pinned\n- [[A]]\n- [[Gone #1]]\n- [Gone](<Sub/Gone %232.md>)\n");
    let b = resolve(&v, Path::new("Board.md")).unwrap();
    assert_eq!(titles(&b.lanes[0].cards), ["A"]);
    assert_eq!(b.lanes[0].missing, ["[[Gone #1]]", "[Gone](<Sub/Gone %232.md>)"]);
    let _ = std::fs::remove_dir_all(d);
}
