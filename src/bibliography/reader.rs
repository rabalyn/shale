//! Reading a `.bib` file into the index of its keys (EDIT-123, ADR-0039 §5 and §6).
//!
//! The reading itself is the `biblatex` crate (Typst's, the part of `hayagriva` that reads BibLaTeX):
//! it resolves `@string`, `crossref`, `{\"u}` escapes and `date` against `year`/`month`. What it
//! lacks is to survive a bad entry: one broken entry fails the whole file. So a file that does not
//! parse as a whole is cut at the lines that start an entry, each entry is read on its own (with
//! the `@string`s of the file), the ones that fail are skipped and counted, and what is left is
//! read once more as a whole so that `crossref` is resolved between the entries that remain.

use crate::markdown::citations::{key_len, reserved_prefix};
use biblatex::{Bibliography, ChunksExt, DateValue, PermissiveType};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

/// Why a key of the file cannot be cited (ADR-0039 §1, §4 rule 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Problem {
    /// the key starts with `fig:`, `tbl:`, `eq:`, `sec:`, `abbr:` or `gls:`, which always make a reference
    ReservedPrefix,
    /// the key holds a character the key grammar does not allow, or is empty
    OutsideGrammar,
}

impl Problem {
    pub fn says(self) -> &'static str {
        match self {
            Problem::ReservedPrefix => "its key starts with a prefix that Shale reads as a reference (fig:, tbl:, eq:, sec:, abbr:, gls:)",
            Problem::OutsideGrammar => "its key has characters that a citation cannot hold",
        }
    }
}

/// One entry of a `.bib` file, as far as the key index needs it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    /// the entry type in lower case (`article`, `book`, `inproceedings`)
    pub kind: String,
    /// the family names of the authors (of the editors when there are no authors), in order
    pub authors: Vec<String>,
    /// `and others` ended the list of names
    pub et_al: bool,
    /// the year of `date`, or of `year`
    pub year: Option<i32>,
    /// the title as plain text: braces and LaTeX escapes resolved
    pub title: String,
    /// set when the key cannot be cited
    pub problem: Option<Problem>,
}

/// What a `.bib` file holds: its entries by key, and how many entries were skipped as bad.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileIndex {
    /// in the order of the keys
    pub entries: Vec<Entry>,
    pub skipped: usize,
    /// the modified time of the file when it was read (a later one means: read again)
    pub modified: Option<SystemTime>,
}

impl FileIndex {
    pub fn get(&self, key: &str) -> Option<&Entry> {
        self.entries.binary_search_by(|e| e.key.as_str().cmp(key)).ok().map(|i| &self.entries[i])
    }
}

/// The entries of a source text and the number of entries that could not be read.
#[derive(Debug)]
pub struct Parsed {
    pub entries: Vec<Entry>,
    pub skipped: usize,
}

/// Read the text of a `.bib` file: all of it at once when it is whole, else entry by entry.
pub fn parse(src: &str) -> Parsed {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    if let Ok(bib) = Bibliography::parse(src) {
        return Parsed { entries: bib.iter().map(entry_of).collect(), skipped: 0 };
    }
    parse_piecewise(src)
}

/// The slices of the text that start with `@` at the start of a line (blanks allowed before it).
fn pieces(src: &str) -> Vec<&str> {
    let mut starts = Vec::new();
    let mut at = 0;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with('@') && t[1..].starts_with(|c: char| c.is_ascii_alphabetic()) {
            starts.push(at);
        }
        at += line.len();
    }
    starts.iter().enumerate().map(|(i, &s)| &src[s..starts.get(i + 1).copied().unwrap_or(src.len())]).collect()
}

