//! The board file (BOARD-001…BOARD-006): a Markdown note with `type: board` in its front matter.
//! A lane is a `##` heading; under it `key: value` lines (`query:`, `sort:`, `drop:`, `done:`)
//! and a list of `- [[links]]` for the pinned cards. [`Board`] reads such a text and writes it
//! again; it keeps every line it does not know exactly where it was (BOARD-006), so reading a
//! file and writing it gives the same bytes, and a change in the view (a lane added, removed,
//! renamed, moved, a query changed) touches only the lines it is about.
//!
//! ```text
//! ---
//! type: board
//! card-shows: due, status, tags
//! ---
//! # Sprint 48
//!
//! ## Doing
//! query: status:doing
//! drop: status: doing
//!
//! ## Pinned by hand
//! - [[Release notes 0.9]]
//! ```

use crate::markdown::{front_matter_lines, front_matter_values};

/// What a card shows besides its title (BOARD-010), as `card-shows:` names them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chip {
    Due,
    /// the `start:` day (BOARD-023)
    Start,
    Status,
    Priority,
    Tags,
}

impl Chip {
    pub const ALL: [Chip; 5] = [Chip::Due, Chip::Start, Chip::Status, Chip::Priority, Chip::Tags];

    /// The word of the `card-shows:` list.
    pub fn word(self) -> &'static str {
        match self {
            Chip::Due => "due",
            Chip::Start => "start",
            Chip::Status => "status",
            Chip::Priority => "priority",
            Chip::Tags => "tags",
        }
    }

    fn of_word(word: &str) -> Option<Chip> {
        Chip::ALL.into_iter().find(|c| c.word().eq_ignore_ascii_case(word.trim()))
    }
}

/// The properties of the board itself, from its front matter (BOARD-005).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Props {
    /// `cards: every-lane`: a card is in every lane it matches; the default is the first (BOARD-009)
    pub every_lane: bool,
    /// the chips of the cards; all of them when `card-shows:` is not there
    pub shows: Vec<Chip>,
}

/// The value of a front matter property (`key` in lower case): its first value, or the values of
/// a list joined.
fn front_value(text: &str, key: &str) -> Option<String> {
    front_matter_values(text).into_iter().find(|(k, _)| k == key).map(|(_, v)| v.join(", "))
}

/// Is the note a board: `type: board` in its front matter (BOARD-001)?
pub fn is_board(text: &str) -> bool {
    front_value(text, "type").is_some_and(|v| v.trim().eq_ignore_ascii_case("board"))
}

/// The properties of the board (BOARD-005).
pub fn props(text: &str) -> Props {
    let every_lane = front_value(text, "cards").is_some_and(|v| v.trim().eq_ignore_ascii_case("every-lane"));
    let shows = match front_value(text, "card-shows") {
        Some(v) => Chip::ALL.into_iter().filter(|c| v.split(',').filter_map(Chip::of_word).any(|w| w == *c)).collect(),
        None => Chip::ALL.to_vec(),
    };
    Props { every_lane, shows }
}

/// The lines of a lane that are `key: value` lines of the board format (BOARD-003), in the order
/// they are written when a lane gets one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Query,
    Sort,
    Drop,
    Done,
}

impl Key {
    const ALL: [Key; 4] = [Key::Query, Key::Sort, Key::Drop, Key::Done];

    fn word(self) -> &'static str {
        match self {
            Key::Query => "query",
            Key::Sort => "sort",
            Key::Drop => "drop",
            Key::Done => "done",
        }
    }
}

/// `key: value` of a line: the word before the first colon and what follows it, trimmed. Only a
/// line that starts with the word counts (an indented one is text).
fn key_line(line: &str) -> Option<(Key, &str)> {
    let (word, value) = line.split_once(':')?;
    let key = Key::ALL.into_iter().find(|k| k.word() == word)?;
    Some((key, value.trim()))
}

/// The inner text of a pinned card, `- [[Name]]`: the whole item is one link (BOARD-004).
fn pin_line(line: &str) -> Option<&str> {
    let item = line.trim_end().strip_prefix("- ")?.trim();
    let inner = item.strip_prefix("[[")?.strip_suffix("]]")?;
    (!inner.is_empty() && !inner.contains("[[") && !inner.contains("]]")).then_some(inner)
}

