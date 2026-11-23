//! What the editor works out from the whole text of a note and keeps while nothing it depends on
//! changes (EDIT-104…EDIT-106): the parse of the text (footnotes, tables) and the laid-out note
//! (its job, its galley and the marks painted over it). A frame of a note of 20 000 lines does
//! none of this again for a blinking cursor or a mouse that moves.

use crate::editor::{FormatCache, MathMark, NumberMark};
use crate::fonts::EditorFonts;
use crate::table::Block;
use eframe::egui::Galley;
use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What is found in one version of the text, each part when it is first asked for.
struct Parsed {
    text: String,
    defined: Option<Arc<HashSet<String>>>,
    tables: Option<Arc<Vec<Block>>>,
    web_links: Option<Arc<Vec<crate::lists::WebLink>>>,
}

/// The parse of the last text it was asked about (EDIT-104). The text is compared with the copy it
/// keeps, which is quicker than any hash of it.
#[derive(Default)]
pub struct Parse {
    last: Option<Parsed>,
}

impl Parse {
    fn of(&mut self, text: &str) -> &mut Parsed {
        if self.last.as_ref().is_none_or(|p| p.text != text) {
            self.last = Some(Parsed { text: text.to_string(), defined: None, tables: None, web_links: None });
        }
        self.last.as_mut().expect("just set")
    }

    /// The labels of the footnotes `text` defines.
    pub fn defined(&mut self, text: &str) -> Arc<HashSet<String>> {
        Arc::clone(self.of(text).defined.get_or_insert_with(|| Arc::new(crate::markdown::find_footnotes(text).defined())))
    }

    /// The web links of `text`.
    pub fn web_links(&mut self, text: &str) -> Arc<Vec<crate::lists::WebLink>> {
        Arc::clone(self.of(text).web_links.get_or_insert_with(|| Arc::new(crate::lists::find_web_links(text))))
    }

    /// The tables of `text`.
    pub fn tables(&mut self, text: &str) -> Arc<Vec<Block>> {
        Arc::clone(self.of(text).tables.get_or_insert_with(|| Arc::new(crate::table::find_tables(text))))
    }
}

/// Everything the layout of the note depends on (EDIT-105). The cursor is not in it, only the raw
/// lines it makes (EDIT-017); nor is anything that only moves the pointer or blinks.
#[derive(PartialEq)]
pub struct LayoutKey {
    pub text: String,
    /// the vault's generation and the note: which links resolve
    pub links: u64,
    pub fonts: EditorFonts,
    /// the fonts of egui ([`crate::fonts::settled`])
    pub font_set: u64,
    pub pixels_per_point: u32,
    pub wrap_width: u32,
    pub numbered_default: bool,
    /// the footnotes the note being drawn has (the scope of [`crate::markdown::DefinedFootnotes`])
    pub scope: HashSet<String>,
    /// the raw lines of the preview, none when it is off
    pub raw: Option<Range<usize>>,
    /// the rows the embeds, tables and folds change: byte range and height
    pub ranges: Vec<(Range<usize>, f32)>,
    /// [`crate::syntax::generation`]: a layout with plain code is made again when the worker has coloured some (EDIT-121)
    pub syntax_gen: u64,
}

/// A laid-out note and what is painted over it.
#[derive(Clone)]
pub struct Laid {
    pub galley: Arc<Galley>,
    pub formulas: Arc<Vec<MathMark>>,
    pub numbers: Arc<Vec<NumberMark>>,
}

/// The layout of the last frame, with the key it was made for.
#[derive(Default)]
pub struct LayoutCache {
    last: Option<(LayoutKey, Laid)>,
    /// the formatted blocks of the text (EDIT-109): a layout that is built again does not format all of the text again
    formats: FormatCache,
    /// how many times the job was built and laid out, for the tests
    #[cfg(test)]
    pub builds: u32,
}

impl LayoutCache {
    /// The layout for `key`: the one kept, or the one `build` makes (and keeps). `keep` is false in
    /// the pass that sets the fonts of egui, when nothing laid out is to be kept.
    pub fn get(&mut self, key: LayoutKey, keep: bool, build: impl FnOnce(&LayoutKey, &mut FormatCache) -> Laid) -> Laid {
        if let Some((k, laid)) = &self.last {
            if keep && *k == key {
                return laid.clone();
            }
        }
        #[cfg(test)]
        {
            self.builds += 1;
        }
        let laid = build(&key, &mut self.formats);
        self.last = keep.then(|| (key, laid.clone()));
        laid
    }
}

/// What the status bar says of the open note (EDIT-107): the counts of the text, which are not
/// worked out again for a text that did not change. The backlinks and open tasks of the vault are in
/// [`crate::vault_cache::VaultCache`].
#[derive(Default)]
pub struct StatusCache {
    /// the note, the text's length and fast hash, then its characters and words
    counts: Option<(PathBuf, usize, u64, usize, usize)>,
}

