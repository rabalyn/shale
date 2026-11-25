//! The thesis syntax in the TeX export (TEX-044 to TEX-047): citations, labelled figures, tables,
//! equations and sections, references, the bibliography and the lists. Against LuaLaTeX and biber:
//! `thesis_env_tests.rs`.

use super::book::{write_book, BookRequest};
use super::sets::*;
use super::writer::*;
use super::writer_tests::{fields, kind, temp_vault, tuda_set};
use crate::bibliography::format::Style;
use crate::pdf::export::ExportOptions;
use std::path::Path;

pub(super) const BIB: &str = include_str!("../../testdata/tex/thesis/refs.bib");
const THESIS: &str = include_str!("../../testdata/tex/thesis/Thesis/Thesis.md");
const INTRO: &str = include_str!("../../testdata/tex/thesis/Thesis/01 Introduction.md");
const METHOD: &str = include_str!("../../testdata/tex/thesis/Thesis/02 Method.md");

/// A picture the sample's figures embed.
pub(super) fn plot() -> Vec<u8> {
    let mut out = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(80, 40, image::Rgba([30, 120, 200, 255])).write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// The sample thesis as a vault: a book of two chapters with a `.bib` file and a picture.
pub(super) fn sample_vault(tag: &str) -> (crate::vault::Vault, std::path::PathBuf) {
    let png = plot();
    let (_, dir) = temp_vault(tag, &[]);
    for (name, bytes) in [("refs.bib", BIB.as_bytes()), ("plot.png", png.as_slice()), ("Thesis/Thesis.md", THESIS.as_bytes()), ("Thesis/01 Introduction.md", INTRO.as_bytes()), ("Thesis/02 Method.md", METHOD.as_bytes())] {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    (crate::vault::Vault::open(&dir).unwrap(), dir)
}

/// A thesis of three chapters (BOOK-020, BOOK-021): the top folder note has the book's properties
/// and cites `knuth1984`; every chapter has a figure and refers to the others; chapter three has a
/// table, an equation and a section; `lamport1994` is cited in two chapters. `extra` replaces files.
pub(super) fn three_chapters(tag: &str, top_properties: &str, extra: &[(&str, &str)]) -> (crate::vault::Vault, std::path::PathBuf) {
    let top = format!("---\ntitle: Thesis\norder: [One, Two, Three]\nbibliography: refs.bib\nlanguage: en\n{top_properties}---\nThe book starts with [@knuth1984].\n");
    let one = "# One {#sec:one}\n\n![[plot.png|First plot]]{#fig:one}\n\nSee @fig:two, and @tbl:t of @sec:three. Cited [@lamport1994].\n";
    let two = "# Two {#sec:two}\n\n![[plot.png|Second plot]]{#fig:two}\n\nBack to @fig:one and [-@fig:one], as [@lamport1994; @knuth1984] say.\n";
    let three = "# Three {#sec:three}\n\n## Method {#sec:method}\n\n| x | y |\n|---|---|\n| cellone | celltwo |\n: Runs of the day {#tbl:t}\n\n$$ E = mc^2 $$ {#eq:e}\n\nBack in @sec:two, @eq:e and @fig:two. See @sec:method.\n";
    let png = plot();
    let mut files: Vec<(&str, &[u8])> =
        vec![("refs.bib", BIB.as_bytes()), ("plot.png", &png), ("Thesis/Thesis.md", top.as_bytes()), ("Thesis/One.md", one.as_bytes()), ("Thesis/Two.md", two.as_bytes()), ("Thesis/Three.md", three.as_bytes())];
    for (name, text) in extra {
        files.retain(|(n, _)| n != name);
        files.push((name, text.as_bytes()));
    }
    let (_, dir) = temp_vault(tag, &[]);
    for (name, bytes) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    (crate::vault::Vault::open(&dir).unwrap(), dir)
}

/// The sample thesis as a kind of the tuda set.
pub(super) fn sample_as(kind_id: &str, opts: &ExportOptions, settings: SetSettings) -> Document {
    sample_in(&tuda_set(settings), kind_id, opts)
}

/// The sample thesis as a kind of any set.
pub(super) fn sample_in(set: &SetInfo, kind_id: &str, opts: &ExportOptions) -> Document {
    let (v, dir) = sample_vault("thesis-sample");
    let plan = crate::pdf::book::plan(&v, Path::new("Thesis")).unwrap();
    let doc = write_book(&BookRequest { vault: &v, plan: &plan, opts, set, kind: kind(set, kind_id), fields: &fields(&[("author", "Ada")]), fonts: None }).unwrap_or_else(|e| panic!("{e}"));
    let _ = std::fs::remove_dir_all(dir);
    doc
}

/// A note as a kind of the tuda set, in a vault that has the sample's `.bib` file and picture.
fn note_as(text: &str, kind_id: &str, opts: &ExportOptions, settings: SetSettings) -> Document {
    let png = plot();
    let (v, dir) = temp_vault("thesis-note", &[("Plan.md", text.as_bytes()), ("refs.bib", BIB.as_bytes()), ("plot.png", &png)]);
    let set = tuda_set(settings);
    let doc = write(&Request { vault: &v, rel: Path::new("Plan.md"), text, opts, set: &set, kind: kind(&set, kind_id), fields: &fields(&[("author", "Ada")]), fonts: None }).unwrap_or_else(|e| panic!("{e}"));
    let _ = std::fs::remove_dir_all(dir);
    doc
}

fn note(text: &str) -> Document {
    note_as(text, "pub", &ExportOptions::for_note(), SetSettings::default())
}

fn tex(text: &str) -> String {
    note(text).tex
}

const FIG: &str = "![[plot.png|Results]]{#fig:r}\n";

fn with_bib(body: &str) -> String {
    format!("---\nbibliography: refs.bib\n---\n{body}")
}

// spec: TEX-044
#[test]
fn tex_044_every_form_of_a_citation_is_a_biblatex_command() {
    let t = tex(&with_bib("A @knuth1984 and @lamport1994 [p. 3] and [@knuth1984] and [@knuth1984, p. 12] and [see @knuth1984, p. 12] and [@knuth1984; @lamport1994] and [-@knuth1984].\n"));
    for want in ["\\textcite{knuth1984}", "\\textcite[p. 3]{lamport1994}", "\\autocite{knuth1984}", "\\autocite[p. 12]{knuth1984}", "\\autocite[see][p. 12]{knuth1984}", "\\autocites{knuth1984}{lamport1994}", "\\autocite*{knuth1984}"] {
        assert!(t.contains(want), "{want} in\n{t}");
    }
}

// spec: TEX-044
#[test]
fn tex_044_the_preamble_has_biblatex_with_biber_the_style_and_the_bib_file_travels_with_the_document() {
    let text = with_bib("See [@knuth1984].\n");
    let d = note(&text);
    assert!(d.tex.contains("\\usepackage[backend=biber,style=authoryear]{biblatex}"), "{}", d.tex);
    assert!(d.tex.contains("\\addbibresource{bibliography-1.bib}"));
    assert!(d.files.iter().any(|(n, b)| n == "bibliography-1.bib" && b == BIB.as_bytes()));
    // the order: the dialog, the note, the set, authoryear
    let note_style = "---\nbibliography: refs.bib\ncitation-style: ieee\n---\nSee [@knuth1984].\n";
    assert!(tex(note_style).contains("style=ieee"));
    let set = SetSettings { citation_style: "alphabetic".into(), ..SetSettings::default() };
    assert!(note_as(&text, "pub", &ExportOptions::for_note(), set.clone()).tex.contains("style=alphabetic"));
    assert!(note_as(note_style, "pub", &ExportOptions::for_note(), set.clone()).tex.contains("style=ieee"), "the note beats the set");
    let mut opts = ExportOptions::for_note();
    opts.thesis.style = Some(Style::Apa);
    assert!(note_as(note_style, "pub", &opts, set).tex.contains("style=apa"), "the dialog beats the note");
}

// spec: TEX-044
#[test]
fn tex_044_the_vault_default_bibliography_applies_when_the_note_names_none() {
    let mut opts = ExportOptions::for_note();
    opts.thesis.default_bibliography = "refs.bib".into();
    let d = note_as("See [@knuth1984].\n", "pub", &opts, SetSettings::default());
    assert!(d.tex.contains("\\autocite{knuth1984}") && d.tex.contains("\\addbibresource{bibliography-1.bib}"), "{}", d.tex);
}

// spec: TEX-044
#[test]
fn tex_044_an_at_sign_that_is_no_citation_stays_text() {
    let t = tex(&with_bib("Mail ada@host.org, a mention @ada@mastodon.social, `@knuth1984` in code, $@knuth1984$, a word @todo, @unknown.\n\n```\n[@knuth1984]\n```\n"));
    assert!(!t.contains("\\autocite") && !t.contains("\\textcite"), "{t}");
    assert!(t.contains("ada@host.org") && t.contains("@ada@mastodon.social") && t.contains("@todo"), "{t}");
    // a note of somebody else's text has no citations at all
    let t = tex("---\ntype: email\nbibliography: refs.bib\n---\nSee [@knuth1984].\n");
    assert!(!t.contains("\\autocite"), "{t}");
}

// spec: TEX-044
#[test]
fn tex_044_citations_without_a_bibliography_and_unknown_keys_are_text_and_a_warning() {
    let d = note("See [@knuth1984].\n");
    assert!(!d.tex.contains("\\autocite") && !d.tex.contains("biblatex") && d.tex.contains("[@knuth1984]"), "{}", d.tex);
    assert!(d.warnings.iter().any(|w| w.contains("no bibliography is in effect")), "{:?}", d.warnings);
    let d = note(&with_bib("See [@nobody2000] and [@knuth1984].\n"));
    assert!(d.tex.contains("[@nobody2000]") && d.tex.contains("\\autocite{knuth1984}"), "{}", d.tex);
    assert!(d.warnings.iter().any(|w| w.contains("nobody2000")), "{:?}", d.warnings);
    let d = note("---\nbibliography: missing.bib\n---\nSee [@knuth1984].\n");
    assert!(d.warnings.iter().any(|w| w.contains("missing.bib")), "{:?}", d.warnings);
}

// spec: TEX-045
#[test]
fn tex_045_a_labelled_figure_and_table_are_floats_with_caption_and_label_in_the_default_places() {
    let t = tex("![[plot.png|Results]]{#fig:r}\n\n| a | b |\n|---|---|\n| 1 | 2 |\n: The runs {#tbl:t}\n");
    let figure = t.split("\\begin{figure}").nth(1).expect("a figure").split("\\end{figure}").next().unwrap();
    assert!(figure.find("\\includegraphics").unwrap() < figure.find("\\caption{Results}\\label{fig:r}").unwrap(), "a figure's caption is below: {figure}");
    let table = t.split("\\begin{table}").nth(1).expect("a table").split("\\end{table}").next().unwrap();
    assert!(table.find("\\caption{The runs}\\label{tbl:t}").unwrap() < table.find("\\begin{tabularx}").unwrap(), "a table's caption is above: {table}");
    assert!(!table.contains("xltabular"), "a float holds no breaking table");
    assert!(t.contains("\\captionsetup[table]{position=top}\\captionsetup[figure]{position=bottom}"), "{t}");
    assert!(t.contains("\\KOMAoptions{captions=tableheading}\\KOMAoptions{captions=figuresignature}"));
}

// spec: TEX-045
#[test]
fn tex_045_the_caption_position_comes_from_the_note_then_the_kind_then_the_set() {
    let body = "![[plot.png|R]]{#fig:r}\n\n| a |\n|---|\n| 1 |\n: T {#tbl:t}\n";
    let t = tex(&format!("---\ntable-caption: below\nfigure-caption: above\n---\n{body}"));
    assert!(t.contains("\\captionsetup[table]{position=bottom}\\captionsetup[figure]{position=top}"), "{t}");
    assert!(t.contains("\\KOMAoptions{captions=tablesignature}\\KOMAoptions{captions=figureheading}"));
    let figure = t.split("\\begin{figure}").nth(1).unwrap();
    assert!(figure.find("\\caption").unwrap() < figure.find("\\includegraphics").unwrap());
    // the set's key, and the note over it
    let set = SetSettings { table_caption: "below".into(), figure_caption: "above".into(), ..SetSettings::default() };
    assert!(note_as(body, "pub", &ExportOptions::for_note(), set.clone()).tex.contains("\\captionsetup[table]{position=bottom}\\captionsetup[figure]{position=top}"));
    let own = format!("---\ntable-caption: above\n---\n{body}");
    assert!(note_as(&own, "pub", &ExportOptions::for_note(), set).tex.contains("\\captionsetup[table]{position=top}\\captionsetup[figure]{position=top}"));
    // the kind's key beats the set's
    let (v, dir) = temp_vault("thesis-kind", &[("Plan.md", body.as_bytes()), ("plot.png", &plot())]);
    let mut set = tuda_set(SetSettings { table_caption: "above".into(), ..SetSettings::default() });
    for (id, k) in &mut set.kinds {
        if id == "pub" {
            k.table_caption = "below".into();
        }
    }
    let d = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: body, opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "Ada")]), fonts: None }).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(d.tex.contains("\\captionsetup[table]{position=bottom}"), "{}", d.tex);
}

