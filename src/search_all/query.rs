//! The query language (QUERY-001…QUERY-013): what the sidebar search, the picker and, later, the
//! lanes of a board read. [`Checked::search`] cuts the prefix of NAV-050 off the text, parses the
//! rest into a tree of [`Expr`] and checks it against the limits, before anything runs
//! (QUERY-007). A query that does not pass is a [`QueryError`] with the bytes to underline and a
//! short reason (QUERY-008); nothing runs for it. [`Cache`] keeps the result with the text it was
//! made for, so a view parses when the text changes and not every frame (QUERY-013).
//!
//! ```text
//! query := or        or := and (OR and)*        and := unary ((AND)? unary)*
//! unary := NOT unary | -term | -( or ) | ( or ) | term
//! term  := #tag | key:value | path:Folder/ | due:… | links:[[X]] | blocked | blocking | text
//! ```
//!
//! `AND`, `OR` and `NOT` are operators in upper case only. A run of plain words with nothing
//! between them is one text term (a phrase, as the search always took it), so "salt and pepper"
//! stays a text search; a word that is a field, a `-x` or an operator ends the run. A quoted word
//! (`"AND"`, `"a (b)"`) is text whatever is inside it.

use crate::calendar::time::parse_ymd;
use crate::search_sources::{Prefixes, Selection};
use std::ops::Range;

/// A query has at most this many characters (QUERY-009).
pub const MAX_QUERY_CHARS: usize = 500;
/// A query has at most this many terms.
pub const MAX_TERMS: usize = 32;
/// Parentheses and `NOT` nest at most this deep.
pub const MAX_DEPTH: usize = 8;
/// A text pattern has at most this many `*`.
pub const MAX_STARS: usize = 8;
/// The levels of `priority:` (VAULT-047), the only values it takes.
pub const PRIORITIES: [&str; 5] = ["highest", "high", "medium", "low", "lowest"];

/// Why a query is refused: the bytes of the text to underline and a short reason (QUERY-008).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryError {
    pub span: Range<usize>,
    pub reason: String,
}

impl QueryError {
    fn new(span: Range<usize>, reason: impl Into<String>) -> QueryError {
        QueryError { span, reason: reason.into() }
    }
}

/// How a due date is compared.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmp {
    On,
    Before,
    OnOrBefore,
    After,
    OnOrAfter,
}

/// A day of a query: a date, or a number of days from today.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Day {
    Date(i64),
    FromToday(i64),
}

/// What `due:` asks (QUERY-003).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Due {
    /// before today
    Overdue,
    Is(Cmp, Day),
}

/// One thing a query looks for (QUERY-002).
#[derive(Clone, Debug, PartialEq)]
pub enum TermKind {
    /// a wildcard pattern (NAV-023), in lower case
    Text(String),
    /// `#tag`, in lower case without the `#`; `#a` matches `#a/b` too
    Tag(String),
    /// `key:value` of the front matter, both in lower case
    Prop {
        key: String,
        value: String,
    },
    /// `path:Folder/`, in lower case
    Path(String),
    Due(Due),
    /// `links:[[X]]`: the lower-case key of the name (`deps::link_key`)
    Links(String),
    Blocked,
    Blocking,
}

/// A term and the bytes of the query it was read from.
#[derive(Clone, Debug, PartialEq)]
pub struct Term {
    pub kind: TermKind,
    pub src: Range<usize>,
}

/// The tree of a query. Precedence: NOT, then AND, then OR (QUERY-001).
#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    And(Vec<Expr>),
    Or(Vec<Expr>),
    Not(Box<Expr>),
    Term(Term),
}

impl Expr {
    /// Does the query have a text term anywhere?
    pub fn has_text(&self) -> bool {
        match self {
            Expr::And(v) | Expr::Or(v) => v.iter().any(Expr::has_text),
            Expr::Not(e) => e.has_text(),
            Expr::Term(t) => matches!(t.kind, TermKind::Text(_)),
        }
    }

    /// The text patterns that are not under a NOT: the ones whose lines a hit shows.
    pub fn positive_texts(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_texts(&mut out);
        out
    }

    fn collect_texts<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Expr::And(v) | Expr::Or(v) => v.iter().for_each(|e| e.collect_texts(out)),
            Expr::Not(_) => {}
            Expr::Term(Term { kind: TermKind::Text(p), .. }) => out.push(p),
            Expr::Term(_) => {}
        }
    }

    /// The terms of a query that is nothing but terms side by side (what mail and posts can
    /// take), or the operator that keeps it from being one (`NOT`, `OR`).
    pub fn flat_terms(&self) -> Result<Vec<&Term>, &'static str> {
        match self {
            Expr::Term(t) => Ok(vec![t]),
            Expr::And(v) => {
                let mut out = Vec::new();
                for e in v {
                    out.extend(e.flat_terms()?);
                }
                Ok(out)
            }
            Expr::Or(_) => Err("OR"),
            Expr::Not(_) => Err("NOT"),
        }
    }
}

