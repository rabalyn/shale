//! A folder's book through a LaTeX template (BOOK-013…BOOK-016, #309): the plan of `pdf::book`
//! (parts, chapters, reading order, titles) written as one document of a kind of the article or
//! book family. Parts and chapters are sectioning commands by their depth, the folder notes' text
//! is under their part, the top folder note's text comes before the first chapter, and the wiki
//! links between the notes of the book are `\hyperlink`s to the anchor of a chapter or heading
//! (the anchors of TEX-032, named by their place in the book). The blocks are written by the single
//! note's writer (`writer.rs`, `body.rs`, `inline.rs`).

use super::blocks::{self, Block};
use super::body::Body;
use super::fonts::{self, FontInput};
use super::inline::{anchor_named, Files, Look, Targets};
use super::sets::{class_and_preamble, tex_escape, KindManifest, Mapping, SetInfo};
use super::thesis::{self, Input, Thesis};
use super::writer::{document, drop_title_heading, field_values, heading, nested_levels, toc_depth, Document, Setup, SECTION};
use crate::board::resolve::Resolved;
use crate::fonts::resolve::Values;
use crate::markdown::labels::{self, Role as LabelRole, Source};
use crate::markdown::{self, citations};
use crate::pdf::book::{Item, Plan};
use crate::pdf::export::ExportOptions;
use crate::vault::{FileKind, Vault};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

/// Why a kind is greyed in the dialog of a folder (BOOK-013).
pub const ONLY_ARTICLES_AND_BOOKS: &str = "A folder is printed as an article or a book: slides, letters and posters take a single note";

/// Whether a kind can hold a folder: the article and the book family (BOOK-013).
pub fn holds_a_folder(kind: &KindManifest) -> bool {
    matches!(kind.mapping, Mapping::Article | Mapping::Book)
}

/// The book and the choices it is written with.
pub struct BookRequest<'a> {
    pub vault: &'a Vault,
    pub plan: &'a Plan,
    pub opts: &'a ExportOptions,
    pub set: &'a SetInfo,
    pub kind: &'a KindManifest,
    /// The values of the dialog's fields by key; an empty one is as good as none.
    pub fields: &'a BTreeMap<String, String>,
    /// What the fonts of the book are decided from (PDF-034); none: the template's own fonts.
    pub fonts: Option<&'a FontInput>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    /// The top folder's note: text before the first chapter, no heading of its own.
    Intro,
    Part,
    Chapter,
}

/// A part, a chapter or the intro, in reading order, as it is written.
struct Section {
    role: Role,
    /// The rank of its own heading in the sectioning commands (`\part` is 0); the headings of its
    /// note go below it. The intro has the rank of a chapter of the top folder.
    rank: usize,
    title: String,
    /// Its note (a part's folder note), none for a folder without one.
    rel: Option<PathBuf>,
    text: String,
    /// The note's blocks, the heading that says the title again left out (PDF-008).
    blocks: Vec<Block>,
    /// The heading that was left out, as written: it may carry the chapter's label (`{#sec:one}`).
    title_heading: Option<String>,
    anchor: String,
}

impl Section {
    /// The anchors of the note's headings, in the order of its blocks.
    fn heading_anchor(&self, n: usize) -> String {
        format!("{}-{n}", self.anchor)
    }
}

/// The sections of the plan in reading order (BOOK-014): a subfolder at depth `d` (the top
/// folder's own are 0) has the rank `d` from `\part` in a book and from `\section` in an article;
/// a note is one rank below its folder's part, and directly in the top folder a `\chapter` in a
/// book, a `\section` in an article.
fn sections(vault: &Vault, plan: &Plan, book: bool) -> Vec<Section> {
    let mut s = Sections { vault, book, out: Vec::new() };
    if let Some(rel) = &plan.intro {
        s.push(Role::Intro, s.chapter_at(0), plan.title.clone(), Some(rel.clone()));
    }
    s.walk(&plan.items, 0);
    s.out
}

/// The sections of a plan as they are found.
struct Sections<'v> {
    vault: &'v Vault,
    book: bool,
    out: Vec<Section>,
}

