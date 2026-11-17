//! The text transforms of the formatting toolbar (EDIT-027): what a button does to the note's text
//! and selection. Pure functions on a string and a byte selection, no UI.

use crate::colour_tags::{self, Kind, Run};
use std::ops::Range;

/// A toolbar command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    Bold,
    Italic,
    Strike,
    Highlight,
    Code,
    /// 1 to 6; 0 is normal text
    Heading(u8),
    Bullet,
    Numbered,
    Task,
    Quote,
    CodeBlock,
    Rule,
    WikiLink,
    WebLink,
    /// starts the `[!` file picker
    Embed,
    Table,
    /// a highlight colour; `None` takes the highlight off
    HighlightColour(Option<[u8; 3]>),
    /// a font colour; `None` takes it off
    FontColour(Option<[u8; 3]>),
    Clear,
}

/// The new text and the new selection (bytes).
pub type Out = (String, Range<usize>);

/// Do `cmd` to `text` with the selection `sel` (bytes; a cursor is an empty range). `None`: nothing to do.
pub fn apply(cmd: Cmd, text: &str, sel: Range<usize>) -> Option<Out> {
    let sel = clamp(text, sel);
    match cmd {
        Cmd::Bold => inline(text, sel, &Style::Wrap { open: "**", close: "**" }),
        Cmd::Italic => inline(text, sel, &Style::Wrap { open: "*", close: "*" }),
        Cmd::Strike => inline(text, sel, &Style::Wrap { open: "~~", close: "~~" }),
        Cmd::Highlight => inline(text, sel, &Style::Wrap { open: "==", close: "==" }),
        Cmd::Code => inline(text, sel, &Style::Wrap { open: "`", close: "`" }),
        Cmd::WikiLink => inline(text, sel, &Style::Wrap { open: "[[", close: "]]" }),
        Cmd::WebLink => web_link(text, sel),
        Cmd::HighlightColour(c) => inline(text, sel, &Style::Colour(Kind::Mark, c)),
        Cmd::FontColour(c) => inline(text, sel, &Style::Colour(Kind::Font, c)),
        Cmd::Heading(n) => lines(text, sel, |ps| heading(ps, n.min(6))),
        Cmd::Bullet => lines(text, sel, bullet),
        Cmd::Numbered => lines(text, sel, numbered),
        Cmd::Task => lines(text, sel, task),
        Cmd::Quote => lines(text, sel, quote),
        Cmd::CodeBlock => code_block(text, sel),
        Cmd::Rule => rule(text, sel),
        Cmd::Embed => Some(insert(text, sel.end, "[!", 2..2)),
        Cmd::Table => table(text, sel),
        Cmd::Clear => clear(text, sel),
    }
}

/// Whether `cmd` can be done to the text of a table cell edited in the grid (EDIT-030): the inline
/// commands. The line commands, the table and the embed (it starts the note's file picker) make no sense there.
pub fn works_in_a_cell(cmd: Cmd) -> bool {
    matches!(cmd, Cmd::Bold | Cmd::Italic | Cmd::Strike | Cmd::Highlight | Cmd::Code | Cmd::WikiLink | Cmd::WebLink | Cmd::HighlightColour(_) | Cmd::FontColour(_) | Cmd::Clear)
}

/// Whether `cmd` is "on" at the selection, for the pressed look of a button: the selection (or the
/// cursor) is inside the markers of an inline command, or the line has the marker of a line command.
pub fn is_on(cmd: Cmd, text: &str, sel: Range<usize>) -> bool {
    let sel = clamp(text, sel);
    let style = match cmd {
        Cmd::Bold => Style::Wrap { open: "**", close: "**" },
        Cmd::Italic => Style::Wrap { open: "*", close: "*" },
        Cmd::Strike => Style::Wrap { open: "~~", close: "~~" },
        Cmd::Highlight => Style::Wrap { open: "==", close: "==" },
        Cmd::Code => Style::Wrap { open: "`", close: "`" },
        Cmd::Heading(_) | Cmd::Bullet | Cmd::Numbered | Cmd::Task | Cmd::Quote => {
            let (start, end) = line_bounds(text, sel.start);
            let p = parse(&text[start..end]);
            return match cmd {
                Cmd::Heading(n) => p.marker == Marker::Heading(n.min(6)),
                Cmd::Bullet => matches!(p.marker, Marker::Bullet),
                Cmd::Numbered => matches!(p.marker, Marker::Numbered),
                Cmd::Task => matches!(p.marker, Marker::Task(_)),
                _ => p.quote_end > 0,
            };
        }
        _ => return false,
    };
    let (start, end) = line_bounds(text, sel.start);
    let line = &text[start..end];
    let seg = (sel.start - start).min(end - start)..(sel.end.min(end) - start).min(end - start);
    style.is_on(line, &seg)
}

fn clamp(text: &str, sel: Range<usize>) -> Range<usize> {
    let fix = |mut i: usize| {
        i = i.min(text.len());
        while !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (fix(sel.start), fix(sel.end));
    a.min(b)..a.max(b)
}

/// The start and end (before the line break) of the line holding byte `at`.
fn line_bounds(text: &str, at: usize) -> (usize, usize) {
    let start = text[..at].rfind('\n').map_or(0, |p| p + 1);
    let mut end = text[at..].find('\n').map_or(text.len(), |p| at + p);
    if text[start..end].ends_with('\r') {
        end -= 1;
    }
    (start, end)
}

/// `text` with `ins` put at `at`, and the selection `sel` (relative to `ins`) selected.
fn insert(text: &str, at: usize, ins: &str, sel: Range<usize>) -> Out {
    let mut out = String::with_capacity(text.len() + ins.len());
    out.push_str(&text[..at]);
    out.push_str(ins);
    out.push_str(&text[at..]);
    (out, at + sel.start..at + sel.end)
}

// ---------------------------------------------------------------- lines

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Marker {
    None,
    Heading(u8),
    Bullet,
    Numbered,
    /// done
    Task(bool),
}

/// A line cut into `quote` + `indent` + `marker` + content.
#[derive(Clone, Debug)]
struct Parts {
    /// end of the `> ` prefix
    quote_end: usize,
    /// end of the indentation after it
    indent_end: usize,
    marker: Marker,
    /// start of the content
    content: usize,
}

fn parse(line: &str) -> Parts {
    let b = line.as_bytes();
    // `> `, repeated; up to three spaces may come before each
    let mut quote_end = 0;
    let mut i = 0;
    loop {
        let mut j = i;
        while j < b.len() && j - i < 3 && b[j] == b' ' {
            j += 1;
        }
        if b.get(j) == Some(&b'>') {
            i = j + 1;
            if b.get(i) == Some(&b' ') {
                i += 1;
            }
            quote_end = i;
        } else {
            break;
        }
    }
    let rest = &line[quote_end..];
    let indent = rest.len() - rest.trim_start_matches([' ', '\t']).len();
    let indent_end = quote_end + indent;
    let body = &line[indent_end..];
    let (marker, len) = if indent == 0 && crate::markdown::heading_level(body).is_some() {
        let n = crate::markdown::heading_level(body).unwrap_or(1);
        (Marker::Heading(n as u8), n + 1)
    } else if let Some(r) = ["- ", "* ", "+ "].iter().find(|m| body.starts_with(**m)).map(|_| &body[2..]) {
        match ["[ ] ", "[x] ", "[X] "].iter().position(|t| r.starts_with(*t)) {
            Some(k) => (Marker::Task(k > 0), 6),
            None => (Marker::Bullet, 2),
        }
    } else {
        let digits = body.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && body[digits..].starts_with(". ") {
            (Marker::Numbered, digits + 2)
        } else {
            (Marker::None, 0)
        }
    };
    Parts { quote_end, indent_end, marker, content: indent_end + len }
}

/// The lines a selection touches: byte ranges of their text (without the line break).
fn touched(text: &str, sel: &Range<usize>) -> Vec<Range<usize>> {
    let (first, _) = line_bounds(text, sel.start);
    // a selection that ends at the start of a line does not touch that line
    let last_at = if sel.end > sel.start && text[..sel.end].ends_with('\n') { sel.end - 1 } else { sel.end };
    let mut out = Vec::new();
    let mut at = first;
    loop {
        let (s, e) = line_bounds(text, at);
        out.push(s..e);
        let next = text[at..].find('\n').map(|p| at + p + 1);
        match next {
            Some(n) if n <= last_at => at = n,
            _ => break,
        }
    }
    out
}

/// One touched line the command may change.
struct Target<'a> {
    line: &'a str,
    parts: Parts,
}

