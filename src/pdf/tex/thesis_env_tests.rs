//! The sample thesis against the real LuaLaTeX and biber of the container (`tests/tex-env/`,
//! ignored unless asked for: `cargo test -- --ignored tex_env`): TEX-048.

use super::book::{write_book, BookRequest};
use super::compile_tests::container_engine;
use super::pdf_text::page_texts;
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient};
use super::thesis_tests::{sample_in, three_chapters};
use super::writer_tests::fields;
use crate::pdf::export::ExportOptions;

/// A set with one book kind of KOMA-Script, which the image has: the thesis syntax does not depend on
/// the templates of a university, so the test needs no download.
fn thesis_set() -> SetInfo {
    thesis_set_in(Language::De)
}

/// The same set with the language of its class.
fn thesis_set_in(language: Language) -> SetInfo {
    let kind = KindManifest::parse(
        "name = \"Book\"\nmapping = \"book\"\nclass = \"scrbook\"\npreamble = [\"\\\\usepackage[{language}]{babel}\"]\n[[fields]]\nkey = \"title\"\ncommand = \"title\"\nrequired = true\n[[fields]]\nkey = \"author\"\ncommand = \"author\"\n",
    )
    .unwrap();
    let meta = SetMeta { name: "Thesis".into(), version: None, imported_at: "2026-11-25".into(), source: Source::Local { path: "x".into() }, logo_file: None };
    let settings = SetSettings { language, ..SetSettings::default() };
    SetInfo { id: "thesis".into(), dir: std::env::temp_dir().join("shale-thesis-test-set-never-read"), meta, settings, kinds: vec![("book".into(), kind)], problems: vec![], logo: LogoStatus::NotNeeded, department_logo: None }
}

// spec: TEX-048, TEX-044, TEX-045, TEX-046, TEX-047, TEX-043
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_thesis_sample_compiles_and_every_reference_resolves() {
    crate::env_lock::hold();
    let set = thesis_set();
    let mut opts = ExportOptions::for_note();
    opts.thesis.list_of_figures = true;
    opts.thesis.list_of_tables = true;
    let doc = sample_in(&set, "book", &opts);
    let engine = container_engine(&mounted_root());
    let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
    assert!(out.output.warnings.iter().all(|w| !w.contains("stale")), "the numbers settled: {:?}", out.output.warnings);
    let text: String = page_texts(out.output.pdf).concat().chars().filter(|c| !c.is_whitespace()).collect();
    for want in [
        // numbers per chapter, in the language of the note
        "Abbildung1.1",
        "Abbildung2.1",
        "Tabelle1.1",
        "Tabelle2.1",
        "Gleichung(1.1)",
        "Gleichung(2.1)",
        "f(x)={",
        "(2.2)",
        "Abschnitt2.1",
        // citations as the style writes them, and the bibliography with its entries
        "(Lamport1994,S.12)",
        "Literaturverzeichnis",
        "LiterateProgramming",
        "Addison-Wesley",
        // the lists
        "Abbildungsverzeichnis",
        "Tabellenverzeichnis",
        "ErgebnissedesLaufs",
        "Laufzeiten",
        "ada@host.org",
    ] {
        assert!(text.contains(want), "{want} is not in the PDF text:\n{text}\n--- main.tex:\n{}", doc.tex);
    }
    assert!(!text.contains("??") && !text.contains("Rerun"), "an unresolved reference or citation:\n{text}");
}

// spec: BOOK-020, BOOK-021
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_three_chapter_book_has_numbers_per_chapter_cross_references_and_one_bibliography() {
    crate::env_lock::hold();
    let set = thesis_set_in(Language::En);
    let (v, dir) = three_chapters("book-env", "citation-style: numeric\n", &[]);
    let plan = crate::pdf::book::plan(&v, std::path::Path::new("Thesis")).unwrap();
    let kind = &set.kinds[0].1;
    let doc = write_book(&BookRequest { vault: &v, plan: &plan, opts: &ExportOptions::for_note(), set: &set, kind, fields: &fields(&[("author", "Ada")]), fonts: None }).unwrap_or_else(|e| panic!("{e}"));
    let _ = std::fs::remove_dir_all(dir);
    assert!(doc.warnings.is_empty(), "{:?}", doc.warnings);
    let engine = container_engine(&mounted_root());
    let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
    assert!(out.output.warnings.iter().all(|w| !w.contains("stale")), "the numbers settled: {:?}", out.output.warnings);
    let text: String = page_texts(out.output.pdf).concat().chars().filter(|c| !c.is_whitespace()).collect();
    for want in [
        // numbers per chapter in reading order, and the references of one chapter to the labels of the others
        "Figure1.1:Firstplot",
        "Figure2.1:Secondplot",
        "Table3.1:Runsoftheday",
        "(3.1)",
        "SeeFigure2.1,andTable3.1ofChapter3.",
        "BacktoFigure1.1and1.1,",
        "BackinChapter2,Equation(3.1)andFigure2.1.SeeSection3.1.",
        // `knuth1984` is cited first (by the top note), so it is [1] in the numeric style
        "Thebookstartswith[1].",
        "Cited[2].",
        "References",
        "LiterateProgramming",
    ] {
        assert!(text.contains(want), "{want} is not in the PDF text:\n{text}\n--- main.tex:\n{}", doc.tex);
    }
    assert_eq!(text.matches("LiterateProgramming").count(), 1, "an entry is printed once, however often it is cited:\n{text}");
    assert_eq!(text.matches("Addison-Wesley").count(), 1, "{text}");
    assert!(!text.contains("??") && !text.contains("Rerun"), "an unresolved reference or citation:\n{text}");
}
