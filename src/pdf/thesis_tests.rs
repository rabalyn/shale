//! Citations, numbering and the bibliography in the built-in PDF (PDF-032, ADR-0039, #648).

use crate::pdf::export::tests::{contents, hex};
use crate::pdf::export::{export_note_until, ExportOptions};
use crate::vault::Vault;
use std::path::{Path, PathBuf};

const BIB: &str = "@book{a, author = {Meier, Anna}, title = {Alpha}, date = {2020}, publisher = {P}}\n@book{b, author = {Schulz, Bo}, title = {Beta}, date = {2021}, publisher = {Q}}\n";

/// A vault with the bibliography `refs.bib`, a picture `plot.png` and these notes.
fn vault_with(tag: &str, notes: &[(&str, &str)]) -> (Vault, PathBuf) {
    let dir = std::env::temp_dir().join(format!("shale-pdf-thesis-{tag}-{}", std::process::id())).join("vault");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("refs.bib"), BIB).unwrap();
    image::RgbaImage::from_pixel(40, 30, image::Rgba([230, 20, 20, 255])).save(dir.join("plot.png")).unwrap();
    for (name, text) in notes {
        std::fs::write(dir.join(name), text).unwrap();
    }
    (Vault::open(&dir).unwrap(), dir)
}

fn plain() -> ExportOptions {
    ExportOptions { title: false, toc: false, page_numbers: false, ..ExportOptions::for_note() }
}

/// The note exported: the PDF's bytes and what the export has to say.
fn export(v: &Vault, name: &str, opts: &ExportOptions) -> (Vec<u8>, Vec<String>) {
    let text = v.notes[Path::new(name)].text.clone();
    let made = export_note_until(v, Path::new(name), &text, opts, None).unwrap();
    (made.bytes, made.warnings)
}

/// Everything the PDF draws as text, in the order it is drawn.
fn drawn(bytes: &[u8]) -> String {
    let c = contents(bytes);
    let mut out = String::new();
    let mut from = 0;
    while let Some(at) = c[from..].find("> Tj").map(|i| i + from) {
        let open = c[..at].rfind('<').unwrap();
        out.extend((open + 1..at).step_by(2).map(|i| u8::from_str_radix(&c[i..i + 2], 16).unwrap() as char));
        from = at + 4;
    }
    out
}

fn clean(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
}

const NOTE: &str = "---\nbibliography: refs.bib\n---\nStart, see @fig:p and [@tbl:t] and @eq:e and @sec:m, as [@a, p. 3] says.\n\n![[plot.png|Plot of runs]]{#fig:p}\n\n| x | y |\n|---|---|\n| cellone | celltwo |\n: Runs of the day {#tbl:t}\n\n$$ E = mc^2 $$ {#eq:e}\n\n## Method {#sec:m}\n\nEnd text.\n";

