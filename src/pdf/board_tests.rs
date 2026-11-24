//! A board as one PDF (BOARD-030, #604): the landscape pages of the board, then the notes of its
//! cards as a book.

use crate::pdf::board::{export_board, plan_of};
use crate::pdf::book::Item;
use crate::pdf::export::tests::{contents, hex, page_ids_and_link_targets, page_streams};
use crate::pdf::export::ExportOptions;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

fn vault_with(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
    let dir = std::env::temp_dir().join(format!("shale-board-pdf-{tag}-{}", std::process::id())).join("vault");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        std::fs::write(dir.join(name), text).unwrap();
    }
    (Vault::open(&dir).unwrap(), dir)
}

fn clean(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// The size of every page, in the order of the document.
fn boxes(bytes: &[u8]) -> Vec<(f32, f32)> {
    String::from_utf8_lossy(bytes)
        .split("/MediaBox [0 0 ")
        .skip(1)
        .map(|s| {
            let mut n = s.split([' ', ']']).filter_map(|t| t.parse::<f32>().ok());
            (n.next().unwrap(), n.next().unwrap())
        })
        .collect()
}

fn landscape(bytes: &[u8]) -> Vec<usize> {
    boxes(bytes).iter().enumerate().filter(|(_, (w, h))| w > h).map(|(i, _)| i).collect()
}

fn opts() -> ExportOptions {
    ExportOptions::for_note()
}

const BOARD: &str = "---\ntype: board\n---\n# Sprint\n\n## Todo\nquery: status:todo\n\n## Doing\nquery: status:doing\n";

fn sprint() -> Vec<(&'static str, &'static str)> {
    vec![
        ("Board.md", BOARD),
        ("Alpha.md", "---\nstatus: todo\n---\nalphabody\n"),
        ("Beta.md", "---\nstatus: todo\npriority: high\ndue: 2026-12-01\n---\nbetabody #urgent\n"),
        ("Gamma.md", "---\nstatus: doing\n---\ngammabody\n"),
        ("Other.md", "otherbody\n"),
    ]
}

// spec: BOARD-030
#[test]
fn board_030_the_board_comes_on_a_landscape_page_then_the_notes_of_the_cards_in_lane_and_card_order() {
    let (v, d) = vault_with("order", &sprint());
    let bytes = export_board(&v, Path::new("Board.md"), &opts(), None).unwrap();
    let sizes = boxes(&bytes);
    assert_eq!(landscape(&bytes), [2], "the title page and the contents, then the board on one landscape page");
    assert_eq!(sizes[2], (842.0, 595.0));
    assert!(sizes.iter().enumerate().all(|(i, s)| i == 2 || *s == (595.0, 842.0)), "the notes are portrait: {sizes:?}");
    let streams = page_streams(&bytes);
    let at = |word: &str| streams.iter().position(|s| s.contains(&hex(word))).unwrap_or_else(|| panic!("{word} is on a page"));
    assert!(at("alphabody") < at("betabody") && at("betabody") < at("gammabody"), "lane by lane, card by card");
    assert!(at("gammabody") > 2 && !streams.iter().any(|s| s.contains(&hex("otherbody"))), "a note no lane has is not in it");
    // the board page: title, lanes with their counts, the cards and their chips
    let board = &streams[2];
    for word in ["Board", "Todo", "Doing", "Alpha", "Beta", "Gamma", "2026-12-01", "high", "#urgent", "todo", "doing"] {
        assert!(board.contains(&hex(word)), "the board page says {word}");
    }
    assert!(!board.contains(&hex("otherbody")) && !board.contains(&hex("alphabody")), "no text of the notes on the board page");
    // the title page and the contents come first; page numbers run through the landscape page
    assert!(streams[0].contains(&hex("Board")) && streams[1].contains(&hex("Contents")));
    assert!(board.contains(&hex("3 / ")));
    clean(&d);
}

// spec: BOARD-030
#[test]
fn board_030_the_contents_start_with_the_board_then_the_lanes_and_their_cards_and_the_cards_are_links() {
    let (v, d) = vault_with("toc", &sprint());
    let bytes = export_board(&v, Path::new("Board.md"), &opts(), None).unwrap();
    let toc = &page_streams(&bytes)[1];
    let place = |w: &str| toc.find(&hex(w)).unwrap_or_else(|| panic!("{w} is in the contents"));
    assert!(place("Board") < place("Todo") && place("Todo") < place("Alpha") && place("Alpha") < place("Beta") && place("Beta") < place("Doing") && place("Doing") < place("Gamma"));
    let text = String::from_utf8_lossy(&bytes);
    for title in ["Board", "1 Todo", "1.1 Alpha", "1.2 Beta", "2 Doing", "2.1 Gamma"] {
        let utf16: String = title.encode_utf16().map(|u| format!("{u:04X}")).collect();
        assert!(text.contains(&format!("/Title <FEFF{utf16}>")), "a bookmark for {title}");
    }
    // each card of the board page goes to the first page of its note
    let (pages, targets) = page_ids_and_link_targets(&bytes);
    let streams = page_streams(&bytes);
    for word in ["alphabody", "betabody", "gammabody"] {
        let page = streams.iter().position(|s| s.contains(&hex(word))).unwrap();
        assert_eq!(targets.iter().filter(|t| **t == pages[page]).count(), 2, "the contents and the card of the board link to the page of {word}");
    }
    // without a table of contents there is no line for the board either
    let plain = export_board(&v, Path::new("Board.md"), &ExportOptions { toc: false, ..opts() }, None).unwrap();
    assert!(!contents(&plain).contains(&hex("Contents")));
    assert_eq!(landscape(&plain), [1], "the title page, then the board");
    clean(&d);
}

// spec: BOARD-030
#[test]
fn board_030_the_options_of_the_dialog_apply_the_board_is_landscape_whatever_the_paper_is() {
    let (v, d) = vault_with("paper", &sprint());
    let o = ExportOptions { paper: crate::pdf::export::Paper::A5, title: false, toc: false, page_numbers: false, ..opts() };
    let bytes = export_board(&v, Path::new("Board.md"), &o, None).unwrap();
    assert_eq!(boxes(&bytes)[0], (595.0, 420.0), "the board is the first page, A5 landscape");
    assert!(boxes(&bytes)[1..].iter().all(|s| *s == (420.0, 595.0)));
    let tall = ExportOptions { landscape: true, ..o };
    let bytes = export_board(&v, Path::new("Board.md"), &tall, None).unwrap();
    assert!(boxes(&bytes).iter().all(|s| *s == (595.0, 420.0)), "a landscape export has them all so");
    clean(&d);
}

// spec: BOARD-030, BOARD-009
#[test]
fn board_030_a_card_in_several_lanes_is_on_the_board_in_each_and_has_its_note_once() {
    let board = "---\ntype: board\ncards: every-lane\n---\n## Todo\nquery: status:todo\n\n## Also\nquery: priority:high\n";
    let (v, d) = vault_with("every", &[("Board.md", board), ("Alpha.md", "---\nstatus: todo\n---\nalphabody\n"), ("Beta.md", "---\nstatus: todo\npriority: high\n---\nbetabody\n")]);
    let bytes = export_board(&v, Path::new("Board.md"), &opts(), None).unwrap();
    let streams = page_streams(&bytes);
    let board_page = &streams[landscape(&bytes)[0]];
    assert_eq!(board_page.matches(&hex("Beta")).count(), 2, "both lanes show the card");
    assert_eq!(streams.iter().filter(|s| s.contains(&hex("betabody"))).count(), 1, "its note is printed once");
    let toc = &streams[1];
    assert_eq!(toc.matches(&hex("Beta")).count(), 1, "one line in the contents");
    assert!(!toc.contains(&hex("Also")), "a lane whose cards are all printed earlier has no part");
    let plan = plan_of(&v, Path::new("Board.md")).unwrap();
    assert_eq!(plan.lines().iter().map(|l| l.title.as_str()).collect::<Vec<_>>(), ["Todo", "Alpha", "Beta"]);
    clean(&d);
}

// spec: BOARD-030
#[test]
fn board_030_more_lanes_than_fit_go_on_further_pages_left_to_right() {
    let mut board = String::from("---\ntype: board\n---\n");
    for i in 1..=8 {
        board.push_str(&format!("## Lane {i}\nquery: status:s{i}\n\n"));
    }
    let (v, d) = vault_with("lanes", &[("Board.md", &board), ("One.md", "---\nstatus: s1\n---\nx\n"), ("Eight.md", "---\nstatus: s8\n---\nx\n")]);
    let o = ExportOptions { title: false, toc: false, new_page_each: false, ..opts() };
    let width = 842.0 - o.margins()[1] - o.margins()[3];
    let fit = ((width + 10.0) / 200.0).floor() as usize;
    assert!((2..8).contains(&fit), "{fit} columns fit");
    let bytes = export_board(&v, Path::new("Board.md"), &o, None).unwrap();
    let groups = 8usize.div_ceil(fit);
    let pages = landscape(&bytes);
    assert_eq!(pages, (0..groups).collect::<Vec<_>>(), "a page for each group of columns, before the notes");
    let streams = page_streams(&bytes);
    for i in 1..=8 {
        assert_eq!(pages.iter().filter(|p| streams[**p].contains(&hex(&format!("Lane {i}")))).count(), 1, "lane {i} is on one page");
    }
    assert!(streams[0].contains(&hex("Lane 1")) && streams[groups - 1].contains(&hex("Lane 8")), "left to right");
    assert!(streams[0].contains(&hex("One")) && streams[groups - 1].contains(&hex("Eight")));
    assert!(streams[1].contains(&hex("Board (continued)")), "the next page says it goes on");
    clean(&d);
}

// spec: BOARD-030
#[test]
fn board_030_more_cards_than_fit_a_page_go_on_the_next_page_of_the_same_columns() {
    let mut files: Vec<(String, String)> = vec![("Board.md".into(), "---\ntype: board\n---\n## Long\nquery: status:long\n\n## Short\nquery: status:short\n".into())];
    for i in 0..60 {
        files.push((format!("n{i:02}.md"), "---\nstatus: long\n---\nx\n".into()));
    }
    files.push(("Lone.md".into(), "---\nstatus: short\n---\nx\n".into()));
    let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
    let (v, d) = vault_with("cards", &refs);
    let o = ExportOptions { title: false, toc: false, ..opts() };
    let bytes = export_board(&v, Path::new("Board.md"), &o, None).unwrap();
    let pages = landscape(&bytes);
    assert_eq!(pages, (0..pages.len()).collect::<Vec<_>>(), "the board pages come first");
    assert!((3..=10).contains(&pages.len()), "{} pages", pages.len());
    let streams = page_streams(&bytes);
    for i in 0..60 {
        let name = format!("n{i:02}");
        assert_eq!(pages.iter().filter(|p| streams[**p].contains(&hex(&name))).count(), 1, "{name} is on one page of the board");
    }
    assert_eq!(pages.iter().filter(|p| streams[**p].contains(&hex("Lone"))).count(), 1, "the card of the short lane is not repeated");
    assert!(streams[0].contains(&hex("n00")) && !streams[0].contains(&hex("n59")) && streams[pages.len() - 1].contains(&hex("n59")), "in order");
    assert!(pages[1..].iter().all(|p| streams[*p].contains(&hex("Long (continued)")) && streams[*p].contains(&hex("Short (continued)"))), "the columns go on, and say so");
    assert!(!streams[0].contains(&hex("(continued)")));
    // the notes follow: 60 + 1 cards and two lanes
    assert!(boxes(&bytes).len() > pages.len() + 61);
    clean(&d);
}

// spec: BOARD-030, PDF-001
#[test]
fn board_030_a_drawing_card_is_a_vector_picture_on_the_board_and_its_note_page_has_the_drawing() {
    let board = "---\ntype: board\n---\n## Pinned\n- [[Sketch.excalidraw]]\n";
    let plain = "---\ntype: board\n---\n## Pinned\n- [[Note]]\n";
    let (v, d) = vault_with("drawing", &[("Board.md", board), ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE), ("Note.md", "words\n")]);
    let (v2, d2) = vault_with("drawing-plain", &[("Board.md", plain), ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE), ("Note.md", "words\n")]);
    let with = export_board(&v, Path::new("Board.md"), &opts(), None).unwrap();
    let without = export_board(&v2, Path::new("Board.md"), &opts(), None).unwrap();
    let paths = |bytes: &[u8]| page_streams(bytes)[landscape(bytes)[0]].matches(" m ").count();
    assert!(paths(&with) > paths(&without) + 3, "the drawing is made of paths: {} against {}", paths(&with), paths(&without));
    assert!(!String::from_utf8_lossy(&with).contains("/Subtype /Image"), "no raster picture");
    let streams = page_streams(&with);
    assert!(streams[1].contains(&hex("Sketch")), "a line in the contents");
    let last = streams.last().unwrap();
    assert!(last.matches(" m ").count() > 3, "the page of the drawing's note draws it as paths too");
    clean(&d);
    clean(&d2);
}

