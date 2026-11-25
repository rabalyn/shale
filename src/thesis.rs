//! The thesis document of a note or a book (ADR-0039, #646): [`View`] holds its numbers, its
//! bibliography and its formatted citations; the rest of this module are the words of a thesis document in its language (ADR-0039 §3, #646): "Abbildung 3" or
//! "Figure 3", "Gleichung (2.1)", "Literaturverzeichnis", and the language itself. The reading
//! view, the built-in PDF and the site take them from here, so they say the same.

pub mod view;

pub mod export;

pub use view::{Book, Marker, Shown, View};

use crate::markdown::citations::Kind;
use crate::markdown::labels::{Entry, Level};

/// The language of a document: German and English have the reference words; any other is English.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lang {
    De,
    #[default]
    En,
}

impl Lang {
    /// The language the property `language:` of a note names (`de`, `de-DE`, `german`, `deutsch`,
    /// `en`, `english`, `ngerman`, …); any other value is English, none gives `None`.
    pub fn property(text: &str) -> Option<Lang> {
        let value = crate::markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "language").and_then(|(_, v)| v.into_iter().next())?;
        let v = value.trim().to_lowercase();
        Some(if v == "de" || v.starts_with("de-") || v.starts_with("de_") || matches!(v.as_str(), "german" | "ngerman" | "deutsch" | "germanb") { Lang::De } else { Lang::En })
    }

    /// The CSL locale of the language.
    pub const fn locale(self) -> &'static str {
        match self {
            Lang::De => "de-DE",
            Lang::En => "en-US",
        }
    }

    /// The heading of the bibliography when it is printed at the end.
    pub const fn bibliography_heading(self) -> &'static str {
        match self {
            Lang::De => "Literaturverzeichnis",
            Lang::En => "References",
        }
    }

    /// The heading of the list of figures or of tables (`Kind::Figure`, `Kind::Table`).
    pub const fn list_heading(self, kind: Kind) -> &'static str {
        match (self, kind) {
            (Lang::De, Kind::Table) => "Tabellenverzeichnis",
            (Lang::De, _) => "Abbildungsverzeichnis",
            (Lang::En, Kind::Table) => "List of Tables",
            (Lang::En, _) => "List of Figures",
        }
    }

    const fn and(self) -> &'static str {
        match self {
            Lang::De => "und",
            Lang::En => "and",
        }
    }
}

/// The word of a kind in front of its number, in the singular or the plural.
pub(crate) fn word(kind: Kind, level: Level, plural: bool, lang: Lang) -> &'static str {
    match (lang, kind, level, plural) {
        (Lang::De, Kind::Figure, _, false) => "Abbildung",
        (Lang::De, Kind::Figure, _, true) => "Abbildungen",
        (Lang::De, Kind::Table, _, false) => "Tabelle",
        (Lang::De, Kind::Table, _, true) => "Tabellen",
        (Lang::De, Kind::Equation, _, false) => "Gleichung",
        (Lang::De, Kind::Equation, _, true) => "Gleichungen",
        (Lang::De, _, Level::Chapter, false) => "Kapitel",
        (Lang::De, _, Level::Chapter, true) => "Kapitel",
        (Lang::De, _, Level::Appendix, false) => "Anhang",
        (Lang::De, _, Level::Appendix, true) => "Anhänge",
        (Lang::De, _, Level::Section, false) => "Abschnitt",
        (Lang::De, _, Level::Section, true) => "Abschnitte",
        (Lang::En, Kind::Figure, _, false) => "Figure",
        (Lang::En, Kind::Figure, _, true) => "Figures",
        (Lang::En, Kind::Table, _, false) => "Table",
        (Lang::En, Kind::Table, _, true) => "Tables",
        (Lang::En, Kind::Equation, _, false) => "Equation",
        (Lang::En, Kind::Equation, _, true) => "Equations",
        (Lang::En, _, Level::Chapter, false) => "Chapter",
        (Lang::En, _, Level::Chapter, true) => "Chapters",
        (Lang::En, _, Level::Appendix, false) => "Appendix",
        (Lang::En, _, Level::Appendix, true) => "Appendices",
        (Lang::En, _, Level::Section, false) => "Section",
        (Lang::En, _, Level::Section, true) => "Sections",
    }
}

/// The number as it is written after the word: an equation's in brackets.
fn written(entry: &Entry) -> Option<String> {
    entry.number.as_ref().map(|n| if entry.kind == Kind::Equation { format!("({n})") } else { n.clone() })
}

/// A list of numbers: `1`, `1 and 2`, `1, 2 and 3`.
fn list(numbers: &[String], lang: Lang) -> String {
    match numbers {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} {} {last}", init.join(", "), lang.and()),
    }
}

/// The word and number of one label, as `\cref` writes it: "Abbildung 3", "Gleichung (2.1)",
/// "Anhang A". A heading of the front matter has no number: it is named by its title.
pub fn reference(entry: &Entry, lang: Lang) -> String {
    group(&[(entry, false)], lang)
}

