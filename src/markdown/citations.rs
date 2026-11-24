//! The one scanner for citations, labels and references (EDIT-122, ADR-0039 §1 to §4, #645).
//!
//! `@` has no meaning in most notes (it is an e-mail address, a mention, a word), so every rule
//! that decides whether an `@` starts a citation lives here, and the editor, the TeX writer, the
//! built-in PDF, the site and the Word filter take their reading from [`scan`]: they cannot
//! disagree. The scanner is a pure function of the note's text and of the one question "does the
//! bibliography in effect hold this key?"; it never panics and every range it returns lies inside
//! the text on char boundaries (LINK-018).

use super::{fenced_code_lines, find_math, front_matter_lines, front_matter_values, heading_level, inline_code_ranges, lines_with_offsets, md_destination, MathSpan};
use std::ops::Range;

/// What a reference or a label is about: the word before the colon (`fig:results`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Figure,
    Table,
    Equation,
    Section,
    /// an abbreviation of the block ```` ```abbreviations ```` (ADR-0039 §8): a reference only
    Abbreviation,
    /// a term of the block ```` ```glossary ````: a reference only
    Glossary,
}

impl Kind {
    /// The reserved prefix, without the colon.
    pub const fn prefix(self) -> &'static str {
        match self {
            Kind::Figure => "fig",
            Kind::Table => "tbl",
            Kind::Equation => "eq",
            Kind::Section => "sec",
            Kind::Abbreviation => "abbr",
            Kind::Glossary => "gls",
        }
    }

    fn from_prefix(p: &str) -> Option<Kind> {
        [Kind::Figure, Kind::Table, Kind::Equation, Kind::Section, Kind::Abbreviation, Kind::Glossary].into_iter().find(|k| k.prefix() == p)
    }

    /// Whether a label (`{#kind:id}`) can be of this kind; abbreviations and terms are written in blocks.
    pub const fn is_labelled(self) -> bool {
        !matches!(self, Kind::Abbreviation | Kind::Glossary)
    }
}

/// What an `@…` points at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// a key of the bibliography
    Citation(String),
    /// a label of the document or a key of the abbreviations and the glossary: `@fig:results`
    Reference { kind: Kind, id: String },
}

/// One `@key` of a group, with what is written around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// the bytes of `@key` or `-@key`
    pub range: Range<usize>,
    pub target: Target,
    /// `-@key`: the author is left out
    pub suppress_author: bool,
    /// the text before the key (`see`), trimmed
    pub prefix: String,
    /// the text after the key, up to the next `;`, without its leading comma (`p. 12`), trimmed;
    /// for a bare `@key` the locator in the `[…]` that follows it
    pub suffix: String,
    /// For a citation: whether the bibliography in effect holds the key; `None` while no
    /// bibliography is in effect (the group is plain text then, ADR-0039 §4 rule 5, and the TeX
    /// export says that the note has citations and no bibliography). Always `None` for a reference.
    pub known: Option<bool>,
}

/// A bracketed group (`[@a; @b]`, `[see @a, p. 12]`) or a bare `@key` / `@fig:x`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// the whole group: the brackets included, or `@key` with the `[locator]` after it
    pub range: Range<usize>,
    pub bracketed: bool,
    /// at least one; the `;`-separated parts of a bracket that hold no key are left out
    pub items: Vec<Item>,
}

/// A label `{#fig:results}` at one of the places of ADR-0039 §2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    /// the bytes of `{#kind:id}`
    pub range: Range<usize>,
    /// one of the kinds for which [`Kind::is_labelled`] holds
    pub kind: Kind,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Group(Group),
    Label(Label),
}

impl Token {
    pub fn range(&self) -> &Range<usize> {
        match self {
            Token::Group(g) => &g.range,
            Token::Label(l) => &l.range,
        }
    }
}

fn is_key_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// How many bytes at the start of `s` are a key (ADR-0039 §1): a letter, digit or `_`, then
/// letters, digits, `_` and the internal punctuation `: . - + /` where a letter, digit or `_`
/// follows (so a full stop at the end of a sentence is not part of the key). 0: no key.
pub fn key_len(s: &str) -> usize {
    let mut chars = s.char_indices().peekable();
    let Some((_, first)) = chars.next().filter(|(_, c)| is_key_char(*c)) else { return 0 };
    let mut end = first.len_utf8();
    while let Some(&(i, c)) = chars.peek() {
        if is_key_char(c) {
            end = i + c.len_utf8();
            chars.next();
        } else if matches!(c, ':' | '.' | '-' | '+' | '/') {
            let mut ahead = chars.clone();
            ahead.next();
            match ahead.peek() {
                Some(&(j, n)) if is_key_char(n) => {
                    end = j + n.len_utf8();
                    chars.next();
                    chars.next();
                }
                _ => break,
            }
        } else {
            break;
        }
    }
    end
}

