//! A folder's book against the real LuaLaTeX of the container (`tests/tex-env/`, ignored unless
//! asked for: `cargo test -- --ignored tex_env`): a small book with two parts, folder notes, a
//! picture of the same name in both parts and links between the chapters compiles through the
//! thesis and the report kinds of the imported tuda-ci set (BOOK-017).

use super::book::*;
use super::compile_tests::container_engine;
use super::pdf_text::{links_inside, outline_entries, page_texts};
use super::sets::*;
use super::sets_env_tests::{mounted_root, patient, the_set};
use super::writer_tests::{fields, temp_vault};
use crate::pdf::export::ExportOptions;
use std::path::Path;

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(80, 40, image::Rgba([rgb[0], rgb[1], rgb[2], 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// The book as a kind of the imported set, compiled: the text of its PDF without spaces, its
/// bookmarks, its links inside the book and the document.
fn compile_book(kind_id: &str) -> (String, usize, usize, String) {
    // the root of the flow is the caller (the export dialog, DEV-028)
    let _flow = tracing::info_span!("flow.tex_book", peer = "export").entered();
    let (red, blue) = (png([200, 30, 30]), png([30, 30, 200]));
    let files: Vec<(&str, &[u8])> = vec![
        ("index.md", b"---\ntitle: The Book\nauthor: Ada Lovelace\nreviewer: [Prof. Babbage, Prof. Turing]\ndepartment: ce\n---\nIntroduction text of the book.\n"),
        ("Overview.md", b"# Overview\nOverview text, see [[Lists]] and [[Lists#Bullets|the bullets]].\n"),
        ("Writing/Writing.md", b"Writing part text.\n"),
        ("Writing/Lists.md", b"## Bullets\n- one\n- two\n\n![[pic.png]]\n\nBack to [[Overview]].\n"),
        ("Writing/pic.png", &red),
        ("Drawing/Drawing.md", b"Drawing part text.\n"),
        ("Drawing/Shapes.md", b"## Circles\nRound things & more.\n\n![[pic.png]]\n"),
        ("Drawing/pic.png", &blue),
    ];
    let (_, dir) = temp_vault("book-env", &[]);
    for (name, bytes) in &files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    let v = crate::vault::Vault::open(&dir).unwrap();
    let plan = crate::pdf::book::plan(&v, Path::new("")).unwrap();
    let set = the_set();
    let kind = &set.kinds.iter().find(|(i, _)| i == kind_id).unwrap().1;
    let doc = write_book(&BookRequest { vault: &v, plan: &plan, opts: &ExportOptions::for_note(), set: &set, kind, fields: &fields(&[]) }).unwrap_or_else(|e| panic!("{kind_id}: the writer failed: {e}"));
    assert_eq!(doc.files.len(), 2, "{kind_id}: the two pictures of the same name are two files");
    let engine = container_engine(&mounted_root());
    let owned: Vec<(&str, &[u8])> = doc.files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    let out = compile_with_set(&engine, &set, "main.tex", &doc.tex, &owned, patient()).unwrap_or_else(|e| panic!("{kind_id}: LuaLaTeX failed: {e}\n{}\n--- main.tex:\n{}", e.log_tail, doc.tex));
    let _ = std::fs::remove_dir_all(dir);
    let pdf = out.output.pdf;
    assert!(pdf.starts_with(b"%PDF"), "{kind_id}: a PDF");
    let text: String = page_texts(pdf.clone()).concat().chars().filter(|c| !c.is_whitespace()).collect();
    (text, outline_entries(&pdf), links_inside(&pdf, "shale-"), doc.tex)
}

fn check(kind_id: &str) {
    let (text, bookmarks, links, tex) = compile_book(kind_id);
    for want in [
        "TheBook",
        "AdaLovelace",
        "Introductiontextofthebook.",
        "Drawing",
        "Drawingparttext.",
        "Shapes",
        "Circles",
        "Roundthings&more.",
        "Overview",
        "Overviewtext,seeListsandthebullets.",
        "Writing",
        "Writingparttext.",
        "Lists",
        "Bullets",
        "BacktoOverview.",
    ] {
        assert!(text.contains(want), "{kind_id}: {want} is not in {text}\n--- main.tex:\n{tex}");
    }
    // the contents list the chapters besides their pages
    assert!(text.matches("Shapes").count() >= 2 && text.matches("Lists").count() >= 3, "{kind_id}: the contents in {text}\n{tex}");
    // two parts, three chapters and their two headings at least
    assert!(bookmarks >= 7, "{kind_id}: {bookmarks} bookmarks\n{tex}");
    // Lists, the bullets and Overview: links that go to their places in the PDF (BOOK-016)
    assert_eq!(links, 3, "{kind_id}: links inside the book\n{tex}");
}

// spec: BOOK-017, BOOK-014, BOOK-015, BOOK-016
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_folder_compiles_as_a_thesis_with_parts_and_chapters() {
    crate::env_lock::hold();
    check("thesis");
}

// spec: BOOK-017, BOOK-014, BOOK-015, BOOK-016, DEV-028
#[test]
#[ignore = "needs the container: tests/tex-env/tex-env.sh build"]
fn tex_env_a_folder_compiles_as_a_report_with_sections() {
    crate::env_lock::hold();
    // the precondition, outside the recording: the template set is imported once per run, by whichever
    // test comes first, so the diagram must not depend on that order (DEV-026)
    let _ = the_set();
    let forest = crate::dataflow::record(|| check("report"));
    crate::dataflow::verify_flow(&forest, "tex_book", &crate::dataflow::doc("tex-book.md"));
}
