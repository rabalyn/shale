//! Setting and removing blockers from the board (BOARD-041, BOARD-042): what `blocked-by:` of the
//! waiting note's front matter becomes when a blocker is added or taken away. The text of every
//! other line, and of the other blockers, stays as it is: the line is changed by the entry, not
//! written again. The window carries the new text out through the checked save of the note
//! (`app/boards.rs`), so it is one undo step and `modified:` stays, as with a drop (BOARD-020).

use super::drag::pin_text;
use crate::note_props;
use crate::search_all::deps::{entries, line_value, name_key, property_lines};
use crate::vault::Vault;
use std::path::Path;

/// The key the notes name `card` by in a `blocked-by:` line: its file name without `.md`, lower case.
pub fn key(vault: &Vault, card: &Path) -> Option<String> {
    vault.file(card).map(|f| name_key(&f.display_name()))
}

/// The entry that names `blocker` in the `blocked-by:` of `waiting` (BOARD-041): `[[Name]]` when the
/// name finds the note from there and `[[ ]]` can hold it, else `[[Folder/Name]]`, else the markdown
/// link with the vault path for a name with `#`, `^`, `|` or brackets (BOARD-004, #637).
pub fn link(vault: &Vault, waiting: &Path, blocker: &Path) -> String {
    pin_text(vault, waiting, blocker)
}

/// `line` with `value` after the key (`key: value`, or `key:` for none), the line end kept.
fn with_value(line: &str, value: &str) -> String {
    let eol = &line[line.trim_end_matches(['\r', '\n']).len()..];
    let key = line.split_once(':').map_or(line, |(k, _)| k);
    if value.is_empty() {
        format!("{key}:{eol}")
    } else {
        format!("{key}: {value}{eol}")
    }
}

/// A list line whose item text is `value` (a slice of `line`) with `new` in its place.
fn with_item(line: &str, value: &str, new: &str) -> String {
    let at = value.as_ptr() as usize - line.as_ptr() as usize;
    format!("{}{new}{}", &line[..at], &line[at + value.len()..])
}

/// Is `value` a YAML flow list, `["[[A]]", "[[B]]"]`, and not a value that starts with a link?
fn is_flow(value: &str) -> bool {
    value.starts_with('[') && value.ends_with(']') && !value.starts_with("[[") && (value[1..].trim_start().starts_with(['"', '\'']) || !value.contains("]("))
}

/// The value `value` with the entry `link` after the others.
fn appended(value: &str, link: &str) -> String {
    if is_flow(value) {
        let quoted = format!("\"{}\"", link.replace('\\', "\\\\").replace('"', "\\\""));
        let inner = value[1..value.len() - 1].trim_end();
        return if inner.trim().is_empty() { format!("[{quoted}]") } else { format!("[{inner}, {quoted}]") };
    }
    format!("{value}, {link}")
}

/// `text` with the blocker `link` (whose key is `key`) added to its `blocked-by:`; none when the note
/// names it already (BOARD-043). A note without the property gets the line at the end of its front
/// matter (and a front matter, when it has none); an inline list gets `, link`, a list of `- ` lines
/// one more line like the last, a flow list one more quoted item.
pub fn add(text: &str, link: &str, key: &str) -> Option<String> {
    let Some((first, count)) = property_lines(text) else { return Some(note_props::set_property(text, "blocked-by", link)) };
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let own = &lines[first..first + count];
    if own.iter().enumerate().any(|(i, l)| entries(line_value(l, i == 0)).iter().any(|e| e.key == key)) {
        return None;
    }
    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    let head = lines[first];
    let value = line_value(head, true);
    if !value.is_empty() {
        out[first] = with_value(head, &appended(value, link));
    } else if count > 1 {
        let last = lines[first + count - 1];
        let indent = &last[..last.len() - last.trim_start().len()];
        // (a line of the front matter is followed by the closing `---`: it has its line end)
        let eol = &last[last.trim_end_matches(['\r', '\n']).len()..];
        out.insert(first + count, format!("{indent}- {link}{eol}"));
    } else {
        out[first] = with_value(head, link);
    }
    Some(out.concat())
}