// spec: TEX-045
#[test]
fn tex_045_references_are_cref_and_the_words_follow_the_language() {
    let text = "---\nlanguage: de\n---\n# Method {#sec:m}\n\nSee @fig:r, [@fig:r; @tbl:t], [-@fig:r] and @sec:m.\n\n![[plot.png|R]]{#fig:r}\n\n| a |\n|---|\n| 1 |\n: T {#tbl:t}\n";
    let t = tex(text);
    for want in [
        "\\cref{fig:r}",
        "\\cref{fig:r,tbl:t}",
        "\\labelcref{fig:r}",
        "\\cref{sec:m}",
        "\\label{sec:m}",
        "\\crefname{figure}{Abbildung}{Abbildungen}",
        "\\crefname{table}{Tabelle}{Tabellen}",
        "\\crefname{equation}{Gleichung}{Gleichungen}",
        "\\crefname{section}{Abschnitt}{Abschnitte}",
        "\\crefname{chapter}{Kapitel}{Kapitel}",
        "\\usepackage[ngerman]{cleveref}",
    ] {
        assert!(t.contains(want), "{want} in\n{t}");
    }
    assert!(!t.contains("{#sec:m}") && t.contains("\\section{Method}"), "the label is not part of the heading text: {t}");
    let en = tex(&text.replace("language: de", "language: en"));
    assert!(en.contains("\\crefname{figure}{Figure}{Figures}") && en.contains("\\crefname{section}{Section}{Sections}") && en.contains("\\usepackage[english]{cleveref}"), "{en}");
}

