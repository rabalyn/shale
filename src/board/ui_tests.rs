//! The board view in the whole window (BOARD-006 to BOARD-014, BOARD-032, QUERY-012, QUERY-019,
//! QUERY-028), driven with real pointer and key events (DEV-041).

use super::file::{props, Board};
use super::LaneView;
use crate::app::{Options, ShaleApp, View};
use crate::keys::{Action, Binding};
use crate::search_all::SearchWorker;
use crate::ui_test_support::Headless;
use eframe::egui::{vec2, Event, Key, Modifiers, PointerButton, Pos2};
use std::path::{Path, PathBuf};

const BOARD: &str = "---\ntype: board\ncard-shows: due, start, status, priority, tags\n---\n# Sprint\n\nA word about the sprint.\n\n## Backlog\nquery: #todo\nunknown line in the lane\n\n## Doing\nquery: status:doing\nsort: priority desc, due asc\n\n## Pinned by hand\n- [[Pinned One]]\n- [[Sketch.excalidraw]]\n";

const FILES: [(&str, &str); 7] = [
    ("Board.md", BOARD),
    ("Alpha.md", "Alpha body #todo\n"),
    ("Beta.md", "---\nstatus: doing\npriority: high\ndue: 2999-01-01\nmodified: 2020-01-01T10:00:00+00:00\n---\nBeta body\n"),
    ("Gamma.md", "---\nstatus: doing\npriority: low\ndue: 2000-01-01\n---\nGamma body\n"),
    ("Pinned One.md", "---\nstatus: doing\n---\nPinned #todo\n"),
    ("Unrelated.md", "Nothing here\n"),
    ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE),
];

/// The left edge of the main area (the ribbon and the left sidebar are left of it).
const MAIN_LEFT: f32 = 305.0;

struct W {
    app: ShaleApp,
    hl: Headless,
    dir: PathBuf,
}

