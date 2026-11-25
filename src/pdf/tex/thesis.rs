//! The thesis syntax of ADR-0039 in the TeX export (TEX-044 to TEX-047, #647): citations as
//! `biblatex` commands, labelled figures, tables, equations and sections with `\label`, references
//! as `\cref`, the bibliography and the lists of figures and tables, and the preamble that makes all
//! of that work (`caption`, `cleveref`, `biblatex` with biber). The scanner of EDIT-122 decides
//! what is a citation or a reference; this module only writes what it found.
//!
//! One [`Thesis`] belongs to one export (a note or a book): it knows the bibliography in effect,
//! the labels of the document, the language, the style and where the captions stand, and it notes
//! what the text used so that the preamble asks for exactly the packages that are needed.

use super::blocks::Marker;
use super::sets::{tex_escape, KindManifest, Language, Mapping, SetInfo};
use crate::bibliography::format::Style;
use crate::bibliography::{reader, resolve, Effect};
use crate::markdown;
use crate::markdown::citations::{self, Group, Kind, Target, Token};
use crate::markdown::labels::{Labels, Level, Numbering};
use crate::pdf::export::ExportOptions;
use crate::thesis::view::CaptionAt;
use crate::thesis::Lang;
use crate::vault::Vault;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::path::Path;

/// The choices of the export dialog for the thesis syntax (TEX-047), carried by [`ExportOptions`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choices {
    /// *Bibliography*: print the list of references (when one is in effect).
    pub bibliography: bool,
    /// *List of figures*, *List of tables*.
    pub list_of_figures: bool,
    pub list_of_tables: bool,
    /// The style of the dialog; none: the note's `citation-style:`, then the set's, then `authoryear`.
    pub style: Option<Style>,
    /// The vault's default bibliography (Settings), for a note that names none.
    pub default_bibliography: String,
}

impl Default for Choices {
    fn default() -> Self {
        Choices { bibliography: true, list_of_figures: false, list_of_tables: false, style: None, default_bibliography: String::new() }
    }
}

/// The bibliography in effect: what was read of the files, and the files themselves, which travel
/// in the work folder for biber.
pub struct Bib {
    effect: Effect,
    /// the name in the work folder and the bytes
    files: Vec<(String, Vec<u8>)>,
}

/// Read the bibliography of a note or a book's top folder note (`top`: its path and text), else the
/// vault's default, now and in this thread (an export reads a few files). `None` when no file could
/// be read; the sentences say why.
pub fn load_bib(vault: &Vault, top: Option<(&Path, &str)>, default: &str) -> (Option<Bib>, Vec<String>) {
    let wanted = resolve::wanted(vault, top, default);
    let mut messages = wanted.problems;
    let mut effect = Effect::default();
    let mut files = Vec::new();
    for rel in &wanted.files {
        let abs = vault.abs(rel);
        let modified = vault.file(rel).and_then(|f| f.modified);
        match (reader::read_file(&abs, modified), crate::fsutil::read(&abs)) {
            (Ok(index), Ok(bytes)) => {
                effect.files.push((rel.clone(), index));
                files.push((format!("bibliography-{}.bib", files.len() + 1), bytes));
            }
            (Err(why), _) => messages.push(format!("The bibliography “{}” {why}: no bibliography is in effect from it", rel.display())),
            (_, Err(e)) => messages.push(format!("The bibliography “{}” cannot be read ({e}): no bibliography is in effect from it", rel.display())),
        }
    }
    (effect.in_effect().then_some(Bib { effect, files }), messages)
}

/// The biblatex style of a named style.
const fn biblatex_style(s: Style) -> &'static str {
    match s {
        Style::Numeric => "numeric",
        Style::AuthorYear => "authoryear",
        Style::Alphabetic => "alphabetic",
        Style::Ieee => "ieee",
        Style::Apa => "apa",
    }
}

/// What a text written for TeX has used of the thesis syntax, and what it needs for it.
pub struct Thesis {
    pub lang: Lang,
    pub style: Style,
    figure_caption: CaptionAt,
    table_caption: CaptionAt,
    numbering: Numbering,
    /// the document is a book: what does not resolve is said by [`Thesis::book_warnings`], naming the notes
    book: bool,
    /// a chapter class (`\chapter`) rather than an article class
    chapters: bool,
    /// the kind has floats and sections to label: the article and the book family
    floats: bool,
    labels: Labels,
    bib: Option<Bib>,
    choices: Choices,
    /// the language of the set's class when the document's is another one
    babel_differs: bool,
    /// what the markers of the text ask for
    markers: Vec<Marker>,
    written: RefCell<HashSet<(Kind, String)>>,
    warnings: RefCell<Vec<String>>,
    cited: Cell<bool>,
    refs_used: Cell<bool>,
    numbered_used: Cell<bool>,
    floats_used: Cell<bool>,
}

