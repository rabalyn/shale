//! The thesis syntax of ADR-0039 in the Word export (DOCX-014 to DOCX-017, ADR-0041, #650).
//!
//! pandoc's commonmark reader has no citations, so Shale does what the scanner of EDIT-122 decided
//! and writes it into the text pandoc reads: a labelled figure, table or equation has its number
//! and caption written out as the numbering of EDIT-128 gives them, a reference `@fig:x` is the word
//! and the number in the language of the note, and a citation group is a mark that Shale's filter
//! turns into a pandoc citation (`Cite`), which `--citeproc` formats with the note's `.bib` file
//! and the CSL file of the style. The marks are private-use characters followed by a number, and
//! the data they stand for (the words, the keys with their prefixes and locators) is a Lua file
//! next to the filter, so no word of a note can be taken for markup.

use crate::bibliography::format::Style;
use crate::markdown::citations::{self, Item, Kind, Target, Token};
use crate::markdown::labels::Entry;
use crate::markdown::{self, MathSpan};
use crate::thesis::export::Bibliographies;
use crate::thesis::{Lang, Marker, View};
use crate::vault::Vault;
use std::ffi::OsString;
use std::ops::Range;
use std::path::Path;

/// The mark of a citation group: `OPEN_CITE`, its number in the data, `CLOSE`.
pub const OPEN_CITE: char = '\u{E010}';
/// The end of every mark.
pub const CLOSE: char = '\u{E011}';
/// The mark of a text of the data (a reference: "Figure 3").
pub const OPEN_TEXT: char = '\u{E012}';
/// A paragraph that is the caption of a figure; the mark stands for the text of the caption.
pub const OPEN_FIGURE: char = '\u{E013}';
/// The paragraph where the bibliography goes (pandoc's `refs` div).
pub const OPEN_REFS: char = '\u{E014}';
/// A paragraph that is the caption of a table.
pub const OPEN_TABLE: char = '\u{E015}';

/// What pandoc needs besides the text for the thesis syntax of a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Setup {
    /// the language of the note's property `language:`, written as `lang` of the document
    pub lang: Option<Lang>,
    /// the bibliography files as pandoc reads them: the name in the work folder and the bytes (empty without a citation)
    pub bibliography: Vec<(String, Vec<u8>)>,
    /// the CSL style of the citations
    pub csl: Option<Vec<u8>>,
    /// the Lua file the filter reads (`return { cites = …, texts = … }`)
    pub data: String,
}

/// The text pandoc reads and what goes with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub text: String,
    pub setup: Option<Setup>,
    /// what the export has to say: a label used twice, a reference nobody answers, a key the bibliography lacks
    pub warnings: Vec<String>,
}

/// The `--citeproc`, `--bibliography` and `--csl` words and the data of the filter, for the files written to `work`.
pub fn args(setup: &Setup, work: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = Vec::new();
    let mut meta = OsString::from("shale-thesis=");
    meta.push(work.join(DATA_FILE));
    a.extend(["-M".into(), meta]);
    if let Some(lang) = setup.lang {
        a.extend(["-M".into(), format!("lang={}", lang.locale()).into()]);
    }
    if !setup.bibliography.is_empty() {
        a.push("--citeproc".into());
        for (name, _) in &setup.bibliography {
            let mut f = OsString::from("--bibliography=");
            f.push(work.join(name));
            a.push(f);
        }
        if setup.csl.is_some() {
            let mut f = OsString::from("--csl=");
            f.push(work.join(CSL_FILE));
            a.push(f);
        }
    }
    a
}

/// What the export says when pandoc does not know `--citeproc` (it came with pandoc 2.11).
pub const OLD_PANDOC: &str = "This pandoc cannot format citations (--citeproc needs pandoc 2.11 or newer): install a newer pandoc (Settings → Import)";

pub const DATA_FILE: &str = "thesis.lua";
pub const CSL_FILE: &str = "style.csl";

