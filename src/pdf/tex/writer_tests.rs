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

/// The files the rich note embeds and links to: a picture, a drawing, a PDF and another note.
pub(super) fn rich_assets() -> Vec<(&'static str, Vec<u8>)> {
    let mut png = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(80, 40, image::Rgba([30, 120, 200, 255])).write_to(&mut png, image::ImageFormat::Png).unwrap();
    vec![("pic.png", png.into_inner()), ("Sketch.excalidraw", crate::drawing::SAMPLE_SCENE.as_bytes().to_vec()), ("doc.pdf", crate::pdf::view::sample_pdf()), ("Another note.md", b"# Another note\n".to_vec())]
}

/// The set of the shipped tuda-ci kinds, as an imported one would be read (no folder on disk).
pub(super) fn tuda_set(settings: SetSettings) -> SetInfo {
    let kinds = TUDA.kinds.iter().map(|(id, text)| (id.to_string(), KindManifest::parse(text).unwrap())).collect();
    let meta = SetMeta { name: "TU Darmstadt".into(), version: Some("4.06".into()), imported_at: "2026-11-17".into(), source: Source::Local { path: "x".into() }, logo_file: Some("tuda_logo".into()) };
    SetInfo { id: "tuda".into(), dir: PathBuf::from("/nowhere/tuda"), meta, settings, kinds, problems: vec![], logo: LogoStatus::Missing, department_logo: None }
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
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text, opts, set: &set, kind: kind(&set, kind_id), fields: &fields(given), fonts: None });
    let _ = std::fs::remove_dir_all(dir);
    out
}

pub(super) fn doc(text: &str, kind_id: &str) -> String {
    doc_with(text, kind_id, &ExportOptions::for_note(), &[("author", "Ada")], &[]).unwrap_or_else(|e| panic!("{e}")).tex
}

/// The body of a document without the anchors of its headings (TEX-032, which has tests of its own).
fn body_of(tex: &str) -> String {
    let body = tex.split_once("\\begin{document}\n").unwrap().1.trim_end_matches("\\end{document}\n");
    without_anchors(body)
}

/// A text with every `\shaleanchor{shale-h…}` taken out, and the line end after one that
/// stands on a line of its own.
fn without_anchors(tex: &str) -> String {
    const MARK: &str = "\\shaleanchor{shale-h";
    let mut out = String::new();
    let mut rest = tex;
    while let Some(at) = rest.find(MARK) {
        out.push_str(&rest[..at]);
        let after = &rest[at + MARK.len()..];
        let end = after.find('}').unwrap() + 1;
        rest = &after[end..];
        if out.ends_with('\n') {
            rest = rest.strip_prefix('\n').unwrap_or(rest);
        }
    }
    out.push_str(rest);
    out
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

// spec: TEX-008, TEX-032
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

// spec: TEX-008, TEX-042
#[test]
fn blocks_are_written_as_environments() {
    let t = doc("## S\nfirst\nsecond line\n\n> q1\n> q2\n>> nested\n\n- a\n- b\n  - c\n    - d\n      - e\n        - f\n1. one\n\n7. seven\n8. eight\n\n- [ ] open\n- [x] done\n\n---\n\n```\nlet a = 1 & 2;\n\tindented\n```\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("first \\newline\nsecond line\n\n"), "a line break of the note is a line break: {b}");
    assert!(b.contains("\\begin{shalequote}\nq1 \\newline\nq2\n\\begin{shalequote}\nnested\n\\end{shalequote}\n\n\\end{shalequote}"), "{b}");
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
    assert!(b.contains("a & 1 & b \\& c \\\\\n") && b.contains("\\textcolor[RGB]{192,28,40}{Plain} & \\textbf{2} & 50\\% \\\\\n") && b.contains("\\bottomrule\n\\end{xltabular}"), "{b}");
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

// spec: TEX-008, TEX-018
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

// spec: TEX-008, TEX-018
#[test]
fn a_required_field_with_no_value_stops_the_export_and_names_the_field() {
    let e = doc_with("# Plan\ntext\n", "pub", &ExportOptions::for_note(), &[], &[]).unwrap_err();
    assert_eq!(e, "“Author” is empty: fill it in the export dialog or add `author:` to the note's properties");
    let e = doc_with("text", "letter", &ExportOptions::for_note(), &[], &[]).unwrap_err();
    assert!(e.starts_with("“Recipient (lines separated by \\\\)” is empty"), "{e}");
    assert!(doc_with("---\nauthor: X\n---\ntext", "pub", &ExportOptions::for_note(), &[], &[]).is_ok(), "a property is enough");
}

// spec: TEX-009, TEX-010
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
    let out = write(&Request { vault: &v, rel: Path::new("261117_Plan.md"), text: "# PLAN\n## A\n", opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "A")]), fonts: None }).unwrap();
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

/// The lines the tuda manifest of a kind has for the department logo, as the writer writes them.
fn department_lines(kind_id: &str) -> String {
    let set = tuda_set(SetSettings::default());
    format!("{}\n", kind(&set, kind_id).department_logo_lines(&set.settings).join("\n"))
}

/// A note written as a kind of a tuda set whose department logo slot is filled or empty.
fn doc_department(kind_id: &str, filled: bool, wanted: bool) -> String {
    let (v, dir) = temp_vault("dept", &[("Plan.md", b"# Plan\ntext\n")]);
    let mut set = tuda_set(SetSettings { accent: "9c".into(), ..SetSettings::default() });
    set.department_logo = filled.then(|| "department_logo.png".to_string());
    let opts = ExportOptions { department_logo: wanted, ..ExportOptions::for_note() };
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: "# Plan\ntext\n", opts: &opts, set: &set, kind: kind(&set, kind_id), fields: &fields(&[("author", "Ada"), ("recipient", "R")]), fonts: None });
    let _ = std::fs::remove_dir_all(dir);
    out.unwrap_or_else(|e| panic!("{e}")).tex
}

// spec: TEX-037, TEX-035
#[test]
fn the_department_logo_lines_are_written_after_the_preamble_only_when_the_slot_is_filled_the_box_is_on_and_the_kind_has_a_place() {
    for id in ["pub", "report", "thesis", "phd", "beamer", "letter", "poster", "sciposter", "leaflet"] {
        let lines = department_lines(id);
        let tex = doc_department(id, true, true);
        let at = tex.find(&lines).unwrap_or_else(|| panic!("{id}: the lines are not in\n{tex}"));
        // after the class and the preamble of the manifest, before the helpers and the document
        assert!(tex[..at].contains("\\usepackage[autostyle]{csquotes}\n") && tex[at..].contains("\\usepackage{graphicx}") && at < tex.find("\\begin{document}").unwrap(), "{id}: {tex}");
        assert!(tex.contains("{department_logo}") && !tex.contains("{{department_logo}"), "{id}: the placeholder is the file's name, with the braces of the command");
        // an empty slot, or the box off: nothing is written, the document is the one without a department logo
        for (filled, wanted) in [(false, true), (true, false), (false, false)] {
            let none = doc_department(id, filled, wanted);
            assert!(!none.contains("department_logo") && none == doc_department(id, false, false), "{id} (slot filled {filled}, box {wanted})");
        }
    }
    // a kind with no place for it never gets it
    for id in ["announcement", "exercise"] {
        assert_eq!(doc_department(id, true, true), doc_department(id, false, false), "{id}");
    }
    // pub: exactly the lines of the manifest
    assert!(doc_department("pub", true, true).contains("\\ExplSyntaxOn\n\\addTitleBoxLogo*{\\includegraphics[height=\\c_ptxcd_logoheight_dim]{department_logo}}\n\\ExplSyntaxOff\n"));
}