impl W {
    fn new(tag: &str, files: &[(&str, &str)]) -> W {
        let dir = std::env::temp_dir().join(format!("shale-boards-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
        app.cfg.date_prefix_new_notes = false;
        // a peek of this size fits to the right of a card in the window of the tests
        (app.cfg.link_preview_width, app.cfg.link_preview_height) = (400, 400);
        let mut w = W { app, hl: Headless::new(), dir };
        w.hl.settle(&mut w.app);
        w.app.show_view(View::Boards, false);
        w.settle();
        w
    }

    fn board() -> W {
        W::new("board", &FILES)
    }

    /// Frames until the lanes have their answers and are drawn.
    fn settle(&mut self) {
        self.hl.settle(&mut self.app);
        self.hl.frames(&mut self.app, 3);
        self.hl.settle(&mut self.app);
    }

    fn lanes(&self) -> Vec<(String, Vec<String>)> {
        self.app.boards.shown.lanes().iter().map(|l: &LaneView| (l.name.clone(), l.cards.iter().map(|c| c.title.clone()).collect())).collect()
    }

    fn lane(&self, name: &str) -> Vec<String> {
        self.lanes().into_iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no lane {name}: {:?}", self.lanes())).1
    }

    fn file(&self, name: &str) -> String {
        std::fs::read_to_string(self.dir.join(name)).unwrap()
    }

    fn board_text(&self) -> String {
        self.app.vault.as_ref().unwrap().notes[Path::new("Board.md")].text.clone()
    }

    /// What is painted in the main area: the sidebars (the file list names every note too) are left of it.
    fn main(&self) -> impl Iterator<Item = &(String, Pos2)> {
        self.hl.text.iter().filter(|(_, p)| p.x >= MAIN_LEFT)
    }

    /// The place of the text `text` of the main area, the rightmost one when it is painted more than once.
    fn at(&self, text: &str) -> Pos2 {
        let found = self.main().filter(|(t, _)| t == text).max_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let (_, p) = found.unwrap_or_else(|| panic!("{text:?} is not painted; painted: {:?}", self.main().map(|(t, _)| t.as_str()).collect::<Vec<_>>()));
        *p + eframe::egui::vec2(6.0, 6.0)
    }

    fn painted(&self, what: &str) -> bool {
        self.main().any(|(t, _)| t.contains(what))
    }

    fn all_text(&self) -> Vec<&str> {
        self.main().map(|(t, _)| t.as_str()).collect()
    }

    fn click_at(&mut self, p: Pos2) {
        self.hl.click(&mut self.app, p, PointerButton::Primary);
        self.settle();
    }

    fn click(&mut self, text: &str) {
        let p = self.at(text);
        self.click_at(p);
    }

    fn double_click(&mut self, text: &str) {
        let p = self.at(text);
        self.hl.frame(&mut self.app, vec![Event::PointerMoved(p)]);
        for _ in 0..2 {
            self.hl.frame(&mut self.app, vec![Headless::button(p, PointerButton::Primary, true)]);
            self.hl.frame(&mut self.app, vec![Headless::button(p, PointerButton::Primary, false)]);
        }
        self.settle();
    }

    fn key(&mut self, key: Key, modifiers: Modifiers, text: &str) {
        let ev = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers };
        let mut down = vec![ev(true)];
        if !text.is_empty() {
            down.push(Event::Text(text.to_string()));
        }
        self.hl.frame(&mut self.app, down);
        self.hl.frame(&mut self.app, vec![ev(false)]);
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        self.hl.frame(&mut self.app, vec![Event::Text(text.to_string())]);
        self.settle();
    }

    /// Press on the card titled `from` and drag to `to`, staying there with the button down.
    fn drag(&mut self, from: &str, to: Pos2) {
        let p = self.at(from);
        self.hl.press_and_move(&mut self.app, p, &Headless::steps(p, to, 8));
    }

    /// Let go at `at` and settle.
    fn drop_at(&mut self, at: Pos2) {
        self.hl.release(&mut self.app, at);
        self.settle();
    }

    fn drag_drop(&mut self, from: &str, to: Pos2) {
        self.drag(from, to);
        self.drop_at(to);
    }

    /// An empty place in the lane named `lane`, below its header.
    fn lane_top(&self, lane: &str) -> Pos2 {
        self.at(lane) + vec2(0.0, 12.0)
    }

    /// Below the card titled `last`, which is the last of its lane.
    fn lane_end(&self, last: &str) -> Pos2 {
        self.at(last) + vec2(0.0, 90.0)
    }

    fn view(&self) -> View {
        self.app.tabbar.tabs[self.app.tabbar.active].view.clone()
    }

    fn open_tabs(&self) -> Vec<View> {
        self.app.tabbar.tabs.iter().map(|t| t.view.clone()).collect()
    }
}

impl Drop for W {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

// spec: BOARD-007
#[test]
fn board_007_the_view_is_a_main_area_tab_with_a_strip_of_every_board() {
    let w = W::new("strip", &[("Board.md", BOARD), ("Other board.md", "---\ntype: board\n---\n## Only\nquery: #x\n"), ("Plain.md", "---\ntype: note\n---\n## Not a lane\n"), ("Alpha.md", "#todo\n")]);
    assert_eq!(w.view(), View::Boards);
    assert_eq!(w.app.tab_title(&View::Boards), "Boards");
    assert!(w.painted("Board") && w.painted("Other board"), "both boards are in the strip");
    assert!(!w.painted("Plain"), "a note without type: board is no board");
    assert_eq!(w.app.boards.current.as_deref(), Some(Path::new("Board.md")), "the first board opens");
    let mut w = w;
    w.click("Other board");
    assert_eq!(w.app.boards.current.as_deref(), Some(Path::new("Other board.md")), "one click switches");
    assert_eq!(w.lanes().iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["Only"]);
    // the board of the tab is saved with it
    let saved = w.app.saved_panes();
    assert_eq!(saved[0].tabs.iter().find(|t| t.view.as_deref() == Some("boards")).and_then(|t| t.board.clone()).as_deref(), Some("Other board.md"));
}

// spec: BOARD-007
#[test]
fn board_007_a_saved_boards_tab_comes_back_with_its_board() {
    let mut w = W::new("restore", &[("Board.md", BOARD), ("Other board.md", "---\ntype: board\n---\n## Only\n")]);
    let saved = crate::vault_state::SavedTab { view: Some("boards".into()), board: Some("Other board.md".into()), ..Default::default() };
    w.app.restore_view_tab("boards", &saved, true);
    assert_eq!(w.view(), View::Boards);
    assert_eq!(w.app.boards.current.as_deref(), Some(Path::new("Other board.md")));
    let none = crate::vault_state::SavedTab { view: Some("boards".into()), ..Default::default() };
    w.app.boards.current = None;
    w.app.restore_view_tab("boards", &none, false);
    w.settle();
    assert_eq!(w.app.boards.current.as_deref(), Some(Path::new("Board.md")), "without a saved board the first one is shown");
}

// spec: BOARD-008
#[test]
fn board_008_space_b_opens_the_boards_and_the_key_can_be_changed() {
    let mut w = W::new("key", &FILES);
    w.app.show_view(View::Tasks, false);
    w.settle();
    assert_eq!(Action::MainBoards.def().id, "main.boards");
    assert_eq!(w.app.keymap.display(Action::MainBoards), "Space b");
    w.key(Key::Space, Modifiers::NONE, " ");
    assert_eq!(w.view(), View::Tasks, "half a sequence opens nothing");
    w.key(Key::B, Modifiers::NONE, "b");
    assert_eq!(w.view(), View::Boards);
    w.app.show_view(View::Tasks, false);
    w.settle();
    w.app.keymap.set(Action::MainBoards, Binding::parse("Space y").unwrap()).unwrap();
    w.key(Key::Space, Modifiers::NONE, " ");
    w.key(Key::B, Modifiers::NONE, "b");
    assert_eq!(w.view(), View::Tasks, "the old key does nothing now");
    w.key(Key::Space, Modifiers::NONE, " ");
    w.key(Key::Y, Modifiers::NONE, "y");
    assert_eq!(w.view(), View::Boards);
}

// spec: BOARD-009, BOARD-018, BOARD-032
#[test]
fn board_009_a_card_is_in_the_first_lane_that_matches_and_a_pinned_card_wins() {
    let w = W::board();
    // Pinned One matches both queries and is pinned in the third lane: only there
    assert_eq!(w.lane("Backlog"), strs(&["Alpha"]));
    assert_eq!(w.lane("Doing"), strs(&["Beta", "Gamma"]));
    assert_eq!(w.lane("Pinned by hand"), strs(&["Pinned One", "Sketch"]), "the links come in their order, a drawing is a card");
    for t in ["Alpha", "Beta", "Gamma", "Pinned One", "Sketch"] {
        assert!(w.painted(t), "{t} is drawn");
    }
    assert!(!w.painted("Unrelated") && !w.painted("Sprint"), "what no lane matches is not a card, nor is the board");
}

// spec: BOARD-005, BOARD-009
#[test]
fn board_009_cards_every_lane_shows_a_card_in_every_lane_it_matches() {
    let mut w = W::new("every", &[("Board.md", "---\ntype: board\n---\n## A\nquery: #x\n## B\nquery: #x OR #y\n"), ("One.md", "#x\n"), ("Two.md", "#y\n")]);
    assert_eq!((w.lane("A"), w.lane("B")), (strs(&["One"]), strs(&["Two"])), "exclusive by default");
    // the switch in the Board menu writes the front matter
    w.click("Board");
    w.click("A card in every lane it matches");
    assert!(props(&w.board_text()).every_lane && w.file("Board.md").contains("cards: every-lane"));
    w.settle();
    assert_eq!((w.lane("A"), w.lane("B")), (strs(&["One"]), strs(&["One", "Two"])));
}

// spec: BOARD-010
#[test]
fn board_010_a_card_shows_its_chips_as_card_shows_says() {
    let mut w = W::board();
    for chip in ["2999-01-01", "doing", "high", "#todo", "2000-01-01", "low"] {
        assert!(w.painted(chip), "{chip} is drawn: {:?}", w.all_text());
    }
    // overdue is red: the chip of Gamma has the fill of a bad status, the chip of Beta has not
    let bad = crate::theme::palette().bad_bg;
    let (overdue, fine) = (w.at("2000-01-01"), w.at("2999-01-01"));
    let fill_at = |w: &W, p: Pos2| w.hl.rects.iter().rev().find(|(r, _)| r.contains(p)).map(|(_, c)| *c);
    assert_eq!(fill_at(&w, overdue), Some(bad), "an overdue date is red");
    assert_ne!(fill_at(&w, fine), Some(bad));
    // card-shows: without the due date and the tags
    w.app.boards.current = Some(PathBuf::from("Board.md"));
    let text = w.board_text().replace("card-shows: due, start, status, priority, tags", "card-shows: status");
    w.app.vault.as_mut().unwrap().set_note_text(Path::new("Board.md"), &text);
    w.settle();
    assert!(w.painted("doing") && !w.painted("2999-01-01") && !w.painted("high") && !w.painted("#todo"), "only the status chip is left");
}

// spec: BOARD-005, BOARD-010
#[test]
fn board_010_the_board_menu_chooses_the_chips() {
    let mut w = W::board();
    w.click("Board");
    w.click("priority");
    assert!(w.file("Board.md").contains("card-shows: due, start, status, tags"), "{}", w.file("Board.md"));
    assert!(!w.painted("high"));
    w.click("Board");
    w.click("priority");
    assert!(!w.file("Board.md").contains("card-shows"), "all the chips are the default: no line");
    assert!(w.painted("high"));
}

// spec: BOARD-011
#[test]
fn board_011_a_drawing_is_a_card_found_by_name_and_has_a_picture() {
    let mut w = W::new("drawing", &[("Board.md", "---\ntype: board\n---\n## Drawings\nquery: path:Sketch\n"), ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE), ("Note.md", "Sketch\n")]);
    assert_eq!(w.lane("Drawings"), strs(&["Sketch"]), "found by its path");
    assert!(w.app.boards.shown.lanes()[0].cards[0].drawing);
    w.hl.frames(&mut w.app, 2);
    assert!(w.painted("Sketch"));
    // (a click on a drawing opens it in a tab: it has no text to edit in the peek)
    let p = w.at("Sketch");
    w.click_at(p);
    assert!(w.app.boards.peek.is_none());
    assert!(w.open_tabs().contains(&View::Drawing(PathBuf::from("Sketch.excalidraw"))));
}

// spec: BOARD-012
#[test]
fn board_012_a_click_selects_the_card_and_opens_the_peek_and_esc_closes_it() {
    let mut w = W::board();
    w.click("Beta");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")));
    let peek = w.app.boards.peek.as_ref().expect("the peek is open");
    assert_eq!(peek.rel, Path::new("Beta.md"));
    assert_eq!(w.view(), View::Boards, "the board stays");
    assert!(w.painted("Beta body"), "the note is in the peek");
    w.click("Alpha");
    assert_eq!(w.app.boards.peek.as_ref().map(|p| p.rel.clone()), Some(PathBuf::from("Alpha.md")), "another card: another peek");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(w.app.boards.peek.is_none(), "Esc closes the peek");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Alpha.md")), "the card stays selected");
    assert_eq!(w.view(), View::Boards);
}

// spec: BOARD-012
#[test]
fn board_012_the_peek_has_the_size_of_the_link_preview_and_is_resized_by_its_edges() {
    let mut w = W::board();
    w.click("Beta");
    let size = w.app.boards.peek.as_ref().unwrap().rect.expect("drawn").size();
    assert_eq!((size.x.round() as u32, size.y.round() as u32), (w.app.cfg.link_preview_width, w.app.cfg.link_preview_height), "EDIT-037");
    // dragging the bottom right corner makes it bigger and the size is kept as the setting
    let r = w.app.boards.peek.as_ref().unwrap().rect.unwrap();
    let from = r.right_bottom();
    w.hl.press_and_move(&mut w.app, from, &Headless::steps(from, from + eframe::egui::vec2(40.0, 30.0), 4));
    w.hl.release(&mut w.app, from + eframe::egui::vec2(40.0, 30.0));
    w.settle();
    let bigger = w.app.boards.peek.as_ref().unwrap().rect.unwrap().size();
    assert!(bigger.x > size.x + 20.0 && bigger.y > size.y + 10.0, "{size:?} -> {bigger:?}");
    assert_eq!(w.app.cfg.link_preview_width, bigger.x.round() as u32);
}

// spec: BOARD-012
#[test]
fn board_012_the_peek_is_moved_by_its_title() {
    let mut w = W::board();
    w.click("Beta");
    let before = w.app.boards.peek.as_ref().unwrap().rect.unwrap();
    // the title of the note is in the peek, right of the card with the same name
    let from = w.at("Beta");
    assert!(before.contains(from));
    let to = from + eframe::egui::vec2(-60.0, 50.0);
    w.hl.press_and_move(&mut w.app, from, &Headless::steps(from, to, 5));
    w.hl.release(&mut w.app, to);
    w.settle();
    let after = w.app.boards.peek.as_ref().unwrap().rect.unwrap();
    assert!((after.min.x - before.min.x + 60.0).abs() < 2.0 && (after.min.y - before.min.y - 50.0).abs() < 2.0, "{before:?} -> {after:?}");
    assert_eq!(after.size(), before.size(), "only moved");
    assert!(w.app.boards.peek.is_some(), "a drag on the title keeps it open");
}

// spec: BOARD-013
#[test]
fn board_013_the_peek_is_an_editor_that_saves_with_the_note() {
    let mut w = W::board();
    w.click("Alpha");
    let r = w.app.boards.peek.as_ref().unwrap().rect.unwrap();
    // a click in the text gives the editor the keyboard; typing changes the note's buffer
    w.click_at(Pos2::new(r.min.x + 60.0, r.min.y + 70.0));
    w.type_text("EDITED ");
    let rel = Path::new("Alpha.md");
    assert!(w.app.notes[rel].text.contains("EDITED"), "{:?}", w.app.notes[rel].text);
    assert!(w.app.vault.as_ref().unwrap().notes[rel].text.contains("EDITED"), "the index follows at once");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(w.app.boards.peek.is_none());
    let saved = w.file("Alpha.md");
    assert!(saved.contains("EDITED") && saved.contains("modified:") && saved.contains("created:"), "closing saves like the editor does (a checked save): {saved}");
    assert!(!w.app.notes[rel].dirty);
}

// spec: BOARD-013
#[test]
fn board_013_the_priority_control_writes_the_front_matter_and_not_modified() {
    let mut w = W::board();
    w.click("Beta");
    assert!(w.painted("Priority: high"));
    w.click("Priority: high");
    w.click("highest");
    let saved = w.file("Beta.md");
    assert!(saved.contains("priority: highest") && !saved.contains("priority: high\n"), "{saved}");
    assert!(saved.contains("modified: 2020-01-01T10:00:00+00:00"), "the body is the same, `modified:` stays: {saved}");
    assert!(saved.contains("Beta body"));
    assert_eq!(w.lane("Doing"), strs(&["Beta", "Gamma"]));
    // none takes the property out
    w.click("Priority: highest");
    w.click("none");
    assert!(!w.file("Beta.md").contains("priority:"), "{}", w.file("Beta.md"));
    assert!(w.painted("Priority: none"));
}

// spec: BOARD-014
#[test]
fn board_014_ctrl_click_and_enter_open_the_note_in_a_tab() {
    let mut w = W::board();
    let p = w.at("Gamma");
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(p)]);
    w.hl.frame(&mut w.app, vec![Event::ModifiersChanged(Modifiers::CTRL), Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: true, modifiers: Modifiers::CTRL }]);
    w.hl.frame(&mut w.app, vec![Event::PointerButton { pos: p, button: PointerButton::Primary, pressed: false, modifiers: Modifiers::CTRL }]);
    w.hl.frame(&mut w.app, vec![Event::ModifiersChanged(Modifiers::NONE)]);
    w.settle();
    assert!(w.open_tabs().contains(&View::Note(PathBuf::from("Gamma.md"))), "{:?}", w.open_tabs());
    assert!(w.open_tabs().contains(&View::Boards), "the board has its own tab");
    assert!(w.app.boards.peek.is_none(), "Ctrl+Click opens no peek");
    // Enter on the selected card
    let mut w = W::board();
    w.click("Alpha");
    w.key(Key::Escape, Modifiers::NONE, "");
    w.key(Key::Enter, Modifiers::NONE, "");
    assert!(w.open_tabs().contains(&View::Note(PathBuf::from("Alpha.md"))), "{:?}", w.open_tabs());
    assert!(w.open_tabs().contains(&View::Boards));
}