/// The reserved prefix a key starts with (`fig:results`: [`Kind::Figure`]): such a key is always a
/// reference (ADR-0039 §4 rule 6), and a `.bib` key that has one cannot be cited.
pub fn reserved_prefix(key: &str) -> Option<Kind> {
    let (prefix, id) = key.split_once(':')?;
    Kind::from_prefix(prefix).filter(|_| !id.is_empty())
}

/// A `{#kind:id}` at the start of `s`: the kind, the id and the length in bytes. The id holds
/// letters, digits, `_`, `-`, `.` and `:`.
pub fn parse_label(s: &str) -> Option<(Kind, String, usize)> {
    let inner = s.strip_prefix("{#")?;
    let close = inner.find('}')?;
    let (kind, id) = inner[..close].split_once(':')?;
    let kind = Kind::from_prefix(kind).filter(|k| k.is_labelled())?;
    (!id.is_empty() && id.chars().all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))).then(|| (kind, id.to_string(), close + 3))
}

/// Notes whose text is somebody else's: nothing in them is a citation or a label (rule 1).
fn is_foreign(text: &str) -> bool {
    front_matter_values(text).iter().any(|(k, v)| k == "type" && v.first().is_some_and(|t| matches!(t.trim().to_lowercase().as_str(), "email" | "social-post")))
}

/// Read the citations, references and labels of a note, in the order of the text.
///
/// `bib` answers whether the bibliography in effect holds a key (case-sensitive); `None` says that
/// no bibliography is in effect. A bare `@key` is a citation only when `bib` holds the key; a
/// bracketed group is reported either way, with `known` telling which case it is. References
/// (`fig:`, `tbl:`, `eq:`, `sec:`, `abbr:`, `gls:`) are always reported. See ADR-0039 §4 for the rules.
pub fn scan(text: &str, bib: Option<&dyn Fn(&str) -> bool>) -> Vec<Token> {
    let mut out = Vec::new();
    if is_foreign(text) {
        return out;
    }
    let lines = lines_with_offsets(text);
    let fenced = fenced_code_lines(text);
    let math = find_math(text);
    for span in math.iter().filter(|m| text[m.range.clone()].starts_with("```")) {
        // the info string of a math fence may carry the label: ```math {#eq:energy}
        let first = &text[span.range.clone()];
        let line = first.lines().next().unwrap_or("");
        if let Some(at) = line.find("{#") {
            if let Some((kind @ Kind::Equation, id, len)) = parse_label(&line[at..]) {
                out.push(Token::Label(Label { range: span.range.start + at..span.range.start + at + len, kind, id }));
            }
        }
    }
    for (li, (off, line)) in lines.iter().enumerate().skip(front_matter_lines(text)) {
        if fenced.get(li).copied().unwrap_or(false) {
            continue;
        }
        let mask = prose_mask(line, *off, &math);
        let before = li.checked_sub(1).and_then(|p| lines.get(p)).filter(|_| !fenced.get(li - 1).copied().unwrap_or(false)).map(|(_, l)| *l);
        labels(line, *off, before, &mask, &math, &mut out);
        cites(line, *off, &mask, bib, &mut out);
    }
    out.sort_by_key(|t| (t.range().start, t.range().end));
    out
}