/// One citation of a group as the filter makes a pandoc citation of it.
struct Cite {
    id: String,
    /// `NormalCitation`, `SuppressAuthor` or `AuthorInText`
    mode: &'static str,
    prefix: String,
    suffix: String,
}

#[derive(Default)]
struct Data {
    cites: Vec<Vec<Cite>>,
    texts: Vec<String>,
}

impl Data {
    fn cite(&mut self, group: Vec<Cite>) -> String {
        self.cites.push(group);
        format!("{OPEN_CITE}{}{CLOSE}", self.cites.len())
    }

    fn text(&mut self, open: char, text: &str) -> String {
        self.texts.push(text.to_string());
        format!("{open}{}{CLOSE}", self.texts.len())
    }

    fn used(&self) -> bool {
        !self.cites.is_empty() || !self.texts.is_empty()
    }

    /// The Lua file: lists, counted from 1 as the marks are.
    fn lua(&self) -> String {
        let mut out = String::from("return {\n  cites = {\n");
        for group in &self.cites {
            out.push_str("    {");
            for c in group {
                out.push_str(&format!("{{id = {}, mode = {}, prefix = {}, suffix = {}}}, ", lua_string(&c.id), lua_string(c.mode), lua_string(&c.prefix), lua_string(&c.suffix)));
            }
            out.push_str("},\n");
        }
        out.push_str("  },\n  texts = {\n");
        for t in &self.texts {
            out.push_str(&format!("    {},\n", lua_string(t)));
        }
        out.push_str("  },\n}\n");
        out
    }
}