impl StatusCache {
    /// The characters and words of the note `rel` with `text`.
    pub fn counts(&mut self, rel: &Path, text: &str) -> (usize, usize) {
        let hash = crate::fold_ui::hash_of(text);
        match &self.counts {
            Some((r, len, h, chars, words)) if r == rel && *len == text.len() && *h == hash => (*chars, *words),
            _ => {
                let counts = (text.chars().count(), text.split_whitespace().count());
                self.counts = Some((rel.to_path_buf(), text.len(), hash, counts.0, counts.1));
                counts
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: EDIT-104
    #[test]
    fn edit_104_an_unchanged_text_is_parsed_once_and_any_change_of_it_parses_again() {
        let mut parse = Parse::default();
        let text = "Note[^a]\n\n[^a]: defined\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\nsee https://example.org/x\n";
        let (a, b) = (parse.tables(text), parse.tables(text));
        assert!(Arc::ptr_eq(&a, &b), "the same text finds its tables once");
        assert_eq!(a.len(), 1);
        assert!(parse.defined(text).contains("a"));
        assert_eq!(parse.web_links(text).len(), 1);
        assert!(Arc::ptr_eq(&a, &parse.tables(text)), "the other parts do not parse the text again");
        // the same length, one byte different
        let edited = text.replace("| 1 |", "| 7 |");
        assert_eq!(edited.len(), text.len());
        let c = parse.tables(&edited);
        assert!(!Arc::ptr_eq(&a, &c));
        assert_eq!(c[0].table.rows[0][0], "7");
        assert!(parse.web_links("no link").is_empty(), "a text without a link");
    }

    // spec: EDIT-105
    #[test]
    fn edit_105_the_layout_is_built_again_only_for_another_key_and_not_at_all_when_it_is_not_to_be_kept() {
        let key = |text: &str| LayoutKey {
            text: text.to_string(),
            links: 1,
            fonts: EditorFonts::uniform(15.0),
            font_set: 0,
            pixels_per_point: 1.0f32.to_bits(),
            wrap_width: 500.0f32.to_bits(),
            numbered_default: false,
            scope: HashSet::new(),
            raw: Some(0..1),
            ranges: vec![(0..1, 0.0)],
            syntax_gen: 0,
        };
        let ctx = eframe::egui::Context::default();
        crate::theme::apply(&ctx);
        let raw = eframe::egui::RawInput { screen_rect: Some(eframe::egui::Rect::from_min_size(eframe::egui::Pos2::ZERO, eframe::egui::Vec2::new(900.0, 700.0))), ..Default::default() };
        let mut galley = None;
        let mut done = ctx.run_ui(raw, |ui| galley = Some(ui.painter().layout_no_wrap("a".into(), eframe::egui::FontId::monospace(12.0), eframe::egui::Color32::WHITE)));
        done.textures_delta.clear();
        let laid = Laid { galley: galley.unwrap(), formulas: Arc::new(vec![]), numbers: Arc::new(vec![]) };
        let mut cache = LayoutCache::default();
        let make = |_: &LayoutKey, _: &mut FormatCache| laid.clone();
        cache.get(key("a"), true, make);
        cache.get(key("a"), true, make);
        assert_eq!(cache.builds, 1, "the same key is kept");
        let mut other = key("a");
        other.raw = Some(1..2);
        cache.get(other, true, make);
        assert_eq!(cache.builds, 2, "other raw lines");
        let mut other = key("a");
        other.ranges = vec![(0..1, 30.0)];
        cache.get(other, true, make);
        assert_eq!(cache.builds, 3, "another row height");
        cache.get(key("b"), true, make);
        assert_eq!(cache.builds, 4, "another text");
        cache.get(key("b"), false, make);
        cache.get(key("b"), true, make);
        assert_eq!(cache.builds, 6, "what is made while the fonts change is not kept");
    }

    // spec: EDIT-107
    #[test]
    fn edit_107_the_counts_of_the_status_bar_are_worked_out_again_only_for_another_text() {
        let mut status = StatusCache::default();
        let rel = Path::new("A.md");
        assert_eq!(status.counts(rel, "one two\nthree"), (13, 3));
        assert_eq!(status.counts(rel, "one two\nthree"), (13, 3));
        assert_eq!(status.counts(rel, "one two\nthrEe"), (13, 3), "one letter different, the same length");
        assert_eq!(status.counts(rel, "one two"), (7, 2));
        assert_eq!(status.counts(Path::new("B.md"), "one two"), (7, 2));
        assert_eq!(status.counts(rel, "ä ö"), (3, 2), "characters, not bytes");
    }
}