// spec: TEX-045
#[test]
fn tex_045_a_reference_without_a_label_and_a_label_twice_are_text_and_a_warning() {
    let d = note("See @fig:nowhere.\n\n![[plot.png|A]]{#fig:r}\n\n![[plot.png|B]]{#fig:r}\n");
    assert!(d.tex.contains("@fig:nowhere") && !d.tex.contains("\\cref{fig:nowhere}"), "{}", d.tex);
    assert!(d.warnings.iter().any(|w| w.contains("fig:nowhere")), "{:?}", d.warnings);
    assert_eq!(d.tex.matches("\\label{fig:r}").count(), 1, "the first label wins");
    assert!(d.warnings.iter().any(|w| w.contains("twice")), "{:?}", d.warnings);
}

// spec: TEX-045
#[test]
fn tex_045_an_embed_or_table_without_a_label_is_written_as_before() {
    let t = tex("![[plot.png|Plain]]\n\n| a |\n|---|\n| 1 |\n");
    assert!(!t.contains("\\begin{figure}") && !t.contains("\\begin{table}") && !t.contains("cleveref") && !t.contains("\\captionsetup"), "{t}");
    assert!(t.contains("\\textit{Plain}"));
}

// spec: TEX-045
#[test]
fn tex_045_a_book_counts_per_chapter_or_continuously_by_the_property_and_an_article_the_other_way() {
    let per_chapter = sample_as("thesis", &ExportOptions::for_note(), SetSettings::default()).tex;
    assert!(!per_chapter.contains("\\counterwithout") && !per_chapter.contains("\\counterwithin"), "a book class counts per chapter already");
    // an article counts continuously, and per section when the note says chapter
    assert!(!tex(FIG).contains("\\counterwithin"));
    let t = tex(&format!("---\nfloat-numbering: chapter\n---\n{FIG}"));
    assert!(t.contains("\\counterwithin{figure}{section}") && t.contains("\\counterwithin{equation}{section}"), "{t}");
    // a book that says continuous
    let (v, dir) = sample_vault("thesis-continuous");
    std::fs::write(dir.join("Thesis/Thesis.md"), "---\ntitle: T\nbibliography: refs.bib\nfloat-numbering: continuous\n---\n").unwrap();
    let v = crate::vault::Vault::open(&v.root).unwrap();
    let set = tuda_set(SetSettings::default());
    let plan = crate::pdf::book::plan(&v, Path::new("Thesis")).unwrap();
    let d = write_book(&BookRequest { vault: &v, plan: &plan, opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "thesis"), fields: &fields(&[("author", "Ada")]), fonts: None }).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(d.tex.contains("\\counterwithout{figure}{chapter}") && d.tex.contains("\\counterwithout{equation}{chapter}"), "{}", d.tex);
}

