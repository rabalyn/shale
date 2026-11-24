//! The reading view of a thesis (EDIT-127, #646): the citations as the style formats them, the
//! references as word and number, hidden labels, numbered figures, tables and equations with their
//! captions, and the bibliography at the end or at its marker.

use super::tests::shapes_text;
use super::*;
use crate::thesis::View;
use egui::{RawInput, Rect};
use std::sync::Arc;

const BIB: &str = "@article{knuth1984, author = {Knuth, Donald E.}, title = {Literate Programming}, journaltitle = {The Computer Journal}, date = {1984}}\n@book{lamport1994, author = {Lamport, Leslie}, title = {LaTeX}, publisher = {Addison-Wesley}, date = {1994}}\n";

/// A vault with a picture `a.png` in it.
fn vault_with_picture(tag: &str) -> (Vault, std::path::PathBuf) {
    let d = std::env::temp_dir().join(format!("shale-reader-thesis-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    crate::vault::write_sample_vault(&d).unwrap();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([200, 0, 0, 255])).save(d.join("a.png")).unwrap();
    (Vault::open(&d).unwrap(), d)
}

fn document(text: &str, bib: Option<&str>) -> Arc<View> {
    Arc::new(View::build(None, &[text], 0, text, bib.map(crate::thesis::view::test_effect).unwrap_or_default(), true))
}

/// Draw `text` in the reading view with its document, until the media look-ups are done, and
/// return what was painted.
fn read_doc(v: &Vault, text: &str, view: Arc<View>) -> Vec<(String, Rect)> {
    let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut painted = Vec::new();
    let mut frame = |painted: &mut Vec<(String, Rect)>| {
        time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(700.0, 900.0))), time: Some(time), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                show(ui, text, v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras { thesis: Some(view.clone()), ..Default::default() });
            });
        });
        out.textures_delta.clear();
        *painted = shapes_text(&out.shapes);
        crate::media::look::Looks::of(&ctx).busy()
    };
    crate::ui_test_support::settle_within(std::time::Duration::from_secs(30), || if frame(&mut painted) { vec!["the media look-ups"] } else { vec![] });
    frame(&mut painted);
    painted
}

fn find<'a>(painted: &'a [(String, Rect)], part: &str) -> &'a (String, Rect) {
    painted.iter().find(|(t, _)| t.contains(part)).unwrap_or_else(|| panic!("{part:?} is not painted: {:?}", painted.iter().map(|(t, _)| t).collect::<Vec<_>>()))
}

fn is_painted(painted: &[(String, Rect)], part: &str) -> bool {
    painted.iter().any(|(t, _)| t.contains(part))
}

