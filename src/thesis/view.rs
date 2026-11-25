//! The thesis document of a note or a book as the editor and the reading view see it (ADR-0039,
//! #646): the numbers of its labels (EDIT-128), the bibliography in effect (EDIT-123), the
//! language, the style, where the captions stand, and the citations as the style formats them
//! (EDIT-127). [`View::build`] makes it from texts, so the built-in PDF and the site (#648) can
//! take the same.

use super::Lang;
use crate::bibliography::format::{self, Reference, Style};
use crate::bibliography::Effect;
use crate::markdown::citations::{self, Group, Kind, Target, Token};
use crate::markdown::labels::Entry;
use crate::markdown::labels::{self, Labels, Role, Source};
use crate::pdf::book as plan;
use crate::vault::Vault;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Where the caption of figures and tables stands (ADR-0039 §3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionAt {
    Above,
    Below,
}

/// The caption positions of a document: the properties `figure-caption:` and `table-caption:`
/// (`above` or `below`) of the note or the top folder note, else tables above and figures below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Captions {
    pub figure: CaptionAt,
    pub table: CaptionAt,
}

impl Captions {
    pub fn of(top: &str) -> Captions {
        let at = |key: &str, default: CaptionAt| match crate::markdown::front_matter_values(top).into_iter().find(|(k, _)| k == key).and_then(|(_, v)| v.first().map(|s| s.trim().to_lowercase())).as_deref() {
            Some("above" | "top") => CaptionAt::Above,
            Some("below" | "bottom") => CaptionAt::Below,
            _ => default,
        };
        Captions { figure: at("figure-caption", CaptionAt::Below), table: at("table-caption", CaptionAt::Above) }
    }
}

/// The notes of a thesis book in reading order, found by [`Book::of`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Book {
    pub dir: PathBuf,
    /// The top folder note: its properties are the book's (`bibliography:`, `float-numbering:`, …).
    pub top: PathBuf,
    /// every printed note; the flag says it is a chapter (not the introduction or the note of a part)
    pub notes: Vec<(PathBuf, bool)>,
}

/// The properties of the top folder note that make a folder a thesis book.
const BOOK_PROPERTIES: [&str; 3] = ["bibliography", "float-numbering", "citation-style"];

impl Book {
    /// The book that `rel` is a note of: the highest folder above it whose folder note has a
    /// property of a thesis (`bibliography:`, `float-numbering:`, `citation-style:`) and that
    /// prints `rel` (BOOK-001). `None` for a note that stands alone. `text` gives the text of a note.
    pub fn of(vault: &Vault, rel: &Path, text: &dyn Fn(&Path) -> String) -> Option<Book> {
        let mut dirs: Vec<&Path> = rel.ancestors().skip(1).filter(|d| !d.as_os_str().is_empty()).collect();
        dirs.reverse();
        dirs.into_iter().find_map(|dir| {
            let book = plan::plan(vault, dir).ok()?;
            let top = book.note.clone()?;
            let has = crate::markdown::front_matter_values(&text(&top)).iter().any(|(k, _)| BOOK_PROPERTIES.contains(&k.as_str()));
            let notes = Book::reading_order(&book);
            (has && notes.iter().any(|(n, _)| n == rel)).then(|| Book { dir: dir.to_path_buf(), top, notes })
        })
    }

    /// The printed notes in reading order, each with whether it is a chapter.
    fn reading_order(book: &plan::Plan) -> Vec<(PathBuf, bool)> {
        fn walk(items: &[plan::Item], out: &mut Vec<(PathBuf, bool)>) {
            for item in items {
                match item {
                    plan::Item::Chapter(c) if !c.left_out => out.push((c.rel.clone(), true)),
                    plan::Item::Chapter(_) => {}
                    plan::Item::Part(p) => {
                        out.extend(p.note.clone().map(|n| (n, false)));
                        walk(&p.items, out);
                    }
                }
            }
        }
        let mut out: Vec<(PathBuf, bool)> = book.intro.iter().map(|n| (n.clone(), false)).collect();
        walk(&book.items, &mut out);
        out
    }
}

/// The citations of the open note as the style formats them, and the entries cited in the document.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Formatted {
    /// the bytes of each group of the open note and its text; a group with no key of the bibliography is not in the list
    pub groups: Vec<(Range<usize>, String)>,
    /// the entries cited in the document, in the order of the style
    pub references: Vec<Reference>,
}

