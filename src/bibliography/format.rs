//! Citations and the bibliography as a style formats them (EDIT-127, ADR-0039 §5 and §6, #646).
//!
//! The built-in exports (the reading view, the PDF, the site) format with the `hayagriva` crate,
//! which runs CSL styles; the entries are the ones the `biblatex` reader of the index already
//! converted ([`super::reader::Entry::csl`]). The five named styles of ADR-0039 are CSL styles of
//! the archive that comes with `hayagriva`, and the document's language decides the terms
//! ("and" or "und", "p." or "S."). One function, [`render`], takes the citation groups of a
//! document in reading order, because numeric styles number the entries by first citation, and
//! returns the text of every group and the list of the entries cited, so that every export draws
//! the same.

use super::reader::Entry;
use crate::markdown::citations::{Group, Item, Target};
use crate::thesis::Lang;
use hayagriva::archive::{self, ArchivedStyle};
use hayagriva::citationberg::taxonomy::Locator;
use hayagriva::citationberg::{IndependentStyle, Locale, LocaleCode, Style as Csl};
use hayagriva::{BibliographyDriver, BibliographyRequest, BufWriteFormat, CitationItem, CitationRequest, CitePurpose, ElemChildren, LocatorPayload, SpecificLocator};
use std::sync::{Arc, OnceLock};

/// The five named styles of ADR-0039 §5 (the property `citation-style:`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Style {
    /// `[1]`, numbered by first citation (Springer, numeric)
    Numeric,
    /// `(Meier 2020, 12)`, the default
    #[default]
    AuthorYear,
    /// `[Mei20]`; the list of references is Chicago's (the style of the labels has none), sorted by label
    Alphabetic,
    /// `[1]` and the references of the IEEE
    Ieee,
    /// `(Meier, 2020)`
    Apa,
}

impl Style {
    pub const ALL: [Style; 5] = [Style::Numeric, Style::AuthorYear, Style::Alphabetic, Style::Ieee, Style::Apa];

    /// The word of the property `citation-style:`.
    pub const fn name(self) -> &'static str {
        match self {
            Style::Numeric => "numeric",
            Style::AuthorYear => "authoryear",
            Style::Alphabetic => "alphabetic",
            Style::Ieee => "ieee",
            Style::Apa => "apa",
        }
    }

    /// The style a word names, ignoring case, `-` and `_` (`author-year`).
    pub fn named(word: &str) -> Option<Style> {
        let w: String = word.trim().chars().filter(|c| !matches!(c, '-' | '_' | ' ')).collect::<String>().to_lowercase();
        Style::ALL.into_iter().find(|s| s.name() == w)
    }

    /// The style a note asks for with `citation-style:`, `None` when it names none (or none of the five).
    pub fn property(text: &str) -> Option<Style> {
        crate::markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "citation-style").and_then(|(_, v)| v.first().and_then(|w| Style::named(w)))
    }

    fn archived(self) -> ArchivedStyle {
        match self {
            Style::Numeric => ArchivedStyle::SpringerMathphys,
            Style::AuthorYear => ArchivedStyle::ChicagoAuthorDate,
            Style::Alphabetic => ArchivedStyle::Alphanumeric,
            Style::Ieee => ArchivedStyle::InstituteOfElectricalAndElectronicsEngineers,
            Style::Apa => ArchivedStyle::AmericanPsychologicalAssociation,
        }
    }

    /// The style as a CSL file, for a program that reads CSL itself (pandoc's `--csl`, DOCX-014): the same style the built-in exports format with.
    pub fn csl_xml(self) -> Result<String, String> {
        Csl::Independent(self.csl().clone()).to_xml().map_err(|e| e.to_string())
    }

    fn csl(self) -> &'static IndependentStyle {
        static STYLES: [OnceLock<IndependentStyle>; 5] = [OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new(), OnceLock::new()];
        let slot = Style::ALL.iter().position(|s| *s == self).unwrap_or(0);
        STYLES[slot].get_or_init(|| match self.archived().get() {
            Csl::Independent(style) => style,
            // the five are independent styles; a dependent one would name its parent
            Csl::Dependent(_) => unreachable!("the styles of ADR-0039 §5 are independent"),
        })
    }
}