/// Run a line command: `f` gets the target lines (not in fenced code, and not blank when there are
/// several) and returns the new text of each.
fn lines(text: &str, sel: Range<usize>, f: impl Fn(&[Target]) -> Vec<String>) -> Option<Out> {
    let touched = touched(text, &sel);
    let fenced = crate::markdown::fenced_code_lines(text);
    let first_line = text[..touched[0].start].matches('\n').count();
    let many = touched.len() > 1;
    let idx: Vec<usize> = (0..touched.len()).filter(|k| !fenced.get(first_line + k).copied().unwrap_or(false) && !(many && text[touched[*k].clone()].trim().is_empty())).collect();
    if idx.is_empty() {
        return None;
    }
    let targets: Vec<Target> = idx
        .iter()
        .map(|k| {
            let line = &text[touched[*k].clone()];
            Target { line, parts: parse(line) }
        })
        .collect();
    let new = f(&targets);
    let head = &text[..touched[0].start];
    let mut out = String::from(head);
    let mut cursor_at = None;
    for (k, r) in touched.iter().enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let start = out.len();
        match idx.iter().position(|i| *i == k) {
            Some(t) => {
                out.push_str(&new[t]);
                if sel.is_empty() {
                    let old = &targets[t].parts;
                    let fresh = parse(&new[t]);
                    cursor_at = Some(start + fresh.content + (sel.start - r.start).saturating_sub(old.content));
                }
            }
            None => out.push_str(&text[r.clone()]),
        }
    }
    let end = out.len();
    out.push_str(&text[touched.last()?.end..]);
    let sel = match cursor_at {
        Some(c) => c..c,
        None => head.len()..end,
    };
    Some((out, sel))
}

fn content<'a>(t: &Target<'a>) -> &'a str {
    &t.line[t.parts.content..]
}

fn quote_of<'a>(t: &Target<'a>) -> &'a str {
    &t.line[..t.parts.quote_end]
}

fn indent_of<'a>(t: &Target<'a>) -> &'a str {
    &t.line[t.parts.quote_end..t.parts.indent_end]
}

fn heading(ts: &[Target], n: u8) -> Vec<String> {
    let all = n > 0 && ts.iter().all(|t| t.parts.marker == Marker::Heading(n));
    ts.iter()
        .map(|t| {
            if n == 0 && !matches!(t.parts.marker, Marker::Heading(_)) {
                return t.line.to_string();
            }
            let head = if n == 0 || all { String::new() } else { format!("{} ", "#".repeat(n as usize)) };
            // a heading starts its line: no indentation
            let indent = if head.is_empty() && !matches!(t.parts.marker, Marker::Heading(_)) { indent_of(t) } else { "" };
            format!("{}{}{}{}", quote_of(t), indent, head, content(t))
        })
        .collect()
}

fn bullet(ts: &[Target]) -> Vec<String> {
    let all = ts.iter().all(|t| t.parts.marker == Marker::Bullet);
    ts.iter().map(|t| format!("{}{}{}{}", quote_of(t), indent_of(t), if all { "" } else { "- " }, content(t))).collect()
}

fn numbered(ts: &[Target]) -> Vec<String> {
    let all = ts.iter().all(|t| t.parts.marker == Marker::Numbered);
    ts.iter().enumerate().map(|(k, t)| format!("{}{}{}{}", quote_of(t), indent_of(t), if all { String::new() } else { format!("{}. ", k + 1) }, content(t))).collect()
}

fn task(ts: &[Target]) -> Vec<String> {
    let all = ts.iter().all(|t| matches!(t.parts.marker, Marker::Task(_)));
    ts.iter().map(|t| if matches!(t.parts.marker, Marker::Task(_)) && !all { t.line.to_string() } else { format!("{}{}{}{}", quote_of(t), indent_of(t), if all { "" } else { "- [ ] " }, content(t)) }).collect()
}

fn quote(ts: &[Target]) -> Vec<String> {
    let all = ts.iter().all(|t| t.parts.quote_end > 0);
    ts.iter()
        .map(|t| {
            if all {
                // take off the outermost level
                let q = quote_of(t);
                let gt = q.find('>').unwrap_or(0);
                let after = &q[gt + 1..];
                let after = after.strip_prefix(' ').unwrap_or(after);
                format!("{after}{}", &t.line[t.parts.quote_end..])
            } else if t.parts.quote_end > 0 {
                t.line.to_string()
            } else {
                format!("> {}", t.line)
            }
        })
        .collect()
}

// ---------------------------------------------------------------- inline

/// What an inline command does to one line.
enum Style {
    Wrap { open: &'static str, close: &'static str },
    Colour(Kind, Option<[u8; 3]>),
    Clear,
}

/// One line's new text and where the formatted part is now.
type LineOut = (String, Range<usize>);

/// The length of the run of `c` ending at `end` (or starting at `start`).
fn run_before(s: &str, end: usize, c: u8) -> usize {
    s.as_bytes()[..end].iter().rev().take_while(|b| **b == c).count()
}
fn run_after(s: &str, start: usize, c: u8) -> usize {
    s.as_bytes()[start..].iter().take_while(|b| **b == c).count()
}