impl Sections<'_> {
    /// The rank of a subfolder of the top folder: `\part` in a book, `\section` in an article.
    fn top(&self) -> usize {
        if self.book {
            0
        } else {
            SECTION
        }
    }

    /// The rank of a note at this depth: one below its folder's part, at least `\chapter` in a book.
    fn chapter_at(&self, depth: usize) -> usize {
        self.top() + if self.book { depth.max(1) } else { depth }
    }

    fn push(&mut self, role: Role, rank: usize, title: String, rel: Option<PathBuf>) {
        // a drawing (a card of a board) has no text: its chapter is the drawing, embedded
        let drawing = rel.as_deref().filter(|r| self.vault.file(r).is_some_and(|f| f.kind == FileKind::Drawing));
        let text = match drawing {
            Some(r) => format!("![[{}]]", r.to_string_lossy()),
            None => rel.as_deref().and_then(|r| self.vault.notes.get(r)).map(|n| n.text.clone()).unwrap_or_default(),
        };
        let mut blocks = blocks::parse(&text);
        let title_heading = if role != Role::Intro { drop_title_heading(&mut blocks, &title, true) } else { None };
        let anchor = format!("shale-{}", self.out.len() + 1);
        self.out.push(Section { role, rank, title, rel, text, blocks, title_heading, anchor });
    }

    fn walk(&mut self, items: &[Item], depth: usize) {
        for item in items {
            match item {
                Item::Chapter(c) if c.left_out => {}
                Item::Chapter(c) => self.push(Role::Chapter, self.chapter_at(depth), c.title.clone(), Some(c.rel.clone())),
                Item::Part(p) => {
                    self.push(Role::Part, self.top() + depth, p.title.clone(), p.note.clone());
                    self.walk(&p.items, depth + 1);
                }
            }
        }
    }
}

/// Where the links of the book go (BOOK-016): every note to its section's anchor, and its headings
/// (the first of a text) to theirs.
fn targets(secs: &[Section]) -> Targets {
    let mut t = Targets::default();
    for s in secs {
        let Some(rel) = &s.rel else { continue };
        let mut heads = HashMap::new();
        for (n, b) in s.blocks.iter().filter(|b| matches!(b, Block::Heading { .. })).enumerate() {
            if let Block::Heading { text, .. } = b {
                heads.entry(markdown::heading_plain(text).to_lowercase()).or_insert_with(|| s.heading_anchor(n + 1));
            }
        }
        t.notes.insert(rel.clone(), (s.anchor.clone(), heads));
    }
    t
}

/// Write the book as one document of the kind (BOOK-014…BOOK-016). Fails with a sentence for the
/// person when the kind cannot hold a folder or a required field has no value.
pub fn write_book(req: &BookRequest) -> Result<Document, String> {
    write_with_board(req, None)
}

/// The book, with the board of BOARD-031 on landscape pages in front of the contents when it is one.
pub(super) fn write_with_board(req: &BookRequest, board: Option<&Resolved>) -> Result<Document, String> {
    // the paper has Shale's colours, whatever the theme of the window (CFG-061)
    tracing::info_span!("flow.tex_book.write", peer = "vault").in_scope(|| crate::theme::with_palette(crate::theme::Palette::shale(), || write_book_in_shale(req, board)))
}