fn locales() -> &'static [Locale] {
    static LOCALES: OnceLock<Vec<Locale>> = OnceLock::new();
    LOCALES.get_or_init(archive::locales)
}

/// One entry of the list of references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    pub key: String,
    /// the label before the entry as a numeric or alphabetic style sets it (`[1]`, `[Mei20]`), when it is a field of its own
    pub label: Option<String>,
    pub text: String,
}

/// What [`render`] made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rendered {
    /// One text for each group asked for, in the same order; `None` for a group none of whose keys is in the bibliography.
    pub citations: Vec<Option<String>>,
    /// The entries that were cited, in the order of the style (by first citation, or sorted).
    pub references: Vec<Reference>,
}

fn plain(children: &ElemChildren) -> String {
    let mut out = String::new();
    // writing to a String cannot fail
    let _ = children.write_buf(&mut out, BufWriteFormat::Plain);
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A locator that a citation writes after its key (`p. 12`, `S. 3 f.`, `ch. 2`, a bare `12`), as
/// the label and the value; `None` when the text is something else (it is then written as it is).
fn locator(text: &str) -> Option<(Locator, &str)> {
    let t = text.trim();
    let (word, rest) = t.split_once(char::is_whitespace).map_or((t, ""), |(w, r)| (w, r.trim()));
    let kind = match word.trim_end_matches('.').to_lowercase().as_str() {
        "p" | "pp" | "page" | "pages" | "s" | "seite" | "seiten" => Locator::Page,
        "ch" | "chap" | "chapter" | "kap" | "kapitel" => Locator::Chapter,
        "sec" | "sect" | "section" | "abschn" | "abschnitt" | "§" => Locator::Section,
        "fig" | "figure" | "abb" | "abbildung" => Locator::Figure,
        "vol" | "volume" | "bd" | "band" => Locator::Volume,
        // a number or a range alone is a page
        _ => return t.chars().next().is_some_and(|c| c.is_ascii_digit()).then_some((Locator::Page, t)),
    };
    (!rest.is_empty()).then_some((kind, rest))
}

/// `text` (a formatted citation, usually inside `(…)` or `[…]`) with the prefix after the opening
/// bracket and the free suffix before the closing one; outside brackets they stand before and after.
fn wrap(text: &str, prefix: &str, suffix: &str) -> String {
    let mut text = text.to_string();
    let bracketed = text.starts_with('(') && text.ends_with(')') || text.starts_with('[') && text.ends_with(']');
    if !suffix.is_empty() {
        if bracketed {
            text.insert_str(text.len() - 1, &format!(", {suffix}"));
        } else {
            text = format!("{text}, {suffix}");
        }
    }
    if !prefix.is_empty() {
        if bracketed {
            text.insert_str(1, &format!("{prefix} "));
        } else {
            text = format!("{prefix} {text}");
        }
    }
    text
}

/// An item of a group with the entry it cites.
struct Part<'a> {
    item: &'a Item,
    entry: Arc<hayagriva::Entry>,
    locator: Option<(Locator, &'a str)>,
    prose: bool,
}

/// The items of a citation group for the driver: a locator the style knows, the author left out
/// (`[-@key]`: the year), or the form for running text (`@key`).
fn items_of<'a>(group: &'a [Part]) -> Vec<CitationItem<'a, hayagriva::Entry>> {
    group
        .iter()
        .map(|p| {
            let spec = p.locator.map(|(l, v)| SpecificLocator(l, LocatorPayload::Str(v)));
            let purpose = if p.item.suppress_author {
                Some(CitePurpose::Year)
            } else if p.prose {
                Some(CitePurpose::Prose)
            } else {
                None
            };
            CitationItem::new(&*p.entry, spec, None, false, purpose)
        })
        .collect()
}