// spec: BOARD-030
#[test]
fn board_030_a_lane_with_a_bad_query_shows_the_reason_and_has_no_cards_and_no_part() {
    let board = "---\ntype: board\n---\n## Broken\nquery: (status:todo\n\n## Good\nquery: status:todo\n";
    let (v, d) = vault_with("bad", &[("Board.md", board), ("Alpha.md", "---\nstatus: todo\n---\nalphabody\n")]);
    let bytes = export_board(&v, Path::new("Board.md"), &opts(), None).unwrap();
    let streams = page_streams(&bytes);
    let board_page = &streams[landscape(&bytes)[0]];
    assert!(board_page.contains(&hex("Broken")) && board_page.contains(&hex("Query: ")), "the column has the reason");
    assert!(streams[1].contains(&hex("Good")) && !streams[1].contains(&hex("Broken")));
    assert_eq!(streams.iter().filter(|s| s.contains(&hex("alphabody"))).count(), 1);
    clean(&d);
}

// spec: BOARD-030
#[test]
fn board_030_a_board_without_lanes_or_cards_is_a_page_without_notes() {
    let (v, d) = vault_with("empty", &[("None.md", "---\ntype: board\n---\n# Nothing yet\n"), ("Cards.md", "---\ntype: board\n---\n## Todo\nquery: status:todo\n\n## Blank\n"), ("Plain.md", "not a board\n")]);
    let o = ExportOptions { page_numbers: false, ..opts() };
    let none = export_board(&v, Path::new("None.md"), &o, None).unwrap();
    assert_eq!(boxes(&none).len(), 2, "the title page and the board");
    assert!(page_streams(&none)[1].contains(&hex("This board has no lanes.")));
    let cards = export_board(&v, Path::new("Cards.md"), &o, None).unwrap();
    assert_eq!(boxes(&cards).len(), 2, "no cards: no notes and no contents");
    assert_eq!(landscape(&cards), [1]);
    for word in ["Todo", "Blank"] {
        assert!(page_streams(&cards)[1].contains(&hex(word)), "the empty lane has its column");
    }
    assert!(export_board(&v, Path::new("Plain.md"), &o, None).unwrap_err().contains("not a board"));
    let plan = plan_of(&v, Path::new("Cards.md")).unwrap();
    assert!(plan.items.is_empty());
    clean(&d);
}