// spec: BOARD-012, BOARD-014
#[test]
fn board_014_the_arrow_keys_move_the_selection_between_the_cards() {
    let mut w = W::board();
    w.app.boards.selected = None;
    w.key(Key::ArrowDown, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Alpha.md")), "the first card of the first lane");
    w.key(Key::ArrowRight, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")));
    w.key(Key::ArrowDown, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Gamma.md")));
    w.key(Key::ArrowDown, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Gamma.md")), "the last stays");
    w.key(Key::ArrowUp, Modifiers::NONE, "");
    w.key(Key::ArrowLeft, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Alpha.md")));
    assert!(w.app.boards.peek.is_none(), "the keys select, they do not open the peek");
}

// spec: BOARD-006
#[test]
fn board_006_adding_renaming_moving_and_removing_lanes_rewrites_the_file() {
    let mut w = W::board();
    let names = |w: &W| w.lanes().iter().map(|(n, _)| n.clone()).collect::<Vec<_>>();
    // the ⋯ button of lane `i`, the lanes left to right
    let more = |w: &W, i: usize| -> Pos2 {
        let mut dots: Vec<Pos2> = w.main().filter(|(t, _)| t == "⋯").map(|(_, p)| *p + eframe::egui::vec2(4.0, 6.0)).collect();
        dots.sort_by(|a, b| a.x.total_cmp(&b.x));
        dots[i]
    };
    // delete the last lane, with its pinned cards
    let p = more(&w, 2);
    w.click_at(p);
    w.click("Delete lane…");
    assert!(w.painted("Delete lane?"), "the question comes first");
    w.click("Cancel");
    assert_eq!(names(&w), ["Backlog", "Doing", "Pinned by hand"], "Cancel leaves the lane");
    assert!(w.board_text().contains("Pinned One"));
    let p = more(&w, 2);
    w.click_at(p);
    w.click("Delete lane…");
    w.click("Delete lane");
    assert_eq!(names(&w), ["Backlog", "Doing"]);
    assert!(w.board_text().ends_with("sort: priority desc, due asc\n"), "{}", w.board_text());
    assert!(!w.board_text().contains("Pinned One") && w.file("Board.md").contains("A word about the sprint."));
    // a lane at the end, named at once
    w.click("+ Add lane");
    assert!(w.board_text().ends_with("sort: priority desc, due asc\n\n## New lane\n"), "{}", w.board_text());
    w.type_text("Review");
    w.key(Key::Enter, Modifiers::NONE, "");
    assert!(w.board_text().ends_with("\n## Review\n"), "{}", w.board_text());
    assert_eq!(names(&w), ["Backlog", "Doing", "Review"]);
    // the lane menu: move left, rename
    let p = more(&w, 2);
    w.click_at(p);
    w.click("Move left");
    assert_eq!(names(&w), ["Backlog", "Review", "Doing"]);
    let t = w.board_text();
    assert!(t.contains("unknown line in the lane\n\n## Review\n\n## Doing\nquery: status:doing\nsort: priority desc, due asc\n"), "{t}");
    assert!(t.contains("A word about the sprint.") && t.contains("type: board"), "lines the view does not know stay");
    let p = more(&w, 1);
    w.click_at(p);
    w.click("Rename");
    w.type_text("Check");
    w.key(Key::Enter, Modifiers::NONE, "");
    assert_eq!(names(&w), ["Backlog", "Check", "Doing"]);
    assert!(w.board_text().contains("\n## Check\n"), "{}", w.board_text());
    // Esc leaves the name as it was
    let p = more(&w, 1);
    w.click_at(p);
    w.click("Rename");
    w.type_text("Nope");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert_eq!(names(&w), ["Backlog", "Check", "Doing"]);
    // the first lane takes its lines with it
    let p = more(&w, 0);
    w.click_at(p);
    w.click("Delete lane…");
    w.click("Delete lane");
    assert_eq!(names(&w), ["Check", "Doing"]);
    assert!(!w.board_text().contains("unknown line in the lane") && !w.board_text().contains("query: #todo"), "{}", w.board_text());
    assert_eq!(Board::parse(&w.file("Board.md")).lanes.len(), 2, "and the file on disk agrees");
}

// spec: BOARD-006
#[test]
fn board_006_a_hand_edit_of_the_file_changes_the_view() {
    let mut w = W::board();
    let text = w.board_text().replace("## Doing\nquery: status:doing", "## Underway\nquery: status:doing OR #todo").replace("- [[Sketch.excalidraw]]\n", "");
    w.app.vault.as_mut().unwrap().set_note_text(Path::new("Board.md"), &text);
    w.settle();
    assert_eq!(w.lane("Underway"), strs(&["Beta", "Gamma"]), "Alpha is in the first lane that matches: Backlog");
    assert_eq!(w.lane("Pinned by hand"), strs(&["Pinned One"]));
    assert!(w.painted("Underway") && !w.painted("Doing"));
    // and an edit of the open note in its tab (the buffer) does the same
    w.app.open(Path::new("Board.md"), true);
    w.settle();
    let edited = w.app.notes[Path::new("Board.md")].text.replace("Underway", "Busy");
    w.app.notes.get_mut(Path::new("Board.md")).unwrap().text = edited.clone();
    w.app.vault.as_mut().unwrap().set_note_text(Path::new("Board.md"), &edited);
    w.app.show_view(View::Boards, false);
    w.settle();
    assert!(w.painted("Busy"));
}

// spec: BOARD-006
#[test]
fn board_006_a_change_in_the_view_while_the_note_is_open_in_a_tab_goes_through_its_buffer() {
    let mut w = W::board();
    w.app.open(Path::new("Board.md"), true);
    w.app.show_view(View::Boards, false);
    w.settle();
    w.click("+ Add lane");
    let buffer = w.app.notes[Path::new("Board.md")].text.clone();
    assert!(buffer.contains("## New lane"), "the open buffer has the change: {buffer}");
    w.key(Key::Escape, Modifiers::NONE, "");
    w.app.save_note(Path::new("Board.md"));
    assert!(w.file("Board.md").contains("## New lane") && w.file("Board.md").contains("unknown line in the lane"));
}

// spec: BOARD-001, BOARD-006
#[test]
fn board_001_new_board_makes_a_note_with_type_board_and_one_lane() {
    let mut w = W::new("new", &[("Alpha.md", "#todo\n")]);
    assert!(w.painted("No board in this vault yet"));
    w.click("New board");
    let made = w.file("Board.md");
    assert_eq!(made, "---\ntype: board\n---\n# Board\n\n## To do\nquery: #todo\n");
    assert!(super::file::is_board(&made));
    assert_eq!(w.app.boards.current.as_deref(), Some(Path::new("Board.md")));
    assert_eq!(w.lane("To do"), strs(&["Alpha"]));
    w.click("+ New board");
    assert_eq!(w.app.boards.listing(w.app.vault.as_ref().unwrap()).len(), 2, "a second one is made next to it");
    assert!(w.dir.join("Board 1.md").exists() || w.dir.join("Board 2.md").exists());
}

// spec: BOARD-001
#[test]
fn board_001_a_board_is_a_normal_note_that_opens_in_the_editor() {
    let mut w = W::board();
    w.click("Edit file");
    assert!(w.open_tabs().contains(&View::Note(PathBuf::from("Board.md"))));
    assert!(w.app.notes[Path::new("Board.md")].text.contains("## Doing"));
    w.app.open(Path::new("Board.md"), false);
    assert!(w.app.vault.as_ref().unwrap().notes[Path::new("Board.md")].links.iter().any(|l| l.target == "Pinned One"), "its links are indexed like any note's");
}

// spec: BOARD-030
#[test]
fn board_030_the_export_button_of_the_board_opens_the_export_dialog_of_the_open_board() {
    let mut w = W::board();
    w.click("Export PDF…");
    let Some(crate::app::Modal::Export(d)) = &w.app.modal else { panic!("the export dialog opens") };
    assert_eq!(d.rel, Path::new("Board.md"));
    assert!(d.book.as_ref().is_some_and(|b| b.board), "the dialog of a board");
}

// spec: QUERY-012
#[test]
fn query_012_a_bad_lane_query_shows_its_error_in_its_header_and_the_rest_works() {
    let mut w = W::new("bad", &[("Board.md", "---\ntype: board\n---\n## Broken\nquery: (#todo AND\n## Prefix\nquery: f: #todo\n## Fine\nquery: #todo\n"), ("Alpha.md", "#todo\n")]);
    let lanes = w.app.boards.shown.lanes().to_vec();
    assert!(lanes[0].error.is_some() && lanes[0].cards.is_empty(), "{:?}", lanes[0]);
    assert!(lanes[1].error.as_deref().is_some_and(|e| e.contains("notes only")), "a prefix is an error in a lane: {:?}", lanes[1].error);
    assert!(lanes[2].error.is_none());
    assert_eq!(w.lane("Fine"), strs(&["Alpha"]));
    assert!(w.painted(lanes[0].error.as_deref().unwrap()), "the reason is drawn in the header");
    assert!(w.painted("Alpha"));
    // a bad sort says so in the same place
    w.app.vault.as_mut().unwrap().set_note_text(Path::new("Board.md"), "---\ntype: board\n---\n## Lane\nquery: #todo\nsort: size desc\n");
    w.settle();
    assert!(w.app.boards.shown.lanes()[0].error.as_deref().is_some_and(|e| e.contains("not a sort field")));
    assert_eq!(w.lane("Lane"), strs(&["Alpha"]), "the cards stay, by title");
}

// spec: QUERY-019
#[test]
fn query_019_lanes_search_on_the_worker_and_the_view_shows_it_while_waiting() {
    fn never(_: &mut crate::search_all::worker::Conns, _: &crate::search_all::Request, cancel: &std::sync::atomic::AtomicBool) -> Option<crate::search_all::Found> {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !cancel.load(std::sync::atomic::Ordering::Relaxed) && std::time::Instant::now() < end {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        None
    }
    let mut w = W::new("wait", &FILES);
    w.app.search_worker = SearchWorker::with_runner(never);
    w.app.boards.runs = Default::default();
    w.hl.frames(&mut w.app, 3);
    assert!(w.app.boards.shown.lanes()[..2].iter().all(|l| l.waiting && l.cards.is_empty()), "no answer yet");
    assert!(w.painted("Searching…"), "the frame is drawn at once, the answer comes later");
    assert!(w.app.search_worker.busy(), "the window did not wait for it");
    w.app.search_worker = Default::default();
    w.app.boards.runs = Default::default();
    w.settle();
    assert_eq!(w.lane("Backlog"), strs(&["Alpha"]));
}

// spec: QUERY-019
#[test]
fn query_019_a_change_asks_again_only_the_lanes_it_can_change() {
    let mut w = W::board();
    let asked = |w: &W| w.app.boards.runs.asked;
    let base = asked(&w);
    assert_eq!(base, 2, "one search per lane that has a query, at the start (the pinned lane has none)");
    w.hl.frames(&mut w.app, 5);
    assert_eq!(asked(&w), base, "nothing changed: nothing is asked");
    // (a change on disk and in the index, as a save makes it)
    let set = |w: &mut W, rel: &str, text: &str| {
        w.app.vault.as_mut().unwrap().edit_note(Path::new(rel), |_| Some(text.to_string())).unwrap();
        w.settle();
    };
    // a note no lane matches, still matching none
    set(&mut w, "Unrelated.md", "Still nothing\n");
    assert_eq!(asked(&w), base, "a note that is in no lane and matches none changes no lane");
    // it starts to match the first lane
    set(&mut w, "Unrelated.md", "#todo now\n");
    assert_eq!(asked(&w), base + 1, "only the lane that matches it now asks");
    assert_eq!(w.lane("Backlog"), strs(&["Alpha", "Unrelated"]));
    // a note of the second lane leaves it
    set(&mut w, "Gamma.md", "---\nstatus: done\n---\nGamma body\n");
    assert_eq!(asked(&w), base + 2, "only the lane the note was in asks");
    assert_eq!(w.lane("Doing"), strs(&["Beta"]));
    // a note of a lane changes in a way that keeps it there: the lane asks again (its cards may differ)
    set(&mut w, "Beta.md", "---\nstatus: doing\npriority: low\n---\nBeta body\n");
    assert_eq!(asked(&w), base + 3);
    // a new note: the vault finds the links of every note again, so every lane asks
    w.app.vault.as_mut().unwrap().create_file(Path::new("Fresh.md"), "---\nstatus: doing\n---\n").unwrap();
    w.settle();
    assert_eq!(asked(&w), base + 5);
    assert_eq!(w.lane("Doing"), strs(&["Beta", "Fresh"]));
    // many notes at once: every lane asks
    for i in 0..10 {
        w.app.vault.as_mut().unwrap().create_file(Path::new(&format!("Bulk{i}.md")), "bulk\n").unwrap();
    }
    w.settle();
    assert_eq!(asked(&w), base + 5 + 2);
}

// spec: QUERY-019
#[test]
fn query_019_a_lane_that_reads_other_notes_asks_for_every_change() {
    let mut w = W::new("deps", &[("Board.md", "---\ntype: board\n---\n## Blocked\nquery: blocked\n## Plain\nquery: #x\n"), ("A.md", "---\nblocked-by: [[B]]\n---\n"), ("B.md", "b\n"), ("C.md", "c\n")]);
    assert_eq!(w.lane("Blocked"), strs(&["A"]));
    let base = w.app.boards.runs.asked;
    w.app.vault.as_mut().unwrap().set_note_text(Path::new("B.md"), "---\nstatus: done\n---\n");
    w.settle();
    assert_eq!(w.app.boards.runs.asked, base + 1, "the blocked lane asks, the plain lane does not");
    assert_eq!(w.lane("Blocked"), Vec::<String>::new(), "B is done, A is free");
}

// spec: QUERY-028
#[test]
fn query_028_a_double_click_on_the_heading_opens_the_settings_that_write_the_file() {
    let mut w = W::board();
    w.double_click("Doing");
    assert!(w.painted("Lane “Doing”"), "the popover is open");
    assert!(w.painted("When a card is dropped here, set"));
    assert!(w.painted("status: doing"), "the drop property is filled from the filter as a hint: {:?}", w.all_text());
    // the drop property is editable: it is a line of the lane
    let p = w.at("status: doing");
    w.click_at(p);
    w.type_text("status: review");
    assert!(w.board_text().contains("query: status:doing\nsort: priority desc, due asc\ndrop: status: review\n"), "{}", w.board_text());
    // Advanced shows the text of the query and counts the cards; changing it changes the lane
    w.click("Advanced");
    assert!(w.painted("2 matches"), "the form counts the cards of the lane");
    let p = w.at("status:doing");
    w.click_at(p);
    w.key(Key::A, Modifiers::COMMAND, "");
    w.type_text("#todo");
    assert!(w.board_text().contains("## Doing\nquery: #todo\nsort: priority desc, due asc\ndrop: status: review\n"), "{}", w.board_text());
    assert!(w.app.cfg.search.advanced, "the switch is a setting (QUERY-026)");
    w.settle();
    assert!(w.painted("Lane “Doing”"), "the popover stays open while it is edited");
    assert_eq!(w.lane("Doing"), Vec::<String>::new(), "#todo is in the first lane, Pinned One is pinned");
    // a click outside keeps it open; Esc closes it (#632)
    w.click_at(Pos2::new(900.0, 700.0));
    assert!(w.painted("Lane “Doing”"), "a click outside does not close the popover");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!w.painted("Lane “Doing”"));
    w.app.cfg.search.advanced = false;
}

// spec: QUERY-028
#[test]
fn query_028_the_menu_opens_the_settings_and_the_form_edits_sort_and_done() {
    let mut w = W::board();
    let mut dots: Vec<Pos2> = w.hl.text.iter().filter(|(t, _)| t == "⋯").map(|(_, p)| *p + eframe::egui::vec2(4.0, 6.0)).collect();
    dots.sort_by(|a, b| a.x.total_cmp(&b.x));
    w.click_at(dots[1]);
    w.click("Settings…");
    assert!(w.painted("Lane “Doing”"));
    // the sort of the lane is in the form, as rows
    assert!(w.painted("Sort") && w.painted("priority"), "{:?}", w.all_text());
    w.click("Cards here are done");
    assert!(w.board_text().contains("## Doing\nquery: status:doing\nsort: priority desc, due asc\ndone: true\n"), "{}", w.board_text());
    w.click("Cards here are done");
    assert!(!w.board_text().contains("done: true"));
    // a query that is no simple property has no derived drop property
    assert_eq!(super::lanes::drop_property("status:doing"), Some("status: doing".into()));
    assert_eq!(super::lanes::drop_property("Status:\"In Progress\""), Some("Status: In Progress".into()));
    assert_eq!(super::lanes::drop_property("#todo"), None);
    assert_eq!(super::lanes::drop_property("status:doing AND #todo"), None);
    assert_eq!(super::lanes::drop_property("status:doing OR status:todo"), None);
    assert_eq!(super::lanes::drop_property("(status:"), None);
    assert_eq!(super::lanes::drop_property(""), None);
}

fn open_settings_of_doing(w: &mut W) {
    w.double_click("Doing");
    assert!(w.painted("Lane “Doing”"), "the popover is open");
}

// spec: QUERY-028
#[test]
fn query_028_a_choice_in_the_menus_of_the_popover_is_applied_and_only_the_close_button_esc_or_the_dots_close_it() {
    let mut w = W::board();
    open_settings_of_doing(&mut w);
    let rows = w.all_text().iter().filter(|t| **t == "tag is").count();
    w.click("+ Add filter");
    w.click("tag is");
    assert!(w.painted("Lane “Doing”"), "choosing in the menu does not close the popover");
    assert_eq!(w.all_text().iter().filter(|t| **t == "tag is").count(), rows + 1, "the row is added: {:?}", w.all_text());
    w.click("+ Add sort");
    w.click("title");
    assert!(w.painted("Lane “Doing”") && w.board_text().contains("title"), "the sort is added: {}", w.board_text());
    w.click_at(Pos2::new(900.0, 700.0));
    assert!(w.painted("Lane “Doing”"), "a click outside keeps it open");
    w.click("✕");
    assert!(!w.painted("Lane “Doing”"), "the close button closes it");
    open_settings_of_doing(&mut w);
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!w.painted("Lane “Doing”"), "Esc closes it");
    open_settings_of_doing(&mut w);
    let mut dots: Vec<Pos2> = w.hl.text.iter().filter(|(t, _)| t == "⋯").map(|(_, p)| *p + eframe::egui::vec2(4.0, 6.0)).collect();
    dots.sort_by(|a, b| a.x.total_cmp(&b.x));
    w.click_at(dots[1]);
    assert!(!w.painted("Lane “Doing”"), "the dots again close it");
}

// spec: QUERY-029
#[test]
fn query_029_the_lane_popover_lists_the_tags_and_esc_closes_the_list_before_the_popover() {
    let mut w = W::board();
    open_settings_of_doing(&mut w);
    w.click("+ Add filter");
    w.click("tag is");
    w.click("tag");
    w.hl.settle(&mut w.app);
    assert!(crate::search_form::list_open(w.hl.ctx()), "the list of the tags of the vault is open");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!crate::search_form::list_open(w.hl.ctx()), "Esc closes the list");
    assert!(w.painted("Lane “Doing”"), "and not the popover");
    assert!(w.app.boards.peek.is_none());
    w.click("tag");
    w.hl.settle(&mut w.app);
    w.click("todo");
    assert!(w.board_text().contains("#todo"), "a click takes the tag into the lane: {}", w.board_text());
    assert!(w.painted("Lane “Doing”"), "the click in the list does not close the popover");
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!w.painted("Lane “Doing”"), "the next Esc closes the popover");
}

// spec: BOARD-013, KEY-045
#[test]
fn board_013_space_space_ticks_the_task_in_the_peek_in_vim_normal_mode_and_esc_leaves_insert_first() {
    let mut w = W::new("vim", &[("Board.md", "---\ntype: board\n---\n## Todo\nquery: #todo\n"), ("Tasky.md", "#todo\n- [ ] one task\n")]);
    w.app.cfg.vim_mode = true;
    w.click("Tasky");
    let r = w.app.boards.peek.as_ref().unwrap().rect.unwrap();
    // a click in the text gives the peek's editor the keyboard; vim starts in normal mode
    w.click_at(Pos2::new(r.min.x + 80.0, r.min.y + 70.0));
    assert!(w.app.boards.peek.as_ref().unwrap().editor.vim_normal_keys(w.hl.ctx()), "vim normal mode in the peek");
    // put the cursor on the task line, as a click would, and tick it
    let id = w.app.boards.peek.as_ref().unwrap().editor.id;
    let mut st = eframe::egui::text_edit::TextEditState::load(w.hl.ctx(), id).unwrap_or_default();
    let at = w.app.notes[Path::new("Tasky.md")].text.find("one task").unwrap();
    st.cursor.set_char_range(Some(eframe::egui::text::CCursorRange::one(eframe::egui::text::CCursor::new(at))));
    st.store(w.hl.ctx(), id);
    w.hl.frames(&mut w.app, 2);
    w.key(Key::Space, Modifiers::NONE, " ");
    w.key(Key::Space, Modifiers::NONE, " ");
    assert!(w.app.notes[Path::new("Tasky.md")].text.contains("- [x] one task"), "{:?}", w.app.notes[Path::new("Tasky.md")].text);
    // Esc in insert mode leaves it and keeps the peek; in normal mode it closes the peek
    w.key(Key::I, Modifiers::NONE, "i");
    assert_eq!(w.app.boards.peek.as_ref().unwrap().editor.vim.mode, crate::vim::Mode::Insert);
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(w.app.boards.peek.is_some(), "the first Esc is the editor's");
    assert_eq!(w.app.boards.peek.as_ref().unwrap().editor.vim.mode, crate::vim::Mode::Normal);
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(w.app.boards.peek.is_none());
    assert!(w.file("Tasky.md").contains("- [x] one task"), "saved with the peek");
}

// spec: BOARD-007, BOARD-008
#[test]
fn board_007_the_ribbon_has_a_boards_icon_that_opens_or_shows_the_boards() {
    use crate::sidebar::Item;
    let mut w = W::board();
    assert!(Item::ALL.contains(&Item::Boards) && w.app.side.layout.ribbon.contains(&Item::Boards), "in the default ribbon");
    assert_eq!(Item::from_id("boards"), Some(Item::Boards));
    assert!(Item::Boards.tip().contains("Space b"));
    w.app.show_view(View::Tasks, false);
    w.settle();
    let at = w.hl.find_label(&mut w.app, Item::Boards.tip());
    w.click_at(at.center());
    assert_eq!(w.view(), View::Boards);
    let tabs = w.open_tabs().len();
    w.click_at(at.center());
    assert_eq!(w.open_tabs().len(), tabs, "the view is shown, not opened twice");
}

const FLOW: &str = "---\ntype: board\n---\n# Flow\n\n## Todo\nquery: #todo\n\n## Doing\nquery: status:doing\n\n## Done\nquery: status:done\n\n## Later\nquery: #later OR status:later\ndrop: status: later\n\n## Manual\n- [[Pinned One]]\n- [[Sketch.excalidraw]]\n";

const FLOW_FILES: [(&str, &str); 6] = [
    ("Board.md", FLOW),
    ("Alpha.md", "Alpha body #todo\n"),
    ("Beta.md", "---\nstatus: doing\nmodified: 2020-01-01T10:00:00+00:00\n---\nBeta body\n"),
    ("Gamma.md", "---\nstatus: doing\n---\nGamma body\n"),
    ("Pinned One.md", "---\nstatus: doing\n---\nPinned #todo\n"),
    ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE),
];

fn flow(tag: &str) -> W {
    let mut w = W::new(tag, &FLOW_FILES);
    w.hl.set_size(vec2(2200.0, 900.0));
    w.settle();
    w
}

// spec: BOARD-019
#[test]
fn board_019_a_drop_on_a_lane_with_a_simple_query_sets_that_property() {
    let mut w = flow("drop-derived");
    assert_eq!(w.lane("Doing"), strs(&["Beta", "Gamma"]));
    let to = w.lane_top("Done");
    w.drag_drop("Beta", to);
    let beta = w.file("Beta.md");
    assert!(beta.contains("status: done") && !beta.contains("status: doing"), "{beta}");
    assert!(beta.contains("Beta body"), "the rest of the note stays: {beta}");
    assert_eq!(w.lane("Done"), strs(&["Beta"]), "the lane searched again and shows the card");
    assert_eq!(w.lane("Doing"), strs(&["Gamma"]));
    assert_eq!(w.board_text(), FLOW, "no pin is written for a lane with a property");
    assert!(w.app.boards.view.over.is_none() && !w.app.boards.view.dragging());
}

// spec: BOARD-019
#[test]
fn board_019_the_drop_line_of_the_lane_wins_over_its_query() {
    let mut w = flow("drop-line");
    let to = w.lane_top("Later");
    w.drag_drop("Gamma", to);
    assert!(w.file("Gamma.md").contains("status: later"), "{}", w.file("Gamma.md"));
    assert_eq!(w.lane("Later"), strs(&["Gamma"]));
}

// spec: BOARD-019
#[test]
fn board_019_the_drag_marks_the_lane_and_the_place_and_a_ghost_follows_the_pointer() {
    let mut w = flow("drag-feedback");
    let before = w.main().filter(|(t, _)| t == "Gamma").count();
    // above the first card of Doing
    let to = w.at("Beta");
    w.drag("Gamma", to);
    assert!(w.app.boards.view.dragging());
    assert_eq!(w.app.boards.view.over, Some((1, 0)), "lane Doing, above its first card");
    assert_eq!(w.main().filter(|(t, _)| t == "Gamma").count(), before + 1, "the ghost shows the title at the pointer");
    let to = w.lane_end("Gamma");
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(to)]);
    assert_eq!(w.app.boards.view.over, Some((1, 2)), "below its last card");
    let to = w.lane_top("Done");
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(to)]);
    assert_eq!(w.app.boards.view.over, Some((2, 0)), "the empty lane Done");
    // nothing is written until the drop
    assert!(w.file("Gamma.md").contains("status: doing"));
    w.drop_at(to);
    assert!(w.file("Gamma.md").contains("status: done"));
}

