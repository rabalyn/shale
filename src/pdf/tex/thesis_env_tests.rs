//! The sample thesis against the real LuaLaTeX and biber of the container (`tests/tex-env/`,
//! ignored unless asked for: `cargo test -- --ignored tex_env`): TEX-048.

use super::compile_tests::container_engine;
use super::pdf_text::page_texts;
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient};
use super::thesis_tests::sample_in;
use crate::pdf::export::ExportOptions;

/// A set with one book kind of KOMA-Script, which the image has: the thesis syntax does not depend on
/// the templates of a university, so the test needs no download.
fn thesis_set() -> SetInfo {
    let kind = KindManifest::parse(
        "name = \"Book\"\nmapping = \"book\"\nclass = \"scrbook\"\npreamble = [\"\\\\usepackage[{language}]{babel}\"]\n[[fields]]\nkey = \"title\"\ncommand = \"title\"\nrequired = true\n[[fields]]\nkey = \"author\"\ncommand = \"author\"\n",
    )
    .unwrap();
    let meta = SetMeta { name: "Thesis".into(), version: None, imported_at: "2026-11-25".into(), source: Source::Local { path: "x".into() }, logo_file: None };
    let settings = SetSettings { language: Language::De, ..SetSettings::default() };
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
