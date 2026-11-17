//! The Markdown to LaTeX writer (TEX-008 to TEX-015): the text it writes for every construct and
//! every kind of the shipped tuda-ci manifests, escaping, headings by kind, the table of contents,
//! slides, letters, tables, colours, embeds and the fields. Against LuaLaTeX itself:
//! `writer_env_tests.rs`.

use super::import::TUDA;
use super::sets::*;
use super::writer::*;
use crate::pdf::export::ExportOptions;
use crate::vault::Vault;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub(super) const RICH_NOTE: &str = include_str!("../../testdata/tex/rich-note.md");

/// A vault in a temporary folder with these files.
pub(super) fn vault_in(dir: &Path, files: &[(&str, &[u8])]) -> Vault {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    for (name, bytes) in files {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
    Vault::open(dir).unwrap()
}

pub(super) fn temp_vault(tag: &str, files: &[(&str, &[u8])]) -> (Vault, PathBuf) {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!("shale-writer-{tag}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
    (vault_in(&dir, files), dir)
}

/// The files the rich note embeds: a picture, a drawing and a PDF.
pub(super) fn rich_assets() -> Vec<(&'static str, Vec<u8>)> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(80, 40, image::Rgba([30, 120, 200, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    vec![("pic.png", png.into_inner()), ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE.as_bytes().to_vec()), ("doc.pdf", crate::pdf::view::sample_pdf())]
}

/// The set of the shipped tuda-ci kinds, as an imported one would be read (no folder on disk).
pub(super) fn tuda_set(settings: SetSettings) -> SetInfo {
    let kinds = TUDA.kinds.iter().map(|(id, text)| (id.to_string(), KindManifest::parse(text).unwrap())).collect();
    let meta = SetMeta { name: "TU Darmstadt".into(), version: Some("4.06".into()), imported_at: "2026-11-17".into(), source: Source::Local { path: "x".into() }, logo_file: Some("tuda_logo".into()) };
    SetInfo { id: "tuda".into(), dir: PathBuf::from("/nowhere/tuda"), meta, settings, kinds, problems: vec![], logo: LogoStatus::Missing }
}

pub(super) fn kind<'a>(set: &'a SetInfo, id: &str) -> &'a KindManifest {
    &set.kinds.iter().find(|(i, _)| i == id).unwrap_or_else(|| panic!("no kind {id}")).1
}

pub(super) fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// Write a note as a kind of the tuda set; the note is `Plan.md` of a vault that has `files` too.
fn doc_with(text: &str, kind_id: &str, opts: &ExportOptions, given: &[(&str, &str)], files: &[(&str, &[u8])]) -> Result<Document, String> {
    let (v, dir) = temp_vault("doc", &[&[("Plan.md", text.as_bytes())][..], files].concat());
    let set = tuda_set(SetSettings { accent: "9c".into(), ..SetSettings::default() });
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text, opts, set: &set, kind: kind(&set, kind_id), fields: &fields(given) });
    let _ = std::fs::remove_dir_all(dir);
    out
}

pub(super) fn doc(text: &str, kind_id: &str) -> String {
    doc_with(text, kind_id, &ExportOptions::for_note(), &[("author", "Ada")], &[]).unwrap_or_else(|e| panic!("{e}")).tex
}

fn body_of(tex: &str) -> &str {
    tex.split_once("\\begin{document}\n").unwrap().1.trim_end_matches("\\end{document}\n")
}

// spec: TEX-008
#[test]
fn text_is_escaped_everywhere_so_hostile_input_stays_text() {
    let nasty = "\\input{/etc/passwd} \\write18{rm -rf} ^^5c \\catcode`\\{=12 } { \\end{document} \"a $x$ %c # & _ ~ < > |";
    let note = format!("---\ntitle: T\nauthor: {nasty}\n---\n# Head {nasty}\n\n{nasty}\n\n- item {nasty}\n- [ ] task {nasty}\n\n> quote {nasty}\n\n| a {nasty} | b |\n| --- | --- |\n| {nasty} | [[x|{nasty}]] |\n\n![[missing {nasty}.png|{nasty}]]\n\n[l {nasty}](https://e.com/{nasty})\n\n```\ncode {nasty}\n```\n");
    let tex = doc_with(&note, "pub", &ExportOptions::for_note(), &[], &[]).unwrap().tex;
    assert!(tex.matches("\\end{document}").count() == 1 && tex.trim_end().ends_with("\\end{document}"), "only the last line ends the document");
    assert!(!tex.contains("\\input{/etc") && !tex.contains("\\write18") && !tex.contains("^^5c") && !tex.contains("\\catcode"), "{tex}");
    assert!(tex.contains("\\textbackslash{}input\\{/etc/passwd\\}") && tex.contains("\\textasciicircum{}\\textasciicircum{}5c"), "literal text: {tex}");
    // the front matter field: the author is text too
    assert!(tex.contains("\\author{\\textbackslash{}input\\{/etc/passwd\\}"), "{tex}");
}