/// What a [`Thesis`] is made from.
pub struct Input<'a> {
    /// the note, or a book's top folder note: its properties are the document's
    pub top: &'a str,
    pub opts: &'a ExportOptions,
    pub set: &'a SetInfo,
    pub kind: &'a KindManifest,
    pub book: bool,
    pub labels: Labels,
    pub bib: Option<Bib>,
    /// the sentences of [`load_bib`], which are warnings of the export
    pub bib_messages: Vec<String>,
    pub markers: Vec<Marker>,
}

/// The position a property or a manifest value says.
fn position(word: &str) -> Option<CaptionAt> {
    match word.trim().to_lowercase().as_str() {
        "above" | "top" => Some(CaptionAt::Above),
        "below" | "bottom" => Some(CaptionAt::Below),
        _ => None,
    }
}

/// Where a caption stands: the property of the note or top folder note, else the kind's manifest
/// key, else the set's, else the built-in default (ADR-0039 §3).
fn caption_at(top: &str, property: &str, kind: &str, set: &str, default: CaptionAt) -> CaptionAt {
    let own = markdown::front_matter_values(top).into_iter().find(|(k, _)| k == property).and_then(|(_, v)| v.first().and_then(|w| position(w)));
    own.or_else(|| position(kind)).or_else(|| position(set)).unwrap_or(default)
}

impl Thesis {
    pub fn new(i: Input) -> Thesis {
        let set_lang = match i.set.settings.language {
            Language::De => Lang::De,
            Language::En => Lang::En,
        };
        let lang = Lang::property(i.top).unwrap_or(set_lang);
        let style = i.opts.thesis.style.or_else(|| Style::property(i.top)).or_else(|| Style::named(&i.set.settings.citation_style)).unwrap_or_default();
        let (k, s) = (i.kind, &i.set.settings);
        let numbering = Numbering::property(i.top).unwrap_or(if i.book { Numbering::PerChapter } else { Numbering::Continuous });
        let mut warnings = i.bib_messages;
        // a book says it with the names of its notes (BOOK-020, `Thesis::book_warnings`)
        if !i.book {
            for d in &i.labels.duplicates {
                warnings.push(format!("The label {{#{}:{}}} is used twice: the first one counts", d.kind.prefix(), d.id));
            }
            for u in i.labels.unresolved.iter().filter(|u| u.kind.is_labelled()) {
                warnings.push(format!("The reference @{}:{} has no label: it stays text", u.kind.prefix(), u.id));
            }
        }
        Thesis {
            lang,
            style,
            figure_caption: caption_at(i.top, "figure-caption", &k.figure_caption, &s.figure_caption, CaptionAt::Below),
            table_caption: caption_at(i.top, "table-caption", &k.table_caption, &s.table_caption, CaptionAt::Above),
            numbering,
            book: i.book,
            chapters: k.mapping == Mapping::Book,
            floats: matches!(k.mapping, Mapping::Article | Mapping::Book),
            labels: i.labels,
            bib: i.bib,
            choices: i.opts.thesis.clone(),
            babel_differs: lang != set_lang,
            markers: i.markers,
            written: RefCell::default(),
            warnings: RefCell::new(warnings),
            cited: Cell::new(false),
            refs_used: Cell::new(false),
            numbered_used: Cell::new(false),
            floats_used: Cell::new(false),
        }
    }

    /// Whether figures and tables are floats with a caption and a label in this kind.
    pub fn has_floats(&self) -> bool {
        self.floats
    }

    pub fn caption_at(&self, kind: Kind) -> CaptionAt {
        if kind == Kind::Table {
            self.table_caption
        } else {
            self.figure_caption
        }
    }

    fn warn(&self, text: String) {
        let mut w = self.warnings.borrow_mut();
        if !w.contains(&text) {
            w.push(text);
        }
    }