/// `value` without the entries named `key`, with the comma that joined them to the others and the
/// quotes around them.
fn cut(value: &str, key: &str) -> String {
    let mut v = value.to_string();
    let named: Vec<_> = entries(value).into_iter().filter(|e| e.key == key).collect();
    for e in named.iter().rev() {
        let (mut s, mut end) = (e.range.start, e.range.end);
        let (before, after) = (v[..s].chars().next_back(), v[end..].chars().next());
        if matches!(before, Some('"' | '\'')) && before == after {
            s -= 1;
            end += 1;
        }
        let tail = &v[end..];
        let gap = tail.len() - tail.trim_start().len();
        if tail[gap..].starts_with(',') {
            let comma = &tail[gap + 1..];
            end += gap + 1 + (comma.len() - comma.trim_start().len());
        } else if let Some(comma) = v[..s].trim_end().strip_suffix(',') {
            s = comma.len();
        }
        v.replace_range(s..end, "");
    }
    v
}

/// `text` without the blocker `key` in its `blocked-by:`; none when it names none such. The other
/// blockers keep their text. When it was the last one the whole property goes, its list lines
/// with it (BOARD-042).
pub fn remove(text: &str, key: &str) -> Option<String> {
    let (first, count) = property_lines(text)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let (mut found, mut left) = (false, 0);
    let mut own = Vec::new();
    for (n, l) in lines[first..first + count].iter().enumerate() {
        let value = line_value(l, n == 0);
        if !entries(value).iter().any(|e| e.key == key) {
            left += entries(value).len();
            own.push(l.to_string());
            continue;
        }
        found = true;
        let new = cut(value, key);
        let rest = entries(&new).len();
        left += rest;
        if n == 0 {
            own.push(with_value(l, &new));
        } else if rest > 0 {
            own.push(with_item(l, value, &new));
        }
    }
    if !found {
        return None;
    }
    if left == 0 {
        return Some(note_props::remove_property(text, "blocked-by"));
    }
    let mut out: Vec<String> = lines[..first].iter().map(|l| l.to_string()).collect();
    out.extend(own);
    out.extend(lines[first + count..].iter().map(|l| l.to_string()));
    Some(out.concat())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search_all::deps::blocked_by;

    fn note(fm: &str) -> String {
        format!("---\n{fm}\n---\nbody\n")
    }

    // spec: BOARD-041
    #[test]
    fn board_041_a_blocker_is_added_in_the_form_the_line_has() {
        let a = |fm: &str| add(&note(fm), "[[New]]", "new").unwrap();
        assert_eq!(a("title: X"), note("title: X\nblocked-by: [[New]]"));
        assert_eq!(a("blocked-by: [[A]]"), note("blocked-by: [[A]], [[New]]"));
        assert_eq!(a("blocked-by: [[A]], [[B]]\ndue: 2030-01-01"), note("blocked-by: [[A]], [[B]], [[New]]\ndue: 2030-01-01"));
        assert_eq!(a("blocked-by:\n  - [[A]]\n  - [[B]]\ndue: 2030-01-01"), note("blocked-by:\n  - [[A]]\n  - [[B]]\n  - [[New]]\ndue: 2030-01-01"));
        assert_eq!(a("blocked-by:\n- [[A]]"), note("blocked-by:\n- [[A]]\n- [[New]]"));
        assert_eq!(a("blocked-by: [\"[[A]]\"]"), note("blocked-by: [\"[[A]]\", \"[[New]]\"]"));
        assert_eq!(a("blocked-by: []"), note("blocked-by: [\"[[New]]\"]"));
        assert_eq!(a("blocked-by:"), note("blocked-by: [[New]]"));
        assert_eq!(a("blocked-by: [X](<X.md>)"), note("blocked-by: [X](<X.md>), [[New]]"));
        // a note without a front matter gets one; the body is the same
        assert_eq!(add("# Note\n", "[[New]]", "new").unwrap(), "---\nblocked-by: [[New]]\n---\n# Note\n");
        // Windows line ends stay
        assert_eq!(add("---\r\nblocked-by:\r\n  - [[A]]\r\n---\r\nx\r\n", "[[N]]", "n").unwrap(), "---\r\nblocked-by:\r\n  - [[A]]\r\n  - [[N]]\r\n---\r\nx\r\n");
        assert_eq!(add("---\r\nblocked-by: [[A]]\r\n---\r\n", "[[N]]", "n").unwrap(), "---\r\nblocked-by: [[A]], [[N]]\r\n---\r\n");
    }

    // spec: BOARD-043
    #[test]
    fn board_043_a_blocker_that_is_there_is_not_added_twice_in_any_form() {
        for fm in ["blocked-by: [[New]]", "blocked-by: [[A]], [[Folder/New#h|x]]", "blocked-by:\n  - [[A]]\n  - \"[[new]]\"", "blocked-by: [\"[[New]]\"]", "blocked-by: [New](<Dir/New.md>)", "blocked-by: New"] {
            assert_eq!(add(&note(fm), "[[New]]", "new"), None, "{fm}");
        }
    }

    // spec: BOARD-042
    #[test]
    fn board_042_a_blocker_is_removed_and_the_others_keep_their_text() {
        let r = |fm: &str, key: &str| remove(&note(fm), key).unwrap();
        assert_eq!(r("blocked-by: [[A]], [[B]], [[C]]", "b"), note("blocked-by: [[A]], [[C]]"));
        assert_eq!(r("blocked-by: [[A]], [[B]]", "a"), note("blocked-by: [[B]]"));
        assert_eq!(r("blocked-by: [[A]],[[B|x]]", "b"), note("blocked-by: [[A]]"));
        assert_eq!(r("blocked-by:\n  - [[A]]\n  - [[B]]\ndue: 2030-01-01", "a"), note("blocked-by:\n  - [[B]]\ndue: 2030-01-01"));
        assert_eq!(r("blocked-by: [\"[[A]]\", \"[[B]]\"]", "b"), note("blocked-by: [\"[[A]]\"]"));
        assert_eq!(r("blocked-by: [\"[[A]]\", \"[[B]]\"]", "a"), note("blocked-by: [\"[[B]]\"]"));
        assert_eq!(r("blocked-by: [[A]], [B #1](<B%20%231.md>), [[C]]", "b #1"), note("blocked-by: [[A]], [[C]]"));
        assert_eq!(r("blocked-by: A, B", "a"), note("blocked-by: B"));
        // the last one takes the line, and the list lines under it, with it
        assert_eq!(r("title: X\nblocked-by: [[A]]\ndue: 2030-01-01", "a"), note("title: X\ndue: 2030-01-01"));
        assert_eq!(r("blocked-by:\n  - [[A]]\ndue: 2030-01-01", "a"), note("due: 2030-01-01"));
        assert_eq!(remove(&note("blocked-by: [\"[[A]]\"]"), "a").unwrap(), "---\n---\nbody\n", "an empty front matter stays");
        // a list line gone while the key line is left for the others
        assert_eq!(r("blocked-by:\n  - [[A]]\n  - [[B]]", "a"), note("blocked-by:\n  - [[B]]"));
        // not named: nothing to do
        assert_eq!(remove(&note("blocked-by: [[A]]"), "z"), None);
        assert_eq!(remove(&note("title: X"), "a"), None);
        assert_eq!(remove("# plain\n", "a"), None);
    }

    // spec: BOARD-041, BOARD-042, QUERY-002
    #[test]
    fn board_041_what_is_written_is_read_back_as_the_same_blockers() {
        let mut text = note("blocked-by: [[A]]");
        for (link, key) in [("[[Folder/B]]", "b"), ("[C #1](<Sub/C%20%231.md>)", "c #1"), ("[[D, E]]", "d, e"), ("[100% (x)](<100%25 %28x%29.md>)", "100% (x)")] {
            text = add(&text, link, key).unwrap();
        }
        assert_eq!(blocked_by(&text), ["a", "b", "c #1", "d, e", "100% (x)"]);
        let text = remove(&text, "c #1").unwrap();
        assert_eq!(blocked_by(&text), ["a", "b", "d, e", "100% (x)"]);
    }
}
