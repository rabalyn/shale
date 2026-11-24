//! A board against the real LuaLaTeX of the container (`tests/tex-env/`, ignored unless asked for:
//! `cargo test -- --ignored tex_env_board_031`): a board of three lanes with a long one compiles through the
//! thesis and the report kind of the imported tuda-ci set, on real landscape pages with the header and the
//! footer of the class in place, the notes after it on portrait pages again (BOARD-031, #641); the same switch
//! in the document classes `article`, `scrartcl` and `scrbook`, which are not the tuda ones.

use super::board::*;
use super::board_tests::vault;
use super::compile::{compile, Options, WorkDir};
use super::compile_tests::container_engine;
use super::pdf_text::{outline_entries, page_layouts, page_texts};
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient, the_set};
use super::writer_tests::fields;
use crate::board::resolve::resolve;
use crate::pdf::export::ExportOptions;
use std::path::Path;

/// The pages of `pdf` that are landscape must be exactly `board_pages`, none of them rotated by the file, and
/// the page number of each of them as far from the bottom edge as on a portrait page after the board, at the
/// right end of the text line, not at a side edge.
fn assert_real_landscape_pages(pdf: &[u8], board_pages: &[usize], what: &str) {
    let layouts = page_layouts(pdf.to_vec());
    let last = *board_pages.last().unwrap();
    let number_at = |i: usize| {
        let number = (i + 1).to_string();
        layouts[i].runs.iter().find(|(t, _, _)| t.trim() == number).map(|(_, x, y)| (*x, *y))
    };
    for (i, l) in layouts.iter().enumerate() {
        assert!(!l.rotated, "{what}: page {} is rotated", i + 1);
        assert_eq!(l.width > l.height, board_pages.contains(&i), "{what}: page {} is {}x{}: only the board's pages are landscape", i + 1, l.width, l.height);
    }
    let (_, portrait_y) = (last + 1..layouts.len()).find_map(number_at).unwrap_or_else(|| panic!("{what}: a portrait page with its number"));
    for i in board_pages {
        let (x, y) = number_at(*i).unwrap_or_else(|| panic!("{what}: the page number is not on page {}: {:?}", i + 1, layouts[*i].runs));
        // PDF coordinates: y counts from the bottom edge
        assert!((y - portrait_y).abs() < 1.0, "{what}: the page number of page {} is {y} from the bottom edge, on a portrait page {portrait_y}", i + 1);
        assert!(x > layouts[*i].width * 0.4 && x < layouts[*i].width * 0.95, "{what}: the page number of page {} is at x {x} of {}", i + 1, layouts[*i].width);
    }
}

// spec: BOARD-031
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_board_031_a_board_compiles_with_real_landscape_pages_parts_and_chapters() {
    crate::env_lock::hold();
    let board = "---\ntype: board\n---\n# Sprint & Plan\n\n## Todo\nquery: status:todo\nsort: title\n\n## Doing\nquery: status:doing\n\n## Broken\nquery: (status:x\n";
    let many: Vec<(String, Vec<u8>)> = (1..=24).map(|n| (format!("Todo card {n:02}.md"), format!("---\nstatus: todo\n---\nText of todo {n:02}.\n").into_bytes())).collect();
    let mut files: Vec<(&str, &[u8])> = vec![("Board.md", board.as_bytes()), ("Work.md", b"---\nstatus: doing\n---\nWork text, 50% of #1.\n")];
    files.extend(many.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
    let (v, dir) = vault("env", &files);
    let set = the_set();
    let resolved = resolve(&v, Path::new("Board.md")).unwrap();
    let engine = container_engine(&mounted_root());
    // the thesis kind (tudapub with its own margins) and the report kind (tudapub with geometry's margins)
    for id in ["thesis", "report"] {
        let kind = &set.kinds.iter().find(|(i, _)| i == id).unwrap().1;
        let doc = write_board(&BoardRequest { vault: &v, board: &resolved, opts: &ExportOptions::for_note(), set: &set, kind, fields: &fields(&[("author", "Ada Lovelace"), ("reviewer", "Prof. Babbage"), ("department", "ce")]) }).unwrap();
        let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("{id}: LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
        let pdf = out.output.pdf;
        // for a look at the pages: target/board-641-<kind>.pdf
        let _ = std::fs::write(Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("target/board-641-{id}.pdf")), &pdf);
        assert!(pdf.starts_with(b"%PDF"));
        let pages = page_texts(pdf.clone());
        let text: String = pages.concat().chars().filter(|c| !c.is_whitespace()).collect();
        for want in ["Board", "Todo", "Doing", "Todocard01", "Todocard24", "Work", "Textoftodo24.", "Worktext,50%of#1.", "Query:", "(continued)"] {
            assert!(text.contains(want), "{id}: {want} is not in {text}\n--- main.tex:\n{}", doc.tex);
        }
        // the board comes first (after the title page): the long lane continues on further landscape pages
        let board_pages: Vec<usize> = pages.iter().enumerate().filter(|(_, p)| p.contains("Todo") && p.contains("Doing") && p.contains("Broken")).map(|(i, _)| i).collect();
        assert!(board_pages.len() >= 3, "{id}: the pages of the board in {pages:?}\n{}", doc.tex);
        assert!(board_pages[0] <= 1 && board_pages.windows(2).all(|w| w[1] == w[0] + 1), "{id}: {board_pages:?}");
        assert!(board_pages[1..].iter().all(|i| pages[*i].contains("(continued)")));
        // real landscape pages (#641), the notes after them portrait again, the footer of the class at the bottom
        assert_real_landscape_pages(&pdf, &board_pages, id);
        // the board and the two lanes with cards, their cards as chapters
        assert!(outline_entries(&pdf) >= 4, "{id}: bookmarks");
    }
    let _ = std::fs::remove_dir_all(dir);
}

// spec: BOARD-031
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_board_031_the_switch_gives_landscape_pages_in_classes_that_are_not_the_tuda_ones() {
    crate::env_lock::hold();
    let engine = container_engine(&mounted_root());
    let fill = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(40);
    for class in ["article", "scrartcl", "scrbook"] {
        let work = WorkDir::new().unwrap();
        let tex = format!("\\documentclass{{{class}}}\n{PREAMBLE}\\begin{{document}}\nPortrait. {fill}\n\\shaleLandscape\nLandscape. {fill}\n\\clearpage\nLandscape again.\n\\shalePortrait\nPortrait again. {fill}\n\\end{{document}}\n");
        work.write("main.tex", tex.as_bytes()).unwrap();
        let out = compile(&engine, work, "main.tex", &Options::default()).unwrap_or_else(|e| panic!("{class}: {e}\n{}", e.log_tail));
        let layouts = page_layouts(out.pdf);
        let shape: Vec<bool> = layouts.iter().map(|l| l.width > l.height).collect();
        // one portrait page, the landscape ones, then portrait again
        let first = shape.iter().position(|l| *l).unwrap_or_else(|| panic!("{class}: no landscape page in {shape:?}"));
        let after = shape.iter().rposition(|l| *l).unwrap();
        assert!(first == 1 && after >= 2 && shape[first..=after].iter().all(|l| *l) && after + 1 < shape.len() && shape[after + 1..].iter().all(|l| !l), "{class}: {shape:?}");
        assert!(layouts.iter().all(|l| !l.rotated), "{class}: no page is rotated");
    }
}