/// A query text as it was checked: its prefix, and its tree or what is wrong with it.
#[derive(Clone, Debug, PartialEq)]
pub struct Checked {
    /// the text as typed, with its prefix; every span of the tree and the error is a byte range of it
    pub text: String,
    /// the sources of the prefix (NAV-050)
    pub prefix: Option<Selection>,
    /// the tree; none: the query is blank, apart from the prefix
    pub tree: Result<Option<Expr>, QueryError>,
}

impl Checked {
    /// Check the text of the sidebar or the picker: a prefix chooses the sources (QUERY-005).
    pub fn search(text: &str, prefixes: Prefixes) -> Checked {
        let parsed = prefixes.parse(text);
        let body = text.len() - parsed.text.len();
        Checked { text: text.to_string(), prefix: parsed.prefix, tree: parse(text, body) }
    }

    /// Check the text of a board lane (the lanes of #601 hold this and its error, QUERY-012): notes only, so a prefix is an error (QUERY-005).
    #[cfg(test)]
    pub fn lane(text: &str, prefixes: Prefixes) -> Checked {
        let parsed = prefixes.parse(text);
        if parsed.prefix.is_some() {
            let body = text.len() - parsed.text.len();
            let tree = Err(QueryError::new(0..body, "a lane searches notes only: no prefix"));
            return Checked { text: text.to_string(), prefix: None, tree };
        }
        Checked { text: text.to_string(), prefix: None, tree: parse(text, 0) }
    }

    /// What is wrong with the query, if anything.
    pub fn error(&self) -> Option<&QueryError> {
        self.tree.as_ref().err()
    }

    /// The tree of a query that is valid and not blank.
    pub fn expr(&self) -> Option<&Expr> {
        self.tree.as_ref().ok().and_then(Option::as_ref)
    }

    /// The bytes of the query a term was read from.
    pub fn source(&self, term: &Term) -> &str {
        &self.text[term.src.clone()]
    }
}

/// The result of checking the text of one view, kept with the text (QUERY-013).
#[derive(Default)]
pub struct Cache {
    made_for: Option<(String, Prefixes)>,
    checked: Option<Checked>,
    /// how often the text was parsed (tests)
    #[cfg(test)]
    pub parses: usize,
}

impl Cache {
    /// The check of `text`: made when the text, or the letters of the prefixes, are not the ones
    /// of the last call.
    pub fn get(&mut self, text: &str, prefixes: Prefixes) -> &Checked {
        if self.made_for.as_ref().is_none_or(|(t, p)| t != text || *p != prefixes) {
            self.made_for = Some((text.to_string(), prefixes));
            self.checked = Some(Checked::search(text, prefixes));
            #[cfg(test)]
            {
                self.parses += 1;
            }
        }
        self.checked.as_ref().expect("a check was made")
    }
}

/// What a word of the text turned out to be.
enum Tok {
    Open,
    Close,
    /// `-` right before a `(`
    Minus,
    Word {
        text: String,
        starts_quoted: bool,
    },
}

struct Token {
    tok: Tok,
    span: Range<usize>,
}

/// Cut `text[body..]` into words and parentheses. A `[[…]]` stays in its word, and so does a
/// quote: `"…"` is read to the closing quote.
fn tokenize(text: &str, body: usize) -> Result<Vec<Token>, QueryError> {
    let mut out = Vec::new();
    let mut i = body;
    while i < text.len() {
        let c = text[i..].chars().next().expect("inside the text");
        if c.is_whitespace() {
            i += c.len_utf8();
        } else if c == '(' || c == ')' {
            out.push(Token { tok: if c == '(' { Tok::Open } else { Tok::Close }, span: i..i + 1 });
            i += 1;
        } else if c == '-' && text[i + 1..].starts_with('(') {
            out.push(Token { tok: Tok::Minus, span: i..i + 1 });
            i += 1;
        } else {
            let start = i;
            let mut word = String::new();
            while i < text.len() {
                let c = text[i..].chars().next().expect("inside the text");
                if c.is_whitespace() || c == '(' || c == ')' {
                    break;
                }
                if c == '"' {
                    let Some(len) = text[i + 1..].find('"') else {
                        return Err(QueryError::new(i..text.len(), "the quote is not closed"));
                    };
                    word.push_str(&text[i + 1..i + 1 + len]);
                    i += len + 2;
                } else if text[i..].starts_with("[[") {
                    let end = text[i + 2..].find("]]").map_or(i + 2, |p| i + 2 + p + 2);
                    word.push_str(&text[i..end]);
                    i = end;
                } else {
                    word.push(c);
                    i += c.len_utf8();
                }
            }
            out.push(Token { tok: Tok::Word { text: word, starts_quoted: text[start..].starts_with('"') }, span: start..i });
        }
    }
    Ok(out)
}

