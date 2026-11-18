//! The Markdown to LaTeX writer (TEX-008 to TEX-015): a note and a document kind of a template
//! set in, one `main.tex` and the files it needs out. The kind's manifest (`sets.rs`) says the
//! class, its options, the preamble and the fields; the writer fills the fields from the note's
//! properties and the dialog, sets the headings by the kind's mapping and writes the body block
//! by block (`body.rs`). The front matter is never printed.

use super::blocks::{self, Block};
use super::body::{Body, NEW_PAGE};
use super::inline::{anchor, Files, Look};
use super::sets::{class_and_preamble, tex_escape, Field, KindManifest, Mapping, SetInfo, SetSettings};
use crate::markdown;
use crate::pdf::export::ExportOptions;
use crate::vault::Vault;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

/// What the writer makes: the text of `main.tex`, the files next to it, and what it could not do
/// without saying so (an embed that is missing).
#[derive(Debug, Default)]
pub struct Document {
    pub tex: String,
    pub files: Vec<(String, Vec<u8>)>,
    pub warnings: Vec<String>,
}

/// The note and the choices an export is made from.
pub struct Request<'a> {
    pub vault: &'a Vault,
    pub rel: &'a Path,
    pub text: &'a str,
    pub opts: &'a ExportOptions,
    pub set: &'a SetInfo,
    pub kind: &'a KindManifest,
    /// The values of the dialog's fields by key; an empty one is as good as none.
    pub fields: &'a BTreeMap<String, String>,
}

/// The title of a note: its name without `.md` and without a date prefix (VAULT-025).
fn note_title(vault: &Vault, rel: &Path) -> String {
    vault.file(rel).map(|f| f.title()).unwrap_or_else(|| crate::vault::title_of(&rel.to_string_lossy()).to_string())
}

/// What separates the lines of a field's value: `\\` for an address, `\and` for authors.
fn joiner(f: &Field) -> &'static str {
    if f.komavar.is_some() || f.command.is_none() {
        "\\\\"
    } else {
        "\\and"
    }
}

/// A value for TeX: split at the separator the field has, each part escaped as text, put
/// together again. Nothing else of the value is ever read as TeX.
fn tex_value(f: &Field, raw: &str) -> String {
    let j = joiner(f);
    raw.split(j).map(|p| tex_escape(p.trim())).collect::<Vec<_>>().join(&format!(" {j} "))
}

/// The value the note's front matter has for a field: the values of its key (and the key
/// `title` for the title), joined by the field's separator.
fn front_value(fm: &[(String, Vec<String>)], f: &Field) -> Option<String> {
    let values = fm.iter().find(|(k, _)| *k == f.key.to_lowercase())?;
    let joined = values.1.iter().map(|v| v.trim()).filter(|v| !v.is_empty()).collect::<Vec<_>>().join(&format!(" {} ", joiner(f)));
    (!joined.is_empty()).then_some(joined)
}

/// What the set's settings give a field when nothing else does: the author and the department
/// the guided setup asked for (TEX-017, TEX-018).
pub fn settings_default(s: &SetSettings, key: &str) -> Option<String> {
    let v = match key {
        "author" => &s.author,
        "department" => &s.department,
        _ => return None,
    };
    (!v.trim().is_empty()).then(|| v.trim().to_string())
}

/// What the dialog shows in the fields of a kind before the person changes anything: the
/// note's properties, and for `title` the note's title.
pub fn prefill(title: &str, props: &[(String, Vec<String>)], kind: &KindManifest) -> Vec<(String, String)> {
    kind.fields.iter().map(|f| (f.key.clone(), front_value(props, f).unwrap_or_else(|| if f.key == "title" { title.to_string() } else { String::new() }))).collect()
}

/// How a kind's headings are written.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Family {
    Article,
    Book,
    Beamer,
    Letter,
    /// Text in the poster class, headings without numbers.
    Poster,
    /// Boxes of a tcolorbox poster.
    Boxes,
}

fn family_of(k: &KindManifest) -> Family {
    match k.mapping {
        Mapping::Article => Family::Article,
        Mapping::Book => Family::Book,
        Mapping::Beamer => Family::Beamer,
        Mapping::Letter => Family::Letter,
        Mapping::Poster if k.class.contains("sciposter") => Family::Boxes,
        Mapping::Poster => Family::Poster,
    }
}

