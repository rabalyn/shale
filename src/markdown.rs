//! Markdown helpers: wiki-link / task / tag / heading parsing.
//! Everything here is pure (no UI) so it is easy to unit test.

use std::ops::Range;

/// A link found in a note: `[[Target]]`, `[[Target|Alias]]`, `[[Target#Heading]]`,
/// `![[Embed]]` or a markdown link `[text](Target.md)` that points inside the vault, or a `file://`
/// address of a media file outside it (MEDIA-015).
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
    /// A library link or a file link to a media file outside the vault (MEDIA-014, MEDIA-015). Such
    /// a link is no note, so it is not red and never creates one; whether the vault has a file of
    /// that path is `Vault::resolve`'s business.
    pub fn is_outside(&self) -> bool {
        crate::media::outside::spot(&self.target).is_some()
    }

    /// The width an embed asks for (`![[pic.png|300]]`, `|300x200` too: the width counts), if its
    /// last `|` part is a number (EDIT-052).
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
/// numeric last `|` part is replaced, otherwise one is added (EDIT-052).
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
                            // a `file://` address of a media file is a link too (MEDIA-015); its target stays as
                            // written, the address is decoded once where it is read
                            let file_link = crate::media::outside::is_file_link(raw);
                            if !raw.is_empty() && (!is_external(raw) || file_link) {
                                let embed = i > 0 && b[i - 1] == b'!';
                                let start = if embed { i - 1 } else { i };
                                // the `#` that starts the heading is the written one: a `%23` is a `#` of the name (#538)
                                let (target, heading) = match raw.find('#').filter(|_| !file_link) {
                                    Some(p) => (percent_decode(&raw[..p]), Some(percent_decode(&raw[p + 1..]))),
                                    None if file_link => (raw.to_string(), None),
                                    None => (percent_decode(raw), None),
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
        // the colour in `<mark style="background: #FFFF00;">` is no tag (EDIT-028)
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

// ------------------------------------------------------------------ footnotes (EDIT-050)

/// A footnote reference `[^label]` in a note.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FootnoteRef {
    /// 0-based line number
    pub line: usize,
    /// byte range of `[^label]` in the line
    pub range: Range<usize>,
    pub label: String,
}

/// A footnote definition `[^label]: text` with the lines it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FootnoteDef {
    pub label: String,
    /// the lines of the definition (0-based, end excluded): its first line and the indented ones after it
    pub lines: Range<usize>,
    /// byte range of `[^label]:` in the first line
    pub marker: Range<usize>,
}

/// The footnotes of a note: references and definitions, in the order of the text.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Footnotes {
    pub refs: Vec<FootnoteRef>,
    pub defs: Vec<FootnoteDef>,
}

impl Footnotes {
    /// The (first) definition of `label`.
    pub fn def(&self, label: &str) -> Option<&FootnoteDef> {
        self.defs.iter().find(|d| d.label == label)
    }

    /// The first reference to `label`.
    pub fn first_ref(&self, label: &str) -> Option<&FootnoteRef> {
        self.refs.iter().find(|r| r.label == label)
    }

    /// The labels that have a definition.
    pub fn defined(&self) -> std::collections::HashSet<String> {
        self.defs.iter().map(|d| d.label.clone()).collect()
    }

    /// The definition that has `line` as its first line.
    pub fn def_at(&self, line: usize) -> Option<&FootnoteDef> {
        self.defs.iter().find(|d| d.lines.start == line)
    }

    /// Whether `line` is an indented line after the first line of a definition.
    pub fn is_continuation(&self, line: usize) -> bool {
        self.defs.iter().any(|d| d.lines.start < line && line < d.lines.end)
    }
}

fn is_footnote_label_char(c: char) -> bool {
    c.is_alphanumeric() || c == '-' || c == '_'
}

/// The `[^label]:` that starts a line: its label and its byte range.
pub fn footnote_def_marker(line: &str) -> Option<(String, Range<usize>)> {
    let rest = line.strip_prefix("[^")?;
    let len: usize = rest.chars().take_while(|c| is_footnote_label_char(*c)).map(char::len_utf8).sum();
    (len > 0 && rest[len..].starts_with("]:")).then(|| (rest[..len].to_string(), 0..2 + len + 2))
}

/// Whether a line continues a footnote definition: indented by 4 spaces or a tab, and not blank.
pub fn is_footnote_continuation(line: &str) -> bool {
    (line.starts_with("    ") || line.starts_with('\t')) && !line.trim().is_empty()
}

/// The footnote references in one line, not the ones in inline code and not the label of a
/// definition at the start of the line. Ranges are in the line.
pub fn footnote_refs_in_line(line: &str) -> Vec<(Range<usize>, String)> {
    if !line.contains("[^") {
        return Vec::new();
    }
    let code = inline_code_ranges(line);
    let own = footnote_def_marker(line).map(|(_, r)| r);
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = line[from..].find("[^") {
        let at = from + p;
        from = at + 2;
        let rest = &line[at + 2..];
        let len: usize = rest.chars().take_while(|c| is_footnote_label_char(*c)).map(char::len_utf8).sum();
        if len == 0 || !rest[len..].starts_with(']') || in_ranges(at, &code) || own.as_ref().is_some_and(|r| r.start == at) {
            continue;
        }
        let end = at + 2 + len + 1;
        out.push((at..end, rest[..len].to_string()));
        from = end;
    }
    out
}

/// The footnotes of a note (EDIT-050). Fenced code and front matter hold none.
pub fn find_footnotes(text: &str) -> Footnotes {
    let code = fenced_code_lines(text);
    let front = front_matter_lines(text);
    let lines = lines_with_offsets(text);
    let mut out = Footnotes::default();
    let mut li = 0;
    while li < lines.len() {
        let line = lines[li].1;
        if li < front || code.get(li).copied().unwrap_or(false) {
            li += 1;
            continue;
        }
        for (range, label) in footnote_refs_in_line(line) {
            out.refs.push(FootnoteRef { line: li, range, label });
        }
        if let Some((label, marker)) = footnote_def_marker(line) {
            let mut end = li + 1;
            while end < lines.len() && !code.get(end).copied().unwrap_or(false) && is_footnote_continuation(lines[end].1) {
                for (range, label) in footnote_refs_in_line(lines[end].1) {
                    out.refs.push(FootnoteRef { line: end, range, label });
                }
                end += 1;
            }
            out.defs.push(FootnoteDef { label, lines: li..end, marker });
            li = end;
            continue;
        }
        li += 1;
    }
    out
}

thread_local! {
    /// The labels that have a definition in the note that is being drawn (EDIT-050).
    static DEFINED: std::cell::RefCell<std::collections::HashSet<String>> = std::cell::RefCell::new(std::collections::HashSet::new());
}