/// Parse and check the language part of `text`, which starts at byte `body`.
fn parse(text: &str, body: usize) -> Result<Option<Expr>, QueryError> {
    if let Some((at, _)) = text.char_indices().nth(MAX_QUERY_CHARS) {
        return Err(QueryError::new(at..text.len(), format!("a query has at most {MAX_QUERY_CHARS} characters")));
    }
    let toks = tokenize(text, body)?;
    if toks.is_empty() {
        return Ok(None);
    }
    let mut p = Parser { text, toks, pos: 0, terms: 0 };
    let expr = p.or(0)?;
    match p.toks.get(p.pos) {
        None => Ok(Some(expr)),
        Some(t) => Err(QueryError::new(t.span.clone(), "no ( for this )")),
    }
}

struct Parser<'a> {
    text: &'a str,
    toks: Vec<Token>,
    pos: usize,
    terms: usize,
}

/// The operator a token is, if it is one (upper case, not quoted).
fn operator(t: &Token) -> Option<&'static str> {
    match &t.tok {
        Tok::Word { text, starts_quoted: false } => ["AND", "OR", "NOT"].into_iter().find(|op| text == op),
        _ => None,
    }
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.toks.get(self.pos)
    }

    fn peek_op(&self, op: &str) -> bool {
        self.peek().is_some_and(|t| operator(t) == Some(op))
    }

    /// Can a term, a group or a NOT start here?
    fn starts_unary(&self) -> bool {
        self.peek().is_some_and(|t| match t.tok {
            Tok::Open | Tok::Minus => true,
            Tok::Close => false,
            Tok::Word { .. } => !matches!(operator(t), Some("AND" | "OR")),
        })
    }

    /// Require a term to start here, after the operator at `op`.
    fn need_unary(&self, op: Range<usize>, name: &str) -> Result<(), QueryError> {
        if self.starts_unary() {
            Ok(())
        } else {
            Err(QueryError::new(op, format!("{name} needs a term after it")))
        }
    }

    /// The span of the token before the next one (the end of the text when there is none).
    fn last_span(&self) -> Range<usize> {
        self.pos.checked_sub(1).map_or(self.text.len()..self.text.len(), |i| self.toks[i].span.clone())
    }

    fn or(&mut self, depth: usize) -> Result<Expr, QueryError> {
        let mut parts = vec![self.and(depth)?];
        while self.peek_op("OR") {
            self.pos += 1;
            self.need_unary(self.last_span(), "OR")?;
            parts.push(self.and(depth)?);
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { Expr::Or(parts) })
    }

    fn and(&mut self, depth: usize) -> Result<Expr, QueryError> {
        if let Some(t) = self.peek().filter(|t| matches!(operator(t), Some("AND" | "OR"))) {
            return Err(QueryError::new(t.span.clone(), format!("{} needs a term before it", operator(t).expect("an operator"))));
        }
        if let Some(Token { tok: Tok::Close, span }) = self.peek() {
            return Err(QueryError::new(span.clone(), "no ( for this )"));
        }
        if !self.starts_unary() {
            return Err(QueryError::new(self.last_span(), "a term is missing"));
        }
        let mut parts = vec![self.unary(depth)?];
        loop {
            if self.peek_op("AND") {
                self.pos += 1;
                self.need_unary(self.last_span(), "AND")?;
            } else if !self.starts_unary() {
                break;
            }
            parts.push(self.unary(depth)?);
        }
        Ok(if parts.len() == 1 { parts.remove(0) } else { Expr::And(parts) })
    }

    fn nested(&self, at: &Range<usize>, depth: usize) -> Result<(), QueryError> {
        if depth >= MAX_DEPTH {
            Err(QueryError::new(at.clone(), format!("nested more than {MAX_DEPTH} levels")))
        } else {
            Ok(())
        }
    }

    fn unary(&mut self, depth: usize) -> Result<Expr, QueryError> {
        let at = self.pos;
        self.pos += 1;
        let span = self.toks[at].span.clone();
        match &self.toks[at].tok {
            Tok::Open => {
                self.nested(&span, depth)?;
                if let Some(Token { tok: Tok::Close, span: close }) = self.peek() {
                    return Err(QueryError::new(span.start..close.end, "nothing inside the parentheses"));
                }
                let inner = self.or(depth + 1)?;
                match self.peek().map(|t| &t.tok) {
                    Some(Tok::Close) => {
                        self.pos += 1;
                        Ok(inner)
                    }
                    _ => Err(QueryError::new(span, "this ( is not closed")),
                }
            }
            Tok::Minus => {
                self.nested(&span, depth)?;
                Ok(Expr::Not(Box::new(self.operand(depth + 1)?)))
            }
            Tok::Close => Err(QueryError::new(span, "no ( for this )")),
            Tok::Word { text, starts_quoted } => {
                if operator(&self.toks[at]) == Some("NOT") {
                    self.nested(&span, depth)?;
                    self.need_unary(span, "NOT")?;
                    return Ok(Expr::Not(Box::new(self.operand(depth + 1)?)));
                }
                if is_dashed(&self.toks[at].tok) {
                    self.nested(&span, depth)?;
                    let rest = Token { tok: Tok::Word { text: text[1..].to_string(), starts_quoted: *starts_quoted }, span: span.start + 1..span.end };
                    return Ok(Expr::Not(Box::new(Expr::Term(self.term(&rest)?))));
                }
                self.pos = at;
                self.run()
            }
        }
    }

    /// The operand of a NOT or a minus: one term or a group, not a run of words.
    fn operand(&mut self, depth: usize) -> Result<Expr, QueryError> {
        match self.peek() {
            Some(t) if matches!(t.tok, Tok::Word { .. }) && operator(t).is_none() && !is_dashed(&t.tok) => {
                let tok = self.take_word();
                Ok(Expr::Term(self.term(&tok)?))
            }
            _ => self.unary(depth),
        }
    }

    /// A word that is a term; when it is text, the plain words after it join it as one text.
    fn run(&mut self) -> Result<Expr, QueryError> {
        let tok = self.take_word();
        let kind = self.classify(&tok)?;
        if !matches!(kind, TermKind::Text(_)) {
            self.count(&tok.span)?;
            return Ok(Expr::Term(Term { kind, src: tok.span }));
        }
        // the words of the run, with the white space between them as it was typed
        let (mut text, mut end) = (word_text(&tok), tok.span.end);
        while let Some(next) = self.peek() {
            let plain = matches!(next.tok, Tok::Word { .. }) && operator(next).is_none() && !is_dashed(&next.tok) && matches!(self.classify(next), Ok(TermKind::Text(_)));
            if !plain {
                break;
            }
            let next = self.take_word();
            text.push_str(&self.text[end..next.span.start]);
            text.push_str(&word_text(&next));
            end = next.span.end;
        }
        let src = tok.span.start..end;
        self.count(&src)?;
        Ok(Expr::Term(Term { kind: text_kind(&text, &src)?, src }))
    }

    fn take_word(&mut self) -> Token {
        let t = &self.toks[self.pos];
        self.pos += 1;
        match &t.tok {
            Tok::Word { text, starts_quoted } => Token { tok: Tok::Word { text: text.clone(), starts_quoted: *starts_quoted }, span: t.span.clone() },
            _ => unreachable!("a word was checked"),
        }
    }

    fn count(&mut self, src: &Range<usize>) -> Result<(), QueryError> {
        self.terms += 1;
        if self.terms > MAX_TERMS {
            return Err(QueryError::new(src.clone(), format!("a query has at most {MAX_TERMS} terms")));
        }
        Ok(())
    }

    fn term(&mut self, t: &Token) -> Result<Term, QueryError> {
        self.count(&t.span)?;
        Ok(Term { kind: self.classify(t)?, src: t.span.clone() })
    }

    fn classify(&self, t: &Token) -> Result<TermKind, QueryError> {
        let Tok::Word { text, starts_quoted } = &t.tok else { unreachable!("a word was checked") };
        classify(text, *starts_quoted, &t.span)
    }
}

