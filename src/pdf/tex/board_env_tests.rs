//! A board against the real LuaLaTeX of the container (`tests/tex-env/`, ignored unless asked for:
//! `cargo test -- --ignored tex_env_board_031`): a board of three lanes with a long one compiles through the
//! thesis kind of the imported tuda-ci set, with `pdflscape`, landscape pages first (BOARD-031).

use super::board::*;
use super::board_tests::vault;
use super::compile_tests::container_engine;
use super::pdf_text::{outline_entries, page_texts};
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient, the_set};
use super::writer_tests::fields;
use crate::board::resolve::resolve;
use crate::pdf::export::ExportOptions;
use std::path::Path;

// spec: BOARD-031
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_board_031_a_board_compiles_with_landscape_pages_parts_and_chapters() {
    crate::env_lock::hold();
    let board = "---\ntype: board\n---\n# Sprint & Plan\n\n## Todo\nquery: status:todo\nsort: title\n\n## Doing\nquery: status:doing\n\n## Broken\nquery: (status:x\n";
    let many: Vec<(String, Vec<u8>)> = (1..=24).map(|n| (format!("Todo card {n:02}.md"), format!("---\nstatus: todo\n---\nText of todo {n:02}.\n").into_bytes())).collect();
    let mut files: Vec<(&str, &[u8])> = vec![("Board.md", board.as_bytes()), ("Work.md", b"---\nstatus: doing\n---\nWork text, 50% of #1.\n")];
    files.extend(many.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let (v, dir) = vault("env", &files);
    let set = the_set();
    let kind = &set.kinds.iter().find(|(i, _)| i == "thesis").unwrap().1;
    let resolved = resolve(&v, Path::new("Board.md")).unwrap();
    let doc = write_board(&BoardRequest { vault: &v, board: &resolved, opts: &ExportOptions::for_note(), set: &set, kind, fields: &fields(&[("author", "Ada Lovelace"), ("reviewer", "Prof. Babbage"), ("department", "ce")]) }).unwrap();
    let engine = container_engine(&mounted_root());
    let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
    let _ = std::fs::remove_dir_all(dir);
    let pdf = out.output.pdf;
    assert!(pdf.starts_with(b"%PDF"));
    let pages = page_texts(pdf.clone());
    let text: String = pages.concat().chars().filter(|c| !c.is_whitespace()).collect();
    for want in ["Board", "Todo", "Doing", "Todocard01", "Todocard24", "Work", "Textoftodo24.", "Worktext,50%of#1.", "Query:", "(continued)"] {
        assert!(text.contains(want), "{want} is not in {text}\n--- main.tex:\n{}", doc.tex);
    }
    // the board comes first (after the title page): the long lane continues on two more landscape pages
    let board_pages: Vec<usize> = pages.iter().enumerate().filter(|(_, p)| p.contains("Todo") && p.contains("Doing") && p.contains("Broken")).map(|(i, _)| i).collect();
    assert_eq!(board_pages.len(), 3, "the pages of the board in {pages:?}\n{}", doc.tex);
    assert!(board_pages[0] <= 1 && board_pages.windows(2).all(|w| w[1] == w[0] + 1), "{board_pages:?}");
    assert!(pages[board_pages[1]].contains("(continued)") && pages[board_pages[2]].contains("(continued)"));
    // the board and the two lanes with cards, their cards as chapters
    assert!(outline_entries(&pdf) >= 4, "bookmarks");
}
