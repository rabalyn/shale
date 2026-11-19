//! The writer against the real LuaLaTeX of the container (`tests/tex-env/`, ignored unless asked
//! for: `cargo test -- --ignored tex_env`): one rich note with every construct is exported through
//! every one of the 11 kinds of the imported tuda-ci set, in German and in English, and the PDF
//! must exist, have pages and say what the note says (TEX-008 to TEX-015, TEX-027). A failure
//! names the kind and the language and carries the document and the end of the log.

use super::compile_tests::container_engine;
use super::pdf_text::{fills_with, links_inside, outline_entries, page_texts, pictures_drawn, squashed};
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient, the_set, the_set_with_a_department_logo};
use super::writer::*;
use super::writer_tests::{fields, rich_assets, temp_vault, RICH_NOTE};
use crate::pdf::export::ExportOptions;
use std::path::Path;

/// Export a note as a kind of the imported set, in a language, and compile it. Returns the PDF.
fn export(note: &str, kind_id: &str, lang: Language, given: &[(&str, &str)], toc: bool) -> (Vec<u8>, Document) {
    // the root of the flow is the caller (the export dialog, DEV-028)
    let _flow = tracing::info_span!("flow.tex_export", peer = "export").entered();
    let what = format!("kind {kind_id}, {}", lang.code());
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let (v, dir) = temp_vault("env", &[&[("Rich sample note.md", note.as_bytes())][..], &files].concat());
    let mut set = the_set();
    set.settings.language = lang;
    let kind = &set.kinds.iter().find(|(i, _)| i == kind_id).unwrap().1;
    let opts = ExportOptions { toc, ..ExportOptions::for_note() };
    let doc = write(&Request { vault: &v, rel: Path::new("Rich sample note.md"), text: note, opts: &opts, set: &set, kind, fields: &fields(given) }).unwrap_or_else(|e| panic!("{what}: the writer failed: {e}"));
    let engine = container_engine(&mounted_root());
    let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("{what}: LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
    let _ = std::fs::remove_dir_all(dir);
    assert!(out.output.pdf.starts_with(b"%PDF"), "{what}: a PDF");
    (out.output.pdf, doc)
}

const AUTHOR: &[(&str, &str)] = &[("author", "Ada Lovelace"), ("recipient", "Dr. Grace Hopper \\\\ Navy Street 1 \\\\ 12345 Arlington")];

/// What the PDF of the rich note must say, whatever the kind.
fn check_rich(kind_id: &str, lang: Language) {
    let what = format!("kind {kind_id}, {}", lang.code());
    let (pdf, doc) = export(RICH_NOTE, kind_id, lang, AUTHOR, true);
    let pages = page_texts(pdf.clone());
    assert!(!pages.is_empty(), "{what}: the PDF has no page");
    // the code on its light box, the picture in a line besides the one on its own, the jump to a heading (TEX-030 to TEX-032)
    assert!(fills_with(&pdf, super::body::CODE_PAPER.to_array()[..3].try_into().unwrap()), "{what}: the code box\n{}", doc.tex);
    assert_eq!(pictures_drawn(pdf.clone()), 2, "{what}: the picture on its own line and the one in a line\n{}", doc.tex);
    // a link that wraps at the end of a line is two link areas
    assert!((1..=2).contains(&links_inside(&pdf, "shale-h")), "{what}: the link to a heading of the note ({} links)\n{}", links_inside(&pdf, "shale-h"), doc.tex);
    let text = squashed(pdf);
    let lower = text.to_lowercase();
    let has =
        |needle: &str, what_is: &str| assert!(lower.contains(&needle.chars().filter(|c| !c.is_whitespace()).collect::<String>().to_lowercase()), "{what}: {what_is} {needle:?} is not in the PDF text:\n{text}\n--- main.tex:\n{}", doc.tex);
    has("Rich sample note", "the title");
    has("First section", "a heading");
    has("A subsection", "a heading of the next level");
    has("Käse, Größe und Ärger", "the umlauts");
    has("& % $ # _ { } ~ ^ \\ end", "the TeX specials as text");
    has("Alpha", "a table cell");
    has("a & b", "a table cell with a special character");
    has("50%", "a table cell with a percent sign");
    has("bullet two", "a list item");
    has("done task", "a task");
    has("A quoted line", "a quote");
    has("println!(\"Hi & 100%\");", "code");
    has("web link", "the text of a link");
    has("that part", "the text of a link to a heading");
    has("Another note", "the text of a link to another note");
    has("An icon", "the text around a picture in a line");
    has("pic.png", "the caption of a picture");
    has("[missing: missing.png]", "the placeholder of a missing embed");
    has("Last paragraph", "the last paragraph");
    assert!(!text.contains("secret") && !text.contains("numbered-headings"), "{what}: the front matter is not printed");
}

macro_rules! kind_tests {
    ($($name:ident: $kind:literal, $lang:ident;)*) => {$(
        // spec: TEX-008, TEX-009, TEX-012, TEX-013, TEX-014, TEX-015, TEX-027
        #[test]
        #[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
        fn $name() {
            check_rich($kind, Language::$lang);
        }
    )*};
}

kind_tests! {
    tex_env_rich_note_as_pub_en: "pub", En;
    tex_env_rich_note_as_pub_de: "pub", De;
    tex_env_rich_note_as_report_en: "report", En;
    tex_env_rich_note_as_report_de: "report", De;
    tex_env_rich_note_as_exercise_en: "exercise", En;
    tex_env_rich_note_as_exercise_de: "exercise", De;
    tex_env_rich_note_as_thesis_en: "thesis", En;
    tex_env_rich_note_as_thesis_de: "thesis", De;
    tex_env_rich_note_as_phd_en: "phd", En;
    tex_env_rich_note_as_phd_de: "phd", De;
    tex_env_rich_note_as_beamer_en: "beamer", En;
    tex_env_rich_note_as_beamer_de: "beamer", De;
    tex_env_rich_note_as_letter_en: "letter", En;
    tex_env_rich_note_as_letter_de: "letter", De;
    tex_env_rich_note_as_poster_en: "poster", En;
    tex_env_rich_note_as_poster_de: "poster", De;
    tex_env_rich_note_as_sciposter_en: "sciposter", En;
    tex_env_rich_note_as_sciposter_de: "sciposter", De;
    tex_env_rich_note_as_leaflet_en: "leaflet", En;
    tex_env_rich_note_as_leaflet_de: "leaflet", De;
    tex_env_rich_note_as_announcement_en: "announcement", En;
    tex_env_rich_note_as_announcement_de: "announcement", De;
}

// spec: TEX-035, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_note_exported_with_a_department_logo_shows_it_in_every_kind_that_has_a_place_and_not_when_switched_off() {
    let set = the_set_with_a_department_logo();
    let engine = container_engine(&mounted_root());
    let note = "# A note\n\nSome text.\n\n## A part\n\nMore text.\n";
    let mut failed = vec![];
    for (id, kind) in &set.kinds {
        let (v, dir) = temp_vault("dept", &[("Plan.md", note.as_bytes())]);
        let pictures = |wanted: bool| {
            let opts = ExportOptions { department_logo: wanted, ..ExportOptions::for_note() };
            let doc = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: note, opts: &opts, set: &set, kind, fields: &fields(&[AUTHOR, &[("reviewer", "Prof. Grace Hopper")]].concat()) })
                .unwrap_or_else(|e| panic!("kind {id}: the writer failed: {e}"));
            assert_eq!(doc.tex.contains("department_logo"), wanted && !kind.department_logo.is_empty(), "kind {id}: the lines are written only when asked for and when the kind has a place");
            let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
            compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).map(|out| pictures_drawn(out.output.pdf)).map_err(|e| format!("kind {id} (department logo {wanted}): {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex))
        };
        match (pictures(false), pictures(true)) {
            (Ok(without), Ok(with)) => {
                let has_place = !kind.department_logo.is_empty();
                if has_place && with <= without || !has_place && with != without {
                    failed.push(format!("{id}: {without} pictures without the department logo and {with} with it (the kind has a place: {has_place})"));
                }
            }
            (a, b) => failed.extend([a.err(), b.err()].into_iter().flatten()),
        }
        let _ = std::fs::remove_dir_all(dir);
    }
    assert!(failed.is_empty(), "{} of {} kinds failed:\n{}", failed.len(), set.kinds.len(), failed.join("\n=====\n"));
}