// spec: BOARD-019
#[test]
fn board_019_esc_cancels_a_drag_and_a_drop_outside_the_lanes_does_nothing() {
    let mut w = flow("drag-cancel");
    let to = w.lane_top("Done");
    w.drag("Beta", to);
    assert!(w.app.boards.view.dragging());
    w.key(Key::Escape, Modifiers::NONE, "");
    assert!(!w.app.boards.view.dragging() && w.app.boards.view.over.is_none());
    w.drop_at(to);
    assert!(w.file("Beta.md").contains("status: doing"), "the drop after Esc does nothing");
    assert!(w.app.boards.peek.is_none(), "a drag is no click");
    // outside: above the lanes, on the strip of boards
    let strip = w.at("+ New board");
    w.drag("Beta", strip);
    assert_eq!(w.app.boards.view.over, None);
    w.drop_at(strip);
    assert!(w.file("Beta.md").contains("status: doing"));
    assert_eq!(w.board_text(), FLOW);
    assert_eq!(w.lane("Doing"), strs(&["Beta", "Gamma"]));
}

// spec: BOARD-020
#[test]
fn board_020_a_drop_does_not_move_modified_on_disk_or_in_an_open_note_and_is_an_undo_step() {
    let mut w = flow("drop-modified");
    let to = w.lane_top("Done");
    w.drag_drop("Beta", to);
    let beta = w.file("Beta.md");
    assert!(beta.contains("modified: 2020-01-01T10:00:00+00:00") && beta.contains("status: done"), "{beta}");
    // a note that is open in a buffer is changed there and saved the checked way
    w.app.open(Path::new("Gamma.md"), true);
    w.app.show_view(View::Boards, false);
    w.settle();
    let to = w.lane_top("Later");
    w.drag_drop("Gamma", to);
    let doc = &w.app.notes[Path::new("Gamma.md")];
    assert!(doc.text.contains("status: later") && !doc.dirty, "saved: {:?}", doc.text);
    assert!(w.file("Gamma.md").contains("status: later"));
    assert!(!w.file("Gamma.md").contains("modified:"), "the body is the same: {}", w.file("Gamma.md"));
    let mut t = doc.text.clone();
    assert!(w.app.undo.of(Path::new("Gamma.md")).undo(&mut t).is_some());
    assert!(t.contains("status: doing"), "the drop is an undo step of the note: {t:?}");
}