    /// What a book has to say about the thesis syntax of its notes (BOOK-020, BOOK-021), naming
    /// them: a label in two notes, a reference no note answers, a key the bibliography lacks, a
    /// chapter with a bibliography of its own. The messages of the bibliography files are in [`Thesis::warnings`].
    pub fn book_warnings(&self, notes: &[crate::thesis::book::Note]) -> Vec<String> {
        let none = Effect::default();
        crate::thesis::book::warnings(notes, &self.labels, self.bib.as_ref().map_or(&none, |b| &b.effect))
    }

    /// What the export has to say about the thesis syntax.
    pub fn warnings(&self) -> Vec<String> {
        self.warnings.borrow().clone()
    }

    /// The `.bib` files for the work folder, when the document uses them.
    pub fn bib_files(&self) -> Vec<(String, Vec<u8>)> {
        match &self.bib {
            Some(b) if self.needs_biblatex() => b.files.clone(),
            _ => Vec::new(),
        }
    }

    fn known(&self, kind: Kind, id: &str) -> bool {
        (self.floats || kind == Kind::Equation) && kind.is_labelled() && self.labels.get(kind, id).is_some()
    }

    /// `\label{kind:id}` the first time a label is written, nothing for a label that was written
    /// already or that the document does not know (the first wins).
    pub fn label(&self, kind: Kind, id: &str) -> Option<String> {
        if !self.known(kind, id) || !self.written.borrow_mut().insert((kind, id.to_string())) {
            return None;
        }
        self.numbered_used.set(true);
        Some(format!("\\label{{{}:{id}}}", kind.prefix()))
    }

    /// The label a heading line ends with (`## Method {#sec:method}`) as `\label`, and the text
    /// without it.
    pub fn heading(&self, text: &str) -> (String, String) {
        let id = text.rfind("{#").and_then(|at| citations::parse_label(&text[at..]).filter(|(kind, _, len)| *kind == Kind::Section && at + len == text.trim_end().len()).map(|(_, id, _)| id));
        let label = id.and_then(|id| self.label(Kind::Section, &id)).unwrap_or_default();
        (crate::markdown::labels::without_labels(text), label)
    }

    /// That a figure or table float was written (the preamble then sets up `caption`).
    pub fn float_written(&self) {
        self.floats_used.set(true);
        self.numbered_used.set(true);
    }

    // ------------------------------------------------------------ citations and references

    /// The line with the groups that this module writes (a citation, a reference) taken out and
    /// put in as private marks, and the TeX each mark stands for. Everything else of the line is
    /// left to the writer of inline text.
    pub fn substitute(&self, line: &str) -> (String, Vec<String>) {
        let clean: String = line.chars().filter(|c| !matches!(*c, MARK_OPEN | MARK_CLOSE)).collect();
        if !clean.contains('@') {
            return (clean, Vec::new());
        }
        let has = |k: &str| self.bib.as_ref().is_some_and(|b| b.effect.has_key(k));
        let pred: Option<&dyn Fn(&str) -> bool> = self.bib.is_some().then_some(&has);
        let mut out = String::new();
        let mut tex = Vec::new();
        let mut at = 0;
        for token in citations::scan(&clean, pred) {
            let Token::Group(g) = token else { continue };
            if g.range.start < at {
                continue;
            }
            let Some(written) = self.group(&g) else { continue };
            out.push_str(&clean[at..g.range.start]);
            out.push_str(&format!("{MARK_OPEN}{}{MARK_CLOSE}", tex.len()));
            tex.push(written);
            at = g.range.end;
        }
        out.push_str(&clean[at..]);
        (out, tex)
    }

    /// The TeX of a group; none when it stays text.
    fn group(&self, g: &Group) -> Option<String> {
        if g.items.iter().all(|i| matches!(i.target, Target::Citation(_))) {
            return self.cite(g);
        }
        if g.items.iter().all(|i| matches!(&i.target, Target::Reference { kind, .. } if kind.is_labelled())) {
            return self.refer(g);
        }
        None
    }

