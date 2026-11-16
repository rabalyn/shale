//! The caption under an embedded picture, drawing, PDF, video or sound (EDIT-025, PDF-004): the
//! editor and the PDF export share what the caption says, how big it is and how it is shortened
//! to the room it has. Each draws it with its own tools (egui, PDF text).

use crate::markdown::LinkSpan;

/// The caption's text size, as a part of the note's text size.
const SIZE_FACTOR: f32 = 0.8;
/// The space between the picture and its caption, in points.
pub const GAP: f32 = 3.0;

/// The caption's text size for a note text size of `base`.
pub fn size(base: f32) -> f32 {
    (base * SIZE_FACTOR).max(8.0)
}

/// The room a caption takes below a picture: its gap and one line.
pub fn height(base: f32) -> f32 {
    GAP + (size(base) * 1.3).ceil()
}

/// What the caption says: the words of the link's alias (`![[a.png|My picture]]`, a width after
/// a last `|` is not one), else the link's target as written (`z_media/2026/a.png`, with the
/// heading of `![[doc.pdf#page=2]]`).
pub fn text(link: &LinkSpan) -> String {
    if let Some(alias) = &link.alias {
        let mut parts: Vec<&str> = alias.split('|').collect();
        if link.embed_width().is_some() {
            parts.pop();
        }
        let words = parts.join("|");
        if !words.trim().is_empty() {
            return words.trim().to_string();
        }
    }
    LinkSpan { alias: None, ..link.clone() }.display()
}

/// `text` shortened with an ellipsis so that it is at most `avail` wide, as `width` measures it.
pub fn fit(text: &str, avail: f32, width: impl Fn(&str) -> f32) -> String {
    if width(text) <= avail {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let shown = |n: usize| format!("{}…", chars[..n].iter().collect::<String>().trim_end());
    // the most characters that still fit (the width grows with the count)
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if width(&shown(mid)) <= avail {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    shown(lo)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::find_links;

    fn cap(t: &str) -> String {
        text(&find_links(t).remove(0))
    }

    // spec: EDIT-025
    #[test]
    fn the_caption_is_the_alias_words_else_the_link_target() {
        assert_eq!(cap("![[z_media/a.png]]"), "z_media/a.png");
        assert_eq!(cap("![[a.png|300]]"), "a.png", "a width is no caption");
        assert_eq!(cap("![[a.png|300x200]]"), "a.png");
        assert_eq!(cap("![[a.png|My picture]]"), "My picture");
        assert_eq!(cap("![[a.png|My picture|300]]"), "My picture", "the width goes, the words stay");
        assert_eq!(cap("![[a.png|a|b]]"), "a|b");
        assert_eq!(cap("![[doc.pdf#page=2]]"), "doc.pdf > page=2");
        assert_eq!(cap("![[a.png|2024]]"), "a.png", "a number is a width");
    }

    // spec: EDIT-025
    #[test]
    fn a_caption_is_shortened_with_an_ellipsis_to_the_room_it_has() {
        let w = |s: &str| s.chars().count() as f32 * 10.0;
        assert_eq!(fit("short.png", 100.0, w), "short.png");
        assert_eq!(fit("a-long-name.png", 100.0, w), "a-long-na…");
        assert_eq!(fit("abc", 5.0, w), "…", "nothing fits but the dots");
        assert!(w(&fit("a-long-name.png", 55.0, w)) <= 55.0);
        assert_eq!(fit("", 0.0, w), "");
    }

    // spec: EDIT-025
    #[test]
    fn the_caption_is_smaller_than_the_text_and_has_room_for_one_line() {
        assert!(size(15.0) < 15.0 && size(5.0) >= 8.0);
        assert!(height(15.0) > size(15.0) + GAP);
    }
}