// spec: BOARD-020
#[test]
fn board_020_a_note_that_changed_since_the_view_drew_it_is_not_moved() {
    let mut w = flow("drop-conflict");
    w.app.open(Path::new("Beta.md"), true);
    w.app.show_view(View::Boards, false);
    w.settle();
    // the buffer says something else than the index the view was drawn from
    w.app.notes.get_mut(Path::new("Beta.md")).unwrap().text.push_str("typed meanwhile\n");
    let to = w.lane_top("Done");
    w.drag_drop("Beta", to);
    assert!(w.file("Beta.md").contains("status: doing"));
    assert!(!w.app.notes[Path::new("Beta.md")].text.contains("status: done"));
    assert_eq!(w.board_text(), FLOW, "the board is not changed either");
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("not moved")), "the user is told");
}

// spec: BOARD-021
#[test]
fn board_021_a_drop_on_a_lane_without_a_property_pins_the_card_and_leaves_its_note_alone() {
    let mut w = flow("drop-pin-only");
    let before = w.file("Beta.md");
    let to = w.lane_top("Todo");
    w.drag_drop("Beta", to);
    assert_eq!(w.file("Beta.md"), before, "the note is not changed");
    assert!(w.board_text().contains("## Todo\nquery: #todo\n- [[Beta]]\n"), "{}", w.board_text());
    assert_eq!(w.lane("Todo"), strs(&["Beta", "Alpha"]), "pinned on top, where it was dropped");
    assert_eq!(w.lane("Doing"), strs(&["Gamma"]), "a pinned card is only in the lanes that pin it (BOARD-032)");
}

