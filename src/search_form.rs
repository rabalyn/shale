//! The filter and sort form (QUERY-021…QUERY-027): one widget that edits a query text, and
//! beside it the sort text of a lane, for the boards, the sidebar search and the picker. The
//! rows are tag, property, due, blocked, blocking, orphan, no backlinks, folder, link and text, each with a "not"
//! switch (QUERY-021), joined by All of or Any of (QUERY-022). The form always writes its text; a
//! text opens in the form only when it is one level of AND or OR of such rows, else the form says
//! so and leaves it as it is (QUERY-025).
//!
//! This file is the model: [`Row`], the writer ([`State::write`]) and the reader
//! ([`State::sync`]). The window of it is [`view`], the sort is in [`sort`].

mod complete;
pub mod sort;
mod view;
pub mod vocab;

pub use complete::list_open;
pub use view::{show, Opts};

use crate::search_all::query::{Checked, Cmp, Due, Expr, Term, TermKind};

/// How the rows are joined (QUERY-022).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// AND
    #[default]
    All,
    /// OR
    Any,
}

impl Mode {
    fn word(self) -> &'static str {
        match self {
            Mode::All => " AND ",
            Mode::Any => " OR ",
        }
    }
}

/// How `due:` compares with its day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DueCmp {
    /// before today, no day
    Overdue,
    On,
    Before,
    OnOrBefore,
    After,
    OnOrAfter,
}

impl DueCmp {
    pub const ALL: [DueCmp; 6] = [DueCmp::Overdue, DueCmp::OnOrBefore, DueCmp::Before, DueCmp::On, DueCmp::After, DueCmp::OnOrAfter];

    /// What comes before the day in the query.
    fn op(self) -> &'static str {
        match self {
            DueCmp::Overdue | DueCmp::On => "",
            DueCmp::Before => "<",
            DueCmp::OnOrBefore => "<=",
            DueCmp::After => ">",
            DueCmp::OnOrAfter => ">=",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            DueCmp::Overdue => "overdue",
            DueCmp::On => "on",
            DueCmp::Before => "before",
            DueCmp::OnOrBefore => "within",
            DueCmp::After => "after",
            DueCmp::OnOrAfter => "on or after",
        }
    }
}

/// What one row asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum What {
    /// `#tag`
    Tag(String),
    /// `key:value`
    Prop {
        key: String,
        value: String,
    },
    /// `due:`; the day is as typed (`2026-12-01`, `today`, `+7d`), none for overdue
    Due {
        cmp: DueCmp,
        day: String,
    },
    Blocked,
    Blocking,
    /// `orphan`
    Orphan,
    /// `no-backlinks`
    NoBacklinks,
    /// `path:Folder/`
    Folder(String),
    /// `links:[[Note]]`
    Links(String),
    Text(String),
}

impl What {
    /// An empty row of each kind, in the order of the menu that adds them.
    pub fn blanks() -> Vec<What> {
        vec![
            What::Tag(String::new()),
            What::Prop { key: String::new(), value: String::new() },
            What::Due { cmp: DueCmp::OnOrBefore, day: "+7d".into() },
            What::Blocked,
            What::Blocking,
            What::Orphan,
            What::NoBacklinks,
            What::Folder(String::new()),
            What::Links(String::new()),
            What::Text(String::new()),
        ]
    }

    /// The words of the row, as the menu and the row say them.
    pub fn label(&self) -> &'static str {
        match self {
            What::Tag(_) => "tag is",
            What::Prop { .. } => "property is",
            What::Due { .. } => "due",
            What::Blocked => "is blocked",
            What::Blocking => "is blocking",
            What::Orphan => "is an orphan",
            What::NoBacklinks => "has no backlinks",
            What::Folder(_) => "in folder",
            What::Links(_) => "links to",
            What::Text(_) => "text contains",
        }
    }
}

/// One filter row: what it asks and whether the answer is turned round.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub not: bool,
    pub what: What,
}

/// A value that is one word: it has no white space, parenthesis or quote.
fn one_word(v: &str) -> bool {
    !v.is_empty() && !v.chars().any(|c| c.is_whitespace() || matches!(c, '(' | ')' | '"'))
}