// spec: TEX-046
#[test]
fn tex_046_a_labelled_display_is_an_equation_with_one_number_and_an_unlabelled_one_is_as_before() {
    let t = tex("$$ E = mc^2 $$ {#eq:e}\n\n$$\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n$$ {#eq:al}\n\n$$ x^2 $$\n\nSee @eq:e and @eq:al.\n");
    assert!(t.contains("\\begin{equation}\n\\label{eq:e}\nE = mc^2\n\\end{equation}"), "{t}");
    assert!(t.contains("\\begin{equation}\n\\label{eq:al}\n\\begin{aligned} a &= b \\\\ c &= d \\end{aligned}\n\\end{equation}"), "{t}");
    assert!(t.contains("\\[\nx^2\n\\]"), "an unlabelled display stays one");
    assert!(t.contains("\\cref{eq:e}") && t.contains("\\cref{eq:al}"));
}

// spec: TEX-047
#[test]
fn tex_047_the_bibliography_is_printed_at_the_end_with_the_heading_of_the_language() {
    let t = tex(&with_bib("See [@knuth1984].\n"));
    assert!(t.contains("\\printbibliography[heading=bibintoc,title={References}]\n\n\\end{document}"), "{t}");
    let de = tex("---\nbibliography: refs.bib\nlanguage: de\n---\nSee [@knuth1984].\n");
    assert!(de.contains("title={Literaturverzeichnis}"), "{de}");
    // nothing cited, or the box off: no list
    assert!(!tex(&with_bib("Nothing.\n")).contains("printbibliography"));
    let mut opts = ExportOptions::for_note();
    opts.thesis.bibliography = false;
    assert!(!note_as(&with_bib("See [@knuth1984].\n"), "pub", &opts, SetSettings::default()).tex.contains("printbibliography"));
}