// spec: TEX-008
#[test]
fn inline_markup_is_written_as_commands_and_links_as_href() {
    let t = doc("a *it* **bo** ~~st~~ `co de` ==hi== <mark style=\"background: #FFFF00;\">m</mark> <span style=\"color: #FF0000;\">r</span> [w](https://e.com/a_b?x=1&y=2#f) [[Wiki|shown]] [[Plain]] #tag\n", "pub");
    let b = body_of(&t);
    for want in [
        "\\textit{it}",
        "\\textbf{bo}",
        "\\sout{st}",
        "\\texttt{co\\ de}",
        "\\colorbox[RGB]{255,241,168}{hi}",
        "\\colorbox[RGB]{255,255,0}{m}",
        "\\textcolor[RGB]{255,0,0}{r}",
        "\\href{https://e.com/a\\_b?x=1\\&y=2\\#f}{w}",
        "shown",
        "Plain",
        "\\#tag",
    ] {
        assert!(b.contains(want), "{want} is in {b}");
    }
    assert!(!b.contains("[[") && !b.contains("<mark") && !b.contains("<span"), "no markup is left: {b}");
    assert!(!t.contains("\\href{Plain}") && !t.contains("\\href{Wiki"), "a wiki link is its text, not a dead link");
}

// spec: TEX-008
#[test]
fn an_address_in_a_link_cannot_end_the_command_it_is_in() {
    let t = doc("[x](https://e.com/a%20b{c}d\"e|f~g^h$i) and [w](www.example.org)\n", "pub");
    assert!(t.contains("\\href{https://e.com/a\\%20b\\%7Bc\\%7Dd\\%22e\\%7Cf\\~g\\^h\\$i}{x}"), "{t}");
    assert!(t.contains("\\href{https://www.example.org}{w}"), "{t}");
}