/// The labels that have a definition, in force while this value lives: the inline layout of the
/// reading view, tables and the PDF export shows a reference as a footnote mark only for these
/// (EDIT-050). The labels before come back when it is dropped.
pub struct DefinedFootnotes(std::collections::HashSet<String>);

impl DefinedFootnotes {
    pub fn set(labels: std::collections::HashSet<String>) -> Self {
        DefinedFootnotes(DEFINED.with(|d| std::mem::replace(&mut *d.borrow_mut(), labels)))
    }
}

impl Drop for DefinedFootnotes {
    fn drop(&mut self) {
        DEFINED.with(|d| *d.borrow_mut() = std::mem::take(&mut self.0));
    }
}

/// The labels set by [`DefinedFootnotes`].
pub fn defined_footnotes_in_scope() -> std::collections::HashSet<String> {
    DEFINED.with(|d| d.borrow().clone())
}

/// Whether the note being drawn has a definition for `label` (see [`DefinedFootnotes`]).
pub fn footnote_is_defined(label: &str) -> bool {
    DEFINED.with(|d| d.borrow().contains(label))
}

// ------------------------------------------------------------------ math (EDIT-046)

/// A formula in a note: `$…$`, `$$…$$` or a ```` ```math ```` block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MathSpan {
    /// the whole formula with its delimiters, bytes of the text
    pub range: Range<usize>,
    /// the TeX inside the delimiters
    pub tex: Range<usize>,
    /// `$$…$$` and ```` ```math ````: typeset in display style
    pub display: bool,
    /// the formula takes whole lines (`$$` on lines of their own, a `math` fence, or `$$…$$` alone
    /// on its line): it is drawn centred on its own
    pub block: bool,
    /// the lines it takes (0-based, end excluded)
    pub lines: Range<usize>,
}

/// The formulas of a note, in the order of the text (EDIT-046). Inline math is `$…$` where the
/// opening `$` is not followed by a space and the closing one is not preceded by a space nor
/// followed by a digit, on one line; `\$` is a dollar sign. Display math is `$$…$$` on one line,
/// or `$$` on lines of their own around other lines; a ```` ```math ```` fence is display math too.
/// Nothing in code, in other fences or in the front matter counts.
pub fn find_math(text: &str) -> Vec<MathSpan> {
    let lines = lines_with_offsets(text);
    let mut out = Vec::new();
    let mut li = front_matter_lines(text);
    // inside a fence: Some(None) is code, Some(Some((line, body start))) a math fence
    let mut fence: Option<Option<(usize, usize)>> = None;
    while li < lines.len() {
        let (off, line) = lines[li];
        let t = line.trim_start();
        let is_fence = t.starts_with("```") || t.starts_with("~~~");
        match fence {
            Some(open) => {
                if is_fence {
                    if let Some((first, body)) = open {
                        let end = off + line.len();
                        out.push(MathSpan { range: lines[first].0..end, tex: body..off.saturating_sub(1).max(body), display: true, block: true, lines: first..li + 1 });
                    }
                    fence = None;
                }
            }
            None if is_fence => {
                let math = t.starts_with("```") && t[3..].trim().eq_ignore_ascii_case("math");
                fence = Some(math.then(|| (li, off + line.len() + 1)));
            }
            None if line.trim() == "$$" => {
                let close = (li + 2..lines.len()).find(|&k| lines[k].1.trim() == "$$");
                if let Some(k) = close {
                    let body = off + line.len() + 1;
                    out.push(MathSpan { range: off..lines[k].0 + lines[k].1.len(), tex: body..lines[k].0 - 1, display: true, block: true, lines: li..k + 1 });
                    li = k;
                }
            }
            None => inline_math(line, off, li, &mut out),
        }
        li += 1;
    }
    out
}

/// The formulas inside one line (see [`find_math`]).
fn inline_math(line: &str, off: usize, li: usize, out: &mut Vec<MathSpan>) {
    let b = line.as_bytes();
    let code = inline_code_ranges(line);
    let mut i = 0;
    // the first position after `from` where `want` (`$` or `$$`) stands, outside code and not escaped
    let find = |from: usize, want: &str| -> Option<usize> {
        let mut k = from;
        while k < b.len() {
            if b[k] == b'\\' {
                k += 1 + line[k + 1..].chars().next().map_or(0, char::len_utf8);
            } else if let Some(r) = code.iter().find(|r| r.contains(&k)) {
                k = r.end;
            } else if b[k..].starts_with(want.as_bytes()) {
                return Some(k);
            } else {
                k += 1;
            }
        }
        None
    };
    while i < b.len() {
        if b[i] == b'\\' {
            i += 1 + line[i + 1..].chars().next().map_or(0, char::len_utf8);
        } else if let Some(r) = code.iter().find(|r| r.contains(&i)) {
            i = r.end;
        } else if b[i] != b'$' {
            i += 1;
        } else if b.get(i + 1) == Some(&b'$') {
            match find(i + 2, "$$").filter(|&j| !line[i + 2..j].trim().is_empty()) {
                Some(j) => {
                    let alone = line.trim() == &line[i..j + 2];
                    out.push(MathSpan { range: off + i..off + j + 2, tex: off + i + 2..off + j, display: true, block: alone, lines: li..li + 1 });
                    i = j + 2;
                }
                None => i += 2,
            }
        } else {
            let opens = line[i + 1..].chars().next().is_some_and(|c| !c.is_whitespace());
            let mut from = i + 1;
            let close = loop {
                let Some(j) = opens.then(|| find(from, "$")).flatten() else { break None };
                let ok = j > i + 1 && !line[..j].ends_with(char::is_whitespace) && !b.get(j + 1).is_some_and(u8::is_ascii_digit);
                if ok {
                    break Some(j);
                }
                from = j + 1;
            };
            match close {
                Some(j) => {
                    out.push(MathSpan { range: off + i..off + j + 1, tex: off + i + 1..off + j, display: false, block: false, lines: li..li + 1 });
                    i = j + 1;
                }
                None => i += 1,
            }
        }
    }
}

/// How many lines the front matter (`---` … `---` at the top) takes; 0 when there is none.
pub fn front_matter_lines(text: &str) -> usize {
    let mut lines = text.split('\n');
    if lines.next().map(str::trim) != Some("---") {
        return 0;
    }
    lines.position(|l| l.trim() == "---").map_or(0, |p| p + 2)
}

