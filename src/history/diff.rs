//! The diffs of the comparison view (HIST-009…HIST-013, HIST-016): lines, words, the blocks of a
//! note for the track-changes view, and the patch text. No crate: a longest common subsequence
//! after the common start and end are cut off; a middle part too big for the table counts as
//! removed and inserted whole.

use std::collections::HashSet;

/// One step of an edit script from `a` to `b`: both indexes of a common element, the index in `a`
/// of a removed one, the index in `b` of an inserted one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Edit {
    Same(usize, usize),
    Del(usize),
    Ins(usize),
}

/// Above this many table cells a differing middle part is not compared element by element (HIST-016).
const MAX_CELLS: usize = 4_000_000;

/// The edit script from `a` to `b`: removed elements come before inserted ones within a changed part.
pub fn edits<T: Eq>(a: &[T], b: &[T]) -> Vec<Edit> {
    let start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let end = a[start..].iter().rev().zip(b[start..].iter().rev()).take_while(|(x, y)| x == y).count();
    let (ma, mb) = (&a[start..a.len() - end], &b[start..b.len() - end]);
    let mut out: Vec<Edit> = (0..start).map(|i| Edit::Same(i, i)).collect();
    let (n, m) = (ma.len(), mb.len());
    if n == 0 || m == 0 || (n + 1).saturating_mul(m + 1) > MAX_CELLS {
        out.extend((0..n).map(|i| Edit::Del(start + i)));
        out.extend((0..m).map(|j| Edit::Ins(start + j)));
    } else {
        // lcs[i][j]: the length of the longest common subsequence of ma[i..] and mb[j..]
        let w = m + 1;
        let mut lcs = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * w + j] = if ma[i] == mb[j] { lcs[(i + 1) * w + j + 1] + 1 } else { lcs[(i + 1) * w + j].max(lcs[i * w + j + 1]) };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n || j < m {
            if i < n && j < m && ma[i] == mb[j] {
                out.push(Edit::Same(start + i, start + j));
                i += 1;
                j += 1;
            } else if i < n && (j == m || lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
                out.push(Edit::Del(start + i));
                i += 1;
            } else {
                out.push(Edit::Ins(start + j));
                j += 1;
            }
        }
    }
    let tail_a = a.len() - end;
    let tail_b = b.len() - end;
    out.extend((0..end).map(|k| Edit::Same(tail_a + k, tail_b + k)));
    out
}

/// What happened to a piece of text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Same,
    Ins,
    Del,
}

/// A piece of text with what happened to it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub text: String,
    pub change: Change,
}

impl Span {
    fn new(text: &str, change: Change) -> Span {
        Span { text: text.to_string(), change }
    }
}

/// The pieces of a text: runs of white space and runs of other characters.
fn words(s: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut start = 0;
    let mut space = None;
    for (i, c) in s.char_indices() {
        let now = c.is_whitespace();
        if space.is_some_and(|was| was != now) {
            out.push(&s[start..i]);
            start = i;
        }
        space = Some(now);
    }
    if start < s.len() {
        out.push(&s[start..]);
    }
    out
}

fn push_span(out: &mut Vec<Span>, text: &str, change: Change) {
    match out.last_mut() {
        Some(last) if last.change == change => last.text.push_str(text),
        _ => out.push(Span::new(text, change)),
    }
}

/// The word-level diff of two texts as one sequence of spans: what is in both, what only the old
/// one has (`Del`) and what only the new one has (`Ins`).
pub fn word_spans(old: &str, new: &str) -> Vec<Span> {
    let (a, b) = (words(old), words(new));
    let mut out = vec![];
    for e in edits(&a, &b) {
        match e {
            Edit::Same(_, j) => push_span(&mut out, b[j], Change::Same),
            Edit::Del(i) => push_span(&mut out, a[i], Change::Del),
            Edit::Ins(j) => push_span(&mut out, b[j], Change::Ins),
        }
    }
    out
}