// spec: TEX-008
#[test]
fn blocks_are_written_as_environments() {
    let t = doc("## S\nfirst\nsecond line\n\n> q1\n> q2\n>> nested\n\n- a\n- b\n  - c\n    - d\n      - e\n        - f\n1. one\n\n7. seven\n8. eight\n\n- [ ] open\n- [x] done\n\n---\n\n```\nlet a = 1 & 2;\n\tindented\n```\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("first \\newline\nsecond line\n\n"), "a line break of the note is a line break: {b}");
    assert!(b.contains("\\begin{quote}\nq1 \\newline\nq2\n\\begin{quote}\nnested\n\\end{quote}\n\n\\end{quote}"), "{b}");
    assert!(
        b.contains("\\begin{itemize}\n\\item a\n\\item b\n\\begin{itemize}\n\\item c\n\\begin{itemize}\n\\item d\n\\begin{itemize}\n\\item e\n\\item f\n\\end{itemize}\n\\end{itemize}\n\\end{itemize}\n\\end{itemize}\n"),
        "four levels, a fifth stays on the fourth: {b}"
    );
    assert!(b.contains("\\begin{enumerate}\n\\item one\n\\end{enumerate}") && b.contains("\\begin{enumerate}\n\\setcounter{enumi}{6}\n\\item seven\n\\item eight\n\\end{enumerate}"), "{b}");
    assert!(b.contains("\\item[\\shaletask{}] open") && b.contains("\\item[\\shaletask{×}] done"), "{b}");
    assert!(b.contains("\\hrulefill"), "{b}");
    assert!(b.contains("\\begin{shalecode}\n\\mbox{}let\\ a\\ =\\ 1\\ \\&\\ 2;\\par\n\\mbox{}\\ \\ \\ \\ indented\\par\n\\end{shalecode}"), "code keeps its spaces, a tab is four: {b}");
}

// spec: TEX-008
#[test]
fn a_line_of_code_can_never_end_its_block_and_an_item_cannot_become_an_option() {
    let t = doc("## S\n```\n\\end{shalecode}\\input{x}\n```\n\n- [x] check\n- [not an option]\n- *star\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("\\textbackslash{}end\\{shalecode\\}\\textbackslash{}input\\{x\\}\\par\n\\end{shalecode}"), "{b}");
    assert!(b.contains("\\item {}[not an option]"), "{b}");
}

// spec: TEX-008, TEX-015
#[test]
fn fenced_code_has_the_colours_of_its_kinds_and_stays_plain_without_a_language() {
    let t = doc("## S\n```rust\nfn main() {}\n```\n\n```\nfn main() {}\n```\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("\\textcolor[RGB]{125,45,170}{fn}"), "a keyword of Rust is purple: {b}");
    assert_eq!(b.matches("\\begin{shalecode}").count(), 2);
    assert_eq!(b.matches("\\textcolor").count(), b.split("\\begin{shalecode}").nth(1).unwrap().matches("\\textcolor").count(), "the block without a language has none: {b}");
}

// spec: TEX-008, TEX-015
#[test]
fn a_table_has_booktabs_rules_a_bold_header_and_the_alignment_of_its_columns() {
    let t = doc("## S\n| Name | Value | Note |\n| :--- | :---: | ---: |\n| a | 1 | b & c |\n| [[Plain]] | **2** | 50% |\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("\\begin{xltabular}{1.000\\linewidth}{>{\\raggedright\\arraybackslash}Xcr}"), "the widest column takes the room that is left: {b}");
    assert!(b.contains("\\toprule\n\\textbf{Name} & \\textbf{Value} & \\textbf{Note} \\\\\n\\midrule\n\\endhead\n"), "{b}");
    assert!(b.contains("a & 1 & b \\& c \\\\\n") && b.contains("Plain & \\textbf{2} & 50\\% \\\\\n") && b.contains("\\bottomrule\n\\end{xltabular}"), "{b}");
    let slides = doc("## S\n| A | B |\n| --- | --- |\n| 1 | 2 |\n", "beamer");
    assert!(body_of(&slides).contains("\\begin{tabularx}") && !slides.contains("\\begin{xltabular}") && !slides.contains("\\usepackage{xltabular}"), "a slide has no table that breaks across pages");
}

// spec: TEX-015
#[test]
fn stored_column_sizes_become_the_proportions_of_the_columns() {
    let note = "## S\n<!-- shale-table cols=100,300 -->\n| A | B |\n| --- | --- |\n| 1 | 2 |\n";
    let t = doc(note, "pub");
    let b = body_of(&t);
    // 400 of the editor's 720 points: a bit over half the text width, split 1 : 3
    assert!(b.contains("\\begin{xltabular}{0.556\\linewidth}{>{\\hsize=0.5000\\hsize\\raggedright\\arraybackslash}X>{\\hsize=1.5000\\hsize\\raggedright\\arraybackslash}X}"), "{b}");
    assert!(!b.contains("shale-table"), "the comment line is not text");
}

// spec: TEX-008
#[test]
fn the_front_matter_is_not_printed_and_the_properties_fill_the_fields() {
    let note = "---\ntitle: The Real Title\nauthor: [Ada, Bob]\nsubtitle: Sub & more\ndate: 2026-11-17\nsecret: printed?\n---\nBody text\n";
    let t = doc_with(note, "pub", &ExportOptions::for_note(), &[], &[]).unwrap().tex;
    assert!(!t.contains("secret") && !t.contains("printed?"), "an unknown property is not printed: {t}");
    assert!(body_of(&t).trim() == "\\maketitle\nBody text", "{:?}", body_of(&t));
    assert!(t.contains("\\title{The Real Title}\n") && t.contains("\\author{Ada \\and Bob}\n") && t.contains("\\subtitle{Sub \\& more}\n") && t.contains("\\date{2026-11-17}\n"), "{t}");
    // the dialog's value beats the property, an empty one does not
    let t = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "Carol"), ("subtitle", "  ")], &[]).unwrap().tex;
    assert!(t.contains("\\author{Carol}\n") && t.contains("\\subtitle{Sub \\& more}\n"), "{t}");
    // nothing given: the note's name is the title, a field of the manifest has its default, no \date
    let t = doc_with("# Plan\ntext\n", "thesis", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex;
    assert!(t.contains("\\title{Plan}\n") && t.contains("\\submissiondate{\\today}\n") && !t.contains("\\subtitle{"), "{t}");
    // only the separators survive in a value: \and for authors, \\ in an address
    let t = doc_with("text", "pub", &ExportOptions::for_note(), &[("author", "A \\and B \\input{x}")], &[]).unwrap().tex;
    assert!(t.contains("\\author{A \\and B \\textbackslash{}input\\{x\\}}"), "{t}");
}

// spec: TEX-008
#[test]
fn a_required_field_with_no_value_stops_the_export_and_names_the_field() {
    let e = doc_with("# Plan\ntext\n", "pub", &ExportOptions::for_note(), &[], &[]).unwrap_err();
    assert_eq!(e, "“Author” is empty: fill it in the export dialog or add `author:` to the note's properties");
    let e = doc_with("text", "letter", &ExportOptions::for_note(), &[], &[]).unwrap_err();
    assert!(e.starts_with("“Recipient (lines separated by \\\\)” is empty"), "{e}");
    assert!(doc_with("---\nauthor: X\n---\ntext", "pub", &ExportOptions::for_note(), &[], &[]).is_ok(), "a property is enough");
}

// spec: TEX-009
#[test]
fn headings_of_an_article_kind_are_levels_by_nesting_and_the_title_heading_is_not_repeated() {
    let note = "# Plan\nintro\n## A\n#### Deep under A\n## B\n### C\n###### D\n# Other\n";
    let t = doc(note, "pub");
    let b = body_of(&t);
    // "# Plan" says the note's title again: gone, so ## A is the first level
    let heads: Vec<&str> = b.lines().filter(|l| l.starts_with("\\section") || l.starts_with("\\sub") || l.starts_with("\\paragraph")).collect();
    assert_eq!(heads, ["\\section{A}", "\\subsection{Deep under A}", "\\section{B}", "\\subsection{C}", "\\subsubsection{D}", "\\section{Other}"], "no level is skipped: {b}");
    assert!(!b.contains("{Plan}"));
    // a first heading that is not the title is a heading
    let t = doc("# Other\n## A\n", "pub");
    assert!(body_of(&t).contains("\\section{Other}\n\n\\subsection{A}"), "{t}");
    // the date prefix of the name is not part of the title (VAULT-025), the comparison ignores case
    let (v, d) = temp_vault("prefix", &[("261117_Plan.md", b"# PLAN\n## A\n")]);
    let set = tuda_set(SetSettings::default());
    let out = write(&Request { vault: &v, rel: Path::new("261117_Plan.md"), text: "# PLAN\n## A\n", opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "A")]) }).unwrap();
    assert!(out.tex.contains("\\title{Plan}") && body_of(&out.tex).contains("\\section{A}") && !body_of(&out.tex).contains("PLAN"), "{}", out.tex);
    let _ = std::fs::remove_dir_all(d);
    // deeper than the commands go: the last ones repeat
    let t = doc("# A\n## B\n### C\n#### D\n##### E\n###### F\n", "pub");
    assert!(body_of(&t).contains("\\subsubsection{C}\n\n\\paragraph{D}\n\n\\subparagraph{E}\n\n\\subparagraph{F}"), "{t}");
}

