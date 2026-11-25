//! The numbers of figures, tables, equations and sections (EDIT-128, ADR-0039 §2 and §3, #646).
//!
//! [`of_note`] and [`of_book`] read the labels (`{#fig:results}`, found by the scanner of EDIT-122)
//! of a note, or of the notes of a book in reading order, and give each one its number, its
//! caption and its place; the references (`@fig:results`) that name no label, and the labels that
//! come twice, are listed too. They are pure functions of the texts, so the reading view, the
//! built-in PDF, the site, the Word filter and the tests of the TeX export take the same numbers,
//! as the numbers of headings come from one function (LINK-014). Numbers are never written into
//! the file.
//!
//! - Figures, tables and equations have a counter each. **Continuous** numbering counts through
//!   the whole document (`1`, `2`, `3`); **per chapter** numbering counts again in each chapter
//!   and puts the chapter's number before it (`3.1`, `3.2`; `A.1` in an appendix chapter). The
//!   property `float-numbering:` (`continuous` or `chapter`) of the note, or of the top folder
//!   note of a book, decides; without it a note is continuous and a book is per chapter.
//!   What stands in no chapter (the front matter of a book, a part's note, the text of a note
//!   before its first `#` heading) is counted on its own, with no chapter before the number.
//! - A section is numbered as its heading is (LINK-014); in a book the headings of a chapter go
//!   below the chapter's number (BOOK-006), so `## Method` first in chapter 3 is `3.1`. A heading
//!   in the front matter has no number. The chapter's own title line (the first `#` heading of a
//!   chapter that says the title, which BOOK-006 does not print again) is the chapter, `3`, and in
//!   an appendix `A`.

use super::citations::{self, Kind, Target, Token};
use super::{find_headings, front_matter_values, heading_numbers, heading_plain, lines_with_offsets};
use std::collections::HashSet;
use std::ops::Range;

/// What a note of a book is in the reading order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// the note of a folder or the introduction: nothing is numbered in it by chapter
    Other,
    /// an abstract, a declaration or a preface (`thesis-part:`): unnumbered
    Front,
    Chapter,
    /// `thesis-part: appendix`: a chapter lettered A, B, …
    Appendix,
}

impl Role {
    /// The role the property `thesis-part:` of a note gives it: `abstract`, `declaration` and
    /// `preface` are front matter, `appendix` an appendix, anything else a chapter.
    pub fn of_note(text: &str) -> Role {
        match front_matter_values(text).into_iter().find(|(k, _)| k == "thesis-part").and_then(|(_, v)| v.first().map(|s| s.trim().to_lowercase())).as_deref() {
            Some("abstract" | "declaration" | "preface") => Role::Front,
            Some("appendix") => Role::Appendix,
            _ => Role::Chapter,
        }
    }
}

/// One note of a book, in reading order.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    pub text: &'a str,
    pub role: Role,
    /// The chapter's title: a first `# Heading` that says it is the chapter's title line (BOOK-006).
    pub title: Option<&'a str>,
    /// The number the chapter is printed with, when the exporter decides it (the built-in PDF
    /// prints `2.1` for the first chapter of the second part); without it chapters are counted 1, 2, 3
    /// and appendix chapters A, B, C. A note that is no chapter has none.
    pub number: Option<&'a str>,
}

/// How the figures, tables and equations are counted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Numbering {
    Continuous,
    PerChapter,
}

impl Numbering {
    /// The property `float-numbering:` of a note: `continuous` or `chapter`.
    pub fn property(text: &str) -> Option<Numbering> {
        match front_matter_values(text).into_iter().find(|(k, _)| k == "float-numbering").and_then(|(_, v)| v.first().map(|s| s.trim().to_lowercase())).as_deref() {
            Some("continuous") => Some(Numbering::Continuous),
            Some("chapter" | "per-chapter") => Some(Numbering::PerChapter),
            _ => None,
        }
    }
}

/// How high a section stands, which decides its word ("Abschnitt", "Kapitel", "Anhang").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Section,
    /// the title line of a chapter of a book
    Chapter,
    /// the title line of an appendix chapter
    Appendix,
}