/// The level each heading has by nesting from `#`: a heading below a shallower one is one level
/// down from it, however many marks it has, and the first heading is level 1.
pub(super) fn nested_levels(levels: &[usize]) -> Vec<usize> {
    let mut open: Vec<usize> = Vec::new();
    levels
        .iter()
        .map(|&l| {
            while open.last().is_some_and(|&o| o >= l) {
                open.pop();
            }
            open.push(l);
            open.len()
        })
        .collect()
}

/// The sectioning commands from the top: a rank is an index into it (deeper ones repeat the last).
const COMMANDS: [&str; 7] = ["part", "chapter", "section", "subsection", "subsubsection", "paragraph", "subparagraph"];

/// The rank of `\section` in [`COMMANDS`]: where an article starts, and the book family one above it.
pub(super) const SECTION: usize = 2;

/// The rank of a heading level of a note: level 1 is `\chapter` in a book, `\section` in an article.
fn rank_of(book: bool, level: usize) -> usize {
    level + usize::from(!book)
}

/// A heading of the article or book family at a rank of [`COMMANDS`]: numbered, or starred and
/// (when the table of contents is there) put into it by hand so it is still listed (TEX-009);
/// `mark` is its anchor (TEX-032, a book's BOOK-016).
pub(super) fn heading(rank: usize, numbered: bool, listed: bool, plain: &str, mark: &str) -> String {
    let name = COMMANDS[rank.min(COMMANDS.len() - 1)];
    // paragraphs have no star: the class does not number them anyway
    if numbered || name.ends_with("paragraph") {
        return format!("\\{name}{{{plain}}}{mark}\n\n");
    }
    let add = if listed { format!("\\addcontentsline{{toc}}{{{name}}}{{{plain}}}\n") } else { String::new() };
    format!("\\{name}*{{{plain}}}{mark}\n{add}\n")
}

/// A heading as a bold line with its anchor (TEX-032), for the places a kind has no headings of its own.
fn bold_line(plain: &str, mark: &str) -> String {
    format!("\\par\\medskip\\noindent{mark}\\textbf{{{plain}}}\\par\\smallskip\n\n")
}

/// The helpers every document has: the packages the body needs, the box of a task, the block of code.
fn helpers(long_tables: bool, columns: bool) -> String {
    let mut t = String::from("\\usepackage{graphicx}\n\\usepackage{xcolor}\n\\usepackage[normalem]{ulem}\n\\usepackage{booktabs}\n\\usepackage{tabularx}\n");
    // math (TEX-029): amsmath for matrices, cases, \\dfrac and \\binom, the symbols and \\mathbb of amssymb
    // unless unicode-math (the template's) has them
    t.push_str("\\IfPackageLoadedTF{amsmath}{}{\\usepackage{amsmath}}\n\\IfPackageLoadedTF{unicode-math}{}{\\IfPackageLoadedTF{amssymb}{}{\\usepackage{amssymb}}}\n");
    if long_tables {
        t.push_str("\\usepackage{xltabular}\n");
    }
    if columns {
        t.push_str("\\IfPackageLoadedTF{multicol}{}{\\usepackage{multicol}}\n");
    }
    t.push_str("\\IfPackageLoadedTF{hyperref}{}{\\usepackage{hyperref}}\n\\hypersetup{bookmarksdepth=5}\n");
    // the anchor of a heading (TEX-032): a named target where it is, raised to the line above as
    // `\phantomsection` does, the same in every class (a `\label` would name it by the class)
    t.push_str("\\makeatletter\\newcommand{\\shaleanchor}[1]{\\Hy@raisedlink{\\hyper@anchorstart{#1}\\hyper@anchorend}}\\makeatother\n");
    t.push_str("\\newcommand{\\shaletask}[1]{\\leavevmode{\\setlength{\\fboxsep}{1pt}\\setlength{\\fboxrule}{0.4pt}\\fbox{\\makebox[0.7em][c]{\\rule{0pt}{0.7em}#1}}}}\n");
    // fenced code on a light box (TEX-030): one that breaks across pages where tables may
    let c = super::body::CODE_PAPER;
    t.push_str(&format!("\\usepackage{{tcolorbox}}\n\\definecolor{{shalecodebg}}{{RGB}}{{{},{},{}}}\n", c.r(), c.g(), c.b()));
    if long_tables {
        t.push_str("\\tcbuselibrary{breakable}\n");
    }
    t.push_str(&format!(
        "\\newtcolorbox{{shalecode}}{{{}colback=shalecodebg,colframe=shalecodebg,boxrule=0pt,arc=1pt,left=0.6em,right=0.6em,top=0.4em,bottom=0.4em,before skip=\\medskipamount,after skip=\\medskipamount,before upper={{\\ttfamily\\small\\raggedright\\setlength{{\\parindent}}{{0pt}}\\setlength{{\\parskip}}{{0pt}}\\hyphenchar\\font=-1 }}}}\n",
        if long_tables { "breakable," } else { "" }
    ));
    t
}

