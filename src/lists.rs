//! Smart editing for lists, to-dos and headings:
//! - Enter continues a list (`- `, `* `, `+ `, `1. `, `1) `, with or without `[ ]`),
//!   and Enter on an empty item ends the list (or outdents it first).
//! - Tab / Shift+Tab indent / outdent list items and raise / lower heading levels.
//!
//! Also finds web links (`[name](https://…)` and bare `https://…`).

use std::ops::Range;

/// Spaces used for one indentation level.
pub const INDENT: &str = "    ";

#[derive(Clone, Debug, PartialEq)]
pub enum Marker {
    Bullet(char),
    /// number and delimiter ('.' or ')')
    Ordered(u64, char),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListPrefix {
    /// Leading whitespace (bytes).
    pub indent: usize,
    pub marker: Marker,
    /// Some(done) when the item is a to-do (`[ ]` / `[x]`).
    pub task: Option<bool>,
    /// Length of the whole prefix in bytes, including the space after it.
    pub len: usize,
}

impl ListPrefix {
    /// The prefix for the next item at the same level.
    pub fn next(&self, line: &str) -> String {
        let indent = &line[..self.indent];
        let m = match &self.marker {
            Marker::Bullet(c) => format!("{c} "),
            Marker::Ordered(n, d) => format!("{}{d} ", n + 1),
        };
        let task = if self.task.is_some() { "[ ] " } else { "" };
        format!("{indent}{m}{task}")
    }
}

/// Parse the list prefix of a line, e.g. `  2. [x] text` → indent 2, Ordered(2,'.'), task Some(true).
pub fn list_prefix(line: &str) -> Option<ListPrefix> {
    let t = line.trim_start_matches([' ', '\t']);
    let indent = line.len() - t.len();
    let b = t.as_bytes();
    let (marker, mut used) = if !b.is_empty() && matches!(b[0], b'-' | b'*' | b'+') {
        (Marker::Bullet(b[0] as char), 1)
    } else {
        let digits = b.iter().take_while(|c| c.is_ascii_digit()).count();
        if digits == 0 || digits > 9 || !matches!(b.get(digits), Some(b'.') | Some(b')')) {
            return None;
        }
        (Marker::Ordered(t[..digits].parse().ok()?, b[digits] as char), digits + 1)
    };
    // the marker must be followed by a space (or end the line, for a fresh empty item)
    match b.get(used) {
        Some(b' ') => used += 1,
        None => {}
        _ => return None,
    }
    let mut task = None;
    let rest = &t[used..];
    let rb = rest.as_bytes();
    if rb.len() >= 3 && rb[0] == b'[' && rb[2] == b']' && matches!(rb[1], b' ' | b'x' | b'X') && (rb.len() == 3 || rb[3] == b' ') {
        task = Some(rb[1] != b' ');
        used += if rb.len() > 3 { 4 } else { 3 };
    }
    // `---` / `***` are rules, not lists
    if let Marker::Bullet(c) = marker {
        if t.chars().all(|x| x == c || x == ' ') && t.chars().filter(|x| *x == c).count() >= 3 {
            return None;
        }
    }
    Some(ListPrefix { indent, marker, task, len: indent + used })
}

fn line_bounds(text: &str, byte: usize) -> Range<usize> {
    let start = text[..byte].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let end = text[byte..].find('\n').map(|p| byte + p).unwrap_or(text.len());
    start..end
}

/// Enter pressed with the cursor at `cursor` (byte). Returns the new text and cursor,
/// or None when Enter should just insert a normal newline.
pub fn continue_list(text: &str, cursor: usize) -> Option<(String, usize)> {
    let lb = line_bounds(text, cursor);
    let line = &text[lb.clone()];
    let p = list_prefix(line)?;
    if cursor - lb.start < p.len.min(line.len()) && cursor - lb.start < p.indent + 1 {
        // cursor before the marker: plain newline
        return None;
    }
    let content = line[p.len.min(line.len())..].trim();
    if content.is_empty() {
        // Empty item: outdent one level, or end the list.
        let mut out = String::with_capacity(text.len());
        out.push_str(&text[..lb.start]);
        let new_line = if p.indent > 0 {
            let (t, _) = outdent_line(line);
            renumber_marker(&t, text, lb.start)
        } else {
            String::new()
        };
        out.push_str(&new_line);
        let cur = lb.start + new_line.len();
        out.push_str(&text[lb.end..]);
        return Some((out, cur));
    }
    let insert = format!("\n{}", p.next(line));
    let mut out = String::with_capacity(text.len() + insert.len());
    out.push_str(&text[..cursor]);
    out.push_str(&insert);
    let cur = cursor + insert.len();
    // the rest of the line moves to the new item (without its leading spaces)
    out.push_str(text[cursor..].trim_start_matches(' '));
    let out = renumber_following(&out, cur);
    Some((out, cur))
}

/// After inserting an ordered item, renumber the following items of the same list level.
fn renumber_following(text: &str, cursor: usize) -> String {
    let lb = line_bounds(text, cursor);
    let Some(p) = list_prefix(&text[lb.clone()]) else { return text.to_string() };
    let Marker::Ordered(mut n, _) = p.marker else { return text.to_string() };
    let mut out = text[..lb.end].to_string();
    let rest = &text[lb.end..];
    let mut lines = rest.split('\n');
    // first element is "" (rest starts with '\n') or the tail of the text
    if let Some(first) = lines.next() {
        out.push_str(first);
    }
    let mut renumbering = true;
    for l in lines {
        out.push('\n');
        if renumbering {
            match list_prefix(l) {
                Some(q) if q.indent == p.indent => {
                    if let Marker::Ordered(_, d) = q.marker {
                        n += 1;
                        let head = &l[..q.indent];
                        let digits_end = q.indent + l[q.indent..].find(d).unwrap_or(0);
                        out.push_str(&format!("{head}{n}{}", &l[digits_end..]));
                        continue;
                    }
                    renumbering = false;
                }
                Some(q) if q.indent > p.indent => {}
                _ => renumbering = false,
            }
        }
        out.push_str(l);
    }
    out
}

/// Number an ordered item to follow the previous item at the same indentation (or 1).
fn renumber_marker(line: &str, text: &str, line_start: usize) -> String {
    let Some(p) = list_prefix(line) else { return line.to_string() };
    let Marker::Ordered(_, d) = p.marker else { return line.to_string() };
    let mut n = 1;
    for prev in text[..line_start].lines().rev() {
        match list_prefix(prev) {
            Some(q) if q.indent == p.indent => {
                if let Marker::Ordered(m, _) = q.marker {
                    n = m + 1;
                }
                break;
            }
            Some(q) if q.indent > p.indent => continue,
            _ => break,
        }
    }
    let digits_end = p.indent + line[p.indent..].find(d).unwrap_or(0);
    format!("{}{n}{}", &line[..p.indent], &line[digits_end..])
}

fn outdent_line(line: &str) -> (String, usize) {
    if let Some(rest) = line.strip_prefix('\t') {
        return (rest.to_string(), 1);
    }
    let spaces = line.bytes().take_while(|c| *c == b' ').count().min(INDENT.len());
    (line[spaces..].to_string(), spaces)
}

/// Tab / Shift+Tab on the lines touched by the selection `sel` (bytes).
/// Works on list items and headings; returns None when no touched line is either
/// (so Tab can behave normally). Returns the new text and the new selection.
pub fn indent(text: &str, sel: Range<usize>, outdent: bool) -> Option<(String, Range<usize>)> {
    let first = line_bounds(text, sel.start).start;
    let last_end = line_bounds(text, sel.end).end;
    let block = &text[first..last_end];
    let lines: Vec<&str> = block.split('\n').collect();
    let applies = |l: &str| list_prefix(l).is_some() || crate::markdown::heading_level(l).is_some();
    if !lines.iter().any(|l| applies(l)) {
        return None;
    }
    let mut out = text[..first].to_string();
    let (mut new_start, mut new_end) = (sel.start as isize, sel.end as isize);
    let mut pos = first;
    for (i, l) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let line_start_out = out.len();
        let new_line: String;
        if let Some(level) = crate::markdown::heading_level(l) {
            new_line = if outdent {
                if level > 1 {
                    l[1..].to_string()
                } else {
                    l.to_string()
                }
            } else if level < 6 {
                format!("#{l}")
            } else {
                l.to_string()
            };
        } else if list_prefix(l).is_some() {
            new_line = if outdent {
                let (t, removed) = outdent_line(l);
                if removed == 0 {
                    t
                } else {
                    renumber_marker(&t, &out, line_start_out)
                }
            } else {
                renumber_marker(&format!("{INDENT}{l}"), &out, line_start_out)
            };
        } else {
            new_line = l.to_string();
        }
        let delta = new_line.len() as isize - l.len() as isize;
        // shift the selection ends that sit at or after this line
        if sel.start >= pos {
            new_start += delta;
        }
        if sel.end >= pos {
            new_end += delta;
        }
        out.push_str(&new_line);
        pos += l.len() + 1;
    }
    out.push_str(&text[last_end..]);
    let clamp = |v: isize, lo: usize| (v.max(lo as isize) as usize).min(out.len());
    let ns = clamp(new_start, first);
    let ne = clamp(new_end, first).max(ns);
    // keep cursor on char boundaries
    let fix = |mut b: usize| {
        while b > 0 && !out.is_char_boundary(b) {
            b -= 1;
        }
        b
    };
    let (ns, ne) = (fix(ns), fix(ne));
    Some((out, ns..ne))
}