    fn cite(&self, g: &Group) -> Option<String> {
        if self.bib.is_none() {
            if !self.book {
                self.warn("The text has citations and no bibliography is in effect: they are written as text (add the property bibliography: to the note)".into());
            }
            return None;
        }
        let keys: Vec<(&citations::Item, &str)> = g.items.iter().filter_map(|i| if let Target::Citation(k) = &i.target { Some((i, k.as_str())) } else { None }).collect();
        let unknown: Vec<&str> = keys.iter().filter(|(i, _)| i.known != Some(true)).map(|(_, k)| *k).collect();
        if !unknown.is_empty() {
            if !self.book {
                self.warn(format!("The bibliography has no entry for {}: the citation is written as text", unknown.join(", ")));
            }
            return None;
        }
        self.cited.set(true);
        let notes = |i: &citations::Item| -> String {
            let (p, s) = (note(&i.prefix), note(&i.suffix));
            match (p.is_empty(), s.is_empty()) {
                (true, true) => String::new(),
                (true, false) => format!("[{s}]"),
                (false, true) => format!("[{p}][]"),
                (false, false) => format!("[{p}][{s}]"),
            }
        };
        Some(match keys.as_slice() {
            [(i, key)] if !g.bracketed => format!("\\textcite{}{{{key}}}", notes(i)),
            [(i, key)] => format!("\\autocite{}{}{{{key}}}", if i.suppress_author { "*" } else { "" }, notes(i)),
            many => format!("\\autocites{}", many.iter().map(|(i, key)| format!("{}{{{key}}}", notes(i))).collect::<String>()),
        })
    }

    fn refer(&self, g: &Group) -> Option<String> {
        let mut ids = Vec::new();
        for i in &g.items {
            let Target::Reference { kind, id } = &i.target else { return None };
            if !self.known(*kind, id) {
                return None;
            }
            ids.push(format!("{}:{id}", kind.prefix()));
        }
        self.refs_used.set(true);
        let alone = g.items.iter().all(|i| i.suppress_author);
        Some(format!("\\{}{{{}}}", if alone { "labelcref" } else { "cref" }, ids.join(",")))
    }

    /// The text of a mark put in by [`Thesis::substitute`] back as TeX.
    pub fn restore(written: String, tex: &[String]) -> String {
        if tex.is_empty() {
            return written;
        }
        let mut out = String::with_capacity(written.len());
        let mut rest = written.as_str();
        while let Some(open) = rest.find(MARK_OPEN) {
            out.push_str(&rest[..open]);
            let after = &rest[open + MARK_OPEN.len_utf8()..];
            match after.find(MARK_CLOSE).and_then(|c| after[..c].parse::<usize>().ok().map(|n| (n, c))) {
                Some((n, c)) if n < tex.len() => {
                    out.push_str(&tex[n]);
                    rest = &after[c + MARK_CLOSE.len_utf8()..];
                }
                _ => rest = after,
            }
        }
        out.push_str(rest);
        out
    }

    // ------------------------------------------------------------ the bibliography and the lists

    fn needs_biblatex(&self) -> bool {
        self.bib.is_some() && (self.cited.get() || self.markers.contains(&Marker::Bibliography))
    }

    /// `\printbibliography` as a chapter that is in the table of contents, with the heading of the
    /// document's language.
    pub fn printbibliography(&self) -> String {
        if self.bib.is_none() {
            return String::new();
        }
        format!("\\printbibliography[heading=bibintoc,title={{{}}}]\n\n", tex_escape(self.lang.bibliography_heading()))
    }

    /// The bibliography at the end of the text, unless a marker puts it elsewhere or the dialog's
    /// box is off or nothing was cited.
    pub fn bibliography_at_end(&self) -> String {
        if self.choices.bibliography && self.floats && self.cited.get() && !self.markers.contains(&Marker::Bibliography) {
            self.printbibliography()
        } else {
            String::new()
        }
    }

    /// `\listoffigures` and `\listoftables` after the contents: the boxes of the dialog, for the
    /// lists that no marker places and that have something to list.
    pub fn lists(&self) -> String {
        let has = |k: Kind| self.floats && self.labels.entries.iter().any(|e| e.kind == k);
        let mut out = String::new();
        if self.choices.list_of_figures && has(Kind::Figure) && !self.markers.contains(&Marker::ListOfFigures) {
            out.push_str("\\listoffigures\n");
        }
        if self.choices.list_of_tables && has(Kind::Table) && !self.markers.contains(&Marker::ListOfTables) {
            out.push_str("\\listoftables\n");
        }
        out
    }

    /// What a marker line writes.
    pub fn marker(&self, m: Marker) -> String {
        match m {
            Marker::Bibliography if self.choices.bibliography && self.floats => self.printbibliography(),
            Marker::ListOfFigures if self.floats => "\\listoffigures\n\n".into(),
            Marker::ListOfTables if self.floats => "\\listoftables\n\n".into(),
            _ => String::new(),
        }
    }