/// A label with what it numbers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    pub id: String,
    /// `3`, `2.1`, `A.1`; none for a heading of the front matter
    pub number: Option<String>,
    /// the caption of a figure or table, the title of a section; empty for an equation
    pub caption: String,
    /// the section's level (always [`Level::Section`] for the other kinds)
    pub level: Level,
    /// the index of the note in the book (0 for a note)
    pub source: usize,
    /// the bytes of `{#kind:id}` in that note
    pub range: Range<usize>,
    /// the line of the label
    pub line: usize,
}

/// A label that an earlier one already has (the first wins).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Duplicate {
    pub kind: Kind,
    pub id: String,
    pub source: usize,
    pub range: Range<usize>,
}

/// A reference to a label that no note has.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unresolved {
    pub kind: Kind,
    pub id: String,
    pub source: usize,
    /// the bytes of `@kind:id`
    pub range: Range<usize>,
}

/// What the labels of a note or a book come to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Labels {
    pub numbering: Numbering,
    /// in reading order
    pub entries: Vec<Entry>,
    pub duplicates: Vec<Duplicate>,
    pub unresolved: Vec<Unresolved>,
}

impl Labels {
    pub fn get(&self, kind: Kind, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.kind == kind && e.id == id)
    }
}

/// The labels of a single note: numbers continuous unless `float-numbering: chapter` says otherwise.
pub fn of_note(text: &str) -> Labels {
    let numbering = Numbering::property(text).unwrap_or(Numbering::Continuous);
    build(&[Source { text, role: Role::Other, title: None, number: None }], numbering, false)
}

/// The labels of the notes of a book in reading order. `top` is the text of the top folder note,
/// whose `float-numbering:` decides; without it the numbers are per chapter.
pub fn of_book(sources: &[Source], top: Option<&str>) -> Labels {
    let numbering = top.and_then(Numbering::property).unwrap_or(Numbering::PerChapter);
    build(sources, numbering, true)
}