/// A value as a query word: in quotes when it has white space or a parenthesis.
fn quoted(v: &str) -> String {
    if v.chars().any(|c| c.is_whitespace() || c == '(' || c == ')') {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

/// A text as a query word: bare when it reads back as the same text, else in quotes.
fn text_word(v: &str) -> String {
    let operator = matches!(v, "AND" | "OR" | "NOT" | "blocked" | "blocking" | "orphan" | "no-backlinks");
    let field = v.contains(':') || v.contains("[[") || v.starts_with(['-', '#']);
    if operator || field || !one_word(v) {
        format!("\"{v}\"")
    } else {
        v.to_string()
    }
}

/// A key of a property: as the language reads it, and not one of the words that have a row of
/// their own.
fn key_ok(k: &str) -> bool {
    k.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') && !matches!(k.to_ascii_lowercase().as_str(), "path" | "due" | "links")
}

impl Row {
    /// The words of the query this row is, or why it is not one yet (an empty row is no filter).
    pub fn term(&self) -> Result<String, &'static str> {
        let text = match &self.what {
            What::Tag(v) if !one_word(v) => return Err("a tag is a word"),
            What::Tag(v) if !v.starts_with(|c: char| c.is_alphanumeric()) => return Err("a tag starts with a letter or a digit"),
            What::Tag(v) => format!("#{v}"),
            What::Prop { key, .. } if !key_ok(key) => return Err("a property has a name that starts with a letter (not path, due or links)"),
            What::Prop { value, .. } if value.trim().is_empty() || value.contains('"') => return Err("a property needs a value"),
            What::Prop { value, .. } if value.starts_with("//") => return Err("a value that starts with // is text"),
            What::Prop { key, value } => format!("{key}:{}", quoted(value)),
            What::Due { cmp: DueCmp::Overdue, .. } => "due:overdue".to_string(),
            What::Due { day, .. } if !one_word(day) => return Err(if day.is_empty() { "due needs a day: 2026-12-01, today or +7d" } else { "a day is one word" }),
            What::Due { cmp, day } => format!("due:{}{day}", cmp.op()),
            What::Blocked => "blocked".to_string(),
            What::Blocking => "blocking".to_string(),
            What::Orphan => "orphan".to_string(),
            What::NoBacklinks => "no-backlinks".to_string(),
            What::Folder(v) if v.trim().is_empty() || v.contains('"') => return Err("a folder is a path"),
            What::Folder(v) => format!("path:{}", quoted(v)),
            What::Links(v) if v.trim().is_empty() || v.contains(['"', '[', ']']) => return Err("a link is the name of a note"),
            What::Links(v) => format!("links:[[{v}]]"),
            What::Text(v) if v.trim().is_empty() || v.contains('"') => return Err("a text is needed"),
            What::Text(v) => text_word(v),
        };
        Ok(if self.not { format!("NOT {text}") } else { text })
    }
}

/// Why the form does not show the query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fit {
    /// the form shows it
    Rows,
    /// it is more than one level of AND or OR of rows (QUERY-025)
    Complex,
    /// it does not pass the check (QUERY-008); the reason
    Invalid(String),
}

/// The text of a term as the user typed it, without its quotes (the tokenizer drops them too).
fn unquoted(checked: &Checked, term: &Term) -> String {
    checked.source(term).replace('"', "")
}

/// The part after the first colon of a field term.
fn after_colon(s: &str) -> &str {
    s.split_once(':').map_or("", |(_, v)| v)
}