// spec: TEX-009, TEX-011
#[test]
fn a_book_kind_starts_with_chapters() {
    let t = doc("# Plan\n# One\n## Two\n### Three\n#### Four\n", "thesis");
    let b = body_of(&t);
    assert!(b.contains("\\chapter{One}\n\n\\section{Two}\n\n\\subsection{Three}\n\n\\subsubsection{Four}\n"), "{b}");
    assert!(doc("# One\n", "phd").contains("\\chapter{One}"), "the doctoral thesis too");
}

// spec: TEX-009
#[test]
fn numbered_headings_follow_the_note_and_the_setting_and_unnumbered_ones_stay_in_the_contents() {
    let note = "# A\n## B\n";
    let on = ExportOptions { numbered_headings: true, ..ExportOptions::for_note() };
    let off = ExportOptions { numbered_headings: false, ..ExportOptions::for_note() };
    let go = |text: &str, o: &ExportOptions| doc_with(text, "pub", o, &[("author", "A")], &[]).unwrap().tex;
    assert!(body_of(&go(note, &on)).contains("\\section{A}\n\n\\subsection{B}"));
    let t = go(note, &off);
    assert!(body_of(&t).contains("\\section*{A}\n\\addcontentsline{toc}{section}{A}\n\n\\subsection*{B}\n\\addcontentsline{toc}{subsection}{B}\n"), "{t}");
    // the property of the note beats the setting
    assert!(body_of(&go("---\nnumbered-headings: false\n---\n# A\n", &on)).contains("\\section*{A}"));
    assert!(body_of(&go("---\nnumbered-headings: true\n---\n# A\n", &off)).contains("\\section{A}"));
    // without a table of contents there is nothing to add the heading to
    let no_toc = ExportOptions { numbered_headings: false, toc: false, ..ExportOptions::for_note() };
    assert!(!go(note, &no_toc).contains("addcontentsline"));
}