/// How much two texts share, 0 to 1: the words in common against the words of the longer one.
fn similarity(a: &str, b: &str) -> f32 {
    let (wa, wb): (Vec<&str>, Vec<&str>) = (words(a).into_iter().filter(|w| !w.starts_with(char::is_whitespace)).collect(), words(b).into_iter().filter(|w| !w.starts_with(char::is_whitespace)).collect());
    let longer = wa.len().max(wb.len());
    if longer == 0 {
        return 1.0;
    }
    let same = edits(&wa, &wb).iter().filter(|e| matches!(e, Edit::Same(..))).count();
    same as f32 / longer as f32
}

// ---------------------------------------------------------------------------------------------
// The blocks of a note

/// What a block of a note is (HIST-009).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Kind {
    Heading(u8),
    Paragraph,
    /// A list item: its depth (0 is the top) and its bullet or number.
    Item(usize, String),
    TableRow,
    Quote,
    Code,
    /// A line of the properties at the top of a note.
    Property,
    Rule,
}

impl Kind {
    /// What the hover of a gutter mark calls it (HIST-011).
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Heading(_) => "heading",
            Kind::Paragraph => "paragraph",
            Kind::Item(..) => "list item",
            Kind::TableRow => "table row",
            Kind::Quote => "quote",
            Kind::Code => "code line",
            Kind::Property => "property",
            Kind::Rule => "rule",
        }
    }

    /// What the fold line calls a run of these (HIST-010).
    fn plural(&self) -> &'static str {
        match self {
            Kind::Heading(_) => "headings",
            Kind::Paragraph => "paragraphs",
            Kind::Item(..) => "list items",
            Kind::TableRow => "table rows",
            Kind::Quote => "quotes",
            Kind::Code => "code lines",
            Kind::Property => "properties",
            Kind::Rule => "rules",
        }
    }
}

/// One block of a note, as the person reads it: a heading, a paragraph, a list item…
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Block {
    pub kind: Kind,
    /// The text without Markdown marks.
    pub text: String,
}