impl Style {
    /// Where the markers of a wrap are, if the segment is wrapped: (left marker, right marker).
    fn wrapped(&self, line: &str, seg: &Range<usize>) -> Option<(Range<usize>, Range<usize>)> {
        let Style::Wrap { open, close } = self else { return None };
        let k = open.len();
        if *open == "*" || *open == "**" {
            // runs of `*`: one is italic, two bold, three both
            let ok = |n: usize| if k == 1 { n % 2 == 1 } else { n >= 2 };
            let (before, after) = (run_before(line, seg.start, b'*'), run_after(line, seg.end, b'*'));
            if ok(before) && ok(after) && !(seg.is_empty() && before != after) {
                return Some((seg.start - k..seg.start, seg.end..seg.end + k));
            }
            if !seg.is_empty() {
                let (l, r) = (run_after(line, seg.start, b'*'), run_before(line, seg.end, b'*'));
                if l + r <= seg.len() && ok(l) && ok(r) {
                    return Some((seg.start..seg.start + k, seg.end - k..seg.end));
                }
            }
            return None;
        }
        if line[..seg.start].ends_with(open) && line[seg.end..].starts_with(close) {
            return Some((seg.start - k..seg.start, seg.end..seg.end + close.len()));
        }
        let s = &line[seg.clone()];
        (s.len() >= open.len() + close.len() && s.starts_with(open) && s.ends_with(close)).then(|| (seg.start..seg.start + open.len(), seg.end - close.len()..seg.end))
    }

    fn is_on(&self, line: &str, seg: &Range<usize>) -> bool {
        match self {
            Style::Wrap { .. } => self.wrapped(line, seg).is_some(),
            Style::Colour(kind, _) => colour_run(line, seg, *kind).is_some(),
            Style::Clear => false,
        }
    }

    /// Apply to one segment of one line; `remove`: take the formatting off (a toggle that is on).
    fn apply(&self, line: &str, seg: &Range<usize>, remove: bool) -> LineOut {
        match self {
            Style::Wrap { open, close } => {
                if remove {
                    if let Some((l, r)) = self.wrapped(line, seg) {
                        let mut out = String::from(&line[..l.start]);
                        out.push_str(&line[l.end..r.start]);
                        out.push_str(&line[r.end..]);
                        return (out, l.start..l.start + (r.start - l.end));
                    }
                }
                let mut out = String::from(&line[..seg.start]);
                out.push_str(open);
                out.push_str(&line[seg.clone()]);
                out.push_str(close);
                out.push_str(&line[seg.end..]);
                (out, seg.start + open.len()..seg.end + open.len())
            }
            Style::Colour(kind, c) => colour_line(line, seg, *kind, *c),
            Style::Clear => clear_line(line, seg),
        }
    }
}

/// The text of `line` from which the tags of `runs` that are in `drop` are cut; `seg` moves along.
fn cut(line: &str, mut cuts: Vec<Range<usize>>, seg: &Range<usize>) -> LineOut {
    cuts.sort_by_key(|r| r.start);
    let mut out = String::new();
    let mut at = 0;
    let (mut a, mut b) = (seg.start, seg.end);
    for r in &cuts {
        out.push_str(&line[at..r.start]);
        at = r.end;
        let len = r.end - r.start;
        if r.end <= seg.start {
            a -= len;
            b -= len;
        } else if r.start >= seg.end {
        } else if r.start >= seg.start && r.end <= seg.end {
            b -= len;
        }
    }
    out.push_str(&line[at..]);
    (out, a..b)
}

/// The run of `kind` whose coloured text holds `seg` (the innermost).
fn colour_run(line: &str, seg: &Range<usize>, kind: Kind) -> Option<Run> {
    colour_tags::find(line).into_iter().filter(|r| r.kind == kind && r.inner().start <= seg.start && seg.end <= r.inner().end).min_by_key(|r| r.inner().len())
}

/// Colour (or, with `None`, un-colour) the segment of a line.
fn colour_line(line: &str, seg: &Range<usize>, kind: Kind, colour: Option<[u8; 3]>) -> LineOut {
    let (open_of, close) = match kind {
        Kind::Mark => (colour_tags::mark_open as fn([u8; 3]) -> String, colour_tags::MARK_CLOSE),
        _ => (colour_tags::font_open as fn([u8; 3]) -> String, colour_tags::FONT_CLOSE),
    };
    // a plain `==highlight==` around the text goes when it gets a colour or is cleared
    let (line, seg) = if kind == Kind::Mark {
        let w = Style::Wrap { open: "==", close: "==" };
        match w.wrapped(line, seg) {
            Some(_) => w.apply(line, seg, true),
            None => (line.to_string(), seg.clone()),
        }
    } else {
        (line.to_string(), seg.clone())
    };
    let (line, seg) = (line.as_str(), seg);
    let runs = colour_tags::find(line);
    // text inside a run of this kind: its colour is replaced, the rest of the run keeps its own
    let inside = if seg.is_empty() {
        runs.iter().filter(|r| r.kind == kind && r.inner().start <= seg.start && seg.start <= r.inner().end).min_by_key(|r| r.inner().len())
    } else {
        runs.iter().filter(|r| r.kind == kind && r.inner().start <= seg.start && seg.end <= r.inner().end).min_by_key(|r| r.inner().len())
    };
    if let Some(r) = inside {
        let inner = r.inner();
        let mut part = if seg.is_empty() { inner.clone() } else { seg.clone() };
        // the pieces of the run must not cut through tags inside it: grow over the runs that part cuts into or sits in
        loop {
            let q = runs.iter().find(|q| *q != r && q.open.start >= inner.start && q.close.end <= inner.end && q.open.start < part.end && part.start < q.close.end && !(part.start <= q.open.start && q.close.end <= part.end));
            match q {
                Some(q) => part = part.start.min(q.open.start)..part.end.max(q.close.end),
                None => break,
            }
        }
        let old_open = &line[r.open.clone()];
        let mut out = String::from(&line[..r.open.start]);
        let mut sel = 0..0;
        if part.start > inner.start {
            out.push_str(old_open);
            out.push_str(&line[inner.start..part.start]);
            out.push_str(close);
        }
        match colour {
            Some(c) => {
                let o = open_of(c);
                out.push_str(&o);
                sel.start = out.len();
                out.push_str(&line[part.clone()]);
                sel.end = out.len();
                out.push_str(close);
            }
            None => {
                sel.start = out.len();
                out.push_str(&line[part.clone()]);
                sel.end = out.len();
            }
        }
        if part.end < inner.end {
            out.push_str(old_open);
            out.push_str(&line[part.end..inner.end]);
            out.push_str(close);
        }
        out.push_str(&line[r.close.end..]);
        return (out, sel);
    }
    let Some(c) = colour else {
        if seg.is_empty() {
            return (line.to_string(), seg.clone());
        }
        // no run holds the segment: drop the runs of this kind inside it
        let (mut a, mut b) = (seg.start, seg.end);
        expand(&runs, &mut a, &mut b);
        let cuts: Vec<Range<usize>> = runs.iter().filter(|r| r.kind == kind && a <= r.open.start && r.close.end <= b).flat_map(|r| [r.open.clone(), r.close.clone()]).collect();
        return cut(line, cuts, &(a..b));
    };
    if seg.is_empty() {
        let o = open_of(c);
        let mut out = String::from(&line[..seg.start]);
        out.push_str(&o);
        let at = out.len();
        out.push_str(close);
        out.push_str(&line[seg.end..]);
        return (out, at..at);
    }
    // the colour goes round the segment, grown to whole runs it cuts into; runs of this kind inside it go
    let (mut a, mut b) = (seg.start, seg.end);
    expand(&runs, &mut a, &mut b);
    let cuts: Vec<Range<usize>> = runs.iter().filter(|r| r.kind == kind && a <= r.open.start && r.close.end <= b).flat_map(|r| [r.open.clone(), r.close.clone()]).collect();
    let (stripped, inner) = cut(line, cuts, &(a..b));
    let o = open_of(c);
    let mut out = String::from(&stripped[..inner.start]);
    out.push_str(&o);
    let s = out.len();
    out.push_str(&stripped[inner.clone()]);
    let e = out.len();
    out.push_str(close);
    out.push_str(&stripped[inner.end..]);
    (out, s..e)
}