// spec: TEX-009
#[test]
fn the_table_of_contents_is_there_when_asked_for_and_the_kind_has_one() {
    let note = "# A\ntext\n## B\n";
    let tex = |kind_id: &str, toc: bool| doc_with(note, kind_id, &ExportOptions { toc, ..ExportOptions::for_note() }, &[("author", "A"), ("recipient", "R")], &[]).unwrap().tex;
    let on = tex("pub", true);
    assert!(body_of(&on).starts_with("\\maketitle\n\\tableofcontents\n\\newpage\n\n") && on.contains("\\setcounter{tocdepth}{3}\n"), "{on}");
    assert!(tex("thesis", true).contains("\\setcounter{tocdepth}{2}\n"), "chapter, section, subsection");
    assert!(!tex("pub", false).contains("tableofcontents") && !tex("pub", false).contains("tocdepth"));
    for k in ["beamer", "letter", "poster", "sciposter", "leaflet", "announcement"] {
        assert!(!tex(k, true).contains("tableofcontents"), "{k} has no contents");
    }
    assert!(!doc_with("no headings\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex.contains("tableofcontents"), "an empty list is not printed");
    assert!(on.contains("\\hypersetup{bookmarksdepth=5}"), "the bookmarks of the PDF reach every level");
}

// spec: TEX-040, TEX-009
#[test]
fn a_page_break_follows_the_table_of_contents_unless_asked_otherwise_and_is_not_doubled() {
    let body = |note: &str, kind_id: &str, opts: &ExportOptions| body_of(&doc_with(note, kind_id, opts, &[("author", "A")], &[]).unwrap().tex);
    let note = "# A\ntext\n## B\n";
    for kind_id in ["pub", "thesis"] {
        let on = body(note, kind_id, &ExportOptions::for_note());
        assert!(on.starts_with("\\maketitle\n\\tableofcontents\n\\newpage\n\n"), "{kind_id}: {on}");
        let off = body(note, kind_id, &ExportOptions { toc_break: false, ..ExportOptions::for_note() });
        assert!(off.contains("\\tableofcontents\n") && !off.contains("\\newpage"), "{kind_id}: {off}");
    }
    // the note's `print-toc-page-break` is what the dialog starts from
    let said = ExportOptions::for_note().with_note_properties("---\nprint-toc-page-break: false\n---\n# A\n");
    assert!(!said.toc_break && !body(note, "pub", &said).contains("\\newpage"));
    // no contents: no break; a page break of the note right after the contents is that break, not a second
    assert!(!body(note, "pub", &ExportOptions { toc: false, ..ExportOptions::for_note() }).contains("\\newpage"));
    let marked = body(&format!("{BREAK}\n# A\ntext\n"), "pub", &ExportOptions { break_before: 1, ..ExportOptions::for_note() });
    assert_eq!(marked.matches("\\newpage").count(), 1, "{marked}");
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
    assert!(without_anchors(&t).contains("\\textbf{A heading}\\par\\smallskip\n\nDear text \\& more\n\n\\closing{Yours}\n\\end{letter}\n\\end{document}"), "{t}");
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

// spec: MEDIA-016, TEX-015, TEX-031
#[test]
fn a_picture_and_a_pdf_outside_the_vault_are_files_in_the_work_folder_and_a_missing_one_is_a_warning() {
    let assets = rich_assets();
    let (mut v, dir) = temp_vault("outside", &[("Plan.md", b"x")]);
    let lib = dir.with_file_name(format!("{}-library", dir.file_name().unwrap().to_string_lossy()));
    std::fs::create_dir_all(lib.join("My Pics")).unwrap();
    std::fs::write(lib.join("My Pics/pic.png"), &assets[0].1).unwrap();
    std::fs::write(lib.join("doc.pdf"), &assets[2].1).unwrap();
    v.set_libraries(&[crate::media::outside::Library { name: "Media".into(), path: lib.clone() }]);
    let address = crate::media::testdata::file_address(&lib.join("My Pics/pic.png"));
    let note = format!("## S\n![[@Media/My Pics/pic.png|200]]\n![[@Media/doc.pdf]]\n![x]({address})\nan icon ![[@Media/My Pics/pic.png]] here\n![[@Media/gone.png]]\n![[@Nowhere/a.png]]\nin a line ![[@Media/gone.png]]\n");
    let set = tuda_set(SetSettings { accent: "9c".into(), ..SetSettings::default() });
    let opts = ExportOptions::for_note();
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: &note, opts: &opts, set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "A")]), fonts: None }).unwrap();
    let b = body_of(&out.tex);
    assert!(b.contains("\\includegraphics[width=0.278\\linewidth,height=0.8\\textheight,keepaspectratio]{picture-1.png}"), "200 points of the editor's 720: {b}");
    assert!(b.contains("\\textit{@Media/My Pics/pic.png}") && b.contains("\\includegraphics[page=1,"), "{b}");
    assert_eq!(b.matches("picture-1.png}").count(), 3, "one file for the picture however it is linked: {b}");
    assert!(b.contains("\\raisebox{-0.2\\baselineskip}{\\includegraphics[height=0.9\\baselineskip"), "the picture in the line: {b}");
    let names: Vec<&str> = out.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["picture-1.png", "document-2.pdf"]);
    assert_eq!(out.warnings, ["missing: @Media/gone.png", "missing: @Nowhere/a.png", "missing: @Media/gone.png"], "a missing file or library is told, not a failure");
    assert!(b.contains("\\textit{[missing: @Nowhere/a.png]}"), "{b}");
    let _ = std::fs::remove_dir_all(lib);
    let _ = std::fs::remove_dir_all(dir);
}

// spec: TEX-015
#[test]
fn an_embed_that_cannot_be_used_is_a_line_of_text_and_a_warning_not_a_failure() {
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let note = "## S\n![[missing.png]]\n![[broken.png]]\n![[data.zip]]\n";
    let out = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], &[("broken.png", b"not a png"), ("data.zip", b"x"), files[0]]).unwrap();
    let b = body_of(&out.tex);
    assert!(b.contains("\\textit{[missing: missing.png]}") && b.contains("\\textit{[not readable: broken.png]}") && b.contains("\\textit{[data.zip: not in a PDF]}"), "{b}");
    assert_eq!(out.warnings, ["missing: missing.png", "not readable: broken.png", "data.zip: not in a PDF"]);
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
    for later in ["\\usepackage[english]{babel}", "\\usepackage{graphicx}", "\\usepackage[normalem]{ulem}", "\\usepackage{xltabular}", "\\newtcolorbox{shalecode}", "\\title{Plan}", "\\author{Ada}", "\\begin{document}"] {
        assert!(t.find(later).is_some_and(|p| p > class), "{later} comes after the class: {t}");
    }
    assert!(t.trim_end().ends_with("\\end{document}") && t.matches("\\begin{document}").count() == 1);
    // the language of the set
    let (v, d) = temp_vault("lang", &[]);
    let set = tuda_set(SetSettings { language: Language::De, ..SetSettings::default() });
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: "x", opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "A")]), fonts: None }).unwrap();
    assert!(out.tex.contains("lang=de}") && out.tex.contains("\\documentclass[ngerman]{tudapub}") && out.tex.contains("\\usepackage[ngerman]{babel}"), "{}", out.tex);
    let _ = std::fs::remove_dir_all(d);
}

