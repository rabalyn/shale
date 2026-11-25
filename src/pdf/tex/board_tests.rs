//! A board through a LaTeX template (BOARD-031): the landscape pages of the board, the parts and
//! chapters after them in the order of the built-in export, the escaping, a failed query, an empty
//! board. Against LuaLaTeX itself: `board_env_tests.rs`.

use super::board::*;
use super::sets::SetSettings;
use super::writer::Document;
use super::writer_tests::{fields, kind, tuda_set};
use crate::board::resolve::resolve;
use crate::pdf::export::ExportOptions;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

pub(super) fn vault(tag: &str, files: &[(&str, &[u8])]) -> (Vault, PathBuf) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("shale-texboard-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    (Vault::open(&dir).unwrap(), dir)
}

fn write_as(v: &Vault, kind_id: &str, opts: &ExportOptions) -> Result<Document, String> {
    let set = tuda_set(SetSettings::default());
    let board = resolve(v, Path::new("Board.md"))?;
    write_board(&BoardRequest { vault: v, board: &board, opts, set: &set, kind: kind(&set, kind_id), fields: &fields(&[("author", "Ada")]), fonts: None })
}

fn tex_of(v: &Vault, kind_id: &str) -> String {
    write_as(v, kind_id, &ExportOptions::for_note()).unwrap_or_else(|e| panic!("{e}")).tex
}

fn at(tex: &str, what: &str) -> usize {
    tex.find(what).unwrap_or_else(|| panic!("{what} is not in\n{tex}"))
}

/// The groups of lanes of the board: each is a table of its own.
fn board_tables(tex: &str) -> usize {
    tex.matches("\\begin{longtable}").count()
}

const SPRINT: &str = "---\ntype: board\n---\n# Sprint\n\n## Todo\nquery: status:todo\n\n## Doing\nquery: status:doing\n";

fn sprint() -> Vec<(&'static str, &'static [u8])> {
    vec![("Board.md", SPRINT.as_bytes()), ("Alpha.md", b"---\nstatus: todo\npriority: high\n---\nalphabody\n"), ("Beta.md", b"---\nstatus: doing\n---\nbetabody\n"), ("Gamma.md", b"---\nstatus: doing\nexport: false\n---\ngammabody\n")]
}