/// Is the word a `-x`: a minus and something after it, not quoted?
fn is_dashed(t: &Tok) -> bool {
    matches!(t, Tok::Word { text, starts_quoted: false } if text.len() > 1 && text.starts_with('-'))
}

/// The text of a word as it is searched for.
fn word_text(t: &Token) -> String {
    match &t.tok {
        Tok::Word { text, .. } => text.clone(),
        _ => String::new(),
    }
}

/// A text pattern: lower case, with at most [`MAX_STARS`] `*` and a letter or something else
/// besides the wildcards (a pattern of wildcards alone would match every line).
fn text_kind(text: &str, src: &Range<usize>) -> Result<TermKind, QueryError> {
    if !text.chars().any(|c| c != '*' && c != '?') {
        return Err(QueryError::new(src.clone(), "a pattern of * and ? alone matches everything"));
    }
    if text.matches('*').count() > MAX_STARS {
        return Err(QueryError::new(src.clone(), format!("a pattern has at most {MAX_STARS} *")));
    }
    Ok(TermKind::Text(text.to_lowercase()))
}

/// What one word is: a field, or text (QUERY-002).
fn classify(word: &str, starts_quoted: bool, span: &Range<usize>) -> Result<TermKind, QueryError> {
    let text = || text_kind(word, span);
    if starts_quoted {
        return text();
    }
    match word {
        "blocked" => return Ok(TermKind::Blocked),
        "blocking" => return Ok(TermKind::Blocking),
        _ => {}
    }
    if let Some(tag) = word.strip_prefix('#') {
        return if tag.chars().next().is_some_and(char::is_alphanumeric) { Ok(TermKind::Tag(tag.to_lowercase())) } else { text() };
    }
    let Some((key, value)) = word.split_once(':') else { return text() };
    let key_ok = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if !key_ok {
        return text();
    }
    let key = key.to_lowercase();
    let known = matches!(key.as_str(), "path" | "due" | "links" | "priority");
    if value.is_empty() {
        return if known { Err(QueryError::new(span.clone(), format!("{key}: needs a value"))) } else { text() };
    }
    match key.as_str() {
        "path" => Ok(TermKind::Path(value.to_lowercase())),
        "due" => due(value).map(TermKind::Due).ok_or_else(|| QueryError::new(span.clone(), "due: takes a date (2026-12-01), today, overdue, or <, <=, > or >= with a date or +7d, -3d")),
        "links" => {
            let name = value.strip_prefix("[[").and_then(|v| v.strip_suffix("]]")).filter(|n| !n.trim().is_empty());
            name.map(|n| TermKind::Links(super::deps::link_key(n))).ok_or_else(|| QueryError::new(span.clone(), "links: takes a note in brackets: links:[[Note]]"))
        }
        "priority" => {
            let level = value.to_lowercase();
            if PRIORITIES.contains(&level.as_str()) {
                Ok(TermKind::Prop { key, value: level })
            } else {
                Err(QueryError::new(span.clone(), format!("priority is one of {}", PRIORITIES.join(", "))))
            }
        }
        // (`http://…` and the like are text)
        _ if value.starts_with("//") => text(),
        _ => Ok(TermKind::Prop { key, value: value.to_lowercase() }),
    }
}

