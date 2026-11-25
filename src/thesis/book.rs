//! What the export of a book has to say about its thesis syntax (BOOK-020, BOOK-021, #649): the
//! built-in PDF and the TeX export give the same sentences, each naming the notes it is about.

use crate::bibliography::Effect;
use crate::markdown::citations::{self, Target, Token};
use crate::markdown::labels::Labels;

/// A note of a book as the warnings need it.
#[derive(Clone, Copy, Debug)]
pub struct Note<'a> {
    /// how the note is called in a sentence: its title
    pub name: &'a str,
    pub text: &'a str,
    /// the top folder note, whose `bibliography:` is the book's
    pub top: bool,
}

/// The warnings of a book, in the order of its notes: the messages about the bibliography files, a
/// chapter that names a bibliography of its own (ignored), a label used in two notes, a reference
/// to a label no note has, a key the bibliography lacks, and citations with no bibliography in
/// effect. `labels` are those of the whole book (`labels::of_book`), `notes` its notes in the same order.
pub fn warnings(notes: &[Note], labels: &Labels, bib: &Effect) -> Vec<String> {
    let name = |source: usize| notes.get(source).map_or("?", |n| n.name);
    let mut out: Vec<String> = bib.messages.clone();
    for n in notes.iter().filter(|n| !n.top) {
        if crate::markdown::front_matter_values(n.text).iter().any(|(k, _)| k == "bibliography") {
            out.push(format!("The note “{}” names a bibliography of its own: a book has one, the top folder note's or the vault's default, so it is ignored", n.name));
        }
    }
    for d in &labels.duplicates {
        let first = labels.get(d.kind, &d.id).map_or("?", |e| name(e.source));
        out.push(format!("The label {{#{}:{}}} is in “{first}” and again in “{}”: the first one counts", d.kind.prefix(), d.id, name(d.source)));
    }
    for u in labels.unresolved.iter().filter(|u| u.kind.is_labelled()) {
        out.push(format!("The note “{}” refers to @{}:{} and no note of the book has {{#{}:{}}}: it stays text", name(u.source), u.kind.prefix(), u.id, u.kind.prefix(), u.id));
    }
    let has = |k: &str| bib.has_key(k);
    let mut cited: Vec<&str> = Vec::new();
    for n in notes {
        let mut unknown: Vec<String> = Vec::new();
        for token in citations::scan(n.text, bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool)) {
            let Token::Group(g) = token else { continue };
            for item in &g.items {
                if let Target::Citation(key) = &item.target {
                    if !cited.contains(&n.name) {
                        cited.push(n.name);
                    }
                    if item.known == Some(false) && !unknown.contains(key) {
                        unknown.push(key.clone());
                    }
                }
            }
        }
        out.extend(unknown.into_iter().map(|k| format!("The key “{k}” cited in the note “{}” is not in the bibliography: the citation stays text", n.name)));
    }
    if !cited.is_empty() && !bib.in_effect() {
        out.push(format!(
            "The notes {} have citations and no bibliography is in effect (bibliography: in the top folder note, or the vault's setting), so they are printed as written",
            cited.iter().map(|n| format!("“{n}”")).collect::<Vec<_>>().join(", ")
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::labels::{self, Role, Source};

    const BIB: &str = "@book{a, author = {Meier, Anna}, title = {Alpha}, date = {2020}, publisher = {P}}\n";

    fn labels_of(texts: &[&str]) -> Labels {
        let sources: Vec<Source> = texts.iter().map(|t| Source { text: t, role: Role::Chapter, title: None, number: None }).collect();
        labels::of_book(&sources, None)
    }

    fn notes<'a>(texts: &[&'a str]) -> Vec<Note<'a>> {
        const NAMES: [&str; 4] = ["Top", "One", "Two", "Three"];
        texts.iter().enumerate().map(|(i, t)| Note { name: NAMES[i], text: t, top: i == 0 }).collect()
    }

    // spec: BOOK-020
    #[test]
    fn book_020_a_duplicate_label_names_both_notes_and_a_missing_one_names_the_note() {
        let texts = ["", "![[a.png|A]]{#fig:a}\n", "![[b.png|B]]{#fig:a}\n\nSee @fig:a and @fig:nowhere and @tbl:nope.\n"];
        let w = warnings(&notes(&texts), &labels_of(&texts), &Effect::default());
        assert_eq!(
            w,
            [
                "The label {#fig:a} is in “One” and again in “Two”: the first one counts",
                "The note “Two” refers to @fig:nowhere and no note of the book has {#fig:nowhere}: it stays text",
                "The note “Two” refers to @tbl:nope and no note of the book has {#tbl:nope}: it stays text",
            ]
        );
    }

    // spec: BOOK-021
    #[test]
    fn book_021_a_note_with_its_own_bibliography_and_an_unknown_key_are_warnings_naming_the_note() {
        let texts = ["---\nbibliography: refs.bib\n---\n", "---\nbibliography: other.bib\n---\nSee [@a] and [@zzz] and [@zzz].\n"];
        let w = warnings(&notes(&texts), &labels_of(&texts), &crate::thesis::view::test_effect(BIB));
        assert_eq!(
            w,
            ["The note “One” names a bibliography of its own: a book has one, the top folder note's or the vault's default, so it is ignored", "The key “zzz” cited in the note “One” is not in the bibliography: the citation stays text"]
        );
        // no bibliography in effect: one sentence for the notes that cite
        let texts = ["", "A [@a].\n", "B [@a] and [@b].\n", "none\n"];
        let w = warnings(&notes(&texts), &labels_of(&texts), &Effect::default());
        assert_eq!(w, ["The notes “One”, “Two” have citations and no bibliography is in effect (bibliography: in the top folder note, or the vault's setting), so they are printed as written"]);
        // a clean book says nothing
        assert!(warnings(&notes(&texts[..2]), &labels_of(&texts[..2]), &crate::thesis::view::test_effect(BIB)).is_empty());
    }
}