/// The name a pin (`Name`, `Folder/Name#Heading|alias`) links to, as the link resolver takes it.
pub fn pin_target(pin: &str) -> &str {
    pin.split('|').next().unwrap_or_default().split('#').next().unwrap_or_default().trim()
}

/// One lane: its heading and the lines under it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lane {
    /// the heading line as it is in the file
    heading: String,
    pub name: String,
    body: Vec<String>,
    /// the file has Windows line ends: the lines this lane gets end like that too
    crlf: bool,
}

fn blank(line: &str) -> bool {
    line.trim().is_empty()
}

impl Lane {
    fn new(name: &str, crlf: bool) -> Lane {
        let name = clean_name(name);
        Lane { heading: format!("## {name}{}", eol(crlf)), name, body: Vec::new(), crlf }
    }

    fn value(&self, key: Key) -> Option<&str> {
        self.body.iter().find_map(|l| key_line(l).filter(|(k, _)| *k == key).map(|(_, v)| v))
    }

    /// The query text, empty when the lane has none (a manual lane, BOARD-004).
    pub fn query(&self) -> &str {
        self.value(Key::Query).unwrap_or_default()
    }

    /// The text of the `sort:` line (BOARD-015).
    pub fn sort(&self) -> &str {
        self.value(Key::Sort).unwrap_or_default()
    }

    /// The `drop:` property (BOARD-019), `status: doing`.
    pub fn drop(&self) -> &str {
        self.value(Key::Drop).unwrap_or_default()
    }

    /// Does the lane hold finished cards (`done: true`, BOARD-024)?
    pub fn done(&self) -> bool {
        self.value(Key::Done).is_some_and(|v| v.eq_ignore_ascii_case("true"))
    }

    /// The pinned cards, the text inside `[[ ]]`, in the order of the lines (BOARD-022).
    pub fn pins(&self) -> Vec<&str> {
        self.body.iter().filter_map(|l| pin_line(l)).collect()
    }

    /// Set the line of `key` to `value`: replaced where it is, added after the lines of the keys
    /// that come before it; a blank `value` takes every line of the key out.
    pub fn set(&mut self, key: Key, value: &str) {
        let value = value.trim().replace(['\r', '\n'], " ");
        let at = self.body.iter().position(|l| key_line(l).is_some_and(|(k, _)| k == key));
        let eol = eol(self.crlf);
        match (at, value.is_empty()) {
            (Some(_), true) => self.body.retain(|l| key_line(l).is_none_or(|(k, _)| k != key)),
            (Some(i), false) => self.body[i] = format!("{}: {value}{eol}", key.word()),
            (None, true) => {}
            (None, false) => {
                let after = self.body.iter().rposition(|l| key_line(l).is_some_and(|(k, _)| Key::ALL.iter().position(|a| *a == k) < Key::ALL.iter().position(|a| *a == key)));
                self.body.insert(after.map_or(0, |i| i + 1), format!("{}: {value}{eol}", key.word()));
            }
        }
    }

    /// Set `done: true`, or remove the line.
    pub fn set_done(&mut self, done: bool) {
        self.set(Key::Done, if done { "true" } else { "" });
    }

    /// Replace the pinned cards by `pins` (the text inside `[[ ]]`), at the place of the first
    /// old one, or after the lines of the keys; the other lines stay (BOARD-022). A drop writes the
    /// manual order with it (`drag.rs`).
    pub fn set_pins(&mut self, pins: &[String]) {
        let first = self.body.iter().position(|l| pin_line(l).is_some());
        self.body.retain(|l| pin_line(l).is_none());
        let end = self.body.len() - self.body.iter().rev().take_while(|l| blank(l)).count();
        let after_keys = self.body.iter().rposition(|l| key_line(l).is_some()).map(|i| i + 1);
        let at = first.or(after_keys).unwrap_or(end).min(self.body.len());
        let eol = eol(self.crlf);
        for (n, pin) in pins.iter().enumerate() {
            self.body.insert(at + n, format!("- [[{pin}]]{eol}"));
        }
    }
}

/// A lane name is one line of text (BOARD-002); a blank one is "Lane".
fn clean_name(name: &str) -> String {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        "Lane".into()
    } else {
        name
    }
}