/// The text of a line without its Markdown marks: `**bold**`, `*em*`, `` `code` ``, `~~gone~~`, `==marked==`,
/// `[[Note|name]]`, `[text](url)`, `![[picture]]`.
pub fn plain(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let starts = |i: usize, pat: &str| pat.chars().enumerate().all(|(k, c)| chars.get(i + k) == Some(&c));
    let find = |from: usize, pat: &str| (from..chars.len()).find(|&k| starts(k, pat));
    while i < chars.len() {
        if starts(i, "![[") || starts(i, "[[") {
            let pic = chars[i] == '!';
            let open = if pic { i + 3 } else { i + 2 };
            if let Some(end) = find(open, "]]") {
                let inner: String = chars[open..end].iter().collect();
                let shown = inner.rsplit_once('|').map_or(inner.as_str(), |(_, alias)| alias);
                if pic {
                    out.push_str("\u{25a3} ");
                }
                out.push_str(shown);
                i = end + 2;
                continue;
            }
        }
        if (starts(i, "![") || chars[i] == '[') && !starts(i, "[[") {
            let pic = chars[i] == '!';
            let open = if pic { i + 2 } else { i + 1 };
            if let Some(mid) = find(open, "](") {
                if let Some(end) = find(mid + 2, ")") {
                    if pic {
                        out.push_str("\u{25a3} ");
                    }
                    out.extend(&chars[open..mid]);
                    i = end + 1;
                    continue;
                }
            }
        }
        if ["**", "__", "~~", "=="].iter().any(|p| starts(i, p)) {
            i += 2;
            continue;
        }
        if chars[i] == '`' || (chars[i] == '*' && chars.get(i + 1).is_some_and(|c| !c.is_whitespace())) || (chars[i] == '*' && i > 0 && !chars[i - 1].is_whitespace()) {
            i += 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The list marker at the start of a line: (indent in characters, bullet or number, the rest).
fn list_marker(line: &str) -> Option<(usize, String, &str)> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let after = |n: usize| rest[n..].strip_prefix(' ').or_else(|| rest[n..].strip_prefix('\t'));
    if rest.starts_with(['-', '*', '+']) {
        return after(1).map(|r| (indent, "\u{2022}".to_string(), r.trim_start()));
    }
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 && rest[digits..].starts_with(['.', ')']) {
        return after(digits + 1).map(|r| (indent, format!("{}.", &rest[..digits]), r.trim_start()));
    }
    None
}

fn is_rule(line: &str) -> bool {
    let t = line.trim();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn is_table_separator(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The blocks of a note, in order. Blank lines separate paragraphs; the rows of a table, the items
/// of a list and the lines of a code block are blocks of their own.
pub fn blocks(text: &str) -> Vec<Block> {
    let mut out: Vec<Block> = vec![];
    let mut para: Vec<&str> = vec![];
    let mut fence = false;
    let flush = |para: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !para.is_empty() {
            out.push(Block { kind: Kind::Paragraph, text: plain(&para.join(" ")) });
            para.clear();
        }
    };
    let mut lines = text.lines();
    // the properties: `---` on the first line up to the next `---`
    if text.lines().next().is_some_and(|l| l.trim_end() == "---") {
        if let Some(close) = text.lines().skip(1).position(|l| l.trim_end() == "---") {
            for l in lines.by_ref().take(close + 2) {
                if l.trim_end() != "---" && !l.trim().is_empty() {
                    out.push(Block { kind: Kind::Property, text: l.trim().to_string() });
                }
            }
        }
    }
    for line in lines {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            flush(&mut para, &mut out);
            fence = !fence;
            continue;
        }
        if fence {
            out.push(Block { kind: Kind::Code, text: line.to_string() });
        } else if line.trim().is_empty() {
            flush(&mut para, &mut out);
        } else if let Some(level) = (line.starts_with('#')).then(|| line.chars().take_while(|c| *c == '#').count()).filter(|n| (1..=6).contains(n) && line[*n..].starts_with(' ')) {
            flush(&mut para, &mut out);
            out.push(Block { kind: Kind::Heading(level as u8), text: plain(line[level..].trim()) });
        } else if is_rule(line) && list_marker(line).is_none() {
            flush(&mut para, &mut out);
            out.push(Block { kind: Kind::Rule, text: String::new() });
        } else if let Some((indent, bullet, rest)) = list_marker(line) {
            flush(&mut para, &mut out);
            let (box_mark, rest) = match rest.get(..3) {
                Some("[ ]") => ("\u{2610} ", rest[3..].trim_start()),
                Some("[x]") | Some("[X]") => ("\u{2611} ", rest[3..].trim_start()),
                _ => ("", rest),
            };
            out.push(Block { kind: Kind::Item(indent / 2, bullet), text: format!("{box_mark}{}", plain(rest)) });
        } else if line.trim_start().starts_with('|') {
            flush(&mut para, &mut out);
            if !is_table_separator(line) {
                let cells: Vec<String> = line.trim().trim_matches('|').split('|').map(|c| plain(c.trim())).collect();
                out.push(Block { kind: Kind::TableRow, text: cells.join("  \u{2502}  ") });
            }
        } else if let Some(rest) = line.trim_start().strip_prefix('>') {
            flush(&mut para, &mut out);
            out.push(Block { kind: Kind::Quote, text: plain(rest.trim()) });
        } else {
            para.push(line.trim());
        }
    }
    flush(&mut para, &mut out);
    out
}

// ---------------------------------------------------------------------------------------------
// The rendered comparison

/// What the gutter says about a block (HIST-011).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    Same,
    Added,
    Removed,
    Changed,
}

/// One block of the comparison with its spans.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Row {
    pub kind: Kind,
    pub mark: Mark,
    pub spans: Vec<Span>,
}

/// Two blocks are the same change of text when they are of one family and share enough words.
const PAIR_MIN: f32 = 0.3;