/// The values of a kind's fields (TEX-018): the dialog's, else the front matter's (`fm`), else the
/// `title`, else the set's settings, else the manifest's default. Fails with a sentence for the
/// person when a required field has no value.
pub(super) fn field_values<'k>(kind: &'k KindManifest, settings: &SetSettings, fm: &[(String, Vec<String>)], title: &str, dialog: &BTreeMap<String, String>) -> Result<Vec<(&'k Field, Option<String>)>, String> {
    let value = |f: &Field| -> Option<String> {
        let given = dialog.get(&f.key).map(|v| v.trim()).filter(|v| !v.is_empty()).map(str::to_string);
        let found = given
            .or_else(|| front_value(fm, f))
            .or_else(|| (f.key == "title").then(|| title.to_string()))
            .or_else(|| (f.key == "subject" && kind.fields.iter().all(|o| o.key != "title")).then(|| title.to_string()))
            .or_else(|| settings_default(settings, &f.key));
        found.map(|v| tex_value(f, &v)).or_else(|| (!f.default.is_empty()).then(|| f.default.clone()))
    };
    let mut values = Vec::new();
    for f in &kind.fields {
        let v = value(f);
        if f.required && v.is_none() {
            let what = if f.label.is_empty() { f.key.as_str() } else { f.label.as_str() };
            return Err(format!("“{what}” is empty: fill it in the export dialog or add `{}:` to the note's properties", f.key));
        }
        values.push((f, v));
    }
    Ok(values)
}

/// Leave out the heading that only says the title again (PDF-008): the first heading, when it is a
/// `#` with the title's text.
pub(super) fn drop_title_heading(blocks: &mut Vec<Block>, title: &str) {
    let first_heading = blocks.iter().position(|b| matches!(b, Block::Heading { .. }));
    if let Some(i) = first_heading {
        if let Block::Heading { level: 1, text, .. } = &blocks[i] {
            if markdown::heading_plain(text).to_lowercase() == markdown::heading_plain(title).to_lowercase() {
                blocks.remove(i);
            }
        }
    }
}

/// The whole document around `main` (what is between `\begin{document}` and `\end{document}`): the
/// kind's lines, the writer's helpers, the depth of the contents when there are any, the fields.
pub(super) fn document(kind: &KindManifest, settings: &SetSettings, long_tables: bool, columns: bool, toc_depth: Option<usize>, values: &[(&Field, Option<String>)], main: &str) -> String {
    let mut tex = class_and_preamble(kind, settings);
    tex.push_str(&helpers(long_tables, columns));
    if let Some(depth) = toc_depth {
        tex.push_str(&format!("\\setcounter{{tocdepth}}{{{depth}}}\n"));
    }
    for (f, v) in values {
        // no \date: the class puts in today's
        if let (Some(c), Some(v)) = (&f.command, v) {
            tex.push_str(&format!("\\{c}{{{v}}}\n"));
        }
    }
    tex.push_str("\\begin{document}\n");
    tex.push_str(main);
    tex.push_str("\\end{document}\n");
    tex
}

/// The depth of the table of contents (TEX-009, TEX-011): `#` to `###` of an article, `#` to `##`
/// below the chapters of a book.
pub(super) fn toc_depth(book: bool) -> usize {
    if book {
        2
    } else {
        3
    }
}

