//! Coloured highlight and font colour as inline HTML (EDIT-027): `<mark style="background: #FFFF00;">`
//! and `<span style="color: #FF0000;">`, the convention of Obsidian's colour plugins. Reading is
//! tolerant, writing is always the canonical form. No UI; the editor, the reading view, tables and
//! the PDF export find the colours here (EDIT-028).

use std::ops::Range;

/// What a tag colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `<mark style="background: …">`: the background of the text
    Mark,
    /// `<span style="color: …">`: the colour of the text
    Font,
}

/// A coloured piece of a line: the opening tag, the closing tag, and the colour (RGBA).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub kind: Kind,
    pub open: Range<usize>,
    pub close: Range<usize>,
    pub rgba: [u8; 4],
}

impl Run {
    /// The coloured text between the tags.
    pub fn inner(&self) -> Range<usize> {
        self.open.end..self.close.start
    }
}

/// An RGB colour as the canonical `#RRGGBB`.
pub fn hex(rgb: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2])
}

/// The canonical opening tag of a coloured highlight.
pub fn mark_open(rgb: [u8; 3]) -> String {
    format!("<mark style=\"background: {};\">", hex(rgb))
}

/// The canonical opening tag of a font colour.
pub fn font_open(rgb: [u8; 3]) -> String {
    format!("<span style=\"color: {};\">", hex(rgb))
}

pub const MARK_CLOSE: &str = "</mark>";
pub const FONT_CLOSE: &str = "</span>";

/// `#RGB`, `#RRGGBB` or `#RRGGBBAA` (any case).
pub fn parse_hex(s: &str) -> Option<[u8; 4]> {
    let h = s.trim().strip_prefix('#')?;
    if !h.is_ascii() || !h.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = |i: usize, n: usize| u8::from_str_radix(&h[i..i + n], 16).ok();
    match h.len() {
        3 => Some([v(0, 1)? * 17, v(1, 1)? * 17, v(2, 1)? * 17, 255]),
        6 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, 255]),
        8 => Some([v(0, 2)?, v(2, 2)?, v(4, 2)?, v(6, 2)?]),
        _ => None,
    }
}

/// Whether text on this background is best drawn dark (a bright background) or light.
pub fn dark_text_on(bg: [u8; 4]) -> bool {
    let lum = 0.299 * bg[0] as f32 + 0.587 * bg[1] as f32 + 0.114 * bg[2] as f32;
    lum > 140.0
}

/// A parsed opening tag: what it is, its length, and its colour if the style has one.
struct Open {
    kind: Kind,
    len: usize,
    rgba: Option<[u8; 4]>,
}

fn style_colour(style: &str, props: &[&str]) -> Option<[u8; 4]> {
    style.split(';').find_map(|decl| {
        let (name, value) = decl.split_once(':')?;
        props.iter().any(|p| name.trim().eq_ignore_ascii_case(p)).then(|| parse_hex(value))?
    })
}

/// The opening tag `<mark …>` or `<span …>` at the start of `s`.
fn parse_open(s: &str) -> Option<Open> {
    let b = s.as_bytes();
    if b.first() != Some(&b'<') || b.len() < 6 {
        return None;
    }
    let name = s.get(1..5).unwrap_or("");
    let (kind, props): (Kind, &[&str]) = if name.eq_ignore_ascii_case("mark") {
        (Kind::Mark, &["background", "background-color"])
    } else if name.eq_ignore_ascii_case("span") {
        (Kind::Font, &["color"])
    } else {
        return None;
    };
    // the end of the tag: the first `>` outside quotes
    let mut quote = None;
    let mut end = None;
    for (i, c) in s.char_indices().skip(5) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => {
                end = Some(i);
                break;
            }
            (None, '<') => return None,
            _ => {}
        }
    }
    let end = end?;
    let attrs = &s[5..end];
    if !attrs.is_empty() && !attrs.starts_with(char::is_whitespace) {
        return None;
    }
    let mut rgba = None;
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(p) = lower[from..].find("style") {
        let at = from + p;
        from = at + 5;
        if at > 0 && !attrs.as_bytes()[at - 1].is_ascii_whitespace() {
            continue;
        }
        let rest = attrs[at + 5..].trim_start();
        let Some(rest) = rest.strip_prefix('=') else { continue };
        let rest = rest.trim_start();
        let Some(q) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else { continue };
        let value = &rest[1..];
        let Some(close) = value.find(q) else { continue };
        rgba = style_colour(&value[..close], props);
        break;
    }
    Some(Open { kind, len: end + 1, rgba })
}

