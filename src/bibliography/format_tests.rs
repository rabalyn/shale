//! The formatter of EDIT-127: the five styles on the entries of the Better BibTeX test file.

use super::format::{render, summary, Rendered, Style};
use super::reader;
use crate::markdown::citations::{scan, Token};
use crate::thesis::Lang;

const BETTER_BIBTEX: &str = include_str!("../testdata/bib/better-bibtex.bib");

fn render_text(text: &str, style: Style, lang: Lang) -> Rendered {
    let parsed = reader::parse(BETTER_BIBTEX);
    let find = |k: &str| parsed.entries.iter().find(|e| e.key == k).and_then(|e| e.csl.clone());
    let has = |k: &str| find(k).is_some();
    let groups: Vec<_> = scan(text, Some(&has)).into_iter().filter_map(|t| if let Token::Group(g) = t { Some(g) } else { None }).collect();
    render(&groups.iter().collect::<Vec<_>>(), &find, style, lang)
}

fn cites(r: &Rendered) -> Vec<&str> {
    r.citations.iter().map(|c| c.as_deref().unwrap_or("-")).collect()
}

fn refs(r: &Rendered) -> Vec<&str> {
    r.references.iter().map(|r| r.key.as_str()).collect()
}

const TEXT: &str = "A [@knuth1984literate, p. 12], [see @lamport1994latex; @chen:2019.deep-learning] and @knuth1984literate, [-@mueller2020umlaut], [@zzz].";

// spec: EDIT-127
#[test]
fn edit_127_the_authoryear_style_writes_author_and_year_with_the_locator_prefix_prose_and_year_alone() {
    let r = render_text(TEXT, Style::AuthorYear, Lang::En);
    assert_eq!(cites(&r), ["(Knuth 1984, 12)", "(see Lamport 1994; Chen and Zhang 2019)", "Knuth (1984)", "2020", "-"]);
    assert_eq!(refs(&r), ["chen:2019.deep-learning", "knuth1984literate", "lamport1994latex", "mueller2020umlaut"], "sorted by author");
    assert!(r.references[1].text.starts_with("Knuth, Donald E. 1984. “Literate Programming.” The Computer Journal 27 (2): 97–111."), "{}", r.references[1].text);
    assert_eq!(r.references[1].label, None);
}

// spec: EDIT-127
#[test]
fn edit_127_the_numeric_style_numbers_by_first_citation_and_labels_the_list() {
    let r = render_text("[@lamport1994latex] then [@knuth1984literate, p. 12] and [@lamport1994latex; @knuth1984literate]", Style::Numeric, Lang::En);
    assert_eq!(cites(&r), ["[1]", "[2, p. 12]", "[1, 2]"], "a locator that the style leaves out is written after the number");
    assert_eq!(refs(&r), ["lamport1994latex", "knuth1984literate"]);
    assert_eq!(r.references.iter().map(|r| r.label.as_deref()).collect::<Vec<_>>(), [Some("[1]"), Some("[2]")]);
}

// spec: EDIT-127
#[test]
fn edit_127_the_alphabetic_style_cites_by_label_and_lists_by_label() {
    let r = render_text(TEXT, Style::Alphabetic, Lang::En);
    assert_eq!(cites(&r)[0], "[Knu84, 12]");
    assert_eq!(cites(&r)[1], "[see CZ19, Lam94]");
    assert_eq!(r.references.iter().map(|r| r.label.as_deref().unwrap_or("")).collect::<Vec<_>>(), ["[CZ19]", "[Knu84]", "[Lam94]", "[MS20]"], "the labels are the ones of the citations, in their order");
    assert!(r.references[1].text.starts_with("Knuth, Donald E. 1984."), "{}", r.references[1].text);
}

// spec: EDIT-127
#[test]
fn edit_127_the_ieee_style_has_brackets_numbers_and_the_reference_label() {
    let r = render_text(TEXT, Style::Ieee, Lang::En);
    assert_eq!(cites(&r)[0], "[1, p. 12]");
    assert_eq!(r.references[0].label.as_deref(), Some("[1]"));
    assert!(r.references[0].text.starts_with("D. E. Knuth, “Literate Programming,” The Computer Journal, vol. 27, no. 2, pp. 97–111"), "{}", r.references[0].text);
}