/// Write the note as a document of the kind. Fails with a sentence for the person when a required
/// field has no value.
pub fn write(req: &Request) -> Result<Document, String> {
    let (vault, kind) = (req.vault, req.kind);
    let family = family_of(kind);
    let fm = markdown::front_matter_values(req.text);
    let title = note_title(vault, req.rel);
    let values = field_values(kind, &req.set.settings, &fm, &title, req.fields)?;
    let get = |key: &str| values.iter().find(|(f, _)| f.key == key).and_then(|(_, v)| v.clone());

    // the blocks, without the heading that only says the title again (PDF-008)
    let mut blocks = blocks::parse(req.text);
    drop_title_heading(&mut blocks, &title);
    // a page break means nothing in slides and boxes, where the frames and boxes are the pages (TEX-035)
    if matches!(family, Family::Beamer | Family::Boxes) {
        blocks.retain(|b| *b != Block::PageBreak);
    }
    let numbered = markdown::headings_numbered(req.text, req.opts.numbered_headings);
    let has_headings = blocks.iter().any(|b| matches!(b, Block::Heading { .. }));
    let toc = req.opts.toc && matches!(family, Family::Article | Family::Book) && has_headings;
    let long_tables = matches!(family, Family::Article | Family::Book | Family::Letter) || (family == Family::Poster && kind.columns <= 1);
    // every heading that is written has an anchor, so a link to it can go there (TEX-032)
    let anchors: HashSet<usize> = blocks.iter().filter_map(|b| if let Block::Heading { line, .. } = b { Some(*line) } else { None }).collect();
    let files = RefCell::new(Files::default());
    let look = Look { vault, rel: req.rel, text: req.text, anchors: &anchors, embeds: req.opts.embeds, files: &files, notes: blocks::footnote_texts(req.text), deferred: RefCell::new(None), targets: None };
    let body = Body::new(look, req.opts, long_tables);
    let plain = |t: &str| tex_escape(&markdown::heading_plain(t));

    // everything between \begin{document} and \end{document}
    let mut main = String::new();
    match family {
        Family::Article | Family::Book | Family::Poster => {
            let levels: Vec<usize> = blocks.iter().filter_map(|b| if let Block::Heading { level, .. } = b { Some(*level) } else { None }).collect();
            let mut nested = nested_levels(&levels).into_iter();
            main.push_str("\\maketitle\n");
            if toc {
                main.push_str("\\tableofcontents\n");
            }
            if family == Family::Poster && kind.columns > 1 {
                main.push_str(&format!("\\begin{{multicols}}{{{}}}\n", kind.columns));
            }
            for (n, b) in blocks.iter().enumerate() {
                // a heading of the level asked for starts a page, but not the first thing of the note (TEX-036)
                let breaks = matches!(b, Block::Heading { level, .. } if family != Family::Poster && (1..=req.opts.break_before as usize).contains(level)) && n > 0;
                if (breaks || *b == Block::PageBreak) && !main.ends_with(NEW_PAGE) {
                    main.push_str(NEW_PAGE);
                }
                match b {
                    Block::PageBreak => {}
                    Block::Heading { text, line, .. } => {
                        let level = nested.next().unwrap_or(1);
                        main.push_str(&match family {
                            Family::Poster => heading(rank_of(false, level), false, false, &plain(text), &anchor(*line)),
                            f => heading(rank_of(f == Family::Book, level), numbered, toc, &plain(text), &anchor(*line)),
                        });
                    }
                    b => main.push_str(&body.block(b)),
                }
            }
            if family == Family::Poster && kind.columns > 1 {
                main.push_str("\\end{multicols}\n");
            }
        }
        Family::Beamer => main.push_str(&beamer(&blocks, &body, &plain)),
        Family::Boxes => main.push_str(&boxes(&blocks, &body, &plain)),
        Family::Letter => {
            let recipient = get("recipient").unwrap_or_default();
            main.push_str(&format!("\\begin{{letter}}{{{recipient}}}\n"));
            for (f, v) in &values {
                if let (Some(var), Some(v)) = (&f.komavar, v) {
                    main.push_str(&format!("\\setkomavar{{{var}}}{{{v}}}\n"));
                }
            }
            main.push_str(&format!("\\opening{{{}}}\n", get("opening").unwrap_or_default()));
            for b in &blocks {
                match b {
                    Block::Heading { text, line, .. } => main.push_str(&bold_line(&plain(text), &anchor(*line))),
                    b => main.push_str(&body.block(b)),
                }
            }
            main.push_str(&format!("\\closing{{{}}}\n\\end{{letter}}\n", get("closing").unwrap_or_default()));
        }
    }

    let tex = document(kind, &req.set.settings, long_tables, family == Family::Poster && kind.columns > 1, toc.then(|| toc_depth(family == Family::Book)), &values, &main);
    drop(body);
    let files = files.into_inner();
    Ok(Document { tex, files: files.list, warnings: files.warnings })
}