// spec: TEX-009
#[test]
fn the_table_of_contents_is_there_when_asked_for_and_the_kind_has_one() {
    let note = "# A\ntext\n## B\n";
    let tex = |kind_id: &str, toc: bool| doc_with(note, kind_id, &ExportOptions { toc, ..ExportOptions::for_note() }, &[("author", "A"), ("recipient", "R")], &[]).unwrap().tex;
    let on = tex("pub", true);
    assert!(body_of(&on).starts_with("\\maketitle\n\\tableofcontents\n") && on.contains("\\setcounter{tocdepth}{3}\n"), "{on}");
    assert!(tex("thesis", true).contains("\\setcounter{tocdepth}{2}\n"), "chapter, section, subsection");
    assert!(!tex("pub", false).contains("tableofcontents") && !tex("pub", false).contains("tocdepth"));
    for k in ["beamer", "letter", "poster", "sciposter", "leaflet", "announcement"] {
        assert!(!tex(k, true).contains("tableofcontents"), "{k} has no contents");
    }
    assert!(!doc_with("no headings\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex.contains("tableofcontents"), "an empty list is not printed");
    assert!(on.contains("\\hypersetup{bookmarksdepth=5}"), "the bookmarks of the PDF reach every level");
}

// spec: TEX-012
#[test]
fn slides_have_a_section_for_h1_and_a_frame_for_h2() {
    let note = "# Plan\n# Part one\nIntro of part one\n## Slide A\ntext a\n### Detail\nmore\n## Slide B\n- x\n# Part two\n## Slide C\n";
    let t = doc(note, "beamer");
    let b = body_of(&t);
    let frames = b.matches("\\begin{frame}").count();
    assert_eq!(frames, 4, "the intro of Part one, A, B and C: {b}");
    assert!(b.starts_with("\\maketitle\n\\section{Part one}\n\n\\begin{frame}[allowframebreaks]{Part one}\nIntro of part one\n\n\\end{frame}\n\n\\begin{frame}[allowframebreaks]{Slide A}\ntext a\n\n\\par\\medskip\\noindent\\textbf{Detail}\\par\\smallskip\n\nmore\n\n\\end{frame}\n"), "{b}");
    assert!(b.contains("\\section{Part two}\n\n\\begin{frame}[allowframebreaks]{Slide C}\n\\end{frame}"), "a section with no text of its own has no frame of its own: {b}");
    assert_eq!(b.matches("\\end{frame}").count(), frames);
    assert!(!b.contains("{Plan}"), "the title is the title slide");
}

// spec: TEX-012
#[test]
fn slides_of_a_note_with_only_h1_have_a_frame_for_each_and_text_before_it_gets_one() {
    let t = doc("Before any heading\n\n# One\nfirst\n# Two\nsecond\n### Small\nx\n", "beamer");
    let b = body_of(&t);
    assert!(!b.contains("\\section"), "no sections without ##: {b}");
    assert!(b.contains("\\maketitle\n\\begin{frame}[allowframebreaks]{}\nBefore any heading\n\n\\end{frame}\n\n\\begin{frame}[allowframebreaks]{One}\nfirst\n\n\\end{frame}\n\n\\begin{frame}[allowframebreaks]{Two}\nsecond\n\n\\par\\medskip\\noindent\\textbf{Small}"), "{b}");
    assert_eq!(b.matches("\\begin{frame}").count(), 3);
}

// spec: TEX-013
#[test]
fn a_letter_has_its_recipient_subject_and_opening_from_the_properties_and_bold_lines_for_headings() {
    let note = "---\nrecipient: |\n  Dr. Hopper\n  Navy Street 1\nsubject: Re: Compilers\nsender: Ada\nopening: Dear Grace,\nclosing: Yours\n---\n# A heading\nDear text & more\n";
    let t = doc_with(note, "letter", &ExportOptions::for_note(), &[], &[]).unwrap().tex;
    assert!(t.contains("\\begin{document}\n\\begin{letter}{Dr. Hopper \\\\ Navy Street 1}\n\\setkomavar{subject}{Re: Compilers}\n\\setkomavar{fromname}{Ada}\n\\opening{Dear Grace,}\n"), "{t}");
    assert!(t.contains("\\textbf{A heading}\\par\\smallskip\n\nDear text \\& more\n\n\\closing{Yours}\n\\end{letter}\n\\end{document}"), "{t}");
    assert!(!t.contains("\\maketitle") && !t.contains("\\setkomavar{date}") && !t.contains("\\setkomavar{fromaddress}"), "what has no value is not set: {t}");
    // the manifest's defaults, and the note's name as the subject
    let t = doc_with("text", "letter", &ExportOptions::for_note(), &[("recipient", "Someone")], &[]).unwrap().tex;
    assert!(t.contains("\\setkomavar{subject}{Plan}") && t.contains("\\opening{Dear Sir or Madam,}") && t.contains("\\closing{Kind regards}"), "{t}");
}

// spec: TEX-014
#[test]
fn posters_leaflets_and_announcements_are_text_with_headings_without_numbers() {
    let note = "# Plan\n## A\ntext\n### B\n";
    for k in ["poster", "leaflet", "announcement"] {
        let t = doc_with(note, k, &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex;
        let b = body_of(&t);
        assert!(b.contains("\\section*{A}\n\ntext") && b.contains("\\subsection*{B}") && !b.contains("addcontentsline") && !b.contains("tableofcontents"), "{k}: {b}");
        assert_eq!(b.contains("\\begin{multicols}{2}"), k == "poster", "{k}: only the poster has columns");
    }
}

// spec: TEX-014
#[test]
fn a_scientific_poster_has_a_box_for_each_top_level_heading_in_two_columns() {
    let note = "Intro text\n# Plan\n## Goal\ntext\n### Detail\nx\n## Method\ny\n## Result\nz\n";
    let t = doc_with(note, "sciposter", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex;
    let b = body_of(&t);
    // "# Plan" is the title; the top level is ## now
    assert!(
        b.starts_with("\\maketitle\n\\begin{tcbposter}[poster={columns=2,rows=2,spacing=1cm}]\n\\begin{posterboxenv}[]{name=box1,column=1,row=1}\nIntro text\n\n\\end{posterboxenv}"),
        "text before the first heading is a box with no title: {b}"
    );
    assert!(b.contains("\\begin{posterboxenv}[title={Goal}]{name=box2,column=2,row=1}\ntext\n\n\\par\\medskip\\noindent\\textbf{Detail}\\par\\smallskip\n\nx\n\n\\end{posterboxenv}"), "{b}");
    assert!(b.contains("[title={Method}]{name=box3,column=1,row=2}") && b.contains("[title={Result}]{name=box4,column=2,row=2}") && b.trim_end().ends_with("\\end{tcbposter}"), "{b}");
    let one = doc_with("only text\n", "sciposter", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex;
    assert!(one.contains("columns=1,rows=1,spacing=1cm") && one.contains("name=box1,column=1,row=1"), "{one}");
}

// spec: TEX-015
#[test]
fn a_picture_a_drawing_and_a_pdf_are_files_in_the_work_folder_sized_like_the_editor_with_a_caption() {
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let note = "## S\n![[pic.png|200]]\n![[Sketch.excalidraw]]\n![[doc.pdf|A document]]\n![[pic.png]]\n";
    let out = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], &files).unwrap();
    let b = body_of(&out.tex);
    // 200 points of the editor's 720
    assert!(b.contains("\\includegraphics[width=0.278\\linewidth,height=0.8\\textheight,keepaspectratio]{picture-1.png}"), "{b}");
    assert!(b.contains("{\\footnotesize\\textit{pic.png}}"), "the caption is the link's text: {b}");
    assert!(b.contains("{drawing-2.pdf}") && b.contains("\\textit{Sketch.excalidraw}"), "{b}");
    // all three pages of the PDF, the caption under the last, a file used twice is one file
    for n in 1..=3 {
        assert!(b.contains(&format!("\\includegraphics[page={n},")), "page {n}: {b}");
    }
    assert!(b.contains("\\textit{A document}") && b.matches("document-3.pdf").count() == 3, "{b}");
    assert_eq!(b.matches("picture-1.png").count(), 2, "{b}");
    let names: Vec<&str> = out.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["picture-1.png", "drawing-2.pdf", "document-3.pdf"]);
    assert!(out.files[0].1.starts_with(&[0x89, b'P', b'N', b'G']) && out.files[1].1.starts_with(b"%PDF") && out.files[2].1.starts_with(b"%PDF"));
    assert!(out.warnings.is_empty());
    // a picture of the size the editor gives it when the link asks for nothing: 80 points of 720
    let small = doc_with("## S\n![[pic.png]]\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &files).unwrap().tex;
    assert!(small.contains("width=0.111\\linewidth"), "{small}");
    // the editor's column is what the share is taken of (PDF-005)
    let narrow = doc_with("## S\n![[pic.png|200]]\n", "pub", &ExportOptions { column: Some(400.0), ..ExportOptions::for_note() }, &[("author", "A")], &files).unwrap().tex;
    assert!(narrow.contains("width=0.500\\linewidth"), "{narrow}");
}

// spec: TEX-015
#[test]
fn an_embed_that_cannot_be_used_is_a_line_of_text_and_a_warning_not_a_failure() {
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let note = "## S\n![[missing.png]]\n![[broken.png]]\n![[nothing.mp3]]\ninline ![[pic.png]] stays text\n";
    let out = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], &[("broken.png", b"not a png"), ("nothing.mp3", b"x"), files[0]]).unwrap();
    let b = body_of(&out.tex);
    assert!(b.contains("\\textit{[missing: missing.png]}") && b.contains("\\textit{[not readable: broken.png]}") && b.contains("\\textit{[nothing.mp3: not in a PDF]}"), "{b}");
    assert!(b.contains("inline pic.png stays text"), "an embed inside a paragraph is its text: {b}");
    assert_eq!(out.warnings, ["missing: missing.png", "not readable: broken.png", "nothing.mp3: not in a PDF"]);
    assert!(out.files.is_empty());
    let off = doc_with("## S\n![[pic.png]]\n", "pub", &ExportOptions { embeds: false, ..ExportOptions::for_note() }, &[("author", "A")], &[files[0]]).unwrap();
    assert!(!off.tex.contains("includegraphics{") && off.files.is_empty());
}

// spec: TEX-015
#[test]
fn the_colours_of_a_note_are_exact_rgb_values() {
    let t = doc("<mark style=\"background: #1A2B3C;\">x</mark> <span style=\"color: #FF8000;\">y</span> <mark style=\"background: #00FF00;\"><span style=\"color: #0000FF;\">z</span></mark>\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("\\colorbox[RGB]{26,43,60}{x}") && b.contains("\\textcolor[RGB]{255,128,0}{y}") && b.contains("\\colorbox[RGB]{0,255,0}{\\textcolor[RGB]{0,0,255}{z}}"), "{b}");
}

// spec: TEX-008
#[test]
fn the_document_has_the_kinds_lines_the_helpers_and_one_main_file() {
    let t = doc("text\n", "thesis");
    assert!(t.starts_with("\\RequirePackage{pdfmanagement}\n\\SetKeys[document/metadata]{pdfstandard=a-2b,pdfversion=1.7,lang=en}\n\\documentclass[english,accentcolor=9c,ruledheaders=section,class=report,"), "{t}");
    let class = t.find("\\documentclass").unwrap();
    for later in ["\\usepackage[english]{babel}", "\\usepackage{graphicx}", "\\usepackage[normalem]{ulem}", "\\usepackage{xltabular}", "\\newenvironment{shalecode}", "\\title{Plan}", "\\author{Ada}", "\\begin{document}"] {
        assert!(t.find(later).is_some_and(|p| p > class), "{later} comes after the class: {t}");
    }
    assert!(t.trim_end().ends_with("\\end{document}") && t.matches("\\begin{document}").count() == 1);
    // the language of the set
    let (v, d) = temp_vault("lang", &[]);
    let set = tuda_set(SetSettings { language: Language::De, ..SetSettings::default() });
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: "x", opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "A")]) }).unwrap();
    assert!(out.tex.contains("lang=de}") && out.tex.contains("\\documentclass[ngerman]{tudapub}") && out.tex.contains("\\usepackage[ngerman]{babel}"), "{}", out.tex);
    let _ = std::fs::remove_dir_all(d);
}