/// The properties of the front matter as (key in lower case, values): `key: value` has one value,
/// `key: [a, b]` and a list of `- item` lines have several, `key: |` takes its indented lines.
/// Quotes around a value are removed; nothing is interpreted (no dates, no numbers, no nesting).
pub fn front_matter_values(text: &str) -> Vec<(String, Vec<String>)> {
    let unquote = |v: &str| {
        let v = v.trim();
        let quoted = v.len() >= 2 && ((v.starts_with('"') && v.ends_with('"')) || (v.starts_with('\'') && v.ends_with('\'')));
        (if quoted { &v[1..v.len() - 1] } else { v }).to_string()
    };
    if front_matter_lines(text) == 0 {
        return vec![];
    }
    let mut lines = text.lines();
    lines.next();
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    // the key whose following lines (a list or a block) are still being read
    let mut open: Option<usize> = None;
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        let item = l.trim().strip_prefix("- ").or_else(|| (l.trim() == "-").then_some(""));
        match (open, item) {
            (Some(i), Some(it)) if l.starts_with(char::is_whitespace) || l.starts_with('-') => {
                out[i].1.push(unquote(it));
                continue;
            }
            (Some(i), _) if l.starts_with(char::is_whitespace) && !l.trim().is_empty() => {
                out[i].1.push(l.trim().to_string());
                continue;
            }
            _ => {}
        }
        let Some((key, value)) = l.split_once(':').filter(|(k, _)| !k.is_empty() && !k.starts_with(char::is_whitespace) && !k.starts_with('#')) else {
            continue;
        };
        let value = value.trim();
        let values = match value {
            "" | "|" | ">" | "|-" | ">-" => vec![],
            v if v.starts_with('[') && v.ends_with(']') => v[1..v.len() - 1].split(',').map(unquote).filter(|x| !x.is_empty()).collect(),
            v => vec![unquote(v)],
        };
        open = matches!(value, "" | "|" | ">" | "|-" | ">-").then_some(out.len());
        out.push((key.trim().to_lowercase(), values));
    }
    out
}

/// Headings: (line number, level, title). Fenced code, `$$` math blocks and front matter hold none.
pub fn find_headings(text: &str) -> Vec<(usize, usize, String)> {
    let mut code = fenced_code_lines(text);
    let front = front_matter_lines(text);
    if text.contains("$$") {
        for m in find_math(text).into_iter().filter(|m| m.block) {
            for line in m.lines {
                code[line] = true;
            }
        }
    }
    let mut out = Vec::new();
    for (li, (_, line)) in lines_with_offsets(text).into_iter().enumerate() {
        if li < front || code.get(li).copied().unwrap_or(false) {
            continue;
        }
        if let Some(level) = heading_level(line) {
            out.push((li, level, line[level..].trim().to_string()));
        }
    }
    out
}

/// `text` with all its headings moved up by the same number of levels so that the highest becomes
/// `#` (`### a` / `#### b` → `# a` / `## b`); None when it has a `#` heading already or no heading
/// at all. Headings in fenced code and front matter, and all other text, stay as they are
/// (IMP-034, EDIT-038).
pub fn lift_headings(text: &str) -> Option<String> {
    let heads = find_headings(text);
    let by = heads.iter().map(|h| h.1).min()?.checked_sub(1).filter(|by| *by > 0)?;
    let lines = lines_with_offsets(text);
    let mut out = text.to_string();
    for (line, _, _) in heads.into_iter().rev() {
        let at = lines[line].0;
        out.replace_range(at..at + by, "");
    }
    Some(out)
}

/// The title of a heading as it reads: no inline markup, links by their shown text.
pub fn heading_plain(title: &str) -> String {
    let links: Vec<(LinkSpan, bool)> = find_links(title).into_iter().map(|l| (l, true)).collect();
    let (job, _, _) = crate::reader::inline_job(title, &links, 14.0, crate::theme::palette().text, false);
    job.text.trim().to_string()
}

/// The line of the first heading whose text is `heading`, ignoring case, surrounding spaces and
/// inline markup (LINK-015): `## **Plan** v2` is found by `Plan v2`.
pub fn find_heading(text: &str, heading: &str) -> Option<usize> {
    let want = heading_plain(heading).to_lowercase();
    find_headings(text).into_iter().find(|(_, _, t)| heading_plain(t).to_lowercase() == want).map(|(line, _, _)| line)
}

/// Whether the headings of this note are numbered: the front matter's `numbered-headings:`
/// (`true`/`false`, `yes`/`no`) when it has one, else `vault_default` (CFG-040).
pub fn headings_numbered(text: &str, vault_default: bool) -> bool {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return vault_default;
    }
    for l in lines {
        if l.trim() == "---" {
            break;
        }
        if let Some(v) = l.strip_prefix("numbered-headings:") {
            match v.trim().trim_matches(|c| c == '"' || c == '\'').to_lowercase().as_str() {
                "true" | "yes" => return true,
                "false" | "no" => return false,
                _ => {}
            }
        }
    }
    vault_default
}

/// The headings with their numbers (LINK-014): (line, level, title as in [`find_headings`], number
/// such as `1.2`). Every level is numbered by nesting from `#` (the note's title is its title line,
/// VAULT-025, not a heading); a heading deeper than the one before it but with nothing shallower
/// above it starts at 1. The number is always `Some` for a real heading; the `Option` stays so the
/// signature is stable for its callers.
pub fn heading_numbers(text: &str) -> Vec<(usize, usize, String, Option<String>)> {
    // the open headings: (level, its number); `kids` counts the children of each, `top` those of the root
    let mut open: Vec<(usize, Vec<usize>, usize)> = Vec::new();
    let mut top = 0;
    let mut out = Vec::new();
    for (line, level, title) in find_headings(text) {
        while open.last().is_some_and(|(l, _, _)| *l >= level) {
            open.pop();
        }
        let mut number = match open.last_mut() {
            Some((_, n, kids)) => {
                *kids += 1;
                let mut n = n.clone();
                n.push(*kids);
                n
            }
            None => {
                top += 1;
                vec![top]
            }
        };
        let label = number.iter().map(usize::to_string).collect::<Vec<_>>().join(".");
        open.push((level, std::mem::take(&mut number), 0));
        out.push((line, level, title, Some(label)));
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

/// Whether `line` is a page-break marker (PDF-020): a line of its own that holds an empty `<div>`
/// whose `style` has `page-break-after: always` (the form Obsidian writes), `page-break-before:
/// always`, `break-after: page` or `break-before: page`; case, spaces, either quote and a
/// trailing `;` do not matter. Whether the line is inside a code fence or the front matter is
/// [`page_break_lines`]'s to say.
pub fn page_break_line(line: &str) -> bool {
    let t = line.trim().to_ascii_lowercase();
    let Some(rest) = t.strip_prefix("<div") else { return false };
    let Some(rest) = rest.strip_prefix(char::is_whitespace) else { return false };
    let Some(rest) = rest.trim_start().strip_prefix("style") else { return false };
    let Some(rest) = rest.trim_start().strip_prefix('=') else { return false };
    let rest = rest.trim_start();
    let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { return false };
    let Some((style, rest)) = rest[1..].split_once(quote) else { return false };
    let Some(rest) = rest.trim_start().strip_prefix('>') else { return false };
    if rest.trim() != "</div>" {
        return false;
    }
    style.split(';').any(|decl| {
        let Some((name, value)) = decl.split_once(':') else { return false };
        matches!((name.trim(), value.trim()), ("page-break-after" | "page-break-before", "always") | ("break-after" | "break-before", "page"))
    })
}

/// The lines (indices) of `text` that are page-break markers (PDF-020): not those in a code
/// fence or the front matter.
pub fn page_break_lines(text: &str) -> Vec<usize> {
    let fenced = fenced_code_lines(text);
    let front = front_matter_lines(text);
    lines_with_offsets(text).iter().enumerate().filter(|(i, (_, l))| *i >= front && !fenced.get(*i).copied().unwrap_or(false) && page_break_line(l)).map(|(i, _)| i).collect()
}

/// How a paragraph is aligned (PDF-028). `Left` is no wrapper at all in what Shale writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
    Justify,
}

impl Align {
    /// The value of the `align` attribute of the wrapper that writes it.
    pub fn word(self) -> &'static str {
        match self {
            Align::Left => "left",
            Align::Center => "center",
            Align::Right => "right",
            Align::Justify => "justify",
        }
    }

    /// The opening line of the wrapper that writes it: `<div align="center">`.
    pub fn open_line(self) -> String {
        format!("<div align=\"{}\">", self.word())
    }
}