// spec: BOARD-030, BOARD-010
#[test]
fn board_030_the_cards_show_the_chips_card_shows_chooses_and_a_note_with_export_false_is_not_printed() {
    let board = "---\ntype: board\ncard-shows: status\n---\n## Todo\nquery: status:todo\n";
    let (v, d) = vault_with("shows", &[("Board.md", board), ("Beta.md", "---\nstatus: todo\npriority: high\ndue: 2026-12-01\n---\nbetabody\n"), ("Hidden.md", "---\nstatus: todo\nexport: false\n---\nhiddenbody\n")]);
    let bytes = export_board(&v, Path::new("Board.md"), &opts(), None).unwrap();
    let streams = page_streams(&bytes);
    let board_page = &streams[landscape(&bytes)[0]];
    assert!(board_page.contains(&hex("todo")) && !board_page.contains(&hex("2026-12-01")) && !board_page.contains(&hex("high")), "the status only");
    assert!(board_page.contains(&hex("Hidden")), "a card that is left out of the notes is still on the board");
    assert!(!streams.iter().any(|s| s.contains(&hex("hiddenbody"))) && streams.iter().any(|s| s.contains(&hex("betabody"))));
    clean(&d);
}

// spec: BOARD-030
#[test]
fn board_030_the_export_counts_its_notes_and_can_be_stopped() {
    let (v, d) = vault_with("progress", &sprint());
    let progress = crate::pdf::book_export::Progress::default();
    export_board(&v, Path::new("Board.md"), &opts(), Some(&progress)).unwrap();
    // two parts and three cards
    assert_eq!((progress.total.load(std::sync::atomic::Ordering::SeqCst), progress.done.load(std::sync::atomic::Ordering::SeqCst)), (5, 5));
    let stopped = crate::pdf::book_export::Progress::default();
    stopped.cancel.cancel();
    assert!(export_board(&v, Path::new("Board.md"), &opts(), Some(&stopped)).unwrap_err().contains("cancelled"));
    let plan = plan_of(&v, Path::new("Board.md")).unwrap();
    assert!(matches!(&plan.items[0], Item::Part(p) if p.title == "Todo" && p.items.len() == 2));
    clean(&d);
}