// spec: TEX-012, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_slides_have_a_page_for_the_title_and_one_for_every_frame() {
    for lang in [Language::En, Language::De] {
        let note = "# Plan\n# Part one\n## Slide A\ntext a\n## Slide B\n- x\n- y\n# Part two\n## Slide C\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
        let (pdf, doc) = export(note, "beamer", lang, AUTHOR, true);
        let pages = page_texts(pdf);
        assert_eq!(pages.len(), 1 + doc.tex.matches("\\begin{frame}").count(), "{lang:?}: the title slide and a page per frame\n{}", doc.tex);
        // the slides' theme sets the titles in capitals
        let all: String = pages.concat().to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
        assert!(all.contains("slidea") && all.contains("slideb") && all.contains("slidec"), "{lang:?}: {all}");
        // a note with only # headings: a frame for each
        let (pdf, doc) = export("# One\nfirst\n# Two\nsecond\n# Three\nthird\n", "beamer", lang, AUTHOR, true);
        assert_eq!(page_texts(pdf).len(), 4, "{lang:?}: the title slide and three frames\n{}", doc.tex);
    }
}

// spec: TEX-013, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_letter_has_its_recipient_subject_and_text() {
    let note = "---\nrecipient: |\n  Dr. Grace Hopper\n  Navy Street 1\nsubject: About the compiler\nsender: Ada Lovelace\nsender_address: Analytical Street 2\nopening: Dear Dr. Hopper,\nclosing: Yours sincerely\ndate: 2026-11-17\n---\nThe body of the letter with a table:\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
    let forest = crate::dataflow::record(|| {
        for lang in [Language::En, Language::De] {
            let (pdf, doc) = export(note, "letter", lang, &[], false);
            let text = squashed(pdf);
            for want in ["Dr.GraceHopper", "NavyStreet1", "Aboutthecompiler", "DearDr.Hopper,", "Yourssincerely", "Thebodyoftheletter"] {
                assert!(text.contains(want), "{lang:?}: {want} is not in {text}\n{}", doc.tex);
            }
        }
    });
    crate::dataflow::verify_flow(&forest, "tex_export", &crate::dataflow::doc("tex-export.md"));
}