// spec: BOARD-022
#[test]
fn board_022_a_drop_inside_a_manual_lane_rewrites_its_link_list() {
    let mut w = flow("drop-reorder");
    assert_eq!(w.lane("Manual"), strs(&["Pinned One", "Sketch"]));
    let to = w.at("Pinned One");
    w.drag_drop("Sketch", to);
    assert_eq!(w.lane("Manual"), strs(&["Sketch", "Pinned One"]));
    assert!(w.board_text().ends_with("## Manual\n- [[Sketch.excalidraw]]\n- [[Pinned One]]\n"), "{}", w.board_text());
    // dropping a card where it is changes nothing
    let text = w.board_text();
    let to = w.at("Sketch");
    w.drag_drop("Sketch", to);
    assert_eq!(w.board_text(), text);
}

// spec: BOARD-022
#[test]
fn board_022_a_card_from_another_lane_is_inserted_where_it_is_dropped_and_its_note_stays() {
    let mut w = flow("drop-manual");
    let before = w.file("Alpha.md");
    let to = w.at("Sketch");
    w.drag_drop("Alpha", to);
    assert_eq!(w.lane("Manual"), strs(&["Pinned One", "Alpha", "Sketch"]));
    assert!(w.board_text().ends_with("- [[Pinned One]]\n- [[Alpha]]\n- [[Sketch.excalidraw]]\n"), "{}", w.board_text());
    assert_eq!(w.file("Alpha.md"), before, "a manual lane has no property to write");
    assert_eq!(w.lane("Todo"), Vec::<String>::new(), "the card is in the lane that pins it only");
}

// spec: BOARD-022, BOARD-019
#[test]
fn board_022_a_card_taken_out_of_a_lane_that_pins_it_is_unpinned_there() {
    let mut w = flow("drop-unpin");
    let to = w.lane_top("Done");
    w.drag_drop("Pinned One", to);
    assert!(w.file("Pinned One.md").contains("status: done"), "{}", w.file("Pinned One.md"));
    assert!(!w.board_text().contains("Pinned One]]"), "no pin is left in the lane it came from: {}", w.board_text());
    assert_eq!(w.lane("Manual"), strs(&["Sketch"]));
    // it matches Todo (#todo) too, and the first lane that matches takes a card (BOARD-009)
    assert_eq!(w.lane("Todo"), strs(&["Alpha", "Pinned One"]));
    assert_eq!(w.lane("Done"), Vec::<String>::new());
}

// spec: BOARD-022
#[test]
fn board_022_a_card_dropped_among_the_pinned_cards_of_a_lane_with_a_property_is_pinned_there_too() {
    let board = "---\ntype: board\n---\n# Flow\n\n## Todo\nquery: status:todo\n\n## Doing\nquery: status:doing\n- [[Gamma]]\n";
    let mut w = W::new("drop-among", &[("Board.md", board), ("Beta.md", "---\nstatus: doing\n---\nB\n"), ("Gamma.md", "---\nstatus: doing\n---\nG\n"), ("Delta.md", "---\nstatus: todo\n---\nD\n"), ("Eps.md", "---\nstatus: todo\n---\nE\n")]);
    assert_eq!(w.lane("Doing"), strs(&["Gamma", "Beta"]));
    // below the pinned card: the property only
    let to = w.at("Beta") + vec2(0.0, 60.0);
    w.drag_drop("Delta", to);
    assert!(w.file("Delta.md").contains("status: doing"));
    assert_eq!(w.board_text(), board, "dropped below the pinned cards: not pinned");
    // on the pinned card: the property and the pin, in front of it
    let to = w.at("Gamma");
    w.drag_drop("Eps", to);
    assert!(w.file("Eps.md").contains("status: doing"));
    assert!(w.board_text().ends_with("query: status:doing\n- [[Eps]]\n- [[Gamma]]\n"), "{}", w.board_text());
    assert_eq!(w.lane("Doing"), strs(&["Eps", "Gamma", "Beta", "Delta"]));
}

// Dependencies (BOARD-023 to BOARD-029)