// spec: PDF-032
#[test]
fn pdf_032_figures_tables_and_equations_have_numbers_captions_and_no_labels() {
    let (v, d) = vault_with("numbers", &[("N.md", NOTE)]);
    let (bytes, warnings) = export(&v, "N.md", &plain());
    let t = drawn(&bytes);
    assert!(t.contains("Figure 1: Plot of runs"), "{t}");
    assert!(t.contains("Table 1: Runs of the day"), "{t}");
    assert!(t.contains("(1)"), "the number of the equation: {t}");
    assert!(!t.contains("{#") && !t.contains("@fig"), "labels are not printed: {t}");
    assert!(!t.contains(": Runs of the day {"), "the caption line is not text: {t}");
    assert!(warnings.is_empty(), "{warnings:?}");
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_captions_stand_above_or_below_as_the_properties_say_tables_above_and_figures_below_by_default() {
    let (v, d) = vault_with("position", &[("N.md", NOTE), ("Flip.md", &NOTE.replace("bibliography: refs.bib", "bibliography: refs.bib\nfigure-caption: above\ntable-caption: below"))]);
    let (bytes, _) = export(&v, "N.md", &plain());
    let (c, t) = (contents(&bytes), drawn(&bytes));
    let image = c.find(" Do").expect("the picture");
    assert!(c.find(&hex("Plot")).unwrap() > image, "a figure's caption is below by default");
    assert!(t.find("Table 1:").unwrap() < t.find("cellone").unwrap(), "a table's caption is above by default: {t}");
    let (bytes, _) = export(&v, "Flip.md", &plain());
    let (c, t) = (contents(&bytes), drawn(&bytes));
    let image = c.find(" Do").expect("the picture");
    assert!(c.find(&hex("Plot")).unwrap() < image, "figure-caption: above");
    assert!(t.find("Table 1:").unwrap() > t.find("celltwo").unwrap(), "table-caption: below: {t}");
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_references_are_word_and_number_citations_are_formatted_and_both_link_to_their_place() {
    let (v, d) = vault_with("refs", &[("N.md", NOTE)]);
    let (bytes, _) = export(&v, "N.md", &plain());
    let t = drawn(&bytes);
    assert!(t.contains("see Figure 1 and Table 1 and Equation (1) and Section 1,"), "{t}");
    assert!(t.contains("(Meier 2020, 3)") || t.contains("(Meier 2020, p. 3)"), "{t}");
    let raw = String::from_utf8_lossy(&bytes);
    assert!(raw.matches("/Dest").count() >= 5, "four references and a citation are links: {}", raw.matches("/Dest").count());
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_the_bibliography_is_at_the_end_or_at_its_marker_and_in_the_contents() {
    let end = "---\nbibliography: refs.bib\nlanguage: de\n---\n# Kapitel\n\nSiehe [@b].\n\nSchluss.\n";
    let marked = "---\nbibliography: refs.bib\n---\n# Chapter\n\nSee [@a].\n\n<!-- shale: bibliography -->\n\nAfter marker.\n";
    let (v, d) = vault_with("bib", &[("End.md", end), ("Marked.md", marked)]);
    let opts = ExportOptions { toc: true, toc_break: false, ..plain() };
    let (bytes, _) = export(&v, "End.md", &opts);
    let t = drawn(&bytes);
    assert!(t.matches("Literaturverzeichnis").count() >= 2, "heading and contents entry: {t}");
    assert!(t.find("Schluss.").unwrap() < t.find("Literaturverzeichnis").unwrap(), "at the end: {t}");
    assert!(t.contains("Schulz") && t.contains("Beta"), "{t}");
    assert!(!t.contains("Meier"), "only what is cited: {t}");
    let (bytes, _) = export(&v, "Marked.md", &opts);
    let t = drawn(&bytes);
    assert!(t.find("References").unwrap() < t.find("After marker.").unwrap(), "at the marker: {t}");
    assert!(!t.contains("shale: bibliography"), "{t}");
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_a_numeric_style_labels_the_entries() {
    let note = "---\nbibliography: refs.bib\ncitation-style: numeric\n---\nSee [@b] and [@a].\n";
    let (v, d) = vault_with("numeric", &[("N.md", note)]);
    let (bytes, _) = export(&v, "N.md", &plain());
    let t = drawn(&bytes);
    assert!(t.contains("See [1] and [2]."), "{t}");
    assert!(t.contains("[1]") && t.find("Schulz").unwrap() < t.find("Meier").unwrap(), "{t}");
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_lists_of_figures_and_tables_stand_at_their_markers_with_their_captions() {
    let note = "---\nbibliography: refs.bib\n---\n<!-- shale: list-of-figures -->\n\n<!-- shale: list-of-tables -->\n\n![[plot.png|Plot of runs]]{#fig:p}\n\n| x |\n|---|\n| cell |\n: Runs {#tbl:t}\n";
    let (v, d) = vault_with("lists", &[("N.md", note)]);
    let (bytes, _) = export(&v, "N.md", &plain());
    let t = drawn(&bytes);
    assert!(t.contains("List of Figures") && t.contains("List of Tables"), "{t}");
    assert_eq!(t.matches("Figure 1: Plot of runs").count(), 2, "in the list and at the figure: {t}");
    assert_eq!(t.matches("Table 1: Runs").count(), 2, "{t}");
    assert!(!t.contains("shale: list"), "{t}");
    // a list with nothing in it prints nothing, and not its marker
    let empty = "---\nbibliography: refs.bib\n---\n<!-- shale: list-of-tables -->\n\nText @a.\n";
    let (v2, d2) = vault_with("lists-empty", &[("E.md", empty)]);
    let (bytes, _) = export(&v2, "E.md", &plain());
    let t = drawn(&bytes);
    assert!(!t.contains("List of Tables") && !t.contains("shale:"), "{t}");
    clean(&d);
    clean(&d2);
}

// spec: PDF-032
#[test]
fn pdf_032_the_export_warns_of_unknown_keys_and_labels_double_labels_and_a_missing_bibliography() {
    let note = "---\nbibliography: refs.bib\n---\nSee [@nokey] and @fig:zz.\n\n![[plot.png|A]]{#fig:p}\n\n![[plot.png|B]]{#fig:p}\n";
    let (v, d) = vault_with("warn", &[("N.md", note), ("M.md", "---\nbibliography: missing.bib\n---\nSee [@a].\n"), ("Q.md", "No bibliography, so [@a] is as written.\n")]);
    let (bytes, warnings) = export(&v, "N.md", &plain());
    let all = warnings.join("\n");
    assert!(all.contains("nokey"), "{all}");
    assert!(all.contains("no label") && all.contains("fig:zz"), "{all}");
    assert!(all.contains("used twice"), "{all}");
    assert!(drawn(&bytes).contains("[@nokey]"), "an unknown key stays as written");
    let (_, warnings) = export(&v, "M.md", &plain());
    assert!(warnings.iter().any(|w| w.contains("missing.bib")), "{warnings:?}");
    let (bytes, warnings) = export(&v, "Q.md", &plain());
    assert!(warnings.iter().any(|w| w.contains("no bibliography")), "{warnings:?}");
    assert!(drawn(&bytes).contains("[@a]"));
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_a_note_without_the_syntax_is_printed_as_before() {
    let (v, d) = vault_with("plain", &[("N.md", "Write to ada@host.org or @todo, costs $5.\n")]);
    let (bytes, warnings) = export(&v, "N.md", &plain());
    let t = drawn(&bytes);
    assert!(t.contains("ada@host.org") && t.contains("@todo"), "{t}");
    assert!(warnings.is_empty(), "{warnings:?}");
    clean(&d);
}

const WITH_FLOATS: &str = "---\nbibliography: refs.bib\n---\nIntro text.\n\n![[plot.png|Plot of runs]]{#fig:p}\n\n| x |\n|---|\n| cell |\n: Runs {#tbl:t}\n";

// spec: PDF-032
#[test]
fn pdf_032_the_boxes_print_the_lists_after_the_contents_and_off_they_print_none() {
    let (v, d) = vault_with("boxes", &[("N.md", WITH_FLOATS)]);
    let on = ExportOptions { toc: true, toc_break: false, list_of_figures: true, list_of_tables: true, ..plain() };
    let t = drawn(&export(&v, "N.md", &on).0);
    assert!(t.contains("List of Figures") && t.contains("List of Tables"), "{t}");
    assert!(t.find("List of Tables").unwrap() < t.find("Intro text.").unwrap(), "after the contents, before the text: {t}");
    assert_eq!(t.matches("Figure 1: Plot of runs").count(), 2, "in the list and at the figure: {t}");
    assert_eq!(t.matches("List of Figures").count(), 2, "heading, and its entry in the contents: {t}");
    let only_figures = ExportOptions { list_of_tables: false, ..on.clone() };
    let t = drawn(&export(&v, "N.md", &only_figures).0);
    assert!(t.contains("List of Figures") && !t.contains("List of Tables"), "{t}");
    let off = ExportOptions { list_of_figures: false, list_of_tables: false, ..on };
    let t = drawn(&export(&v, "N.md", &off).0);
    assert!(!t.contains("List of"), "{t}");
    clean(&d);
}

// spec: PDF-032
#[test]
fn pdf_032_a_marker_wins_over_its_box_so_a_list_is_never_printed_twice() {
    let note = WITH_FLOATS.replace("Intro text.", "Intro text.\n\n<!-- shale: list-of-figures -->\n\nMiddle text.");
    let (v, d) = vault_with("boxes-marker", &[("N.md", &note)]);
    let on = ExportOptions { list_of_figures: true, list_of_tables: true, ..plain() };
    let t = drawn(&export(&v, "N.md", &on).0);
    assert_eq!(t.matches("List of Figures").count(), 1, "the marker's list only: {t}");
    assert!(t.find("Intro text.").unwrap() < t.find("List of Figures").unwrap() && t.find("List of Figures").unwrap() < t.find("Middle text.").unwrap(), "{t}");
    assert!(t.contains("List of Tables"), "the other list has no marker, so its box counts: {t}");
    assert!(t.find("List of Tables").unwrap() < t.find("Intro text.").unwrap(), "{t}");
    // a box for a list with nothing in it prints nothing
    let (v2, d2) = vault_with("boxes-empty", &[("E.md", "---\nbibliography: refs.bib\n---\nText @a.\n")]);
    let t = drawn(&export(&v2, "E.md", &on).0);
    assert!(!t.contains("List of"), "{t}");
    clean(&d);
    clean(&d2);
}

// spec: PDF-032
#[test]
fn pdf_032_the_citation_style_of_the_dialog_wins_over_the_property_of_the_note() {
    let note = "---\nbibliography: refs.bib\ncitation-style: apa\n---\nSee [@a].\n";
    let (v, d) = vault_with("style", &[("N.md", note)]);
    assert!(drawn(&export(&v, "N.md", &plain()).0).contains("(Meier, 2020)"), "the note's style");
    let numeric = ExportOptions { citation_style: Some(crate::bibliography::format::Style::Numeric), ..plain() };
    assert!(drawn(&export(&v, "N.md", &numeric).0).contains("See [1]."), "the dialog's style");
    clean(&d);
}
