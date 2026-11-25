//! The writer against the real LuaLaTeX of the container (`tests/tex-env/`, ignored unless asked
//! for: `cargo test -- --ignored tex_env`): one rich note with every construct is exported through
//! every one of the 11 kinds of the imported tuda-ci set, in German and in English, and the PDF
//! must exist, have pages and say what the note says (TEX-008 to TEX-015, TEX-027). A failure
//! names the kind and the language and carries the document and the end of the log.

use super::book::{write_book, BookRequest};
use super::compile::Options;
use super::compile_tests::container_engine;
use super::fonts::FontInput;
use super::pdf_text::{fills_with, font_names, links_inside, outline_entries, page_texts, pictures_drawn, squashed};
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient, the_set, the_set_with_a_department_logo};
use super::writer::*;
use super::writer_tests::{fields, rich_assets, temp_vault, RICH_NOTE};
use crate::fonts::resolve::Values;
use crate::fonts::scan::{scan_vault, Catalogue};
use crate::pdf::export::ExportOptions;
use std::path::Path;
use std::sync::Arc;

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
    let doc = write(&Request { vault: &v, rel: Path::new("Rich sample note.md"), text: note, opts: &opts, set: &set, kind, fields: &fields(given), fonts: None }).unwrap_or_else(|e| panic!("{what}: the writer failed: {e}"));
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
    has("A deep heading", "a heading of the deepest level");
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
            crate::env_lock::hold();
            check_rich($kind, Language::$lang);
        }
    )*};
}

kind_tests! {
    // spec: TEX-008, TEX-010, TEX-019, TEX-030, TEX-031, TEX-032
    tex_env_rich_note_as_pub_en: "pub", En;
    tex_env_rich_note_as_pub_de: "pub", De;
    tex_env_rich_note_as_report_en: "report", En;
    // spec: TEX-010
    tex_env_rich_note_as_report_de: "report", De;
    // spec: TEX-010
    tex_env_rich_note_as_exercise_en: "exercise", En;
    tex_env_rich_note_as_exercise_de: "exercise", De;
    tex_env_rich_note_as_thesis_en: "thesis", En;
    tex_env_rich_note_as_thesis_de: "thesis", De;
    tex_env_rich_note_as_phd_en: "phd", En;
    tex_env_rich_note_as_phd_de: "phd", De;
    tex_env_rich_note_as_beamer_en: "beamer", En;
    // spec: TEX-012
    tex_env_rich_note_as_beamer_de: "beamer", De;
    // spec: TEX-013
    tex_env_rich_note_as_letter_en: "letter", En;
    tex_env_rich_note_as_letter_de: "letter", De;
    // spec: TEX-014
    tex_env_rich_note_as_poster_en: "poster", En;
    tex_env_rich_note_as_poster_de: "poster", De;
    tex_env_rich_note_as_sciposter_en: "sciposter", En;
    // spec: TEX-014
    tex_env_rich_note_as_sciposter_de: "sciposter", De;
    // spec: TEX-014
    tex_env_rich_note_as_leaflet_en: "leaflet", En;
    tex_env_rich_note_as_leaflet_de: "leaflet", De;
    tex_env_rich_note_as_announcement_en: "announcement", En;
    // spec: TEX-014
    tex_env_rich_note_as_announcement_de: "announcement", De;
}

// spec: TEX-035, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_note_exported_with_a_department_logo_shows_it_in_every_kind_that_has_a_place_and_not_when_switched_off() {
    crate::env_lock::hold();
    let set = the_set_with_a_department_logo();
    let engine = container_engine(&mounted_root());
    let note = "# A note\n\nSome text.\n\n## A part\n\nMore text.\n";
    let mut failed = vec![];
    for (id, kind) in &set.kinds {
        let (v, dir) = temp_vault("dept", &[("Plan.md", note.as_bytes())]);
        let pictures = |wanted: bool| {
            let opts = ExportOptions { department_logo: wanted, ..ExportOptions::for_note() };
            let doc = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: note, opts: &opts, set: &set, kind, fields: &fields(&[AUTHOR, &[("reviewer", "Prof. Grace Hopper")]].concat()), fonts: None })
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