// spec: TEX-013, TEX-018
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

// spec: TEX-008, TEX-030, TEX-031, TEX-032
#[test]
fn every_kind_of_the_tuda_set_writes_the_rich_note() {
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let set = tuda_set(SetSettings::default());
    for (id, k) in &set.kinds {
        let (v, d) = temp_vault("rich", &[&[("Plan.md", RICH_NOTE.as_bytes())][..], &files].concat());
        let out =
            write(&Request { vault: &v, rel: Path::new("Plan.md"), text: RICH_NOTE, opts: &ExportOptions::for_note(), set: &set, kind: k, fields: &fields(&[("author", "Ada"), ("recipient", "Dr. Hopper \\\\ Navy Street 1")]), fonts: None })
                .unwrap_or_else(|e| panic!("{id}: {e}"));
        assert!(out.tex.contains("\\begin{document}") && out.tex.trim_end().ends_with("\\end{document}") && out.warnings == ["missing: missing.png"], "{id}: {:?}", out.warnings);
        assert!(!out.tex.contains("secret") && !out.tex.contains("numbered-headings"), "{id}: the front matter is not printed");
        // the code box, the picture in a line, the jump to a heading and the other note (TEX-030 to TEX-032)
        assert!(out.tex.contains("\\begin{shalecode}") && out.tex.contains("\\includegraphics[height=0.9\\baselineskip"), "{id}: {}", out.tex);
        assert!(out.tex.contains("\\hyperlink{shale-h") && out.tex.contains("\\textcolor[RGB]{26,95,180}{Another note}"), "{id}: {}", out.tex);
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

// spec: TEX-030
#[test]
fn fenced_code_is_on_a_light_box_that_breaks_across_pages_where_tables_may() {
    let note = "## S\n```rust\nfn main() {}\n```\n";
    let t = doc(note, "pub");
    assert!(t.contains("\\usepackage{tcolorbox}\n\\definecolor{shalecodebg}{RGB}{244,244,246}\n\\tcbuselibrary{skins,breakable}\n\\newtcolorbox{shalecode}{breakable,colback=shalecodebg,colframe=shalecodebg,"), "{t}");
    let b = body_of(&t);
    assert!(b.contains("\\begin{shalecode}\n\\mbox{}\\textcolor[RGB]{125,45,170}{fn}") && b.contains("\\end{shalecode}"), "the box keeps the colours of the code: {b}");
    for k in ["thesis", "letter", "leaflet"] {
        assert!(doc_with(note, k, &ExportOptions::for_note(), &[("author", "A"), ("recipient", "R")], &[]).unwrap().tex.contains("\\newtcolorbox{shalecode}{breakable,"), "{k}: long code breaks across pages");
    }
    // slides, boxes and columns: a box that stays whole
    for k in ["beamer", "sciposter", "poster"] {
        let t = doc_with(note, k, &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap().tex;
        assert!(t.contains("\\newtcolorbox{shalecode}{colback=shalecodebg,") && !t.contains("breakable") && body_of(&t).contains("\\begin{shalecode}"), "{k}: {t}");
    }
}

// spec: TEX-015
#[test]
fn tex_015_the_missing_placeholder_of_a_picture_is_in_the_paper_red_of_missing_links() {
    let note = "## S\nOn its own:\n\n![[gone.png]]\n\nIn a line ![[away.png]] here and [[Nowhere]].\n";
    let out = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap();
    let b = body_of(&out.tex);
    for what in ["gone.png", "away.png"] {
        assert!(b.contains(&format!("\\textcolor[RGB]{{192,28,40}}{{\\textit{{[missing: {what}]}}}}")), "{what} in the paper red: {b}");
    }
    assert!(!b.contains("255,104,104"), "never the screen colour: {b}");
}

// spec: TEX-031
#[test]
fn a_picture_inside_a_line_is_as_high_as_the_line_and_a_missing_one_says_so() {
    let assets = rich_assets();
    let files: Vec<(&str, &[u8])> = assets.iter().map(|(n, b)| (*n, b.as_slice())).collect();
    let note = "## S\nA ![[pic.png]] icon, ![[pic.png|Sea|30]] again, ![[gone.png]] and ![[Sketch.excalidraw]] here.\n\n| A |\n| --- |\n| cell ![[pic.png]] |\n\n![[pic.png]]\n";
    let out = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], &files).unwrap();
    let b = body_of(&out.tex);
    let pic = "\\raisebox{-0.2\\baselineskip}{\\includegraphics[height=0.9\\baselineskip,width=\\linewidth,keepaspectratio]{picture-1.png}}";
    assert!(b.contains(&format!("A {pic} icon, {pic} again, ")), "in the line, as high as it: {b}");
    assert!(b.contains(&format!("cell {pic}")), "in a table cell too: {b}");
    assert!(b.contains("\\textcolor[RGB]{192,28,40}{\\textit{[missing: gone.png]}} and"), "a missing picture says so, in the line: {b}");
    assert!(b.contains("{Sketch.excalidraw}") && !b.contains("drawing-"), "a drawing in a line stays its link text: {b}");
    assert!(b.contains("{picture-1.png}\\par\\nopagebreak"), "the same picture on its own line is the same file: {b}");
    assert_eq!(out.files.len(), 1, "one file");
    assert_eq!(out.warnings, ["missing: gone.png"]);
    let off = doc_with("A ![[pic.png]] icon\n", "pub", &ExportOptions { embeds: false, ..ExportOptions::for_note() }, &[("author", "A")], &files).unwrap();
    assert!(!off.tex.contains("includegraphics[") && off.files.is_empty() && body_of(&off.tex).contains("{pic.png}"), "with embeds off it is its link text: {}", off.tex);
    let broken = doc_with("A ![[broken.png]] icon\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &[("broken.png", b"not a png")]).unwrap();
    assert!(body_of(&broken.tex).contains("\\textit{[not readable: broken.png]}") && broken.warnings == ["not readable: broken.png"], "{}", broken.tex);
}

// spec: TEX-032
#[test]
fn a_link_to_a_heading_of_the_note_goes_there_and_links_to_other_notes_are_link_coloured_text() {
    let note = "# Plan\n## Goal\nSee [[#Goal]], [[Plan#Method|the method]], [[#Nowhere]], [[#Plan]], [[Other]], [[Other#Goal]] and [[Gone]].\n## Method\nx\n";
    let files: &[(&str, &[u8])] = &[("Other.md", b"## Goal\n")];
    let tex = doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], files).unwrap().tex;
    let blue = |t: &str| format!("\\textcolor[RGB]{{26,95,180}}{{{t}}}");
    assert!(tex.contains("\\section{Goal}\\shaleanchor{shale-h1}\n") && tex.contains("\\section{Method}\\shaleanchor{shale-h3}\n"), "every heading has an anchor: {tex}");
    assert!(tex.contains(&format!("See \\hyperlink{{shale-h1}}{{{}}}, \\hyperlink{{shale-h3}}{{{}}}, ", blue("Goal"), blue("the method"))), "a link to a heading of the note goes there: {tex}");
    assert!(tex.contains(&format!(", {}, {}, ", blue("Nowhere"), blue("Plan"))), "a heading that is not written has no anchor to go to: {tex}");
    assert!(tex.contains(&format!(", {}, {} and \\textcolor[RGB]{{192,28,40}}{{Gone}}.", blue("Other"), blue("Other \\textgreater{} Goal"))), "another note is link-coloured text, a missing one red: {tex}");
    assert_eq!(tex.matches("\\hyperlink{").count(), 2, "{tex}");
    // every family has the anchors where the headings are
    let go = |k: &str| doc_with(note, k, &ExportOptions::for_note(), &[("author", "A"), ("recipient", "R")], files).unwrap().tex;
    let beamer = go("beamer");
    assert!(beamer.contains("\\begin{frame}[allowframebreaks]{Goal}\n\\shaleanchor{shale-h1}\n") && beamer.contains("\\hyperlink{shale-h3}"), "{beamer}");
    let boxes = go("sciposter");
    assert!(boxes.contains("[title={Goal}]{name=box1,column=1,row=1}\n\\shaleanchor{shale-h1}\n"), "{boxes}");
    let letter = go("letter");
    assert!(letter.contains("\\noindent\\shaleanchor{shale-h1}\\textbf{Goal}") && letter.contains("\\hyperlink{shale-h1}"), "{letter}");
    let slides = doc("# Part\ntext\n## Slide\n[[#Part]]\n", "beamer");
    assert!(slides.contains("\\section{Part}\n\n\\begin{frame}[allowframebreaks]{Part}\n\\shaleanchor{shale-h0}\n") && slides.contains("{Slide}\n\\shaleanchor{shale-h2}\n"), "a section's anchor is in its first frame: {slides}");
}