/// The comparison of two notes as rows (HIST-009): what is the same, added, removed, or changed
/// within a block (word by word).
pub fn compare(old: &str, new: &str) -> Vec<Row> {
    let (a, b) = (blocks(old), blocks(new));
    let keys_a: Vec<(&Kind, &str)> = a.iter().map(|x| (&x.kind, x.text.as_str())).collect();
    let keys_b: Vec<(&Kind, &str)> = b.iter().map(|x| (&x.kind, x.text.as_str())).collect();
    let script = edits(&keys_a, &keys_b);
    let mut rows = vec![];
    let mut k = 0;
    while k < script.len() {
        if let Edit::Same(_, j) = script[k] {
            rows.push(Row { kind: b[j].kind.clone(), mark: Mark::Same, spans: vec![Span::new(&b[j].text, Change::Same)] });
            k += 1;
            continue;
        }
        let mut gone = vec![];
        let mut came = vec![];
        while k < script.len() {
            match script[k] {
                Edit::Del(i) => gone.push(i),
                Edit::Ins(j) => came.push(j),
                Edit::Same(..) => break,
            }
            k += 1;
        }
        let mut c = 0;
        while c < gone.len().max(came.len()) {
            match (gone.get(c), came.get(c)) {
                (Some(&i), Some(&j)) if similarity(&a[i].text, &b[j].text) >= PAIR_MIN => {
                    rows.push(Row { kind: b[j].kind.clone(), mark: Mark::Changed, spans: word_spans(&a[i].text, &b[j].text) });
                }
                (g, n) => {
                    if let Some(&i) = g {
                        rows.push(Row { kind: a[i].kind.clone(), mark: Mark::Removed, spans: vec![Span::new(&a[i].text, Change::Del)] });
                    }
                    if let Some(&j) = n {
                        rows.push(Row { kind: b[j].kind.clone(), mark: Mark::Added, spans: vec![Span::new(&b[j].text, Change::Ins)] });
                    }
                }
            }
            c += 1;
        }
    }
    rows
}

/// A block of unchanged rows that is longer than this is folded (HIST-010).
const FOLD_OVER: usize = 3;

/// What the track-changes view shows: a row, or a fold of rows `from..to` that were not changed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Shown {
    Row(usize),
    Fold { from: usize, to: usize },
}

impl Shown {
    /// "… 34 unchanged paragraphs" (HIST-010); "parts" when they are not all of one kind.
    pub fn fold_text(rows: &[Row], from: usize, to: usize) -> String {
        let n = to - from;
        let kind = &rows[from].kind;
        let word = if rows[from..to].iter().all(|r| std::mem::discriminant(&r.kind) == std::mem::discriminant(kind)) {
            if n == 1 { kind.name() } else { kind.plural() }
        } else if n == 1 {
            "part"
        } else {
            "parts"
        };
        format!("\u{2026} {n} unchanged {word}")
    }
}

/// The rows to draw: runs of more than `FOLD_OVER` unchanged rows keep one row next to each change
/// and fold the rest; a run that is `open` (by the index of its first folded row) is shown whole.
pub fn fold(rows: &[Row], open: &HashSet<usize>) -> Vec<Shown> {
    let mut out = vec![];
    let mut i = 0;
    while i < rows.len() {
        if rows[i].mark != Mark::Same {
            out.push(Shown::Row(i));
            i += 1;
            continue;
        }
        let start = i;
        while i < rows.len() && rows[i].mark == Mark::Same {
            i += 1;
        }
        let (before, after) = (start > 0, i < rows.len());
        let keep_head = usize::from(before);
        let keep_tail = usize::from(after);
        let len = i - start;
        if len > FOLD_OVER && !open.contains(&(start + keep_head)) {
            out.extend((start..start + keep_head).map(Shown::Row));
            out.push(Shown::Fold { from: start + keep_head, to: i - keep_tail });
            out.extend((i - keep_tail..i).map(Shown::Row));
        } else {
            out.extend((start..i).map(Shown::Row));
        }
    }
    out
}

/// Where the changes begin: the first row of every run of changed rows (for next and previous change).
pub fn change_starts(rows: &[Row]) -> Vec<usize> {
    (0..rows.len()).filter(|&i| rows[i].mark != Mark::Same && (i == 0 || rows[i - 1].mark == Mark::Same)).collect()
}

// ---------------------------------------------------------------------------------------------
// The source, side by side

/// One row of the side-by-side source: the line number and the spans on each side (HIST-012).
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Side {
    pub left: Option<(usize, Vec<Span>)>,
    pub right: Option<(usize, Vec<Span>)>,
    pub mark: Mark,
}