// spec: EDIT-127
#[test]
fn edit_127_the_apa_style_writes_comma_year_and_an_ampersand() {
    let r = render_text(TEXT, Style::Apa, Lang::En);
    assert_eq!(cites(&r), ["(Knuth, 1984, p. 12)", "(see Chen & Zhang, 2019; Lamport, 1994)", "Knuth (1984)", "2020", "-"]);
    assert!(r.references[1].text.starts_with("Knuth, D. E. (1984). Literate Programming. The Computer Journal, 27(2), 97–111."), "{}", r.references[1].text);
}

// spec: EDIT-127
#[test]
fn edit_127_the_language_of_the_document_sets_the_terms() {
    let en = render_text("[@chen:2019.deep-learning]", Style::AuthorYear, Lang::En);
    let de = render_text("[@chen:2019.deep-learning]", Style::AuthorYear, Lang::De);
    assert_eq!(cites(&en), ["(Chen and Zhang 2019)"]);
    assert_eq!(cites(&de), ["(Chen und Zhang 2019)"]);
}

// spec: EDIT-127
#[test]
fn edit_127_a_suffix_that_is_no_locator_is_written_as_it_is_and_a_key_the_bibliography_lacks_is_left_out() {
    let r = render_text("[@knuth1984literate, esp. the introduction] and [@nobody; @lamport1994latex] and [@nobody]", Style::AuthorYear, Lang::En);
    assert_eq!(cites(&r), ["(Knuth 1984, esp. the introduction)", "(Lamport 1994)", "-"]);
    assert_eq!(refs(&r), ["knuth1984literate", "lamport1994latex"], "only the entries that were cited are listed");
}

// spec: EDIT-127
#[test]
fn edit_127_nothing_cited_gives_nothing() {
    assert_eq!(render_text("No citation here.", Style::AuthorYear, Lang::En), Rendered::default());
}

// spec: EDIT-127
#[test]
fn edit_127_and_others_is_no_person_in_the_list_of_authors() {
    let r = render_text("[@mueller2020umlaut]", Style::AuthorYear, Lang::En);
    assert_eq!(cites(&r), ["(Müller and Schön 2020)"]);
    assert!(!r.references[0].text.to_lowercase().contains("others"), "{}", r.references[0].text);
}

// spec: EDIT-127
#[test]
fn edit_127_the_property_names_one_of_the_five_styles() {
    assert_eq!(Style::property("---\ncitation-style: ieee\n---\n"), Some(Style::Ieee));
    assert_eq!(Style::property("---\ncitation-style: Author-Year\n---\n"), Some(Style::AuthorYear));
    assert_eq!(Style::property("---\ncitation-style: numeric\n---\n"), Some(Style::Numeric));
    assert_eq!(Style::property("---\ncitation-style: chicago\n---\n"), None);
    assert_eq!(Style::property("text"), None);
    assert_eq!(Style::default(), Style::AuthorYear);
    assert_eq!(Style::ALL.map(Style::name), ["numeric", "authoryear", "alphabetic", "ieee", "apa"]);
}

// spec: EDIT-125
#[test]
fn edit_125_the_summary_of_an_entry_is_authors_year_title_and_venue() {
    let parsed = reader::parse(BETTER_BIBTEX);
    let of = |k: &str| summary(parsed.entries.iter().find(|e| e.key == k).expect("entry"));
    assert_eq!(of("knuth1984literate"), "Knuth (1984): Literate Programming. The Computer Journal");
    assert_eq!(of("chen:2019.deep-learning"), "Chen and Zhang (2019): Deep learning for notes: a survey. Proceedings of the 36th International Conference");
    assert_eq!(of("mueller2020umlaut"), "Müller et al. (2020): Über die Bedeutung von ähnlichen Zeichen. Zeitschrift für Beispiele");
    assert_eq!(of("lamport1994latex"), "Lamport (1994): LaTeX: A Document Preparation System. Addison-Wesley");
}