/// The alignment an opening wrapper line asks for (PDF-028): a line of its own that holds
/// `<div align="center">` (also `right`, `justify` and `left`). Case, spaces around `=` and either
/// quote kind do not matter; another attribute, another tag, another value or text before or after
/// it on the line is no wrapper.
pub fn align_open_line(line: &str) -> Option<Align> {
    let t = line.trim().to_ascii_lowercase();
    let rest = t.strip_prefix("<div")?.strip_prefix(char::is_whitespace)?.trim_start().strip_prefix("align")?.trim_start().strip_prefix('=')?.trim_start();
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let (value, rest) = rest[1..].split_once(quote)?;
    if rest.trim() != ">" {
        return None;
    }
    match value.trim() {
        "left" => Some(Align::Left),
        "center" => Some(Align::Center),
        "right" => Some(Align::Right),
        "justify" => Some(Align::Justify),
        _ => None,
    }
}

/// Whether `line` is the closing line of a wrapper: `</div>` alone.
pub fn align_close_line(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("</div>")
}

/// A wrapper that aligns the paragraphs and headings between its two lines (PDF-028): the lines
/// (indices) of its opening and closing line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlignBlock {
    pub open: usize,
    pub close: usize,
    pub align: Align,
}

impl AlignBlock {
    /// Whether `line` is the opening or the closing line of the wrapper.
    pub fn is_edge(&self, line: usize) -> bool {
        line == self.open || line == self.close
    }

    /// Whether `line` is between the two lines of the wrapper.
    pub fn holds(&self, line: usize) -> bool {
        line > self.open && line < self.close
    }
}