/// `{#kind:id}` taken out of a heading, caption or other line, and what is left trimmed.
pub fn without_labels(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(at) = rest.find("{#") {
        out.push_str(&rest[..at]);
        match citations::parse_label(&rest[at..]) {
            Some((_, _, len)) => rest = &rest[at + len..],
            None => {
                out.push_str("{#");
                rest = &rest[at + 2..];
            }
        }
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// `A` for 1, `B` for 2, … `Z`, `AA`.
fn letter(n: usize) -> String {
    let mut n = n;
    let mut out = Vec::new();
    while n > 0 {
        n -= 1;
        out.push(b'A' + (n % 26) as u8);
        n /= 26;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// The chapter a note stands for in a book: its number or letter, none for what is no chapter.
fn chapter_numbers(sources: &[Source], book: bool) -> Vec<Option<String>> {
    let (mut chapters, mut appendices) = (0, 0);
    sources
        .iter()
        .map(|s| match s.role {
            Role::Chapter | Role::Appendix if book && s.number.is_some() => s.number.map(str::to_string),
            Role::Chapter if book => {
                chapters += 1;
                Some(chapters.to_string())
            }
            Role::Appendix if book => {
                appendices += 1;
                Some(letter(appendices))
            }
            _ => None,
        })
        .collect()
}

/// The text with its chapter title line blanked, as BOOK-006 prints it (the line stays, so the
/// line numbers do), and the line that was blanked.
fn blank_title(s: &Source, book: bool) -> (String, Option<usize>) {
    let (Some(title), true, true) = (s.title, book, matches!(s.role, Role::Chapter | Role::Appendix)) else { return (s.text.to_string(), None) };
    let Some((line, 1, first)) = find_headings(s.text).into_iter().next() else { return (s.text.to_string(), None) };
    if heading_plain(&without_labels(&first)).to_lowercase() != heading_plain(title).to_lowercase() {
        return (s.text.to_string(), None);
    }
    let text = s.text.split('\n').enumerate().map(|(i, l)| if i == line { String::new() } else { l.to_string() }).collect::<Vec<_>>().join("\n");
    (text, Some(line))
}

fn build(sources: &[Source], numbering: Numbering, book: bool) -> Labels {
    let chapters = chapter_numbers(sources, book);
    let mut out = Labels { numbering, entries: Vec::new(), duplicates: Vec::new(), unresolved: Vec::new() };
    let mut seen: HashSet<(Kind, String)> = HashSet::new();
    // the counters of figures, tables and equations; per chapter they start again at each chapter
    let mut counters = [0usize; 3];
    let mut references: Vec<(usize, Kind, String, Range<usize>)> = Vec::new();
    // the note and chapter the last float was counted in
    let mut scope: Option<(usize, Option<String>)> = None;
    for (index, source) in sources.iter().enumerate() {
        let (blanked, title_line) = blank_title(source, book);
        let lines = lines_with_offsets(source.text);
        let heads: Vec<(usize, String)> = heading_numbers(&blanked).into_iter().map(|(line, _, _, n)| (line, n.unwrap_or_default())).collect();
        // a note outside a book that counts per chapter: the chapter is the `#` heading above
        let level_one: Vec<(usize, String)> = heading_numbers(&blanked).into_iter().filter(|(_, level, _, _)| *level == 1).map(|(line, _, _, n)| (line, n.unwrap_or_default())).collect();
        let chapter = &chapters[index];
        for token in citations::scan(source.text, None) {
            match token {
                Token::Group(group) => {
                    for item in group.items {
                        if let Target::Reference { kind, id } = item.target {
                            if kind.is_labelled() {
                                references.push((index, kind, id, item.range));
                            }
                        }
                    }
                }
                Token::Label(label) => {
                    let key = (label.kind, label.id.clone());
                    if !seen.insert(key) {
                        out.duplicates.push(Duplicate { kind: label.kind, id: label.id, source: index, range: label.range });
                        continue;
                    }
                    let line = lines.partition_point(|(off, _)| *off <= label.range.start).saturating_sub(1);
                    let text_line = lines.get(line).map_or("", |(_, l)| *l);
                    // the chapter whose number goes before a float's: the book's, or the `#` heading above in a note
                    let prefix = if book { chapter.clone() } else { level_one.iter().rev().find(|(l, _)| *l < line).map(|(_, n)| n.clone()) };
                    // per chapter the counters start again where the chapter changes
                    if numbering == Numbering::PerChapter && scope.as_ref() != Some(&(index, prefix.clone())) {
                        counters = [0; 3];
                    }
                    scope = Some((index, prefix.clone()));
                    let (number, caption, level) = match label.kind {
                        Kind::Section => section(&heads, title_line, line, text_line, source, chapter, book),
                        kind => {
                            let slot = match kind {
                                Kind::Figure => 0,
                                Kind::Table => 1,
                                _ => 2,
                            };
                            counters[slot] += 1;
                            let n = match (numbering, &prefix) {
                                (Numbering::PerChapter, Some(p)) => format!("{p}.{}", counters[slot]),
                                _ => counters[slot].to_string(),
                            };
                            (Some(n), float_caption(kind, text_line), Level::Section)
                        }
                    };
                    out.entries.push(Entry { kind: label.kind, id: label.id, number, caption, level, source: index, range: label.range, line });
                }
            }
        }
    }
    for (source, kind, id, range) in references {
        if !seen.contains(&(kind, id.clone())) {
            out.unresolved.push(Unresolved { kind, id, source, range });
        }
    }
    out
}

/// The number, title and level of the heading on `line`.
fn section(heads: &[(usize, String)], title_line: Option<usize>, line: usize, text_line: &str, source: &Source, chapter: &Option<String>, book: bool) -> (Option<String>, String, Level) {
    let raw = text_line.trim_start_matches('#').trim();
    let caption = heading_plain(&without_labels(raw));
    if title_line == Some(line) {
        let level = if source.role == Role::Appendix { Level::Appendix } else { Level::Chapter };
        return (chapter.clone(), caption, level);
    }
    let own = heads.iter().find(|(l, _)| *l == line).map(|(_, n)| n.clone());
    // in a book a chapter's headings go below the chapter's number (BOOK-006); what is no chapter has none
    let number = match (book, chapter, own) {
        (true, Some(c), Some(own)) => Some(format!("{c}.{own}")),
        (true, _, _) => None,
        (false, _, own) => own,
    };
    (number, caption, Level::Section)
}

/// The caption on the line of a label: the alias of the embed before a figure's label, the text of
/// the `: …` line of a table.
fn float_caption(kind: Kind, line: &str) -> String {
    match kind {
        Kind::Figure => super::find_links(line).into_iter().find(|l| l.embed).map(|l| crate::caption::text(&l)).unwrap_or_default(),
        Kind::Table => without_labels(line.trim_start().strip_prefix(':').unwrap_or(line)),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbers(l: &Labels) -> Vec<(String, Option<String>)> {
        l.entries.iter().map(|e| (format!("{}:{}", e.kind.prefix(), e.id), e.number.clone())).collect()
    }

    fn n(list: &[(&str, &str)]) -> Vec<(String, Option<String>)> {
        list.iter().map(|(k, v)| (k.to_string(), Some(v.to_string()))).collect()
    }

    const FLOATS: &str =
        "# One\n\n![[a.png|First plot]]{#fig:a}\n\n| x |\n|---|\n| 1 |\n: Runs of the test {#tbl:runs}\n\n$$ E = mc^2 $$ {#eq:e}\n\n## Method {#sec:m}\n\n![[b.png|Second]]{#fig:b}\n\n# Two {#sec:two}\n\n![[c.png]]{#fig:c}\n";

    // spec: EDIT-128
    #[test]
    fn edit_128_a_note_counts_continuously_with_a_counter_for_each_kind() {
        let l = of_note(FLOATS);
        assert_eq!(l.numbering, Numbering::Continuous);
        assert_eq!(numbers(&l), n(&[("fig:a", "1"), ("tbl:runs", "1"), ("eq:e", "1"), ("sec:m", "1.1"), ("fig:b", "2"), ("sec:two", "2"), ("fig:c", "3")]));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_the_property_makes_a_note_count_per_chapter_and_a_chapter_less_float_has_no_prefix() {
        let text = format!("![[z.png|Before]]{{#fig:z}}\n\n{FLOATS}");
        let with = format!("---\nfloat-numbering: chapter\n---\n{text}");
        let l = of_note(&with);
        assert_eq!(l.numbering, Numbering::PerChapter);
        assert_eq!(numbers(&l), n(&[("fig:z", "1"), ("fig:a", "1.1"), ("tbl:runs", "1.1"), ("eq:e", "1.1"), ("sec:m", "1.1"), ("fig:b", "1.2"), ("sec:two", "2"), ("fig:c", "2.1")]));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_captions_are_the_alias_of_the_embed_the_table_line_and_the_heading_text() {
        let l = of_note(FLOATS);
        let caption = |k: Kind, id: &str| l.get(k, id).map(|e| e.caption.clone());
        assert_eq!(caption(Kind::Figure, "a").as_deref(), Some("First plot"));
        assert_eq!(caption(Kind::Table, "runs").as_deref(), Some("Runs of the test"));
        assert_eq!(caption(Kind::Equation, "e").as_deref(), Some(""));
        assert_eq!(caption(Kind::Section, "m").as_deref(), Some("Method"));
        assert_eq!(l.get(Kind::Figure, "c").map(|e| e.caption.as_str()), Some("c.png"), "an embed without an alias is captioned by its name");
    }

    fn book<'a>(texts: &'a [(&'a str, Role, Option<&'a str>)], top: Option<&str>) -> Labels {
        let sources: Vec<Source> = texts.iter().map(|(text, role, title)| Source { text, role: *role, title: *title, number: None }).collect();
        of_book(&sources, top)
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_book_counts_per_chapter_and_the_chapter_title_line_is_the_chapter() {
        let l = book(
            &[
                ("# Intro\n\n![[a.png|A]]{#fig:a}\n", Role::Front, Some("Intro")),
                ("# Methods {#sec:methods}\n\n## Setup {#sec:setup}\n\n![[b.png|B]]{#fig:b}\n\n![[c.png|C]]{#fig:c}\n", Role::Chapter, Some("Methods")),
                ("# Results\n\n![[d.png|D]]{#fig:d}\n\n| t |\n|---|\n| 1 |\n: T {#tbl:t}\n\n$$x$$ {#eq:x}\n", Role::Chapter, Some("Results")),
            ],
            None,
        );
        assert_eq!(l.numbering, Numbering::PerChapter);
        assert_eq!(numbers(&l), n(&[("fig:a", "1"), ("sec:methods", "1"), ("sec:setup", "1.1"), ("fig:b", "1.1"), ("fig:c", "1.2"), ("fig:d", "2.1"), ("tbl:t", "2.1"), ("eq:x", "2.1")]));
        assert_eq!(l.get(Kind::Section, "methods").map(|e| e.level), Some(Level::Chapter));
        assert_eq!(l.get(Kind::Section, "setup").map(|e| e.level), Some(Level::Section));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_float_numbering_continuous_in_the_top_note_makes_a_book_count_through() {
        let texts = [("# A\n\n![[a.png|A]]{#fig:a}\n", Role::Chapter, Some("A")), ("# B\n\n![[b.png|B]]{#fig:b}\n", Role::Chapter, Some("B"))];
        let l = book(&texts, Some("---\nfloat-numbering: continuous\n---\nTop"));
        assert_eq!(numbers(&l), n(&[("fig:a", "1"), ("fig:b", "2")]));
        let l = book(&texts, Some("---\nfloat-numbering: chapter\n---\nTop"));
        assert_eq!(numbers(&l), n(&[("fig:a", "1.1"), ("fig:b", "2.1")]));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_appendix_chapters_are_lettered_and_the_front_matter_is_unnumbered() {
        let l = book(
            &[
                ("# Abstract {#sec:abs}\n\n## Aim {#sec:aim}\n", Role::Front, Some("Abstract")),
                ("# One\n\n![[a.png|A]]{#fig:a}\n", Role::Chapter, Some("One")),
                ("# Extra {#sec:extra}\n\n## More {#sec:more}\n\n![[b.png|B]]{#fig:b}\n\n$$y$$ {#eq:y}\n", Role::Appendix, Some("Extra")),
                ("# Second {#sec:second}\n", Role::Appendix, Some("Second")),
            ],
            None,
        );
        let got = numbers(&l);
        assert_eq!(got[0], ("sec:abs".to_string(), None));
        assert_eq!(got[1], ("sec:aim".to_string(), None));
        assert_eq!(&got[2..], n(&[("fig:a", "1.1"), ("sec:extra", "A"), ("sec:more", "A.1"), ("fig:b", "A.1"), ("eq:y", "A.1"), ("sec:second", "B")]).as_slice());
        assert_eq!(l.get(Kind::Section, "extra").map(|e| e.level), Some(Level::Appendix));
    }

    // spec: EDIT-128, BOOK-020
    #[test]
    fn edit_128_a_source_with_a_number_is_that_chapter_and_the_floats_count_below_it() {
        let sources = [
            Source { text: "![[p.png|P]]{#fig:p}\n", role: Role::Other, title: None, number: Some("1") },
            Source { text: "# Methods {#sec:methods}\n\n## Setup {#sec:setup}\n\n![[a.png|A]]{#fig:a}\n\n![[b.png|B]]{#fig:b}\n", role: Role::Chapter, title: Some("Methods"), number: Some("2.1") },
            Source { text: "![[c.png|C]]{#fig:c}\n", role: Role::Chapter, title: Some("Results"), number: Some("2.2") },
        ];
        let l = of_book(&sources, None);
        assert_eq!(
            numbers(&l),
            n(&[("fig:p", "1"), ("sec:methods", "2.1"), ("sec:setup", "2.1.1"), ("fig:a", "2.1.1"), ("fig:b", "2.1.2"), ("fig:c", "2.2.1")]).as_slice(),
            "a note that is no chapter takes no number, even when it is given one"
        );
        let l = of_book(&sources, Some("---\nfloat-numbering: continuous\n---\n"));
        assert_eq!(numbers(&l)[3..], n(&[("fig:a", "2"), ("fig:b", "3"), ("fig:c", "4")])[..], "continuous counts through whatever the chapters are called");
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_title_line_that_is_no_title_is_a_section_of_the_chapter() {
        let l = book(&[("# Other words {#sec:o}\n", Role::Chapter, Some("The chapter"))], None);
        assert_eq!(numbers(&l), n(&[("sec:o", "1.1")]), "BOOK-006: it is printed, and numbered below the chapter");
        let l = book(&[("# Other words {#sec:o}\n", Role::Chapter, None)], None);
        assert_eq!(numbers(&l), n(&[("sec:o", "1.1")]));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_second_label_is_a_duplicate_and_does_not_count_and_the_first_wins() {
        let l = of_note("![[a.png|A]]{#fig:x}\n\n![[b.png|B]]{#fig:x}\n\n![[c.png|C]]{#fig:y}\n");
        assert_eq!(numbers(&l), n(&[("fig:x", "1"), ("fig:y", "2")]));
        assert_eq!(l.get(Kind::Figure, "x").map(|e| e.caption.as_str()), Some("A"));
        assert_eq!(l.duplicates.len(), 1);
        assert_eq!((l.duplicates[0].kind, l.duplicates[0].id.as_str()), (Kind::Figure, "x"));
        let text = "![[a.png|A]]{#fig:x}\n\n![[b.png|B]]{#fig:x}\n";
        assert_eq!(&text[l.duplicates[0].range.clone()], "{#fig:x}");
        assert_eq!(l.duplicates[0].range.start, text.rfind("{#").unwrap());
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_label_of_another_kind_with_the_same_id_is_no_duplicate() {
        let l = of_note("![[a.png|A]]{#fig:x}\n\n| a |\n|---|\n| 1 |\n: T {#tbl:x}\n");
        assert!(l.duplicates.is_empty());
        assert_eq!(l.entries.len(), 2);
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_reference_to_a_label_nobody_has_is_unresolved_and_the_others_are_not() {
        let text = "![[a.png|A]]{#fig:a}\n\nSee @fig:a, @fig:zz, [@tbl:t; @eq:q] and @abbr:ml and @gls:x.\n";
        let l = of_note(text);
        let got: Vec<(Kind, &str, &str)> = l.unresolved.iter().map(|u| (u.kind, u.id.as_str(), &text[u.range.clone()])).collect();
        assert_eq!(got, vec![(Kind::Figure, "zz", "@fig:zz"), (Kind::Table, "t", "@tbl:t"), (Kind::Equation, "q", "@eq:q")], "abbreviations and terms are no labels");
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_reference_in_a_later_chapter_finds_a_label_of_an_earlier_one_and_the_reverse() {
        let l = book(&[("# A\n\nSee @fig:late.\n", Role::Chapter, Some("A")), ("# B\n\n![[b.png|B]]{#fig:late}\n\nand @fig:none\n", Role::Chapter, Some("B"))], None);
        assert_eq!(l.unresolved.len(), 1);
        assert_eq!((l.unresolved[0].id.as_str(), l.unresolved[0].source), ("none", 1));
        assert_eq!(l.get(Kind::Figure, "late").map(|e| (e.source, e.number.as_deref())), Some((1, Some("2.1"))));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_nothing_is_counted_in_code_front_matter_or_a_note_without_labels() {
        assert!(of_note("").entries.is_empty());
        assert!(of_note("# Title\n\nText.\n").entries.is_empty());
        let l = of_note("```\n![[a.png]]{#fig:a}\n```\n\n![[b.png|B]]{#fig:b}\n");
        assert_eq!(numbers(&l), n(&[("fig:b", "1")]));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_a_display_on_one_line_with_the_label_of_an_equation_is_a_block_formula() {
        let block = |text: &str| super::super::find_math(text).first().map(|m| (m.block, m.display));
        assert_eq!(block("$$ E = mc^2 $$ {#eq:energy}"), Some((true, true)));
        assert_eq!(block("$$E$${#eq:e}"), Some((true, true)));
        assert_eq!(block("$$ E $$"), Some((true, true)));
        assert_eq!(block("so $$ E $$ {#eq:e}"), Some((false, true)), "text before it: a formula in the line");
        assert_eq!(block("$$ E $$ {#fig:e}"), Some((false, true)), "the label of another kind is text");
        assert_eq!(block("$$ E $$ and more"), Some((false, true)));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_letters_run_on_after_z() {
        assert_eq!((letter(1), letter(26), letter(27), letter(28)), ("A".to_string(), "Z".to_string(), "AA".to_string(), "AB".to_string()));
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_the_role_comes_from_the_property_thesis_part() {
        assert_eq!(Role::of_note("---\nthesis-part: abstract\n---\nx"), Role::Front);
        assert_eq!(Role::of_note("---\nthesis-part: Declaration\n---\nx"), Role::Front);
        assert_eq!(Role::of_note("---\nthesis-part: appendix\n---\nx"), Role::Appendix);
        assert_eq!(Role::of_note("x"), Role::Chapter);
    }

    // spec: EDIT-128
    #[test]
    fn edit_128_labels_are_taken_out_of_a_line() {
        assert_eq!(without_labels("Method {#sec:m}"), "Method");
        assert_eq!(without_labels("a {#nolabel} b {#fig:x}"), "a {#nolabel} b");
    }
}
