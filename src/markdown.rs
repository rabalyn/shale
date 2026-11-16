//! Markdown helpers: wiki-link / task / tag / heading parsing.
//! Everything here is pure (no UI) so it is easy to unit test.

use std::ops::Range;

/// A link found in a note: `[[Target]]`, `[[Target|Alias]]`, `[[Target#Heading]]`,
/// `![[Embed]]` or a markdown link `[text](Target.md)` that points inside the vault.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkSpan {
    /// Byte range of the whole link including brackets (and the `!` of an embed).
    pub range: Range<usize>,
    /// Byte range of the target name inside the text.
    pub target_range: Range<usize>,
    /// Target note name (without `#heading` and `|alias`), trimmed.
    pub target: String,
    pub heading: Option<String>,
    pub alias: Option<String>,
    pub embed: bool,
    /// true for `[text](target)` style links.
    pub markdown_style: bool,
}

impl LinkSpan {
    /// The text shown to the reader (alias, or the target).
    pub fn display(&self) -> String {
        if let Some(a) = &self.alias {
            return a.clone();
        }
        match &self.heading {
            Some(h) if self.target.is_empty() => h.clone(),
            Some(h) => format!("{} > {}", self.target, h),
            None => self.target.clone(),
        }
    }
}

impl LinkSpan {
    /// The width an embed asks for (`![[pic.png|300]]`, `|300x200` too: the width counts), if its
    /// last `|` part is a number (EDIT-024).
    pub fn embed_width(&self) -> Option<f32> {
        if !self.embed || self.markdown_style {
            return None;
        }
        let a = self.alias.as_deref()?.rsplit('|').next()?.trim();
        let w = a.split(['x', 'X']).next()?;
        w.parse::<u32>().ok().filter(|w| *w > 0).map(|w| w as f32)
    }
}

/// The text of a `![[…]]` embed with its width set to `width` points (`|300`): an existing
/// numeric last `|` part is replaced, otherwise one is added (EDIT-024).
pub fn embed_with_width(link: &str, width: u32) -> String {
    let Some(inner) = link.strip_suffix("]]") else { return link.to_string() };
    let (head, last) = inner.rsplit_once('|').map(|(h, l)| (Some(h), l)).unwrap_or((None, inner));
    let numeric = !last.is_empty() && last.split(['x', 'X']).all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    match head {
        Some(h) if numeric => format!("{h}|{width}]]"),
        _ => format!("{inner}|{width}]]"),
    }
}

/// A `- [ ] task` line.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    /// 0-based line number.
    pub line: usize,
    /// Byte offset of the character inside the brackets (` ` or `x`).
    pub check_offset: usize,
    pub done: bool,
    pub text: String,
    pub indent: usize,
    /// `📅 2026-01-31` or `due:2026-01-31`, if present.
    pub due: Option<String>,
}

/// Byte ranges of lines that are inside fenced code blocks (``` or ~~~), fences included.
pub fn fenced_code_lines(text: &str) -> Vec<bool> {
    let mut out = Vec::new();
    let mut in_code = false;
    for line in text.split('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            out.push(true);
            in_code = !in_code;
        } else {
            out.push(in_code);
        }
    }
    out
}

/// Iterate lines with their starting byte offsets (lines split on '\n', '\r' kept off).
pub fn lines_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        out.push((start, line.strip_suffix('\r').unwrap_or(line)));
        start += line.len() + 1;
    }
    out
}

/// Byte ranges of inline code spans (`code`) in a single line, relative to the line.
pub(crate) fn inline_code_ranges(line: &str) -> Vec<Range<usize>> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'`' {
            if let Some(end) = line[i + 1..].find('`') {
                out.push(i..i + 1 + end + 1);
                i = i + 1 + end + 1;
                continue;
            } else {
                break;
            }
        }
        i += 1;
    }
    out
}

fn in_ranges(pos: usize, ranges: &[Range<usize>]) -> bool {
    ranges.iter().any(|r| r.contains(&pos))
}