// spec: EDIT-127
#[test]
fn edit_127_a_citation_is_drawn_as_the_style_formats_it_and_a_reference_as_word_and_number() {
    let (v, d) = vault_with_picture("words");
    let text = "---\nlanguage: de\ncitation-style: apa\n---\nAls [@knuth1984, S. 3] und @lamport1994 zeigen, siehe @fig:plot, @tbl:runs und @eq:e.\n\n![[a.png|Ergebnisse]]{#fig:plot}\n\n| a |\n|---|\n| 1 |\n: Läufe {#tbl:runs}\n\n$$E = mc^2$$ {#eq:e}\n";
    let painted = read_doc(&v, text, document(text, Some(BIB)));
    let line = &find(&painted, "zeigen").0;
    assert!(line.contains("(Knuth, 1984, S. 3)"), "{line}");
    assert!(line.contains("Lamport (1994)"), "{line}");
    assert!(line.contains("Abbildung 1, Tabelle 1 und Gleichung (1)"), "{line}");
    assert!(!line.contains('@') && !line.contains("[@"), "no markup is left: {line}");
    assert!(!painted.iter().any(|(t, _)| t.contains("{#")), "labels are not shown: {:?}", painted.iter().map(|(t, _)| t).collect::<Vec<_>>());
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_the_language_of_the_document_sets_the_words_and_english_is_the_default() {
    let (v, d) = vault_with_picture("english");
    let text = "See @fig:plot and [@knuth1984].\n\n![[a.png|Results]]{#fig:plot}\n";
    let painted = read_doc(&v, text, document(text, Some(BIB)));
    assert!(find(&painted, "See").0.contains("See Figure 1 and (Knuth 1984)."), "{:?}", find(&painted, "See").0);
    assert!(is_painted(&painted, "Figure 1: Results"), "the caption");
    assert!(is_painted(&painted, "References"), "the heading of the bibliography");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_a_figure_has_its_number_and_caption_below_it_or_above_where_the_property_says() {
    let (v, d) = vault_with_picture("figure");
    let below = "Text.\n\n![[a.png|Results of run 2]]{#fig:r}\n\nAfter.\n";
    let painted = read_doc(&v, below, document(below, None));
    let caption = find(&painted, "Figure 1: Results of run 2").1;
    assert!(caption.top() > find(&painted, "Text.").1.bottom(), "after the text it stands under");
    assert!(caption.bottom() <= find(&painted, "After.").1.top(), "and the picture is above it: the next paragraph is below");
    let picture_to_caption = caption.top() - find(&painted, "Text.").1.bottom();
    assert!(picture_to_caption > 15.0, "the picture is between the text and the caption: {picture_to_caption}");
    let above = "---\nfigure-caption: above\n---\nText.\n\n![[a.png|Results of run 2]]{#fig:r}\n\nAfter.\n";
    let painted = read_doc(&v, above, document(above, None));
    let caption = find(&painted, "Figure 1: Results of run 2").1;
    assert!(caption.bottom() <= find(&painted, "After.").1.top() - 15.0, "above: the picture is between the caption and the next paragraph");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_a_table_has_its_number_and_caption_above_it_by_default_and_below_where_the_property_says() {
    let (v, d) = vault_with_picture("table");
    let table = "| head |\n|---|\n| cell |\n: Runs of the test {#tbl:runs}\n\nAfter.\n";
    let painted = read_doc(&v, table, document(table, None));
    assert!(is_painted(&painted, "Table 1: Runs of the test"));
    assert!(!painted.iter().any(|(t, _)| t.starts_with(':')), "the caption line is not drawn as it is written: {:?}", painted.iter().map(|(t, _)| t).collect::<Vec<_>>());
    assert!(find(&painted, "Table 1").1.bottom() <= find(&painted, "head").1.top(), "above by default");
    let below = format!("---\ntable-caption: below\n---\n{table}");
    let painted = read_doc(&v, &below, document(&below, None));
    assert!(find(&painted, "Table 1").1.top() >= find(&painted, "cell").1.bottom(), "below when the note says so");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_a_numbered_equation_has_its_number_at_the_right_margin() {
    let (v, d) = vault_with_picture("equation");
    let text = "Before.\n\n$$E = mc^2$$ {#eq:e}\n\nAfter @eq:e.\n";
    let painted = read_doc(&v, text, document(text, None));
    let number = find(&painted, "(1)");
    let row = find(&painted, "After");
    assert!(number.1.right() > 600.0, "at the right margin of the page: {:?}", number.1);
    assert!(number.1.top() > find(&painted, "Before.").1.bottom() && number.1.bottom() < row.1.top(), "on the row of the formula: {:?}", number.1);
    assert!(row.0.contains("After Equation (1)."), "{}", row.0);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_the_bibliography_comes_at_the_end_or_at_its_marker_and_a_book_chapter_has_it_only_at_the_marker() {
    let (v, d) = vault_with_picture("bibliography");
    let end = "First [@knuth1984].\n\nLast words.\n";
    let painted = read_doc(&v, end, document(end, Some(BIB)));
    let heading = find(&painted, "References").1;
    assert!(heading.top() > find(&painted, "Last words.").1.bottom(), "at the end");
    assert!(is_painted(&painted, "Knuth, Donald E. 1984."), "the entry as the style writes it");
    assert!(!is_painted(&painted, "Lamport"), "only what is cited");
    let marked = "First [@knuth1984].\n\n<!-- shale: bibliography -->\n\nLast words.\n";
    let painted = read_doc(&v, marked, document(marked, Some(BIB)));
    assert!(find(&painted, "References").1.bottom() < find(&painted, "Last words.").1.top(), "at the marker");
    assert!(!is_painted(&painted, "shale: bibliography"), "the marker is not shown");
    assert_eq!(painted.iter().filter(|(t, _)| t == "References").count(), 1, "once");
    // a note of a book: the book's end is not this chapter's
    let book = Arc::new(crate::thesis::Book { dir: "T".into(), top: "T/T.md".into(), notes: vec![("T/one.md".into(), true)] });
    let in_book = Arc::new(View::build(Some(book), &[end], 0, "", crate::thesis::view::test_effect(BIB), true));
    let painted = read_doc(&v, end, in_book);
    assert!(!is_painted(&painted, "References"), "{:?}", painted.iter().map(|(t, _)| t).collect::<Vec<_>>());
    assert!(find(&painted, "First").0.contains("(Knuth 1984)"));
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_a_numeric_style_labels_the_list_and_a_note_without_citations_has_no_list() {
    let (v, d) = vault_with_picture("numeric");
    let text = "---\ncitation-style: numeric\n---\nSee [@lamport1994] and [@knuth1984].\n";
    let painted = read_doc(&v, text, document(text, Some(BIB)));
    assert!(find(&painted, "See").0.contains("See [1] and [2]."), "{:?}", find(&painted, "See").0);
    assert!(is_painted(&painted, "[1]") && is_painted(&painted, "Lamport"), "the list has the numbers");
    let none = "Nothing cited @todo here.\n";
    let painted = read_doc(&v, none, document(none, Some(BIB)));
    assert!(!is_painted(&painted, "References"), "no citation, no list");
    assert!(find(&painted, "Nothing").0.contains("@todo"), "a bare word is plain text");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: EDIT-127
#[test]
fn edit_127_what_does_not_resolve_stays_as_written_and_says_why_in_its_tooltip() {
    let text = "A [@nobody], @fig:zz and [@knuth1984].\n";
    let view = document(text, Some(BIB));
    let spans = thesis::spans(text, &view);
    let by_text: Vec<(String, bool, String)> = spans.iter().map(|(l, ok)| (l.alias.clone().unwrap_or_default(), *ok, thesis::tip_of(l).unwrap_or_default().to_string())).collect();
    assert_eq!(by_text[0], ("[@nobody]".to_string(), false, "“nobody” is not in the bibliography".to_string()));
    assert_eq!(by_text[1], ("@fig:zz".to_string(), false, "There is no label {#fig:zz} in the document".to_string()));
    assert_eq!((by_text[2].0.as_str(), by_text[2].1), ("(Knuth 1984)", true));
    assert!(by_text[2].2.starts_with("Knuth, Donald E. 1984."), "the entry is its tooltip: {}", by_text[2].2);
}

// spec: EDIT-127
#[test]
fn edit_127_a_note_without_a_document_is_drawn_as_it_always_was() {
    let (v, d) = vault_with_picture("plain");
    let text = "Mail ada@host.org and @todo [@a] {#fig:x}\n";
    let (_, plain) = super::tests::read(&v, text, &Extras::default(), vec![]);
    assert!(plain.iter().any(|(t, _)| t == text.trim_end()), "the text is as written: {plain:?}");
    let _ = std::fs::remove_dir_all(&d);
}