const BREAK: &str = "<div style=\"page-break-after: always;\"></div>";

// spec: TEX-037
#[test]
fn a_page_break_marker_is_a_newpage_once_and_the_html_is_not_written() {
    let note = format!("# One\n\nbefore\n\n{BREAK}\n\nafter\n\n<div style='break-before: page'></div>\n\n```\n{BREAK}\n```\n\nlast\n");
    for kind in ["pub", "report"] {
        let t = doc_with(&note, kind, &ExportOptions { toc_break: false, ..ExportOptions::for_note() }, &[("author", "Ada")], &[]).unwrap().tex;
        assert_eq!(t.matches("\\newpage").count(), 2, "{kind}: one for each marker, none for the one in the code: {t}");
        assert_eq!(t.matches("<div").count(), 0, "{kind}: no raw HTML (the code's is escaped): {t}");
        assert!(!t.contains("break-before"), "{kind}: {t}");
        let b = body_of(&t);
        let at = b.find("before").unwrap();
        assert!(b[at..].starts_with("before\n\n\\newpage\n\nafter"), "{kind}: {b}");
    }
    // markers one after the other are one new page, and one at the very end is written too
    let t = doc_with(&format!("a\n{BREAK}\n{BREAK}\n\n{BREAK}\nb\n{BREAK}\n"), "pub", &ExportOptions { toc_break: false, ..ExportOptions::for_note() }, &[("author", "Ada")], &[]).unwrap().tex;
    assert_eq!(t.matches("\\newpage").count(), 2, "{t}");
    // in slides and boxes the frames and boxes are the pages
    for kind in ["beamer", "sciposter"] {
        let t = doc(&format!("# One\n\n## Frame\n\na\n\n{BREAK}\n\nb\n"), kind);
        assert!(!t.contains("\\newpage") && !t.contains("<div"), "{kind}: {t}");
    }
    // the block is a block of its own that ends a paragraph
    use super::blocks::{parse, Block};
    let blocks = parse(&format!("a\n{BREAK}\nb\n"));
    assert_eq!(blocks, [Block::Para(vec!["a".into()]), Block::PageBreak, Block::Para(vec!["b".into()])]);
}

// spec: TEX-039
#[test]
fn an_alignment_wrapper_is_a_center_or_flushright_environment_and_the_html_is_not_written() {
    let note = "# One\n\nplain\n\n<div align=\"center\">\n\n## Middle\n\ncentred text\n\n</div>\n\n<div align=\"right\">\n\nright text\n\n</div>\n\n<div align=\"justify\">\n\njustified text\n\n</div>\n\n<div align=\"center\">\n\nlost\n";
    for kind in ["pub", "report"] {
        let t = doc(note, kind);
        let b = body_of(&t);
        assert_eq!(b.matches("\\begin{center}").count(), 1, "{kind}: {b}");
        assert_eq!(b.matches("\\end{center}").count(), 1, "{kind}: {b}");
        assert_eq!((b.matches("\\begin{flushright}").count(), b.matches("\\end{flushright}").count()), (1, 1), "{kind}: {b}");
        let (begin, end) = (b.find("\\begin{center}").unwrap(), b.find("\\end{center}").unwrap());
        assert!(b[begin..end].contains("Middle") && b[begin..end].contains("centred text") && !b[begin..end].contains("plain"), "{kind}: {b}");
        let (begin, end) = (b.find("\\begin{flushright}").unwrap(), b.find("\\end{flushright}").unwrap());
        assert!(b[begin..end].contains("right text") && !b[begin..end].contains("centred"), "{kind}: {b}");
        assert!(b.contains("justified text"), "justified is what LaTeX sets: {b}");
        assert_eq!((t.matches("<div").count(), t.matches("textless{}div").count()), (0, 1), "{kind}: only the opening line without a closing one is text, and it is escaped: {t}");
    }
    // in slides and boxes the frames and boxes are the units: no environment
    for kind in ["beamer", "sciposter"] {
        let t = doc("# One\n\n## Frame\n\na\n\n<div align=\"center\">\n\nb\n\n</div>\n", kind);
        assert!(!t.contains("\\begin{center}") && !t.contains("flushright") && !t.contains("<div"), "{kind}: {t}");
    }
    // the blocks
    use super::blocks::{parse, Block};
    use crate::markdown::Align;
    let blocks = parse("a\n<div align=\"center\">\nb\n</div>\nc\n");
    assert_eq!(blocks, [Block::Para(vec!["a".into()]), Block::AlignStart(Align::Center), Block::Para(vec!["b".into()]), Block::AlignEnd(Align::Center), Block::Para(vec!["c".into()])]);
}