/// The row a term is. The value keeps the case it was typed in.
fn row_of(checked: &Checked, term: &Term, not: bool) -> Row {
    let src = unquoted(checked, term);
    let what = match &term.kind {
        TermKind::Text(_) => What::Text(src),
        TermKind::Tag(_) => What::Tag(src.trim_start_matches('#').to_string()),
        TermKind::Prop { .. } => {
            let (key, value) = src.split_once(':').unwrap_or((&src, ""));
            What::Prop { key: key.to_string(), value: value.to_string() }
        }
        TermKind::Path(_) => What::Folder(after_colon(&src).to_string()),
        TermKind::Due(Due::Overdue) => What::Due { cmp: DueCmp::Overdue, day: String::new() },
        TermKind::Due(Due::Is(cmp, _)) => {
            let cmp = match cmp {
                Cmp::On => DueCmp::On,
                Cmp::Before => DueCmp::Before,
                Cmp::OnOrBefore => DueCmp::OnOrBefore,
                Cmp::After => DueCmp::After,
                Cmp::OnOrAfter => DueCmp::OnOrAfter,
            };
            let value = after_colon(&src);
            What::Due { cmp, day: value.strip_prefix(cmp.op()).unwrap_or(value).to_string() }
        }
        TermKind::Links(_) => What::Links(after_colon(&src).trim_start_matches("[[").trim_end_matches("]]").to_string()),
        TermKind::Blocked => What::Blocked,
        TermKind::Blocking => What::Blocking,
        TermKind::Orphan => What::Orphan,
        TermKind::NoBacklinks => What::NoBacklinks,
    };
    Row { not, what }
}

/// The row of a node of the tree: a term, or a NOT of one.
fn row_of_node(checked: &Checked, e: &Expr) -> Option<Row> {
    match e {
        Expr::Term(t) => Some(row_of(checked, t, false)),
        Expr::Not(inner) => match &**inner {
            Expr::Term(t) => Some(row_of(checked, t, true)),
            _ => None,
        },
        _ => None,
    }
}

/// The rows of a checked query, if it fits the form (QUERY-025): blank, one row, or one level of
/// AND or of OR of rows.
pub fn read(checked: &Checked) -> Result<(Mode, Vec<Row>), Fit> {
    let expr = match &checked.tree {
        Err(e) => return Err(Fit::Invalid(e.reason.clone())),
        Ok(None) => return Ok((Mode::All, Vec::new())),
        Ok(Some(e)) => e,
    };
    let (mode, nodes): (Mode, Vec<&Expr>) = match expr {
        Expr::And(v) => (Mode::All, v.iter().collect()),
        Expr::Or(v) => (Mode::Any, v.iter().collect()),
        one => (Mode::All, vec![one]),
    };
    let rows: Option<Vec<Row>> = nodes.into_iter().map(|e| row_of_node(checked, e)).collect();
    rows.map(|r| (mode, r)).ok_or(Fit::Complex)
}

/// The text of a form: the prefix as it was (NAV-050), then the rows that are complete, joined by
/// AND or OR. A row that is not complete yet is left out.
pub fn write(prefix: &str, mode: Mode, rows: &[Row]) -> String {
    let terms: Vec<String> = rows.iter().filter_map(|r| r.term().ok()).collect();
    format!("{prefix}{}", terms.join(mode.word()))
}

/// What the form shows of one query: its rows, and the text they were made from or made.
pub struct State {
    pub(crate) rows: Vec<Row>,
    pub(crate) mode: Mode,
    /// what stands before the language in the text: the prefix of NAV-050, which the form keeps
    prefix: String,
    pub(crate) fit: Fit,
    /// the text the rows are those of; a text that differs was changed from outside (the box, the
    /// board file) and is read again
    synced: Option<String>,
    pub(crate) sort: sort::State,
    /// the first widget takes the keyboard in the next frame
    pub(crate) focus: bool,
    /// the lists of the value fields (QUERY-029)
    pub(crate) complete: complete::Complete,
}

impl Default for State {
    fn default() -> State {
        State { rows: Vec::new(), mode: Mode::All, prefix: String::new(), fit: Fit::Rows, synced: None, sort: sort::State::default(), focus: false, complete: complete::Complete::default() }
    }
}

impl State {
    /// Ask the first widget of the form for the keyboard, in the next frame it is drawn.
    pub fn focus(&mut self) {
        self.focus = true;
    }

    /// Read `text` (checked as `checked`) when it is not the text the rows are those of.
    pub(crate) fn sync(&mut self, text: &str, checked: &Checked) {
        if self.synced.as_deref() == Some(text) {
            return;
        }
        self.synced = Some(text.to_string());
        match read(checked) {
            Ok((mode, rows)) => {
                // (one row has no mode of its own: the one the user chose stays)
                self.mode = if rows.len() < 2 { self.mode } else { mode };
                self.rows = rows;
                self.prefix = text[..checked.body].to_string();
                self.fit = Fit::Rows;
            }
            Err(fit) => self.fit = fit,
        }
    }

