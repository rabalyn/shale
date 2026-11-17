//! A folder's book through a LaTeX template (BOOK-013…BOOK-016): the structure commands by depth
//! for the book and the article family, the folder notes' and the top note's text, the anchors and
//! the links between the chapters, the embeds of different folders, the fields from the top folder
//! note, and the kinds that cannot hold a folder. Against LuaLaTeX itself: `book_env_tests.rs`.

use super::book::*;
use super::dialog::TexExport;
use super::import::TUDA;
use super::sets::*;
use super::writer::Document;
use super::writer_tests::{fields, kind, tuda_set};
use crate::pdf::book;
use crate::pdf::export::ExportOptions;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

/// A vault in a temporary folder with these files (in subfolders as their names say).
fn vault(tag: &str, files: &[(&str, &[u8])]) -> (Vault, PathBuf) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("shale-texbook-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&dir);
    for (name, bytes) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    (Vault::open(&dir).unwrap(), dir)
}

/// The book of the folder `Book` as a kind of the tuda set.
fn write_as(v: &Vault, kind_id: &str, opts: &ExportOptions, given: &[(&str, &str)]) -> Result<Document, String> {
    let set = tuda_set(SetSettings::default());
    let plan = book::plan(v, Path::new("Book")).unwrap();
    write_book(&BookRequest { vault: v, plan: &plan, opts, set: &set, kind: kind(&set, kind_id), fields: &fields(given) })
}

fn tex_of(v: &Vault, kind_id: &str, opts: &ExportOptions) -> String {
    write_as(v, kind_id, opts, &[("author", "Ada")]).unwrap_or_else(|e| panic!("{e}")).tex
}

/// Where a piece of text is in the document, so tests can check the order.
fn at(tex: &str, what: &str) -> usize {
    tex.find(what).unwrap_or_else(|| panic!("{what:?} is not in\n{tex}"))
}

/// A book with an introduction, chapters in the top folder, a part with a folder note and a part inside it.
fn handbook(tag: &str) -> (Vault, PathBuf) {
    vault(
        tag,
        &[
            ("Book/index.md", b"---\ntitle: Handbook\n---\nIntro text\n## Intro head\n"),
            ("Book/Getting started.md", b"# Getting started\nStart here\n## First steps\n"),
            ("Book/Appendix.md", b"The end\n"),
            ("Book/R&D/R&D.md", b"Part intro text\n## Note on the part\n"),
            ("Book/R&D/Lists.md", b"---\nnumbered-headings: false\n---\n## Bullets\n### Deeper\n"),
            ("Book/R&D/Deep/x.md", b"## In x\n"),
        ],
    )
}

// spec: BOOK-014
#[test]
fn a_book_kind_has_parts_for_the_subfolders_chapters_for_the_notes_and_the_headings_below() {
    let (v, dir) = handbook("book");
    let tex = tex_of(&v, "thesis", &ExportOptions::for_note());
    // the reading order by name: Appendix, Getting started, R&D (Deep, Lists); the intro first
    let order = [
        "\\maketitle\n\\tableofcontents\n\\shaleanchor{shale-1}\nIntro text",
        "\\section*{Intro head}\\shaleanchor{shale-1-1}\n\n",
        "\\chapter{Appendix}\\shaleanchor{shale-2}\n\nThe end",
        "\\chapter{Getting started}\\shaleanchor{shale-3}\n\nStart here",
        "\\section{First steps}\\shaleanchor{shale-3-1}\n",
        "\\part{R\\&D}\\shaleanchor{shale-4}\n\nPart intro text",
        "\\chapter*{Note on the part}\\shaleanchor{shale-4-1}\n\n",
        "\\chapter{Deep}\\shaleanchor{shale-5}\n",
        "\\section{x}\\shaleanchor{shale-6}\n\n\\subsection{In x}\\shaleanchor{shale-6-1}\n",
        "\\chapter{Lists}\\shaleanchor{shale-7}\n\n\\section{Bullets}\\shaleanchor{shale-7-1}\n\n\\subsection{Deeper}\\shaleanchor{shale-7-2}\n",
    ];
    let places: Vec<usize> = order.iter().map(|o| at(&tex, o)).collect();
    assert!(places.windows(2).all(|w| w[0] < w[1]), "in reading order: {places:?}\n{tex}");
    assert_eq!(tex.matches("Getting started}").count(), 1, "the heading that says the chapter's title is not repeated\n{tex}");
    assert!(tex.contains("\\setcounter{tocdepth}{2}") && tex.trim_end().ends_with("\\end{document}"), "{tex}");
    // a book without subfolders has no part
    let (flat, flat_dir) = vault("flat", &[("Book/a.md", b"## A head\n"), ("Book/b.md", b"text\n")]);
    let tex = tex_of(&flat, "phd", &ExportOptions::for_note());
    assert!(!tex.contains("\\part") && tex.contains("\\chapter{a}\\shaleanchor{shale-1}") && tex.contains("\\section{A head}") && tex.contains("\\chapter{b}"), "{tex}");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(flat_dir);
}