// spec: TEX-038
#[test]
fn print_break_before_writes_a_newpage_before_the_headings_up_to_its_level_but_not_the_first_block() {
    let note = "# One\n\ntext\n\n## Sub\n\ntext\n\n### Deep\n\n# Two\n\ntext\n";
    let at = |level: u8, kind: &str| doc_with(note, kind, &ExportOptions { break_before: level, toc_break: false, ..ExportOptions::for_note() }, &[("author", "Ada")], &[]).unwrap().tex;
    assert_eq!(at(0, "pub").matches("\\newpage").count(), 0);
    let one = at(1, "pub");
    assert_eq!(one.matches("\\newpage").count(), 1, "before Two only: {one}");
    assert!(one.find("\\newpage").unwrap() < one.find("Two").unwrap() && one.find("\\newpage").unwrap() > one.find("Deep").unwrap());
    assert_eq!(at(2, "pub").matches("\\newpage").count(), 2, "before Sub and Two");
    assert_eq!(at(6, "pub").matches("\\newpage").count(), 3);
    // a marker right before such a heading does not double it
    let t = doc_with(&format!("a\n{BREAK}\n# One\nb\n# Two\n"), "pub", &ExportOptions { break_before: 1, toc_break: false, ..ExportOptions::for_note() }, &[("author", "Ada")], &[]).unwrap().tex;
    assert_eq!(t.matches("\\newpage").count(), 2, "{t}");
    // slides and the like have their own pages
    assert_eq!(at(1, "beamer").matches("\\newpage").count(), 0);
}

// ---------------------------------------------------------------- media cards (TEX-015)

/// A note written with these media files of the test fixtures.
fn with_media(note: &str, names: &[&str]) -> Document {
    let media: Vec<(String, Vec<u8>)> = names.iter().map(|n| (n.to_string(), std::fs::read(crate::media::testdata::fixture(n)).unwrap())).collect();
    let files: Vec<(&str, &[u8])> = media.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
    doc_with(note, "pub", &ExportOptions::for_note(), &[("author", "A")], &files).unwrap()
}

/// The drawn content of one of the PDFs of the work folder.
fn pdf_text(out: &Document, name: &str) -> String {
    let bytes = &out.files.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no file {name}")).1;
    crate::pdf::export::tests::contents(bytes)
}

// spec: TEX-015
#[test]
fn tex_015_a_sound_is_its_player_card_as_a_vector_pdf_with_the_caption_below() {
    use crate::pdf::export::tests::hex;
    let out = with_media("## S\n![[tone-cover.mp3]]\n\n![[tone.wav]]\n", &["tone-cover.mp3", "tone.wav"]);
    let b = body_of(&out.tex);
    assert!(out.warnings.is_empty() && !b.contains("not in a PDF"), "{b}");
    // the card is as wide in proportion to the column as the editor's
    let card = crate::media::widget::card_size(&crate::media::testdata::fixture("tone.wav"), crate::embed_size::DEFAULT_COLUMN, None);
    let share = format!("{:.3}", card.x / crate::embed_size::DEFAULT_COLUMN);
    assert!(b.contains(&format!("\\includegraphics[width={share}\\linewidth,height=0.8\\textheight,keepaspectratio]{{player-1.pdf}}")), "{b}");
    assert!(b.contains("\\textit{tone-cover.mp3}") && b.contains("\\textit{tone.wav}"), "the caption below: {b}");
    let names: Vec<&str> = out.files.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["player-1.pdf", "player-2.pdf"]);
    // the cover is the artwork; the name and the times are text
    let with = pdf_text(&out, "player-1.pdf");
    assert!(with.contains("/Im") && with.contains(&hex("tone-cover.mp3")) && with.contains(&hex("0:00")), "{with}");
    // without a cover: a grey tile with the name, no picture
    let without = pdf_text(&out, "player-2.pdf");
    assert!(!without.contains("/Im") && without.contains(&hex("tone.wav")), "{without}");
    // one page the size of the card
    let pdf = hayro::hayro_syntax::Pdf::new(out.files[1].1.clone()).unwrap();
    assert_eq!(pdf.pages().len(), 1);
    let (w, h) = pdf.pages()[0].render_dimensions();
    assert!((w - card.x).abs() < 0.5 && (h - card.y).abs() < 0.5, "{w}x{h} against {card:?}");
}

// spec: TEX-015
#[test]
fn tex_015_a_video_is_its_first_frame_with_the_play_button_or_the_tile_without_ffmpeg() {
    let out = with_media("## S\n![[clip.mp4|320]]\n", &["clip.mp4"]);
    let b = body_of(&out.tex);
    assert!(out.warnings.is_empty() && b.contains("]{player-1.pdf}") && b.contains("\\textit{clip.mp4}"), "{b}");
    let card = pdf_text(&out, "player-1.pdf");
    assert!(card.contains(&crate::pdf::export::tests::hex("clip.mp4")) || card.contains("/Im"), "the card has the name or the frame");
    if crate::media::ffmpeg::locate().is_some() {
        assert!(card.contains("/Im"), "the first frame is the artwork");
        assert!(b.contains(&format!("width={:.3}\\linewidth", 320.0 / crate::embed_size::DEFAULT_COLUMN)), "as wide as the link says: {b}");
    } else {
        // without ffmpeg the card is as the built-in export draws it: the tile, no picture
        assert!(!card.contains("/Im"));
        println!("the first frame is not tested: ffmpeg is not installed");
    }
}