/// The wrappers of `text` (PDF-028), in order: an opening line with the next `</div>` line after
/// it, as long as no other opening line comes first. An opening line without a closing one is
/// text; so is anything in a code fence or the front matter.
pub fn align_blocks(text: &str) -> Vec<AlignBlock> {
    let fenced = fenced_code_lines(text);
    let front = front_matter_lines(text);
    let lines = lines_with_offsets(text);
    let plain = |i: usize| i >= front && !fenced.get(i).copied().unwrap_or(false);
    let mut out = Vec::new();
    let mut i = front;
    while i < lines.len() {
        let Some(align) = align_open_line(lines[i].1).filter(|_| plain(i)) else {
            i += 1;
            continue;
        };
        let mut j = i + 1;
        while j < lines.len() && !(plain(j) && (align_close_line(lines[j].1) || align_open_line(lines[j].1).is_some())) {
            j += 1;
        }
        if j < lines.len() && align_close_line(lines[j].1) {
            out.push(AlignBlock { open: i, close: j, align });
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// The alignment of line `line` of a note with these wrappers (PDF-028): that of the wrapper that
/// holds it, `None` outside every wrapper (and on the wrapper lines).
pub fn align_at(blocks: &[AlignBlock], line: usize) -> Option<Align> {
    blocks.iter().find(|b| b.holds(line)).map(|b| b.align)
}

/// Whether `line` is the opening or closing line of a wrapper.
pub fn is_align_edge(blocks: &[AlignBlock], line: usize) -> bool {
    blocks.iter().any(|b| b.is_edge(line))
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
    percent_decode(core.find('#').map_or(core, |p| &core[..p]))
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

/// Point every link of `text` whose target is `target` (as [`LinkSpan::target`]: no `.md`, case
/// ignored) at the note `new_stem` in the same place (the last part of the path is replaced), so a
/// red link still reaches the note made for it under a name with a date prefix (CFG-043):
/// `[[Plan]]` → `[[261117_Plan|Plan]]` (a wiki link keeps its alias, or gets the text it showed as
/// one; an embed gets none), `[[Plan#H]]` → `[[261117_Plan#H|Plan > H]]`, `[t](Plan.md)` →
/// `[t](261117_Plan.md)`, `[[./Plan]]` → `[[./261117_Plan|./Plan]]`. None when no link matches.
pub fn relink_to(text: &str, target: &str, new_stem: &str) -> Option<String> {
    let want = strip_md_ext(target.trim());
    let mut out = text.to_string();
    let mut changed = false;
    for link in find_links(text).into_iter().rev().filter(|l| l.target.eq_ignore_ascii_case(want)) {
        let written = written_target(text, &link);
        let (dir, file) = written.rsplit_once('/').map_or(("", written.as_str()), |(d, f)| (&written[..d.len() + 1], f));
        let ext = if file.len() > 3 && file.is_char_boundary(file.len() - 3) && file[file.len() - 3..].eq_ignore_ascii_case(".md") { &file[file.len() - 3..] } else { "" };
        let new_written = format!("{dir}{new_stem}{ext}");
        if !link.markdown_style && link.alias.is_none() && !link.embed {
            out.insert_str(link.range.end - 2, &format!("|{}", link.display()));
        }
        out.replace_range(link.target_range.clone(), &replace_target(text, &link, &new_written));
        changed = true;
    }
    changed.then_some(out)
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
    // the common case, without taking the line apart: plain ASCII on both sides
    if query_lower.is_ascii() && line.is_ascii() {
        let q = query_lower.as_bytes();
        return line.as_bytes().windows(q.len()).position(|w| w.eq_ignore_ascii_case(q));
    }
    line.char_indices().map(|(i, _)| i).find(|&i| {
        let mut rest = line[i..].chars().flat_map(char::to_lowercase);
        query_lower.chars().all(|q| rest.next() == Some(q))
    })
}

pub fn sanitize_note_name(name: &str) -> String {
    sanitize_note_name_max(name, crate::fsutil::TYPED_NAME_MAX)
}

/// [`sanitize_note_name`] cut to `max_bytes`.
pub fn sanitize_note_name_max(name: &str, max_bytes: usize) -> String {
    let name = name.trim_start_matches(|c: char| c == '.' || c.is_whitespace());
    crate::fsutil::portable_name(name, max_bytes).trim().to_string()
}

/// Simple case-insensitive fuzzy score: higher is better, None = no match.
pub fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let q = crate::vault::name_key(query);
    let c = crate::vault::name_key(candidate);
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
    // spec: PDF-020
    #[test]
    fn page_break_lines_are_the_obsidian_div_in_its_forms_and_near_misses_are_not() {
        for yes in [
            r#"<div style="page-break-after: always;"></div>"#,
            r#"<div style="page-break-after: always"></div>"#,
            r#"  <DIV  STYLE = 'Page-Break-Before:ALWAYS;' > </Div>  "#,
            r#"<div style="break-after: page;"></div>"#,
            r#"<div style='break-before:page'></div>"#,
            r#"<div style="color: red; page-break-after: always;"></div>"#,
        ] {
            assert!(page_break_line(yes), "{yes}");
        }
        for no in [
            r#"<div style="page-break-after: avoid;"></div>"#,
            r#"<div style="break-after: always;"></div>"#,
            r#"<div style="page-break-after: always;">text</div>"#,
            r#"text <div style="page-break-after: always;"></div>"#,
            r#"<div style="page-break-after: always;">"#,
            r#"<div class="page-break-after: always;"></div>"#,
            r#"<span style="page-break-after: always;"></span>"#,
            r#"<div style="page-break-after: always;"></div> more"#,
            "<div></div>",
            "",
        ] {
            assert!(!page_break_line(no), "{no}");
        }
    }

    // spec: PDF-020
    #[test]
    fn page_breaks_in_code_fences_and_front_matter_are_not_markers() {
        let m = r#"<div style="page-break-after: always;"></div>"#;
        let text = format!(
            "---
title: {m}
{m}
---
a
{m}
```
{m}
```
{m}"
        );
        assert_eq!(page_break_lines(&text), vec![5, 9]);
    }

    // spec: PDF-028
    #[test]
    fn the_align_wrapper_is_a_div_with_an_align_attribute_on_its_own_line() {
        for (line, want) in [(r#"<div align="center">"#, Align::Center), ("  <DIV  Align = 'Right' >  ", Align::Right), (r#"<div align="justify">"#, Align::Justify), (r#"<div align="left">"#, Align::Left)] {
            assert_eq!(align_open_line(line), Some(want), "{line}");
        }
        for no in [
            r#"<div align="middle">"#,
            r#"<div align="center" class="x">"#,
            r#"<div>"#,
            r#"text <div align="center">"#,
            r#"<div align="center">text"#,
            r#"<div align="center"></div>"#,
            r#"<div style="text-align: center">"#,
            r#"<p align="center">"#,
            r#"<divalign="center">"#,
        ] {
            assert_eq!(align_open_line(no), None, "{no}");
        }
        assert!(align_close_line("</div>") && align_close_line("  </DIV> "));
        assert!(!align_close_line("</div> x") && !align_close_line("</p>"));
        assert_eq!(Align::Center.open_line(), r#"<div align="center">"#);
    }

    // spec: PDF-028
    #[test]
    fn a_wrapper_holds_the_lines_up_to_the_next_closing_line_and_is_none_without_one() {
        let text = "a\n<div align=\"center\">\n\nb\n\n</div>\nc\n<div align=\"right\">\nd\n</div>\n<div align=\"center\">\nlost";
        let blocks = align_blocks(text);
        assert_eq!(blocks, vec![AlignBlock { open: 1, close: 5, align: Align::Center }, AlignBlock { open: 7, close: 9, align: Align::Right }]);
        assert_eq!(align_at(&blocks, 3), Some(Align::Center));
        assert_eq!(align_at(&blocks, 8), Some(Align::Right));
        assert_eq!((align_at(&blocks, 0), align_at(&blocks, 1), align_at(&blocks, 6), align_at(&blocks, 11)), (None, None, None, None));
        assert!(is_align_edge(&blocks, 1) && is_align_edge(&blocks, 9) && !is_align_edge(&blocks, 3));
        // another opening line first: the first has no closing line
        let nested = "<div align=\"center\">\n<div align=\"right\">\nx\n</div>";
        assert_eq!(align_blocks(nested), vec![AlignBlock { open: 1, close: 3, align: Align::Right }]);
    }

    // spec: PDF-028
    #[test]
    fn wrappers_in_code_fences_and_front_matter_are_text() {
        let o = r#"<div align="center">"#;
        let text = format!("---\ntitle: x\n{o}\n</div>\n---\n```\n{o}\n</div>\n```\n{o}\nok\n</div>");
        assert_eq!(align_blocks(&text), vec![AlignBlock { open: 9, close: 11, align: Align::Center }]);
        // the page-break marker is not a wrapper
        assert!(align_blocks(r#"<div style="page-break-after: always;"></div>"#).is_empty());
    }

    // spec: EDIT-052
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

    // spec: MEDIA-014, MEDIA-015, LINK-001
    #[test]
    fn library_links_and_file_links_to_media_are_links() {
        let t = "![[@Videos/2024/a.mp4|480]] [[@Music/Album/b.flac]]\n![](<file:///home/ada/My Videos/a.mp4>) ![pic](file:///home/ada/My%20Pics/a%20b.png) [song](<file:///home/ada/Track #3.mp3>)";
        let l = find_links(t);
        assert_eq!(l.len(), 5, "{l:?}");
        assert_eq!((l[0].target.as_str(), l[0].embed, l[0].embed_width()), ("@Videos/2024/a.mp4", true, Some(480.0)));
        assert_eq!((l[1].target.as_str(), l[1].embed), ("@Music/Album/b.flac", false));
        assert_eq!((l[2].target.as_str(), l[2].embed, l[2].markdown_style), ("file:///home/ada/My Videos/a.mp4", true, true), "the angle brackets are not part of it");
        assert_eq!(&t[l[2].range.clone()], "![](<file:///home/ada/My Videos/a.mp4>)");
        assert_eq!((l[3].target.as_str(), l[3].alias.as_deref()), ("file:///home/ada/My%20Pics/a%20b.png", Some("pic")), "the address is kept as written, it is decoded once where it is read");
        assert_eq!((l[4].target.as_str(), l[4].heading.as_deref()), ("file:///home/ada/Track #3.mp3", None), "a # is part of a file name, not a heading");
        assert!(l.iter().all(|l| l.is_outside()));
        // not links: another kind of file, another scheme, an address with a host, inside code
        let not = "[a](file:///home/ada/notes.docx) [b](https://x.org/a.mp4) [c](file://host/a.mp4) `![](file:///a.mp4)` [[@Videos]] [[@Videos/notes.docx]]";
        let found: Vec<_> = find_links(not).into_iter().map(|l| l.target).collect();
        assert_eq!(found, ["@Videos", "@Videos/notes.docx"], "only the two wiki links (notes, red as before); none is an outside link");
        assert!(find_links("[[@Videos]]")[0].target == "@Videos" && !find_links("[[@Videos]]")[0].is_outside());
        assert!(!find_links("[[@Videos/notes.docx]]")[0].is_outside());
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

    // spec: EDIT-116
    #[test]
    fn edit_116_a_heading_in_a_math_block_is_no_heading() {
        let text = "# A\n$$\n# comment\nx = 1\n$$\n## B\n```math\n# not either\n```\n$$ # one line $$\n### C\n";
        let h: Vec<_> = find_headings(text).into_iter().map(|h| (h.0, h.1)).collect();
        assert_eq!(h, vec![(0, 1), (5, 2), (10, 3)]);
        assert_eq!(lift_headings("## A\n$$\n## in math\n$$\n### B").as_deref(), Some("# A\n$$\n## in math\n$$\n## B"), "the shift leaves the math alone");
        assert_eq!(find_headings("$$\n# no closing line\n").len(), 1, "a $$ that is never closed is no math block");
    }

    // spec: EDIT-001, PERF-004
    // spec: NAV-009
    #[test]
    fn find_ignores_case_also_for_other_letters() {
        assert_eq!(find_ci("first Apple", "apple"), Some(6));
        assert_eq!(find_ci("  Ünïcode ÄPFEL", "äpfel"), Some("  Ünïcode ".len()));
        assert_eq!(find_ci("Die STRAẞE", "straße"), Some(4));
        assert_eq!(find_ci("nothing", "apple"), None);
        assert_eq!(find_ci("x", ""), Some(0));
        // the ASCII shortcut finds what the general search finds
        let general = |line: &str, q: &str| {
            line.char_indices().map(|(i, _)| i).find(|&i| {
                let mut rest = line[i..].chars().flat_map(char::to_lowercase);
                q.chars().all(|c| rest.next() == Some(c))
            })
        };
        for line in ["", "a", "Apple pie and APPLE jam", "xxapple", "applx", "The Kelvin K sign \u{212A} here", "İstanbul is", "plain text, no match"] {
            for q in ["a", "apple", "k", "kelvin k", "i", "istanbul", "pie and", "zzz", "jam"] {
                assert_eq!(find_ci(line, q), general(line, q), "{line:?} {q:?}");
            }
        }
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

    // spec: EDIT-028
    #[test]
    fn the_colour_in_a_mark_or_span_tag_is_no_hashtag() {
        let text = "<mark style=\"background: #FFFF00;\">hi</mark> <span style='color: #F00'>x</span> <mark style=\"background: #0F0\">open #real";
        let tags: Vec<String> = find_tags(text).into_iter().map(|(_, t)| t).collect();
        assert_eq!(tags, vec!["real"], "only the real tag is one, also after an unclosed mark");
    }

    fn numbers(text: &str) -> Vec<(usize, Option<String>)> {
        heading_numbers(text).into_iter().map(|(l, _, _, n)| (l, n)).collect()
    }

    // spec: LINK-014
    #[test]
    fn headings_are_numbered_by_nesting_from_h1() {
        let text = "# Title\n## A\n### B\n### C\n## D\n#### E\n### F\n## G\n# Next\n## H\n";
        let n: Vec<Option<String>> = numbers(text).into_iter().map(|(_, n)| n).collect();
        let s = |x: &str| Some(x.to_string());
        assert_eq!(n, vec![s("1"), s("1.1"), s("1.1.1"), s("1.1.2"), s("1.2"), s("1.2.1"), s("1.2.2"), s("1.3"), s("2"), s("2.1")], "a #### under a ## is 1.2.1, a later ### goes on after it");
        let (line, level, title, num) = &heading_numbers("x\n## Plan v2")[0];
        assert_eq!((*line, *level, title.as_str(), num.as_deref()), (1, 2, "Plan v2", Some("1")));
        let n: Vec<Option<String>> = numbers("# A\n### C\n## B").into_iter().map(|(_, n)| n).collect();
        assert_eq!(n, vec![s("1"), s("1.1"), s("1.2")], "a ### directly under a # is 1.1");
    }

    // spec: LINK-014
    #[test]
    fn a_heading_with_nothing_shallower_above_it_starts_at_one() {
        let n: Vec<Option<String>> = numbers("### X\n#### Y\n## A\n# Another\n## B").into_iter().map(|(_, n)| n).collect();
        let s = |x: &str| Some(x.to_string());
        assert_eq!(n, vec![s("1"), s("1.1"), s("2"), s("3"), s("3.1")]);
    }

    // spec: LINK-014
    #[test]
    fn code_fences_and_front_matter_hold_no_headings() {
        let text = "---\n# a yaml comment\ntitle: x\n---\n## A\n```\n## not\n```\n## B\n";
        assert_eq!(numbers(text), vec![(4, Some("1".into())), (8, Some("2".into()))]);
        assert_eq!(find_headings(text).len(), 2);
        assert_eq!(front_matter_lines(text), 4);
        assert_eq!(front_matter_lines("---\nunclosed\n"), 0);
        assert_eq!(front_matter_lines("text"), 0);
    }

    // spec: TEX-013
    #[test]
    fn front_matter_values_are_scalars_lists_and_blocks_with_their_quotes_removed() {
        let text = "---\nTitle: \"A: B\"\nauthor: [Ada, 'Bob C.']\nreviewer:\n  - Prof. X\n  - Prof. Y\nrecipient: |\n  Line one\n  Line two\nempty:\nnumbered-headings: false\n# comment: no\n---\nbody: not a property\n";
        let got = front_matter_values(text);
        let get = |k: &str| got.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone());
        assert_eq!(get("title"), Some(vec!["A: B".to_string()]), "keys are lower case, quotes go");
        assert_eq!(get("author"), Some(vec!["Ada".to_string(), "Bob C.".to_string()]));
        assert_eq!(get("reviewer"), Some(vec!["Prof. X".to_string(), "Prof. Y".to_string()]));
        assert_eq!(get("recipient"), Some(vec!["Line one".to_string(), "Line two".to_string()]));
        assert_eq!(get("empty"), Some(vec![]));
        assert_eq!(get("numbered-headings"), Some(vec!["false".to_string()]));
        assert!(get("body").is_none() && get("# comment").is_none() && got.len() == 6, "{got:?}");
        assert!(front_matter_values("no front matter\nkey: value").is_empty());
        assert!(front_matter_values("---\nunclosed: value\n").is_empty(), "no closing line, no front matter (as front_matter_lines)");
    }

    // spec: IMP-034, EDIT-038
    #[test]
    fn lifting_moves_every_heading_up_so_the_highest_becomes_h1_and_leaves_the_rest() {
        assert_eq!(lift_headings("### a\ntext\n#### b\n").as_deref(), Some("# a\ntext\n## b\n"));
        assert_eq!(lift_headings("## a\n## b\n### c").as_deref(), Some("# a\n# b\n## c"));
        assert_eq!(lift_headings("###### deep").as_deref(), Some("# deep"), "also from the deepest level");
        // code fences, front matter, other lines and the line ends stay
        let text = "---\n# yaml\n---\n## A\r\n```\n## in code\n```\n#tag and ## not at the start\n### B\n";
        assert_eq!(lift_headings(text).as_deref(), Some("---\n# yaml\n---\n# A\r\n```\n## in code\n```\n#tag and ## not at the start\n## B\n"));
        // nothing to do: a # heading, no heading, headings only in code
        assert_eq!(lift_headings("# A\n## B"), None);
        assert_eq!(lift_headings("## A\n# B"), None, "a # anywhere");
        assert_eq!(lift_headings("just text\n#nospace"), None);
        assert_eq!(lift_headings("```\n## code\n```\n"), None);
        assert_eq!(lift_headings(""), None);
    }

    // spec: VAULT-025
    #[test]
    fn a_date_prefix_is_exactly_six_digits_and_an_underscore() {
        use crate::vault::title_of;
        assert_eq!(title_of("261117_Plan.md"), "Plan");
        assert_eq!(title_of("261117_Plan"), "Plan");
        assert_eq!(title_of("Plan.md"), "Plan");
        assert_eq!(title_of("12345_Plan"), "12345_Plan", "five digits");
        assert_eq!(title_of("1234567_Plan"), "1234567_Plan", "seven digits");
        assert_eq!(title_of("26111a_Plan"), "26111a_Plan");
        assert_eq!(title_of("261117 Plan"), "261117 Plan", "no underscore");
        assert_eq!(title_of("261117_"), "261117_", "a name that is only the prefix keeps it");
        assert_eq!(title_of("261117_261118_Plan"), "261118_Plan", "only one prefix goes");
        assert_eq!(title_of("٢٦١١١٧_Plan"), "٢٦١١١٧_Plan", "ASCII digits only");
    }

    // spec: CFG-043
    #[test]
    fn red_links_are_pointed_at_the_note_made_with_a_date_prefix() {
        let t = "a [[Plan]] b [[plan|My plan]] c [[Plan#Goals]] d ![[Plan]] e [t](Plan.md) f [u](Plan.md#Goals) g [[Other]] h `[[Plan]]`\n";
        let want = "a [[261117_Plan|Plan]] b [[261117_Plan|My plan]] c [[261117_Plan#Goals|Plan > Goals]] d ![[261117_Plan]] e [t](261117_Plan.md) f [u](261117_Plan.md#Goals) g [[Other]] h `[[Plan]]`\n";
        assert_eq!(relink_to(t, "Plan", "261117_Plan").as_deref(), Some(want));
        assert_eq!(relink_to("[[Other]]", "Plan", "261117_Plan"), None);
        // a path keeps its folder; the new name may carry a number after a clash
        assert_eq!(relink_to("[[./Sub]] [[../x/Sub.md]]", "./Sub", "261117_Sub 1").as_deref(), Some("[[./261117_Sub 1|./Sub]] [[../x/Sub.md]]"));
        assert_eq!(relink_to("[a](../Notes/Fresh)", "../Notes/Fresh", "261117_Fresh").as_deref(), Some("[a](../Notes/261117_Fresh)"));
    }

    // spec: CFG-040
    #[test]
    fn front_matter_overrides_the_vault_default_for_numbered_headings() {
        let with = |v: &str| format!("---\nnumbered-headings: {v}\n---\n## A");
        for (v, want) in [("true", true), ("yes", true), ("false", false), ("no", false), ("\"No\"", false), ("TRUE", true)] {
            assert_eq!(headings_numbered(&with(v), !want), want, "{v} wins over the default");
        }
        assert!(headings_numbered("## A", true));
        assert!(!headings_numbered("## A", false));
        assert!(headings_numbered(&with("maybe"), true), "an unknown value is ignored");
        assert!(!headings_numbered("## A\nnumbered-headings: true", false), "only the front matter counts");
        assert!(!headings_numbered("---\ntags: x\n---\nnumbered-headings: true", false));
    }

    // spec: LINK-015
    #[test]
    fn a_heading_is_found_ignoring_case_spaces_and_markup() {
        let text = "# T\n## **Plan** v2\n## Plan v2\n## [[Note|Linked]] part\n```\n## Hidden\n```\n";
        assert_eq!(find_heading(text, "plan v2"), Some(1), "the first match wins");
        assert_eq!(find_heading(text, "  PLAN V2  "), Some(1));
        assert_eq!(find_heading(text, "Linked part"), Some(3));
        assert_eq!(find_heading(text, "Hidden"), None);
        assert_eq!(find_heading(text, "Nope"), None);
    }

    // spec: EDIT-050
    #[test]
    fn footnote_references_and_definitions_are_found_with_their_lines() {
        let text = "Text[^1] and[^note-2], again [^1].\n\n[^1]: The body[^3]\n    goes on\n\tand on\nnot indented\n[^note-2]: two\n[^3]: three\n";
        let f = find_footnotes(text);
        let refs: Vec<(usize, &str, Range<usize>)> = f.refs.iter().map(|r| (r.line, r.label.as_str(), r.range.clone())).collect();
        assert_eq!(refs, [(0, "1", 4..8), (0, "note-2", 12..21), (0, "1", 29..33), (2, "3", 14..18)]);
        let defs: Vec<(&str, Range<usize>, Range<usize>)> = f.defs.iter().map(|d| (d.label.as_str(), d.lines.clone(), d.marker.clone())).collect();
        assert_eq!(defs, [("1", 2..5, 0..5), ("note-2", 6..7, 0..10), ("3", 7..8, 0..5)]);
        assert_eq!(f.def("note-2").map(|d| d.lines.start), Some(6));
        assert_eq!(f.first_ref("1").map(|r| r.line), Some(0));
        assert!(f.first_ref("nobody").is_none() && f.def("nobody").is_none());
        assert_eq!(f.def_at(2).map(|d| d.label.as_str()), Some("1"));
        assert!(f.def_at(3).is_none());
        assert!(f.is_continuation(3) && f.is_continuation(4) && !f.is_continuation(2) && !f.is_continuation(5));
        assert_eq!(f.defined(), ["1", "note-2", "3"].iter().map(|s| s.to_string()).collect());
    }

    // spec: EDIT-050
    #[test]
    fn a_footnote_marker_at_the_start_of_a_line_is_a_definition_and_no_reference() {
        assert_eq!(footnote_def_marker("[^a]: x"), Some(("a".to_string(), 0..5)));
        assert_eq!(footnote_def_marker("[^Ä_-9]:x"), Some(("Ä_-9".to_string(), 0..9)));
        for no in ["[^a] x", " [^a]: x", "[^]: x", "[^a b]: x", "[a]: x", "[^a:"] {
            assert_eq!(footnote_def_marker(no), None, "{no}");
        }
        assert!(footnote_refs_in_line("[^a]: x").is_empty(), "the label of a definition is no reference");
        assert_eq!(footnote_refs_in_line(" [^a]: x").len(), 1, "indented, it is a reference");
        assert!(is_footnote_continuation("    x") && is_footnote_continuation("\tx"));
        assert!(!is_footnote_continuation("   x") && !is_footnote_continuation("    ") && !is_footnote_continuation("x"));
    }

    // spec: EDIT-050
    #[test]
    fn footnotes_in_code_front_matter_and_with_odd_labels_are_none() {
        for text in ["`a[^1]b`", "```\n[^1]\n[^1]: x\n```", "---\nk: [^1]\n---\n", "[^]", "[^a b]", "[^1", "[1]", "^[inline]"] {
            assert_eq!(find_footnotes(text), Footnotes::default(), "{text}");
        }
        // the code ends at its backticks; a reference after it is one
        assert_eq!(find_footnotes("`x` y[^1]").refs.len(), 1);
        // an indented line after a definition in fenced code is no continuation
        let f = find_footnotes("[^1]: a\n```\n    x\n```\n");
        assert_eq!(f.defs[0].lines, 0..1);
    }

    // spec: EDIT-050
    #[test]
    fn the_defined_labels_hold_while_their_guard_lives() {
        assert!(!footnote_is_defined("a"));
        let outer = DefinedFootnotes::set(["a".to_string()].into());
        assert!(footnote_is_defined("a") && defined_footnotes_in_scope().len() == 1);
        {
            let _inner = DefinedFootnotes::set(["b".to_string()].into());
            assert!(footnote_is_defined("b") && !footnote_is_defined("a"));
        }
        assert!(footnote_is_defined("a") && !footnote_is_defined("b"), "the outer labels come back");
        drop(outer);
        assert!(!footnote_is_defined("a"));
    }

    fn tex_of(text: &str) -> Vec<(String, bool, bool)> {
        find_math(text).into_iter().map(|m| (text[m.tex].to_string(), m.display, m.block)).collect()
    }

    // spec: LINK-001
    #[test]
    fn a_percent_encoded_hash_is_part_of_the_name_and_a_written_one_starts_the_heading() {
        let text = "[t](a%23b.md#h%20x)";
        let l = &find_links(text)[0];
        assert_eq!((l.target.as_str(), l.heading.as_deref()), ("a#b", Some("h x")));
        assert_eq!(written_target(text, l), "a#b.md");
        // a new target with a `#` is written escaped, and the link reads back with it (#538)
        let (r, new) = (&l.target_range, format!("{}{}{}", &text[..l.target_range.start], replace_target(text, l, "#"), &text[l.target_range.end..]));
        assert_eq!((new.as_str(), r.start), ("[t](%23#h%20x)", 4));
        assert_eq!(find_links(&new)[0].target, "#");
    }

    // spec: EDIT-046
    #[test]
    fn a_dollar_before_a_multibyte_character_is_no_math_and_no_panic() {
        // the scan for the closing `$` stepped over bytes and sliced inside the `é` (#538)
        for text in ["$é", "$é$$", "$$é", "$ü ü", "$$📅 x"] {
            let _ = find_math(text);
        }
        assert_eq!(find_math("$é$").len(), 1);
    }

    // spec: EDIT-046
    #[test]
    fn inline_math_follows_the_dollar_rules() {
        assert_eq!(tex_of("Let $x^2$ be, and $a+b$."), vec![("x^2".into(), false, false), ("a+b".into(), false, false)]);
        assert!(tex_of("it costs $5 and $10 today").is_empty(), "a price is no math");
        assert!(tex_of("$ x$ and $x $ end").is_empty(), "a space inside the dollars: no math");
        assert!(tex_of("a $$ b").is_empty(), "no closing dollars");
        assert!(tex_of("$x$5 is not math").is_empty(), "the closing dollar before a digit is no closing");
        assert_eq!(tex_of(r"pay \$5 and $x$"), vec![("x".into(), false, false)], "an escaped dollar is a dollar");
        assert_eq!(tex_of(r"$a\$b$"), vec![(r"a\$b".into(), false, false)], "an escaped dollar inside the formula stays in it");
        let t = "a $x$ b";
        let m = &find_math(t)[0];
        assert_eq!((&t[m.range.clone()], &t[m.tex.clone()], m.lines.clone()), ("$x$", "x", 0..1));
    }

    // spec: EDIT-046
    #[test]
    fn display_math_is_one_line_or_dollar_lines_or_a_math_fence() {
        assert_eq!(tex_of("$$x=1$$"), vec![("x=1".into(), true, true)], "alone on its line: a block");
        assert_eq!(tex_of("a $$x=1$$ b"), vec![("x=1".into(), true, false)], "in a line of text: display style, no block");
        let t = "a\n$$\nx = 1\ny = 2\n$$\nb";
        let m = find_math(t);
        assert_eq!(m.len(), 1);
        assert_eq!((&t[m[0].tex.clone()], m[0].display, m[0].block, m[0].lines.clone()), ("x = 1\ny = 2", true, true, 1..5));
        assert_eq!(&t[m[0].range.clone()], "$$\nx = 1\ny = 2\n$$");
        let f = "```math\n\\frac{a}{b}\n```\nafter $y$";
        let m = find_math(f);
        assert_eq!(m.len(), 2);
        assert_eq!((&f[m[0].tex.clone()], m[0].display, m[0].block, m[0].lines.clone()), ("\\frac{a}{b}", true, true, 0..3));
        assert_eq!(&f[m[1].tex.clone()], "y");
        assert!(tex_of("$$\nx\n").is_empty(), "no closing $$ line: no math");
        assert!(tex_of("$$ $$").is_empty(), "an empty formula is none");
    }

    // spec: EDIT-046
    #[test]
    fn math_in_code_and_front_matter_is_none() {
        assert!(tex_of("a `$x$` b").is_empty());
        assert_eq!(tex_of("`a` $x$ `b`"), vec![("x".into(), false, false)]);
        assert!(tex_of("```\n$x$\n$$\ny\n$$\n```\n").is_empty());
        assert!(tex_of("~~~\n$x$\n~~~").is_empty());
        assert!(tex_of("---\ntitle: $x$ and $y$\n---\ntext").is_empty());
        assert_eq!(tex_of("---\na: 1\n---\n$z$"), vec![("z".into(), false, false)]);
        assert!(tex_of("```rust\n$x$\n```").is_empty(), "other fences are code");
    }
}