// spec: BOOK-014
#[test]
fn an_article_kind_starts_the_same_structure_at_sections() {
    let (v, dir) = handbook("article");
    let tex = tex_of(&v, "report", &ExportOptions::for_note());
    let order = [
        "\\subsection*{Intro head}",
        "\\section{Appendix}\\shaleanchor{shale-2}",
        "\\section{Getting started}\\shaleanchor{shale-3}",
        "\\subsection{First steps}",
        "\\section{R\\&D}\\shaleanchor{shale-4}",
        "\\subsection*{Note on the part}",
        "\\subsection{Deep}\\shaleanchor{shale-5}",
        "\\subsubsection{x}\\shaleanchor{shale-6}",
        "\\paragraph{In x}\\shaleanchor{shale-6-1}",
        "\\subsection{Lists}\\shaleanchor{shale-7}",
        "\\subsubsection{Bullets}",
        "\\paragraph{Deeper}",
    ];
    let places: Vec<usize> = order.iter().map(|o| at(&tex, o)).collect();
    assert!(places.windows(2).all(|w| w[0] < w[1]), "in reading order: {places:?}\n{tex}");
    assert!(!tex.contains("\\part") && !tex.contains("\\chapter") && tex.contains("\\setcounter{tocdepth}{3}"), "{tex}");
    let _ = std::fs::remove_dir_all(dir);
}

// spec: BOOK-014, BOOK-006
#[test]
fn the_books_numbering_and_contents_decide_and_a_notes_own_switch_does_not() {
    let (v, dir) = handbook("numbers");
    // numbering off: everything starred, and still in the contents
    let tex = tex_of(&v, "thesis", &ExportOptions { numbered_headings: false, ..ExportOptions::for_note() });
    for want in [
        "\\part*{R\\&D}\\shaleanchor{shale-4}\n\\addcontentsline{toc}{part}{R\\&D}\n",
        "\\chapter*{Appendix}\\shaleanchor{shale-2}\n\\addcontentsline{toc}{chapter}{Appendix}\n",
        "\\section*{Bullets}\\shaleanchor{shale-7-1}\n\\addcontentsline{toc}{section}{Bullets}\n",
    ] {
        assert!(tex.contains(want), "{want:?} in\n{tex}");
    }
    // a folder note's headings are never in the contents
    assert!(tex.contains("\\chapter*{Note on the part}\\shaleanchor{shale-4-1}\n\n") && !tex.contains("{Note on the part}\n\\addcontentsline"), "{tex}");
    // no contents: no \tableofcontents and nothing added to it
    let tex = tex_of(&v, "thesis", &ExportOptions { numbered_headings: false, toc: false, ..ExportOptions::for_note() });
    assert!(!tex.contains("\\tableofcontents") && !tex.contains("\\addcontentsline") && !tex.contains("tocdepth"), "{tex}");
    // numbering on: `numbered-headings: false` of Lists does not change the book's
    let tex = tex_of(&v, "thesis", &ExportOptions { numbered_headings: true, ..ExportOptions::for_note() });
    assert!(tex.contains("\\section{Bullets}\\shaleanchor{shale-7-1}"), "{tex}");
    let _ = std::fs::remove_dir_all(dir);
}