/// A link to the web: `[label](https://…)` or a bare `https://…`.
#[derive(Clone, Debug, PartialEq)]
pub struct WebLink {
    /// Whole link in the text (bytes).
    pub range: Range<usize>,
    /// The visible label (bytes); for bare URLs the URL itself.
    pub label_range: Range<usize>,
    pub label: String,
    pub url: String,
}

pub fn is_web_url(u: &str) -> bool {
    let l = u.to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://") || l.starts_with("mailto:") || l.starts_with("www.")
}

/// Find web links in `text` (skips fenced and inline code).
pub fn find_web_links(text: &str) -> Vec<WebLink> {
    let fenced = crate::markdown::fenced_code_lines(text);
    let mut out = Vec::new();
    for (li, (off, line)) in crate::markdown::lines_with_offsets(text).into_iter().enumerate() {
        if fenced.get(li).copied().unwrap_or(false) {
            continue;
        }
        let mut code: Vec<Range<usize>> = Vec::new();
        let mut i = 0;
        while let Some(p) = line[i..].find('`') {
            let s = i + p;
            match line[s + 1..].find('`') {
                Some(e) => {
                    code.push(s..s + e + 2);
                    i = s + e + 2;
                }
                None => break,
            }
        }
        let in_code = |x: usize| code.iter().any(|r| r.contains(&x));
        let mut taken: Vec<Range<usize>> = Vec::new();
        // [label](url)
        let mut i = 0;
        while let Some(p) = line[i..].find('[') {
            let s = i + p;
            i = s + 1;
            if in_code(s) || line[..s].ends_with('[') || line[s + 1..].starts_with('[') {
                continue;
            }
            let Some(close) = line[s + 1..].find("](") else { continue };
            let label_end = s + 1 + close;
            let label = &line[s + 1..label_end];
            if label.contains('[') {
                continue;
            }
            let url_start = label_end + 2;
            let Some(pe) = line[url_start..].find(')') else { continue };
            let url = line[url_start..url_start + pe].trim();
            let url = url.trim_start_matches('<').trim_end_matches('>');
            if !is_web_url(url) {
                continue;
            }
            let start = if s > 0 && line.as_bytes()[s - 1] == b'!' { s - 1 } else { s };
            let end = url_start + pe + 1;
            out.push(WebLink { range: off + start..off + end, label_range: off + s + 1..off + label_end, label: if label.is_empty() { url.to_string() } else { label.to_string() }, url: url.to_string() });
            taken.push(start..end);
            i = end;
        }
        // bare URLs
        for scheme in ["https://", "http://"] {
            let mut i = 0;
            while let Some(p) = line[i..].find(scheme) {
                let s = i + p;
                let len = line[s..].find(|c: char| c.is_whitespace() || c == '>' || c == '"').unwrap_or(line.len() - s);
                let mut e = s + len;
                while e > s && matches!(line.as_bytes()[e - 1], b'.' | b',' | b';' | b':' | b')' | b'!' | b'?') {
                    e -= 1;
                }
                i = s + len.max(1);
                if in_code(s) || taken.iter().any(|r| r.contains(&s)) || line[..s].ends_with("](") {
                    continue;
                }
                let url = &line[s..e];
                out.push(WebLink { range: off + s..off + e, label_range: off + s..off + e, label: url.to_string(), url: url.to_string() });
                taken.push(s..e);
            }
        }
    }
    out.sort_by_key(|w| w.range.start);
    out
}