/// What the editor and the reading view need to know about the document of the open note.
#[derive(Clone, Debug)]
pub struct View {
    /// the book, with the open note at `index` of its notes
    pub book: Option<Arc<Book>>,
    pub index: usize,
    pub labels: Labels,
    pub lang: Lang,
    pub captions: Captions,
    /// the bibliography in effect
    pub bib: Effect,
    /// Set when the view was built for the reading view: the citations formatted.
    pub formatted: Option<Formatted>,
}

impl Default for View {
    fn default() -> Self {
        View { book: None, index: 0, labels: labels::of_note(""), lang: Lang::En, captions: Captions::of(""), bib: Effect::default(), formatted: None }
    }
}

impl View {
    /// The view of a note, or of the book it is a note of: `texts` are the texts of its notes in
    /// reading order (the one note when it stands alone) and `index` the open one; `top` is the text
    /// of the top folder note (the open note itself when it stands alone). `format` asks for the
    /// citations to be formatted too (the reading view).
    pub fn build(book: Option<Arc<Book>>, texts: &[&str], index: usize, top: &str, bib: Effect, format: bool) -> View {
        let labels = match &book {
            Some(b) => {
                // a chapter's title for BOOK-006's rule that a first `# Title` says it
                let titles: Vec<String> = b.notes.iter().zip(texts).map(|((rel, _), text)| plan::property(text, "title").unwrap_or_else(|| crate::vault::title_of(&rel.to_string_lossy()).to_string())).collect();
                let sources: Vec<Source> = texts.iter().zip(&b.notes).zip(&titles).map(|((text, (_, chapter)), title)| Source { text, role: if *chapter { Role::of_note(text) } else { Role::Other }, title: Some(title.as_str()) }).collect();
                labels::of_book(&sources, Some(top))
            }
            None => labels::of_note(texts.first().copied().unwrap_or("")),
        };
        let lang = Lang::property(top).unwrap_or_default();
        let style = Style::property(top).unwrap_or_default();
        let formatted = format.then(|| format_document(texts, index, &bib, style, lang));
        View { book, index, labels, lang, captions: Captions::of(top), bib, formatted }
    }

    /// Whether a bibliography is in effect.
    pub fn has_bibliography(&self) -> bool {
        self.bib.in_effect()
    }

    /// The text of a reference group `[@fig:a; @fig:b]`; `None` while one of its labels does not exist.
    pub fn reference_text(&self, group: &Group) -> Option<String> {
        let mut items = Vec::new();
        for item in &group.items {
            let Target::Reference { kind, id } = &item.target else { continue };
            if !kind.is_labelled() {
                return None;
            }
            items.push((self.labels.get(*kind, id)?, item.suppress_author));
        }
        (!items.is_empty()).then(|| super::group(&items, self.lang))
    }
}

/// What stands for a span of the note text in a document that is not edited (the reading view,
/// the built-in PDF, the site): the thesis syntax of ADR-0039 §1 to §4, resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Shown {
    /// `{#fig:x}`: it is no text
    Label,
    /// a citation group: the text the style gives it, the entries it cites as the style writes them (one to a line) and their keys
    Citation { text: String, tip: String, keys: Vec<String> },
    /// a reference group: the word and number, the captions it names and the labels
    Reference { text: String, tip: String, targets: Vec<(Kind, String)> },
    /// a group that does not resolve stays as written; the tip says why
    Broken { tip: String },
}

/// A span of the note text and what stands for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub shown: Shown,
}

/// The lists a document prints where a marker line (`<!-- shale: bibliography -->`) stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Bibliography,
    ListOfFigures,
    ListOfTables,
}

impl Marker {
    /// The marker a trimmed line is, if it is one.
    pub fn of(trimmed: &str) -> Option<Marker> {
        let inner = trimmed.strip_prefix("<!--")?.strip_suffix("-->")?;
        match inner.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["shale:", "bibliography"] => Some(Marker::Bibliography),
            ["shale:", "list-of-figures"] => Some(Marker::ListOfFigures),
            ["shale:", "list-of-tables"] => Some(Marker::ListOfTables),
            _ => None,
        }
    }
}