fn write_book_in_shale(req: &BookRequest, board: Option<&Resolved>) -> Result<Document, String> {
    let (vault, plan, kind) = (req.vault, req.plan, req.kind);
    if !holds_a_folder(kind) {
        return Err(format!("“{}” cannot hold a folder. {ONLY_ARTICLES_AND_BOOKS}.", kind.name));
    }
    let book = kind.mapping == Mapping::Book;
    // the fields: the dialog, else the top folder note's front matter, else the book's title (BOOK-015)
    let front = plan.note.as_deref().and_then(|n| vault.notes.get(n)).map(|n| markdown::front_matter_values(&n.text)).unwrap_or_default();
    let values = field_values(kind, &req.set.settings, &front, &plan.title, req.fields)?;
    let secs = sections(vault, plan, book);
    let targets = targets(&secs);
    // the thesis syntax of the whole book: one bibliography (the top folder note's), the labels of all its notes (ADR-0039)
    let top_note = plan.note.as_deref().and_then(|n| vault.notes.get(n).map(|note| (n, note.text.as_str())));
    let top_text = top_note.map_or("", |(_, t)| t);
    let sources: Vec<Source> = secs.iter().map(|s| Source { text: &s.text, role: if s.role == Role::Chapter { LabelRole::of_note(&s.text) } else { LabelRole::Other }, title: Some(s.title.as_str()), number: None }).collect();
    let markers = secs.iter().flat_map(|s| s.blocks.iter()).filter_map(|b| if let Block::Marker(m) = b { Some(*m) } else { None }).collect();
    let (bib, bib_messages) = if secs.iter().any(|s| s.text.contains('@')) { thesis::load_bib(vault, top_note, &req.opts.thesis.default_bibliography) } else { (None, Vec::new()) };
    let thesis = Thesis::new(Input { top: top_text, opts: req.opts, set: req.set, kind, book: true, labels: labels::of_book(&sources, top_note.map(|(_, t)| t)), bib, bib_messages, markers });
    // the book's numbering, whatever a note's own front matter says (BOOK-006)
    let (numbered, toc) = (req.opts.numbered_headings, req.opts.toc);
    let plain = |t: &str| tex_escape(&markdown::heading_plain(&labels::without_labels(t)));
    // one list of files for all notes: the work folder's files are named by it, so they never collide
    let files = RefCell::new(Files::default());
    // the headings have the book's anchors, not those of a single note (TEX-032)
    let no_anchors = HashSet::new();
    let mut main = String::from("\\maketitle\n");
    if let Some(board) = board {
        main.push_str(&super::board::pages(board, &targets, toc.then_some(if book { 1 } else { SECTION })));
    }
    if toc {
        main.push_str("\\tableofcontents\n");
    }
    // the lists of figures and tables follow the contents (TEX-047)
    let lists = thesis.lists();
    main.push_str(&lists);
    // the top folder note's text starts a page; a chapter starts one anyway and does not make a second (BOOK-018)
    if (toc || !lists.is_empty()) && req.opts.toc_break {
        main.push_str(super::body::NEW_PAGE);
    }
    for s in &secs {
        match s.role {
            Role::Intro => main.push_str(&format!("{}\n", anchor_named(&s.anchor))),
            _ => {
                // the chapter's own label, when its title line carried one (BOOK-020)
                let label = s.title_heading.as_deref().map(|h| thesis.heading(h).1).unwrap_or_default();
                main.push_str(&heading(s.rank, numbered, toc, &plain(&s.title), &format!("{}{label}", anchor_named(&s.anchor))));
            }
        }
        let Some(rel) = &s.rel else { continue };
        let look = Look {
            vault,
            rel,
            text: &s.text,
            anchors: &no_anchors,
            embeds: req.opts.embeds,
            files: &files,
            notes: blocks::footnote_texts(&s.text),
            deferred: RefCell::new(None),
            targets: Some(&targets),
            thesis: (!citations::is_foreign(&s.text)).then_some(&thesis),
        };
        let body = Body::new(look, req.opts, true);
        // a chapter's headings are numbered and listed like it; a folder note's are neither (BOOK-014)
        let chapter = s.role == Role::Chapter;
        let levels: Vec<usize> = s.blocks.iter().filter_map(|b| if let Block::Heading { level, .. } = b { Some(*level) } else { None }).collect();
        let mut nested = nested_levels(&levels).into_iter().enumerate();
        for b in &s.blocks {
            match b {
                Block::Heading { text, .. } => {
                    let (n, level) = nested.next().unwrap_or((0, 1));
                    let mark = format!("{}{}", anchor_named(&s.heading_anchor(n + 1)), thesis.heading(text).1);
                    main.push_str(&heading(s.rank + level, numbered && chapter, toc && chapter, &plain(text), &mark));
                }
                b => main.push_str(&body.block(b)),
            }
        }
    }
    main.push_str(&thesis.bibliography_at_end());
    let mut head = class_and_preamble(kind, &req.set.settings, kind.shows_department_logo(req.set, req.opts.department_logo));
    if board.is_some() {
        // the board is on landscape pages of its own (BOARD-031)
        head.push_str(super::board::PREAMBLE);
    }
    // one font for the book: the top folder note's, else the set's and the vault's; a chapter's own is ignored (BOOK-022)
    let top = plan.note.as_deref().and_then(|n| vault.notes.get(n)).map(|n| Values::of_note(&n.text)).unwrap_or_default();
    let top = req.opts.font_note.clone().unwrap_or(top);
    let chapters = secs.iter().filter(|s| s.role != Role::Intro && s.rel.is_some()).map(|s| (s.title.clone(), Values::of_note(&s.text))).collect();
    let font_lines = fonts::overrides(req.fonts, fonts::Source { vault, set: req.set, note: top, book: true, chapters });
    let setup = Setup { fonts: &font_lines.fonts, thesis: thesis.preamble() };
    let tex = document(head, true, false, toc.then(|| toc_depth(book)), &values, &setup, &main);
    let mut files = files.into_inner();
    files.list.extend(thesis.bib_files());
    files.warnings.extend(thesis.warnings());
    let notes: Vec<crate::thesis::book::Note> = secs.iter().map(|s| crate::thesis::book::Note { name: &s.title, text: &s.text, top: s.role == Role::Intro }).collect();
    files.warnings.extend(thesis.book_warnings(&notes));
    Ok(Document::new(tex, files, font_lines))
}