/// The text of a bracket of references (`[@fig:a; @fig:b]`): each item is a label and whether its
/// number stands alone (`[-@fig:a]`). Items of one kind come together under the plural word
/// ("Abbildungen 1 und 2"); the kinds follow in the order they first appear, set apart by `;`.
pub fn group(items: &[(&Entry, bool)], lang: Lang) -> String {
    let key = |e: &Entry| (e.kind, if e.kind == Kind::Section { e.level } else { Level::Section });
    // the items of one kind of label (a section's level counts as a kind of its own: "Kapitel", "Anhang")
    type OfKind<'a> = ((Kind, Level), Vec<(&'a Entry, bool)>);
    let mut kinds: Vec<OfKind> = Vec::new();
    for &(e, alone) in items {
        match kinds.iter_mut().find(|(k, _)| *k == key(e)) {
            Some((_, v)) => v.push((e, alone)),
            None => kinds.push((key(e), vec![(e, alone)])),
        }
    }
    kinds
        .into_iter()
        .map(|((kind, level), v)| {
            let numbers: Vec<String> = v.iter().map(|(e, _)| written(e).unwrap_or_else(|| format!("“{}”", e.caption))).collect();
            if v.iter().all(|(_, alone)| *alone) {
                list(&numbers, lang)
            } else {
                format!("{} {}", word(kind, level, v.len() > 1, lang), list(&numbers, lang))
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The label of a caption: "Abbildung 3", "Table 2.1" (an equation has none, its number stands at the margin).
pub fn caption_label(entry: &Entry, lang: Lang) -> String {
    format!("{} {}", word(entry.kind, Level::Section, false, lang), entry.number.clone().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::labels;

    fn entries(text: &str) -> labels::Labels {
        labels::of_note(text)
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_a_reference_is_the_word_and_the_number_in_the_language_of_the_document() {
        let l = entries("![[a.png|A]]{#fig:a}\n\n| a |\n|---|\n| 1 |\n: T {#tbl:t}\n\n$$x$$ {#eq:x}\n\n# Method {#sec:m}\n");
        let get = |k: Kind, id: &str| l.get(k, id).expect("label");
        assert_eq!(reference(get(Kind::Figure, "a"), Lang::De), "Abbildung 1");
        assert_eq!(reference(get(Kind::Figure, "a"), Lang::En), "Figure 1");
        assert_eq!(reference(get(Kind::Table, "t"), Lang::De), "Tabelle 1");
        assert_eq!(reference(get(Kind::Equation, "x"), Lang::De), "Gleichung (1)");
        assert_eq!(reference(get(Kind::Equation, "x"), Lang::En), "Equation (1)");
        assert_eq!(reference(get(Kind::Section, "m"), Lang::De), "Abschnitt 1");
        assert_eq!(reference(get(Kind::Section, "m"), Lang::En), "Section 1");
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_several_of_a_kind_take_the_plural_and_a_minus_gives_the_number_alone() {
        let l = entries("![[a.png|A]]{#fig:a}\n\n![[b.png|B]]{#fig:b}\n\n![[c.png|C]]{#fig:c}\n\n$$x$$ {#eq:x}\n");
        let f = |id: &str| l.get(Kind::Figure, id).expect("label");
        assert_eq!(group(&[(f("a"), false), (f("b"), false)], Lang::De), "Abbildungen 1 und 2");
        assert_eq!(group(&[(f("a"), false), (f("b"), false), (f("c"), false)], Lang::En), "Figures 1, 2 and 3");
        assert_eq!(group(&[(f("c"), true)], Lang::En), "3");
        let eq = l.get(Kind::Equation, "x").expect("label");
        assert_eq!(group(&[(eq, true)], Lang::De), "(1)");
        assert_eq!(group(&[(f("a"), false), (eq, false)], Lang::En), "Figure 1; Equation (1)");
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_chapters_and_appendices_have_their_own_words_and_front_matter_is_named_by_title() {
        let sources = [
            labels::Source { text: "# Abstract {#sec:abs}\n", role: labels::Role::Front, title: Some("Abstract") },
            labels::Source { text: "# One {#sec:one}\n\n## Two {#sec:two}\n", role: labels::Role::Chapter, title: Some("One") },
            labels::Source { text: "# Extra {#sec:extra}\n", role: labels::Role::Appendix, title: Some("Extra") },
        ];
        let l = labels::of_book(&sources, None);
        let s = |id: &str| l.get(Kind::Section, id).expect("label");
        assert_eq!(reference(s("one"), Lang::De), "Kapitel 1");
        assert_eq!(reference(s("two"), Lang::De), "Abschnitt 1.1");
        assert_eq!(reference(s("extra"), Lang::De), "Anhang A");
        assert_eq!(reference(s("extra"), Lang::En), "Appendix A");
        assert_eq!(reference(s("one"), Lang::En), "Chapter 1");
        assert_eq!(reference(s("abs"), Lang::En), "Section “Abstract”");
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_the_property_language_names_german_or_english_and_anything_else_is_english() {
        assert_eq!(Lang::property("---\nlanguage: de\n---\n"), Some(Lang::De));
        assert_eq!(Lang::property("---\nlanguage: de-DE\n---\n"), Some(Lang::De));
        assert_eq!(Lang::property("---\nlanguage: german\n---\n"), Some(Lang::De));
        assert_eq!(Lang::property("---\nlanguage: en\n---\n"), Some(Lang::En));
        assert_eq!(Lang::property("---\nlanguage: fr\n---\n"), Some(Lang::En));
        assert_eq!(Lang::property("no front matter"), None);
        assert_eq!(Lang::De.bibliography_heading(), "Literaturverzeichnis");
        assert_eq!(Lang::En.bibliography_heading(), "References");
    }

    // spec: EDIT-127
    #[test]
    fn edit_127_the_label_of_a_caption_is_the_word_and_the_number() {
        let l = entries("![[a.png|A]]{#fig:a}\n");
        assert_eq!(caption_label(l.get(Kind::Figure, "a").expect("label"), Lang::De), "Abbildung 1");
        assert_eq!(caption_label(l.get(Kind::Figure, "a").expect("label"), Lang::En), "Figure 1");
    }
}