// spec: TEX-015
#[test]
fn tex_015_a_sound_used_twice_is_one_file_and_a_missing_one_keeps_the_placeholder() {
    let out = with_media("## S\n![[tone.wav]]\n\n![[tone.wav|300]]\n", &["tone.wav"]);
    assert_eq!(out.files.len(), 1, "one file for the sound however often it is embedded");
    assert_eq!(body_of(&out.tex).matches("{player-1.pdf}").count(), 2);
    let gone = doc_with("## S\n![[gone.mp3]]\n![[gone.mp4]]\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap();
    let b = body_of(&gone.tex);
    assert!(b.contains("\\textit{[missing: gone.mp3]}") && b.contains("\\textit{[missing: gone.mp4]}") && gone.files.is_empty(), "{b}");
    assert_eq!(gone.warnings, ["missing: gone.mp3", "missing: gone.mp4"]);
}

// spec: TEX-041
#[test]
fn tex_041_emoji_are_set_in_the_emoji_font_that_travels_with_the_document() {
    let out = doc_with("## S\n- [ ] pay 📅 2026-12-01\n- done ✅ and 👍🏽 and 👨‍👩‍👧 and ⚠\u{fe0f}\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap();
    let b = body_of(&out.tex);
    assert!(b.contains("pay \\shaleemoji{📅} 2026-12-01") && b.contains("\\shaleemoji{✅}") && b.contains("\\shaleemoji{⚠}"), "{b}");
    assert!(b.contains("\\shaleemoji{👍🏽}") && b.contains("\\shaleemoji{👨👩👧}"), "a run of emoji is one group, the joiner left out: {b}");
    assert!(!b.contains('\u{fe0f}') && !b.contains('\u{200d}'), "{b}");
    // the font file is in the work folder and the preamble sets it up, with a text version for bookmarks
    assert!(out.files.iter().any(|(n, bytes)| n == "shale-emoji.ttf" && bytes.len() > 10_000), "{:?}", out.files.iter().map(|f| &f.0).collect::<Vec<_>>());
    assert!(out.tex.contains("\\newfontface\\shaleemojiface[Path=./]{shale-emoji.ttf}") && out.tex.contains("\\pdfstringdefDisableCommands{\\renewcommand{\\shaleemoji}[1]{#1}}"));
    // a heading and a field too
    let head = doc_with("## Plan 📅\n", "pub", &ExportOptions::for_note(), &[("author", "A 🎓")], &[]).unwrap();
    assert!(head.tex.contains("Plan \\shaleemoji{📅}") && head.tex.contains("A \\shaleemoji{🎓}"), "{}", head.tex);
    // no emoji: no font, no setup
    let plain = doc_with("## S\ntext × → é\n", "pub", &ExportOptions::for_note(), &[("author", "A")], &[]).unwrap();
    assert!(plain.files.is_empty() && !plain.tex.contains("shaleemoji"));
}

// spec: TEX-008
#[test]
fn tex_008_tags_are_violet_on_their_light_field_as_in_the_built_in_export() {
    let b = body_of(&doc("## S\nsee #ideas and #a/b here\n", "pub"));
    assert!(b.contains("\\colorbox[RGB]{236,232,251}{\\textcolor[RGB]{105,65,198}{\\#ideas}}"), "{b}");
    assert!(b.contains("\\textcolor[RGB]{105,65,198}{\\#a/b}"), "{b}");
}

// spec: TEX-042
#[test]
fn tex_042_a_quote_has_the_bar_and_a_note_with_print_properties_the_plain_indent() {
    let t = doc("## S\n> quoted\n> words\n", "pub");
    let b = body_of(&t);
    assert!(b.contains("\\begin{shalequote}\nquoted \\newline\nwords\n\\end{shalequote}"), "{b}");
    assert!(t.contains("\\newtcolorbox{shalequote}{breakable,enhanced,blanker,borderline west={2.5pt}{0pt}{shalequotebar},colupper=shalequoteink"), "{t}");
    assert!(t.contains("\\definecolor{shalequotebar}{RGB}{149,121,215}") && t.contains("\\tcbuselibrary{skins,breakable}"), "{t}");
    let paper = doc_with("## S\n> quoted\n", "pub", &ExportOptions { paper_look: true, ..ExportOptions::for_note() }, &[("author", "A")], &[]).unwrap();
    assert!(body_of(&paper.tex).contains("\\begin{quote}\nquoted\n\\end{quote}"), "{}", paper.tex);
}

// ---------------------------------------------------------------- fonts (TEX-054 to TEX-059, #658)

use super::fonts::FontInput;
use crate::fonts::resolve::Values;
use crate::fonts::scan::{scan_system, scan_vault, Catalogue};
use crate::fonts::test_fonts::{folder, Spec};
use std::sync::Arc;

/// A note with the `print-font*` properties given.
fn font_note(props: &[(&str, &str)]) -> String {
    let lines: String = props.iter().map(|(k, v)| format!("{k}: {v}\n")).collect();
    format!("---\n{lines}---\n# Plan\ntext\n")
}

fn garamond_faces() -> Vec<(&'static str, Spec)> {
    vec![
        ("Garamond-Regular.ttf", Spec::new("Garamond", 400, false)),
        ("Garamond-Bold.ttf", Spec::new("Garamond", 700, false)),
        ("Garamond-Italic.ttf", Spec::new("Garamond", 400, true)),
        ("Garamond-BoldItalic.ttf", Spec::new("Garamond", 700, true)),
    ]
}

/// What a font document is made from: the vault's files below `fonts/` (or another folder when the
/// name has one), the installed fonts (a test folder: a test never reads the machine's), the set's
/// and the vault's settings.
struct Fonts<'a> {
    vault: Vec<(&'a str, Spec)>,
    system: Vec<(&'a str, Spec)>,
    set: SetSettings,
    settings: Values,
    /// The fields of the export dialog (PDF-039), which take the place of the note's properties.
    dialog: Option<Values>,
}

impl<'a> Fonts<'a> {
    fn new() -> Self {
        Fonts { vault: vec![], system: vec![], set: SetSettings::default(), settings: Values::default(), dialog: None }
    }

    /// The note written as the `pub` kind of the tuda set with these fonts.
    fn write(&self, note: &str) -> Result<Document, String> {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let (v, dir) = temp_vault("fonts", &[("Plan.md", note.as_bytes())]);
        let sys = folder(&format!("texfonts-{n}-sys"));
        for (name, spec) in &self.vault {
            let rel = if name.contains('/') { name.to_string() } else { format!("fonts/{name}") };
            spec.write(&dir, &rel);
        }
        for (name, spec) in &self.system {
            spec.write(&sys, name);
        }
        let catalogue = Catalogue { vault: Arc::new(scan_vault(&dir)), system: Arc::new(scan_system(Some(std::slice::from_ref(&sys)))), system_ready: true, generation: 1 };
        let input = FontInput { catalogue, vault: self.settings.clone() };
        let set = tuda_set(SetSettings { accent: "9c".into(), ..self.set.clone() });
        let out = write(&Request {
            vault: &v,
            rel: Path::new("Plan.md"),
            text: note,
            opts: &ExportOptions { font_note: self.dialog.clone(), ..ExportOptions::for_note() },
            set: &set,
            kind: kind(&set, "pub"),
            fields: &fields(&[("author", "Ada")]),
            fonts: Some(&input),
        });
        let _ = std::fs::remove_dir_all(dir);
        let _ = std::fs::remove_dir_all(sys);
        out
    }

    fn tex(&self, note: &str) -> String {
        self.write(note).unwrap_or_else(|e| panic!("{e}")).tex
    }
}

fn position(tex: &str, what: &str) -> usize {
    tex.find(what).unwrap_or_else(|| panic!("{what:?} is not in\n{tex}"))
}

// spec: TEX-054
#[test]
fn tex_054_the_three_commands_follow_the_class_and_the_templates_setup_and_come_before_the_document() {
    let note = font_note(&[("print-font", "TeX Gyre Pagella"), ("print-font-sans", "TeX Gyre Heros"), ("print-font-mono", "TeX Gyre Cursor")]);
    let tex = Fonts::new().tex(&note);
    let (main, sans, mono) = (position(&tex, "\\setmainfont{TeX Gyre Pagella}\n"), position(&tex, "\\setsansfont{TeX Gyre Heros}\n"), position(&tex, "\\setmonofont{TeX Gyre Cursor}\n"));
    assert!(main < sans && sans < mono, "main, sans, mono: {tex}");
    // after the class, the template's own lines and the writer's other helpers, before the document
    let set = tuda_set(SetSettings::default());
    let last_template_line = kind(&set, "pub").preamble.last().map(|l| position(&tex, l.trim())).unwrap_or(0);
    assert!(position(&tex, "\\documentclass") < main && last_template_line < main && position(&tex, "\\newtcolorbox{shalequote}") < main, "{tex}");
    assert!(mono < position(&tex, "\\begin{document}"), "{tex}");
    // fontspec comes first, once, also though the class did not load it and the text has no emoji
    let load = "\\IfPackageLoadedTF{fontspec}{}{\\usepackage{fontspec}}\n";
    assert_eq!(tex.matches(load).count(), 1, "{tex}");
    assert!(position(&tex, load) < main);
}

// spec: TEX-054
#[test]
fn tex_054_a_document_without_a_font_value_is_written_as_before_and_the_words_of_the_pdf_select_nothing() {
    let plain = font_note(&[]);
    let tex = Fonts::new().tex(&plain);
    assert_eq!(tex, doc(&plain, "pub"), "the same document as without fonts");
    assert!(!tex.contains("\\setmainfont") && !tex.contains("\\setsansfont") && !tex.contains("\\setmonofont") && !tex.contains("usepackage{fontspec}"), "{tex}");
    // sans, serif and mono are the fonts of the built-in PDF: the template keeps its own
    let words = Fonts::new().write(&font_note(&[("print-font", "serif")])).unwrap();
    assert_eq!(words.tex, tex);
    assert!(words.warnings.is_empty(), "{:?}", words.warnings);
    assert!(words.fonts.is_empty() && words.files.is_empty());
}

// spec: TEX-054, TEX-059
#[test]
fn tex_059_the_note_wins_then_the_set_then_the_vault_for_each_role_on_its_own() {
    let mut f = Fonts::new();
    f.settings = Values { text: Some("Vault Text".into()), sans: Some("Vault Sans".into()), mono: Some("Vault Mono".into()) };
    f.set = SetSettings { font: "Set Text".into(), font_sans: "Set Sans".into(), ..SetSettings::default() };
    let tex = f.tex(&font_note(&[("print-font", "Note Text")]));
    assert!(tex.contains("\\setmainfont{Note Text}"), "{tex}");
    assert!(tex.contains("\\setsansfont{Set Sans}"), "the set before the vault: {tex}");
    assert!(tex.contains("\\setmonofont{Vault Mono}"), "the vault when nothing else says: {tex}");
    let tex = f.tex(&font_note(&[]));
    assert!(tex.contains("\\setmainfont{Set Text}"), "{tex}");
    // the keys of the set may be padded or empty: an empty one is a missing one
    f.set = SetSettings { font: "  ".into(), font_sans: " Set Sans ".into(), ..SetSettings::default() };
    let tex = f.tex(&font_note(&[]));
    assert!(tex.contains("\\setmainfont{Vault Text}") && tex.contains("\\setsansfont{Set Sans}"), "{tex}");
}

// spec: PDF-039, TEX-054
#[test]
fn pdf_039_the_fields_of_the_dialog_take_the_place_of_the_notes_properties_in_the_latex_export() {
    let mut f = Fonts::new();
    f.settings.mono = Some("Vault Mono".into());
    f.dialog = Some(Values { text: Some("Dialog Text".into()), sans: None, mono: None });
    let tex = f.tex(&font_note(&[("print-font", "Note Text"), ("print-font-sans", "Note Sans")]));
    assert!(tex.contains("\\setmainfont{Dialog Text}") && !tex.contains("Note Text"), "the field is the note's value: {tex}");
    assert!(!tex.contains("Note Sans"), "a cleared field is not the note's: {tex}");
    assert!(tex.contains("\\setmonofont{Vault Mono}"), "an empty field falls to the vault: {tex}");
}

// spec: TEX-059
#[test]
fn tex_059_a_bad_value_of_the_set_is_a_warning_and_the_next_entry_is_used() {
    let mut f = Fonts::new();
    f.set = SetSettings { font: "x".repeat(201), ..SetSettings::default() };
    f.settings.text = Some("Vault Text".into());
    let out = f.write(&font_note(&[])).unwrap();
    assert!(out.tex.contains("\\setmainfont{Vault Text}"), "{}", out.tex);
    assert!(out.warnings.iter().any(|w| w.contains("template set") && w.contains("longer than 200")), "{:?}", out.warnings);
}

// spec: TEX-054
#[test]
fn tex_054_a_system_font_is_passed_by_name_and_not_copied() {
    let mut f = Fonts::new();
    f.system = vec![("Libertinus-Regular.ttf", Spec::new("Libertinus Serif", 400, false)), ("Libertinus-Bold.ttf", Spec::new("Libertinus Serif", 700, false))];
    let out = f.write(&font_note(&[("print-font", "libertinus-serif")])).unwrap();
    assert!(out.tex.contains("\\setmainfont{Libertinus Serif}\n"), "the name as the font states it: {}", out.tex);
    assert!(out.files.is_empty(), "{:?}", out.files.iter().map(|f| &f.0).collect::<Vec<_>>());
    assert_eq!(out.fonts.len(), 1);
}

// spec: TEX-054, TEX-008
#[test]
fn tex_054_a_font_name_that_could_end_the_command_is_not_written() {
    let mut f = Fonts::new();
    for name in ["X}{x}%", "A\"B", "a#b", "a~b", "a^b", "a$b", "a&b"] {
        f.settings.text = Some(name.to_string());
        let out = f.write(&font_note(&[])).unwrap();
        assert!(!out.tex.contains("\\setmainfont") && !out.tex.contains("{x}%"), "{name}: {}", out.tex);
        assert!(out.warnings.iter().any(|w| w.starts_with("`print-font` (the vault's settings)") && w.contains("cannot be passed to TeX") && w.contains("the template's font is used")), "{name}: {:?}", out.warnings);
        assert!(out.fonts.is_empty());
    }
    // with a slash it is read as a path, and that is not a font file
    f.settings.text = Some("X}\\input{/etc/passwd}%".into());
    let out = f.write(&font_note(&[])).unwrap();
    assert!(!out.tex.contains("setmainfont") && !out.tex.contains("/etc/passwd") && out.warnings.iter().any(|w| w.contains("not a font file")), "{}: {:?}", out.tex, out.warnings);
    // the characters that stay: spaces, digits, dots, hyphens, underscores, colons and parentheses of real names
    f.settings.text = Some("Source Serif 4 (Display)-1.0_b:x".into());
    assert!(f.tex(&font_note(&[])).contains("\\setmainfont{Source Serif 4 (Display)-1.0_b:x}"));
}

// spec: TEX-055
#[test]
fn tex_055_a_family_of_the_vault_is_copied_once_and_written_with_path_and_its_faces() {
    let mut f = Fonts::new();
    f.vault = garamond_faces();
    f.vault.push(("Other-Regular.ttf", Spec::new("Other", 400, false)));
    let out = f.write(&font_note(&[("print-font", "Garamond")])).unwrap();
    assert!(out.tex.contains("\\setmainfont{Garamond-Regular.ttf}[Path=./, BoldFont=Garamond-Bold.ttf, ItalicFont=Garamond-Italic.ttf, BoldItalicFont=Garamond-BoldItalic.ttf]\n"), "{}", out.tex);
    let mut names: Vec<&str> = out.files.iter().map(|(n, _)| n.as_str()).collect();
    names.sort();
    assert_eq!(names, ["Garamond-Bold.ttf", "Garamond-BoldItalic.ttf", "Garamond-Italic.ttf", "Garamond-Regular.ttf"], "the family only, a font the document does not use is not copied");
    let regular = out.files.iter().find(|(n, _)| n == "Garamond-Regular.ttf").unwrap();
    assert_eq!(regular.1, Spec::new("Garamond", 400, false).bytes(), "the file as it is");
    assert_eq!(out.fonts[0].names, ["Garamond-Regular.ttf", "Garamond-Bold.ttf", "Garamond-Italic.ttf", "Garamond-BoldItalic.ttf"]);
    assert_eq!(out.fonts[0].label, "Garamond");
    // a face the family lacks is left out
    f.vault = vec![garamond_faces().remove(0), garamond_faces().remove(2)];
    let tex = f.tex(&font_note(&[("print-font", "Garamond")]));
    assert!(tex.contains("\\setmainfont{Garamond-Regular.ttf}[Path=./, ItalicFont=Garamond-Italic.ttf]\n"), "{tex}");
}

// spec: TEX-055
#[test]
fn tex_055_a_vault_path_gives_one_face_and_one_file_is_copied_once_for_two_roles() {
    let mut f = Fonts::new();
    f.vault = garamond_faces();
    let out = f.write(&font_note(&[("print-font", "fonts/Garamond-Bold.ttf"), ("print-font-sans", "\"[[Garamond-Bold.ttf]]\""), ("print-font-mono", "Garamond")])).unwrap();
    assert!(out.tex.contains("\\setmainfont{Garamond-Bold.ttf}[Path=./]\n") && out.tex.contains("\\setsansfont{Garamond-Bold.ttf}[Path=./]\n"), "a path names one face and no more: {}", out.tex);
    assert!(out.tex.contains("\\setmonofont{Garamond-Regular.ttf}[Path=./, BoldFont=Garamond-Bold.ttf,"), "{}", out.tex);
    let bold: Vec<_> = out.files.iter().filter(|(n, _)| n == "Garamond-Bold.ttf").collect();
    assert_eq!(bold.len(), 1, "the file the roles share is one file of the work folder");
    assert_eq!(out.files.len(), 4);
}

// spec: TEX-055
#[test]
fn tex_055_the_file_of_a_set_counts_and_a_name_that_fontspec_cannot_take_is_made_one() {
    let mut f = Fonts::new();
    f.vault = vec![("templates/tex/tuda/fonts/My Font_Regular.ttf", Spec::new("Institute Serif", 400, false)), ("templates/tex/tuda/fonts/My Font_Bold.ttf", Spec::new("Institute Serif", 700, false))];
    let out = f.write(&font_note(&[("print-font", "Institute Serif")])).unwrap();
    assert!(out.tex.contains("\\setmainfont{My-Font-Regular.ttf}[Path=./, BoldFont=My-Font-Bold.ttf]\n"), "letters, digits, . and - only: {}", out.tex);
    assert_eq!(out.files.len(), 2);
}

// spec: TEX-055
#[test]
fn tex_055_two_files_with_one_name_in_different_folders_are_two_files_and_the_emoji_font_is_not_replaced() {
    let mut f = Fonts::new();
    f.vault = vec![("fonts/a/Same.ttf", Spec::new("Aaa", 400, false)), ("fonts/b/Same.ttf", Spec::new("Bbb", 400, false)), ("fonts/shale-emoji.ttf", Spec::new("Ccc", 400, false))];
    let out = f.write(&font_note(&[("print-font", "Aaa"), ("print-font-sans", "Bbb"), ("print-font-mono", "Ccc")])).unwrap();
    assert!(out.tex.contains("\\setmainfont{Same.ttf}[Path=./]") && out.tex.contains("\\setsansfont{Same-2.ttf}[Path=./]"), "{}", out.tex);
    assert!(out.tex.contains("\\setmonofont{shale-emoji-2.ttf}[Path=./]"), "{}", out.tex);
    let a = out.files.iter().find(|(n, _)| n == "Same.ttf").unwrap();
    let b = out.files.iter().find(|(n, _)| n == "Same-2.ttf").unwrap();
    assert_eq!((a.1.clone(), b.1.clone()), (Spec::new("Aaa", 400, false).bytes(), Spec::new("Bbb", 400, false).bytes()));
}

// spec: TEX-055
#[test]
fn tex_055_a_font_that_may_not_be_embedded_is_not_written_or_copied() {
    let mut f = Fonts::new();
    f.vault = vec![("Locked.ttf", Spec::new("Locked", 400, false).licence(0x0002, 4))];
    f.settings.text = Some("Vault Text".into());
    let out = f.write(&font_note(&[("print-font", "Locked")])).unwrap();
    assert!(out.tex.contains("\\setmainfont{Vault Text}") && !out.tex.contains("Locked"), "the next entry of the precedence: {}", out.tex);
    assert!(out.files.is_empty());
    assert!(out.warnings.iter().any(|w| w.contains("Locked") && w.contains("cannot be used")), "{:?}", out.warnings);
}

// spec: TEX-056
#[test]
fn tex_056_the_emoji_face_is_written_before_the_overrides_and_its_font_travels_with_the_document() {
    let mut f = Fonts::new();
    f.vault = garamond_faces();
    let note = format!("{}\n\u{1F4C5} due\n", font_note(&[("print-font", "Garamond"), ("print-font-sans", "Fira Sans"), ("print-font-mono", "Fira Mono")]));
    let out = f.write(&note).unwrap();
    let face = position(&out.tex, "\\newfontface\\shaleemojiface[Path=./]{shale-emoji.ttf}");
    for command in ["\\setmainfont", "\\setsansfont", "\\setmonofont"] {
        assert!(face < position(&out.tex, command), "{command} after the emoji face: {}", out.tex);
    }
    assert!(out.tex.contains("\\shaleemoji{\u{1F4C5}}"), "{}", out.tex);
    assert!(out.files.iter().any(|(n, _)| n == "shale-emoji.ttf"), "the emoji font is in the work folder");
    assert_eq!(out.tex.matches("\\usepackage{fontspec}").count(), 1, "fontspec is loaded once: {}", out.tex);
    // the override does not take the face over: it is its own font file
    assert!(!out.tex.contains("{shale-emoji.ttf}[Path=./, "), "{}", out.tex);
}

// spec: TEX-055, PDF-035
#[test]
fn tex_055_an_export_started_before_the_font_scan_is_in_still_gets_the_fonts_of_the_vault() {
    use crate::fonts::scan::FontScan;
    let (v, dir) = temp_vault("early", &[("Plan.md", font_note(&[("print-font", "Garamond")]).as_bytes())]);
    for (name, spec) in garamond_faces() {
        spec.write(&dir, &format!("fonts/{name}"));
    }
    // no tick, no scan: the catalogue the window has is empty, the one for the export has the vault's fonts
    let scan = FontScan::default();
    assert!(scan.catalogue().vault.is_empty() && !scan.catalogue().system_ready);
    let input = FontInput { catalogue: scan.catalogue_for(&v), vault: Values::default() };
    let note = font_note(&[("print-font", "Garamond")]);
    let set = tuda_set(SetSettings::default());
    let out = write(&Request { vault: &v, rel: Path::new("Plan.md"), text: &note, opts: &ExportOptions::for_note(), set: &set, kind: kind(&set, "pub"), fields: &fields(&[("author", "Ada")]), fonts: Some(&input) }).unwrap();
    assert!(out.tex.contains("\\setmainfont{Garamond-Regular.ttf}[Path=./, BoldFont=Garamond-Bold.ttf"), "{}", out.tex);
    assert_eq!(out.files.len(), 4);
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    let _ = std::fs::remove_dir_all(dir);
}