/// The parts of a line that are not prose (ADR-0039 §4 rule 1), relative to the line: inline code,
/// math, wiki links, the target of a link, URLs, mail addresses, autolinks, HTML tags and comments.
fn prose_mask(line: &str, off: usize, math: &[MathSpan]) -> Vec<Range<usize>> {
    let mut mask = inline_code_ranges(line);
    for m in math {
        if m.range.start < off + line.len() && m.range.end > off {
            mask.push(m.range.start.saturating_sub(off)..(m.range.end - off).min(line.len()));
        }
    }
    let mut from = 0;
    while let Some(st) = line[from..].find("[[").map(|p| p + from) {
        let Some(e) = line[st + 2..].find("]]") else { break };
        mask.push(st..st + 2 + e + 2);
        from = st + 2 + e + 2;
    }
    let mut from = 0;
    while let Some(st) = line[from..].find("](").map(|p| p + from) {
        from = st + 2;
        if let Some((_, close)) = md_destination(&line[st + 2..]) {
            mask.push(st..st + 2 + close + 1);
            from = st + 2 + close + 1;
        }
    }
    let mut from = 0;
    while let Some(st) = line[from..].find("://").map(|p| p + from) {
        let scheme = line[..st].len() - line[..st].trim_end_matches(|c: char| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-')).len();
        let end = line[st..].find(|c: char| c.is_whitespace() || matches!(c, ']' | '<' | '>' | '"')).map_or(line.len(), |p| p + st);
        mask.push(st - scheme..end);
        from = end.max(st + 3);
    }
    let mut from = 0;
    while let Some(st) = line[from..].find("mailto:").map(|p| p + from) {
        let end = line[st..].find(|c: char| c.is_whitespace() || matches!(c, ']' | '<' | '>' | '"')).map_or(line.len(), |p| p + st);
        mask.push(st..end);
        from = end;
    }
    let mut from = 0;
    while let Some(st) = line[from..].find('<').map(|p| p + from) {
        from = st + 1;
        if line[st + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '/' || c == '!') {
            if let Some(e) = line[st..].find('>') {
                mask.push(st..st + e + 1);
                from = st + e + 1;
            }
        }
    }
    mask
}

fn masked(mask: &[Range<usize>], i: usize) -> bool {
    mask.iter().any(|r| r.contains(&i))
}

/// The labels of a prose line (ADR-0039 §2): after an embed on a line of its own, in the caption
/// line below a table, after a closing `$$`, at the end of a heading.
fn labels(line: &str, off: usize, before: Option<&str>, mask: &[Range<usize>], math: &[MathSpan], out: &mut Vec<Token>) {
    let body = line.trim_end();
    let Some(at) = body.rfind("{#").filter(|&at| !masked(mask, at)) else { return };
    let Some((kind, id, len)) = parse_label(&body[at..]).filter(|(_, _, len)| at + len == body.len()) else { return };
    let head = &body[..at];
    let ok = match kind {
        Kind::Section => heading_level(line).is_some(),
        Kind::Table => head.trim_start().starts_with(':') && before.is_some_and(|b| b.contains('|')),
        Kind::Equation => {
            let end = off + head.strip_suffix(' ').unwrap_or(head).len();
            math.iter().any(|m| m.display && m.range.end == end && text_ends_with_dollars(line, m, off))
        }
        Kind::Figure => {
            let lead = line.len() - line.trim_start().len();
            let embeds = super::find_links(line);
            embeds.iter().any(|l| l.embed && l.range.start == lead && l.range.end == head.strip_suffix(' ').unwrap_or(head).len())
        }
        _ => false,
    };
    if ok {
        out.push(Token::Label(Label { range: off + at..off + at + len, kind, id }));
    }
}

/// A display formula that ends the part of the line before a label ends in `$$` (not a fence).
fn text_ends_with_dollars(line: &str, m: &MathSpan, off: usize) -> bool {
    line.get(..m.range.end.saturating_sub(off)).is_some_and(|s| s.ends_with("$$"))
}

/// The `]` that closes the `[` at `open`: brackets nest, a backslash escapes, masked text is skipped.
fn find_close(line: &str, open: usize, mask: &[Range<usize>]) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while i < line.len() {
        let c = line[i..].chars().next()?;
        if masked(mask, i) {
            i += c.len_utf8();
            continue;
        }
        match c {
            '\\' => i += 1 + line[i + 1..].chars().next().map_or(0, char::len_utf8),
            '[' => {
                depth += 1;
                i += 1;
            }
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += c.len_utf8(),
        }
    }
    None
}

/// Whether an `@` after `prev` can start a citation (rule 2).
fn starts_after(prev: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_whitespace() || matches!(c, '(' | '[' | ';' | ',' | '"' | '\'' | '“' | '‘' | '„' | '«'))
}

/// `@ada@mastodon.social`: a key that another `@` and a letter or digit follow is a mention (rule 3).
fn is_mention(after_key: &str) -> bool {
    after_key.strip_prefix('@').is_some_and(|r| r.starts_with(|c: char| c.is_alphanumeric()))
}

fn target_of(key: &str) -> Target {
    match reserved_prefix(key) {
        Some(kind) => Target::Reference { kind, id: key[kind.prefix().len() + 1..].to_string() },
        None => Target::Citation(key.to_string()),
    }
}

fn known(target: &Target, bib: Option<&dyn Fn(&str) -> bool>) -> Option<bool> {
    match (target, bib) {
        (Target::Citation(key), Some(has)) => Some(has(key)),
        _ => None,
    }
}

/// Whether what follows a `]` makes the bracket a link, a reference link or a definition (rule 5).
fn is_link_tail(after: &str) -> bool {
    after.starts_with(['(', '[', ':'])
}

fn cites(line: &str, off: usize, mask: &[Range<usize>], bib: Option<&dyn Fn(&str) -> bool>, out: &mut Vec<Token>) {
    let mut i = 0;
    while i < line.len() {
        let Some(c) = line[i..].chars().next() else { break };
        if masked(mask, i) {
            i += c.len_utf8();
            continue;
        }
        match c {
            '\\' => i += 1 + line[i + 1..].chars().next().map_or(0, char::len_utf8),
            '[' => match bracket(line, off, i, mask, bib) {
                Bracket::Group(g, end) => {
                    out.push(Token::Group(g));
                    i = end;
                }
                Bracket::Skip(end) => i = end,
                Bracket::No => i += 1,
            },
            '@' => {
                let klen = if starts_after(line[..i].chars().next_back()) { key_len(&line[i + 1..]) } else { 0 };
                let end = i + 1 + klen;
                if klen == 0 {
                    i += 1;
                } else if is_mention(&line[end..]) {
                    i = end;
                } else {
                    i = bare(line, off, i, end, bib, mask, out);
                }
            }
            _ => i += c.len_utf8(),
        }
    }
}