fn eol(crlf: bool) -> &'static str {
    if crlf {
        "\r"
    } else {
        ""
    }
}

/// A board file: what comes before the first lane, and the lanes (BOARD-002).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Board {
    /// the lines above the first `##` heading: the front matter, the title, whatever else
    head: Vec<String>,
    pub lanes: Vec<Lane>,
    /// the text ended with a line end
    final_newline: bool,
    crlf: bool,
}

impl Board {
    /// Read a note's text. Any text can be read: what is not a lane is kept as it is. A `##`
    /// heading inside the front matter or a code fence is no lane.
    pub fn parse(text: &str) -> Board {
        let final_newline = text.ends_with('\n');
        let body = if final_newline { &text[..text.len() - 1] } else { text };
        let crlf = text.contains("\r\n");
        let front = front_matter_lines(text);
        let mut head = Vec::new();
        let mut lanes: Vec<Lane> = Vec::new();
        let mut fenced = false;
        for (i, line) in body.split('\n').enumerate() {
            let t = line.trim_start();
            if i >= front && (t.starts_with("```") || t.starts_with("~~~")) {
                fenced = !fenced;
            }
            let heading = if i >= front && !fenced { line.strip_prefix("## ") } else { None };
            match (heading, lanes.last_mut()) {
                (Some(name), _) => lanes.push(Lane { heading: line.to_string(), name: name.trim().to_string(), body: Vec::new(), crlf }),
                (None, Some(lane)) => lane.body.push(line.to_string()),
                (None, None) => head.push(line.to_string()),
            }
        }
        Board { head, lanes, final_newline, crlf }
    }

    /// The text of the file.
    pub fn write(&self) -> String {
        let mut lines: Vec<&str> = self.head.iter().map(String::as_str).collect();
        for lane in &self.lanes {
            lines.push(&lane.heading);
            lines.extend(lane.body.iter().map(String::as_str));
        }
        let mut text = lines.join("\n");
        if self.final_newline {
            text.push('\n');
        }
        text
    }

    /// A lane at the end, named `name` (BOARD-006).
    pub fn add_lane(&mut self, name: &str) {
        self.lanes.push(Lane::new(name, self.crlf));
        self.tidy();
    }

    /// Take the lane `i` out, with its lines.
    pub fn remove_lane(&mut self, i: usize) {
        if i < self.lanes.len() {
            self.lanes.remove(i);
            self.tidy();
        }
    }

    /// Give the lane `i` another name; the lines under it stay.
    pub fn rename_lane(&mut self, i: usize, name: &str) {
        let crlf = self.crlf;
        if let Some(lane) = self.lanes.get_mut(i) {
            let name = clean_name(name);
            lane.heading = format!("## {name}{}", eol(crlf));
            lane.name = name;
        }
    }

    /// Move the lane `from` to the place `to` (the other lanes keep their order).
    pub fn move_lane(&mut self, from: usize, to: usize) {
        if from < self.lanes.len() && to < self.lanes.len() && from != to {
            let lane = self.lanes.remove(from);
            self.lanes.insert(to, lane);
            self.tidy();
        }
    }