// spec: TEX-009, TEX-011, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_thesis_has_chapters_its_fields_and_a_table_of_contents() {
    let note = "---\nauthor: Ada Lovelace\nsubtitle: A study\nreviewer: [Prof. Babbage, Prof. Turing]\ndepartment: ce\ninstitute: The Institute\n---\n# Rich sample note\n# One chapter\nText\n## A section\n### A subsection\n#### A subsubsection\n# Second chapter\nMore\n";
    for kind in ["thesis", "phd"] {
        for lang in [Language::En, Language::De] {
            let (pdf, doc) = export(note, kind, lang, &[], true);
            let pages = page_texts(pdf);
            assert!(pages.len() >= 4, "{kind} {lang:?}: title page, contents and chapters\n{}", doc.tex);
            let text: String = pages.concat().chars().filter(|c| !c.is_whitespace()).collect();
            for want in ["Onechapter", "Asection", "Asubsection", "Secondchapter", "AdaLovelace", "Astudy", "TheInstitute"] {
                assert!(text.contains(want), "{kind} {lang:?}: {want} is not in {text}\n{}", doc.tex);
            }
            // the table of contents lists the chapters (a second time, besides their pages)
            assert!(text.matches("Onechapter").count() >= 2, "{kind} {lang:?}: contents in {text}\n{}", doc.tex);
        }
    }
}