/// `s` as a Lua string literal.
fn lua_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push_str(&format!("\\u{{{:x}}}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A change of the text: the bytes of `range` become `with`.
struct Edit {
    range: Range<usize>,
    with: String,
}

/// The style of the citations: the `.csl` file the property `citation-style:` names (a vault path, a file name or a
/// wiki link), else the named style of the five (ADR-0039 §5), else `authoryear`, as CSL.
fn csl_of(vault: &Vault, rel: &Path, text: &str, warnings: &mut Vec<String>) -> Option<Vec<u8>> {
    let value = markdown::front_matter_values(text).into_iter().find(|(k, _)| k == "citation-style").and_then(|(_, v)| v.into_iter().next());
    let named = value.map(|v| crate::bibliography::resolve::target_of(&v)).filter(|t| t.to_lowercase().ends_with(".csl"));
    if let Some(target) = named {
        match vault.resolve(&target, Some(rel)).map(|p| vault.abs(&p)).map(crate::fsutil::read) {
            Some(Ok(bytes)) => return Some(bytes),
            Some(Err(e)) => warnings.push(format!("The citation style “{target}” cannot be read ({e}); the style authoryear is used")),
            None => warnings.push(format!("The citation style “{target}” is not a file of the vault; the style authoryear is used")),
        }
    }
    match Style::property(text).unwrap_or_default().csl_xml() {
        Ok(xml) => Some(xml.into_bytes()),
        Err(e) => {
            warnings.push(format!("The citation style cannot be written for pandoc ({e}); pandoc's own style is used"));
            None
        }
    }
}

/// The offsets of the lines of `text` and whether each is inside a fenced block.
struct Lines<'a> {
    lines: Vec<(usize, &'a str)>,
}

impl Lines<'_> {
    fn start(&self, line: usize) -> usize {
        self.lines.get(line).map_or(0, |(off, _)| *off)
    }

    fn end(&self, line: usize) -> usize {
        self.lines.get(line).map_or(0, |(off, l)| off + l.len())
    }
}

/// The text of a note as pandoc reads it: the labels, captions, numbers and references of the thesis syntax
/// written out, the citations marked, the bibliography placed (DOCX-014 to DOCX-016).
pub fn prepare(vault: &Vault, rel: &Path, text: &str) -> Prepared {
    let unchanged = || Prepared { text: text.to_string(), setup: None, warnings: Vec::new() };
    let Some(view) = Bibliographies::new(vault).unformatted(vault, rel, text, None) else { return unchanged() };
    let has = |k: &str| view.bib.has_key(k);
    let tokens = citations::scan(text, view.bib.in_effect().then_some(&has as &dyn Fn(&str) -> bool));
    let mut warnings = view.warnings(text);
    let lines = Lines { lines: markdown::lines_with_offsets(text) };
    let math = markdown::find_math(text);
    let mut data = Data::default();
    let mut edits: Vec<Edit> = Vec::new();
    let mut cited = false;
    for token in &tokens {
        match token {
            Token::Group(g) => {
                let cites: Vec<&Item> = g.items.iter().filter(|i| matches!(i.target, Target::Citation(_))).collect();
                if !cites.is_empty() {
                    // a group with a key the bibliography lacks stays as written; the warning names the key
                    if cites.iter().all(|i| i.known == Some(true)) {
                        cited = true;
                        edits.push(Edit { range: g.range.clone(), with: data.cite(cites.iter().map(|i| cite_of(i, g.bracketed)).collect()) });
                    }
                } else if let Some(shown) = view.reference_text(g) {
                    edits.push(Edit { range: g.range.clone(), with: data.text(OPEN_TEXT, &shown) });
                }
            }
            Token::Label(l) => label_edits(&view, text, &lines, &math, l, &mut data, &mut edits),
        }
    }
    let heading = view.lang.bibliography_heading();
    let refs = format!("# {heading}\n\n{OPEN_REFS}{CLOSE}");
    let mut out = String::new();
    if cited && view.has_bibliography() {
        let fenced = markdown::fenced_code_lines(text);
        let front = markdown::front_matter_lines(text);
        let marker = lines.lines.iter().enumerate().find(|(n, (_, l))| *n >= front && !fenced.get(*n).copied().unwrap_or(false) && Marker::of(l.trim()) == Some(Marker::Bibliography));
        match marker {
            Some((n, _)) => edits.push(Edit { range: lines.start(n)..lines.end(n), with: refs }),
            None => out = format!("{}\n{refs}\n", if text.ends_with('\n') { "" } else { "\n" }),
        }
    }
    let mut written = text.to_string();
    // the last change first, so the offsets of the others hold
    edits.sort_by_key(|e| std::cmp::Reverse((e.range.start, e.range.end)));
    for e in edits {
        written.replace_range(e.range, &e.with);
    }
    written.push_str(&out);
    if !data.used() {
        return Prepared { text: text.to_string(), setup: None, warnings };
    }
    let (bibliography, csl) = if cited { bibliography_of(vault, rel, text, &view, &mut warnings) } else { (Vec::new(), None) };
    let lang = Lang::property(text);
    Prepared { text: written, setup: Some(Setup { lang, bibliography, csl, data: data.lua() }), warnings }
}

/// The files written to the work folder: the name and the bytes.
type Files = Vec<(String, Vec<u8>)>;

/// The bibliography files of the view with their bytes, and the style.
fn bibliography_of(vault: &Vault, rel: &Path, text: &str, view: &View, warnings: &mut Vec<String>) -> (Files, Option<Vec<u8>>) {
    let mut files = Vec::new();
    for (file, _) in &view.bib.files {
        match crate::fsutil::read(vault.abs(file)) {
            Ok(bytes) => files.push((format!("bibliography-{}.bib", files.len() + 1), bytes)),
            Err(e) => warnings.push(format!("The bibliography “{}” cannot be read ({e})", file.display())),
        }
    }
    let csl = if files.is_empty() { None } else { csl_of(vault, rel, text, warnings) };
    (files, csl)
}

/// One item of a citation group for pandoc: the locator after a comma, as pandoc reads `[@key, p. 12]`.
fn cite_of(item: &Item, bracketed: bool) -> Cite {
    let Target::Citation(id) = &item.target else { unreachable!("only citations are asked") };
    let mode = if !bracketed {
        "AuthorInText"
    } else if item.suppress_author {
        "SuppressAuthor"
    } else {
        "NormalCitation"
    };
    // `@key [p. 12]` has its locator as it is; in a bracket pandoc keeps the comma in front of it
    let suffix = match (bracketed, item.suffix.is_empty()) {
        (_, true) => String::new(),
        (true, false) => format!(", {}", item.suffix),
        (false, false) => item.suffix.clone(),
    };
    Cite { id: id.clone(), mode, prefix: item.prefix.clone(), suffix }
}

/// The changes a label asks for: it goes, and the figure, table, equation or heading it belongs to gets
/// its caption, its number or its number in front of the title (DOCX-015).
fn label_edits(view: &View, text: &str, lines: &Lines, math: &[MathSpan], label: &citations::Label, data: &mut Data, edits: &mut Vec<Edit>) {
    // the label with the space in front of it
    let start = label.range.start - usize::from(text[..label.range.start].ends_with(' '));
    let gone = Edit { range: start..label.range.end, with: String::new() };
    let Some(entry) = view.labels.entries.iter().find(|e| e.source == view.index && e.range == label.range) else {
        // a label that an earlier one has: it is no caption and has no number
        edits.push(gone);
        return;
    };
    match entry.kind {
        Kind::Figure => {
            edits.push(gone);
            let mark = data.text(OPEN_FIGURE, &view.caption_text(entry));
            if view.caption_above(Kind::Figure) {
                edits.push(Edit { range: lines.start(entry.line)..lines.start(entry.line), with: format!("{mark}\n\n") });
            } else {
                edits.push(Edit { range: lines.end(entry.line)..lines.end(entry.line), with: format!("\n\n{mark}") });
            }
        }
        Kind::Table => {
            // the caption line `: Caption {#tbl:x}` is the caption, above or below the table
            let mark = data.text(OPEN_TABLE, &view.caption_text(entry));
            let line = lines.start(entry.line)..lines.end(entry.line);
            if view.caption_above(Kind::Table) {
                let first = (0..entry.line).rev().take_while(|n| lines.lines[*n].1.contains('|')).last().unwrap_or(entry.line);
                edits.push(Edit { range: lines.start(first)..lines.start(first), with: format!("{mark}\n\n") });
                edits.push(Edit { range: line, with: String::new() });
            } else {
                edits.push(Edit { range: line, with: format!("\n{mark}") });
            }
        }
        Kind::Equation => equation_edits(entry, label, text, math, gone, edits),
        Kind::Section => {
            edits.push(gone);
            if let Some(n) = &entry.number {
                let (off, line) = lines.lines[entry.line];
                let lead = line.len() - line.trim_start().len();
                let hashes = line[lead..].len() - line[lead..].trim_start_matches('#').len();
                let after = lead + hashes + (line[lead + hashes..].len() - line[lead + hashes..].trim_start().len());
                edits.push(Edit { range: off + after..off + after, with: format!("{n} ") });
            }
        }
        Kind::Abbreviation | Kind::Glossary => {}
    }
}

/// A numbered display formula ends in `\qquad (N)`, which Word sets at the right of the formula; a labelled
/// ```` ```math ```` block is a `$$` block for pandoc, which has no math fences.
fn equation_edits(entry: &Entry, label: &citations::Label, text: &str, math: &[MathSpan], gone: Edit, edits: &mut Vec<Edit>) {
    let Some(number) = &entry.number else { return edits.push(gone) };
    let tail = format!(" \\qquad ({number})");
    let fence = math.iter().find(|m| m.display && m.range.contains(&label.range.start) && text[m.range.clone()].starts_with("```"));
    if let Some(m) = fence {
        edits.push(Edit { range: m.range.clone(), with: format!("$$\n{}{tail}\n$$", text[m.tex.clone()].trim_end()) });
        return;
    }
    edits.push(gone);
    if let Some(m) = math.iter().find(|m| m.display && m.range.end <= label.range.start && text[m.range.end..label.range.start].trim().is_empty()) {
        edits.push(Edit { range: m.tex.end..m.tex.end, with: tail });
    }
}