/// A list the document prints at `line` (the marker's line; the number of lines for the end of the note).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub line: usize,
    pub what: Marker,
}

impl View {
    /// The spans of the citations, references and labels of `text` (the open note) with what stands
    /// for each, in the order of the text. A group that has nothing to show (a citation without a
    /// bibliography) is no span.
    pub fn spans(&self, text: &str) -> Vec<Span> {
        let has = |k: &str| self.bib.has_key(k);
        let formatted = self.formatted.as_ref();
        let mut out = Vec::new();
        for token in citations::scan(text, self.bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool)) {
            match token {
                Token::Label(l) => out.push(Span { range: l.range, shown: Shown::Label }),
                Token::Group(g) => {
                    let range = g.range.clone();
                    if g.items.iter().any(|i| matches!(i.target, Target::Citation(_))) {
                        if let Some(shown) = formatted.and_then(|f| f.groups.iter().find(|(r, _)| *r == range)).map(|(_, t)| t.clone()) {
                            let keys = g.items.iter().filter_map(|i| if let Target::Citation(k) = &i.target { Some(k.clone()) } else { None }).collect();
                            out.push(Span { range, shown: Shown::Citation { text: shown, tip: self.entries_of(&g), keys } });
                        } else if let Some(Target::Citation(key)) = g.items.iter().find(|i| i.known == Some(false)).map(|i| &i.target) {
                            out.push(Span { range, shown: Shown::Broken { tip: format!("“{key}” is not in the bibliography") } });
                        }
                    } else if g.items.iter().any(|i| matches!(&i.target, Target::Reference { kind, .. } if kind.is_labelled())) {
                        match self.reference_text(&g) {
                            Some(shown) => {
                                let targets = g.items.iter().filter_map(|i| if let Target::Reference { kind, id } = &i.target { Some((*kind, id.clone())) } else { None }).collect();
                                out.push(Span { range, shown: Shown::Reference { text: shown, tip: self.captions_of(&g), targets } });
                            }
                            None => {
                                let missing = g.items.iter().find_map(|i| match &i.target {
                                    Target::Reference { kind, id } if kind.is_labelled() && self.labels.get(*kind, id).is_none() => Some(format!("There is no label {{#{}:{id}}} in the document", kind.prefix())),
                                    _ => None,
                                });
                                out.push(Span { range, shown: Shown::Broken { tip: missing.unwrap_or_default() } });
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// The entries a citation group cites, as the style writes them, one to a line.
    fn entries_of(&self, g: &Group) -> String {
        let Some(f) = &self.formatted else { return String::new() };
        g.items
            .iter()
            .filter_map(|i| match &i.target {
                Target::Citation(key) => f.references.iter().find(|r| r.key == *key).map(|r| format!("{}{}", r.label.as_deref().map(|l| format!("{l} ")).unwrap_or_default(), r.text)),
                Target::Reference { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The captions of the labels a group of references names, one to a line.
    fn captions_of(&self, g: &Group) -> String {
        g.items
            .iter()
            .filter_map(|i| match &i.target {
                Target::Reference { kind, id } => self.labels.get(*kind, id).filter(|e| !e.caption.is_empty()).map(|e| e.caption.clone()),
                Target::Citation(_) => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The label of `kind` that ends the line `line` of the open note (a figure's embed has it after
    /// it, a table's caption line has it), if the document has it.
    pub fn label_on(&self, kind: Kind, line: usize) -> Option<&Entry> {
        self.labels.entries.iter().find(|e| e.kind == kind && e.source == self.index && e.line == line)
    }

    /// The number of the display equation on `lines`, in brackets, as it stands at the right margin.
    pub fn equation_number(&self, lines: &Range<usize>) -> Option<String> {
        self.labels.entries.iter().find(|e| e.kind == Kind::Equation && e.source == self.index && lines.contains(&e.line)).and_then(|e| e.number.as_ref()).map(|n| format!("({n})"))
    }

    /// The line of text a figure's or table's caption is: "Figure 3: Results of run 2".
    pub fn caption_text(&self, entry: &Entry) -> String {
        let label = super::caption_label(entry, self.lang);
        if entry.caption.is_empty() {
            label
        } else {
            format!("{label}: {}", entry.caption)
        }
    }

    /// Whether the captions of `kind` (figures or tables) stand above (the properties `figure-caption:` and `table-caption:`).
    pub fn caption_above(&self, kind: Kind) -> bool {
        (if kind == Kind::Table { self.captions.table } else { self.captions.figure }) == CaptionAt::Above
    }

    /// The labelled figures or tables of the open note, in the order of the text.
    pub fn floats(&self, kind: Kind) -> Vec<&Entry> {
        self.labels.entries.iter().filter(|e| e.kind == kind && e.source == self.index).collect()
    }

    /// The entries cited in the document, as the style writes them; empty when none is.
    pub fn references(&self) -> &[Reference] {
        self.formatted.as_ref().map_or(&[], |f| &f.references)
    }

    /// Where the document prints its lists: the bibliography (at its marker, else at the end of
    /// `text`, given as the number of its lines), the list of figures and the list of tables (at
    /// their markers). A list with nothing in it is left out. In a book the lists belong to the
    /// book, so a note has none.
    pub fn blocks(&self, text: &str) -> Vec<Block> {
        if self.book.is_some() {
            return Vec::new();
        }
        let lines = crate::markdown::lines_with_offsets(text);
        let fenced = crate::markdown::fenced_code_lines(text);
        let front = crate::markdown::front_matter_lines(text);
        let has = |m: Marker| match m {
            Marker::Bibliography => !self.references().is_empty(),
            Marker::ListOfFigures => !self.floats(Kind::Figure).is_empty(),
            Marker::ListOfTables => !self.floats(Kind::Table).is_empty(),
        };
        let marked = |n: usize, l: &str| Marker::of(l.trim()).filter(|_| n >= front && !fenced.get(n).copied().unwrap_or(false));
        let mut out: Vec<Block> = lines.iter().enumerate().filter_map(|(n, (_, l))| marked(n, l).filter(|m| has(*m)).map(|what| Block { line: n, what })).collect();
        if has(Marker::Bibliography) && !lines.iter().enumerate().any(|(n, (_, l))| marked(n, l) == Some(Marker::Bibliography)) {
            out.push(Block { line: lines.len(), what: Marker::Bibliography });
        }
        out
    }

    /// What an export of the note has to say about its thesis syntax: a label used twice, a
    /// reference to a label nobody has, a key the bibliography lacks, citations without a bibliography.
    pub fn warnings(&self, text: &str) -> Vec<String> {
        let mut out: Vec<String> = self.bib.messages.clone();
        out.extend(self.labels.duplicates.iter().filter(|d| d.source == self.index).map(|d| format!("The label {{#{}:{}}} is used twice; the first one counts", d.kind.prefix(), d.id)));
        out.extend(self.labels.unresolved.iter().filter(|u| u.source == self.index).map(|u| format!("There is no label {{#{0}:{1}}} for the reference @{0}:{1}", u.kind.prefix(), u.id)));
        let has = |k: &str| self.bib.has_key(k);
        let mut cited = false;
        for token in citations::scan(text, self.bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool)) {
            let Token::Group(g) = token else { continue };
            for item in &g.items {
                if let Target::Citation(key) = &item.target {
                    cited = true;
                    if item.known == Some(false) {
                        out.push(format!("The key “{key}” is not in the bibliography"));
                    }
                }
            }
        }
        if cited && !self.has_bibliography() {
            out.push("The note has citations and no bibliography is in effect (bibliography: or the vault's setting), so they are printed as written".to_string());
        }
        out
    }
}

/// Format the citations of a document: all the groups of the notes in reading order go through the
/// style at once, so that numbers follow the first citation in the document; those of `index` are returned.
fn format_document(texts: &[&str], index: usize, bib: &Effect, style: Style, lang: Lang) -> Formatted {
    let has = |k: &str| bib.has_key(k);
    let all: Vec<Vec<Group>> = texts
        .iter()
        .map(|t| {
            citations::scan(t, bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool))
                .into_iter()
                .filter_map(|tok| match tok {
                    Token::Group(g) if g.items.iter().any(|i| matches!(i.target, Target::Citation(_)) && i.known == Some(true)) => Some(g),
                    _ => None,
                })
                .collect()
        })
        .collect();
    let groups: Vec<&Group> = all.iter().flatten().collect();
    let entry = |k: &str| bib.entry(k).and_then(|e| e.csl.clone());
    let rendered = format::render(&groups, &entry, style, lang);
    let before: usize = all[..index.min(all.len())].iter().map(Vec::len).sum();
    let groups = all.get(index).map_or_else(Vec::new, |mine| mine.iter().enumerate().filter_map(|(n, g)| rendered.citations.get(before + n).cloned().flatten().map(|t| (g.range.clone(), t))).collect());
    Formatted { groups, references: rendered.references }
}

/// The bibliography in effect that a `.bib` text gives, for the tests of the editor and the reading view.
#[cfg(test)]
pub(crate) fn test_effect(bib: &str) -> Effect {
    let mut parsed = crate::bibliography::reader::parse(bib);
    parsed.entries.sort_by(|a, b| a.key.cmp(&b.key));
    let index = crate::bibliography::reader::FileIndex { entries: parsed.entries, skipped: 0, modified: None };
    Effect { files: vec![(PathBuf::from("refs.bib"), Arc::new(index))], ..Effect::default() }
}

#[cfg(test)]
mod tests {
    use super::test_effect as effect_with;
    use super::*;

    const BIB: &str = "@book{a, author = {Meier, Anna}, title = {Alpha}, date = {2020}, publisher = {P}}\n@book{b, author = {Schulz, Bo}, title = {Beta}, date = {2021}, publisher = {Q}}\n";

    // spec: EDIT-127
    #[test]
    fn edit_127_a_view_of_a_note_numbers_its_labels_and_formats_its_citations_in_its_language() {
        let text = "---\nlanguage: de\ncitation-style: apa\n---\nAls [@a, S. 3] und @b zeigen.\n\n![[x.png|Plot]]{#fig:p}\n\nSiehe @fig:p.\n";
        let v = View::build(None, &[text], 0, text, effect_with(BIB), true);
        assert_eq!(v.lang, Lang::De);
        let f = v.formatted.expect("formatted");
        let shown: Vec<&str> = f.groups.iter().map(|(_, t)| t.as_str()).collect();
        assert_eq!(shown, ["(Meier, 2020, S. 3)", "Schulz (2021)"]);
        assert_eq!(f.references.len(), 2);
        assert_eq!(&text[f.groups[0].0.clone()], "[@a, S. 3]");
        assert_eq!(v.labels.entries.len(), 1);
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_a_reference_group_is_its_word_and_number_and_unknown_when_a_label_is_missing() {
        let text = "![[x.png|A]]{#fig:a}\n\n![[y.png|B]]{#fig:b}\n\n[@fig:a; @fig:b] and [-@fig:b] and @fig:zz\n";
        let v = View::build(None, &[text], 0, text, Effect::default(), false);
        let groups: Vec<Group> = citations::scan(text, None).into_iter().filter_map(|t| if let Token::Group(g) = t { Some(g) } else { None }).collect();
        assert_eq!(v.reference_text(&groups[0]).as_deref(), Some("Figures 1 and 2"));
        assert_eq!(v.reference_text(&groups[1]).as_deref(), Some("2"));
        assert_eq!(v.reference_text(&groups[2]), None);
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_a_book_formats_by_the_first_citation_in_the_whole_book() {
        let book = Arc::new(Book { dir: PathBuf::from("T"), top: PathBuf::from("T/T.md"), notes: vec![(PathBuf::from("T/one.md"), true), (PathBuf::from("T/two.md"), true)] });
        let (one, two) = ("# One\n\nSee [@b].\n", "# Two\n\nSee [@a] and [@b].\n");
        let top = "---\ncitation-style: numeric\n---\n";
        let v = View::build(Some(book), &[one, two], 1, top, effect_with(BIB), true);
        let f = v.formatted.expect("formatted");
        assert_eq!(f.groups.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(), ["[2]", "[1]"], "b was cited first, in chapter one");
        assert_eq!(f.references.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(), ["b", "a"]);
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_the_captions_stand_where_the_properties_say_and_tables_above_figures_below_by_default() {
        assert_eq!(Captions::of(""), Captions { figure: CaptionAt::Below, table: CaptionAt::Above });
        assert_eq!(Captions::of("---\nfigure-caption: above\ntable-caption: below\n---\n"), Captions { figure: CaptionAt::Above, table: CaptionAt::Below });
    }
}