// spec: TEX-013
#[test]
fn the_dialog_is_prefilled_from_the_properties_and_the_title_is_the_notes_name() {
    let set = tuda_set(SetSettings::default());
    let props = crate::markdown::front_matter_values("---\nauthor: [Ada, Bob]\nreviewer: Prof. X\n---\nbody");
    let got = prefill("Plan", &props, kind(&set, "thesis"));
    let get = |k: &str| got.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
    assert_eq!((get("title"), get("author"), get("reviewer"), get("subtitle")), (Some("Plan"), Some("Ada \\and Bob"), Some("Prof. X"), Some("")));
    let props = crate::markdown::front_matter_values("---\nrecipient: |\n  A\n  B\n---\n");
    let letter = prefill("Plan", &props, kind(&set, "letter"));
    assert_eq!(letter.iter().find(|(k, _)| k == "recipient").map(|(_, v)| v.as_str()), Some("A \\\\ B"));
}

// spec: TEX-008
#[test]
fn every_kind_of_the_tuda_set_writes_the_rich_note() {
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let set = tuda_set(SetSettings::default());
    for (id, k) in &set.kinds {
        let (v, d) = temp_vault("rich", &[&[("Plan.md", RICH_NOTE.as_bytes())][..], &files].concat());
        let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: RICH_NOTE, opts: &ExportOptions::for_note(), set: &set, kind: k, fields: &fields(&[("author", "Ada"), ("recipient", "Dr. Hopper \\\\ Navy Street 1")]) })
            .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(out.tex.contains("\\begin{document}") && out.tex.trim_end().ends_with("\\end{document}") && out.warnings == ["missing: missing.png"], "{id}: {:?}", out.warnings);
        assert!(!out.tex.contains("secret") && !out.tex.contains("numbered-headings"), "{id}: the front matter is not printed");
        let _ = std::fs::remove_dir_all(d);
    }
}