// spec: TEX-047
#[test]
fn tex_047_a_marker_puts_the_lists_where_it_stands_and_the_box_does_not_write_it_twice() {
    let text = with_bib(&format!("# One\n\nSee [@knuth1984].\n\n<!-- shale: list-of-figures -->\n\n<!-- shale: list-of-tables -->\n\n{FIG}\n| a |\n|---|\n| 1 |\n: T {{#tbl:t}}\n\n<!-- shale: bibliography -->\n\nAfter.\n"));
    let mut opts = ExportOptions::for_note();
    opts.thesis.list_of_figures = true;
    opts.thesis.list_of_tables = true;
    let t = note_as(&text, "pub", &opts, SetSettings::default()).tex;
    for what in ["\\listoffigures", "\\listoftables", "\\printbibliography"] {
        assert_eq!(t.matches(what).count(), 1, "{what} in\n{t}");
    }
    assert!(t.find("\\printbibliography").unwrap() < t.find("After.").unwrap(), "the marker's place, not the end");
    assert!(t.find("\\listoffigures").unwrap() > t.find("See ").unwrap(), "the marker's place, not behind the contents");
}

// spec: TEX-047
#[test]
fn tex_047_the_boxes_write_the_lists_behind_the_contents_when_there_are_figures_and_tables() {
    let text = format!("# One\n\n{FIG}\n| a |\n|---|\n| 1 |\n: T {{#tbl:t}}\n");
    let mut opts = ExportOptions::for_note();
    opts.thesis.list_of_figures = true;
    opts.thesis.list_of_tables = true;
    let t = note_as(&text, "pub", &opts, SetSettings::default()).tex;
    assert!(t.find("\\tableofcontents").unwrap() < t.find("\\listoffigures").unwrap() && t.find("\\listoffigures").unwrap() < t.find("\\listoftables").unwrap(), "{t}");
    // no figure: no empty list
    let t = note_as("# One\n\nText\n", "pub", &opts, SetSettings::default()).tex;
    assert!(!t.contains("\\listoffigures") && !t.contains("\\listoftables"));
    // off by default
    assert!(!tex(&text).contains("\\listoffigures"));
}