/// Format the citation groups of a document in reading order, with the entries `entry` finds, as
/// `style` says, with the terms of `lang`. A key that `entry` does not find is left out of its group.
pub fn render(groups: &[&Group], entry: &dyn Fn(&str) -> Option<Arc<hayagriva::Entry>>, style: Style, lang: Lang) -> Rendered {
    let locale = Some(LocaleCode(lang.locale().to_string()));
    let csl = style.csl();
    let parts: Vec<Vec<Part>> = groups
        .iter()
        .map(|g| {
            g.items
                .iter()
                .filter_map(|item| {
                    let Target::Citation(key) = &item.target else { return None };
                    Some(Part { item, entry: entry(key)?, locator: locator(&item.suffix), prose: !g.bracketed })
                })
                .collect()
        })
        .collect();
    let mut driver = BibliographyDriver::new();
    for group in parts.iter().filter(|g| !g.is_empty()) {
        driver.citation(CitationRequest::new(items_of(group), csl, locale.clone(), locales(), None));
    }
    // the entries cited, by first citation: the labels of the alphabetic style are asked for each of them
    let mut cited: Vec<(&str, &Arc<hayagriva::Entry>)> = Vec::new();
    for p in parts.iter().flatten() {
        if let Target::Citation(key) = &p.item.target {
            if !cited.iter().any(|(k, _)| k == key) {
                cited.push((key, &p.entry));
            }
        }
    }
    if style == Style::Alphabetic {
        for (_, e) in &cited {
            driver.citation(CitationRequest::new(vec![CitationItem::with_entry(&***e)], csl, locale.clone(), locales(), None));
        }
    }
    let done = driver.finish(BibliographyRequest { style: csl, locale: locale.clone(), locale_files: locales() });
    let mut rendered = done.citations.iter().map(|c| plain(&c.citation));
    let citations = parts
        .iter()
        .map(|group| {
            let first = group.first()?;
            let text = rendered.next()?;
            // a prefix belongs to the first item and a suffix that is no locator to the last
            // (or a locator the style leaves out)
            let free = group.last().filter(|p| p.locator.is_none_or(|(_, v)| !text.contains(v))).map_or("", |p| p.item.suffix.as_str());
            Some(wrap(&text, &first.item.prefix, free))
        })
        .collect();
    let references = if style == Style::Alphabetic {
        let labels: Vec<String> = rendered.collect();
        alphabetic_references(&cited, &labels, &locale)
    } else {
        done.bibliography
            .map(|b| {
                b.items
                    .iter()
                    .enumerate()
                    .map(|(n, i)| {
                        let label = i.first_field.as_ref().map(|f| plain(&ElemChildren(vec![f.clone()])));
                        // a numeric style numbers the list by the order it is in
                        let label = if style == Style::Numeric { Some(format!("[{}]", n + 1)) } else { label };
                        Reference { key: i.key.clone(), label, text: plain(&i.content) }
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Rendered { citations, references }
}

/// The list of references of the alphabetic style: the entries as Chicago's author-date style
/// writes them, each with its label (`[Knu84]`), sorted by the label.
fn alphabetic_references(cited: &[(&str, &Arc<hayagriva::Entry>)], labels: &[String], locale: &Option<LocaleCode>) -> Vec<Reference> {
    let csl = Style::AuthorYear.csl();
    let mut driver = BibliographyDriver::new();
    for (_, e) in cited {
        driver.citation(CitationRequest::new(vec![CitationItem::with_entry(&***e)], csl, locale.clone(), locales(), None));
    }
    let done = driver.finish(BibliographyRequest { style: csl, locale: locale.clone(), locale_files: locales() });
    let mut out: Vec<Reference> = done
        .bibliography
        .map(|b| {
            b.items
                .iter()
                .map(|i| {
                    let label = cited.iter().position(|(k, _)| *k == i.key).and_then(|n| labels.get(n)).cloned();
                    Reference { key: i.key.clone(), label, text: plain(&i.content) }
                })
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// The summary of an entry for the peek (EDIT-125): authors, year, title and venue in one line.
pub fn summary(e: &Entry) -> String {
    let mut out = match e.authors.as_slice() {
        [] => String::new(),
        [a] => a.clone(),
        [a, b] if !e.et_al => format!("{a} and {b}"),
        [a, ..] => format!("{a} et al."),
    };
    if let Some(y) = e.year {
        out.push_str(&format!(" ({y})"));
    }
    if !e.title.is_empty() {
        out.push_str(&format!(": {}", e.title));
    }
    if !e.venue.is_empty() {
        out.push_str(&format!(". {}", e.venue));
    }
    out.trim_start_matches(": ").trim().to_string()
}