fn parse_piecewise(src: &str) -> Parsed {
    let (mut strings, mut good) = (String::new(), String::new());
    let mut candidates = Vec::new();
    for piece in pieces(src) {
        let word: String = piece[1..].chars().take_while(char::is_ascii_alphabetic).collect::<String>().to_ascii_lowercase();
        match word.as_str() {
            "string" => {
                strings.push_str(piece);
                strings.push('\n');
            }
            "preamble" | "comment" => {}
            _ => candidates.push(piece),
        }
    }
    let (mut skipped, mut seen) = (0, HashSet::new());
    let mut alone = Vec::new();
    for piece in candidates {
        match Bibliography::parse(&format!("{strings}\n{piece}")) {
            Ok(bib) if !bib.is_empty() => {
                // a key that an earlier entry has is a duplicate: the first one stays
                if bib.iter().all(|e| !seen.contains(&e.key)) {
                    seen.extend(bib.iter().map(|e| e.key.clone()));
                    alone.extend(bib.iter().map(entry_of));
                    good.push_str(piece);
                    good.push('\n');
                } else {
                    skipped += 1;
                }
            }
            _ => skipped += 1,
        }
    }
    // the entries that are left, read as one so that `crossref` finds its parent (an entry whose
    // parent was skipped keeps what it has itself)
    let entries = match Bibliography::parse(&format!("{strings}\n{good}")) {
        Ok(bib) => bib.iter().map(entry_of).collect(),
        Err(_) => alone,
    };
    Parsed { entries, skipped }
}

fn entry_of(e: &biblatex::Entry) -> Entry {
    let mut people = e.author().unwrap_or_default();
    if people.is_empty() {
        people = e.editors().ok().and_then(|v| v.into_iter().next()).map(|(p, _)| p).unwrap_or_default();
    }
    let et_al = people.last().is_some_and(|p| p.name.eq_ignore_ascii_case("others") && p.given_name.is_empty());
    let authors = people.iter().filter(|p| !(p.name.eq_ignore_ascii_case("others") && p.given_name.is_empty())).map(|p| if p.name.is_empty() { p.given_name.clone() } else { p.name.clone() }).filter(|n| !n.is_empty()).collect();
    let year = match e.date() {
        Ok(PermissiveType::Typed(d)) => Some(match d.value {
            DateValue::At(t) | DateValue::After(t) | DateValue::Before(t) | DateValue::Between(t, _) => t.year,
        }),
        // a date that is written out ("circa 1990"): the first four digits
        Ok(PermissiveType::Chunks(c)) => year_in(&c.format_verbatim()),
        Err(_) => None,
    };
    let title = e.title().map(|t| t.format_verbatim()).unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ");
    let problem = if reserved_prefix(&e.key).is_some() {
        Some(Problem::ReservedPrefix)
    } else if key_len(&e.key) != e.key.len() || e.key.is_empty() {
        Some(Problem::OutsideGrammar)
    } else {
        None
    };
    Entry { key: e.key.clone(), kind: e.entry_type.to_string(), authors, et_al, year, title, problem }
}

/// The first run of four digits.
fn year_in(s: &str) -> Option<i32> {
    let digits: Vec<(usize, char)> = s.char_indices().collect();
    digits.windows(4).find(|w| w.iter().all(|(_, c)| c.is_ascii_digit())).and_then(|w| s[w[0].0..w[3].0 + 1].parse().ok())
}

/// Read a file into its index. `Err` is the sentence for the person: the file cannot be read, or
/// holds no entry (so it is no bibliography).
pub fn read_file(path: &Path, modified: Option<SystemTime>) -> Result<Arc<FileIndex>, String> {
    let bytes = crate::fsutil::read(path).map_err(|e| format!("cannot be read: {e}"))?;
    let parsed = parse(&String::from_utf8_lossy(&bytes));
    if parsed.entries.is_empty() {
        return Err(if parsed.skipped > 0 { format!("holds no entry that can be read ({} skipped)", parsed.skipped) } else { "holds no entries (is it a BibTeX or BibLaTeX file?)".into() });
    }
    let mut entries = parsed.entries;
    entries.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(Arc::new(FileIndex { entries, skipped: parsed.skipped, modified }))
}