/// The lines of the two texts side by side; a changed line pair is compared word by word.
pub fn side_by_side(old: &str, new: &str) -> Vec<Side> {
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let script = edits(&a, &b);
    let mut rows = vec![];
    let mut k = 0;
    while k < script.len() {
        if let Edit::Same(i, j) = script[k] {
            rows.push(Side { left: Some((i + 1, vec![Span::new(a[i], Change::Same)])), right: Some((j + 1, vec![Span::new(b[j], Change::Same)])), mark: Mark::Same });
            k += 1;
            continue;
        }
        let (mut gone, mut came) = (vec![], vec![]);
        while k < script.len() {
            match script[k] {
                Edit::Del(i) => gone.push(i),
                Edit::Ins(j) => came.push(j),
                Edit::Same(..) => break,
            }
            k += 1;
        }
        for c in 0..gone.len().max(came.len()) {
            match (gone.get(c), came.get(c)) {
                (Some(&i), Some(&j)) => {
                    let spans = word_spans(a[i], b[j]);
                    let left = spans.iter().filter(|s| s.change != Change::Ins).map(|s| Span { text: s.text.clone(), change: s.change }).collect();
                    let right = spans.iter().filter(|s| s.change != Change::Del).map(|s| Span { text: s.text.clone(), change: s.change }).collect();
                    rows.push(Side { left: Some((i + 1, left)), right: Some((j + 1, right)), mark: Mark::Changed });
                }
                (Some(&i), None) => rows.push(Side { left: Some((i + 1, vec![Span::new(a[i], Change::Del)])), right: None, mark: Mark::Removed }),
                (None, Some(&j)) => rows.push(Side { left: None, right: Some((j + 1, vec![Span::new(b[j], Change::Ins)])), mark: Mark::Added }),
                (None, None) => {}
            }
        }
    }
    rows
}

/// Where the changes begin in the side-by-side source: the first row of every run of changed rows.
pub fn side_change_starts(rows: &[Side]) -> Vec<usize> {
    (0..rows.len()).filter(|&i| rows[i].mark != Mark::Same && (i == 0 || rows[i - 1].mark == Mark::Same)).collect()
}

/// The change as unified diff text (HIST-013), with three lines of context.
pub fn patch(old_label: &str, new_label: &str, old: &str, new: &str) -> String {
    const CONTEXT: usize = 3;
    let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let script = edits(&a, &b);
    let changed: Vec<usize> = (0..script.len()).filter(|&k| !matches!(script[k], Edit::Same(..))).collect();
    let mut out = String::new();
    if changed.is_empty() {
        return out;
    }
    out.push_str(&format!("--- {old_label}\n+++ {new_label}\n"));
    // the ranges of the script that make a hunk: changes with their context, joined when they touch
    let mut ranges: Vec<(usize, usize)> = vec![];
    for &k in &changed {
        let (lo, hi) = (k.saturating_sub(CONTEXT), (k + CONTEXT + 1).min(script.len()));
        match ranges.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => ranges.push((lo, hi)),
        }
    }
    for (lo, hi) in ranges {
        let part = &script[lo..hi];
        let count_a = part.iter().filter(|e| !matches!(e, Edit::Ins(_))).count();
        let count_b = part.iter().filter(|e| !matches!(e, Edit::Del(_))).count();
        // an empty side starts at the line before it, as `diff -u` writes it
        let before_a = script[..lo].iter().filter(|e| !matches!(e, Edit::Ins(_))).count();
        let before_b = script[..lo].iter().filter(|e| !matches!(e, Edit::Del(_))).count();
        let start = |before: usize, count: usize| if count == 0 { before } else { before + 1 };
        out.push_str(&format!("@@ -{},{} +{},{} @@\n", start(before_a, count_a), count_a, start(before_b, count_b), count_b));
        for e in part {
            match e {
                Edit::Same(i, _) => out.push_str(&format!(" {}\n", a[*i])),
                Edit::Del(i) => out.push_str(&format!("-{}\n", a[*i])),
                Edit::Ins(j) => out.push_str(&format!("+{}\n", b[*j])),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests;
