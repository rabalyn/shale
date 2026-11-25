//! The tests of the thesis syntax in the Word export (DOCX-014 to DOCX-017): what Shale hands to pandoc, without
//! pandoc, and the real pandoc on a sample with citations and references, whose Word file is read back.

use super::package::Package;
use super::thesis::{self, prepare, Setup};
use super::*;
use crate::import::docx;

const BIB: &str = "@book{meier2020, author = {Meier, Anna}, title = {Alpha}, date = {2020}, publisher = {P}}\n@book{schulz2021, author = {Schulz, Bo}, title = {Beta}, date = {2021}, publisher = {Q}}\n";

/// A 1 by 1 picture.
const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0x0D, 0x49, 0x48, 0x44, 0x52, 0, 0, 0, 1, 0, 0, 0, 1, 8, 6, 0, 0, 0, 0x1F, 0x15, 0xC4, 0x89, 0, 0, 0, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0, 1, 0, 0, 5, 0, 1, 0x0D,
    0x0A, 0x2D, 0xB4, 0, 0, 0, 0, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
];

fn temp_dir(tag: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-docxthesis-{tag}-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A vault with `refs.bib`, `plot.png` and the note `Thesis.md` of `text`.
fn vault_with(tag: &str, text: &str) -> Vault {
    let dir = temp_dir(tag);
    std::fs::write(dir.join("refs.bib"), BIB).unwrap();
    std::fs::write(dir.join("plot.png"), PNG).unwrap();
    std::fs::write(dir.join("Thesis.md"), text).unwrap();
    Vault::open(&dir).unwrap()
}

fn note(front: &str, body: &str) -> String {
    format!("---\nbibliography: refs.bib\n{front}---\n{body}")
}

fn prepared(tag: &str, text: &str) -> thesis::Prepared {
    prepare(&vault_with(tag, text), Path::new("Thesis.md"), text)
}

fn setup(p: &thesis::Prepared) -> &Setup {
    p.setup.as_ref().expect("the note has thesis syntax")
}

// spec: DOCX-014
#[test]
fn docx_014_a_citation_group_is_a_mark_with_its_keys_modes_prefixes_and_locators_in_the_data() {
    let text = note("", "Als [see @meier2020, p. 3; -@schulz2021] und @schulz2021 [S. 4] zeigen, @meier2020.\n");
    let p = prepared("cites", &text);
    assert_eq!(p.text, format!("{}Als \u{E010}1\u{E011} und \u{E010}2\u{E011} zeigen, \u{E010}3\u{E011}.\n\n# References\n\n\u{E014}\u{E011}\n", &text[..text.find("Als").unwrap()]));
    let lua = &setup(&p).data;
    assert!(lua.contains(r#"{id = "meier2020", mode = "NormalCitation", prefix = "see", suffix = ", p. 3"}, {id = "schulz2021", mode = "SuppressAuthor", prefix = "", suffix = ""}"#), "{lua}");
    assert!(lua.contains(r#"{id = "schulz2021", mode = "AuthorInText", prefix = "", suffix = "S. 4"}"#), "{lua}");
    assert!(lua.contains(r#"{id = "meier2020", mode = "AuthorInText", prefix = "", suffix = ""}"#), "{lua}");
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
}

// spec: DOCX-014
#[test]
fn docx_014_the_bibliography_is_at_the_end_headed_in_the_language_of_the_note_or_at_its_marker() {
    let end = prepared("end", &note("language: de\n", "Siehe [@meier2020].\n"));
    assert!(end.text.ends_with("\n# Literaturverzeichnis\n\n\u{E014}\u{E011}\n"), "{:?}", end.text);
    let english = prepared("end-en", &note("", "Siehe [@meier2020].\n"));
    assert!(english.text.ends_with("\n# References\n\n\u{E014}\u{E011}\n"), "{:?}", english.text);
    let at = prepared("marker", &note("", "Siehe [@meier2020].\n\n<!-- shale: bibliography -->\n\n# Anhang\n"));
    assert!(at.text.contains("\n# References\n\n\u{E014}\u{E011}\n\n# Anhang\n"), "{:?}", at.text);
    assert_eq!(at.text.matches('\u{E014}').count(), 1);
    assert!(!at.text.ends_with("\u{E014}\u{E011}\n"), "not a second time at the end");
}

// spec: DOCX-014
#[test]
fn docx_014_pandoc_gets_citeproc_after_the_filter_with_the_bibliography_files_the_style_and_the_language() {
    let p = prepared("args", &note("language: de\ncitation-style: ieee\n", "Siehe [@meier2020].\n"));
    let s = setup(&p);
    assert_eq!(s.lang, Some(crate::thesis::Lang::De));
    assert_eq!(s.bibliography, vec![("bibliography-1.bib".to_string(), BIB.as_bytes().to_vec())]);
    let csl = String::from_utf8(s.csl.clone().expect("a style")).unwrap();
    assert!(csl.contains("<style") && csl.contains("<citation"), "{csl:.200}");
    let work = Path::new("/work");
    let args: Vec<String> = thesis::args(s, work).iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let at = |name: &str| work.join(name).display().to_string();
    assert_eq!(args, ["-M".to_string(), format!("shale-thesis={}", at("thesis.lua")), "-M".into(), "lang=de-DE".into(), "--citeproc".into(), format!("--bibliography={}", at("bibliography-1.bib")), format!("--csl={}", at("style.csl"))]);
    // the filter comes first in the argument list of pandoc, so that it has made the citations when citeproc runs
    let all: Vec<String> = args_of(&p, Path::new("/work")).iter().map(|a| a.to_string_lossy().into_owned()).collect();
    let (filter, citeproc) = (all.iter().position(|a| a.starts_with("--lua-filter=")).unwrap(), all.iter().position(|a| a == "--citeproc").unwrap());
    assert!(filter < citeproc, "{all:?}");
}

/// The whole argument list of the export of a prepared note.
fn args_of(p: &thesis::Prepared, work: &Path) -> Vec<std::ffi::OsString> {
    let mut a = args(&work.join("note.md"), &work.join("filter.lua"), Path::new("/vault"), &work.join("reference.docx"), 6.0, &work.join("out.docx"), false);
    a.extend(thesis::args(setup(p), work));
    a
}

// spec: DOCX-014
#[test]
fn docx_014_a_csl_file_of_the_vault_is_the_style_when_the_property_names_one() {
    let text = note("citation-style: [[mine.csl]]\n", "Siehe [@meier2020].\n");
    let vault = vault_with("csl", &text);
    std::fs::write(vault.root.join("mine.csl"), "<style>mine</style>").unwrap();
    let mut vault = vault;
    vault.rescan();
    let p = prepare(&vault, Path::new("Thesis.md"), &text);
    assert_eq!(setup(&p).csl.as_deref(), Some("<style>mine</style>".as_bytes()));
    let missing = prepared("csl-missing", &note("citation-style: gone.csl\n", "Siehe [@meier2020].\n"));
    assert!(missing.warnings.iter().any(|w| w.contains("“gone.csl” is not a file of the vault")), "{:?}", missing.warnings);
    assert!(setup(&missing).csl.is_some(), "the default style instead");
}

// spec: DOCX-014
#[test]
fn docx_014_what_is_no_citation_stays_as_written_and_a_note_without_the_syntax_is_not_changed() {
    let text = note("", "Mail ada@host.org, @todo, `[@meier2020]` and e@x.org.\n");
    let p = prepared("plain", &text);
    assert!(p.setup.is_none(), "nothing in it is a citation");
    assert_eq!(p.text, text);
    let none = prepared("none", "# Title\n\nJust text.\n");
    assert_eq!((none.setup, none.text.as_str()), (None, "# Title\n\nJust text.\n"));
}

// spec: DOCX-015
#[test]
fn docx_015_a_labelled_figure_has_its_caption_above_or_below_with_its_number_and_the_label_is_gone() {
    let below = prepared("fig-below", &note("language: de\n", "![[plot.png|Ergebnisse]]{#fig:plot}\n\nSiehe @fig:plot.\n"));
    assert_eq!(below.text, "---\nbibliography: refs.bib\nlanguage: de\n---\n![[plot.png|Ergebnisse]]\n\n\u{E013}1\u{E011}\n\nSiehe \u{E012}2\u{E011}.\n");
    let lua = &setup(&below).data;
    assert!(lua.contains("\"Abbildung 1: Ergebnisse\"") && lua.contains("\"Abbildung 1\""), "{lua}");
    let above = prepared("fig-above", &note("figure-caption: above\n", "![[plot.png|Results]]{#fig:plot}\n"));
    assert!(above.text.ends_with("---\n\u{E013}1\u{E011}\n\n![[plot.png|Results]]\n"), "{:?}", above.text);
    assert!(setup(&above).data.contains("\"Figure 1: Results\""));
}

// spec: DOCX-015
#[test]
fn docx_015_a_labelled_table_has_its_caption_line_as_the_caption_above_or_below_the_table() {
    let table = "| a | b |\n|---|---|\n| 1 | 2 |\n: Runs {#tbl:runs}\n";
    let above = prepared("tbl-above", &note("", &format!("Text\n\n{table}\nMore\n")));
    assert!(above.text.ends_with("Text\n\n\u{E015}1\u{E011}\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n\nMore\n"), "{:?}", above.text);
    assert!(setup(&above).data.contains("\"Table 1: Runs\""));
    let below = prepared("tbl-below", &note("table-caption: below\nlanguage: de\n", &format!("Text\n\n{table}\nMore\n")));
    assert!(below.text.ends_with("| 1 | 2 |\n\n\u{E015}1\u{E011}\n\nMore\n"), "{:?}", below.text);
    assert!(setup(&below).data.contains("\"Tabelle 1: Runs\""));
}

// spec: DOCX-015
#[test]
fn docx_015_a_numbered_equation_ends_in_its_number_and_a_labelled_math_block_becomes_a_display_formula() {
    let p = prepared("eq", &note("", "$$E = mc^2$$ {#eq:energy}\n\n$$\na = b\n$$ {#eq:two}\n\n```math {#eq:three}\nx = y\n```\n\nSee [@eq:energy; @eq:two] and @eq:three.\n"));
    assert!(p.text.contains("$$E = mc^2 \\qquad (1)$$\n"), "{:?}", p.text);
    assert!(p.text.contains("$$\na = b \\qquad (2)\n$$\n"), "{:?}", p.text);
    assert!(p.text.contains("$$\nx = y \\qquad (3)\n$$\n"), "{:?}", p.text);
    assert!(setup(&p).data.contains("\"Equations (1) and (2)\"") && setup(&p).data.contains("\"Equation (3)\""), "{}", setup(&p).data);
}

// spec: DOCX-015
#[test]
fn docx_015_a_labelled_heading_has_its_number_in_front_of_the_title_and_numbers_follow_float_numbering() {
    let p = prepared("sec", &note("", "# One\n\n## Method {#sec:method}\n\nSee @sec:method.\n"));
    assert!(p.text.contains("# One\n\n## 1.1 Method\n"), "{:?}", p.text);
    assert!(setup(&p).data.contains("\"Section 1.1\""));
    let chapter = prepared("chap", &note("float-numbering: chapter\n", "# One\n\n![[plot.png|A]]{#fig:a}\n\n# Two\n\n![[plot.png|B]]{#fig:b}\n\n@fig:b\n"));
    assert!(setup(&chapter).data.contains("\"Figure 2.1: B\"") && setup(&chapter).data.contains("\"Figure 2.1\""), "{}", setup(&chapter).data);
}

// spec: DOCX-016
#[test]
fn docx_016_an_unresolved_key_or_label_a_double_label_and_missing_bibliography_are_warnings() {
    let p = prepared("warn", &note("", "[@meier2020] and [@nobody] and @fig:nowhere.\n\n![[plot.png|A]]{#fig:a}\n\n![[plot.png|B]]{#fig:a}\n"));
    let said = p.warnings.join("\n");
    assert!(said.contains("The key “nobody” is not in the bibliography"), "{said}");
    assert!(said.contains("no label {#fig:nowhere}") || said.contains("There is no label {#fig:nowhere}"), "{said}");
    assert!(said.contains("{#fig:a} is used twice"), "{said}");
    // the group with the unknown key and the reference with no label stay as written, the others are made
    assert!(p.text.contains("[@nobody] and @fig:nowhere."), "{:?}", p.text);
    assert!(p.text.contains("\u{E010}1\u{E011}"));
    let none = prepared("warn-no-bib", "Siehe [@meier2020].\n\n![[plot.png|A]]{#fig:a}\n");
    assert!(none.warnings.iter().any(|w| w.contains("no bibliography is in effect")), "{:?}", none.warnings);
    assert!(none.text.contains("[@meier2020]") && setup(&none).bibliography.is_empty(), "no citation without a bibliography: {:?}", none.text);
}

// spec: DOCX-016
#[test]
fn docx_016_the_warnings_of_the_note_are_in_the_job_and_the_toast_names_them() {
    let text = note("", "[@nobody] and [@meier2020].\n");
    let vault = vault_with("job", &text);
    let job = Job::new(PathBuf::from("pandoc"), &vault, Path::new("Thesis.md"), &text, None, false, true);
    assert_eq!(job.warnings, vec!["The key “nobody” is not in the bibliography".to_string()]);
    assert!(job.thesis.is_some() && job.text.contains('\u{E010}') && !job.text.contains("bibliography: refs.bib"), "{:?}", job.text);
}

// spec: DOCX-017
#[test]
fn docx_017_a_pandoc_without_citeproc_says_so_and_a_missing_pandoc_is_an_error_that_names_it() {
    let old = proc::Run { ending: Ending::Exited(Some(2)), tail: vec!["pandoc: unrecognized option `--citeproc'".to_string(), "Try pandoc --help".to_string()] };
    let said = ended(&old, Duration::from_secs(1)).unwrap_err();
    assert_eq!(said, thesis::OLD_PANDOC);
    assert!(said.contains("--citeproc") && said.contains("pandoc 2.11") && said.contains("Settings → Import"), "{said}");
    let other = proc::Run { ending: Ending::Exited(Some(1)), tail: vec!["Error running filter".to_string()] };
    assert_eq!(ended(&other, Duration::from_secs(1)).unwrap_err(), "pandoc could not make the Word file: Error running filter");
    let dir = temp_dir("gone");
    let text = note("", "Siehe [@meier2020].\n");
    let vault = vault_with("gone-vault", &text);
    let job = Job::new(dir.join("no-pandoc"), &vault, Path::new("Thesis.md"), &text, None, false, true);
    assert!(export(&job, &AtomicBool::new(false)).unwrap_err().starts_with("pandoc could not be started"));
}

// ---------------------------------------------------------------- the real pandoc

fn real_pandoc() -> Option<PathBuf> {
    let found = docx::locate("");
    if found.is_none() {
        eprintln!("skipping: pandoc is not installed here");
    }
    found
}

/// The Word file's text as pandoc reads it back.
fn text_of(pandoc: &Path, file: &Path) -> String {
    let out = std::process::Command::new(pandoc).arg(file).args(["-t", "plain", "--wrap=none"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

const SAMPLE: &str = "# Einleitung\n\nWie [@meier2020, S. 3] und @schulz2021 zeigen (siehe [-@meier2020]), ist es so. Siehe @fig:plot, @tbl:runs, @eq:energy und @sec:method.\n\n![[plot.png|Ergebnisse]]{#fig:plot}\n\n| a | b |\n|---|---|\n| 1 | 2 |\n: Läufe {#tbl:runs}\n\n$$E = mc^2$$ {#eq:energy}\n\n## Methode {#sec:method}\n\nMail ada@host.org und @todo bleiben stehen; [@nobody] und @fig:nowhere auch.\n";

fn run(tag: &str, front: &str, body: &str) -> Option<(thesis::Prepared, String)> {
    let pandoc = real_pandoc()?;
    let text = note(front, body);
    let vault = vault_with(tag, &text);
    let job = Job::new(pandoc.clone(), &vault, Path::new("Thesis.md"), &text, None, false, true);
    let bytes = export(&job, &AtomicBool::new(false)).unwrap_or_else(|e| panic!("{e}\n{}", job.text));
    let out = vault.root.join("out.docx");
    std::fs::write(&out, &bytes).unwrap();
    let prepared = prepare(&vault, Path::new("Thesis.md"), &text);
    let xml = Package::read(&bytes).unwrap().text("word/document.xml").unwrap().to_string();
    let words = xml.split('<').map(|t| t.split_once('>').map_or("", |(_, text)| text)).collect::<Vec<_>>().join(" ");
    Some((prepared, format!("{}\n{words}", text_of(&pandoc, &out))))
}

// spec: DOCX-014
// spec: DOCX-015
// spec: DOCX-016
#[test]
fn docx_014_the_real_pandoc_makes_the_citations_the_bibliography_the_numbers_and_the_references_of_a_german_note() {
    let Some((prepared, plain)) = run("e2e-de", "language: de\ncitation-style: apa\n", SAMPLE) else { return };
    // citations as the style APA writes them, with the locator in German and without the author
    assert!(plain.contains("(Meier, 2020, S. 3)") || plain.contains("Meier, 2020, S. 3"), "{plain}");
    assert!(plain.contains("Schulz (2021)"), "{plain}");
    assert!(plain.contains("(2020)"), "the author left out: {plain}");
    // the bibliography at the end under its German heading, with the entries of the two keys that were cited
    let at = plain.find("Literaturverzeichnis").unwrap_or_else(|| panic!("{plain}"));
    let list = &plain[at..];
    assert!(list.contains("Meier, A.") && list.contains("Alpha") && list.contains("Schulz, B.") && list.contains("Beta"), "{list}");
    // captions and references with the German words and the numbers
    assert!(plain.contains("Abbildung 1: Ergebnisse") && plain.contains("Tabelle 1: Läufe") && plain.contains("mc²") && plain.contains("(1)"), "{plain}");
    assert!(plain.contains("Siehe Abbildung 1, Tabelle 1, Gleichung (1) und Abschnitt 1.1."), "{plain}");
    assert!(plain.contains("1.1 Methode"), "{plain}");
    // what is no citation or reference stays, and the bad ones are as written
    assert!(plain.contains("ada@host.org und @todo bleiben stehen; [@nobody] und @fig:nowhere auch."), "{plain}");
    assert_eq!(prepared.warnings.len(), 2, "{:?}", prepared.warnings);
}

// spec: DOCX-014
// spec: DOCX-015
#[test]
fn docx_014_the_real_pandoc_names_the_references_in_english_with_the_style_numeric_and_the_bibliography_at_its_marker() {
    let body = "Per [@meier2020; @schulz2021] and @fig:a. Also [@fig:a; @fig:b].\n\n![[plot.png|One]]{#fig:a}\n\n![[plot.png|Two]]{#fig:b}\n\n<!-- shale: bibliography -->\n\n# Appendix\n\nEnd.\n";
    let Some((_, plain)) = run("e2e-en", "citation-style: numeric\ntable-caption: below\nfigure-caption: above\n", body) else { return };
    assert!(plain.contains("[1, 2]") || plain.contains("[1,2]") || plain.contains("[1–2]"), "{plain}");
    assert!(plain.contains("Figure 1: One") && plain.contains("Figure 2: Two") && plain.contains("Figures 1 and 2"), "{plain}");
    assert!(plain.contains("and Figure 1."), "{plain}");
    let refs = plain.find("References").unwrap_or_else(|| panic!("{plain}"));
    assert!(plain.find("Appendix").unwrap() > refs, "the list stands at the marker, before the appendix: {plain}");
    let order = (plain.find("Figure 1: One").unwrap(), plain.find("Figure 2: Two").unwrap());
    assert!(order.0 < order.1);
}

// spec: DOCX-014
#[test]
fn docx_014_the_real_pandoc_leaves_a_note_without_the_syntax_as_it_was() {
    let Some((prepared, plain)) = run("e2e-plain", "", "Just a text with ada@host.org.\n") else { return };
    assert!(prepared.setup.is_none() && plain.contains("Just a text with ada@host.org."), "{plain}");
    assert!(!plain.contains("References"), "{plain}");
}