/// Slides: `#` is a section for the navigation and `##` a frame; deeper headings are bold lines
/// in the frame. A note with no `##` has a frame for every `#` (TEX-012). Content before the
/// first heading, and the text between a `#` and its first `##`, goes into a frame of its own.
fn beamer(blocks: &[Block], body: &Body, plain: &dyn Fn(&str) -> String) -> String {
    let frames_are_h2 = blocks.iter().any(|b| matches!(b, Block::Heading { level: 2, .. }));
    let frame_level = if frames_are_h2 { 2 } else { 1 };
    let mut out = String::from("\\maketitle\n");
    let mut open = false;
    // the title of a `#` section, for the frame its text (if it has any) is put into
    let mut section_title: Option<String> = None;
    // the anchors of the headings above the next frame (a `#` section is not on a page of its own):
    // they are the first thing in it (TEX-032)
    let mut marks = String::new();
    let open_frame = |out: &mut String, open: &mut bool, title: Option<String>, marks: &mut String| {
        out.push_str(&format!("\\begin{{frame}}[allowframebreaks]{{{}}}\n", title.unwrap_or_default()));
        if !marks.is_empty() {
            out.push_str(&format!("{}\n", std::mem::take(marks)));
        }
        *open = true;
    };
    let close = |out: &mut String, open: &mut bool| {
        if *open {
            out.push_str("\\end{frame}\n\n");
            *open = false;
        }
    };
    for b in blocks {
        match b {
            Block::Heading { level: 1, text, line } if frames_are_h2 => {
                close(&mut out, &mut open);
                out.push_str(&format!("\\section{{{}}}\n\n", plain(text)));
                section_title = Some(plain(text));
                marks.push_str(&anchor(*line));
            }
            Block::Heading { level, text, line } if *level <= frame_level => {
                close(&mut out, &mut open);
                section_title = None;
                marks.push_str(&anchor(*line));
                open_frame(&mut out, &mut open, Some(plain(text)), &mut marks);
            }
            Block::Heading { text, line, .. } => {
                if !open {
                    open_frame(&mut out, &mut open, section_title.take(), &mut marks);
                }
                out.push_str(&bold_line(&plain(text), &anchor(*line)));
            }
            b => {
                if !open {
                    open_frame(&mut out, &mut open, section_title.take(), &mut marks);
                }
                out.push_str(&body.block(b));
            }
        }
    }
    close(&mut out, &mut open);
    out
}

/// A poster of boxes: every top level heading starts a box with its title, and what is below it,
/// deeper headings as bold lines included, is the box's text; the boxes are in two columns.
fn boxes(blocks: &[Block], body: &Body, plain: &dyn Fn(&str) -> String) -> String {
    let top = blocks.iter().filter_map(|b| if let Block::Heading { level, .. } = b { Some(*level) } else { None }).min().unwrap_or(1);
    let mut cards: Vec<(Option<String>, String)> = Vec::new();
    for b in blocks {
        match b {
            // the box's anchor is the first thing in it (TEX-032)
            Block::Heading { level, text, line } if *level == top => cards.push((Some(plain(text)), format!("{}\n", anchor(*line)))),
            Block::Heading { text, line, .. } => {
                if cards.is_empty() {
                    cards.push((None, String::new()));
                }
                if let Some(c) = cards.last_mut() {
                    c.1.push_str(&bold_line(&plain(text), &anchor(*line)));
                }
            }
            b => {
                if cards.is_empty() {
                    cards.push((None, String::new()));
                }
                let t = body.block(b);
                if let Some(c) = cards.last_mut() {
                    c.1.push_str(&t);
                }
            }
        }
    }
    if cards.is_empty() {
        cards.push((None, String::new()));
    }
    let columns = cards.len().min(2);
    let rows = cards.len().div_ceil(columns);
    let mut out = format!("\\maketitle\n\\begin{{tcbposter}}[poster={{columns={columns},rows={rows},spacing=1cm}}]\n");
    for (i, (title, text)) in cards.iter().enumerate() {
        let title = title.as_ref().map(|t| format!("title={{{t}}}")).unwrap_or_default();
        out.push_str(&format!("\\begin{{posterboxenv}}[{title}]{{name=box{},column={},row={}}}\n{text}\\end{{posterboxenv}}\n\n", i + 1, i % columns + 1, i / columns + 1));
    }
    out.push_str("\\end{tcbposter}\n");
    out
}