/// Open a web address in the default browser.
pub fn open_in_browser(url: &str) {
    let url = if url.to_ascii_lowercase().starts_with("www.") { format!("https://{url}") } else { url.to_string() };
    #[cfg(target_os = "linux")]
    let r = std::process::Command::new("xdg-open").arg(&url).spawn();
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("cmd").args(["/C", "start", "", &url]).spawn();
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    let r: std::io::Result<std::process::Child> = Err(std::io::Error::other("unsupported"));
    if let Err(e) = r {
        eprintln!("could not open {url}: {e}");
    }
}

/// Alt+Up / Alt+Down: move the lines touched by `sel` one line up or down.
/// Afterwards list items in the surrounding list are kept valid: an item is never indented
/// more than one level deeper than the item above it (or at all when nothing is above),
/// and ordered items are renumbered. Returns the new text and selection.
pub fn move_lines(text: &str, sel: Range<usize>, up: bool) -> Option<(String, Range<usize>)> {
    let mut lines: Vec<String> = text.split('\n').map(|s| s.to_string()).collect();
    let line_of = |b: usize| text[..b].matches('\n').count();
    let first = line_of(sel.start);
    let mut last = line_of(sel.end);
    // a selection ending at the very start of a line doesn't include that line
    if last > first && text[..sel.end].ends_with('\n') {
        last -= 1;
    }
    if (up && first == 0) || (!up && last + 1 >= lines.len()) {
        return None;
    }
    let line_start = |lines: &[String], i: usize| lines[..i].iter().map(|l| l.len() + 1).sum::<usize>();
    let col_start = sel.start - line_start(&lines, first);
    let col_end = sel.end - line_start(&lines, line_of(sel.end));
    let end_line_rel = line_of(sel.end) - first;
    let (new_first, new_last) = if up {
        let l = lines.remove(first - 1);
        lines.insert(last, l);
        (first - 1, last - 1)
    } else {
        let l = lines.remove(last + 1);
        lines.insert(first, l);
        (first + 1, last + 1)
    };
    let before: Vec<usize> = (new_first..=new_last).map(|i| lines[i].len()).collect();
    normalize_list_around(&mut lines, new_first, new_last);
    // keep the cursor on the same spot of the moved text
    let shift = |i: usize, col: usize, lines: &[String]| -> usize {
        let delta = lines[i].len() as isize - before[i - new_first] as isize;
        let c = (col as isize + delta).clamp(0, lines[i].len() as isize) as usize;
        let mut c = c;
        while c > 0 && !lines[i].is_char_boundary(c) {
            c -= 1;
        }
        c
    };
    let s_line = new_first;
    let e_line = (new_first + end_line_rel).min(new_last.max(new_first + end_line_rel).min(lines.len() - 1));
    let ns = line_start(&lines, s_line) + shift(s_line, col_start, &lines);
    let ne = if e_line <= new_last { line_start(&lines, e_line) + shift(e_line, col_end, &lines) } else { ns };
    Some((lines.join("\n"), ns..ne.max(ns)))
}