fn is_external(target: &str) -> bool {
    let t = target.to_ascii_lowercase();
    t.contains("://") || t.starts_with("mailto:") || t.starts_with('#') || t.starts_with("www.")
}

/// Undo `%20` style escapes in markdown link targets.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Some(v) = s.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Strip a trailing `.md` from a link target (case-insensitive).
pub fn strip_md_ext(name: &str) -> &str {
    if name.len() > 3 && name.is_char_boundary(name.len() - 3) && name[name.len() - 3..].eq_ignore_ascii_case(".md") {
        &name[..name.len() - 3]
    } else {
        name
    }
}

/// Find every internal link in `text`, skipping fenced and inline code.
pub fn find_links(text: &str) -> Vec<LinkSpan> {
    let code = fenced_code_lines(text);
    let mut out = Vec::new();
    for (li, (off, line)) in lines_with_offsets(text).into_iter().enumerate() {
        if code.get(li).copied().unwrap_or(false) {
            continue;
        }
        let codes = inline_code_ranges(line);
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if in_ranges(i, &codes) {
                i += 1;
                continue;
            }
            // [[wiki]]
            if b[i] == b'[' && i + 1 < b.len() && b[i + 1] == b'[' {
                if let Some(close) = line[i + 2..].find("]]") {
                    let inner_start = i + 2;
                    let inner = &line[inner_start..inner_start + close];
                    if !inner.contains('[') && !inner.trim().is_empty() {
                        let embed = i > 0 && b[i - 1] == b'!';
                        let start = if embed { i - 1 } else { i };
                        let end = inner_start + close + 2;
                        let (before_alias, alias) = match inner.find('|') {
                            Some(p) => (&inner[..p], Some(inner[p + 1..].trim().to_string())),
                            None => (inner, None),
                        };
                        // "Receipt #2583.eml" is a file name, not "Receipt" with a heading
                        let (target, heading) = match before_alias.find('#').filter(|_| !is_file_with_hash(before_alias)) {
                            Some(p) => (&before_alias[..p], Some(before_alias[p + 1..].trim().to_string())),
                            None => (before_alias, None),
                        };
                        let lead = target.len() - target.trim_start().len();
                        let t_trim = target.trim();
                        let target_range = off + inner_start + lead..off + inner_start + lead + t_trim.len();
                        out.push(LinkSpan {
                            range: off + start..off + end,
                            target_range,
                            target: strip_md_ext(t_trim).to_string(),
                            heading: heading.filter(|h| !h.is_empty()),
                            alias: alias.filter(|a| !a.is_empty()),
                            embed,
                            markdown_style: false,
                        });
                        i = end;
                        continue;
                    }
                }
            }
            // [text](target)
            if b[i] == b'[' && (i == 0 || b[i - 1] != b'[') {
                if let Some(close_rel) = line[i + 1..].find("](") {
                    let text_end = i + 1 + close_rel;
                    let label = &line[i + 1..text_end];
                    if !label.contains('[') && !label.contains(']') {
                        if let Some(paren_rel) = line[text_end + 2..].find(')') {
                            let t_start = text_end + 2;
                            let t_end = t_start + paren_rel;
                            let raw = line[t_start..t_end].trim();
                            let raw = raw.trim_start_matches('<').trim_end_matches('>');
                            if !raw.is_empty() && !is_external(raw) {
                                let embed = i > 0 && b[i - 1] == b'!';
                                let start = if embed { i - 1 } else { i };
                                let decoded = percent_decode(raw);
                                let (target, heading) = match decoded.find('#') {
                                    Some(p) => (decoded[..p].to_string(), Some(decoded[p + 1..].to_string())),
                                    None => (decoded.clone(), None),
                                };
                                // Use the file name part: folder/Note.md -> keep the path for resolution.
                                out.push(LinkSpan {
                                    range: off + start..off + t_end + 1,
                                    target_range: off + t_start..off + t_end,
                                    target: strip_md_ext(&target).to_string(),
                                    heading,
                                    alias: Some(label.to_string()).filter(|s| !s.is_empty()),
                                    embed,
                                    markdown_style: true,
                                });
                                i = t_end + 1;
                                continue;
                            }
                        }
                    }
                }
            }
            i += 1;
        }
    }
    out
}