/// The closing tag `</mark>` or `</span>` at the start of `s`: its kind and length.
fn parse_close(s: &str) -> Option<(Kind, usize)> {
    let b = s.as_bytes();
    if b.len() < 7 || b[0] != b'<' || b[1] != b'/' {
        return None;
    }
    let name = s.get(2..6).unwrap_or("");
    let kind = if name.eq_ignore_ascii_case("mark") {
        Kind::Mark
    } else if name.eq_ignore_ascii_case("span") {
        Kind::Font
    } else {
        return None;
    };
    let rest = &s[6..];
    let blanks = rest.len() - rest.trim_start().len();
    (rest[blanks..].starts_with('>')).then_some((kind, 6 + blanks + 1))
}

/// The coloured runs of one line, in the order their closing tags come (so inner ones come before the
/// ones around them). Tags in inline code, a tag without its partner and a tag with a style that
/// has no colour are left alone.
pub fn find(line: &str) -> Vec<Run> {
    if !line.contains('<') {
        return Vec::new();
    }
    let code = crate::markdown::inline_code_ranges(line);
    let mut stack: Vec<(Kind, Range<usize>, Option<[u8; 4]>)> = Vec::new();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = line[i..].find('<') {
        let at = i + p;
        i = at + 1;
        if code.iter().any(|r| r.contains(&at)) {
            continue;
        }
        if let Some(o) = parse_open(&line[at..]) {
            stack.push((o.kind, at..at + o.len, o.rgba));
            i = at + o.len;
        } else if let Some((kind, len)) = parse_close(&line[at..]) {
            if let Some(n) = stack.iter().rposition(|(k, ..)| *k == kind) {
                let (_, open, rgba) = stack.remove(n);
                // tags opened after this one and not closed yet are broken: drop them
                stack.truncate(n);
                if let Some(rgba) = rgba {
                    out.push(Run { kind, open, close: at..at + len, rgba });
                }
            }
            i = at + len;
        }
    }
    out
}

/// The opening tags of marks and spans in a line (also those without a closing tag or a colour): the
/// colour written in them is no `#tag`.
pub fn open_tags(line: &str) -> Vec<Range<usize>> {
    if !line.contains('<') {
        return Vec::new();
    }
    let code = crate::markdown::inline_code_ranges(line);
    line.match_indices('<').filter(|(at, _)| !code.iter().any(|r| r.contains(at))).filter_map(|(at, _)| parse_open(&line[at..]).map(|o| at..at + o.len)).collect()
}