// spec: BOOK-016
#[test]
fn links_to_notes_of_the_book_are_hyperlinks_to_their_anchors_and_others_are_text() {
    let (v, dir) = vault(
        "links",
        &[
            ("Outside.md", b"not in the book\n"),
            ("Book/a.md", b"See [[b]], [[b#Grid|the grid]], [[b#Nowhere|no heading]], [[Part]], [[Outside]], [[Missing]], [c](c.md), [[#Local]] and [web](https://e.com).\n## Local\n| x |\n| --- |\n| [[p]] |\n"),
            ("Book/b.md", b"## Grid\nBack to [[a]][^1].\n\n[^1]: Also [[a#Local]].\n"),
            ("Book/c.md", b"c\n"),
            ("Book/Part/Part.md", b"Folder note\n"),
            ("Book/Part/p.md", b"p\n"),
        ],
    );
    let tex = tex_of(&v, "thesis", &ExportOptions::for_note());
    // a = shale-1, b = shale-2, c = shale-3, Part = shale-4, p = shale-5
    for want in [
        "\\hyperlink{shale-2}{\\textcolor[RGB]{26,95,180}{b}}",
        "\\hyperlink{shale-2-1}{\\textcolor[RGB]{26,95,180}{the grid}}",
        "\\hyperlink{shale-2}{\\textcolor[RGB]{26,95,180}{no heading}}",
        "\\hyperlink{shale-4}{\\textcolor[RGB]{26,95,180}{Part}}",
        "\\hyperlink{shale-3}{\\textcolor[RGB]{26,95,180}{c}}",
        "\\hyperlink{shale-1-1}{\\textcolor[RGB]{26,95,180}{Local}}",
        "\\hyperlink{shale-5}{\\textcolor[RGB]{26,95,180}{p}}",
        "\\hyperlink{shale-1}{\\textcolor[RGB]{26,95,180}{a}}",
        "\\footnote{Also \\hyperlink{shale-1-1}{",
        "\\href{https://e.com}{web}",
    ] {
        assert!(tex.contains(want), "{want:?} in\n{tex}");
    }
    assert!(tex.contains("}, \\textcolor[RGB]{26,95,180}{Outside}, \\textcolor[RGB]{192,28,40}{Missing}, "), "links outside the book are link-coloured text (TEX-032)\n{tex}");
    // every anchor a link goes to is in the document
    for name in tex.match_indices("\\hyperlink{").map(|(i, _)| tex[i + 11..].split('}').next().unwrap().to_string()) {
        assert!(tex.contains(&format!("\\shaleanchor{{{name}}}")), "{name} has an anchor\n{tex}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

fn png(rgb: [u8; 3]) -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(8, 4, image::Rgba([rgb[0], rgb[1], rgb[2], 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

// spec: BOOK-015
#[test]
fn embeds_of_the_same_name_in_different_folders_are_two_files_and_one_file_is_written_once() {
    let (red, blue) = (png([200, 0, 0]), png([0, 0, 200]));
    let (v, dir) = vault("embeds", &[("Book/A/one.md", b"![[pic.png]]\n"), ("Book/A/pic.png", &red), ("Book/B/two.md", b"![[pic.png]]\n\n![[pic.png]]\n"), ("Book/B/pic.png", &blue), ("Book/c.md", b"![[A/pic.png]]\n\n![[gone.png]]\n")]);
    let doc = write_as(&v, "thesis", &ExportOptions::for_note(), &[("author", "Ada")]).unwrap();
    let names: Vec<&str> = doc.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["picture-1.png", "picture-2.png"], "one file per picture of the vault, named by the writer");
    assert_eq!((&doc.files[0].1, &doc.files[1].1), (&red, &blue), "A's picture and B's picture");
    assert_eq!((doc.tex.matches("{picture-1.png}").count(), doc.tex.matches("{picture-2.png}").count()), (2, 2), "{}", doc.tex);
    assert_eq!(doc.warnings, ["missing: gone.png"], "a missing embed is a warning, as for a note");
    let _ = std::fs::remove_dir_all(dir);
}

// spec: BOOK-015
#[test]
fn the_fields_come_from_the_dialog_then_the_top_folder_note_then_the_books_title() {
    let note = b"---\ntitle: The Handbook\nsubtitle: All of it\nauthor: [Ada, Bob]\ndate: 2026-11-17\nexport: false\n---\nNot printed\n";
    let (v, dir) = vault("fields", &[("Book/index.md", note), ("Book/a.md", b"a\n")]);
    let tex = write_as(&v, "report", &ExportOptions::for_note(), &[]).unwrap().tex;
    for want in ["\\title{The Handbook}", "\\subtitle{All of it}", "\\author{Ada \\and Bob}", "\\date{2026-11-17}"] {
        assert!(tex.contains(want), "{want:?} in\n{tex}");
    }
    assert!(!tex.contains("Not printed") && !tex.contains("shale-1}\nNot"), "a folder note with export: false still gives the fields but is not printed\n{tex}");
    let tex = write_as(&v, "report", &ExportOptions::for_note(), &[("author", "Carol"), ("title", "")]).unwrap().tex;
    assert!(tex.contains("\\author{Carol}") && tex.contains("\\title{The Handbook}"), "the dialog's value first, an empty one is none\n{tex}");
    // no folder note: the folder's name is the title, and a required field without a value stops it
    let (bare, bare_dir) = vault("fields-bare", &[("Book/a.md", b"a\n")]);
    assert!(write_as(&bare, "thesis", &ExportOptions::for_note(), &[("author", "Ada")]).unwrap().tex.contains("\\title{Book}"));
    let e = write_as(&bare, "thesis", &ExportOptions::for_note(), &[]).unwrap_err();
    assert!(e.starts_with("“Author” is empty"), "{e}");
    // the dialog starts with the same values
    std::fs::create_dir_all(dir.join("templates/tex")).unwrap();
    let kinds: Vec<(&str, &str)> = TUDA.kinds.iter().map(|(id, text)| (*id, *text)).collect();
    super::sets_tests::make_set(&dir, "tuda", super::sets_tests::META, &kinds);
    let plan = book::plan(&v, Path::new("Book")).unwrap();
    let mut t = TexExport::for_folder(&dir, plan.title.clone(), std::str::from_utf8(note).unwrap(), Some(("tuda".into(), "report".into())));
    assert_eq!((t.values["title"].as_str(), t.values["author"].as_str(), t.values["subtitle"].as_str(), t.values["date"].as_str()), ("The Handbook", "Ada \\and Bob", "All of it", "2026-11-17"));
    t.select_kind("thesis");
    assert_eq!(t.kind, "thesis");
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(bare_dir);
}

// spec: BOOK-013
#[test]
fn only_kinds_of_the_article_and_book_family_hold_a_folder() {
    let set = tuda_set(SetSettings::default());
    let holding: Vec<&str> = set.kinds.iter().filter(|(_, k)| holds_a_folder(k)).map(|(id, _)| id.as_str()).collect();
    let mut sorted = holding.clone();
    sorted.sort();
    assert_eq!(sorted, ["exercise", "phd", "pub", "report", "thesis"]);
    let (v, dir) = vault("kinds", &[("Book/a.md", b"a\n")]);
    for k in ["beamer", "letter", "poster", "sciposter", "leaflet", "announcement"] {
        let e = write_as(&v, k, &ExportOptions::for_note(), &[("author", "Ada")]).unwrap_err();
        assert!(e.contains("cannot hold a folder") && e.contains(ONLY_ARTICLES_AND_BOOKS), "{k}: {e}");
    }
    // the dialog of a folder: a kind that cannot hold it is not chosen, by name or by the last export
    let kinds: Vec<(&str, &str)> = TUDA.kinds.iter().map(|(id, text)| (*id, *text)).collect();
    super::sets_tests::make_set(&dir, "tuda", super::sets_tests::META, &kinds);
    let mut t = TexExport::for_folder(&dir, "Book".into(), "", Some(("tuda".into(), "beamer".into())));
    assert_eq!(t.kind, "pub", "the test page's kind instead of the slides");
    t.select_kind("letter");
    assert_eq!(t.kind, "pub", "a letter cannot be chosen");
    let beamer = &t.current_set().unwrap().kinds.iter().find(|(id, _)| id == "beamer").unwrap().1;
    assert!(!t.allows(beamer) && TexExport::new(&dir, "x".into(), "", None).allows(beamer), "a note's dialog has every kind");
    // a set with no kind that can hold a folder: no kind, so no export
    super::sets_tests::make_set(&dir, "slides", &super::sets_tests::META.replace("Test set", "Slides only"), &[("talk", "name = \"Talk\"\nmapping = \"beamer\"\nclass = \"my\"\n")]);
    let mut t = TexExport::for_folder(&dir, "Book".into(), "", Some(("slides".into(), "talk".into())));
    t.status = super::dialog::TexStatus::Ready("x".into());
    assert_eq!((t.current_set().unwrap().id.as_str(), t.kind.as_str(), t.current_kind().is_none(), t.can_export()), ("slides", "", true, false));
    let _ = std::fs::remove_dir_all(dir);
}