/// Fix indentation and numbering of the list that contains lines `a..=b`.
fn normalize_list_around(lines: &mut [String], a: usize, b: usize) {
    let is_item = |l: &str| list_prefix(l).is_some();
    if !((a..=b).any(|i| is_item(&lines[i])) || (a > 0 && is_item(&lines[a - 1])) || (b + 1 < lines.len() && is_item(&lines[b + 1]))) {
        return;
    }
    let mut start = a;
    while start > 0 && is_item(&lines[start - 1]) {
        start -= 1;
    }
    let mut end = b;
    while end + 1 < lines.len() && is_item(&lines[end + 1]) {
        end += 1;
    }
    // indentation: at most one level deeper than the item above
    let mut prev_indent: Option<usize> = None;
    #[allow(clippy::needless_range_loop)] // lines[i] is replaced inside the loop
    for i in start..=end {
        let Some(p) = list_prefix(&lines[i]) else {
            prev_indent = None;
            continue;
        };
        let allowed = prev_indent.map(|x| x + INDENT.len()).unwrap_or(0);
        let mut indent = p.indent;
        if indent > allowed {
            let rest = lines[i][p.indent..].to_string();
            lines[i] = format!("{}{rest}", " ".repeat(allowed));
            indent = allowed;
        }
        prev_indent = Some(indent);
    }
    // numbering: each run of ordered items at one level counts up from the run's lowest number
    let mut run_of: Vec<Option<usize>> = vec![None; end + 1];
    let mut run_min: Vec<u64> = Vec::new();
    let mut open: Vec<(usize, usize)> = Vec::new(); // (indent, run id)
    for i in start..=end {
        let Some(p) = list_prefix(&lines[i]) else { continue };
        open.retain(|(ind, _)| *ind <= p.indent);
        match p.marker {
            Marker::Ordered(n, _) => {
                let id = match open.last() {
                    Some((ind, id)) if *ind == p.indent => *id,
                    _ => {
                        run_min.push(u64::MAX);
                        open.push((p.indent, run_min.len() - 1));
                        run_min.len() - 1
                    }
                };
                run_min[id] = run_min[id].min(n);
                run_of[i] = Some(id);
            }
            Marker::Bullet(_) => {
                if open.last().map(|(ind, _)| *ind == p.indent).unwrap_or(false) {
                    open.pop();
                }
            }
        }
    }
    let mut next: Vec<u64> = run_min.clone();
    for i in start..=end {
        let Some(id) = run_of[i] else { continue };
        let Some(p) = list_prefix(&lines[i]) else { continue };
        if let Marker::Ordered(_, d) = p.marker {
            let n = next[id];
            next[id] += 1;
            let digits_end = p.indent + lines[i][p.indent..].find(d).unwrap_or(0);
            lines[i] = format!("{}{n}{}", &lines[i][..p.indent], &lines[i][digits_end..]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enter(t: &str) -> (String, usize) {
        continue_list(t, t.len()).expect("should continue")
    }

    // spec: EDIT-005
    #[test]
    fn prefixes() {
        assert_eq!(list_prefix("- a").unwrap().marker, Marker::Bullet('-'));
        assert_eq!(list_prefix("  3. b").unwrap().marker, Marker::Ordered(3, '.'));
        let p = list_prefix("1. [x] done").unwrap();
        assert_eq!(p.task, Some(true));
        assert_eq!(p.len, 7);
        assert_eq!(list_prefix("2. [ ] open").unwrap().task, Some(false));
        assert!(list_prefix("-no space").is_none());
        assert!(list_prefix("---").is_none());
        assert!(list_prefix("text").is_none());
        assert!(list_prefix("2026. year").is_some());
    }

    // spec: EDIT-005
    #[test]
    fn enter_continues_lists() {
        assert_eq!(enter("- apples"), ("- apples\n- ".to_string(), 11));
        assert_eq!(enter("* x").0, "* x\n* ");
        assert_eq!(enter("1. first").0, "1. first\n2. ");
        assert_eq!(enter("9) nine").0, "9) nine\n10) ");
        assert_eq!(enter("    - nested").0, "    - nested\n    - ");
        assert_eq!(enter("- [x] done").0, "- [x] done\n- [ ] ");
        assert_eq!(enter("1. [ ] todo").0, "1. [ ] todo\n2. [ ] ");
        assert_eq!(enter("2. [x] todo").0, "2. [x] todo\n3. [ ] ");
        assert!(continue_list("plain", 5).is_none());
    }

    // spec: EDIT-005
    #[test]
    fn enter_on_empty_item_ends_or_outdents() {
        let (t, c) = enter("- a\n- ");
        assert_eq!(t, "- a\n");
        assert_eq!(c, 4);
        let (t, _) = enter("- a\n    - ");
        assert_eq!(t, "- a\n- ");
        let (t, _) = enter("1. a\n    1. [ ] ");
        assert_eq!(t, "1. a\n2. [ ] ");
    }

    // spec: EDIT-005
    #[test]
    fn enter_in_the_middle_renumbers() {
        let t = "1. a\n2. b\n3. c";
        let (out, cur) = continue_list(t, 4).unwrap();
        assert_eq!(out, "1. a\n2. \n3. b\n4. c");
        assert_eq!(cur, 8);
    }

    // spec: EDIT-005
    #[test]
    fn enter_splits_line_at_cursor() {
        let t = "- hello world";
        let (out, _) = continue_list(t, 7).unwrap();
        assert_eq!(out, "- hello\n- world");
    }

    // spec: EDIT-005
    #[test]
    fn tab_indents_lists_and_headings() {
        let (t, s) = indent("- a\n- b", 6..6, false).unwrap();
        assert_eq!(t, "- a\n    - b");
        assert_eq!(s, 10..10);
        let (t, _) = indent("- a\n    - b", 9..9, true).unwrap();
        assert_eq!(t, "- a\n- b");
        let (t, _) = indent("1. a\n2. b", 8..8, false).unwrap();
        assert_eq!(t, "1. a\n    1. b");
        let (t, _) = indent("1. a\n    1. b", 12..12, true).unwrap();
        assert_eq!(t, "1. a\n2. b");
        let (t, s) = indent("# Title", 3..3, false).unwrap();
        assert_eq!(t, "## Title");
        assert_eq!(s, 4..4);
        assert_eq!(indent("### T", 0..0, false).unwrap().0, "#### T");
        assert_eq!(indent("## T", 4..4, true).unwrap().0, "# T");
        assert_eq!(indent("# T", 2..2, true).unwrap().0, "# T");
        assert!(indent("plain text", 3..3, false).is_none());
        // multi-line selection
        let (t, _) = indent("- a\n- b\n- c", 0..11, false).unwrap();
        assert_eq!(t, "    - a\n    - b\n    - c");
    }

    // spec: EDIT-005
    #[test]
    fn moving_lines() {
        // plain lines swap, cursor follows
        let (t, s) = move_lines("a\nb\nc", 2..2, true).unwrap();
        assert_eq!(t, "b\na\nc");
        assert_eq!(s, 0..0);
        let (t, s) = move_lines("a\nb\nc", 3..3, false).unwrap();
        assert_eq!(t, "a\nc\nb");
        assert_eq!(s, 5..5);
        assert!(move_lines("a\nb", 0..0, true).is_none());
        assert!(move_lines("a\nb", 2..2, false).is_none());
        // a nested item moved above its parent loses the extra indent
        let (t, _) = move_lines("- a\n    - b", 6..6, true).unwrap();
        assert_eq!(t, "- b\n- a");
        // a nested item moved under another item keeps its level
        let (t, _) = move_lines("- a\n- b\n    - c", 10..10, true).unwrap();
        assert_eq!(t, "- a\n    - c\n- b");
        // ordered lists are renumbered
        let (t, _) = move_lines("1. one\n2. two\n3. three", 16..16, true).unwrap();
        assert_eq!(t, "1. one\n2. three\n3. two");
        let (t, _) = move_lines("1. [ ] one\n2. [x] two", 0..0, false).unwrap();
        assert_eq!(t, "1. [x] two\n2. [ ] one");
        // a block of selected lines moves together
        let (t, _) = move_lines("x\na\nb\ny", 2..5, true).unwrap();
        assert_eq!(t, "a\nb\nx\ny");
    }

    // spec: EDIT-009
    #[test]
    fn web_links() {
        let t = "See [this is google](https://google.com) and https://example.org/x. Not [[Note]] or [n](Note.md) `https://code`";
        let w = find_web_links(t);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].label, "this is google");
        assert_eq!(w[0].url, "https://google.com");
        assert_eq!(&t[w[0].range.clone()], "[this is google](https://google.com)");
        assert_eq!(&t[w[0].label_range.clone()], "this is google");
        assert_eq!(w[1].url, "https://example.org/x");
    }
}