const DEP_BOARD: &str = "---\ntype: board\n---\n# Plan\n\n## Chain\nquery: #t\nsort: dependencies\n\n## Finished\nquery: #fin\ndone: true\n\n## Other\nquery: #o\n";

/// A chain A, then B and C (C is due first), then D; E waits for a note of the done lane; G waits for
/// a note that is on no lane.
const DEP_FILES: [(&str, &str); 10] = [
    ("Board.md", DEP_BOARD),
    ("A.md", "---\nstatus: doing\ndue: 2030-01-02\n---\n#t\n"),
    ("B.md", "---\nblocked-by: [[A]]\ndue: 2030-01-05\n---\n#t\n"),
    ("C.md", "---\nblocked-by: [[A]]\ndue: 2030-01-03\n---\n#t\n"),
    ("D.md", "---\nblocked-by: [[B]], [[C]]\n---\n#t\n"),
    ("Fin.md", "#fin\n"),
    ("E.md", "---\nblocked-by: [[Fin]]\n---\n#o\n"),
    ("G.md", "---\nblocked-by: [[Elsewhere]]\n---\n#o\n"),
    ("Elsewhere.md", "Nothing\n"),
    ("H.md", "#o\n"),
];

impl W {
    fn dep(&self, title: &str) -> super::deps::Dep {
        let lanes = self.app.boards.shown.lanes();
        lanes.iter().flat_map(|l| &l.cards).find(|c| c.title == title).unwrap_or_else(|| panic!("no card {title}")).dep.clone()
    }

    fn blocked(&self, titles: &[&str]) -> Vec<bool> {
        titles.iter().map(|t| self.dep(t).blocked()).collect()
    }

    fn set_note(&mut self, name: &str, text: &str) {
        self.app.vault.as_mut().unwrap().set_note_text(Path::new(name), text);
        self.settle();
    }

    fn count(&self, text: &str) -> usize {
        self.main().filter(|(t, _)| t == text).count()
    }
}

// spec: BOARD-023, BOARD-024
#[test]
fn board_023_a_card_is_blocked_while_a_blocker_is_not_done_by_status_or_by_a_done_lane() {
    let mut w = W::new("dep-badge", &DEP_FILES);
    let all = ["A", "B", "C", "D", "E", "G"];
    // E waits for Fin, which is in the done lane; G for a note that is not done
    assert_eq!(w.blocked(&all), [false, true, true, true, false, true]);
    assert_eq!(w.count("blocked"), 4, "the badge is on B, C, D and G: {:?}", w.all_text());
    // a blocker with status: done
    w.set_note("A.md", "---\nstatus: done\ndue: 2030-01-02\n---\n#t\n");
    assert_eq!(w.blocked(&all), [false, false, false, true, false, true], "D still waits for B and C");
    assert_eq!(w.count("blocked"), 2);
    // a blocker that comes into the done lane
    w.set_note("Elsewhere.md", "#fin\n");
    assert_eq!(w.blocked(&["G"]), [false]);
    assert_eq!(w.dep("G").blockers.len(), 1, "it still names its blocker, for the arrow");
    // the lane no longer marked done: E and G wait again
    let text = w.board_text().replace("done: true\n", "");
    w.set_note("Board.md", &text);
    assert_eq!(w.blocked(&["E", "G"]), [true, true]);
    assert_eq!(w.dep("A").dependants.len(), 2, "B and C name A");
}

// spec: BOARD-023
#[test]
fn board_023_start_is_a_chip_when_card_shows_has_it() {
    let board = "---\ntype: board\n---\n## L\nquery: #t\n";
    let mut w = W::new("dep-start", &[("Board.md", board), ("S.md", "---\nstart: 2030-03-04\n---\n#t\n")]);
    assert!(w.painted("start 2030-03-04"), "{:?}", w.all_text());
    w.set_note("Board.md", "---\ntype: board\ncard-shows: due\n---\n## L\nquery: #t\n");
    assert!(!w.painted("start 2030-03-04"));
}

// spec: BOARD-026, BOARD-027
#[test]
fn board_026_a_lane_sorted_by_dependencies_comes_in_order_with_step_numbers() {
    let w = W::new("dep-order", &DEP_FILES);
    // C is due before B: it comes first among the cards of step 2
    assert_eq!(w.lane("Chain"), strs(&["A", "C", "B", "D"]));
    let steps: Vec<Option<String>> = ["A", "C", "B", "D"].iter().map(|t| w.dep(t).step.and_then(|s| s.label())).collect();
    assert_eq!(steps, [Some("1".into()), Some("2a".into()), Some("2b".into()), Some("3".into())]);
    for label in ["step 1", "step 2a", "step 2b", "step 3"] {
        assert!(w.painted(label), "{label}: {:?}", w.all_text());
    }
    assert_eq!(w.dep("E").step, None, "a lane without the sort has no steps");
}

// spec: BOARD-028
#[test]
fn board_028_cards_in_a_cycle_are_marked_and_listed_after_the_ordered_cards() {
    let board = "---\ntype: board\n---\n## Chain\nquery: #t\nsort: dependencies\n";
    let w = W::new(
        "dep-cycle",
        &[("Board.md", board), ("X.md", "---\nblocked-by: [[Y]]\n---\n#t\n"), ("Y.md", "---\nblocked-by: [[X]]\n---\n#t\n"), ("Z.md", "#t\n"), ("W.md", "---\nblocked-by: [[X]]\n---\n#t\n"), ("V.md", "---\nblocked-by: [[Z]]\n---\n#t\n")],
    );
    assert_eq!(w.lane("Chain"), strs(&["Z", "V", "W", "X", "Y"]), "the ordered ones, then the cards that cannot be ordered, by title");
    assert_eq!(w.count("circular"), 2, "X and Y: {:?}", w.all_text());
    assert_eq!(w.dep("W").step.and_then(|s| s.label()), None, "W only waits for the cycle: no mark");
    assert!(w.painted("step 1") && w.painted("step 2"));
}

// spec: BOARD-025
#[test]
fn board_025_the_selected_card_gets_arrows_to_its_blockers_and_dependants() {
    let mut w = W::new("dep-arrows", &DEP_FILES);
    assert!(w.app.boards.view.arrows.is_empty(), "nothing selected: no arrows");
    w.click("B");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("B.md")));
    let arrows = w.app.boards.view.arrows.clone();
    assert_eq!(arrows.len(), 2, "A to B and B to D: {arrows:?}");
    let (a, d) = (w.at("A"), w.at("D"));
    // the cards are in one lane, one above the other (A, C, B, D): the arrows run along the lane
    assert!(arrows[0].from.y < arrows[0].to.y && arrows[0].from.y > a.y, "{arrows:?} {a:?}");
    assert!(arrows[1].from.y < arrows[1].to.y && arrows[1].to.y < d.y + 40.0, "{arrows:?} {d:?}");
    // a card without dependencies gets none
    w.key(Key::Escape, Modifiers::NONE, "");
    w.click("H");
    assert!(w.app.boards.view.arrows.is_empty(), "{:?} {:?}", w.app.boards.selected, w.app.boards.view.arrows);
    // a blocker in another lane, and the dependant of a card in the done lane
    w.key(Key::Escape, Modifiers::NONE, "");
    w.click("E");
    assert_eq!(w.app.boards.view.arrows.len(), 1);
    w.key(Key::Escape, Modifiers::NONE, "");
    w.click("Fin");
    assert_eq!(w.app.boards.view.arrows.len(), 1);
}

// spec: BOARD-025
#[test]
fn board_025_a_blocker_that_is_not_on_the_board_is_named_on_the_card_and_has_no_arrow() {
    let mut w = W::new("dep-off", &DEP_FILES);
    w.key(Key::Escape, Modifiers::NONE, "");
    w.click("G");
    assert!(w.app.boards.view.arrows.is_empty());
    assert!(w.painted("Also blocked by Elsewhere (not on this board)"), "{:?}", w.all_text());
    w.key(Key::Escape, Modifiers::NONE, "");
    w.click("E");
    assert!(!w.painted("Also blocked by"), "Fin is a card of the board");
}

// spec: BOARD-029
#[test]
fn board_029_the_preset_adds_my_dependency_chain_lane() {
    let board = "---\ntype: board\n---\n# Plan\n\n## Todo\nquery: #x\n";
    let files: Vec<(&str, &str)> = DEP_FILES.iter().map(|(n, t)| if *n == "Board.md" { (*n, board) } else { (*n, *t) }).collect();
    let mut w = W::new("dep-preset", &files);
    w.click("▾");
    w.click("My dependency chain");
    assert!(w.board_text().ends_with("## My dependency chain\nquery: blocked OR blocking\nsort: dependencies\n"), "{}", w.board_text());
    // A, Elsewhere and Fin come first; B is due after C; E and G have no due day
    assert_eq!(w.lane("My dependency chain"), strs(&["A", "Elsewhere", "Fin", "C", "B", "E", "G", "D"]));
}

// Keys and auto-scroll while dragging (BOARD-033, BOARD-034)

/// Which lane shows the card titled `title`, and its place there.
fn place_of(w: &W, title: &str) -> Option<(usize, usize)> {
    w.app.boards.shown.lanes().iter().enumerate().find_map(|(l, lane)| lane.cards.iter().position(|c| c.title == title).map(|c| (l, c)))
}