// spec: TEX-008
#[test]
fn blocks_are_found_line_by_line_by_the_rules_of_the_reading_view() {
    use super::blocks::{parse, Block, Item, QuoteLine};
    let blocks = parse("---\nfront: matter\n---\n# H\npara one\npara two\n\n\t- tabbed\n    - four spaces\n> > deep\n>shallow\n***\n``` js\ncode\n```\n![[a.png]]\ntext ![[b.png]] inline\n");
    assert!(matches!(&blocks[0], Block::Heading { level: 1, text, line: 3 } if text == "H"), "{blocks:?}");
    assert_eq!(blocks[1], Block::Para(vec!["para one".into(), "para two".into()]));
    let Block::List(items) = &blocks[2] else { panic!("{blocks:?}") };
    assert_eq!(items.iter().map(|i| (i.indent, i.item.clone(), i.text.as_str())).collect::<Vec<_>>(), [(4, Item::Bullet, "tabbed"), (4, Item::Bullet, "four spaces")], "a tab is four columns");
    assert_eq!(blocks[3], Block::Quote(vec![QuoteLine { depth: 2, text: "deep".into() }, QuoteLine { depth: 1, text: "shallow".into() }]));
    assert_eq!(blocks[4], Block::Rule);
    assert_eq!(blocks[5], Block::Code { lang: "js".into(), lines: vec!["code".into()] });
    assert!(matches!(&blocks[6], Block::Embed(l) if l.target == "a.png") && matches!(&blocks[7], Block::Para(p) if p == &vec!["text ![[b.png]] inline".to_string()]), "{blocks:?}");
}