    /// After lanes came, went or moved: a blank line ends each lane but the last, which has none.
    fn tidy(&mut self) {
        let eol = eol(self.crlf);
        if !self.lanes.is_empty() && self.head.last().is_some_and(|l| !blank(l)) {
            self.head.push(eol.to_string());
        }
        let last = self.lanes.len().saturating_sub(1);
        for (i, lane) in self.lanes.iter_mut().enumerate() {
            if i == last {
                while lane.body.last().is_some_and(|l| blank(l)) {
                    lane.body.pop();
                }
            } else if lane.body.last().is_none_or(|l| !blank(l)) {
                lane.body.push(eol.to_string());
            }
        }
        if self.lanes.is_empty() {
            while self.head.len() > 1 && self.head.last().is_some_and(|l| blank(l)) {
                self.head.pop();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::property_test_support::config;
    use proptest::prelude::*;

    const SAMPLE: &str = "---\ntype: board\ncard-shows: due, status, tags\n---\n# Sprint 48\n\nSome words about the sprint.\n\n## Backlog\nquery: #todo -#done\nsort: priority desc, due asc\nNote to self: keep this line\n\n## Doing\nquery: status:doing\ndrop: status: doing\n\n## Pinned by hand\n- [[Release notes 0.9]]\n- [[Kanban sketch.excalidraw]]\n- [[Folder/Name#Head|alias]]\n";

    fn summary(b: &Board) -> Vec<(String, String, String, String, bool, Vec<String>)> {
        b.lanes.iter().map(|l| (l.name.clone(), l.query().into(), l.sort().into(), l.drop().into(), l.done(), l.pins().into_iter().map(String::from).collect())).collect()
    }

    // spec: BOARD-001
    #[test]
    fn board_001_only_type_board_makes_a_note_a_board() {
        assert!(is_board(SAMPLE));
        assert!(is_board("---\ntype: Board\n---\n"));
        assert!(is_board("---\ntype: \"board\"\n---\n"));
        assert!(!is_board("---\ntype: note\n---\n## Lane\n"));
        assert!(!is_board("type: board\n"), "outside the front matter it is text");
        assert!(!is_board("# Plain\n"));
    }

    // spec: BOARD-002
    #[test]
    fn board_002_a_lane_is_a_level_two_heading_in_file_order() {
        let b = Board::parse(SAMPLE);
        assert_eq!(b.lanes.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Backlog", "Doing", "Pinned by hand"]);
        // a level one or three heading, a heading in a code fence and one in the front matter are no lanes
        let b = Board::parse("---\ntype: board\n## not a lane\n---\n# T\n### Sub\n```\n## in code\n```\n~~~\n## in tilde code\n~~~\n## Real\n");
        assert_eq!(b.lanes.len(), 1);
        assert_eq!(b.lanes[0].name, "Real");
    }

    // spec: BOARD-003
    #[test]
    fn board_003_lane_lines_take_everything_after_the_first_colon() {
        let b = Board::parse(SAMPLE);
        assert_eq!((b.lanes[0].query(), b.lanes[0].sort()), ("#todo -#done", "priority desc, due asc"));
        assert_eq!((b.lanes[1].query(), b.lanes[1].drop()), ("status:doing", "status: doing"));
        assert!(!b.lanes[1].done());
        let b = Board::parse("## A\ndone: true\n## B\ndone: no\n## C\n  query: indented\nquery: first\nquery: second\n");
        assert!(b.lanes[0].done() && !b.lanes[1].done());
        assert_eq!(b.lanes[2].query(), "first", "an indented line is text, the first of two counts");
    }

    // spec: BOARD-004
    #[test]
    fn board_004_a_list_of_links_is_the_pinned_cards_and_no_query_is_a_manual_lane() {
        let b = Board::parse(SAMPLE);
        assert_eq!(b.lanes[2].pins(), ["Release notes 0.9", "Kanban sketch.excalidraw", "Folder/Name#Head|alias"]);
        assert_eq!(b.lanes[2].query(), "", "a manual lane");
        assert_eq!(pin_target("Folder/Name#Head|alias"), "Folder/Name");
        assert_eq!(pin_target(" Plain "), "Plain");
        // a link with text around it, an embed and an empty link are no pins
        let b = Board::parse("## L\n- [[A]] and more\n- ![[B]]\n- [[]]\n* [[C]]\n- [[D]]\n");
        assert_eq!(b.lanes[0].pins(), ["D"]);
    }

    // spec: BOARD-005
    #[test]
    fn board_005_the_front_matter_sets_the_card_mode_and_the_chips() {
        let p = props(SAMPLE);
        assert!(!p.every_lane, "exclusive is the default");
        assert_eq!(p.shows, [Chip::Due, Chip::Status, Chip::Tags]);
        assert!(props("---\ntype: board\ncards: every-lane\n---\n").every_lane);
        assert!(props("---\ntype: board\ncards: Every-Lane\n---\n").every_lane);
        assert_eq!(props("---\ntype: board\n---\n").shows, Chip::ALL, "all chips without the property");
        assert!(props("---\ntype: board\ncard-shows:\n---\n").shows.is_empty(), "an empty list shows none");
        assert_eq!(props("---\ntype: board\ncard-shows: [priority, bogus, due]\n---\n").shows, [Chip::Due, Chip::Priority]);
    }

    // spec: BOARD-006
    #[test]
    fn board_006_reading_and_writing_a_file_gives_the_same_bytes() {
        assert_eq!(Board::parse(SAMPLE).write(), SAMPLE);
        for t in ["", "\n", "no newline", "## A", "## A\n", "a\r\n## L\r\nquery: x\r\n", "\n\n## A\n\n\n## B\n\n"] {
            assert_eq!(Board::parse(t).write(), t, "{t:?}");
        }
    }

    // spec: BOARD-006
    #[test]
    fn board_006_changes_touch_only_their_lines_and_keep_the_unknown_ones() {
        let mut b = Board::parse(SAMPLE);
        b.lanes[0].set(Key::Query, "#todo");
        b.rename_lane(1, "In progress");
        b.lanes[1].set(Key::Sort, "due");
        b.lanes[1].set(Key::Drop, "");
        let text = b.write();
        assert_eq!(text, SAMPLE.replace("query: #todo -#done", "query: #todo").replace("## Doing", "## In progress").replace("drop: status: doing\n", "").replace("query: status:doing\n", "query: status:doing\nsort: due\n"));
        assert!(text.contains("Note to self: keep this line") && text.contains("Some words about the sprint."));
    }

    // spec: BOARD-006
    #[test]
    fn board_006_lanes_are_added_removed_and_moved_with_their_lines() {
        let mut b = Board::parse(SAMPLE);
        b.add_lane("Done");
        b.lanes[3].set(Key::Query, "status:done");
        b.lanes[3].set_done(true);
        assert_eq!(b.lanes.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Backlog", "Doing", "Pinned by hand", "Done"]);
        let text = b.write();
        assert!(text.ends_with("- [[Folder/Name#Head|alias]]\n\n## Done\nquery: status:done\ndone: true\n"), "{text}");
        b.move_lane(3, 0);
        let moved = Board::parse(&b.write());
        assert_eq!(moved.lanes.iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["Done", "Backlog", "Doing", "Pinned by hand"]);
        assert_eq!(moved.lanes[1].body, Board::parse(SAMPLE).lanes[0].body, "the lines moved with their lane");
        assert!(b.write().ends_with("- [[Folder/Name#Head|alias]]\n"), "no blank line at the end of the file");
        b.remove_lane(0);
        b.remove_lane(2);
        assert_eq!(Board::parse(&b.write()).lanes.len(), 2);
        assert!(b.write().ends_with("query: status:doing\ndrop: status: doing\n"), "{}", b.write());
        // a file without lanes gets the first one after a blank line
        let mut b = Board::parse("---\ntype: board\n---\n# Empty");
        b.add_lane("First");
        assert_eq!(b.write(), "---\ntype: board\n---\n# Empty\n\n## First");
    }

    // spec: BOARD-006
    #[test]
    fn board_006_a_lane_name_is_one_line_and_never_blank() {
        let mut b = Board::parse("## A\n");
        b.rename_lane(0, "  two\nlines \t here ");
        assert_eq!(b.write(), "## two lines here\n");
        b.rename_lane(0, "   ");
        assert_eq!(b.write(), "## Lane\n");
        b.lanes[0].set(Key::Query, "a\nb");
        assert_eq!(b.lanes[0].query(), "a b");
    }

    // spec: BOARD-006
    #[test]
    fn board_006_the_pins_are_replaced_in_place() {
        let mut b = Board::parse("## L\nquery: x\nsome text\n- [[A]]\nmiddle\n- [[B]]\n\n## M\n");
        b.lanes[0].set_pins(&["B".into(), "C|c".into(), "A".into()]);
        assert_eq!(b.write(), "## L\nquery: x\nsome text\n- [[B]]\n- [[C|c]]\n- [[A]]\nmiddle\n\n## M\n");
        b.lanes[0].set_pins(&[]);
        assert_eq!(b.write(), "## L\nquery: x\nsome text\nmiddle\n\n## M\n");
        // the first pins come after the keys, and before the blank line that ends the lane
        b.lanes[0].set_pins(&["N".into()]);
        assert_eq!(b.write(), "## L\nquery: x\n- [[N]]\nsome text\nmiddle\n\n## M\n");
        let mut m = Board::parse("## M\n\n## N\n");
        m.lanes[0].set_pins(&["X".into()]);
        assert_eq!(m.write(), "## M\n- [[X]]\n\n## N\n");
    }

    /// What a lane of generated text may hold besides the lines of the format.
    fn doc_text(fences: bool) -> impl Strategy<Value = String> {
        let line = prop_oneof![
            2 => Just("---".to_string()),
            1 => Just("type: board".to_string()),
            1 => Just("# Title".to_string()),
            4 => "## [a-zA-Z ]{0,8}",
            2 => Just("### sub".to_string()),
            // (an unbalanced fence is only for the property that does not move lanes)
            1 => Just(if fences { "```".to_string() } else { "```\n## in code\nquery: in code\n```".to_string() }),
            1 => Just(if fences { "~~~".to_string() } else { "~~~\n## in tilde code\n~~~".to_string() }),
            3 => "query: [#a-z: -]{0,10}",
            2 => "sort: [a-z ,]{0,10}",
            2 => "drop: [a-z: ]{0,10}",
            1 => Just("done: true".to_string()),
            3 => "- \\[\\[[A-Za-z ]{0,6}\\]\\]",
            2 => Just("".to_string()),
            3 => "[a-z: ]{0,10}",
            2 => "\\PC{0,8}",
            1 => Just("\r".to_string()),
        ];
        (prop::collection::vec(line, 0..24), any::<bool>()).prop_map(|(lines, nl)| lines.join("\n") + if nl { "\n" } else { "" })
    }

    proptest! {
        #![proptest_config(config())]

        // spec: BOARD-006 (reading and writing again is the identity, for any text)
        #[test]
        fn board_006_any_text_is_written_back_unchanged(t in doc_text(true)) {
            prop_assert_eq!(Board::parse(&t).write(), t);
        }

        // spec: BOARD-006 (a change in the view reads back as that change and nothing else)
        #[test]
        fn board_006_an_edit_reads_back_as_the_edit_and_keeps_the_unknown_lines(
            t in doc_text(false), pick in 0usize..8, to in 0usize..8, op in 0u8..6, name in "[a-zA-Z #:]{0,8}",
        ) {
            let mut b = Board::parse(&t);
            let before = summary(&b);
            let unknown = |b: &Board| -> Vec<Vec<String>> { b.lanes.iter().map(|l| l.body.iter().filter(|x| key_line(x).is_none() && pin_line(x).is_none() && !blank(x)).cloned().collect()).collect() };
            let unknown_before = unknown(&b);
            let n = b.lanes.len();
            let mut expect = before.clone();
            let mut expect_unknown = unknown_before.clone();
            match op {
                0 => {
                    b.add_lane(&name);
                    expect.push((clean_name(&name), String::new(), String::new(), String::new(), false, vec![]));
                    expect_unknown.push(vec![]);
                }
                1 if n > 0 => {
                    let i = pick % n;
                    b.remove_lane(i);
                    expect.remove(i);
                    expect_unknown.remove(i);
                }
                2 if n > 0 => {
                    let i = pick % n;
                    b.rename_lane(i, &name);
                    expect[i].0 = clean_name(&name);
                }
                3 if n > 0 => {
                    let (i, j) = (pick % n, to % n);
                    b.move_lane(i, j);
                    let e = expect.remove(i);
                    expect.insert(j, e);
                    let u = expect_unknown.remove(i);
                    expect_unknown.insert(j, u);
                }
                4 if n > 0 => {
                    let i = pick % n;
                    b.lanes[i].set(Key::Query, &name);
                    expect[i].1 = name.trim().to_string();
                }
                5 if n > 0 => {
                    let i = pick % n;
                    let pins: Vec<String> = name.split(' ').filter(|p| !p.is_empty() && !p.contains(['[', ']'])).map(String::from).collect();
                    b.lanes[i].set_pins(&pins);
                    expect[i].5 = pins;
                }
                _ => {}
            }
            let again = Board::parse(&b.write());
            // (a `## ` line made by a name or a query inside a lane is the one case that is not a lane any more: the names here have no newline)
            prop_assert_eq!(summary(&again), expect);
            prop_assert_eq!(unknown(&again), expect_unknown);
        }
    }
}