    // ------------------------------------------------------------ the preamble

    /// The lines of the preamble for what the text used: after the class and its helpers (so after
    /// `hyperref`, which `cleveref` needs to follow).
    pub fn preamble(&self) -> String {
        let mut t = String::new();
        if self.needs_biblatex() {
            if let Some(b) = &self.bib {
                let language = if self.babel_differs { format!(",language={}", if self.lang == Lang::De { "ngerman" } else { "english" }) } else { String::new() };
                t.push_str(&format!("\\IfPackageLoadedTF{{csquotes}}{{}}{{\\usepackage{{csquotes}}}}\n\\IfPackageLoadedTF{{biblatex}}{{}}{{\\usepackage[backend=biber,style={}{language}]{{biblatex}}}}\n", biblatex_style(self.style)));
                for (name, _) in &b.files {
                    t.push_str(&format!("\\addbibresource{{{name}}}\n"));
                }
            }
        }
        if self.floats_used.get() {
            let at = |a: CaptionAt| if a == CaptionAt::Above { "top" } else { "bottom" };
            t.push_str(&format!("\\IfPackageLoadedTF{{caption}}{{}}{{\\usepackage{{caption}}}}\n\\captionsetup[table]{{position={}}}\\captionsetup[figure]{{position={}}}\n", at(self.table_caption), at(self.figure_caption)));
            // the spaces KOMA-Script leaves around a caption
            let koma = |above: bool, what: &str| format!("\\KOMAoptions{{captions={what}{}}}", if above { "heading" } else { "signature" });
            t.push_str(&format!("\\ifdefined\\KOMAoptions{}{}\\fi\n", koma(self.table_caption == CaptionAt::Above, "table"), koma(self.figure_caption == CaptionAt::Above, "figure")));
        }
        if self.numbered_used.get() {
            // continuous or per chapter (ADR-0039 §3): a chapter class counts per chapter, an article continuously
            let within = if self.chapters { "chapter" } else { "section" };
            for counter in ["figure", "table", "equation"] {
                match (self.numbering, self.chapters) {
                    (Numbering::Continuous, true) => t.push_str(&format!("\\counterwithout{{{counter}}}{{chapter}}\n")),
                    (Numbering::PerChapter, false) => t.push_str(&format!("\\counterwithin{{{counter}}}{{{within}}}\n")),
                    _ => {}
                }
            }
        }
        if self.refs_used.get() {
            // the language of cleveref sets the words between numbers ("und", "and"); the names follow below
            let language = if self.lang == Lang::De { "ngerman" } else { "english" };
            t.push_str(&format!("\\IfPackageLoadedTF{{cleveref}}{{}}{{\\usepackage[{language}]{{cleveref}}}}\n"));
            t.push_str(&self.cref_names());
        }
        t
    }

    /// `\crefname` of every kind in the language of the document, and the words between numbers.
    fn cref_names(&self) -> String {
        use crate::thesis::word;
        let names: [(&str, Kind, Level); 8] = [
            ("figure", Kind::Figure, Level::Section),
            ("table", Kind::Table, Level::Section),
            ("equation", Kind::Equation, Level::Section),
            ("chapter", Kind::Section, Level::Chapter),
            ("section", Kind::Section, Level::Section),
            ("subsection", Kind::Section, Level::Section),
            ("subsubsection", Kind::Section, Level::Section),
            ("appendix", Kind::Section, Level::Appendix),
        ];
        let mut t = String::new();
        for (counter, kind, level) in names {
            let (one, many) = (word(kind, level, false, self.lang), word(kind, level, true, self.lang));
            t.push_str(&format!("\\crefname{{{counter}}}{{{one}}}{{{many}}}\\Crefname{{{counter}}}{{{one}}}{{{many}}}\n"));
        }
        t
    }
}

/// The marks that stand for a citation or reference while a line is turned into TeX: two characters of
/// the Unicode private use area around the number of the TeX.
const MARK_OPEN: char = '\u{E000}';
const MARK_CLOSE: char = '\u{E001}';

/// A prefix or locator as an optional argument: escaped text, in braces when it holds a `]`.
fn note(text: &str) -> String {
    let t = tex_escape(text.trim());
    if t.contains(']') {
        format!("{{{t}}}")
    } else {
        t
    }
}
