//! The thesis document of a note or a book as the editor and the reading view see it (ADR-0039,
//! #646): the numbers of its labels (EDIT-128), the bibliography in effect (EDIT-123), the
//! language, the style, where the captions stand, and the citations as the style formats them
//! (EDIT-127). [`View::build`] makes it from texts, so the built-in PDF and the site (#648) can
//! take the same.

use super::Lang;
use crate::bibliography::format::{self, Reference, Style};
use crate::bibliography::Effect;
use crate::markdown::citations::{self, Group, Target, Token};
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