/// A bare `@key` at `start..end`; returns where the scan goes on.
fn bare(line: &str, off: usize, start: usize, end: usize, bib: Option<&dyn Fn(&str) -> bool>, mask: &[Range<usize>], out: &mut Vec<Token>) -> usize {
    let key = &line[start + 1..end];
    let target = target_of(key);
    // a bare key is a citation only when the bibliography in effect holds it (rule 4)
    if matches!(target, Target::Citation(_)) && !bib.is_some_and(|has| has(key)) {
        return end;
    }
    let mut item = Item { range: off + start..off + end, known: known(&target, bib), target, suppress_author: false, prefix: String::new(), suffix: String::new() };
    let mut group_end = end;
    if matches!(item.target, Target::Citation(_)) {
        // `@key [p. 12]`: the locator after the key
        let rest = &line[end..];
        let open = end + rest.len() - rest.trim_start_matches(' ').len();
        if open > end && line[open..].starts_with('[') && !masked(mask, open) {
            if let Some(close) = find_close(line, open, mask) {
                let inner = &line[open + 1..close];
                if !inner.trim().is_empty() && !inner.starts_with(['@', '^']) && !is_link_tail(&line[close + 1..]) {
                    item.suffix = inner.trim().to_string();
                    group_end = close + 1;
                }
            }
        }
    }
    out.push(Token::Group(Group { range: off + start..off + group_end, bracketed: false, items: vec![item] }));
    group_end
}

enum Bracket {
    Group(Group, usize),
    /// a link, a reference link or a definition that holds `@`: nothing in it is a citation
    Skip(usize),
    No,
}

fn bracket(line: &str, off: usize, open: usize, mask: &[Range<usize>], bib: Option<&dyn Fn(&str) -> bool>) -> Bracket {
    let Some(close) = find_close(line, open, mask) else { return Bracket::No };
    let inner = &line[open + 1..close];
    if !inner.contains('@') {
        return Bracket::No;
    }
    if is_link_tail(&line[close + 1..]) {
        return Bracket::Skip(close + 1);
    }
    let mut items = Vec::new();
    let mut from = open + 1;
    let mut depth = 0usize;
    for (k, c) in line[open + 1..close].char_indices().map(|(k, c)| (k + open + 1, c)).chain(std::iter::once((close, ';'))) {
        match c {
            '[' if !masked(mask, k) => depth += 1,
            ']' if !masked(mask, k) => depth = depth.saturating_sub(1),
            ';' if depth == 0 && !masked(mask, k) => {
                items.extend(item_in(line, off, from, k, mask, bib));
                from = k + 1;
            }
            _ => {}
        }
    }
    if items.is_empty() {
        Bracket::No
    } else {
        Bracket::Group(Group { range: off + open..off + close + 1, bracketed: true, items }, close + 1)
    }
}

/// The first `@key` of the part `from..to` of the line (a part between two `;`) with its prefix
/// and suffix; a part without one is no item.
fn item_in(line: &str, off: usize, from: usize, to: usize, mask: &[Range<usize>], bib: Option<&dyn Fn(&str) -> bool>) -> Option<Item> {
    let part = &line[from..to];
    let mut skip_to = 0;
    for (p, c) in part.char_indices() {
        if p < skip_to || c != '@' || masked(mask, from + p) {
            continue;
        }
        let before = part[..p].chars().next_back();
        // `-@key` only where an `@` could start: the minus then stands at the start of the part or after a blank
        let minus = before == Some('-') && starts_after(part[..p - 1].chars().next_back());
        if !(starts_after(before) || minus) {
            continue;
        }
        let klen = key_len(&part[p + 1..]);
        if klen == 0 {
            continue;
        }
        let end = p + 1 + klen;
        if is_mention(&part[end..]) {
            skip_to = end;
            continue;
        }
        let key = &part[p + 1..end];
        let target = target_of(key);
        let start = if minus { p - 1 } else { p };
        let suffix = part[end..].trim();
        return Some(Item {
            range: off + from + start..off + from + end,
            known: known(&target, bib),
            target,
            suppress_author: minus,
            prefix: part[..start].trim().to_string(),
            suffix: suffix.strip_prefix(',').unwrap_or(suffix).trim().to_string(),
        });
    }
    None
}
