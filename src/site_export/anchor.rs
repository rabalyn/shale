//! The anchor Hugo makes for a heading (SITE-020): Goldmark's GitHub style, which Hugo uses by
//! default (`markup.goldmark.parser.autoHeadingIDType`). The text of the heading, trimmed and in
//! lower case, keeps its letters, digits, `_` and `-`, a space is a `-` and everything else is
//! dropped; an id that is taken gets `-1`, `-2`, … and a heading with nothing left is `heading`.

use crate::markdown;
use std::collections::HashSet;

/// A character Hugo keeps: a letter or a digit. Rust counts every character with a numeric value
/// (`½`, `²`) as a number; Go's `unicode.IsDigit` only the decimal digits, so those go.
fn kept(c: char) -> bool {
    if c.is_numeric() && !c.is_ascii_digit() {
        let non_decimal = matches!(c, '\u{00B2}' | '\u{00B3}' | '\u{00B9}' | '\u{00BC}'..='\u{00BE}' | '\u{2070}'..='\u{2079}' | '\u{2080}'..='\u{2089}' | '\u{2150}'..='\u{218F}' | '\u{2460}'..='\u{24FF}' | '\u{2776}'..='\u{2793}' | '\u{3007}' | '\u{3021}'..='\u{3029}' | '\u{3038}'..='\u{303A}' | '\u{3220}'..='\u{3229}' | '\u{3248}'..='\u{324F}' | '\u{3251}'..='\u{325F}' | '\u{3280}'..='\u{3289}' | '\u{32B1}'..='\u{32BF}');
        return !non_decimal;
    }
    c.is_alphabetic() || c.is_numeric()
}

/// `s` with the character references (`&amp;`, `&#38;`) read: Hugo makes the id of what the browser shows.
fn unescape(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at + 1..];
        let read = tail.split_once(';').filter(|(name, _)| name.len() <= 8).and_then(|(name, after)| {
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{a0}',
                n => char::from_u32(n.strip_prefix("#x").or_else(|| n.strip_prefix("#X")).map_or_else(|| n.strip_prefix('#')?.parse().ok(), |h| u32::from_str_radix(h, 16).ok())?)?,
            };
            Some((c, after))
        });
        match read {
            Some((c, after)) => {
                out.push(c);
                rest = after;
            }
            None => {
                out.push('&');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The id of a heading with this text, before a taken id gets its number.
fn base_id(plain: &str) -> String {
    let mut out = String::new();
    for c in unescape(plain).trim().chars().flat_map(char::to_lowercase) {
        if c.is_whitespace() {
            out.push('-');
        } else if c == '_' || c == '-' || kept(c) {
            out.push(c);
        }
    }
    if out.is_empty() {
        "heading".into()
    } else {
        out
    }
}

/// A heading of a note: its plain text (lower case, for finding it, LINK-015) and its id.
#[derive(Debug, PartialEq, Eq)]
pub struct Anchor {
    pub plain: String,
    pub id: String,
}

/// The ids of the headings of a note's text, in order, as Hugo numbers them.
pub fn anchors(text: &str) -> Vec<Anchor> {
    let mut taken = HashSet::new();
    markdown::find_headings(text)
        .into_iter()
        // a heading with no text has no id at all
        .filter(|(_, _, title)| !title.trim().is_empty())
        .map(|(_, _, title)| {
            let plain = markdown::heading_plain(&title);
            let base = base_id(&plain);
            let mut id = base.clone();
            let mut n = 1;
            while !taken.insert(id.clone()) {
                id = format!("{base}-{n}");
                n += 1;
            }
            Anchor { plain: plain.to_lowercase(), id }
        })
        .collect()
}

/// The id of the first of `anchors` that reads `heading` (ignoring case, spaces and inline markup,
/// LINK-015); `None` when the note has no such heading.
pub fn id_in(anchors: &[Anchor], heading: &str) -> Option<String> {
    let want = markdown::heading_plain(heading).to_lowercase();
    anchors.iter().find(|a| a.plain == want).map(|a| a.id.clone())
}
