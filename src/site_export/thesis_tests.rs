//! Citations, numbering and the bibliography in the site (SITE-077, ADR-0039, #648).

use super::test_support::{page, Fixture};

const BIB: &str = "@book{a, author = {Meier, Anna}, title = {Alpha}, date = {2020}, publisher = {P}}\n@book{b, author = {Schulz, Bo}, title = {Beta}, date = {2021}, publisher = {Q}}\n";

const NOTE: &str = "---\nbibliography: refs.bib\n---\nSee @fig:p, @tbl:t, @eq:e and @sec:m, as [@a, p. 3] says.\n\n![[plot.png|Plot of runs]]{#fig:p}\n\n| x | y |\n|---|---|\n| cellone | celltwo |\n: Runs of the day {#tbl:t}\n\n$$ E = mc^2 $$ {#eq:e}\n\n## Method {#sec:m}\n\nEnd.\n";

fn fixture(tag: &str, note: &str) -> Fixture {
    let f = Fixture::with(tag, "\"all_notes\": true", &[("Thesis.md", note), ("refs.bib", BIB)]);
    f.bytes("plot.png", &{
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(4, 3, image::Rgba([200, 20, 20, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        png
    });
    f
}

fn thesis_page(note: &str) -> (String, Vec<String>) {
    let f = fixture("page", note);
    let e = f.export();
    (page(&e, "thesis.md"), e.warnings.clone())
}

// spec: SITE-077
#[test]
fn site_077_figures_tables_and_equations_are_numbered_with_their_captions_and_anchors() {
    let (p, warnings) = thesis_page(NOTE);
    assert!(p.contains("<figure id=\"fig-p\" class=\"shale-figure\">"), "{p}");
    assert!(p.contains("<figcaption>Figure 1: Plot of runs</figcaption>"), "{p}");
    assert!(p.contains("<figure id=\"tbl-t\" class=\"shale-table\">") && p.contains("<figcaption>Table 1: Runs of the day</figcaption>"), "{p}");
    assert!(p.contains("<div id=\"eq-e\" class=\"shale-equation\">") && p.contains("<span class=\"eq-number\">(1)</span>"), "{p}");
    assert!(p.contains("<a id=\"sec-m\"></a>"), "{p}");
    assert!(!p.contains("{#") && !p.contains("\n: Runs of the day"), "labels and the caption line are gone: {p}");
    assert!(warnings.is_empty(), "{warnings:?}");
}

// spec: SITE-077
#[test]
fn site_077_captions_are_first_or_last_as_the_caption_properties_say() {
    let (p, _) = thesis_page(NOTE);
    assert!(p.find("<figcaption>Table").unwrap() < p.find("cellone").unwrap(), "tables above by default");
    assert!(p.find("<figcaption>Figure").unwrap() > p.find("plot.png").unwrap(), "figures below by default: {p}");
    let (p, _) = thesis_page(&NOTE.replace("bibliography: refs.bib", "bibliography: refs.bib\nfigure-caption: above\ntable-caption: below"));
    assert!(p.find("<figcaption>Table").unwrap() > p.find("celltwo").unwrap());
    assert!(p.find("<figcaption>Figure").unwrap() < p.find("plot.png").unwrap(), "{p}");
}

// spec: SITE-077
#[test]
fn site_077_references_and_citations_are_links_to_their_place() {
    let (p, _) = thesis_page(NOTE);
    for (href, text) in [("fig-p", "Figure 1"), ("tbl-t", "Table 1"), ("eq-e", "Equation (1)"), ("sec-m", "Section 1")] {
        assert!(p.contains(&format!("<a class=\"thesis-ref\" href=\"#{href}\">{text}</a>")), "{href}: {p}");
    }
    assert!(p.contains("href=\"#cite-a\">") && p.contains("Meier"), "{p}");
    assert!(p.contains("id=\"cite-a\""), "the bibliography has the entry: {p}");
}

// spec: SITE-077
#[test]
fn site_077_the_bibliography_is_at_the_end_or_at_its_marker_and_in_the_language_of_the_note() {
    let (p, _) = thesis_page(NOTE);
    assert!(p.contains("## References") && p.find("## References").unwrap() > p.find("End.").unwrap(), "{p}");
    assert!(p.contains("class=\"bib-entry\"") && p.contains("Alpha") && !p.contains("Beta"), "only what is cited: {p}");
    let marked = "---\nbibliography: refs.bib\nlanguage: de\n---\nSee [@b].\n\n<!-- shale: bibliography -->\n\nAfter.\n";
    let (p, _) = thesis_page(marked);
    assert!(p.find("## Literaturverzeichnis").unwrap() < p.find("After.").unwrap(), "{p}");
    assert_eq!(p.matches("Literaturverzeichnis").count(), 1, "once, at the marker: {p}");
    assert!(!p.contains("shale: bibliography"));
}

// spec: SITE-077
#[test]
fn site_077_lists_of_figures_and_tables_link_to_their_entries_and_an_empty_one_is_nothing() {
    let note = "---\nbibliography: refs.bib\n---\n<!-- shale: list-of-figures -->\n\n<!-- shale: list-of-tables -->\n\n![[plot.png|Plot]]{#fig:p}\n";
    let (p, _) = thesis_page(note);
    assert!(p.contains("## List of Figures") && p.contains("<li><a href=\"#fig-p\">Figure 1: Plot</a></li>"), "{p}");
    assert!(!p.contains("List of Tables") && !p.contains("shale: list"), "{p}");
}

// spec: SITE-077
#[test]
fn site_077_a_heading_with_a_label_keeps_the_anchor_of_its_text() {
    let anchors = super::anchor::anchors("## Method {#sec:m}\n\n## Other {#nolabel}\n");
    assert_eq!(anchors[0].id, "method");
    assert_eq!(anchors[1].id, "other-nolabel");
}

// spec: SITE-077
#[test]
fn site_077_the_export_warns_of_what_does_not_resolve_and_a_note_without_the_syntax_is_unchanged() {
    let (_, warnings) = thesis_page("---\nbibliography: refs.bib\n---\nSee [@nokey] and @fig:zz.\n");
    let all = warnings.join("\n");
    assert!(all.contains("Thesis.md") && all.contains("nokey") && all.contains("fig:zz"), "{all}");
    let (p, warnings) = thesis_page("Mail ada@host.org costs $5.\n");
    assert!(p.contains("ada@host.org") && warnings.is_empty(), "{p} {warnings:?}");
}
