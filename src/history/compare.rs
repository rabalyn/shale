//! What the comparison view needs for two versions, made in the background job (HIST-007,
//! HIST-015): the rows of the rendered view, the side-by-side source and the patch for a text, the two
//! texts of a drawing, or the sizes of a binary file.

use super::diff::{self, Row, Side};
use crate::vault::FileKind;
use std::path::Path;

/// The two versions as the view shows them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Loaded {
    Text(Box<TextPair>),
    /// A drawing: shown as two thumbnails, not as text (HIST-015).
    Drawing {
        before: String,
        after: String,
    },
    /// Not text, or an LFS pointer: shown as "changed" with the sizes in bytes.
    Binary {
        before: usize,
        after: usize,
    },
}

/// The comparison of two texts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextPair {
    pub rows: Vec<Row>,
    pub side: Vec<Side>,
    pub patch: String,
    pub old: String,
    pub new: String,
}

impl Loaded {
    /// Compare two versions of the file at `path`; `binary` is what git said about them; `labels` name
    /// the two sides in the patch.
    pub fn of(path: &Path, binary: bool, old: Vec<u8>, new: Vec<u8>, labels: (&str, &str)) -> Loaded {
        let (before, after) = (old.len(), new.len());
        let (Ok(old), Ok(new), false) = (String::from_utf8(old), String::from_utf8(new), binary) else {
            return Loaded::Binary { before, after };
        };
        if crate::vault::kind_of(path) == FileKind::Drawing {
            return Loaded::Drawing { before: old, after: new };
        }
        Loaded::Text(Box::new(TextPair { rows: diff::compare(&old, &new), side: diff::side_by_side(&old, &new), patch: diff::patch(labels.0, labels.1, &old, &new), old, new }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: HIST-015
    #[test]
    fn hist_015_a_note_is_compared_as_text_a_drawing_and_a_binary_file_are_not() {
        let p = Path::new("Note.md");
        let t = Loaded::of(p, false, b"one\n".to_vec(), b"two\n".to_vec(), ("a", "b"));
        let Loaded::Text(t) = t else { panic!("text") };
        assert_eq!((t.old.as_str(), t.new.as_str()), ("one\n", "two\n"));
        assert!(t.patch.starts_with("--- a\n+++ b\n") && !t.rows.is_empty() && !t.side.is_empty());
        let d = Loaded::of(Path::new("Sketch.excalidraw"), false, b"{}".to_vec(), b"{\"a\":1}".to_vec(), ("a", "b"));
        assert_eq!(d, Loaded::Drawing { before: "{}".into(), after: "{\"a\":1}".into() });
        assert_eq!(Loaded::of(p, true, b"x".to_vec(), b"xyz".to_vec(), ("a", "b")), Loaded::Binary { before: 1, after: 3 }, "git says binary");
        assert_eq!(Loaded::of(p, false, vec![0xff, 0xfe], b"x".to_vec(), ("a", "b")), Loaded::Binary { before: 2, after: 1 }, "not UTF-8 text");
    }
}