/// Grow `a..b` over every run (of any kind) that it cuts into, so that the tags stay nested.
fn expand(runs: &[Run], a: &mut usize, b: &mut usize) {
    loop {
        let mut grew = false;
        for r in runs {
            let total = r.open.start..r.close.end;
            let disjoint = total.end <= *a || total.start >= *b;
            let holds_all = total.start >= *a && total.end <= *b;
            let in_inner = r.inner().start <= *a && *b <= r.inner().end;
            if !disjoint && !holds_all && !in_inner {
                *a = (*a).min(total.start);
                *b = (*b).max(total.end);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
}

/// Clear the formatting of a segment: colour tags and the markers of bold, italic, strikethrough,
/// highlight and code in it and right around it.
fn clear_line(line: &str, seg: &Range<usize>) -> LineOut {
    let (mut line, mut seg) = (line.to_string(), seg.clone());
    for kind in [Kind::Mark, Kind::Font] {
        (line, seg) = colour_line(&line, &seg, kind, None);
    }
    // markers just outside the segment, as long as both sides have the same one
    for m in ["**", "__", "~~", "==", "`", "*", "_"] {
        while line[..seg.start].ends_with(m) && line[seg.end..].starts_with(m) {
            let n = m.len();
            line = format!("{}{}{}", &line[..seg.start - n], &line[seg.start..seg.end], &line[seg.end + n..]);
            seg = seg.start - n..seg.end - n;
        }
    }
    let s = line[seg.clone()].to_string();
    let cleaned = strip_markers(&s);
    line.replace_range(seg.clone(), &cleaned);
    let end = seg.start + cleaned.len();
    (line, seg.start..end)
}

/// `s` without its `**`, `__`, `~~`, `==`, backticks, `*` and `_` that mark italics.
fn strip_markers(s: &str) -> String {
    let mut t = s.to_string();
    for m in ["**", "__", "~~", "==", "`", "*"] {
        t = t.replace(m, "");
    }
    // `_italic_`: an underscore that opens (not inside a word) with its closing one
    let b: Vec<char> = t.chars().collect();
    let mut drop = vec![false; b.len()];
    let mut i = 0;
    while i < b.len() {
        if b[i] == '_' && (i == 0 || !b[i - 1].is_alphanumeric()) && b.get(i + 1).is_some_and(|c| !c.is_whitespace() && *c != '_') {
            if let Some(j) = (i + 1..b.len()).find(|j| b[*j] == '_' && !b[*j - 1].is_whitespace() && b.get(*j + 1).is_none_or(|c| !c.is_alphanumeric())) {
                drop[i] = true;
                drop[j] = true;
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    b.iter().zip(&drop).filter(|(_, d)| !**d).map(|(c, _)| *c).collect()
}

/// The table lines of `text` by line number: `true` for a delimiter row (EDIT-024).
fn table_lines(text: &str) -> std::collections::HashMap<usize, bool> {
    let mut out = std::collections::HashMap::new();
    for b in crate::table::find_tables(text) {
        let t = b.table_lines();
        for l in t.clone() {
            out.insert(l, l == t.start + 1);
        }
    }
    out
}

/// The pieces of `a..b` in a table line: one per cell it covers, without the pipes and the blanks
/// around the cell text, so that markers never span a `|` (raba/shale#234).
fn cell_pieces(line: &str, a: usize, b: usize) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    for c in crate::table::cell_ranges(line) {
        let (mut x, mut y) = (a.max(c.start), b.min(c.end));
        while x < y && line[x..].starts_with(char::is_whitespace) {
            x += line[x..].chars().next().map_or(1, char::len_utf8);
        }
        while y > x && line[..y].ends_with(char::is_whitespace) {
            y -= line[..y].chars().next_back().map_or(1, char::len_utf8);
        }
        if x < y {
            out.push(x..y);
        }
    }
    out
}

/// Run an inline command over the selection, line by line (the markers do not span lines), and in
/// a table cell by cell (the delimiter row is left alone).
fn inline(text: &str, sel: Range<usize>, style: &Style) -> Option<Out> {
    let touched = touched(text, &sel);
    let fenced = crate::markdown::fenced_code_lines(text);
    let tables = table_lines(text);
    let first_line = text[..touched[0].start].matches('\n').count();
    // the segments: the part of each line the selection covers, without the line's own markers and blanks
    let mut segs: Vec<(Range<usize>, Vec<Range<usize>>)> = Vec::new();
    for (k, r) in touched.iter().enumerate() {
        let n = first_line + k;
        if fenced.get(n).copied().unwrap_or(false) || tables.get(&n) == Some(&true) {
            continue;
        }
        let line = &text[r.clone()];
        let from = (sel.start.max(r.start) - r.start).min(line.len());
        let to = sel.end.min(r.end) - r.start;
        if sel.is_empty() {
            segs.push((r.clone(), std::iter::once(from..from).collect()));
            continue;
        }
        let (mut a, mut b) = (from.max(parse(line).content), to);
        if a >= b {
            continue;
        }
        if tables.contains_key(&n) {
            let pieces = cell_pieces(line, a, b);
            if !pieces.is_empty() {
                segs.push((r.clone(), pieces));
            }
            continue;
        }
        while a < b && line[a..].starts_with(char::is_whitespace) {
            a += line[a..].chars().next().map_or(1, char::len_utf8);
        }
        while b > a && line[..b].ends_with(char::is_whitespace) {
            b -= line[..b].chars().next_back().map_or(1, char::len_utf8);
        }
        if a < b {
            segs.push((r.clone(), std::iter::once(a..b).collect()));
        }
    }
    if segs.is_empty() {
        return None;
    }
    let toggle = matches!(style, Style::Wrap { .. });
    let all_on = toggle && segs.iter().all(|(r, ps)| ps.iter().all(|s| style.is_on(&text[r.clone()], s)));
    let mut out = String::from(&text[..segs[0].0.start]);
    let mut at = segs[0].0.start;
    let mut sel_out: Option<Range<usize>> = None;
    let mut last = 0..0;
    for (r, pieces) in &segs {
        out.push_str(&text[at..r.start]);
        let orig = &text[r.clone()];
        // the pieces of a line from the last to the first, so that the earlier offsets stay valid
        let mut line = orig.to_string();
        let (mut first_in, mut last_in): (Option<usize>, Option<(usize, usize)>) = (None, None);
        for s in pieces.iter().rev() {
            let (new, inner) = if toggle && !all_on && style.is_on(orig, s) { (line.clone(), s.clone()) } else { style.apply(&line, s, all_on) };
            line = new;
            if last_in.is_none() {
                last_in = Some((inner.end, line.len()));
            }
            first_in = Some(inner.start);
        }
        let base = out.len();
        out.push_str(&line);
        let (end, len_then) = last_in.unwrap_or((0, line.len()));
        // what the earlier pieces added moves the end of the last one
        let abs = base + first_in.unwrap_or(0)..(base + end + line.len()).saturating_sub(len_then);
        sel_out.get_or_insert(abs.clone());
        last = abs;
        at = r.end;
    }
    out.push_str(&text[at..]);
    let first = sel_out?;
    let result = first.start..last.end;
    (out != text).then_some((out, result))
}

/// `[text](url)`: wraps the selection and selects `url`; a selected URL becomes the link's target;
/// a selected link goes back to its text.
fn web_link(text: &str, sel: Range<usize>) -> Option<Out> {
    let s = &text[sel.clone()];
    // a whole `[label](url)` selected: back to its label
    if let Some(label) = s.strip_prefix('[').and_then(|r| r.strip_suffix(')')).and_then(|r| r.split_once("](")).map(|(l, _)| l) {
        let mut out = String::from(&text[..sel.start]);
        out.push_str(label);
        let end = out.len();
        out.push_str(&text[sel.end..]);
        return Some((out, sel.start..end));
    }
    if s.contains('\n') {
        return None;
    }
    let is_url = crate::lists::is_web_url(s.trim());
    let (label, url) = if is_url { ("", s.trim()) } else { (s, "url") };
    let mut ins = format!("[{label}](");
    let from = ins.len();
    ins.push_str(url);
    let to = ins.len();
    ins.push(')');
    let mut out = String::from(&text[..sel.start]);
    out.push_str(&ins);
    out.push_str(&text[sel.end..]);
    Some(if is_url { (out, sel.start + 1..sel.start + 1) } else { (out, sel.start + from..sel.start + to) })
}

// ---------------------------------------------------------------- blocks

fn code_block(text: &str, sel: Range<usize>) -> Option<Out> {
    let ts = touched(text, &sel);
    let (a, b) = (ts[0].start, ts.last()?.end);
    let blank = sel.is_empty() && text[a..b].trim().is_empty();
    // already fenced: the fences go
    let before = text[..a].strip_suffix('\n').map(|t| &t[t.rfind('\n').map_or(0, |p| p + 1)..]);
    let after = text[b..].strip_prefix('\n').map(|t| &t[..t.find('\n').unwrap_or(t.len())]);
    if let (Some(f1), Some(f2)) = (before, after) {
        if f1.trim_start().starts_with("```") && f2.trim() == "```" && !blank {
            let start = a - 1 - f1.len();
            let end = b + 1 + f2.len();
            let mut out = String::from(&text[..start]);
            out.push_str(&text[a..b]);
            let e = out.len();
            out.push_str(&text[end..]);
            return Some((out, start..e));
        }
    }
    if blank {
        let mut out = String::from(&text[..a]);
        out.push_str("```\n");
        let at = out.len();
        out.push_str("\n```");
        out.push_str(&text[b..]);
        return Some((out, at..at));
    }
    if sel.is_empty() {
        // an empty fence after the cursor's line
        let mut out = String::from(&text[..b]);
        out.push_str("\n```\n");
        let at = out.len();
        out.push_str("\n```");
        out.push_str(&text[b..]);
        return Some((out, at..at));
    }
    let mut out = String::from(&text[..a]);
    out.push_str("```\n");
    let s = out.len();
    out.push_str(&text[a..b]);
    let e = out.len();
    out.push_str("\n```");
    out.push_str(&text[b..]);
    Some((out, s..e))
}

/// A line of its own after the cursor's line: `ins` (a block that ends with a line break), with a
/// blank line before it unless the line is blank; the cursor goes after it.
fn block_after(text: &str, at: usize, ins: &str, select: Option<Range<usize>>) -> Out {
    let (s, e) = line_bounds(text, at);
    let blank = text[s..e].trim().is_empty();
    let (pos, lead) = if blank { (s, "") } else { (e, "\n\n") };
    let mut out = String::from(&text[..pos]);
    out.push_str(lead);
    let base = out.len();
    out.push_str(ins);
    let end = out.len();
    let rest = &text[if blank { e } else { pos }..];
    // the line break of the line the block replaces or follows already ends it
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    out.push_str(rest);
    match select {
        Some(r) => (out, base + r.start..base + r.end),
        None => (out, end..end),
    }
}

fn rule(text: &str, sel: Range<usize>) -> Option<Out> {
    Some(block_after(text, sel.end, "---\n", None))
}

const TABLE: &str = "| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n|  |  |\n";

fn table(text: &str, sel: Range<usize>) -> Option<Out> {
    Some(block_after(text, sel.end, TABLE, Some(2..10)))
}

// ---------------------------------------------------------------- clearing

/// A line without its quote, indentation, heading, list and task markers.
fn strip_all(mut line: &str) -> &str {
    loop {
        let p = parse(line);
        if p.content == 0 {
            return line;
        }
        line = &line[p.content..];
    }
}

fn clear(text: &str, sel: Range<usize>) -> Option<Out> {
    // the markers of the lines first, then the inline formatting of what is selected
    let by_line = lines(text, sel.clone(), |ts| ts.iter().map(|t| strip_all(t.line).to_string()).collect());
    let (t1, s1) = match &by_line {
        Some((t, s)) if t != text => (t.as_str(), s.clone()),
        _ => (text, sel.clone()),
    };
    let inl = if sel.is_empty() { None } else { inline(t1, s1.clone(), &Style::Clear) };
    match (inl, by_line) {
        (Some(o), _) => Some(o),
        (None, Some((t, s))) if t != text => Some((t, s)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Apply with the selection marked in `src` by `[` and `]` (a cursor by `|`); the result in the same notation.
    fn run(cmd: Cmd, src: &str) -> String {
        let (text, sel) = parse_marks(src);
        match apply(cmd, &text, sel) {
            Some((t, s)) => show(&t, s),
            None => "<none>".to_string(),
        }
    }

    fn parse_marks(src: &str) -> (String, Range<usize>) {
        let mut text = String::new();
        let (mut a, mut b) = (None, None);
        for c in src.chars() {
            match c {
                '⟦' => a = Some(text.len()),
                '⟧' => b = Some(text.len()),
                '|' => {
                    a = Some(text.len());
                    b = Some(text.len())
                }
                c => text.push(c),
            }
        }
        (text, a.unwrap()..b.unwrap())
    }

    fn show(text: &str, sel: Range<usize>) -> String {
        if sel.is_empty() {
            format!("{}|{}", &text[..sel.start], &text[sel.start..])
        } else {
            format!("{}⟦{}⟧{}", &text[..sel.start], &text[sel.clone()], &text[sel.end..])
        }
    }

    // spec: EDIT-027
    #[test]
    fn inline_buttons_wrap_the_selection_and_unwrap_it_again() {
        assert_eq!(run(Cmd::Bold, "a ⟦word⟧ b"), "a **⟦word⟧** b");
        assert_eq!(run(Cmd::Bold, "a **⟦word⟧** b"), "a ⟦word⟧ b", "markers around the selection");
        assert_eq!(run(Cmd::Bold, "a ⟦**word**⟧ b"), "a ⟦word⟧ b", "markers inside the selection");
        assert_eq!(run(Cmd::Strike, "⟦x⟧"), "~~⟦x⟧~~");
        assert_eq!(run(Cmd::Highlight, "⟦x⟧"), "==⟦x⟧==");
        assert_eq!(run(Cmd::Code, "⟦x⟧"), "`⟦x⟧`");
        assert_eq!(run(Cmd::WikiLink, "⟦Note⟧"), "[[⟦Note⟧]]");
        assert_eq!(run(Cmd::WikiLink, "[[⟦Note⟧]]"), "⟦Note⟧");
        assert_eq!(run(Cmd::Italic, "⟦é ü⟧"), "*⟦é ü⟧*");
    }

    // spec: EDIT-027
    #[test]
    fn with_no_selection_the_markers_go_in_with_the_cursor_between_and_come_out_when_empty() {
        assert_eq!(run(Cmd::Bold, "a |b"), "a **|**b");
        assert_eq!(run(Cmd::Italic, "a |"), "a *|*");
        assert_eq!(run(Cmd::Code, "|"), "`|`");
        assert_eq!(run(Cmd::Bold, "a **|** b"), "a | b", "an empty pair is taken off");
        assert_eq!(run(Cmd::WikiLink, "|"), "[[|]]");
    }

    // spec: EDIT-027
    #[test]
    fn bold_and_italic_do_not_take_each_other_apart() {
        assert_eq!(run(Cmd::Italic, "**⟦x⟧**"), "***⟦x⟧***", "italic on bold text adds");
        assert_eq!(run(Cmd::Italic, "***⟦x⟧***"), "**⟦x⟧**", "italic off keeps bold");
        assert_eq!(run(Cmd::Bold, "***⟦x⟧***"), "*⟦x⟧*", "bold off keeps italic");
        assert_eq!(run(Cmd::Bold, "*⟦x⟧*"), "***⟦x⟧***");
        assert_eq!(run(Cmd::Italic, "*⟦x⟧*"), "⟦x⟧");
        assert_eq!(run(Cmd::Italic, "⟦***x***⟧"), "⟦**x**⟧");
    }

    // spec: EDIT-027
    #[test]
    fn a_selection_over_several_lines_is_wrapped_line_by_line_without_the_list_markers() {
        assert_eq!(run(Cmd::Bold, "⟦- one\n- two\n> three⟧"), "- **⟦one**\n- **two**\n> **three⟧**");
        assert_eq!(run(Cmd::Bold, "- ⟦**one**\n- **two**⟧"), "- ⟦one\n- two⟧");
        // one line done and the other not: the missing one gets it
        assert_eq!(run(Cmd::Bold, "⟦**a**\nb⟧"), "⟦**a**\n**b⟧**", "the selection is the text inside the markers");
        assert_eq!(run(Cmd::Bold, "⟦ spaced \n⟧x"), " **⟦spaced⟧** \nx", "blanks at the edges stay outside the markers");
    }

    // spec: EDIT-027
    #[test]
    fn heading_buttons_set_replace_and_toggle_off() {
        assert_eq!(run(Cmd::Heading(2), "ab|c"), "## ab|c");
        assert_eq!(run(Cmd::Heading(3), "## ab|c"), "### ab|c", "another level replaces");
        assert_eq!(run(Cmd::Heading(2), "## ab|c"), "ab|c", "the same level toggles off");
        assert_eq!(run(Cmd::Heading(0), "### ab|c"), "ab|c", "normal text");
        assert_eq!(run(Cmd::Heading(0), "ab|c"), "ab|c");
        assert_eq!(run(Cmd::Heading(1), "- ab|c"), "# ab|c", "a list marker is replaced");
        assert_eq!(run(Cmd::Heading(1), "> ab|c"), "> # ab|c", "inside a quote");
        assert_eq!(run(Cmd::Heading(1), "|"), "# |");
        assert_eq!(run(Cmd::Heading(6), "⟦a\nb⟧"), "⟦###### a\n###### b⟧");
        assert_eq!(run(Cmd::Heading(9), "a|"), "###### a|", "levels above 6 are 6");
        assert_eq!(run(Cmd::Heading(0), "- a|"), "- a|", "normal text leaves a list alone");
    }

    // spec: EDIT-027
    #[test]
    fn list_buttons_apply_to_every_line_touched_and_toggle_off() {
        assert_eq!(run(Cmd::Bullet, "a|b\nc"), "- a|b\nc");
        assert_eq!(run(Cmd::Bullet, "a⟦b\nc⟧"), "⟦- ab\n- c⟧", "the selection becomes the lines");
        assert_eq!(run(Cmd::Bullet, "⟦- a\n- b⟧"), "⟦a\nb⟧");
        assert_eq!(run(Cmd::Bullet, "⟦- a\nb⟧"), "⟦- a\n- b⟧", "mixed: all get it");
        assert_eq!(run(Cmd::Numbered, "⟦a\nb\n\nc⟧"), "⟦1. a\n2. b\n\n3. c⟧", "blank lines in a selection stay blank");
        assert_eq!(run(Cmd::Numbered, "⟦1. a\n2. b⟧"), "⟦a\nb⟧");
        assert_eq!(run(Cmd::Numbered, "⟦- a\n- b⟧"), "⟦1. a\n2. b⟧");
        assert_eq!(run(Cmd::Task, "⟦a\n- [x] b⟧"), "⟦- [ ] a\n- [x] b⟧", "a ticked task stays ticked");
        assert_eq!(run(Cmd::Task, "⟦- [ ] a\n- [x] b⟧"), "⟦a\nb⟧");
        assert_eq!(run(Cmd::Task, "  - a|"), "  - [ ] a|", "indentation stays");
        assert_eq!(run(Cmd::Bullet, "  1. a|"), "  - a|");
        assert_eq!(run(Cmd::Bullet, "|"), "- |");
    }

    // spec: EDIT-027
    #[test]
    fn a_selection_ending_at_the_start_of_a_line_does_not_touch_that_line() {
        assert_eq!(run(Cmd::Quote, "⟦a\n⟧b"), "⟦> a⟧\nb");
        assert_eq!(run(Cmd::Quote, "⟦a\nb⟧"), "⟦> a\n> b⟧");
    }

    // spec: EDIT-027
    #[test]
    fn quote_adds_a_level_to_lines_without_one_and_takes_the_outer_level_off() {
        assert_eq!(run(Cmd::Quote, "a|b"), "> a|b");
        assert_eq!(run(Cmd::Quote, "> a|b"), "a|b");
        assert_eq!(run(Cmd::Quote, "> > a|b"), "> a|b");
        assert_eq!(run(Cmd::Quote, "⟦> a\nb⟧"), "⟦> a\n> b⟧");
        assert_eq!(run(Cmd::Quote, "- a|"), "> - a|");
    }

    // spec: EDIT-027
    #[test]
    fn lines_in_fenced_code_are_left_alone_by_the_line_buttons() {
        assert_eq!(run(Cmd::Bullet, "```\na|\n```"), "<none>");
        assert_eq!(run(Cmd::Bold, "```\n⟦a⟧\n```"), "<none>");
        assert_eq!(run(Cmd::Bullet, "⟦a\n```\nb\n```⟧"), "⟦- a\n```\nb\n```⟧");
    }

    // spec: EDIT-027
    #[test]
    fn code_block_fences_the_lines_and_toggles() {
        assert_eq!(run(Cmd::CodeBlock, "x\n⟦a\nb⟧\ny"), "x\n```\n⟦a\nb⟧\n```\ny");
        assert_eq!(run(Cmd::CodeBlock, "x\n```\n⟦a⟧\n```\ny"), "x\n⟦a⟧\ny");
        assert_eq!(run(Cmd::CodeBlock, "|"), "```\n|\n```");
        assert_eq!(run(Cmd::CodeBlock, "ab|c"), "abc\n```\n|\n```");
    }

    // spec: EDIT-027
    #[test]
    fn rule_and_table_are_blocks_of_their_own_after_the_line() {
        assert_eq!(run(Cmd::Rule, "a|b\nc"), "ab\n\n---\n|c");
        assert_eq!(run(Cmd::Rule, "ab|"), "ab\n\n---\n|");
        assert_eq!(run(Cmd::Rule, "|\nc"), "---\n|c");
        let (t, s) = apply(Cmd::Table, "intro", 5..5).unwrap();
        assert_eq!(t, "intro\n\n| Column 1 | Column 2 |\n| --- | --- |\n|  |  |\n|  |  |\n");
        assert_eq!(&t[s], "Column 1", "the first header cell is selected");
        let tables = crate::table::find_tables(&t);
        assert_eq!(tables.len(), 1, "the table is a table to Shale");
        assert_eq!((tables[0].table.header.len(), tables[0].table.rows.len()), (2, 2));
    }

    // spec: EDIT-027
    #[test]
    fn links_wrap_the_selection_and_the_embed_starts_the_file_picker() {
        assert_eq!(run(Cmd::WebLink, "⟦text⟧"), "[text](⟦url⟧)");
        assert_eq!(run(Cmd::WebLink, "|"), "[](⟦url⟧)");
        assert_eq!(run(Cmd::WebLink, "⟦https://example.com⟧"), "[|](https://example.com)");
        assert_eq!(run(Cmd::WebLink, "⟦[text](https://x.y)⟧"), "⟦text⟧");
        assert_eq!(run(Cmd::WebLink, "⟦a\nb⟧"), "<none>");
        assert_eq!(run(Cmd::Embed, "a|"), "a[!|");
        assert_eq!(run(Cmd::Embed, "a⟦b⟧c"), "ab[!|c", "inserted after the selection");
    }

    // spec: EDIT-028
    #[test]
    fn a_highlight_colour_wraps_in_a_mark_and_a_font_colour_in_a_span() {
        let y = Some([255, 255, 0]);
        assert_eq!(run(Cmd::HighlightColour(y), "a ⟦word⟧"), "a <mark style=\"background: #FFFF00;\">⟦word⟧</mark>");
        assert_eq!(run(Cmd::FontColour(Some([255, 0, 0])), "⟦w⟧"), "<span style=\"color: #FF0000;\">⟦w⟧</span>");
        assert_eq!(run(Cmd::HighlightColour(y), "|"), "<mark style=\"background: #FFFF00;\">|</mark>");
        assert_eq!(run(Cmd::HighlightColour(None), "|"), "<none>");
        // each line of a selection gets its own pair (tags do not span lines)
        assert_eq!(run(Cmd::FontColour(Some([0, 0, 255])), "⟦a\nb⟧"), "<span style=\"color: #0000FF;\">⟦a</span>\n<span style=\"color: #0000FF;\">b⟧</span>");
    }

    // spec: EDIT-028
    #[test]
    fn a_colour_on_coloured_text_replaces_the_colour_instead_of_nesting() {
        let g = Some([0x39, 0xff, 0x14]);
        let m = |c: &str, t: &str| format!("<mark style=\"background: {c};\">{t}</mark>");
        assert_eq!(run(Cmd::HighlightColour(g), &m("#FFFF00", "⟦word⟧")), m("#39FF14", "⟦word⟧"), "the text of a mark selected");
        assert_eq!(run(Cmd::HighlightColour(g), &m("#FFFF00", "wo|rd")), m("#39FF14", "⟦word⟧"), "a cursor in a mark recolours the whole mark");
        assert_eq!(run(Cmd::HighlightColour(g), "<mark style='background:#ff0'>⟦word⟧</mark>"), m("#39FF14", "⟦word⟧"), "a tolerant tag is read and the canonical one written");
        // a part of the mark: the mark is split in three
        assert_eq!(run(Cmd::HighlightColour(g), &m("#FFFF00", "a⟦b⟧c")), format!("{}{}{}", m("#FFFF00", "a"), m("#39FF14", "⟦b⟧"), m("#FFFF00", "c")));
        // the whole mark with its tags selected
        assert_eq!(run(Cmd::HighlightColour(g), &format!("⟦{}⟧", m("#FFFF00", "word"))), m("#39FF14", "⟦word⟧"));
        // text with two marks in it: one mark round all, no nesting
        assert_eq!(run(Cmd::HighlightColour(g), &format!("⟦{} and {}⟧", m("#111111", "a"), m("#222222", "b"))), m("#39FF14", "⟦a and b⟧"));
        // a selection that cuts into a mark grows over the whole mark
        assert_eq!(run(Cmd::HighlightColour(g), "x ⟦y <mark style=\"background: #FFFF00;\">a⟧b</mark>"), format!("x {}", m("#39FF14", "⟦y ab⟧")));
    }

    // spec: EDIT-028
    #[test]
    fn no_highlight_takes_off_the_mark_and_the_plain_highlight() {
        let m = |t: &str| format!("<mark style=\"background: #FFFF00;\">{t}</mark>");
        assert_eq!(run(Cmd::HighlightColour(None), &m("⟦word⟧")), "⟦word⟧");
        assert_eq!(run(Cmd::HighlightColour(None), &m("wo|rd")), "⟦word⟧", "the cursor's mark");
        assert_eq!(run(Cmd::HighlightColour(None), "a ==⟦word⟧== b"), "a ⟦word⟧ b");
        assert_eq!(run(Cmd::HighlightColour(None), "⟦==word==⟧"), "⟦word⟧");
        assert_eq!(run(Cmd::HighlightColour(None), &m("a⟦b⟧c")), format!("{}⟦b⟧{}", m("a"), m("c")));
        assert_eq!(run(Cmd::HighlightColour(Some([255, 0, 255])), "==⟦word⟧=="), "<mark style=\"background: #FF00FF;\">⟦word⟧</mark>", "a colour replaces the plain highlight");
        assert_eq!(run(Cmd::HighlightColour(None), "⟦plain⟧"), "<none>", "nothing to take off");
        // a span inside the mark is not cut in two: the mark ends and starts around it
        let src = "<mark style=\"background: #FFFF00;\"><span style=\"color: #FF0000;\">a⟦b⟧c</span></mark>";
        let (t, _) = apply(Cmd::HighlightColour(None), &parse_marks(src).0, parse_marks(src).1).unwrap();
        assert_eq!(t, "<span style=\"color: #FF0000;\">abc</span>");
    }

    // spec: EDIT-028, EDIT-029
    #[test]
    fn mark_and_span_nest_properly_and_never_cross() {
        let src = "<mark style=\"background: #FFFF00;\">ab</mark>";
        let at = src.find("ab").unwrap();
        let (t, _) = apply(Cmd::FontColour(Some([255, 0, 0])), src, at..at + 1).unwrap();
        assert!(colour_tags::find(&t).len() == 2 && t.contains("</span>b</mark>"), "{t}");
        // a span cut by the end of the selection: the mark grows over the whole span
        let src = "a <span style=\"color: #FF0000;\">b c</span> d";
        let (t, _) = apply(Cmd::HighlightColour(Some([255, 255, 0])), src, 0..src.find('c').unwrap() + 1).unwrap();
        let runs = colour_tags::find(&t);
        assert_eq!(runs.len(), 2, "{t}");
        let mark = runs.iter().find(|r| r.kind == Kind::Mark).unwrap();
        let font = runs.iter().find(|r| r.kind == Kind::Font).unwrap();
        assert!(mark.open.start < font.open.start && font.close.end <= mark.close.start, "properly nested: {t}");
    }

    // spec: EDIT-027
    #[test]
    fn clear_formatting_removes_markers_colours_and_the_line_markers() {
        assert_eq!(run(Cmd::Clear, "⟦**a** *b* ~~c~~ ==d== `e`⟧"), "⟦a b c d e⟧");
        assert_eq!(run(Cmd::Clear, "**⟦a⟧**"), "⟦a⟧", "markers around the selection");
        assert_eq!(run(Cmd::Clear, "⟦<span style=\"color: #FF0000;\"><mark style=\"background: #FFFF00;\">a</mark></span>⟧"), "⟦a⟧");
        assert_eq!(run(Cmd::Clear, "⟦## - [ ] a_b and _it_⟧"), "⟦a_b and it⟧", "snake_case keeps its underscore");
        assert_eq!(run(Cmd::Clear, "⟦# Title\n> - [ ] **x**⟧"), "⟦Title\nx⟧");
        assert_eq!(run(Cmd::Clear, "# a|b"), "a|b");
        assert_eq!(run(Cmd::Clear, "a|b"), "<none>");
    }

    // spec: EDIT-027
    #[test]
    fn is_on_tells_what_is_applied_at_the_cursor() {
        let on = |cmd, src: &str| {
            let (t, s) = parse_marks(src);
            is_on(cmd, &t, s)
        };
        assert!(on(Cmd::Bold, "**⟦a⟧**"));
        assert!(!on(Cmd::Bold, "⟦a⟧"));
        assert!(on(Cmd::Italic, "*⟦a⟧*"));
        assert!(!on(Cmd::Italic, "**⟦a⟧**"));
        assert!(on(Cmd::Heading(2), "## a|"));
        assert!(!on(Cmd::Heading(1), "## a|"));
        assert!(on(Cmd::Bullet, "- a|"));
        assert!(on(Cmd::Task, "- [x] a|"));
        assert!(on(Cmd::Numbered, "3. a|"));
        assert!(on(Cmd::Quote, "> a|"));
        assert!(!on(Cmd::Rule, "a|"));
    }

    // spec: EDIT-027
    #[test]
    fn odd_selections_never_panic_or_split_a_character() {
        for cmd in [
            Cmd::Bold,
            Cmd::Italic,
            Cmd::Code,
            Cmd::Heading(1),
            Cmd::Bullet,
            Cmd::Quote,
            Cmd::CodeBlock,
            Cmd::Rule,
            Cmd::Table,
            Cmd::WebLink,
            Cmd::WikiLink,
            Cmd::Embed,
            Cmd::Clear,
            Cmd::HighlightColour(Some([1, 2, 3])),
            Cmd::FontColour(None),
        ] {
            for text in ["", "\n", "é", "aé\r\nb\n", "<mark style=\"background: #fff\">é</mark>", "```\n"] {
                for a in 0..=text.len() + 2 {
                    for b in 0..=text.len() + 2 {
                        if let Some((t, s)) = apply(cmd, text, a..b) {
                            assert!(s.start <= s.end && s.end <= t.len() && t.is_char_boundary(s.start) && t.is_char_boundary(s.end), "{cmd:?} {text:?} {a}..{b}");
                        }
                    }
                }
            }
        }
    }

    // spec: EDIT-027, EDIT-029
    #[test]
    fn in_a_table_the_markers_go_round_each_cell_and_never_across_a_pipe() {
        let text = "| A | B |\n|---|---|\n| bar | baz |\n";
        let from = text.find("bar").unwrap();
        let to = text.find("baz").unwrap() + 3;
        let (t, sel) = apply(Cmd::FontColour(Some([240, 138, 28])), text, from..to).unwrap();
        assert_eq!(t, "| A | B |\n|---|---|\n| <span style=\"color: #F08A1C;\">bar</span> | <span style=\"color: #F08A1C;\">baz</span> |\n");
        assert_eq!(&t[sel], "bar</span> | <span style=\"color: #F08A1C;\">baz");
        let tables = crate::table::find_tables(&t);
        assert_eq!(tables[0].table.rows[0].len(), 2, "still two cells");
        // the whole table: the delimiter row is left alone, every cell gets its own markers
        let (t, _) = apply(Cmd::Bold, text, 0..text.len()).unwrap();
        assert_eq!(t, "| **A** | **B** |\n|---|---|\n| **bar** | **baz** |\n");
        // and a second press takes them off again
        let (back, _) = apply(Cmd::Bold, &t, 0..t.len()).unwrap();
        assert_eq!(back, text);
        // part of a cell
        let at = text.find("az").unwrap();
        assert_eq!(apply(Cmd::Italic, text, at..at + 2).unwrap().0, "| A | B |\n|---|---|\n| bar | b*az* |\n");
    }

    // spec: EDIT-030
    #[test]
    fn only_the_inline_commands_work_in_a_table_cell() {
        let inline = [Cmd::Bold, Cmd::Italic, Cmd::Strike, Cmd::Highlight, Cmd::Code, Cmd::WikiLink, Cmd::WebLink, Cmd::HighlightColour(None), Cmd::FontColour(Some([1, 2, 3])), Cmd::Clear];
        let other = [Cmd::Heading(0), Cmd::Heading(2), Cmd::Bullet, Cmd::Numbered, Cmd::Task, Cmd::Quote, Cmd::CodeBlock, Cmd::Rule, Cmd::Embed, Cmd::Table];
        assert!(inline.into_iter().all(works_in_a_cell));
        assert!(!other.into_iter().any(works_in_a_cell));
    }
}