/// The colour (background, font) that applies at byte `at` of a line: the innermost run of each kind.
pub fn colours_at(runs: &[Run], at: usize) -> (Option<[u8; 4]>, Option<[u8; 4]>) {
    let innermost = |kind: Kind| runs.iter().filter(|r| r.kind == kind && r.inner().contains(&at)).min_by_key(|r| r.inner().len()).map(|r| r.rgba);
    (innermost(Kind::Mark), innermost(Kind::Font))
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: EDIT-027
    #[test]
    fn the_canonical_tags_are_written_in_upper_case_hex() {
        assert_eq!(mark_open([255, 255, 0]), "<mark style=\"background: #FFFF00;\">");
        assert_eq!(font_open([0x39, 0xff, 0x14]), "<span style=\"color: #39FF14;\">");
        assert_eq!(MARK_CLOSE, "</mark>");
    }

    // spec: EDIT-027
    #[test]
    fn hex_colours_come_in_three_six_and_eight_digits() {
        assert_eq!(parse_hex("#fa0"), Some([255, 170, 0, 255]));
        assert_eq!(parse_hex(" #FFFF00 "), Some([255, 255, 0, 255]));
        assert_eq!(parse_hex("#11223380"), Some([0x11, 0x22, 0x33, 0x80]));
        for bad in ["FFFF00", "#FFFF0", "#GGGGGG", "#", "red", "#ééé"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    // spec: EDIT-027
    #[test]
    fn tolerant_variants_are_read() {
        for tag in [
            "<mark style=\"background: #FFFF00;\">x</mark>",
            "<mark style='background:#ffff00'>x</mark>",
            "<mark style=\"background-color: #FF0;\">x</mark>",
            "<MARK  style = \"  Background-Color :  #FFFF00 ; \" >x</MARK >",
            "<mark class=\"a\" style=\"background: #FFFF00FF\">x</mark>",
        ] {
            let runs = find(tag);
            assert_eq!(runs.len(), 1, "{tag}");
            assert_eq!((runs[0].kind, runs[0].rgba), (Kind::Mark, [255, 255, 0, 255]), "{tag}");
            assert_eq!(&tag[runs[0].inner()], "x", "{tag}");
        }
        let runs = find("<span style=\"color:#f00\">r</span> and <span style='color: #00FF0080;'>g</span>");
        assert_eq!(runs.iter().map(|r| (r.kind, r.rgba)).collect::<Vec<_>>(), vec![(Kind::Font, [255, 0, 0, 255]), (Kind::Font, [0, 255, 0, 128])]);
    }

    // spec: EDIT-027
    #[test]
    fn a_span_takes_color_but_no_background_and_a_mark_the_other_way_round() {
        assert!(find("<span style=\"background: #FFFF00;\">x</span>").is_empty());
        assert!(find("<mark style=\"color: #FF0000;\">x</mark>").is_empty());
        assert!(find("<span style=\"background-color: #FFFF00; color: #123456\">x</span>").iter().all(|r| r.rgba == [0x12, 0x34, 0x56, 255]));
    }

    // spec: EDIT-027, EDIT-028
    #[test]
    fn nesting_and_mixing_find_each_tag_pair() {
        let line = "<mark style=\"background: #FFFF00;\">a <span style=\"color: #FF0000;\">b</span> c</mark>";
        let runs = find(line);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].kind, Kind::Font, "the inner run closes first");
        assert_eq!(&line[runs[0].inner()], "b");
        assert_eq!(&line[runs[1].inner()], "a <span style=\"color: #FF0000;\">b</span> c");
        let b = line.find(">b<").unwrap() + 1;
        assert_eq!(colours_at(&runs, b), (Some([255, 255, 0, 255]), Some([255, 0, 0, 255])));
        assert_eq!(colours_at(&runs, 0), (None, None));
        // two marks inside each other: the inner colour wins
        let line = "<mark style=\"background: #111\"><mark style=\"background: #222\">x</mark>y</mark>";
        let runs = find(line);
        let (x, y) = (line.find(">x<").unwrap() + 1, line.find(">y<").unwrap() + 1);
        assert_eq!(colours_at(&runs, x).0, Some([0x22, 0x22, 0x22, 255]));
        assert_eq!(colours_at(&runs, y).0, Some([0x11, 0x11, 0x11, 255]));
    }

    // spec: EDIT-027, EDIT-028
    #[test]
    fn garbled_unclosed_and_code_tags_are_plain_text() {
        for line in [
            "<mark style=\"background: #FFFF00;\">never closed",
            "closed never</mark>",
            "<mark style=\"background: #FFFF00;>x</mark>",
            "<mark style=\"background: banana;\">x</mark>",
            "<mark",
            "<",
            "<span style=\"color: #F00\"",
            "`<mark style=\"background: #FFFF00;\">`x</mark>",
            "<markdown style=\"background: #FFFF00;\">x</markdown>",
            "<mark style=\"background: #FFFF00\" <b>x</mark>",
            "é<ménage>",
            "<marö style=\"background: #FFF\">x</marö>",
            "<mar\u{f6}x>",
            "</mar\u{f6}>",
            "</mark\u{f6}",
        ] {
            assert!(find(line).is_empty(), "{line}");
        }
        // a stray opening tag does not take the pair after it along
        let runs = find("<mark style=\"background: #111\">a <mark style=\"background: #222\">b</mark>");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].rgba[0], 0x22);
    }

    // spec: EDIT-028
    #[test]
    fn text_on_a_highlight_is_dark_on_bright_and_light_on_dark_backgrounds() {
        assert!(dark_text_on([255, 255, 0, 255]));
        assert!(dark_text_on([0, 255, 255, 255]));
        assert!(!dark_text_on([0, 0, 128, 255]));
        assert!(!dark_text_on([255, 0, 255, 255]));
    }
}