/// The value of `due:`: `overdue`, a day, or a comparison with a day.
fn due(value: &str) -> Option<Due> {
    if value.eq_ignore_ascii_case("overdue") {
        return Some(Due::Overdue);
    }
    let (cmp, rest) = [("<=", Cmp::OnOrBefore), (">=", Cmp::OnOrAfter), ("<", Cmp::Before), (">", Cmp::After)].into_iter().find_map(|(p, c)| value.strip_prefix(p).map(|r| (c, r))).unwrap_or((Cmp::On, value));
    Some(Due::Is(cmp, day(rest)?))
}

/// A day: `2026-12-01`, `today`, `+7d` or `-3d`.
fn day(s: &str) -> Option<Day> {
    if s.eq_ignore_ascii_case("today") {
        return Some(Day::FromToday(0));
    }
    if let Some(days) = s.strip_suffix(['d', 'D']) {
        let sign = match days.chars().next()? {
            '+' => 1,
            '-' => -1,
            _ => return parse_date(s),
        };
        let n = &days[1..];
        let n: i64 = n.parse().ok().filter(|_| days.len() <= 6)?;
        return Some(Day::FromToday(sign * n));
    }
    parse_date(s)
}

/// `YYYY-MM-DD`, strictly.
fn parse_date(s: &str) -> Option<Day> {
    let b = s.as_bytes();
    let shape = b.len() == 10 && b[4] == b'-' && b[7] == b'-' && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    shape.then(|| parse_ymd(s).map(Day::Date)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property_test_support::{config, query_text};
    use proptest::prelude::*;

    fn check(text: &str) -> Checked {
        Checked::search(text, Prefixes::default())
    }

    fn tree(text: &str) -> Expr {
        let c = check(text);
        c.expr().cloned().unwrap_or_else(|| panic!("{text:?}: {:?}", c.tree))
    }

    fn err(text: &str) -> QueryError {
        check(text).error().cloned().unwrap_or_else(|| panic!("{text:?} should be refused"))
    }

    /// The tree without the spans, to compare shapes.
    fn shape(e: &Expr) -> String {
        match e {
            Expr::And(v) => format!("(and {})", v.iter().map(shape).collect::<Vec<_>>().join(" ")),
            Expr::Or(v) => format!("(or {})", v.iter().map(shape).collect::<Vec<_>>().join(" ")),
            Expr::Not(e) => format!("(not {})", shape(e)),
            Expr::Term(t) => match &t.kind {
                TermKind::Text(p) => format!("'{p}'"),
                TermKind::Tag(t) => format!("#{t}"),
                TermKind::Prop { key, value } => format!("{key}={value}"),
                TermKind::Path(p) => format!("path={p}"),
                TermKind::Due(d) => format!("due={d:?}"),
                TermKind::Links(l) => format!("links={l}"),
                TermKind::Blocked => "blocked".into(),
                TermKind::Blocking => "blocking".into(),
            },
        }
    }

    fn sh(text: &str) -> String {
        shape(&tree(text))
    }

    // spec: QUERY-001
    #[test]
    fn query_001_operators_are_upper_case_and_precedence_is_not_and_or() {
        assert_eq!(sh("a AND b"), "(and 'a' 'b')");
        assert_eq!(sh("a OR b"), "(or 'a' 'b')");
        assert_eq!(sh("NOT a"), "(not 'a')");
        assert_eq!(sh("a OR b AND c"), "(or 'a' (and 'b' 'c'))", "AND binds tighter than OR");
        assert_eq!(sh("a AND b OR c"), "(or (and 'a' 'b') 'c')");
        assert_eq!(sh("NOT a AND b"), "(and (not 'a') 'b')", "NOT binds tighter than AND");
        assert_eq!(sh("(a OR b) AND c"), "(and (or 'a' 'b') 'c')", "parentheses group");
        assert_eq!(sh("NOT (a OR b)"), "(not (or 'a' 'b'))");
        assert_eq!(sh("a OR b OR c"), "(or 'a' 'b' 'c')");
        assert_eq!(sh("#x #y"), "(and #x #y)", "two terms side by side are AND");
        assert_eq!(sh("#x AND #y"), sh("#x #y"));
        assert_eq!(sh("NOT NOT a"), "(not (not 'a'))");
    }

    // spec: QUERY-001
    #[test]
    fn query_001_lower_case_words_are_text_and_a_run_of_words_is_one_phrase() {
        assert_eq!(sh("salt and pepper"), "'salt and pepper'", "\"and\" in lower case is a word");
        assert_eq!(sh("a or b not c"), "'a or b not c'");
        assert_eq!(sh("Salt  AND  Pepper"), "(and 'salt' 'pepper')", "upper case is the operator");
        assert_eq!(sh("two  spaces"), "'two  spaces'", "the white space of a phrase stays as typed");
        assert_eq!(sh("milk run #todo"), "(and 'milk run' #todo)", "a field ends the run");
        assert_eq!(sh("milk #todo run"), "(and 'milk' #todo 'run')");
        assert_eq!(sh("\"AND\""), "'and'", "a quoted word is text");
        assert_eq!(sh("\"a (b)\" c"), "'a (b) c'");
        assert_eq!(sh("http://example.com"), "'http://example.com'");
        assert_eq!(sh("12:30 lunch"), "'12:30 lunch'", "a key starts with a letter");
        assert_eq!(sh("TODO: fix"), "'todo: fix'", "a colon with nothing after it is text");
        assert_eq!(sh("-"), "'-'");
        assert_eq!(sh("#"), "'#'");
        assert_eq!(sh("# Heading"), "'# heading'");
    }

    // spec: QUERY-001
    #[test]
    fn query_001_a_minus_is_not() {
        assert_eq!(sh("a -b"), "(and 'a' (not 'b'))");
        assert_eq!(sh("-b"), "(not 'b')");
        assert_eq!(sh("-#done"), "(not #done)");
        assert_eq!(sh("-(a OR b)"), "(not (or 'a' 'b'))");
        assert_eq!(sh("well-known"), "'well-known'", "only a leading minus counts");
        assert_eq!(sh("a NOT b c"), "(and 'a' (not 'b') 'c')", "NOT takes one term, the words after it are another");
        assert_eq!(sh("NOT \"x y\""), "(not 'x y')");
    }

    // spec: QUERY-002
    #[test]
    fn query_002_fields() {
        assert_eq!(sh("#Todo"), "#todo");
        assert_eq!(sh("#a/b"), "#a/b");
        assert_eq!(sh("status:Doing"), "status=doing");
        assert_eq!(sh("type:drawing"), "type=drawing");
        assert_eq!(sh("path:Projects/"), "path=projects/");
        assert_eq!(sh("links:[[My Note]]"), "links=my note");
        assert_eq!(sh("links:[[Folder/My Note#Head|alias]]"), "links=my note");
        assert_eq!(sh("blocked"), "blocked");
        assert_eq!(sh("blocking"), "blocking");
        assert_eq!(sh("blocked AND NOT blocking"), "(and blocked (not blocking))");
        assert_eq!(sh("the blocked road"), "(and 'the' blocked 'road')", "blocked is a field, a quote makes it text");
        assert_eq!(sh("status:\"in progress\""), "status=in progress");
        assert_eq!(sh("links:[[A (b)]] x"), "(and links=a (b) 'x')", "a link may hold parentheses");
    }

    // spec: QUERY-003
    #[test]
    fn query_003_dates_and_ranges() {
        let d = |s: &str| match tree(s) {
            Expr::Term(Term { kind: TermKind::Due(d), .. }) => d,
            other => panic!("{other:?}"),
        };
        let day = parse_ymd("2026-12-01").unwrap();
        assert_eq!(d("due:2026-12-01"), Due::Is(Cmp::On, Day::Date(day)));
        assert_eq!(d("due:<+7d"), Due::Is(Cmp::Before, Day::FromToday(7)));
        assert_eq!(d("due:>-3d"), Due::Is(Cmp::After, Day::FromToday(-3)));
        assert_eq!(d("due:<=+7d"), Due::Is(Cmp::OnOrBefore, Day::FromToday(7)));
        assert_eq!(d("due:>=2026-12-01"), Due::Is(Cmp::OnOrAfter, Day::Date(day)));
        assert_eq!(d("due:<today"), Due::Is(Cmp::Before, Day::FromToday(0)));
        assert_eq!(d("due:overdue"), Due::Overdue);
        assert_eq!(d("due:OVERDUE"), Due::Overdue);
        for bad in ["due:", "due:2026-13-01", "due:2026-02-30", "due:soon", "due:<", "due:<+d", "due:+7", "due:2026-1-5", "due:<+999999d", "due:<<+7d"] {
            assert!(err(bad).reason.starts_with("due"), "{bad}: {:?}", err(bad));
        }
    }

    // spec: QUERY-004
    #[test]
    fn query_004_text_has_wildcards_in_lower_case() {
        assert_eq!(sh("Ban*a b?d"), "'ban*a b?d'");
        assert_eq!(err("*").reason, "a pattern of * and ? alone matches everything");
        assert_eq!(err("???").span, 0..3);
        assert_eq!(sh("*a"), "'*a'");
    }

    // spec: QUERY-005
    #[test]
    fn query_005_a_prefix_chooses_the_sources_and_a_lane_refuses_it() {
        let c = check("fm: milk #todo");
        assert_eq!(c.prefix.map(|s| s.kinds().count()), Some(2));
        assert_eq!(shape(c.expr().unwrap()), "(and 'milk' #todo)");
        assert_eq!(
            c.source(match c.expr().unwrap() {
                Expr::And(v) => match &v[1] {
                    Expr::Term(t) => t,
                    _ => unreachable!(),
                },
                _ => unreachable!(),
            }),
            "#todo",
            "spans are bytes of the whole text, prefix included"
        );
        assert_eq!(check("m:").tree, Ok(None), "a prefix alone is blank");
        assert_eq!(check("  ").tree, Ok(None));
        assert_eq!(check("from:ann").prefix, None, "a field word of mail is no prefix");
        let lane = Checked::lane("status:doing", Prefixes::default());
        assert!(lane.error().is_none());
        let lane = Checked::lane("f: status:doing", Prefixes::default());
        let e = lane.error().expect("a prefix in a lane");
        assert_eq!((e.span.clone(), e.reason.as_str()), (0..3, "a lane searches notes only: no prefix"));
    }

    // spec: QUERY-007, QUERY-008
    #[test]
    fn query_008_syntax_errors_name_their_place_and_reason() {
        let e = |t: &str| {
            let e = err(t);
            (t[e.span.clone()].to_string(), e.reason)
        };
        assert_eq!(e("(a OR b"), ("(".into(), "this ( is not closed".into()));
        assert_eq!(e("a OR b)"), (")".into(), "no ( for this )".into()));
        assert_eq!(e("a AND"), ("AND".into(), "AND needs a term after it".into()));
        assert_eq!(e("a OR"), ("OR".into(), "OR needs a term after it".into()));
        assert_eq!(e("NOT"), ("NOT".into(), "NOT needs a term after it".into()));
        assert_eq!(e("AND a"), ("AND".into(), "AND needs a term before it".into()));
        assert_eq!(e("a OR OR b"), ("OR".into(), "OR needs a term after it".into()));
        assert_eq!(e("OR b"), ("OR".into(), "OR needs a term before it".into()));
        assert_eq!(e("a AND OR b"), ("AND".into(), "AND needs a term after it".into()));
        assert_eq!(e("()"), ("()".into(), "nothing inside the parentheses".into()));
        assert_eq!(e("a ( )"), ("( )".into(), "nothing inside the parentheses".into()));
        assert_eq!(e("(a OR )"), ("OR".into(), "OR needs a term after it".into()));
        assert_eq!(e("("), ("(".into(), "a term is missing".into()));
        assert_eq!(e(")"), (")".into(), "no ( for this )".into()));
        assert_eq!(e("priority:urgent"), ("priority:urgent".into(), "priority is one of highest, high, medium, low, lowest".into()));
        assert_eq!(e("due:soon x").0, "due:soon");
        assert_eq!(e("links:Note").0, "links:Note");
        assert_eq!(e("path:").1, "path: needs a value");
        assert_eq!(e("priority:").1, "priority: needs a value");
        assert_eq!(e("a \"b").1, "the quote is not closed");
        for ok in ["priority:high", "priority:LOWEST", "priority:medium"] {
            assert!(check(ok).error().is_none(), "{ok}");
        }
    }

    // spec: QUERY-009
    #[test]
    fn query_009_limits_are_syntax_errors() {
        let long = "a".repeat(MAX_QUERY_CHARS);
        assert!(check(&long).error().is_none(), "500 characters are the limit");
        let e = err(&format!("{long}b"));
        assert_eq!((e.span.clone(), e.reason.as_str()), (500..501, "a query has at most 500 characters"));
        assert_eq!(err(&"é".repeat(501)).span, 1000..1002, "characters, not bytes");

        let terms = |n: usize| (0..n).map(|i| format!("#t{i}")).collect::<Vec<_>>().join(" ");
        assert!(check(&terms(32)).error().is_none());
        assert_eq!(err(&terms(33)).reason, "a query has at most 32 terms");
        assert_eq!(err(&format!("{} OR -z", terms(32))).reason, "a query has at most 32 terms", "a minus term counts");

        let nest = |n: usize| format!("{}a{}", "(".repeat(n), ")".repeat(n));
        assert!(check(&nest(8)).error().is_none());
        assert_eq!(err(&nest(9)).reason, "nested more than 8 levels");
        assert_eq!(err(&format!("{}a", "NOT ".repeat(9))).reason, "nested more than 8 levels");
        assert_eq!(err(&format!("{}a{}", "-(".repeat(9), ")".repeat(9))).reason, "nested more than 8 levels", "a minus before a group nests like a NOT");
        assert!(check(&format!("{}a", "NOT ".repeat(8))).error().is_none());

        assert!(check("a*b*c*d*e*f*g*h*i").error().is_none(), "eight stars");
        assert_eq!(err("a*b*c*d*e*f*g*h*i*").reason, "a pattern has at most 8 *");
        assert_eq!(err("a*a*a*a*a*a b*b*b*b*b").reason, "a pattern has at most 8 *", "a run of words is one pattern");
        assert!(check("a*a*a*a*a*a #x b*b*b*b*b").error().is_none(), "the limit is per pattern");
    }

    // spec: QUERY-013
    #[test]
    fn query_013_a_text_is_parsed_once_not_every_frame() {
        let mut cache = Cache::default();
        let p = Prefixes::default();
        for _ in 0..5 {
            assert!(cache.get("#a OR b", p).expr().is_some());
        }
        assert_eq!(cache.parses, 1, "the same text, the same result");
        assert!(cache.get("#a OR (b", p).error().is_some());
        assert_eq!(cache.parses, 2, "a new text is parsed");
        cache.get("#a OR (b", Prefixes { notes: 'n', ..p });
        assert_eq!(cache.parses, 3, "other letters of the prefixes make another reading");
    }

    // spec: QUERY-011
    // (the property test of the whole search is in `search_all::matcher`; this is the parser's half)
    proptest! {
        #![proptest_config(config())]
        #[test]
        fn query_011_any_text_is_refused_or_read_within_the_limits(s in query_text()) {
            let c = check(&s);
            match &c.tree {
                Ok(Some(e)) => {
                    prop_assert!(s.chars().count() <= MAX_QUERY_CHARS);
                    prop_assert!(terms_in(e) <= MAX_TERMS);
                    prop_assert!(spans_in(e, &s));
                }
                Ok(None) => prop_assert!(Prefixes::default().parse(&s).text.trim().is_empty()),
                Err(e) => {
                    prop_assert!(e.span.start <= e.span.end && e.span.end <= s.len());
                    prop_assert!(s.is_char_boundary(e.span.start) && s.is_char_boundary(e.span.end), "{e:?}");
                    prop_assert!(!e.reason.is_empty());
                }
            }
        }
    }

    fn terms_in(e: &Expr) -> usize {
        match e {
            Expr::And(v) | Expr::Or(v) => v.iter().map(terms_in).sum(),
            Expr::Not(e) => terms_in(e),
            Expr::Term(_) => 1,
        }
    }

    fn spans_in(e: &Expr, s: &str) -> bool {
        match e {
            Expr::And(v) | Expr::Or(v) => v.iter().all(|e| spans_in(e, s)),
            Expr::Not(e) => spans_in(e, s),
            Expr::Term(t) => t.src.start <= t.src.end && s.get(t.src.clone()).is_some(),
        }
    }
}