/// Find `- [ ]` / `* [x]` / `1. [ ]` tasks, skipping fenced code.
pub fn find_tasks(text: &str) -> Vec<Task> {
    let code = fenced_code_lines(text);
    let mut out = Vec::new();
    for (li, (off, line)) in lines_with_offsets(text).into_iter().enumerate() {
        if code.get(li).copied().unwrap_or(false) {
            continue;
        }
        if let Some((check_rel, done, rest, indent)) = parse_task_line(line) {
            let text = rest.trim().to_string();
            let due = find_due(&text);
            out.push(Task { line: li, check_offset: off + check_rel, done, text, indent, due });
        }
    }
    out
}

/// If `line` is a task, returns (offset of check char, done, text after `] `, indent).
pub fn parse_task_line(line: &str) -> Option<(usize, bool, &str, usize)> {
    let indent = line.len() - line.trim_start().len();
    let t = &line[indent..];
    let after_marker = if t.starts_with("- ") || t.starts_with("* ") || t.starts_with("+ ") {
        2
    } else {
        let digits = t.bytes().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
            digits + 2
        } else {
            return None;
        }
    };
    let rest = &t[after_marker..];
    let rb = rest.as_bytes();
    if rb.len() >= 3 && rb[0] == b'[' && rb[2] == b']' && (rb.len() == 3 || rb[3] == b' ') {
        let c = rb[1];
        let done = match c {
            b' ' => false,
            b'x' | b'X' => true,
            _ => return None,
        };
        let text = if rest.len() > 4 { &rest[4..] } else { "" };
        return Some((indent + after_marker + 1, done, text, indent));
    }
    None
}

fn find_due(text: &str) -> Option<String> {
    for key in ["📅", "due:", "@due("] {
        if let Some(p) = text.find(key) {
            let rest = text[p + key.len()..].trim_start();
            // (get: the 10th byte may be inside a character, as in "due:Müllabfuhr")
            if let Some(d) = rest.get(..10) {
                let ok = d.bytes().enumerate().all(|(i, c)| if i == 4 || i == 7 { c == b'-' } else { c.is_ascii_digit() });
                if ok {
                    return Some(d.to_string());
                }
            }
        }
    }
    None
}

/// Toggle a task checkbox at `check_offset` in `text`. Returns true if changed.
pub fn toggle_task_at(text: &mut String, check_offset: usize) -> bool {
    match text.as_bytes().get(check_offset) {
        Some(b' ') => {
            text.replace_range(check_offset..check_offset + 1, "x");
            true
        }
        Some(b'x') | Some(b'X') => {
            text.replace_range(check_offset..check_offset + 1, " ");
            true
        }
        _ => false,
    }
}

/// `#tags` in text (not inside code, not headings). Returns (byte range, tag without '#').
/// Tags in the note's properties at the top: `tags: [a, b]`, `tags: a, b` or a `- a` list.
pub fn frontmatter_tags(text: &str) -> Vec<String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return vec![];
    }
    let mut out = Vec::new();
    let mut in_list = false;
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        let clean = |s: &str| s.trim().trim_matches(|c| c == '"' || c == '\'').trim_start_matches('#').to_string();
        if in_list {
            if let Some(item) = l.trim_start().strip_prefix("- ") {
                out.push(clean(item));
                continue;
            }
            in_list = false;
        }
        if let Some(rest) = l.strip_prefix("tags:").or_else(|| l.strip_prefix("tag:")) {
            let rest = rest.trim().trim_start_matches('[').trim_end_matches(']');
            if rest.is_empty() {
                in_list = true;
            }
            out.extend(rest.split(|c: char| c == ',' || c.is_whitespace()).map(clean));
        }
    }
    out.retain(|t| !t.is_empty());
    out
}