// spec: TEX-009, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_unnumbered_headings_are_still_in_the_contents_and_the_pdf_has_bookmarks_of_every_level() {
    let note = "---\nnumbered-headings: false\n---\n# A\n## B\n### C\n#### D\ntext\n";
    let (pdf, doc) = export(note, "pub", Language::En, AUTHOR, true);
    let text = squashed(pdf.clone());
    // the headings are in the text and in the contents (twice); the fourth level is in the text only
    assert!(["B", "C"].iter().all(|h| text.matches(h).count() >= 2) && text.matches('D').count() == 1, "{text}\n{}", doc.tex);
    // A, B, C and D: the fourth level has a bookmark too, though the contents stop at the third
    assert_eq!(outline_entries(&pdf), 4, "the bookmarks of every level\n{}", doc.tex);
}

// spec: TEX-029, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_underline_scripts_footnotes_breaks_in_cells_and_math_compile_in_every_family() {
    let note = "# Rich sample note\n\nWater H<sub>2</sub>O, x<sup>2</sup> and <u>under *lined*</u> text[^1] with $\\frac{a}{b} + \\sum_{i=1}^n \\binom{n}{i} + \\mathbb{R}$ and $$\\dfrac{x}{y}$$ inline, $\\input{/etc/passwd}$ and $\\text{50% ^^5cinput{x}}$.\n\n$$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix} = \\begin{cases} x & y \\\\ z & \\text{else} \\end{cases}\n$$\n\n```math\n\\sqrt[3]{x^2} + \\operatorname{rank} A\n```\n\n| A | B |\n| --- | --- |\n| one<br>two[^2] | z |\n\n[^1]: A *footnote*\n    with a second line and $x^2$.\n[^2]: In a cell.\n\nLast paragraph\n";
    for kind in ["pub", "report", "beamer", "letter", "poster", "sciposter", "leaflet"] {
        let (pdf, doc) = export(note, kind, Language::En, AUTHOR, false);
        let text = squashed(pdf);
        for want in ["Afootnote", "withasecondline", "Inacell", "one", "two", "Lastparagraph", "else", "\\input{/etc/passwd}", "50%^^5c"] {
            assert!(text.contains(want), "{kind}: {want} is not in {text}\n{}", doc.tex);
        }
    }
}

// spec: TEX-030, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_long_code_breaks_across_pages_on_its_box_in_articles_and_books() {
    let code: String = (1..=160).map(|n| format!("let line_{n} = {n}; // code\n")).collect();
    let note = format!("# Rich sample note\n\n## Code\n\n```rust\n{code}```\n\nAfter the code\n");
    for kind in ["pub", "thesis"] {
        let (pdf, doc) = export(&note, kind, Language::En, &[("author", "Ada Lovelace"), ("reviewer", "Prof. A")], false);
        let pages = page_texts(pdf.clone());
        let squash = |t: &str| t.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let first = pages.iter().position(|p| squash(p).contains("letline_1=1;")).unwrap_or_else(|| panic!("{kind}: the first line\n{}", doc.tex));
        let last = pages.iter().position(|p| squash(p).contains("letline_160=160;")).unwrap_or_else(|| panic!("{kind}: the last line\n{}", doc.tex));
        assert!(last > first, "{kind}: the code goes on on the next page ({first}, {last})");
        assert!(fills_with(&pdf, super::body::CODE_PAPER.to_array()[..3].try_into().unwrap()), "{kind}: on its box");
        assert!(squash(&pages.concat()).contains("Afterthecode"), "{kind}");
    }
}