const ALT_SHIFT: Modifiers = Modifiers { alt: true, shift: true, ..Modifiers::NONE };

/// The flow board with Beta selected with the keys: Down selects Alpha, Right goes to Doing, where Beta is.
fn flow_with_beta_selected(tag: &str) -> W {
    let mut w = flow(tag);
    w.key(Key::ArrowDown, Modifiers::NONE, "");
    w.key(Key::ArrowRight, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")));
    w
}

// spec: BOARD-033
#[test]
fn board_033_the_keys_move_the_selected_card_to_the_next_and_previous_lane_like_a_drop() {
    assert_eq!(Action::BoardCardNextLane.def().id, "board.card_next_lane");
    assert_eq!(Action::BoardCardPrevLane.def().id, "board.card_prev_lane");
    let mut w = flow_with_beta_selected("key-move");
    assert_eq!(w.app.keymap.display(Action::BoardCardNextLane), "Alt+Shift+→");
    // next: Doing to Done, which says status: done
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert!(w.file("Beta.md").contains("status: done"), "{}", w.file("Beta.md"));
    assert_eq!(place_of(&w, "Beta"), Some((2, 0)));
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")), "the card stays selected");
    assert_eq!(w.board_text(), FLOW, "a lane with a property pins nothing");
    // next again: Later, whose drop: line says status: later
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert!(w.file("Beta.md").contains("status: later"));
    // next: Manual has no property: the card is pinned after the pinned cards, its note stays
    let note = w.file("Beta.md");
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert_eq!(w.file("Beta.md"), note);
    assert!(w.board_text().ends_with("- [[Pinned One]]\n- [[Sketch.excalidraw]]\n- [[Beta]]\n"), "{}", w.board_text());
    assert_eq!(place_of(&w, "Beta"), Some((4, 2)));
    // the last lane: nothing more
    let text = w.board_text();
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert_eq!((w.board_text(), w.file("Beta.md")), (text, note));
    // previous: back to Later, unpinned in Manual
    w.key(Key::ArrowLeft, ALT_SHIFT, "");
    assert_eq!(w.board_text(), FLOW);
    assert_eq!(place_of(&w, "Beta"), Some((3, 0)));
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")));
}

// spec: BOARD-033
#[test]
fn board_033_the_first_lane_has_no_previous_one_and_without_a_selection_nothing_moves() {
    let mut w = flow("key-edges");
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    w.key(Key::ArrowLeft, ALT_SHIFT, "");
    assert!(w.app.boards.selected.is_none());
    assert_eq!((w.board_text(), w.file("Alpha.md"), w.file("Beta.md")), (FLOW.to_string(), FLOW_FILES[1].1.to_string(), FLOW_FILES[2].1.to_string()), "no selection: nothing moves");
    w.key(Key::ArrowDown, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Alpha.md")));
    w.key(Key::ArrowLeft, ALT_SHIFT, "");
    assert_eq!((w.file("Alpha.md"), place_of(&w, "Alpha")), (FLOW_FILES[1].1.to_string(), Some((0, 0))), "Alpha is in the first lane");
}

// spec: BOARD-033
#[test]
fn board_033_the_keys_do_nothing_while_a_text_field_or_the_peek_has_the_keyboard() {
    let mut w = flow_with_beta_selected("key-focus");
    w.app.boards.view.rename(1, "Doing");
    w.settle();
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert!(w.file("Beta.md").contains("status: doing"), "the lane name field has the keyboard");
    w.key(Key::Escape, Modifiers::NONE, "");
    w.settle();
    w.click("Beta");
    assert!(w.app.boards.peek.is_some(), "a click opens the peek");
    w.click("---\nstatus: doing\nmodified: 2020-01-01T10:00:00+00:00\n---\nBeta body\n");
    assert!(w.hl.ctx().memory(|m| m.focused().is_some()), "a click into the peek's editor gives it the keyboard");
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert!(w.file("Beta.md").contains("status: doing"), "the editor of the peek has the keyboard");
    assert_eq!(place_of(&w, "Beta"), Some((1, 0)));
}

// spec: BOARD-033
#[test]
fn board_033_the_keys_can_be_changed() {
    let mut w = flow_with_beta_selected("key-rebind");
    w.app.keymap.set(Action::BoardCardNextLane, Binding::parse("Alt+Shift+Down").unwrap()).unwrap();
    w.key(Key::ArrowRight, ALT_SHIFT, "");
    assert!(w.file("Beta.md").contains("status: doing"), "the old key does nothing now");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")), "an unbound Alt+Shift+arrow is no selection move");
    w.key(Key::ArrowDown, ALT_SHIFT, "");
    assert!(w.file("Beta.md").contains("status: done"));
}

/// A board of one manual lane of 30 cards, in a window that shows about half of them.
fn long_lane(tag: &str) -> W {
    let mut board = String::from("---\ntype: board\n---\n# Long\n\n## Long\n");
    let mut files = vec![];
    for n in 0..30 {
        board.push_str(&format!("- [[C{n:02}]]\n"));
        files.push((format!("C{n:02}.md"), format!("card {n}\n")));
    }
    files.push(("Board.md".to_string(), board));
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let mut w = W::new(tag, &refs);
    w.hl.set_size(vec2(1500.0, 600.0));
    w.settle();
    w
}

// spec: BOARD-034
#[test]
fn board_034_a_card_held_at_the_bottom_of_a_long_lane_scrolls_it_and_drops_below_the_visible_cards() {
    let mut w = long_lane("auto-vertical");
    assert!(!w.painted("C29") || w.at("C29").y > 576.0, "the last card is out of view");
    let from = w.at("C00");
    let to = Pos2::new(from.x, 555.0);
    w.drag("C00", to);
    w.hl.frames(&mut w.app, 60);
    let offset = w.app.boards.view.lane_offsets[0];
    assert!(offset > 300.0, "the lane scrolled down while the card was held at its bottom: {offset}");
    w.drop_at(to);
    let at = place_of(&w, "C00").expect("still a card").1;
    assert!(at > 15, "dropped far below the cards that were visible: {at}");
}

// spec: BOARD-034
#[test]
fn board_034_a_card_held_at_the_top_scrolls_back_up_and_one_in_the_middle_scrolls_nothing() {
    let mut w = long_lane("auto-up");
    let from = w.at("C00");
    let low = Pos2::new(from.x, 555.0);
    w.drag("C00", low);
    w.hl.frames(&mut w.app, 60);
    let down = w.app.boards.view.lane_offsets[0];
    assert!(down > 300.0);
    let mid = Pos2::new(from.x, 300.0);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(mid)]);
    w.hl.frames(&mut w.app, 20);
    assert_eq!(w.app.boards.view.lane_offsets[0], down, "away from the edges nothing scrolls");
    let top = Pos2::new(from.x, w.at("Long").y + 30.0);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(top)]);
    w.hl.frames(&mut w.app, 80);
    assert!(w.app.boards.view.lane_offsets[0] < down - 200.0, "scrolled up: {}", w.app.boards.view.lane_offsets[0]);
    w.drop_at(top);
    assert!(!w.app.boards.view.dragging());
}

// spec: BOARD-034
#[test]
fn board_034_a_card_held_at_the_side_of_the_board_scrolls_it_and_drops_on_a_lane_that_was_out_of_view() {
    let mut board = String::from("---\ntype: board\n---\n# Wide\n\n## L0\nquery: #a\n");
    for n in 1..12 {
        board.push_str(&format!("\n## L{n}\nquery: status:s{n}\n"));
    }
    let mut w = W::new("auto-horizontal", &[("Board.md", &board), ("Card.md", "#a\n")]);
    w.hl.set_size(vec2(1500.0, 600.0));
    w.settle();
    assert!(w.app.boards.view.board_offset == 0.0 && !w.painted("L11"), "the last lane is out of view");
    let from = w.at("Card");
    let to = Pos2::new(1205.0, from.y);
    w.drag("Card", to);
    w.hl.frames(&mut w.app, 80);
    assert!(w.app.boards.view.board_offset > 400.0, "the board scrolled right: {}", w.app.boards.view.board_offset);
    // move in from the edge, where the board stops scrolling
    let to = Pos2::new(1100.0, from.y);
    w.hl.frame(&mut w.app, vec![Event::PointerMoved(to)]);
    let (lane, _) = w.app.boards.view.over.expect("over a lane");
    assert!(lane > 4, "the pointer is over a lane that was out of view: {lane}");
    w.drop_at(to);
    assert!(w.file("Card.md").contains(&format!("status: s{lane}")), "{}", w.file("Card.md"));
}

// spec: BOARD-033
#[test]
fn board_033_arrows_with_a_modifier_do_not_move_the_selection_and_bare_arrows_do() {
    let mut w = flow_with_beta_selected("key-modifiers");
    w.key(Key::ArrowRight, Modifiers::ALT, "");
    w.key(Key::ArrowDown, Modifiers::ALT, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Beta.md")), "Alt+arrows are back / forward, not selection");
    w.key(Key::ArrowDown, Modifiers::NONE, "");
    assert_eq!(w.app.boards.selected.as_deref(), Some(Path::new("Gamma.md")), "a bare arrow still selects");
}