pub fn find_tags(text: &str) -> Vec<(Range<usize>, String)> {
    let code = fenced_code_lines(text);
    let mut out = Vec::new();
    for (li, (off, line)) in lines_with_offsets(text).into_iter().enumerate() {
        if code.get(li).copied().unwrap_or(false) {
            continue;
        }
        let mut codes = inline_code_ranges(line);
        // the colour in `<mark style="background: #FFFF00;">` is no tag (EDIT-027)
        codes.extend(crate::colour_tags::open_tags(line));
        // a "#" inside a [[link]] is part of the link (heading or file name), not a tag
        let mut from = 0;
        while let Some(st) = line[from..].find("[[").map(|p| p + from) {
            match line[st..].find("]]") {
                Some(e) => {
                    codes.push(st..st + e + 2);
                    from = st + e + 2;
                }
                None => break,
            }
        }
        let b = line.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'#' && (i == 0 || b[i - 1] == b' ' || b[i - 1] == b'\t' || b[i - 1] == b'(') && !in_ranges(i, &codes) {
                let rest = &line[i + 1..];
                let len: usize = rest.char_indices().take_while(|(_, c)| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == '/').map(|(_, c)| c.len_utf8()).sum();
                let tag = &rest[..len];
                if len > 0 && !tag.chars().all(|c| c.is_ascii_digit()) {
                    out.push((off + i..off + i + 1 + len, tag.to_string()));
                    i += 1 + len;
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

/// Headings: (line number, level, title).
pub fn find_headings(text: &str) -> Vec<(usize, usize, String)> {
    let code = fenced_code_lines(text);
    let mut out = Vec::new();
    for (li, (_, line)) in lines_with_offsets(text).into_iter().enumerate() {
        if code.get(li).copied().unwrap_or(false) {
            continue;
        }
        if let Some(level) = heading_level(line) {
            out.push((li, level, line[level..].trim().to_string()));
        }
    }
    out
}

/// `# Title` → Some(1). Requires a space after the hashes.
pub fn heading_level(line: &str) -> Option<usize> {
    let n = line.bytes().take_while(|c| *c == b'#').count();
    if (1..=6).contains(&n) && line.as_bytes().get(n) == Some(&b' ') {
        Some(n)
    } else {
        None
    }
}

/// The link's target as written in the text: `Projects/Old.md`, `../Old` (percent-decoded,
/// without `<…>`, `#heading` or `|alias`). Unlike [`LinkSpan::target`] it keeps a `.md`.
pub fn written_target(text: &str, link: &LinkSpan) -> String {
    let raw = text.get(link.target_range.clone()).unwrap_or("");
    if !link.markdown_style {
        return raw.trim().to_string();
    }
    let (_, core, _) = split_md_target(raw);
    let core = core.trim_start_matches('<').trim_end_matches('>');
    let decoded = percent_decode(core);
    match decoded.find('#') {
        Some(p) => decoded[..p].to_string(),
        None => decoded,
    }
}

/// Leading space, the target itself and trailing space of a markdown link's `(…)` part.
fn split_md_target(raw: &str) -> (&str, &str, &str) {
    let core = raw.trim();
    let lead = raw.len() - raw.trim_start().len();
    (&raw[..lead], core, &raw[lead + core.len()..])
}

/// The text that replaces `link.target_range` so the link points to `new_target` (a path or name
/// as it should be written, e.g. `Projects/New` or `New.md`). Everything else is kept: alias and
/// heading (outside the range for `[[…]]`), and for markdown links the `#heading`, `<…>` and the
/// spaces around the target. Markdown links without `<…>` get spaces and parentheses encoded.
pub fn replace_target(text: &str, link: &LinkSpan, new_target: &str) -> String {
    if !link.markdown_style {
        return new_target.to_string();
    }
    let raw = text.get(link.target_range.clone()).unwrap_or("");
    let (lead, core, trail) = split_md_target(raw);
    let angle = core.starts_with('<') && core.ends_with('>') && core.len() >= 2;
    let inner = if angle { &core[1..core.len() - 1] } else { core };
    let fragment = inner.find('#').map(|p| &inner[p..]).unwrap_or("");
    // `%`, `#` and parentheses are always encoded (the parser stops at the first `)` and splits
    // at `#`); spaces only outside `<…>`
    let mut target = String::with_capacity(new_target.len());
    for c in new_target.chars() {
        match c {
            '%' => target.push_str("%25"),
            ' ' if !angle => target.push_str("%20"),
            '(' => target.push_str("%28"),
            ')' => target.push_str("%29"),
            '#' => target.push_str("%23"),
            c => target.push(c),
        }
    }
    if angle {
        format!("{lead}<{target}{fragment}>{trail}")
    } else {
        format!("{lead}{target}{fragment}{trail}")
    }
}

/// Byte index of the start of 0-based line `line`.
pub fn line_start_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    let mut n = 0;
    for (i, c) in text.char_indices() {
        if c == '\n' {
            n += 1;
            if n == line {
                return i + 1;
            }
        }
    }
    text.len()
}

/// Convert a byte offset to a char index.
pub fn byte_to_char(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// Convert a char index to a byte offset.
pub fn char_to_byte(text: &str, ch: usize) -> usize {
    text.char_indices().nth(ch).map(|(b, _)| b).unwrap_or(text.len())
}

/// Is `name` a valid file name for a note? (no slashes or reserved characters)
/// A link target like `z_media/…/Receipt #2583.eml`: the part after the `#` ends in a file
/// extension, so the `#` belongs to the file name (headings don't end in `.pdf`).
fn is_file_with_hash(t: &str) -> bool {
    let Some(p) = t.rfind('#') else { return false };
    let after = t[p + 1..].trim();
    match after.rsplit_once('.') {
        Some((stem, ext)) => !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.chars().any(|c| c.is_ascii_alphabetic()),
        None => false,
    }
}

/// A file or note name that works inside `[[…]]` links: `#` (heading), `|` (alias), `^`
/// (block) and brackets would break the link, so they are left out or replaced.
pub fn link_safe_name(name: &str) -> String {
    let s: String = name
        .chars()
        .filter(|c| *c != '#')
        .map(|c| match c {
            '|' | '^' => '-',
            '[' => '(',
            ']' => ')',
            c => c,
        })
        .collect();
    s.split(' ').filter(|w| !w.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Byte offset of the first place in `line` where `query_lower` (already lowercase) starts,
/// ignoring case: also for letters whose lowercase form has another length (`ẞ`, `İ`).
pub fn find_ci(line: &str, query_lower: &str) -> Option<usize> {
    if query_lower.is_empty() {
        return Some(0);
    }
    line.char_indices().map(|(i, _)| i).find(|&i| {
        let mut rest = line[i..].chars().flat_map(char::to_lowercase);
        query_lower.chars().all(|q| rest.next() == Some(q))
    })
}

pub fn sanitize_note_name(name: &str) -> String {
    let cleaned: String = name.chars().map(|c| if matches!(c, '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '\0') { '-' } else { c }).collect();
    cleaned.trim().trim_matches('.').trim().to_string()
}

/// Simple case-insensitive fuzzy score: higher is better, None = no match.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let c = candidate.to_lowercase();
    if c == q {
        return Some(1000);
    }
    if c.starts_with(&q) {
        return Some(800 - c.len() as i32);
    }
    if let Some(p) = c.find(&q) {
        return Some(600 - p as i32 - c.len() as i32);
    }
    // subsequence
    let mut ci = c.chars();
    let mut score = 300;
    for qc in q.chars() {
        let mut skipped = 0;
        loop {
            match ci.next() {
                Some(x) if x == qc => break,
                Some(_) => skipped += 1,
                None => return None,
            }
        }
        score -= skipped;
    }
    Some(score - c.len() as i32)
}

#[cfg(test)]
mod tests {
    // spec: EDIT-024
    #[test]
    fn an_embeds_width_is_its_last_numeric_alias_part() {
        let w = |t: &str| find_links(t).into_iter().next().and_then(|l| l.embed_width());
        assert_eq!(w("![[a.png|300]]"), Some(300.0));
        assert_eq!(w("![[a.png|300x200]]"), Some(300.0));
        assert_eq!(w("![[a.png|a caption|120]]"), Some(120.0));
        assert_eq!(w("![[a.png|a caption]]"), None);
        assert_eq!(w("![[a.png]]"), None);
        assert_eq!(w("![[a.png|0]]"), None);
        assert_eq!(w("[[a|300]]"), None, "only an embed has a width");
        assert_eq!(embed_with_width("![[a.png]]", 300), "![[a.png|300]]");
        assert_eq!(embed_with_width("![[a.png|100]]", 300), "![[a.png|300]]");
        assert_eq!(embed_with_width("![[a.png|100x50]]", 300), "![[a.png|300]]");
        assert_eq!(embed_with_width("![[a.png|a caption]]", 300), "![[a.png|a caption|300]]");
        assert_eq!(embed_with_width("![[2024]]", 300), "![[2024|300]]");
    }

    use super::*;

    // spec: LINK-001
    #[test]
    fn wiki_links_basic() {
        let t = "See [[Alpha]] and [[Beta|the beta]] and [[Gamma#Intro]].";
        let l = find_links(t);
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].target, "Alpha");
        assert_eq!(&t[l[0].range.clone()], "[[Alpha]]");
        assert_eq!(&t[l[0].target_range.clone()], "Alpha");
        assert_eq!(l[1].alias.as_deref(), Some("the beta"));
        assert_eq!(l[1].target, "Beta");
        assert_eq!(l[2].heading.as_deref(), Some("Intro"));
        assert_eq!(l[2].target, "Gamma");
    }

    // spec: LINK-001
    #[test]
    fn embeds_and_md_links() {
        let t = "![[Sketch.excalidraw]]\n[text](My%20Note.md) [web](https://x.org)";
        let l = find_links(t);
        assert_eq!(l.len(), 2);
        assert!(l[0].embed);
        assert_eq!(l[0].target, "Sketch.excalidraw");
        assert!(l[1].markdown_style);
        assert_eq!(l[1].target, "My Note");
    }

    // spec: LINK-001
    #[test]
    fn links_skip_code() {
        let t = "`[[Not]]` [[Yes]]\n```\n[[NoFence]]\n```\n[[After]]";
        let names: Vec<_> = find_links(t).into_iter().map(|l| l.target).collect();
        assert_eq!(names, vec!["Yes", "After"]);
    }

    // spec: LINK-001
    #[test]
    fn unicode_links() {
        let t = "Über [[Ärger]] ok";
        let l = find_links(t);
        assert_eq!(l[0].target, "Ärger");
        assert_eq!(&t[l[0].range.clone()], "[[Ärger]]");
    }

    // spec: TASK-001, TASK-002
    #[test]
    fn tasks_parse_and_toggle() {
        let mut t = String::from("- [ ] one\n  * [x] two 📅 2026-12-01\n- [?] not\n1. [ ] three\n- [] no");
        let tasks = find_tasks(&t);
        assert_eq!(tasks.len(), 3);
        assert!(!tasks[0].done);
        assert!(tasks[1].done);
        assert_eq!(tasks[1].indent, 2);
        assert_eq!(tasks[1].due.as_deref(), Some("2026-12-01"));
        assert_eq!(tasks[2].text, "three");
        assert!(toggle_task_at(&mut t, tasks[0].check_offset));
        assert!(t.starts_with("- [x] one"));
        assert!(toggle_task_at(&mut t, tasks[1].check_offset));
        assert!(find_tasks(&t).iter().all(|x| x.line != 1 || !x.done));
    }

    // spec: TAG-001
    #[test]
    fn tags() {
        let t = "#one two #two/sub `#code` x#no #123\n# Heading";
        let tags: Vec<_> = find_tags(t).into_iter().map(|t| t.1).collect();
        assert_eq!(tags, vec!["one", "two/sub"]);
    }

    // spec: VAULT-016
    #[test]
    fn replace_target_keeps_heading_and_brackets() {
        let t = "[[Projects/Old#H|alias]] [x](Old.md#Intro) [y]( <My Old.md#A b> ) [z](../Old%20One.md)";
        let l = find_links(t);
        assert_eq!(written_target(t, &l[0]), "Projects/Old");
        assert_eq!(written_target(t, &l[1]), "Old.md");
        assert_eq!(written_target(t, &l[2]), "My Old.md");
        assert_eq!(written_target(t, &l[3]), "../Old One.md");
        let mut out = t.to_string();
        let new = ["Projects/New Name", "New Name.md", "New (1).md", "../a%b.md"];
        for (l, n) in l.iter().zip(new).rev() {
            out.replace_range(l.target_range.clone(), &replace_target(t, l, n));
        }
        assert_eq!(out, "[[Projects/New Name#H|alias]] [x](New%20Name.md#Intro) [y]( <New %281%29.md#A b> ) [z](../a%25b.md)");
        // what was written reads back as the new target
        let back = find_links(&out);
        assert_eq!(back[1].target, "New Name");
        assert_eq!(back[1].heading.as_deref(), Some("Intro"));
        assert_eq!(back[2].target, "New (1)");
        assert_eq!(back[3].target, "../a%b");
    }

    // spec: TAG-001, LINK-006, VAULT-012
    #[test]
    fn file_names_with_a_hash_sign() {
        let l = find_links("- [[z_media/2026/11/261112_Your receipt from Shop, Limited #2583-3353-7982.eml]] and [[Note#Intro]] and [[Plan#Version 1.2]]");
        assert_eq!(l[0].target, "z_media/2026/11/261112_Your receipt from Shop, Limited #2583-3353-7982.eml");
        assert_eq!(l[0].heading, None);
        assert_eq!((l[1].target.as_str(), l[1].heading.as_deref()), ("Note", Some("Intro")));
        assert_eq!((l[2].target.as_str(), l[2].heading.as_deref()), ("Plan", Some("Version 1.2")), "a number is no extension");
        assert_eq!(link_safe_name("Receipt #2583 [paid] | A^B.eml"), "Receipt 2583 (paid) - A-B.eml");
        let tags: Vec<String> = find_tags("- [[z_media/x/Receipt #2583-3353.eml]] #real").into_iter().map(|t| t.1).collect();
        assert_eq!(tags, vec!["real"], "no tag from inside the link");
    }

    // spec: LINK-007
    #[test]
    fn headings() {
        let h = find_headings("# A\ntext\n## B\n#notheading\n```\n# code\n```");
        assert_eq!(h, vec![(0, 1, "A".into()), (2, 2, "B".into())]);
    }

    // spec: EDIT-001
    // spec: NAV-009
    #[test]
    fn find_ignores_case_also_for_other_letters() {
        assert_eq!(find_ci("first Apple", "apple"), Some(6));
        assert_eq!(find_ci("  Ünïcode ÄPFEL", "äpfel"), Some("  Ünïcode ".len()));
        assert_eq!(find_ci("Die STRAẞE", "straße"), Some(4));
        assert_eq!(find_ci("nothing", "apple"), None);
        assert_eq!(find_ci("x", ""), Some(0));
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_score("al", "Alpha").unwrap() > fuzzy_score("al", "Total").unwrap());
        assert!(fuzzy_score("xyz", "Alpha").is_none());
        assert!(fuzzy_score("apa", "Alpha").is_some());
    }

    // spec: LINK-001
    #[test]
    fn offsets() {
        let t = "äb\ncd";
        assert_eq!(line_start_byte(t, 1), 4);
        assert_eq!(byte_to_char(t, 4), 3);
        assert_eq!(char_to_byte(t, 3), 4);
    }

    // spec: EDIT-027
    #[test]
    fn the_colour_in_a_mark_or_span_tag_is_no_hashtag() {
        let text = "<mark style=\"background: #FFFF00;\">hi</mark> <span style='color: #F00'>x</span> <mark style=\"background: #0F0\">open #real";
        let tags: Vec<String> = find_tags(text).into_iter().map(|(_, t)| t).collect();
        assert_eq!(tags, vec!["real"], "only the real tag is one, also after an unclosed mark");
    }
}