// spec: TEX-012, TEX-027, TEX-019
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_slides_have_a_page_for_the_title_and_one_for_every_frame() {
    crate::env_lock::hold();
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

// spec: TEX-013, TEX-027, DEV-028, TEX-019
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_letter_has_its_recipient_subject_and_text() {
    crate::env_lock::hold();
    let note = "---\nrecipient: |\n  Dr. Grace Hopper\n  Navy Street 1\nsubject: About the compiler\nsender: Ada Lovelace\nsender_address: Analytical Street 2\nopening: Dear Dr. Hopper,\nclosing: Yours sincerely\ndate: 2026-11-17\n---\nThe body of the letter with a table:\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
    // the precondition, outside the recording: the template set is imported once per run, by whichever
    // test comes first, so the diagram must not depend on that order (DEV-026)
    let _ = the_set();
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

// spec: TEX-009, TEX-011, TEX-027, TEX-019
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_thesis_has_chapters_its_fields_and_a_table_of_contents() {
    crate::env_lock::hold();
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
    crate::env_lock::hold();
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
    crate::env_lock::hold();
    let note = "# Rich sample note\n\nWater H<sub>2</sub>O, x<sup>2</sup> and <u>under *lined*</u> text[^1] with $\\frac{a}{b} + \\sum_{i=1}^n \\binom{n}{i} + \\mathbb{R}$ and $$\\dfrac{x}{y}$$ inline, $\\input{/etc/passwd}$ and $\\text{50% ^^5cinput{x}}$.\n\n$$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix} = \\begin{cases} x & y \\\\ z & \\text{else} \\end{cases}\n$$\n\n```math\n\\sqrt[3]{x^2} + \\operatorname{rank} A\n```\n\n| A | B |\n| --- | --- |\n| one<br>two[^2] | z |\n\n[^1]: A *footnote*\n    with a second line and $x^2$.\n[^2]: In a cell.\n\nLast paragraph\n";
    for kind in ["pub", "report", "beamer", "letter", "poster", "sciposter", "leaflet"] {
        let (pdf, doc) = export(note, kind, Language::En, AUTHOR, false);
        let text = squashed(pdf);
        for want in ["Afootnote", "withasecondline", "Inacell", "one", "two", "Lastparagraph", "else", "\\input{/etc/passwd}", "50%^^5c"] {
            assert!(text.contains(want), "{kind}: {want} is not in {text}\n{}", doc.tex);
        }
    }
}

// spec: TEX-030, TEX-027, TEX-019
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_long_code_breaks_across_pages_on_its_box_in_articles_and_books() {
    crate::env_lock::hold();
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

// spec: TEX-015, TEX-041, TEX-042, TEX-008, TEX-027
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_media_cards_emoji_tags_and_quotes_compile_with_their_look() {
    crate::env_lock::hold();
    let note = "# Plan\n\nA task 📅 2026-12-01 about #ideas.\n\n> quoted words\n\n![[tone-cover.mp3]]\n\n![[tone.wav]]\n";
    let media: Vec<(String, Vec<u8>)> = ["tone-cover.mp3", "tone.wav"].iter().map(|n| (n.to_string(), std::fs::read(crate::media::testdata::fixture(n)).unwrap())).collect();
    let files: Vec<(&str, &[u8])> = media.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let (v, dir) = temp_vault("media", &[&[("Plan.md", note.as_bytes())][..], &files].concat());
    let set = the_set();
    let engine = container_engine(&mounted_root());
    for kind_id in ["pub", "thesis", "beamer"] {
        let kind = &set.kinds.iter().find(|(i, _)| i == kind_id).unwrap().1;
        let doc = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: note, opts: &ExportOptions::for_note(), set: &set, kind, fields: &fields(&[AUTHOR, &[("reviewer", "Prof. A")]].concat()), fonts: None })
            .unwrap_or_else(|e| panic!("{kind_id}: {e}"));
        let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("{kind_id}: LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
        let pdf = out.output.pdf;
        let text = squashed(pdf.clone());
        for want in ["2026-12-01", "ideas", "quotedwords", "tone-cover.mp3", "tone.wav", "0:00"] {
            assert!(text.contains(want), "{kind_id}: {want} is not in {text}\n{}", doc.tex);
        }
        assert!(text.contains('\u{1F4C5}'), "{kind_id}: the calendar has a glyph of the emoji font, not a box: {text}");
        assert!(fills_with(&pdf, [149, 121, 215]), "{kind_id}: the bar of the quote");
        assert_eq!(pictures_drawn(pdf), 1, "{kind_id}: the cover of the sound");
    }
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------- fonts (TEX-054 to TEX-058, #658)

/// A set with one article kind of KOMA-Script, which the image has: the fonts do not depend on the
/// templates of a university, so the test needs no download.
fn font_test_set() -> SetInfo {
    let kind = KindManifest::parse("name = \"Article\"\nmapping = \"article\"\nclass = \"scrartcl\"\n[[fields]]\nkey = \"title\"\ncommand = \"title\"\nrequired = true\n[[fields]]\nkey = \"author\"\ncommand = \"author\"\n").unwrap();
    let meta = SetMeta { name: "Fonts".into(), version: None, imported_at: "2026-11-24".into(), source: Source::Local { path: "x".into() }, logo_file: None };
    SetInfo {
        id: "fonts".into(),
        dir: std::env::temp_dir().join("shale-font-test-set-never-read"),
        meta,
        settings: SetSettings::default(),
        kinds: vec![("article".into(), kind)],
        problems: vec![],
        logo: LogoStatus::NotNeeded,
        department_logo: None,
    }
}

/// A vault with Liberation Serif in its `fonts/` (the font of `assets/fonts/`, with its licence file beside it),
/// and the fonts it knows: never the machine's.
fn font_vault(tag: &str, files: &[(&str, &[u8])]) -> (crate::vault::Vault, std::path::PathBuf, FontInput) {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
    let (v, dir) = temp_vault(tag, files);
    std::fs::create_dir_all(dir.join("fonts")).unwrap();
    for f in ["LiberationSerif-Regular.ttf", "LiberationSerif-LICENSE.txt"] {
        std::fs::copy(assets.join(f), dir.join("fonts").join(f)).unwrap();
    }
    let catalogue = Catalogue { vault: Arc::new(scan_vault(&dir)), system: Arc::default(), system_ready: true, generation: 1 };
    (v, dir, FontInput { catalogue, vault: Values::default() })
}

fn compile_font_doc(doc: &Document, set: &SetInfo) -> Result<SetOutput, String> {
    let engine = container_engine(&mounted_root());
    let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    compile_with_set(&engine, set, "main.tex", &doc.tex, &owned, Options { fonts: doc.fonts.clone(), ..patient() }).map_err(|e| format!("LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex))
}

fn write_font_note(v: &crate::vault::Vault, input: &FontInput, set: &SetInfo, note: &str) -> Document {
    write(&Request { vault: v, rel: Path::new("Plan.md"), text: note, opts: &ExportOptions::for_note(), set, kind: &set.kinds[0].1, fields: &fields(&[("author", "Ada")]), fonts: Some(input) })
        .unwrap_or_else(|e| panic!("the writer failed: {e}"))
}

// spec: TEX-058, TEX-056, TEX-057, BOOK-022
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_vault_font_is_embedded_a_texlive_font_compiles_and_an_unknown_one_warns_and_retries() {
    crate::env_lock::hold();
    let set = font_test_set();
    let pdf_has = |pdf: &[u8], name: &str| font_names(pdf).iter().any(|n| n.contains(name));

    // a font file of the vault is copied into the work folder and embedded; the emoji are still drawn by their own font
    let note = "---\nprint-font: Liberation Serif\n---\n# Plan\n\nA text \u{1F4C5} 2026-12-01 with an emoji.\n";
    let (v, dir, input) = font_vault("font-env-vault", &[("Plan.md", note.as_bytes())]);
    let doc = write_font_note(&v, &input, &set, note);
    assert!(doc.tex.contains("\\setmainfont{LiberationSerif-Regular.ttf}[Path=./]"), "{}", doc.tex);
    let out = compile_font_doc(&doc, &set).unwrap_or_else(|e| panic!("{e}"));
    let names = font_names(&out.output.pdf);
    assert!(pdf_has(&out.output.pdf, "LiberationSerif"), "the vault font is embedded: {names:?}");
    assert!(pdf_has(&out.output.pdf, "Emoji"), "the emoji font is embedded: {names:?}");
    assert!(squashed(out.output.pdf.clone()).contains('\u{1F4C5}'), "the calendar is a glyph of the emoji font, not a box");
    assert!(out.output.warnings.is_empty() && doc.warnings.is_empty(), "{:?} {:?}", out.output.warnings, doc.warnings);
    assert_eq!(out.output.runs, 1);

    // the sans and mono roles too, and without the font the template's own is used
    let plain = write_font_note(&v, &input, &set, "# Plan\n\ntext\n");
    let plain_out = compile_font_doc(&plain, &set).unwrap_or_else(|e| panic!("{e}"));
    assert!(!pdf_has(&plain_out.output.pdf, "LiberationSerif"), "{:?}", font_names(&plain_out.output.pdf));

    // a name that TeX Live knows is passed through
    let note = "---\nprint-font: XCharter\nprint-font-sans: Roboto\n---\n# Plan\n\ntext\n";
    let doc = write_font_note(&v, &input, &set, note);
    assert!(doc.tex.contains("\\setmainfont{XCharter}") && doc.tex.contains("\\setsansfont{Roboto}") && doc.files.is_empty(), "{}", doc.tex);
    let out = compile_font_doc(&doc, &set).unwrap_or_else(|e| panic!("{e}"));
    assert!(pdf_has(&out.output.pdf, "XCharter") && out.output.warnings.is_empty(), "{:?} {:?}", font_names(&out.output.pdf), out.output.warnings);

    // an unknown name: the warning, the font of the template and a PDF; the vault font of the other role stays
    let note = "---\nprint-font: No_Such Font 4U\nprint-font-mono: Liberation Serif\n---\n# Plan\n\n`code` and text\n";
    let doc = write_font_note(&v, &input, &set, note);
    let out = compile_font_doc(&doc, &set).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out.output.warnings, ["the font “No_Such Font 4U” was not found by TeX; the template's font is used"]);
    assert_eq!(out.output.runs, 2, "the run that stopped, and the one without the font");
    assert!(out.output.pdf.starts_with(b"%PDF") && !pdf_has(&out.output.pdf, "No_Such"), "{:?}", font_names(&out.output.pdf));
    assert!(pdf_has(&out.output.pdf, "LiberationSerif"), "the mono font that was found stays: {:?}", font_names(&out.output.pdf));

    // two unknown names: fontspec stops at the first, so each costs a run, and the export is still a PDF with two warnings
    let note = "---\nprint-font: No Such Text\nprint-font-sans: No Such Sans\n---\n# Plan\n\ntext\n";
    let doc = write_font_note(&v, &input, &set, note);
    let out = compile_font_doc(&doc, &set).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(out.output.warnings.len(), 2, "{:?}", out.output.warnings);
    assert_eq!(out.output.runs, 3);
    assert!(out.output.pdf.starts_with(b"%PDF"));

    // a book: the top folder note sets one font for all of it, and a chapter's own is ignored
    let book_files: Vec<(&str, &[u8])> =
        vec![("Book/index.md", b"---\ntitle: Handbook\nprint-font: Liberation Serif\n---\nIntro text\n"), ("Book/One.md", b"---\nprint-font: XCharter\n---\n# One\nFirst chapter\n"), ("Book/Two.md", b"# Two\nSecond chapter\n")];
    for (name, bytes) in &book_files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let vb = crate::vault::Vault::open(&dir).unwrap();
    let plan = crate::pdf::book::plan(&vb, Path::new("Book")).unwrap();
    let doc = write_book(&BookRequest { vault: &vb, plan: &plan, opts: &ExportOptions::for_note(), set: &set, kind: &set.kinds[0].1, fields: &fields(&[("author", "Ada")]), fonts: Some(&input) }).unwrap();
    assert!(doc.warnings.iter().any(|w| w.contains("sets its own font")), "{:?}", doc.warnings);
    let out = compile_font_doc(&doc, &set).unwrap_or_else(|e| panic!("{e}"));
    assert!(pdf_has(&out.output.pdf, "LiberationSerif") && !pdf_has(&out.output.pdf, "XCharter"), "{:?}", font_names(&out.output.pdf));
    let _ = std::fs::remove_dir_all(dir);
}