// spec: BOARD-031, BOARD-030
#[test]
fn board_031_the_board_is_on_landscape_pages_and_the_cards_notes_follow_as_parts_and_chapters() {
    let (v, d) = vault("order", &sprint());
    let tex = tex_of(&v, "thesis");
    // the board is between the switch to landscape and the one back, before the contents and the parts
    assert_eq!(tex.matches("\n\\shaleLandscape\n").count(), 1);
    assert_eq!(tex.matches("\n\\shalePortrait\n").count(), 1);
    let (begin, end, toc, todo, doing) = (at(&tex, "\n\\shaleLandscape\n"), at(&tex, "\n\\shalePortrait\n"), at(&tex, "\\tableofcontents"), at(&tex, "\\part{Todo}"), at(&tex, "\\part{Doing}"));
    assert!(at(&tex, "\\maketitle") < begin && begin < end && end < toc && toc < todo && todo < doing, "{tex}");
    // the lanes are columns with their cards, the chips of card-shows follow the title
    let board = &tex[begin..end];
    assert!(board.contains("{\\large\\bfseries Board}"), "{board}");
    // one table, a column for each lane, the head of the first page and the head of the next ones
    assert_eq!(board.matches("\\begin{longtable}{@{}p{0.32\\linewidth}@{\\hspace{0.02\\linewidth}}p{0.32\\linewidth}@{}}").count(), 1, "{board}");
    assert!(board.contains("{\\bfseries Todo}") && board.contains("{\\bfseries Doing}") && board.contains("{\\bfseries Todo (continued)}"), "{board}");
    assert!(at(board, "\\endfirsthead") < at(board, "\\endhead") && at(board, "\\endhead") < at(board, "Alpha"), "{board}");
    assert!(at(board, "{\\bfseries Doing}") < at(board, "\\endfirsthead"), "{board}");
    // row 1: the first card of each lane
    assert!(at(board, "Alpha") < at(board, "Beta") && !board[at(board, "Alpha")..at(board, "Beta")].contains("\\\\"), "{board}");
    assert!(board.contains("todo \\textperiodcentered{} high") || board.contains("todo") && board.contains("high"), "{board}");
    // the cards link to their chapters; the note left out with export: false stays on the board only
    assert!(board.contains("\\hyperlink{shale-2}{Alpha}") && board.contains("\\hyperlink{shale-4}{Beta}"), "{board}");
    assert!(board.contains("Gamma") && !board.contains("\\hyperlink{shale-5}"), "{board}");
    assert!(tex.contains("alphabody") && tex.contains("betabody") && !tex.contains("gammabody"), "{tex}");
    assert!(at(&tex, "\\chapter{Alpha}") < at(&tex, "\\part{Doing}") && at(&tex, "\\part{Doing}") < at(&tex, "\\chapter{Beta}"));
    // the board is the first line of the contents
    assert!(at(&tex, "\\addcontentsline{toc}{chapter}{Board}") < end, "{tex}");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031
#[test]
fn board_031_an_article_has_the_board_in_the_contents_as_a_section_and_the_lanes_as_sections() {
    let (v, d) = vault("article", &sprint());
    let tex = tex_of(&v, "report");
    assert!(tex.contains("\\addcontentsline{toc}{section}{Board}") && tex.contains("\n\\shaleLandscape\n"), "{tex}");
    let opts = ExportOptions { toc: false, ..ExportOptions::for_note() };
    let tex = write_as(&v, "report", &opts).unwrap().tex;
    assert!(!tex.contains("\\addcontentsline{toc}{section}{Board}") && !tex.contains("\\tableofcontents") && tex.contains("\n\\shaleLandscape\n"), "{tex}");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031
#[test]
fn board_031_a_card_of_several_lanes_is_one_chapter_in_the_first_lane() {
    let board = "---\ntype: board\ncards: every-lane\n---\n# Sprint\n\n## All\nquery: status:doing OR status:todo\n\n## Doing\nquery: status:doing\n";
    let (v, d) = vault("every", &[("Board.md", board.as_bytes()), ("Alpha.md", b"---\nstatus: todo\n---\nalphabody\n"), ("Beta.md", b"---\nstatus: doing\n---\nbetabody\n")]);
    let tex = tex_of(&v, "thesis");
    assert_eq!(tex.matches("betabody").count(), 1, "{tex}");
    assert_eq!(tex.matches("\\chapter{Beta}").count(), 1);
    assert!(!tex.contains("\\part{Doing}"), "the second lane has nothing left: no part\n{tex}");
    // on the board page the view's second lane still lists the card
    assert_eq!(tex.matches("\\hyperlink{shale-3}{Beta}").count(), 2, "{tex}");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031
#[test]
fn board_031_everything_that_comes_from_the_vault_is_escaped() {
    let board = "---\ntype: board\ncard-shows: status\n---\n# Q&A\n\n## 50% {done} #1 \\input{x}\nquery: status:a_b\n";
    let (v, d) = vault("escape", &[("Board.md", board.as_bytes()), ("R&D_plan $5 ~ ^.md", b"---\nstatus: a_b\n---\ntext\n")]);
    let tex = tex_of(&v, "thesis");
    let page = &tex[at(&tex, "\n\\shaleLandscape\n")..at(&tex, "\n\\shalePortrait\n")];
    assert!(page.contains("50\\% \\{done\\} \\#1 \\textbackslash{}input\\{x\\}"), "{page}");
    assert!(page.contains("{R\\&D\\_plan \\$5 \\textasciitilde{} \\textasciicircum{}}"), "{page}");
    assert!(page.contains("{\\footnotesize a\\_b}"), "{page}");
    assert!(!page.contains("\\input{x}"), "{page}");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031
#[test]
fn board_031_a_lane_whose_query_fails_shows_the_reason_and_has_no_cards() {
    let board = "---\ntype: board\n---\n# Sprint\n\n## Broken\nquery: (status:doing\n\n## Good\nquery: status:todo\n";
    let (v, d) = vault("broken", &[("Board.md", board.as_bytes()), ("Alpha.md", b"---\nstatus: todo\n---\nalphabody\n")]);
    let tex = tex_of(&v, "thesis");
    let page = &tex[at(&tex, "\n\\shaleLandscape\n")..at(&tex, "\n\\shalePortrait\n")];
    let broken = &page[at(page, "{\\bfseries Broken}")..at(page, "{\\bfseries Good}")];
    assert!(broken.contains("\\itshape Query: ") && !broken.contains("\\fbox"), "{broken}");
    assert!(!tex.contains("\\part{Broken}") && tex.contains("\\part{Good}"), "{tex}");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031
#[test]
fn board_031_a_board_without_lanes_or_cards_is_the_board_page_only() {
    let (v, d) = vault("nolanes", &[("Board.md", b"---\ntype: board\n---\n# Empty\n")]);
    let tex = tex_of(&v, "thesis");
    assert!(tex.contains("This board has no lanes.") && tex.contains("{\\large\\bfseries Board}") && !tex.contains("\\chapter") && !tex.contains("\\part"), "{tex}");
    assert_eq!(board_tables(&tex), 0, "no lanes, no table");
    let (v2, d2) = vault("nocards", &[("Board.md", b"---\ntype: board\n---\n# Empty\n\n## Todo\nquery: status:todo\n")]);
    let tex = tex_of(&v2, "thesis");
    assert!(tex.contains("{\\bfseries Todo}") && !tex.contains("\\chapter") && !tex.contains("\\part"), "{tex}");
    let _ = std::fs::remove_dir_all(d);
    let _ = std::fs::remove_dir_all(d2);
}

// spec: BOARD-031
#[test]
fn board_031_more_than_three_lanes_make_more_groups_of_landscape_pages() {
    let mut board = String::from("---\ntype: board\n---\n# Wide\n");
    for n in 1..=5 {
        board.push_str(&format!("\n## Lane{n}\nquery: status:l{n}\n"));
    }
    let notes: Vec<(String, Vec<u8>)> = (1..=5).map(|n| (format!("N{n}.md"), format!("---\nstatus: l{n}\n---\nbody{n}\n").into_bytes())).collect();
    let mut files: Vec<(&str, &[u8])> = vec![("Board.md", board.as_bytes())];
    files.extend(notes.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let (v, d) = vault("groups", &files);
    let tex = tex_of(&v, "thesis");
    assert_eq!(board_tables(&tex), 2, "three lanes and two");
    assert!(tex.contains("{\\large\\bfseries Board}") && tex.contains("{\\large\\bfseries Board (continued)}"), "{tex}");
    assert_eq!(tex.matches("\n\\clearpage\n").count(), 1, "the second group starts a page");
    assert!(at(&tex, "Lane3") < at(&tex, "Lane4"), "{tex}");
    assert_eq!(tex.matches("\\addcontentsline{toc}{chapter}{Board}").count(), 1, "the board is listed once");
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031
#[test]
fn board_031_the_cards_of_a_group_are_the_rows_of_one_table_that_latex_breaks_with_continued_heads() {
    let mut board = String::from("---\ntype: board\n---\n# Long\n\n## Many\nquery: status:many\nsort: title\n\n## Few\nquery: status:few\n");
    board.push('\n');
    let mut notes: Vec<(String, Vec<u8>)> = (1..=30).map(|n| (format!("Card {n:02}.md"), b"---\nstatus: many\n---\nx\n".to_vec())).collect();
    notes.push(("Solo.md".into(), b"---\nstatus: few\n---\nx\n".to_vec()));
    let mut files: Vec<(&str, &[u8])> = vec![("Board.md", board.as_bytes())];
    files.extend(notes.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let (v, d) = vault("long", &files);
    let tex = tex_of(&v, "thesis");
    // no page height is estimated: one table, 30 rows (the shorter lane has empty cells), LaTeX breaks it
    assert_eq!(board_tables(&tex), 1, "{tex}");
    let table = &tex[at(&tex, "\\begin{longtable}")..at(&tex, "\\end{longtable}")];
    let body = &table[at(table, "\\endhead") + 8..];
    assert_eq!(body.matches("\\\\[12pt]\n").count(), 30, "{body}");
    assert!(!table.contains("\\clearpage"), "{table}");
    assert_eq!(table.matches("{\\bfseries Many}").count(), 1, "the first head");
    assert_eq!(table.matches("{\\bfseries Many (continued)}").count(), 1, "the head of the next pages");
    assert_eq!(table.matches("{\\bfseries Few}").count(), 1);
    assert_eq!(table.matches("{\\bfseries Few (continued)}").count(), 1);
    assert_eq!(table.matches("{\\large\\bfseries Board}").count(), 1);
    assert_eq!(table.matches("{\\large\\bfseries Board (continued)}").count(), 1);
    // every card is on the board exactly once, and in order
    for n in 1..=30 {
        assert_eq!(tex.matches(&format!("{{Card {n:02}}}")).count(), 2, "Card {n}: its link on the board and its chapter");
    }
    assert!(at(&tex, "{Card 01}") < at(&tex, "{Card 30}"));
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031, PDF-001
#[test]
fn board_031_a_drawing_card_is_a_chapter_with_its_drawing_embedded_as_a_pdf_file() {
    let board = "---\ntype: board\n---\n# B\n\n## Pinned\n- [[Sketch.excalidraw]]\n- [[Note]]\n";
    let (v, d) = vault("drawing", &[("Board.md", board.as_bytes()), ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE.as_bytes()), ("Note.md", b"words\n")]);
    let doc = write_as(&v, "thesis", &ExportOptions::for_note()).unwrap();
    assert!(doc.tex.contains("\\chapter{Sketch}") && doc.tex.contains("\\includegraphics"), "{}", doc.tex);
    assert_eq!(doc.files.len(), 1, "the drawing as one vector PDF");
    assert!(doc.files[0].0.ends_with(".pdf") && doc.files[0].1.starts_with(b"%PDF"));
    assert!(at(&doc.tex, "\\chapter{Sketch}") < at(&doc.tex, "\\chapter{Note}"));
    let _ = std::fs::remove_dir_all(d);
}

// spec: BOARD-031, BOOK-013
#[test]
fn board_031_a_kind_that_cannot_hold_a_book_and_a_note_that_is_no_board_are_refused_in_words() {
    let (v, d) = vault("refuse", &sprint());
    let why = write_as(&v, "beamer", &ExportOptions::for_note()).unwrap_err();
    assert!(why.contains("cannot hold a folder"), "{why}");
    let (v2, d2) = vault("nonboard", &[("Board.md", b"just a note\n")]);
    let why = write_as(&v2, "thesis", &ExportOptions::for_note()).unwrap_err();
    assert!(why.contains("is not a board"), "{why}");
    let _ = std::fs::remove_dir_all(d);
    let _ = std::fs::remove_dir_all(d2);
}

// spec: BOARD-031
#[test]
fn board_031_the_landscape_comes_from_geometry_for_every_class_and_pdflscape_is_not_used() {
    let (v, d) = vault("geometry", &sprint());
    for kind in ["thesis", "report"] {
        let tex = tex_of(&v, kind);
        // geometry is there (the class's own, or loaded with `pass`, which leaves the layout alone), the page box is
        // set along with the paper, and the layout of the class comes back
        assert!(tex.contains("\\IfPackageLoadedTF{geometry}{}{\\usepackage[pass]{geometry}}"), "{tex}");
        assert!(tex.contains("\\newgeometry") && tex.contains("\\loadgeometry{shale-portrait}") && tex.contains("\\pagewidth=\\paperwidth \\pageheight=\\paperheight"), "{tex}");
        assert!(!tex.contains("pdflscape") && !tex.contains("{landscape}"), "{tex}");
        assert!(at(&tex, "\n\\shaleLandscape\n") < at(&tex, "\n\\shalePortrait\n"));
    }
    let _ = std::fs::remove_dir_all(d);
}
