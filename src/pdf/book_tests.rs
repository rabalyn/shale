//! A folder as one PDF (BOOK, #306): the reading order and the titles (`book`), and the PDF the
//! book is laid out as (`book_export`).

use crate::pdf::book::{self, natural_cmp, Item, Plan};
use crate::pdf::book_export::{export_book, export_folder, Progress};
use crate::pdf::export::tests::{contents, hex, page_ids_and_link_targets, page_streams};
use crate::pdf::export::ExportOptions;
use crate::vault::{rel_to_string, Vault};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;

/// A vault in a temporary folder with these files (a name ending in `/` is an empty folder).
fn vault_with(tag: &str, files: &[(&str, &str)]) -> (Vault, PathBuf) {
    let dir = std::env::temp_dir().join(format!("shale-book-{tag}-{}", std::process::id())).join("vault");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    for (name, text) in files {
        let path = dir.join(name);
        if name.ends_with('/') {
            std::fs::create_dir_all(&path).unwrap();
        } else {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }
    (Vault::open(&dir).unwrap(), dir)
}

fn clean(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

/// The reading order as lines: `1 Title`, indented by depth; `- Title` for a note that is left out.
fn outline(plan: &Plan) -> Vec<String> {
    plan.lines().iter().map(|l| format!("{}{}", "  ".repeat(l.depth), l.number.as_ref().map_or(format!("- {}", l.title), |n| format!("{n} {}", l.title)))).collect()
}

/// The handbook of the spec's example, with a note outside it.
const HANDBOOK: &[(&str, &str)] = &[
    ("Handbook/index.md", "---\ntitle: The Handbook\nsubtitle: A book about notes\nauthor: Ada Example\ndate: 2026-11-17\n---\nWelcome to the handbook. Start with [[Getting started]], then [[Writing]].\n"),
    ("Handbook/Getting started.md", "# Getting started\n\nRead [[Tables#Grid|the grid]] first, see [[Outside]] and [[Missing note]], and [site](https://example.com).\n\n## First steps\n\nDo things.\n"),
    ("Handbook/Writing/Writing.md", "---\ntitle: Writing well\n---\nHow to write.\n\n## Part heading\n\ntext\n"),
    ("Handbook/Writing/Lists.md", "## Bullets\n\n- one\n- two\n\n### Nested\n\ntext\n"),
    ("Handbook/Writing/Tables.md", "## Grid\n\ntable text\n"),
    ("Handbook/Appendix.md", "Extra.\n"),
    ("Handbook/Draft.md", "---\nexport: false\n---\nsecret\n"),
    ("Handbook/z_media/pic.png", "not a note"),
    ("Outside.md", "Not in the book.\n"),
];

fn handbook(tag: &str) -> (Vault, PathBuf) {
    vault_with(tag, HANDBOOK)
}

fn opts() -> ExportOptions {
    ExportOptions::for_note()
}

// spec: BOOK-003
#[test]
fn the_links_of_the_folder_note_give_the_order_then_the_rest_by_name() {
    let (v, d) = handbook("order-links");
    let plan = book::plan(&v, Path::new("Handbook")).unwrap();
    assert_eq!(outline(&plan), ["1 Getting started", "2 Writing well", "  2.1 Lists", "  2.2 Tables", "3 Appendix", "- Draft"]);
    clean(&d);
}

// spec: BOOK-003
#[test]
fn order_names_the_notes_and_folders_and_ignores_names_that_are_not_there() {
    let (v, d) = vault_with(
        "order-prop",
        &[
            ("B/index.md", "---\norder: [Appendix, Writing, Not there, Getting started]\n---\nLinks that do not count: [[Getting started]] [[Appendix]]\n"),
            ("B/Getting started.md", "x\n"),
            ("B/Appendix.md", "x\n"),
            ("B/Writing/a.md", "x\n"),
            ("B/Zebra.md", "x\n"),
            ("C/index.md", "---\norder:\n  - Two\n  - One\n---\n"),
            ("C/One.md", "x\n"),
            ("C/Two.md", "x\n"),
        ],
    );
    assert_eq!(outline(&book::plan(&v, Path::new("B")).unwrap()), ["1 Appendix", "2 Writing", "  2.1 a", "3 Getting started", "4 Zebra"]);
    assert_eq!(outline(&book::plan(&v, Path::new("C")).unwrap()), ["1 Two", "2 One"], "a list on lines too");
    clean(&d);
}

// spec: BOOK-003
#[test]
fn names_are_compared_naturally_notes_and_folders_mixed_and_a_note_is_printed_once() {
    let (v, d) = vault_with(
        "natural",
        &[("N/10 Tables.md", "x\n"), ("N/2 Lists.md", "x\n"), ("N/b.md", "x\n"), ("N/A.md", "x\n"), ("N/3 Sub/x.md", "x\n"), ("L/index.md", "[[b]] [[A]] [[b]] [[A#heading]]\n"), ("L/A.md", "x\n"), ("L/b.md", "x\n"), ("L/c.md", "x\n")],
    );
    assert_eq!(outline(&book::plan(&v, Path::new("N")).unwrap()), ["1 2 Lists", "2 3 Sub", "  2.1 x", "3 10 Tables", "4 A", "5 b"]);
    assert_eq!(outline(&book::plan(&v, Path::new("L")).unwrap()), ["1 b", "2 A", "3 c"], "once, at the first place");
    assert_eq!(natural_cmp("2 Lists", "10 Tables"), std::cmp::Ordering::Less);
    assert_eq!(natural_cmp("apple", "Banana"), std::cmp::Ordering::Less, "case does not matter");
    assert_eq!(natural_cmp("x2", "x10"), std::cmp::Ordering::Less);
    assert_eq!(natural_cmp("x002", "x10"), std::cmp::Ordering::Less, "numbers by value");
    assert_eq!(natural_cmp("a", "a1"), std::cmp::Ordering::Less);
    assert_ne!(natural_cmp("a", "A"), std::cmp::Ordering::Equal, "never equal for different names");
    clean(&d);
}

// spec: BOOK-001
#[test]
fn hidden_and_underscore_folders_and_the_exports_and_templates_of_the_root_are_left_out() {
    let (v, d) =
        vault_with("reserved", &[("d.md", "x\n"), ("_drafts/b.md", "x\n"), ("exports/old.md", "x\n"), ("Templates/t.md", "x\n"), ("Real/c.md", "x\n"), ("Real/exports/e.md", "x\n"), ("Real/_hidden/h.md", "x\n"), (".git/g.md", "x\n")]);
    let plan = book::plan(&v, Path::new("")).unwrap();
    assert_eq!(outline(&plan), ["1 d", "2 Real", "  2.1 c", "  2.2 exports", "    2.2.1 e"]);
    clean(&d);
}

// spec: BOOK-001
#[test]
fn folders_without_a_chapter_are_left_out_and_other_files_are_no_chapters() {
    let (v, d) = vault_with("empty", &[("T/Empty/", ""), ("T/Media/pic.png", "x"), ("T/Drafts/d.md", "---\nexport: false\n---\n"), ("T/Keep/k.md", "x\n"), ("T/sketch.excalidraw", "{}"), ("T/doc.pdf", "x"), ("T/n.md", "x\n")]);
    assert_eq!(outline(&book::plan(&v, Path::new("T")).unwrap()), ["1 Keep", "  1.1 k", "2 n"]);
    assert!(book::plan(&v, Path::new("T/Empty")).unwrap_err().contains("no notes"));
    assert!(book::plan(&v, Path::new("T/Drafts")).unwrap_err().contains("no notes"), "a folder with only left-out notes has no book");
    assert!(book::plan(&v, Path::new("Nothing")).unwrap_err().contains("no folder"));
    clean(&d);
}

// spec: BOOK-001
#[test]
fn a_note_with_export_false_is_a_chapter_that_is_left_out() {
    let (v, d) = handbook("left-out");
    let plan = book::plan(&v, Path::new("Handbook")).unwrap();
    let draft = plan.items.iter().find_map(|i| match i {
        Item::Chapter(c) if c.title == "Draft" => Some(c),
        _ => None,
    });
    assert!(draft.is_some_and(|c| c.left_out));
    assert!(!plan.notes().iter().any(|n| n.ends_with("Draft.md")));
    clean(&d);
}

// spec: BOOK-002
#[test]
fn the_folder_note_is_index_then_readme_then_the_note_named_like_the_folder_then_folder_note_true() {
    let (v, d) = vault_with(
        "folder-note",
        &[
            ("T/A/Index.md", "x\n"),
            ("T/A/README.md", "x\n"),
            ("T/A/A.md", "x\n"),
            ("T/A/z.md", "x\n"),
            ("T/B/readme.md", "x\n"),
            ("T/B/B.md", "x\n"),
            ("T/C/c.MD", "x\n"),
            ("T/C/other.md", "x\n"),
            ("T/D/a.md", "x\n"),
            ("T/D/chosen.md", "---\nfolder-note: true\n---\n"),
            ("T/E/e1.md", "x\n"),
            ("T/E/e2.md", "x\n"),
            ("T/F/F.md", "---\nfolder-note: true\n---\n"),
            ("T/F/x.md", "---\nfolder-note: true\n---\n"),
        ],
    );
    let plan = book::plan(&v, Path::new("T")).unwrap();
    let part = |name: &str| {
        plan.items
            .iter()
            .find_map(|i| match i {
                Item::Part(p) if p.dir.ends_with(name) => Some(p.clone()),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no part {name}"))
    };
    // the vault's paths are OS paths (`T\A\Index.md` on Windows): compare them as vault paths with `/` (#322)
    assert_eq!(part("A").note, Some(Path::new("T").join("A").join("Index.md")), "the folder note is the vault's own path");
    let note = |name: &str| part(name).note.map(|n| rel_to_string(&n));
    assert_eq!(note("A").as_deref(), Some("T/A/Index.md"), "index.md before README.md and A.md");
    assert_eq!(note("B").as_deref(), Some("T/B/readme.md"), "README.md before B.md, any case");
    assert_eq!(note("C").as_deref(), Some("T/C/c.MD"), "the note named like the folder, any case");
    assert_eq!(note("D").as_deref(), Some("T/D/chosen.md"), "folder-note: true");
    assert_eq!(note("E"), None, "none: the part has a heading and no text");
    assert_eq!(note("F").as_deref(), Some("T/F/F.md"), "the name before the property");
    let titles = |name: &str| outline(&Plan { items: part(name).items, ..plan.clone() });
    assert_eq!(titles("A"), ["1 A", "2 README", "3 z"], "the folder note (index.md) is no chapter of its own, the other notes are");
    assert_eq!(titles("E"), ["1 e1", "2 e2"]);
    assert_eq!(part("E").title, "E");
    clean(&d);
}

// spec: BOOK-002
#[test]
fn a_folder_note_that_is_left_out_is_not_printed_but_still_orders() {
    let (v, d) = vault_with("note-left-out", &[("T/P/index.md", "---\nexport: false\norder: [z, a]\n---\nsecret\n"), ("T/P/a.md", "x\n"), ("T/P/z.md", "x\n")]);
    let plan = book::plan(&v, Path::new("T")).unwrap();
    let Item::Part(p) = &plan.items[0] else { panic!("a part") };
    assert_eq!(p.note, None);
    assert_eq!(outline(&Plan { items: p.items.clone(), ..plan.clone() }), ["1 z", "2 a"]);
    clean(&d);
}

// spec: BOOK-004
#[test]
fn titles_come_from_front_matter_the_title_line_or_the_folder_name() {
    let (v, d) = vault_with(
        "titles",
        &[("Shelf/261117_Plan.md", "x\n"), ("Shelf/Named.md", "---\ntitle: A better title\n---\n"), ("Shelf/Sub/Sub.md", "---\ntitle: Sub title\n---\n"), ("Shelf/Sub/a.md", "x\n"), ("Shelf/Plain/a.md", "x\n"), ("Bare/a.md", "x\n")],
    );
    let plan = book::plan(&v, Path::new("Shelf")).unwrap();
    assert_eq!(outline(&plan), ["1 Plan", "2 A better title", "3 Plain", "  3.1 a", "4 Sub title", "  4.1 a"]);
    let titles: Vec<String> = plan.lines().into_iter().map(|l| l.title).collect();
    assert_eq!(titles, ["Plan", "A better title", "Plain", "a", "Sub title", "a"], "front matter, folder name, title line without the date prefix");
    assert_eq!(plan.title, "Shelf", "no title in a folder note: the folder's name");
    assert!(plan.subtitle.is_none() && plan.author.is_none() && plan.date.is_none());
    assert_eq!(book::plan(&v, Path::new("Bare")).unwrap().title, "Bare");
    let root = book::plan(&v, Path::new("")).unwrap();
    assert_eq!(root.title, "vault", "the vault's name for the vault itself");
    let (h, d2) = handbook("book-title");
    let hb = book::plan(&h, Path::new("Handbook")).unwrap();
    assert_eq!((hb.title.as_str(), hb.subtitle.as_deref(), hb.author.as_deref(), hb.date.as_deref()), ("The Handbook", Some("A book about notes"), Some("Ada Example"), Some("2026-11-17")));
    assert_eq!(hb.intro.as_deref(), Some(Path::new("Handbook/index.md")));
    clean(&d);
    clean(&d2);
}

/// The titles of the bookmarks of a PDF, in order, with their object number and the one of their parent.
fn bookmarks(bytes: &[u8]) -> Vec<(usize, String, Option<usize>)> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    for obj in text.split("\nendobj") {
        let Some((head, body)) = obj.split_once(" 0 obj\n") else { continue };
        let Some((_, rest)) = body.split_once("/Title <FEFF") else { continue };
        let digits = rest.split('>').next().unwrap();
        let units: Vec<u16> = digits.as_bytes().chunks(4).map(|c| u16::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap()).collect();
        let id: usize = head.trim().rsplit('\n').next().unwrap().trim().parse().unwrap();
        let parent = body.split_once("/Parent ").and_then(|(_, p)| p.split_whitespace().next()?.parse().ok());
        out.push((id, String::from_utf16(&units).unwrap(), parent));
    }
    out
}

fn pages(bytes: &[u8]) -> usize {
    hayro::hayro_syntax::Pdf::new(bytes.to_vec()).expect("a valid PDF").pages().len()
}

// spec: BOOK-005
#[test]
fn the_book_has_a_title_page_with_the_title_subtitle_author_and_date_and_no_page_number() {
    let (v, d) = handbook("title-page");
    let bytes = export_book(&v, Path::new("Handbook"), &opts(), None).unwrap();
    let streams = page_streams(&bytes);
    for text in ["The Handbook", "A book about notes", "Ada Example", "2026-11-17"] {
        assert!(streams[0].contains(&hex(text)), "the title page says {text}");
    }
    assert!(!streams[0].contains(&hex("Contents")), "the contents are on the next page");
    assert!(streams[1].contains(&hex("Contents")));
    assert!(!streams[0].contains(&hex("1 / ")), "no page number on the title page");
    assert!(streams[1].contains(&hex("2 / ")), "the other pages have theirs");
    let without = export_book(&v, Path::new("Handbook"), &ExportOptions { title: false, ..opts() }, None).unwrap();
    let streams = page_streams(&without);
    assert!(streams[0].contains(&hex("Contents")) && !streams[0].contains(&hex("A book about notes")), "no title page: the contents come first");
    assert!(streams[0].contains(&hex("1 / ")), "and the first page is numbered");
    let no_toc = export_book(&v, Path::new("Handbook"), &ExportOptions { toc: false, ..opts() }, None).unwrap();
    assert!(!contents(&no_toc).contains(&hex("Contents")));
    assert!(pages(&no_toc) < pages(&bytes));
    clean(&d);
}

// spec: BOOK-005
#[test]
fn every_part_and_chapter_starts_on_a_new_page_unless_asked_otherwise() {
    let (v, d) = handbook("new-pages");
    let on = export_book(&v, Path::new("Handbook"), &opts(), None).unwrap();
    let off = export_book(&v, Path::new("Handbook"), &ExportOptions { new_page_each: false, ..opts() }, None).unwrap();
    // title page, contents, the introduction, then Getting started, Writing well, Lists, Tables, Appendix
    assert_eq!(pages(&on), 8, "a page each");
    assert!(pages(&off) < 5, "{} pages without the breaks", pages(&off));
    let streams = page_streams(&on);
    for (i, word) in [(2, "Welcome"), (3, "First"), (4, "How"), (5, "Bullets"), (6, "Grid"), (7, "Extra.")] {
        assert!(streams[i].contains(&hex(word)), "page {} starts with {word}", i + 1);
    }
    clean(&d);
}

// spec: BOOK-005
#[test]
fn the_paper_and_the_options_of_the_note_dialog_apply_to_the_whole_book() {
    let (v, d) = handbook("paper");
    let o = ExportOptions { paper: crate::pdf::export::Paper::A5, landscape: true, page_numbers: false, ..opts() };
    let bytes = export_book(&v, Path::new("Handbook"), &o, None).unwrap();
    assert_eq!(String::from_utf8_lossy(&bytes).matches("/MediaBox [0 0 595.00 420.00]").count(), pages(&bytes), "A5 landscape on every page");
    assert!(!contents(&bytes).contains(&hex(" / ")), "page numbers off");
    clean(&d);
}

// spec: BOOK-006, BOOK-007
#[test]
fn parts_chapters_and_headings_are_numbered_by_place_and_nested_in_the_bookmarks() {
    let (v, d) = handbook("numbers");
    let bytes = export_book(&v, Path::new("Handbook"), &opts(), None).unwrap();
    let marks = bookmarks(&bytes);
    let titles: Vec<&str> = marks.iter().map(|m| m.1.as_str()).collect();
    assert_eq!(
        titles,
        ["1 Getting started", "1.1 First steps", "2 Writing well", "Part heading", "2.1 Lists", "2.1.1 Bullets", "2.1.1.1 Nested", "2.2 Tables", "2.2.1 Grid", "3 Appendix"],
        "the `# Getting started` that says the title is not a heading again"
    );
    let id = |t: &str| marks.iter().find(|m| m.1 == t).unwrap().0;
    let parent = |t: &str| marks.iter().find(|m| m.1 == t).unwrap().2.unwrap();
    assert_eq!(parent("1.1 First steps"), id("1 Getting started"));
    assert_eq!(parent("2.1 Lists"), id("2 Writing well"));
    assert_eq!(parent("Part heading"), id("2 Writing well"), "the folder note's headings are below its part");
    assert_eq!(parent("2.1.1 Bullets"), id("2.1 Lists"));
    assert_eq!(parent("2.1.1.1 Nested"), id("2.1.1 Bullets"));
    assert_eq!(parent("2.2.1 Grid"), id("2.2 Tables"));
    assert_ne!(parent("3 Appendix"), id("2 Writing well"), "a part's chapters end with it");
    let c = contents(&bytes);
    assert!(c.contains(&hex("2.1.1.1 ")), "the numbers are printed before the headings");
    // numbering off: the same without numbers
    let plain = export_book(&v, Path::new("Handbook"), &ExportOptions { numbered_headings: false, ..opts() }, None).unwrap();
    let titles: Vec<String> = bookmarks(&plain).into_iter().map(|m| m.1).collect();
    assert_eq!(titles, ["Getting started", "First steps", "Writing well", "Part heading", "Lists", "Bullets", "Nested", "Tables", "Grid", "Appendix"]);
    clean(&d);
}

// spec: BOOK-006
#[test]
fn a_notes_own_numbering_switch_does_not_change_the_books() {
    let (v, d) = vault_with("switch", &[("Bk/a.md", "---\nnumbered-headings: false\n---\n## One\n"), ("Bk/c.md", "## Two\n")]);
    let titles: Vec<String> = bookmarks(&export_book(&v, Path::new("Bk"), &opts(), None).unwrap()).into_iter().map(|m| m.1).collect();
    assert_eq!(titles, ["1 a", "1.1 One", "2 c", "2.1 Two"]);
    clean(&d);
}

// spec: BOOK-007
#[test]
fn the_contents_list_parts_chapters_and_first_level_headings_with_links_to_their_pages() {
    let (v, d) = handbook("contents");
    let bytes = export_book(&v, Path::new("Handbook"), &opts(), None).unwrap();
    let streams = page_streams(&bytes);
    let toc = &streams[1];
    for label in ["1 Getting started", "1.1 First steps", "2 Writing well", "2.1 Lists", "2.1.1 Bullets", "2.2 Tables", "2.2.1 Grid", "3 Appendix"] {
        assert!(toc.contains(&hex(label)), "the contents have {label}");
    }
    for left_out in ["Nested", "Draft", "Part heading"] {
        assert!(!toc.contains(&hex(left_out)), "{left_out} is not in the contents: only the first level of headings of a chapter");
    }
    assert!(toc.contains(&hex("8")), "the page of the appendix: the eighth");
    let (page_ids, targets) = page_ids_and_link_targets(&bytes);
    assert_eq!(targets.len(), 13, "8 lines of the contents and 5 words of links inside the book (a link has a place per word)");
    assert!(targets.iter().all(|t| page_ids.contains(t)));
    // the contents link to the pages the chapters are on: the first eight links of the file
    assert_eq!(targets.iter().filter(|t| **t == page_ids[7]).count(), 1, "the appendix is linked from the contents only");
    clean(&d);
}

// spec: BOOK-008
#[test]
fn a_page_of_a_chapter_has_its_title_at_the_top_but_not_its_first_page() {
    let long: String = (1..=140).map(|n| format!("Line {n} of a long chapter that goes on and on.\n\n")).collect();
    let (v, d) = vault_with("header", &[("Bk/Long.md", &long), ("Bk/Short.md", "short\n")]);
    let bytes = export_book(&v, Path::new("Bk"), &opts(), None).unwrap();
    let n = pages(&bytes);
    let long_pages = n - 3; // the title page, the contents and the short chapter are the others
    assert!(long_pages >= 3, "a chapter of several pages: {long_pages}");
    let c = contents(&bytes);
    let count = |s: &str| c.matches(&hex(s)).count();
    assert_eq!(count("1 Long"), 1 + (long_pages - 1), "the contents, and every page of the chapter but its first");
    assert_eq!(count("2 Short"), 1, "a chapter of one page has no header");
    let streams = page_streams(&bytes);
    assert!(!streams[2].contains(&hex("1 Long")), "the first page of the chapter has none");
    assert!(streams[3].contains(&hex("1 Long")), "the second page of the chapter has it");
    let off = export_book(&v, Path::new("Bk"), &ExportOptions { title: false, toc: false, ..opts() }, None).unwrap();
    assert!(!page_streams(&off)[0].contains(&hex("1 Long")), "the first page of the first chapter has none");
    clean(&d);
}

// spec: BOOK-009
#[test]
fn a_link_to_a_note_of_the_book_goes_to_its_page_or_heading_and_other_links_are_text() {
    let (v, d) = handbook("links");
    let bytes = export_book(&v, Path::new("Handbook"), &opts(), None).unwrap();
    let (page_ids, targets) = page_ids_and_link_targets(&bytes);
    let streams = page_streams(&bytes);
    let grid_page = streams.iter().rposition(|s| s.contains(&hex("Grid"))).unwrap();
    assert_eq!(targets.iter().filter(|t| **t == page_ids[grid_page]).count(), 4, "[[Tables#Grid|the grid]], two words, and the two lines of the contents");
    let intro = &streams[2];
    assert!(intro.contains(&hex("Getting")) && intro.contains(&hex("Writing")));
    assert_eq!(targets.iter().filter(|t| **t == page_ids[3]).count(), 4, "[[Getting started]] of the introduction, two words, and the two lines of the contents");
    let c = contents(&bytes);
    assert!(c.contains(&hex("the ")) && c.contains(&hex("Outside")), "a link to a note outside the book is its text");
    assert!(!c.contains(&hex("[[")), "no link is printed with its brackets");
    assert!(String::from_utf8_lossy(&bytes).contains("/URI (https://example.com)"), "web links stay web links");
    clean(&d);
}

// spec: BOOK-010
#[test]
fn the_export_counts_the_notes_and_can_be_stopped() {
    let (v, d) = handbook("progress");
    let p = Progress::default();
    export_book(&v, Path::new("Handbook"), &opts(), Some(&p)).unwrap();
    assert_eq!((p.done.load(Ordering::SeqCst), p.total.load(Ordering::SeqCst)), (6, 6), "the introduction, the parts and the chapters");
    let stopped = Progress::default();
    stopped.cancel.store(true, Ordering::SeqCst);
    let e = export_book(&v, Path::new("Handbook"), &opts(), Some(&stopped)).unwrap_err();
    assert!(e.contains("cancelled"), "{e}");
    clean(&d);
}

// spec: BOOK-011
#[test]
fn the_command_line_export_writes_a_pdf_and_says_why_it_cannot() {
    let (_, d) = handbook("cli");
    let out = d.join("out/book.pdf");
    export_folder(&d.join("Handbook"), &out).unwrap();
    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    // the folder is the vault: its notes are the chapters
    assert!(page_streams(&bytes)[0].contains(&hex("Handbook")), "the title: the folder's name");
    assert!(bookmarks(&bytes).iter().any(|m| m.1 == "1 Getting started"));
    assert!(export_folder(&d.join("nothing"), &d.join("x.pdf")).unwrap_err().contains("not a folder"));
    assert!(!d.join("nothing").exists(), "a folder that is not there is not made");
    std::fs::create_dir_all(d.join("empty")).unwrap();
    assert!(export_folder(&d.join("empty"), &d.join("y.pdf")).unwrap_err().contains("no notes"));
    assert!(!d.join("y.pdf").exists());
    clean(&d);
}

/// The text of a note that is code, inline or fenced: what is printed as written.
fn code_text(text: &str) -> String {
    let fenced = crate::markdown::fenced_code_lines(text);
    let mut out = String::new();
    for (i, (_, line)) in crate::markdown::lines_with_offsets(text).into_iter().enumerate() {
        if fenced.get(i).copied().unwrap_or(false) {
            out.push_str(line);
        } else {
            crate::markdown::inline_code_ranges(line).into_iter().for_each(|r| out.push_str(&line[r]));
        }
        out.push('\n');
    }
    out
}

// spec: BOOK-012
#[test]
fn the_user_guide_is_a_book_with_every_note_once_in_the_order_of_start_here() {
    let guide = Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/userguide");
    let v = Vault::open(&guide).unwrap();
    let plan = book::plan(&v, Path::new("")).unwrap();
    assert_eq!((plan.title.as_str(), plan.intro.as_deref()), ("Shale user guide", Some(Path::new("Start here.md"))));
    assert!(plan.subtitle.is_some(), "a subtitle for the title page");
    // every note of the guide but the excluded one is in the book, once
    let printed = plan.notes();
    let mut all: Vec<PathBuf> = v.files.iter().filter(|f| f.kind == crate::vault::FileKind::Note).map(|f| f.rel.clone()).collect();
    all.retain(|n| n != Path::new("People/Ada Example.md"));
    let mut sorted = printed.clone();
    sorted.sort();
    all.sort();
    assert_eq!(sorted, all, "every note once: {printed:?}");
    assert!(v.notes[Path::new("People/Ada Example.md")].text.contains("export: false"));
    // the chapters come in the order of the links of "Start here"
    let start = &v.notes[Path::new("Start here.md")].text;
    let mut linked: Vec<PathBuf> = Vec::new();
    for l in crate::markdown::find_links(start).into_iter().filter(|l| !l.embed) {
        if let Some(t) = v.resolve(&l.target, Some(Path::new("Start here.md"))) {
            if !linked.contains(&t) {
                linked.push(t);
            }
        }
    }
    assert!(linked.len() >= 20, "the guide links its chapters: {linked:?}");
    assert_eq!(&printed[1..1 + linked.len()], linked.as_slice(), "in the order of Start here");
    // the PDF
    let bytes = export_book(&v, Path::new(""), &opts(), None).unwrap();
    let raw = String::from_utf8_lossy(&bytes);
    let c = contents(&bytes);
    let marks = bookmarks(&bytes);
    assert!(raw.contains("/Type /Outlines") && marks.len() > printed.len(), "bookmarks for every chapter and heading");
    assert_eq!(marks.iter().filter(|m| m.1 == "1 Notes and links").count(), 1);
    assert!(c.matches(&hex("1 Notes and links")).count() > 2, "the contents and the running header of the chapter's later pages");
    assert!(c.contains(&hex("Contents")) && c.contains(&hex("Shale user guide")));
    // every character of the guide has a glyph in the PDF fonts (PDF-010): arrows, ellipses, calendars
    let missing: std::collections::BTreeSet<char> = printed.iter().flat_map(|n| v.notes[n].text.chars()).filter(|c| !crate::pdf::write::has_glyph(*c)).collect();
    assert!(missing.is_empty(), "no glyph for {missing:?}");
    // no link is printed as its markup: a word of the brackets that code does not have is not in the PDF
    // (words are drawn one by one)
    let mut words = std::collections::BTreeSet::new();
    let mut code = std::collections::BTreeSet::new();
    for rel in &printed {
        let text = &v.notes[rel].text;
        code.extend(code_text(text).replace('`', "").split_whitespace().map(str::to_string));
        for l in crate::markdown::find_links(text) {
            words.extend(text[l.range].split_whitespace().filter(|w| w.contains("[[") || w.contains("]]")).map(str::to_string));
        }
    }
    assert!(words.len() > 20);
    assert!(c.contains(&hex("[[wiki")), "code that shows a link is printed as written");
    let shown: Vec<&String> = words.iter().filter(|w| !code.iter().any(|c| c.contains(w.as_str())) && c.contains(&hex(w))).collect();
    assert!(shown.is_empty(), "printed with their brackets: {shown:?}");
    assert!(!c.contains(&hex("Back to [[")), "the link of every chapter is a link");
}

// spec: BOOK-006, PDF-011, PDF-012
#[test]
fn footnotes_link_within_their_own_chapter_and_math_is_drawn_in_a_book() {
    let (v, d) = vault_with("notes", &[("Book/One.md", "# One\n\nfirst claim[^1]\n\n[^1]: firstdef\n\n$$\\frac{a}{b}$$\n"), ("Book/Two.md", "# Two\n\nsecond claim[^1]\n\n[^1]: seconddef\n")]);
    let bytes = export_book(&v, Path::new("Book"), &ExportOptions { toc: false, ..opts() }, None).unwrap();
    let streams = page_streams(&bytes);
    let (ids, targets) = page_ids_and_link_targets(&bytes);
    let first = streams.iter().position(|s| s.contains(&hex("firstdef"))).unwrap();
    let second = streams.iter().position(|s| s.contains(&hex("seconddef"))).unwrap();
    assert_ne!(first, second, "the chapters are on pages of their own");
    for (page, def) in [(first, "firstdef"), (second, "seconddef")] {
        assert_eq!(targets.iter().filter(|t| **t == ids[page]).count(), 1, "the one reference of the chapter links to its own definition {def}");
    }
    let c = streams[first].as_str();
    assert!(!c.contains(&hex("[^1")) && !c.contains(&hex("frac")), "no raw footnote or TeX text");
    assert!(c.matches(" f Q").count() >= 3, "the formula is drawn as outlines: {}", c.matches(" f Q").count());
    clean(&d);
}

// spec: BOOK-002
#[test]
fn a_folder_note_path_joined_with_the_os_separator_reads_as_a_vault_path_with_slashes() {
    // the vault scan joins a note's path with the OS separator; shown or compared as text it has `/` (#322)
    assert_eq!(rel_to_string(&Path::new("T").join("A").join("Index.md")), "T/A/Index.md");
    #[cfg(windows)]
    assert_eq!(rel_to_string(Path::new(r"T\A\Index.md")), "T/A/Index.md");
}

/// Keeps a second dispatcher alive while a flow is recorded. With only one live dispatcher
/// `tracing` computes the interest of a new span callsite from the dispatcher of the thread that
/// first reaches it, so another test's thread (which has none) could switch the spans off for the
/// recording thread; with two or more it asks every registered dispatcher.
fn hold_callsites() -> tracing::Dispatch {
    tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default())
}

fn dataflow_doc(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow").join(name)
}

// spec: DEV-026, DEV-029
#[test]
fn dev_029_dataflow_of_a_book_export() {
    let (v, d) = handbook("dataflow-book");
    let _hold = hold_callsites();
    let flow = crate::dataflow::record(|| {
        export_book(&v, Path::new("Handbook"), &opts(), None).unwrap();
    });
    clean(&d);
    crate::dataflow::verify(&dataflow_doc("book-export.md"), "book_export", &crate::dataflow::render_mermaid(&flow));
}

// spec: DEV-026, DEV-029
#[test]
fn dev_029_dataflow_of_a_note_export() {
    let (v, d) = handbook("dataflow-note");
    let rel = Path::new("Handbook/Getting started.md");
    let text = std::fs::read_to_string(d.join(rel)).unwrap();
    let _hold = hold_callsites();
    let flow = crate::dataflow::record(|| {
        crate::pdf::export::export_note(&v, rel, &text, &opts()).unwrap();
    });
    clean(&d);
    crate::dataflow::verify(&dataflow_doc("pdf-export.md"), "pdf_export", &crate::dataflow::render_mermaid(&flow));
}