    /// The text of the rows: the one the query is now.
    pub(crate) fn written(&mut self) -> String {
        let text = write(&self.prefix, self.mode, &self.rows);
        self.synced = Some(text.clone());
        text
    }

    /// The rows and their mode as the query text says them (tests).
    #[cfg(test)]
    pub fn rows(&self) -> (Mode, &[Row]) {
        (self.mode, &self.rows)
    }
}

/// How the live count of Advanced says the matches (QUERY-024).
pub fn matches_text(n: usize, cut: bool) -> String {
    format!("{n}{} {}", if cut { "+" } else { "" }, if n == 1 && !cut { "match" } else { "matches" })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_all::query::Cache;
    use crate::search_sources::Prefixes;
    use proptest::prelude::*;

    fn row(not: bool, what: What) -> Row {
        Row { not, what }
    }
    fn text(v: &str) -> What {
        What::Text(v.into())
    }
    fn prop(k: &str, v: &str) -> What {
        What::Prop { key: k.into(), value: v.into() }
    }
    fn due(cmp: DueCmp, day: &str) -> What {
        What::Due { cmp, day: day.into() }
    }

    /// The rows of a text, as the form reads it.
    fn read_text(t: &str) -> Result<(Mode, Vec<Row>), Fit> {
        read(&Cache::default().get(t, Prefixes::default()).clone())
    }

    // spec: QUERY-021
    #[test]
    fn query_021_every_row_kind_is_written_with_and_without_not() {
        let rows = [
            (What::Tag("todo".into()), "#todo"),
            (prop("status", "in progress"), "status:\"in progress\""),
            (prop("status", "doing"), "status:doing"),
            (due(DueCmp::Overdue, ""), "due:overdue"),
            (due(DueCmp::OnOrBefore, "+7d"), "due:<=+7d"),
            (due(DueCmp::Before, "2026-12-01"), "due:<2026-12-01"),
            (due(DueCmp::On, "today"), "due:today"),
            (due(DueCmp::After, "-3d"), "due:>-3d"),
            (due(DueCmp::OnOrAfter, "today"), "due:>=today"),
            (What::Blocked, "blocked"),
            (What::Blocking, "blocking"),
            (What::Orphan, "orphan"),
            (What::NoBacklinks, "no-backlinks"),
            (What::Folder("Projects/".into()), "path:Projects/"),
            (What::Folder("My Notes/".into()), "path:\"My Notes/\""),
            (What::Links("Big Plan".into()), "links:[[Big Plan]]"),
            (text("milk"), "milk"),
            (text("milk run"), "\"milk run\""),
            (text("AND"), "\"AND\""),
            (text("#not a tag"), "\"#not a tag\""),
            (text("12:30"), "\"12:30\""),
            (text("-x"), "\"-x\""),
        ];
        for (what, want) in rows {
            let plain = row(false, what.clone());
            assert_eq!(plain.term().as_deref(), Ok(want));
            let not = row(true, what.clone());
            assert_eq!(not.term(), Ok(format!("NOT {want}")));
            // each, with and without NOT, is read back as the same row
            for r in [plain, not] {
                let t = write("", Mode::All, std::slice::from_ref(&r));
                assert_eq!(read_text(&t), Ok((Mode::All, vec![r.clone()])), "{t}");
            }
        }
    }

    // spec: QUERY-021
    #[test]
    fn query_021_a_row_that_is_not_complete_is_no_filter_and_says_why() {
        for what in [
            What::Tag(String::new()),
            What::Tag("two words".into()),
            What::Tag("-x".into()),
            prop("", "x"),
            prop("status", ""),
            prop("due", "x"),
            prop("a", "//x"),
            due(DueCmp::On, ""),
            What::Folder("  ".into()),
            What::Links(String::new()),
            What::Links("a]]b".into()),
            text(""),
            text("a\"b"),
        ] {
            let r = row(false, what.clone());
            assert!(r.term().is_err(), "{what:?}");
            assert_eq!(write("", Mode::All, &[r, row(false, text("x"))]), "x", "the other rows are written, this one is left out");
        }
    }

    // spec: QUERY-022
    #[test]
    fn query_022_all_of_is_and_and_any_of_is_or() {
        let rows = [row(false, What::Tag("a".into())), row(true, prop("status", "done")), row(false, text("x"))];
        assert_eq!(write("", Mode::All, &rows), "#a AND NOT status:done AND x");
        assert_eq!(write("", Mode::Any, &rows), "#a OR NOT status:done OR x");
        assert_eq!(read_text("#a AND NOT status:done AND x"), Ok((Mode::All, rows.to_vec())));
        assert_eq!(read_text("#a OR NOT status:done OR x"), Ok((Mode::Any, rows.to_vec())));
        assert_eq!(read_text("#a -status:done x"), Ok((Mode::All, rows.to_vec())), "side by side is AND, -x is NOT");
        // two text rows stay two texts, not one phrase
        let two = [row(false, text("milk")), row(false, text("run"))];
        assert_eq!(write("", Mode::All, &two), "milk AND run");
        assert_eq!(read_text("milk AND run"), Ok((Mode::All, two.to_vec())));
    }

    // spec: QUERY-025
    #[test]
    fn query_025_a_text_fits_when_it_is_one_level_and_otherwise_is_left_alone() {
        assert_eq!(read_text(""), Ok((Mode::All, vec![])));
        assert_eq!(read_text("  "), Ok((Mode::All, vec![])));
        for complex in ["a OR b AND c", "(a OR b) AND c", "NOT (a OR b)", "NOT NOT a", "#a AND (b OR c)", "(a OR b) OR c AND d"] {
            let got = read_text(complex);
            assert_eq!(got, Err(Fit::Complex), "{complex}");
        }
        assert!(matches!(read_text("(a OR b)"), Ok((Mode::Any, _))), "one group is one level");
        assert!(matches!(read_text("a AND (b"), Err(Fit::Invalid(_))));
        assert!(matches!(read_text("due:soon"), Err(Fit::Invalid(r)) if r.contains("due:")));
    }

    // spec: QUERY-025
    #[test]
    fn query_025_the_prefix_and_the_case_of_a_text_are_kept() {
        let mut cache = Cache::default();
        let mut st = State::default();
        for t in ["m: Status:Doing AND NOT \"Big Plan\"", "f:#Todo", "sm:"] {
            let checked = cache.get(t, Prefixes::default()).clone();
            st.sync(t, &checked);
            assert_eq!(st.fit, Fit::Rows, "{t}");
            assert_eq!(st.written(), if t.starts_with("m:") { "m: Status:Doing AND NOT \"Big Plan\"" } else { t }, "the same text comes back");
        }
        let checked = cache.get("m: a OR b AND c", Prefixes::default()).clone();
        st.sync("m: a OR b AND c", &checked);
        assert_eq!(st.fit, Fit::Complex);
    }

    /// The query a text is, without where in the text its words are.
    fn shape(t: &str) -> String {
        let checked = Cache::default().get(t, Prefixes::default()).clone();
        format!("{:?}", checked.tree.map(|e| e.map(strip)))
    }
    fn strip(e: Expr) -> String {
        match e {
            Expr::And(v) => format!("and({})", v.into_iter().map(strip).collect::<Vec<_>>().join(",")),
            Expr::Or(v) => format!("or({})", v.into_iter().map(strip).collect::<Vec<_>>().join(",")),
            Expr::Not(e) => format!("not({})", strip(*e)),
            Expr::Term(t) => format!("{:?}", t.kind),
        }
    }

    fn value() -> impl Strategy<Value = String> {
        prop_oneof![3 => "[a-zA-Z0-9][a-zA-Z0-9_/.-]{0,6}", 1 => "[a-z]{1,4} [a-z]{1,4}", 1 => "[a-z]{1,3}  [a-z]{1,3}", 1 => "[a-z]{1,4}\\([a-z]{1,3}\\)", 1 => Just("AND".to_string()), 1 => Just("-a".to_string()), 1 => "[a-z]{1,3}:[a-z0-9]{1,3}", 1 => Just("blocked".to_string()), 1 => Just("orphan".to_string()), 1 => Just("no-backlinks".to_string())]
    }

    fn what() -> impl Strategy<Value = What> {
        let key = "[a-zA-Z][a-zA-Z0-9_-]{2,6}".prop_filter("not a prefix, not a reserved word", |k| k.chars().any(|c| !"fmsFMS".contains(c)) && key_ok(k) && !k.eq_ignore_ascii_case("priority"));
        let tag = "[a-zA-Z0-9][a-zA-Z0-9_/.:-]{0,6}";
        let day = prop_oneof![Just("today"), Just("+7d"), Just("-3d"), Just("2026-12-01")];
        let cmp = prop::sample::select(DueCmp::ALL[1..].to_vec());
        let text = value().prop_filter("a letter", |v| v.chars().any(char::is_alphanumeric));
        prop_oneof![
            tag.prop_map(What::Tag),
            (key, value().prop_filter("not //", |v| !v.starts_with("//"))).prop_map(|(key, value)| What::Prop { key, value }),
            Just(What::Due { cmp: DueCmp::Overdue, day: String::new() }),
            (cmp, day).prop_map(|(cmp, day)| What::Due { cmp, day: day.to_string() }),
            Just(What::Blocked),
            Just(What::Blocking),
            Just(What::Orphan),
            Just(What::NoBacklinks),
            value().prop_map(What::Folder),
            "[a-zA-Z0-9][a-zA-Z0-9 ]{0,6}".prop_map(What::Links),
            text.prop_map(What::Text),
        ]
    }

    proptest! {
        #![proptest_config(crate::property_test_support::config())]

        // spec: QUERY-021, QUERY-022, QUERY-025
        #[test]
        fn query_025_the_text_of_any_rows_reads_back_as_the_same_rows(rows in prop::collection::vec((any::<bool>(), what()), 0..6), any_of in any::<bool>(), prefix in prop_oneof![Just(""), Just("m:"), Just("fs: ")]) {
            let rows: Vec<Row> = rows.into_iter().map(|(not, what)| Row { not, what }).collect();
            let mode = if any_of { Mode::Any } else { Mode::All };
            let t = write(prefix, mode, &rows);
            let mut cache = Cache::default();
            let checked = cache.get(&t, Prefixes::default()).clone();
            prop_assert!(checked.error().is_none(), "{} is refused: {:?}", t, checked.error());
            let (got_mode, got) = read(&checked).map_err(|f| TestCaseError::fail(format!("{t}: {f:?}")))?;
            prop_assert_eq!(&got, &rows, "{}", t);
            if rows.len() > 1 {
                prop_assert_eq!(got_mode, mode, "{}", t);
            }
            // and the state writes the very text it read
            let mut st = State::default();
            st.sync(&t, &checked);
            prop_assert_eq!(st.written(), t);
        }

        // spec: QUERY-025
        #[test]
        fn query_025_a_text_that_fits_writes_a_text_that_means_the_same(t in crate::property_test_support::query_text()) {
            let checked = Cache::default().get(&t, Prefixes::default()).clone();
            if let Ok((mode, rows)) = read(&checked) {
                let back = write(&t[..checked.body], mode, &rows);
                prop_assert_eq!(shape(&back), shape(&t), "{:?} -> {:?}", t, back);
            }
        }
    }

    #[test]
    fn a_value_the_language_refuses_is_the_error_of_the_text() {
        // the generator leaves priority out: its value must be a level, and the check says so
        let r = row(false, prop("priority", "urgent"));
        let t = write("", Mode::All, &[r]);
        assert!(Cache::default().get(&t, Prefixes::default()).error().is_some());
    }

    #[test]
    fn the_matches_are_counted_with_a_plus_when_cut() {
        assert_eq!(matches_text(1, false), "1 match");
        assert_eq!(matches_text(0, false), "0 matches");
        assert_eq!(matches_text(20, true), "20+ matches");
    }
}