// spec: TEX-012
#[test]
fn a_heading_that_is_smaller_than_a_slide_opens_a_slide_and_a_deeper_one_before_a_box_is_text() {
    let t = doc("### Detail first\ntext\n", "beamer");
    assert!(body_of(&t).starts_with("\\maketitle\n\\begin{frame}[allowframebreaks]{}\n\\par\\medskip\\noindent\\textbf{Detail first}"), "{t}");
    let t = doc_with("## Deeper\nfirst\n# Top\nsecond\n", "sciposter", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex;
    assert!(body_of(&t).contains("[]{name=box1,column=1,row=1}\n\\par\\medskip\\noindent\\textbf{Deeper}") && body_of(&t).contains("[title={Top}]{name=box2,column=2,row=1}"), "{t}");
}

// spec: TEX-015
#[test]
fn other_picture_formats_become_png_and_a_broken_drawing_pdf_or_empty_drawing_is_a_warning() {
    let mut bmp = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(10, 10, image::Rgba([1, 2, 3, 255])).write_to(&mut bmp, image::ImageFormat::Bmp).unwrap();
    let mut jpg = std::io::Cursor::new(Vec::new());
    image::RgbImage::from_pixel(10, 10, image::Rgb([1, 2, 3])).write_to(&mut jpg, image::ImageFormat::Jpeg).unwrap();
    let files: Vec<(&str, &[u8])> =
        vec![("a.bmp", bmp.get_ref()), ("b.jpg", jpg.get_ref()), ("broken.pdf", b"not a pdf"), ("broken.excalidraw", b"{"), ("empty.excalidraw", b"{\"type\":\"excalidraw\",\"version\":2,\"elements\":[],\"appState\":{},\"files\":{}}")];
    let out = doc_with("## S\n![[a.bmp]]\n![[b.jpg]]\n![[broken.pdf]]\n![[broken.excalidraw]]\n![[empty.excalidraw]]\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &files).unwrap();
    let names: Vec<&str> = out.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["picture-1.png", "picture-2.jpg"], "a BMP is converted, a JPEG kept");
    assert!(out.files[0].1.starts_with(&[0x89, b'P', b'N', b'G']) && out.files[1].1.starts_with(&[0xFF, 0xD8]));
    assert_eq!(out.warnings, ["not readable: broken.pdf", "not readable: broken.excalidraw", "empty drawing: empty.excalidraw"]);
}

const NEW_FORMATS: &str = "Water H<sub>2</sub>O, x<sup>2</sup>, <u>under</u> and <u><b>x</b></u>[^1] plus [^nope] and <u>a **b** c</u>.\n\n[^1]: The *note* with $x^2$\n    goes on with [^2] here.\n[^2]: Second.\n\nAfter.\n";

// spec: TEX-029
#[test]
fn underline_sub_and_superscript_are_written_as_commands_around_bold_and_italic() {
    let b = body_of(&doc("<u>under</u> H<sub>2</sub>O x<sup>2</sup> <u>a **b**</u> <sup>*i*</sup>\n", "pub")).to_string();
    for want in ["\\underline{under}", "\\textsubscript{2}", "\\textsuperscript{2}", "\\underline{\\textbf{b}}", "\\textsuperscript{\\textit{i}}"] {
        assert!(b.contains(want), "{want} is in {b}");
    }
    assert!(!b.contains("<u>") && !b.contains("<sub>") && !b.contains("<sup>"), "{b}");
}

// spec: TEX-029
#[test]
fn a_footnote_reference_is_the_definitions_text_and_the_definition_is_not_written_where_it_stands() {
    let b = body_of(&doc(NEW_FORMATS, "pub")).to_string();
    assert!(b.contains("\\footnote{The \\textit{note} with \\(x^2\\) goes on with [\\textasciicircum{}2] here.}"), "the definition with its continuation, nested reference as text: {b}");
    assert!(b.contains("[\\textasciicircum{}nope]"), "a reference without a definition stays text: {b}");
    assert_eq!(b.matches("\\footnote{").count(), 1, "{b}");
    assert!(!b.contains("Second") && !b.contains("]:") && !b.contains("\\textsuperscript{1}"), "the definitions and the mark text are not written: {b}");
    assert!(b.contains("After."), "{b}");
}

// spec: TEX-029
#[test]
fn a_footnote_in_a_table_cell_is_a_mark_and_its_text_comes_after_the_table() {
    let b = body_of(&doc("| a | b |\n| --- | --- |\n| x[^1] | y[^2] |\n\n[^1]: One\n[^2]: Two\n", "pub")).to_string();
    assert!(b.contains("x\\footnotemark{} & y\\footnotemark{}"), "{b}");
    assert!(b.contains("\\addtocounter{footnote}{-2}\n\\stepcounter{footnote}\\footnotetext{One}\n\\stepcounter{footnote}\\footnotetext{Two}\n"), "{b}");
    assert!(!b.contains("\\footnote{"), "{b}");
}

// spec: TEX-029
#[test]
fn br_in_a_table_cell_is_a_line_break_in_a_paragraph_column() {
    let b = body_of(&doc("| a | b |\n| --- | --- |\n| one<br>two | x<BR/>y |\n| three | z |\n", "pub")).to_string();
    assert!(b.contains("one \\newline two") && b.contains("x \\newline y"), "{b}");
    assert!(b.contains(">{\\raggedright\\arraybackslash}X>{\\raggedright\\arraybackslash}X"), "both columns are X columns: {b}");
    assert!(!b.to_ascii_lowercase().contains("<br"), "{b}");
}

// spec: TEX-029
#[test]
fn math_that_parses_is_written_unchanged_and_math_that_does_not_is_code() {
    let note = "Inline $\\frac{a}{b} + \\text{if } x \\in \\mathbb{R}$ and $$\\sum_{i=1}^n i$$ in text, $\\foo$ and $\\input{/etc/passwd}$.\n\n$$\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}\n$$\n\n```math\n\\sqrt[3]{x}\n```\n\n$$\\write18{x}$$\n\n```math\n\\frac{a\n```\n";
    let b = body_of(&doc(note, "pub")).to_string();
    assert!(b.contains("\\(\\frac{a}{b} + \\text{if } x \\in \\mathbb{R}\\)"), "{b}");
    assert!(b.contains("\\(\\displaystyle \\sum_{i=1}^n i\\)"), "{b}");
    assert!(b.contains("\\[\n\\begin{pmatrix} a & b \\\\ c & d \\end{pmatrix}\n\\]"), "{b}");
    assert!(b.contains("\\[\n\\sqrt[3]{x}\n\\]"), "{b}");
    // a formula Shale cannot typeset is code text, and the hidden TeX is not written besides
    assert!(b.contains("\\texttt{\\$\\textbackslash{}foo\\$}"), "{b}");
    assert!(!b.contains("\\input{/etc") && !b.contains("\\write18"), "{b}");
    assert!(b.contains("\\textbackslash{}input\\{/etc/passwd\\}") && b.contains("\\textbackslash{}write18\\{x\\}"), "{b}");
    assert!(b.contains("\\textbackslash{}frac\\{a"), "the failed block is code: {b}");
    assert_eq!(b.matches("\\frac{a}{b}").count(), 1, "the hidden TeX is not written twice: {b}");
    assert!(doc(note, "pub").contains("\\usepackage{amsmath}"), "amsmath for matrices and \\dfrac");
}

// spec: TEX-008, TEX-029
#[test]
fn a_hostile_formula_can_never_run_tex() {
    // everything TeX could take as a command comes out as text, and what is written as math has only known commands
    for hostile in [
        "$\\input{/etc/passwd}$",
        "$\\write18{x}$",
        "$\\text{\\input{a}}$",
        "$\\text{^^5cinput{a}}$",
        "$\\operatorname{^^5cinput}$",
        "$\\catcode`\\{=12$",
        "$a^^5cinput{x}$",
        "$\\text{50% rest}$",
        "$\\text{a}} \\end{document} \\text{b}$",
        "$$\\end{document}$$",
        "$a\\end{document}$",
        "$\\text{a {b}$",
        "$\\verb|x|$",
        "$\\csname x\\endcsname$",
        "$\\def\\a{b}$",
    ] {
        let t = doc(&format!("x {hostile} y\n\n{hostile}\n"), "pub");
        let b = body_of(&t);
        assert_eq!(t.matches("\\end{document}").count(), 1, "{hostile}: only the last line ends the document: {t}");
        for bad in ["\\input{", "\\write18", "\\catcode", "^^5c", "\\csname", "\\def\\", "\\verb"] {
            assert!(!b.contains(bad), "{hostile}: {bad} would be a command in {b}");
        }
    }
}