// spec: TEX-045, TEX-047
#[test]
fn tex_045_a_book_writes_the_thesis_syntax_of_all_its_notes_with_the_top_notes_bibliography() {
    let d = sample_as("thesis", &ExportOptions::for_note(), SetSettings::default());
    for want in
        ["\\label{fig:plot}", "\\label{fig:second}", "\\label{tbl:runs}", "\\label{eq:aligned}", "\\label{sec:details}", "\\cref{tbl:runs}", "\\textcite{knuth1984}", "\\printbibliography[heading=bibintoc,title={Literaturverzeichnis}]"]
    {
        assert!(d.tex.contains(want), "{want} in\n{}", d.tex);
    }
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    assert_eq!(d.files.iter().filter(|(n, _)| n.ends_with(".bib")).count(), 1);
}

// spec: TEX-044, TEX-045
#[test]
fn tex_044_the_set_keys_are_checked_and_kept() {
    assert!(SetSettings { citation_style: "apa".into(), table_caption: "below".into(), figure_caption: "above".into(), ..SetSettings::default() }.check().is_ok());
    assert!(SetSettings { citation_style: "chicago".into(), ..SetSettings::default() }.check().unwrap_err().contains("citation_style"));
    assert!(SetSettings { table_caption: "left".into(), ..SetSettings::default() }.check().unwrap_err().contains("table_caption"));
    // left out of the file while empty, kept when set
    assert!(!toml::to_string(&SetSettings::default()).unwrap().contains("caption"));
    let s = SetSettings { figure_caption: "above".into(), ..SetSettings::default() };
    assert_eq!(toml::from_str::<SetSettings>(&toml::to_string(&s).unwrap()).unwrap(), s);
}
