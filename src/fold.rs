//! Folding (EDIT-070…EDIT-085): the blocks of a note that can be folded away, the fold state of an
//! open note and the commands on it. No UI here; the editor (`editor.rs`) draws the result: a
//! folded block shows its header line and a badge, its other lines are hidden text (one row of
//! no height, like the text of a table drawn as a grid, EDIT-024).
//!
//! **To add a kind of block** (fenced code, front matter and a mail signature were added so): add a variant to
//! [`FoldKind`], write a finder `fn(&Doc) -> Vec<Raw>` that returns the header line and the line
//! after the block's last line for each block (a block needs at least one line besides its
//! header; [`Doc`] has the lines and says which of them are fenced code or front matter), and put
//! the finder into [`FINDERS`]. Everything else (the state that survives edits, the commands, the
//! markers and the badge) works for every kind.

use crate::markdown;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ops::Range;

/// What a foldable block is.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FoldKind {
    /// A heading and everything up to the next heading of the same or a higher level.
    Heading,
    /// Consecutive `>` lines.
    Quote,
    /// A list item with its nested and continued lines.
    Item,
    /// A run of two or more items of one level (a whole list, or a nested list).
    List,
    /// A fenced code block: the opening fence line and everything to the closing fence (EDIT-080).
    Code,
    /// The front matter, `---` to `---` at the top of the note (EDIT-081).
    Front,
    /// A mail signature: a line `-- ` up to the next heading or the end (EDIT-083).
    Signature,
}

/// The text a finder looks at: its lines and what is code or front matter in them.
pub struct Doc<'a> {
    pub lines: Vec<&'a str>,
    /// per line: inside a fenced code block (fences included)
    pub code: Vec<bool>,
    /// the first this many lines are the front matter
    pub front: usize,
}

impl<'a> Doc<'a> {
    pub fn new(text: &'a str) -> Doc<'a> {
        Doc { lines: markdown::lines_with_offsets(text).into_iter().map(|(_, l)| l).collect(), code: markdown::fenced_code_lines(text), front: markdown::front_matter_lines(text) }
    }

    /// Is this line code or front matter, where no markdown starts a block?
    pub fn skip(&self, line: usize) -> bool {
        line < self.front || self.code.get(line).copied().unwrap_or(false)
    }

    fn blank(&self, line: usize) -> bool {
        self.lines[line].trim().is_empty()
    }

    /// `end` pulled back over the blank lines before it (the gap to the next block stays visible).
    fn trim_blank(&self, header: usize, mut end: usize) -> usize {
        while end > header + 1 && self.blank(end - 1) {
            end -= 1;
        }
        end
    }
}

/// What a finder returns: a block of `kind` from its header line to the line after its last.
pub struct Raw {
    pub kind: FoldKind,
    pub header: usize,
    pub end: usize,
}

/// The finders of all kinds of blocks.
const FINDERS: [fn(&Doc) -> Vec<Raw>; 6] = [find_headings, find_quotes, find_lists, find_code, find_front, find_signatures];

/// A block that can be folded: its header line stays, the lines after it (up to `end`) are hidden.
#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: FoldKind,
    /// the header line (from 0)
    pub header: usize,
    /// the line after the block's last line
    pub end: usize,
    /// the header line's text, trimmed
    pub title: String,
    /// how many blocks of this kind before this one have the same title
    pub nth: usize,
}

impl Block {
    /// The lines that a fold hides.
    pub fn body(&self) -> Range<usize> {
        self.header + 1..self.end
    }
}

/// The blocks of `text`, by header line; at one line the larger block first.
pub fn find_blocks(text: &str) -> Vec<Block> {
    let doc = Doc::new(text);
    let mut raw: Vec<Raw> = FINDERS.iter().flat_map(|f| f(&doc)).filter(|r| r.end > r.header + 1).collect();
    raw.sort_by_key(|a| (a.header, std::cmp::Reverse(a.end)));
    let mut seen: HashMap<(FoldKind, &str), usize> = HashMap::new();
    raw.into_iter()
        .map(|r| {
            let title = doc.lines[r.header].trim();
            let count = seen.entry((r.kind, title)).or_insert(0);
            let nth = *count;
            *count += 1;
            Block { kind: r.kind, header: r.header, end: r.end, title: title.to_string(), nth }
        })
        .collect()
}

fn find_headings(doc: &Doc) -> Vec<Raw> {
    let heads: Vec<(usize, usize)> = (0..doc.lines.len()).filter(|&l| !doc.skip(l)).filter_map(|l| markdown::heading_level(doc.lines[l]).map(|lv| (l, lv))).collect();
    heads
        .iter()
        .enumerate()
        .map(|(i, &(header, level))| {
            let end = heads[i + 1..].iter().find(|(_, l)| *l <= level).map_or(doc.lines.len(), |(l, _)| *l);
            Raw { kind: FoldKind::Heading, header, end: doc.trim_blank(header, end) }
        })
        .collect()
}

fn find_quotes(doc: &Doc) -> Vec<Raw> {
    let is_quote = |l: usize| !doc.skip(l) && doc.lines[l].trim_start().starts_with('>');
    let mut out = Vec::new();
    let mut l = 0;
    while l < doc.lines.len() {
        if is_quote(l) {
            let start = l;
            while l < doc.lines.len() && is_quote(l) {
                l += 1;
            }
            out.push(Raw { kind: FoldKind::Quote, header: start, end: l });
        } else {
            l += 1;
        }
    }
    out
}

fn find_code(doc: &Doc) -> Vec<Raw> {
    crate::syntax::fences(&doc.lines).into_iter().map(|f| Raw { kind: FoldKind::Code, header: f.open, end: if f.closed { f.end } else { doc.trim_blank(f.open, f.end) } }).collect()
}

fn find_front(doc: &Doc) -> Vec<Raw> {
    if doc.front > 0 {
        vec![Raw { kind: FoldKind::Front, header: 0, end: doc.front }]
    } else {
        Vec::new()
    }
}

/// A line `-- ` (or `--`) up to the next heading, or the end of the note; behind quote marks up to
/// the end of that quoted mail's segment (EDIT-083).
fn find_signatures(doc: &Doc) -> Vec<Raw> {
    let mut out: Vec<Raw> = Vec::new();
    for l in 0..doc.lines.len() {
        if doc.skip(l) || doc.lines[l].trim_end() != "--" || out.last().is_some_and(|r| l < r.end) {
            continue;
        }
        let end = (l + 1..doc.lines.len()).find(|&n| !doc.skip(n) && markdown::heading_level(doc.lines[n]).is_some()).unwrap_or(doc.lines.len());
        out.push(Raw { kind: FoldKind::Signature, header: l, end: doc.trim_blank(l, end) });
    }
    // the signature of a mail quoted in the note: `> -- ` up to where that quote's depth changes
    for (l, end) in crate::import::mail_segments::quoted_signatures(&doc.lines, |l| doc.skip(l)) {
        if !out.iter().any(|r| r.header < l && l < r.end) {
            out.push(Raw { kind: FoldKind::Signature, header: l, end: doc.trim_blank(l, end) });
        }
    }
    out
}

/// Is this a mail note: front matter with `type: email` (EDIT-082)?
pub fn is_mail(text: &str) -> bool {
    markdown::front_matter_values(text).iter().any(|(k, v)| k == "type" && v.first().is_some_and(|v| v.eq_ignore_ascii_case("email")))
}

/// The badge of a folded block that says how many lines it hides (EDIT-071; in a mail note a quote
/// says "quoted lines", EDIT-082).
pub fn badge_label(kind: FoldKind, mail: bool, n: usize) -> String {
    let what = if kind == FoldKind::Quote && mail { "quoted line" } else { "line" };
    format!("\u{2026} {n} {what}{}", if n == 1 { "" } else { "s" })
}

/// The width of a line's indentation (a tab counts as four).
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn find_lists(doc: &Doc) -> Vec<Raw> {
    // every item with the line after its last line, and its indentation
    let mut items: Vec<(usize, usize, usize)> = Vec::new();
    for i in 0..doc.lines.len() {
        if doc.skip(i) || crate::lists::list_prefix(doc.lines[i]).is_none() {
            continue;
        }
        let n = indent_of(doc.lines[i]);
        let mut end = i + 1;
        for j in i + 1..doc.lines.len() {
            if doc.blank(j) {
                continue;
            }
            let indent = indent_of(doc.lines[j]);
            // a sibling indented by a space or two is no child; a text line needs only to be indented
            let child = if crate::lists::list_prefix(doc.lines[j]).is_some() && !doc.code[j] { indent >= n + 2 } else { indent > n };
            if !child {
                break;
            }
            end = j + 1;
        }
        items.push((i, end, n));
    }
    let mut out: Vec<Raw> = items.iter().map(|&(header, end, _)| Raw { kind: FoldKind::Item, header, end }).collect();
    // a list: items of one level that follow each other (blank lines between), in one parent
    let mut member = vec![false; items.len()];
    for k in 0..items.len() {
        if member[k] {
            continue;
        }
        let (first, mut end, n) = items[k];
        let mut count = 1;
        for next in k + 1..items.len() {
            let (start, e, indent) = items[next];
            // an item inside the last one (a deeper one) is not a sibling: on to the next
            if start < end {
                continue;
            }
            // a sibling is indented like the first one, or by a space (as for children)
            if !(n..n + 2).contains(&indent) || !(end..start).all(|b| doc.blank(b)) {
                break;
            }
            member[next] = true;
            end = e;
            count += 1;
        }
        if count >= 2 {
            out.push(Raw { kind: FoldKind::List, header: first, end });
        }
    }
    out
}

/// What a command does to the folds (the keys, EDIT-077, and vim-lite's `za` `zc` `zo` `zM` `zR`, EDIT-078).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FoldCmd {
    /// Fold the block of the cursor's line, or unfold it when it is folded (`za`).
    Toggle,
    /// Fold the block of the cursor's line; again: the block around it (`zc`).
    Fold,
    /// Unfold the block of the cursor's line (`zo`).
    Unfold,
    /// Fold every block (`zM`).
    FoldAll,
    /// Unfold every block (`zR`).
    UnfoldAll,
}

/// A folded block as it is remembered: by what it is and says, so that edits above or below it
/// leave it folded; the line is the fall-back when its header was edited. This is also what is
/// written into the vault state (EDIT-079).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Key {
    kind: FoldKind,
    title: String,
    nth: usize,
    line: usize,
}

impl Key {
    fn of(b: &Block) -> Key {
        Key { kind: b.kind, title: b.title.clone(), nth: b.nth, line: b.header }
    }
}

/// Up to this many keys [`NoteFolds::folded`] scans the blocks for each; more build a lookup first.
const SCAN_KEYS: usize = 8;

/// The folded blocks of one note, for this session only: never written into the file.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct NoteFolds {
    keys: Vec<Key>,
}

impl NoteFolds {
    pub fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// The folds as they are written into the vault state (EDIT-079).
    pub fn to_saved(&self) -> Vec<Key> {
        self.keys.clone()
    }

    /// The folds read back from the vault state; a block that is gone drops out at the next
    /// [`NoteFolds::sync`] (EDIT-079).
    pub fn from_saved(keys: &[Key]) -> NoteFolds {
        NoteFolds { keys: keys.to_vec() }
    }

    /// The indices of the folded blocks among `blocks` (of the current text): a block is folded when
    /// it is what a key remembers (kind, title and number among equal ones), or else the block of that
    /// kind whose header is on the remembered line (the header is being edited).
    pub fn folded(&self, blocks: &[Block]) -> Vec<usize> {
        let mut taken = vec![false; blocks.len()];
        let mut out: Vec<usize> = Vec::new();
        let mut keep = |found: Option<usize>| {
            if let Some(i) = found.filter(|&i| !std::mem::replace(&mut taken[i], true)) {
                out.push(i);
            }
        };
        if self.keys.len() <= SCAN_KEYS {
            for k in &self.keys {
                keep(blocks.iter().position(|b| b.kind == k.kind && b.title == k.title && b.nth == k.nth).or_else(|| blocks.iter().position(|b| b.kind == k.kind && b.header == k.line)));
            }
        } else {
            // many keys (everything folded in a large note): look the blocks up instead of scanning for each (EDIT-101)
            let mut by_name: HashMap<(FoldKind, &str, usize), usize> = HashMap::new();
            let mut by_line: HashMap<(FoldKind, usize), usize> = HashMap::new();
            for (i, b) in blocks.iter().enumerate() {
                by_name.entry((b.kind, b.title.as_str(), b.nth)).or_insert(i);
                by_line.entry((b.kind, b.header)).or_insert(i);
            }
            for k in &self.keys {
                keep(by_name.get(&(k.kind, k.title.as_str(), k.nth)).or_else(|| by_line.get(&(k.kind, k.line))).copied());
            }
        }
        out.sort_unstable();
        out
    }

    /// Follow the text: remember the blocks as they are now, forget the ones that are gone.
    pub fn sync(&mut self, blocks: &[Block]) {
        self.keys = self.folded(blocks).into_iter().map(|i| Key::of(&blocks[i])).collect();
    }

    /// The folds a note starts with when it is opened (EDIT-081, EDIT-082): its front matter when
    /// `front`; in a mail note also its quotes and its signature.
    pub fn defaults(blocks: &[Block], mail: bool, front: bool) -> NoteFolds {
        let keys = blocks.iter().filter(|b| (front && b.kind == FoldKind::Front) || (mail && matches!(b.kind, FoldKind::Quote | FoldKind::Signature))).map(Key::of).collect();
        NoteFolds { keys }
    }

    /// Fold or unfold block `i` of `blocks`.
    pub fn set(&mut self, blocks: &[Block], i: usize, fold: bool) {
        let mut now = self.folded(blocks);
        now.retain(|n| *n != i);
        if fold {
            now.push(i);
        }
        self.keys = now.into_iter().map(|i| Key::of(&blocks[i])).collect();
    }

    /// Unfold every folded block whose hidden lines include `line` (the cursor is there). True when
    /// something was unfolded.
    pub fn reveal(&mut self, blocks: &[Block], line: usize) -> bool {
        let folded = self.folded(blocks);
        let keep: Vec<usize> = folded.iter().copied().filter(|&i| !blocks[i].body().contains(&line)).collect();
        let changed = keep.len() != folded.len();
        if changed {
            self.keys = keep.into_iter().map(|i| Key::of(&blocks[i])).collect();
        }
        changed
    }

    /// Do a command with the cursor in `line`. Folding hides the cursor's line when it is in the block's
    /// body: the answer is then the header line the cursor has to go to.
    pub fn apply(&mut self, blocks: &[Block], cmd: FoldCmd, line: usize) -> Option<usize> {
        let folded = self.folded(blocks);
        let is_folded = |i: &usize| folded.contains(i);
        // the blocks with their header on the line, the smallest first
        let mut here: Vec<usize> = (0..blocks.len()).filter(|&i| blocks[i].header == line).collect();
        here.sort_by_key(|&i| blocks[i].end);
        // the blocks around the line, the innermost first
        let mut around: Vec<usize> = (0..blocks.len()).filter(|&i| blocks[i].body().contains(&line)).collect();
        around.sort_by_key(|&i| std::cmp::Reverse((blocks[i].header, std::cmp::Reverse(blocks[i].end))));
        match cmd {
            FoldCmd::FoldAll => {
                self.keys = blocks.iter().map(Key::of).collect();
                blocks.iter().filter(|b| b.body().contains(&line)).map(|b| b.header).min()
            }
            FoldCmd::UnfoldAll => {
                self.keys.clear();
                None
            }
            FoldCmd::Unfold => {
                if let Some(&i) = here.iter().filter(|i| is_folded(i)).max_by_key(|&&i| blocks[i].end) {
                    self.set(blocks, i, false);
                }
                None
            }
            FoldCmd::Toggle if here.iter().any(is_folded) => {
                let i = *here.iter().filter(|i| is_folded(i)).max_by_key(|&&i| blocks[i].end).expect("one is folded");
                self.set(blocks, i, false);
                None
            }
            FoldCmd::Toggle | FoldCmd::Fold => {
                let target = here.iter().copied().find(|i| !is_folded(i)).or_else(|| around.first().copied());
                let i = target?;
                self.set(blocks, i, true);
                (blocks[i].header != line).then_some(blocks[i].header)
            }
        }
    }
}

/// The folds as they are drawn: the hidden lines and the badges.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// the hidden lines, as ranges that do not touch or overlap
    pub hidden: Vec<Range<usize>>,
    /// per folded block that is not hidden itself: the header line and the number of hidden lines
    pub badges: Vec<(usize, usize)>,
}

impl View {
    /// The view of the folded blocks (indices into `blocks`).
    pub fn of(blocks: &[Block], folded: &[usize]) -> View {
        let mut ranges: Vec<&Block> = folded.iter().map(|&i| &blocks[i]).collect();
        ranges.sort_by_key(|b| (b.header, std::cmp::Reverse(b.end)));
        let mut view = View::default();
        for b in ranges {
            let body = b.body();
            // the blocks come by header and the ranges are in that order: only the last can hold the header
            if view.hidden.last().is_some_and(|h| h.contains(&b.header)) {
                // inside a folded block: its header is hidden, so its lines are too
                if let Some(last) = view.hidden.last_mut() {
                    last.end = last.end.max(body.end);
                }
                continue;
            }
            view.badges.push((b.header, body.len()));
            match view.hidden.last_mut() {
                Some(last) if last.end == body.start => last.end = body.end,
                _ => view.hidden.push(body),
            }
        }
        view
    }

    pub fn is_hidden(&self, line: usize) -> bool {
        self.hidden_at(line).is_some()
    }

    /// The hidden lines that include `line`. The ranges are in order and apart: the first that ends after
    /// the line is the only one that can hold it.
    pub fn hidden_at(&self, line: usize) -> Option<&Range<usize>> {
        self.hidden.get(self.hidden.partition_point(|h| h.end <= line)).filter(|h| h.start <= line)
    }

    /// Do any hidden lines overlap these lines?
    pub fn hides_any(&self, lines: &Range<usize>) -> bool {
        self.hidden.iter().any(|h| h.start < lines.end && lines.start < h.end)
    }

    /// The hidden lines as byte ranges of `text` (a line's break belongs to it).
    pub fn byte_ranges(&self, text: &str) -> Vec<Range<usize>> {
        let starts = markdown::lines_with_offsets(text);
        self.hidden.iter().filter(|h| h.start < starts.len()).map(|h| starts[h.start].0..starts.get(h.end).map_or(text.len(), |(o, _)| *o)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(text: &str) -> Vec<(FoldKind, usize, usize)> {
        find_blocks(text).into_iter().map(|b| (b.kind, b.header, b.end)).collect()
    }

    // spec: EDIT-070
    #[test]
    fn heading_sections_run_to_the_next_heading_of_the_same_or_a_higher_level() {
        let t = "# A\ntext\n## B\nb text\n\n## C\nc\n# D\nd\n";
        assert_eq!(found(t), vec![(FoldKind::Heading, 0, 7), (FoldKind::Heading, 2, 4), (FoldKind::Heading, 5, 7), (FoldKind::Heading, 7, 9)]);
        // a heading without anything under it, or only blank lines, is no block
        assert_eq!(found("# A\n# B\n\n# C\nx"), vec![(FoldKind::Heading, 3, 5)]);
        // the blank lines before the next heading stay visible
        assert_eq!(find_blocks("# A\nx\n\n\n# B\ny")[0].end, 2);
    }

    // spec: EDIT-070
    #[test]
    fn headings_in_code_fences_and_front_matter_are_ignored() {
        let t = "---\n# not a heading\nx: 1\n---\n# Real\n```\n# comment\n```\ntext\n# Next\nz";
        assert_eq!(found(t), vec![(FoldKind::Front, 0, 4), (FoldKind::Heading, 4, 9), (FoldKind::Code, 5, 8), (FoldKind::Heading, 9, 11)]);
        // a hash line in a fence does not end the section above it
        assert_eq!(found("# A\n```\n# x\n```\nafter\n"), vec![(FoldKind::Heading, 0, 5), (FoldKind::Code, 1, 4)]);
    }

    // spec: EDIT-070
    #[test]
    fn quotes_are_runs_of_quote_lines_of_two_or_more() {
        let t = "intro\n> one\n> two\n>\n> four\n\n> lonely\n\n```\n> in code\n> in code\n```\n";
        assert_eq!(found(t), vec![(FoldKind::Quote, 1, 5), (FoldKind::Code, 8, 12)], "the quote lines in the code are none");
        assert_eq!(found("   > indented\n   > quote"), vec![(FoldKind::Quote, 0, 2)]);
    }

    // spec: EDIT-070
    #[test]
    fn a_list_item_holds_its_nested_and_continued_lines_and_a_list_its_items() {
        let t = "- a\n    - a1\n    - a2\n        - deep\n- b\n- c\n    continued\n\ntext";
        assert_eq!(found(t), vec![(FoldKind::List, 0, 7), (FoldKind::Item, 0, 4), (FoldKind::List, 1, 4), (FoldKind::Item, 2, 4), (FoldKind::Item, 5, 7)]);
    }

    // spec: EDIT-070
    #[test]
    fn a_blank_line_inside_an_item_stays_in_it_and_one_before_a_sibling_is_left_out() {
        let t = "1. one\n\n    para\n\n2. two\n3. three";
        let items: Vec<_> = found(t).into_iter().filter(|(k, ..)| *k == FoldKind::Item).collect();
        assert_eq!(items, vec![(FoldKind::Item, 0, 3)]);
        assert!(found(t).contains(&(FoldKind::List, 0, 6)), "the loose list is one list: {:?}", found(t));
        // a sibling with one space is no child
        assert_eq!(found("- a\n - b"), vec![(FoldKind::List, 0, 2)]);
        // rules are no items
        assert!(found("- a\n---\n- b").iter().all(|(k, ..)| *k != FoldKind::List));
    }

    // spec: EDIT-070
    #[test]
    fn a_quote_inside_a_list_item_is_a_block_of_its_own_and_task_items_fold() {
        let t = "- [ ] task\n    > q1\n    > q2\n- [x] done";
        let b = found(t);
        assert!(b.contains(&(FoldKind::Item, 0, 3)));
        assert!(b.contains(&(FoldKind::Quote, 1, 3)));
        assert!(b.contains(&(FoldKind::List, 0, 4)));
    }

    // spec: EDIT-070
    #[test]
    fn code_in_a_list_item_does_not_start_items() {
        let t = "- a\n    ```\n    - not an item\n    more\n    ```\n- b";
        let b = found(t);
        assert!(b.contains(&(FoldKind::Item, 0, 5)));
        assert!(!b.iter().any(|(k, h, _)| *k == FoldKind::Item && *h == 2));
    }

    fn state(text: &str, header: &str) -> (NoteFolds, Vec<Block>) {
        let blocks = find_blocks(text);
        let mut f = NoteFolds::default();
        let at = blocks.iter().find(|b| b.title == header).unwrap().header;
        f.apply(&blocks, FoldCmd::Toggle, at);
        (f, blocks)
    }

    // spec: EDIT-072
    #[test]
    fn a_fold_stays_with_its_block_while_the_text_around_it_is_edited() {
        let t = "# A\nx\n# B\ny\n# C\nz\n";
        let (mut f, blocks) = state(t, "# B");
        assert_eq!(f.folded(&blocks).len(), 1);
        // lines added above
        let t2 = "intro\nmore\n# A\nx\n# B\ny\n# C\nz\n";
        let b2 = find_blocks(t2);
        let folded = f.folded(&b2);
        assert_eq!(folded.iter().map(|&i| b2[i].header).collect::<Vec<_>>(), vec![4], "# B is still folded");
        f.sync(&b2);
        // the header is edited: it is still the block on that line
        let t3 = "intro\nmore\n# A\nx\n# Bee\ny\n# C\nz\n";
        let b3 = find_blocks(t3);
        assert_eq!(f.folded(&b3).iter().map(|&i| b3[i].header).collect::<Vec<_>>(), vec![4]);
        f.sync(&b3);
        // the block is deleted: the fold is forgotten
        let b4 = find_blocks("intro\n# A\nx\n");
        assert!(f.folded(&b4).is_empty());
        f.sync(&b4);
        assert!(f.is_empty());
    }

    // spec: EDIT-072
    #[test]
    fn equal_headers_are_told_apart_by_their_order() {
        let t = "- same\n    a\n- same\n    b\n";
        let blocks = find_blocks(t);
        let mut f = NoteFolds::default();
        f.apply(&blocks, FoldCmd::Toggle, 2);
        let t2 = "x\n\n- same\n    a\n- same\n    b\n";
        let b2 = find_blocks(t2);
        let items: Vec<_> = f.folded(&b2).into_iter().map(|i| (b2[i].kind, b2[i].header)).collect();
        assert_eq!(items, vec![(FoldKind::Item, 4)]);
    }

    // spec: EDIT-073
    #[test]
    fn commands_fold_unfold_and_toggle_the_block_of_the_line() {
        let t = "# A\nx\n## B\ny\nz\n";
        let blocks = find_blocks(t);
        let mut f = NoteFolds::default();
        // fold on a header folds it; on a line of the body the block around it, and the cursor goes to its header
        assert_eq!(f.apply(&blocks, FoldCmd::Fold, 2), None);
        assert_eq!(f.folded(&blocks), vec![1]);
        // again on the header: the block around it
        assert_eq!(f.apply(&blocks, FoldCmd::Fold, 2), Some(0));
        assert_eq!(f.folded(&blocks), vec![0, 1]);
        // unfold opens the outermost folded block on the line first
        f.apply(&blocks, FoldCmd::Unfold, 0);
        assert_eq!(f.folded(&blocks), vec![1], "the inner one stays folded");
        assert_eq!(f.apply(&blocks, FoldCmd::Toggle, 2), None);
        assert!(f.folded(&blocks).is_empty());
        // a line of the body folds its block and says where the cursor goes
        assert_eq!(f.apply(&blocks, FoldCmd::Toggle, 4), Some(2));
        // all
        f.apply(&blocks, FoldCmd::UnfoldAll, 0);
        assert_eq!(f.apply(&blocks, FoldCmd::FoldAll, 3), Some(0), "the cursor goes to the outermost folded header");
        assert_eq!(f.folded(&blocks), vec![0, 1]);
        f.apply(&blocks, FoldCmd::UnfoldAll, 0);
        assert!(f.is_empty());
        // a line that belongs to no block does nothing
        assert_eq!(f.apply(&find_blocks("plain\ntext"), FoldCmd::Toggle, 1), None);
    }

    // spec: EDIT-070, EDIT-073
    #[test]
    fn the_first_item_of_a_list_folds_alone_first_and_the_whole_list_on_a_second_fold() {
        let t = "- a\n    - a1\n- b\n    - b1\n";
        let blocks = find_blocks(t);
        let mut f = NoteFolds::default();
        f.apply(&blocks, FoldCmd::Fold, 0);
        let kinds = |f: &NoteFolds| f.folded(&blocks).into_iter().map(|i| blocks[i].kind).collect::<Vec<_>>();
        assert_eq!(kinds(&f), vec![FoldKind::Item]);
        f.apply(&blocks, FoldCmd::Fold, 0);
        assert_eq!(kinds(&f), vec![FoldKind::List, FoldKind::Item]);
        f.apply(&blocks, FoldCmd::Unfold, 0);
        assert_eq!(kinds(&f), vec![FoldKind::Item], "unfold opens the whole list first");
    }

    // spec: EDIT-075
    #[test]
    fn revealing_a_line_unfolds_every_block_that_hides_it_and_no_other() {
        let t = "# A\nx\n## B\ny\n# C\nz\n";
        let blocks = find_blocks(t);
        let mut f = NoteFolds::default();
        f.apply(&blocks, FoldCmd::FoldAll, 0);
        assert!(!f.reveal(&blocks, 0), "a header line is not hidden");
        assert!(f.reveal(&blocks, 3));
        assert_eq!(f.folded(&blocks).iter().map(|&i| blocks[i].header).collect::<Vec<_>>(), vec![4], "A and B are open, C stays folded");
        assert!(!f.reveal(&blocks, 3));
    }

    // spec: EDIT-071
    #[test]
    fn the_view_merges_hidden_lines_and_gives_badges_to_visible_headers_only() {
        let t = "# A\nx\n## B\ny\nz\n# C\nw\n";
        let blocks = find_blocks(t);
        let v = View::of(&blocks, &[1, 0]);
        assert_eq!(v.hidden, vec![1..5]);
        assert_eq!(v.badges, vec![(0, 4)], "B is hidden inside A");
        assert!(v.is_hidden(3) && !v.is_hidden(0) && !v.is_hidden(5));
        assert!(v.hides_any(&(4..6)) && !v.hides_any(&(5..7)));
        let v = View::of(&blocks, &[1, 2]);
        assert_eq!(v.hidden, vec![3..5, 6..7]);
        assert_eq!(v.badges, vec![(2, 2), (5, 1)]);
        assert_eq!(v.byte_ranges(t), vec![t.find("y").unwrap()..t.find("# C").unwrap(), t.find("w").unwrap()..t.len()]);
    }

    // spec: EDIT-080
    #[test]
    fn a_fenced_code_block_folds_from_its_opening_fence_to_the_closing_one_with_or_without_a_language() {
        let t = "intro\n```rust\nfn main() {}\n```\n\n```\nplain\nmore\n```\n~~~py\nx\n~~~\n";
        assert_eq!(found(t), vec![(FoldKind::Code, 1, 4), (FoldKind::Code, 5, 9), (FoldKind::Code, 9, 12)]);
        assert_eq!(find_blocks(t)[0].title, "```rust");
        // a fence with nothing in it is a block of its closing line
        assert_eq!(found("```\n```\nafter"), vec![(FoldKind::Code, 0, 2)]);
        // a heading inside the code is no block
        assert_eq!(found("```\n# not\ncode\n```"), vec![(FoldKind::Code, 0, 4)]);
    }

    // spec: EDIT-080
    #[test]
    fn a_fence_in_a_list_item_is_a_block_inside_the_item_and_an_unclosed_one_runs_to_the_end() {
        let t = "- item\n    ```sh\n    ls\n    ```\n- next";
        let b = found(t);
        assert!(b.contains(&(FoldKind::Code, 1, 4)), "{b:?}");
        assert!(b.contains(&(FoldKind::Item, 0, 4)), "{b:?}");
        // unclosed: to the end, the blank lines at the end stay out
        assert_eq!(found("text\n```\ncode\nmore\n\n"), vec![(FoldKind::Code, 1, 4)]);
        // a lone opening fence has nothing to hide
        assert!(found("text\n```").is_empty());
    }

    // spec: EDIT-081
    #[test]
    fn the_front_matter_is_a_block_from_its_first_line_and_one_that_is_not_closed_is_none() {
        let t = "---\ntype: email\nsubject: Hi\n---\n# Body\ntext";
        assert_eq!(found(t), vec![(FoldKind::Front, 0, 4), (FoldKind::Heading, 4, 6)]);
        assert_eq!(find_blocks(t)[0].title, "---");
        assert!(found("---\ntype: email\nno end\n").iter().all(|(k, ..)| *k != FoldKind::Front));
        assert!(found("text\n---\na: b\n---\n").iter().all(|(k, ..)| *k != FoldKind::Front), "only at the top");
    }

    // spec: EDIT-083
    #[test]
    fn a_signature_runs_from_its_dash_dash_line_to_the_next_heading_or_the_end() {
        let t = "Hi\n\n-- \nAnna\nAcme\n\n# Next\ntext\n";
        assert_eq!(found(t), vec![(FoldKind::Signature, 2, 5), (FoldKind::Heading, 6, 8)]);
        // without the space, and to the end of the note
        assert_eq!(found("Hi\n--\nAnna\nAcme"), vec![(FoldKind::Signature, 1, 4)]);
        // not in code, not a longer rule
        assert!(found("```\n-- \nx\n```\n> ```\n> -- \n> ```\n---\nz").iter().all(|(k, ..)| *k != FoldKind::Signature));
        // in a quote it ends with the quoted mail's segment: at a change of the quote depth or a reply header
        let q = "> text\n> -- \n> Anna\n> Acme\n>\n> > older\n";
        assert_eq!(found(q).iter().filter(|(k, ..)| *k == FoldKind::Signature).collect::<Vec<_>>(), vec![&(FoldKind::Signature, 1, 5)]);
        let q = "> text\n> -- \n> Anna\n> On Mon, 1 Jan 2026, Bob wrote:\n> > older\n";
        assert_eq!(found(q).iter().filter(|(k, ..)| *k == FoldKind::Signature).collect::<Vec<_>>(), vec![&(FoldKind::Signature, 1, 3)]);
        // a second one inside the first is part of it
        assert_eq!(found("-- \na\n-- \nb").iter().filter(|(k, ..)| *k == FoldKind::Signature).count(), 1);
    }

    // spec: EDIT-081, EDIT-082
    #[test]
    fn a_note_starts_folded_by_its_front_matter_and_in_a_mail_note_by_its_quotes_and_signature() {
        let mail = "---\ntype: email\n---\nHi\n> q1\n> q2\n\n-- \nAnna\n";
        assert!(is_mail(mail) && !is_mail("---\ntype: note\n---\nHi") && !is_mail("type: email"));
        let blocks = find_blocks(mail);
        let kinds = |f: &NoteFolds| f.folded(&blocks).into_iter().map(|i| blocks[i].kind).collect::<Vec<_>>();
        assert_eq!(kinds(&NoteFolds::defaults(&blocks, true, true)), vec![FoldKind::Front, FoldKind::Quote, FoldKind::Signature]);
        assert_eq!(kinds(&NoteFolds::defaults(&blocks, true, false)), vec![FoldKind::Quote, FoldKind::Signature], "the front matter by its setting");
        assert!(NoteFolds::defaults(&blocks, false, false).is_empty(), "a note that is no mail has no folds of its own");
    }

    // spec: EDIT-071, EDIT-082
    #[test]
    fn the_badge_counts_lines_and_a_quote_in_a_mail_note_counts_quoted_lines() {
        assert_eq!(badge_label(FoldKind::Heading, false, 4), "\u{2026} 4 lines");
        assert_eq!(badge_label(FoldKind::Heading, true, 1), "\u{2026} 1 line");
        assert_eq!(badge_label(FoldKind::Quote, false, 2), "\u{2026} 2 lines");
        assert_eq!(badge_label(FoldKind::Quote, true, 2), "\u{2026} 2 quoted lines");
        assert_eq!(badge_label(FoldKind::Quote, true, 1), "\u{2026} 1 quoted line");
    }

    // spec: EDIT-101
    #[test]
    fn edit_101_many_folded_blocks_are_found_as_the_scan_finds_them() {
        // the first matching block by kind, title and number, else the one of that kind at the remembered line
        fn by_scan(keys: &[Key], blocks: &[Block]) -> Vec<usize> {
            let mut out: Vec<usize> = Vec::new();
            for k in keys {
                let found = blocks.iter().position(|b| b.kind == k.kind && b.title == k.title && b.nth == k.nth).or_else(|| blocks.iter().position(|b| b.kind == k.kind && b.header == k.line));
                if let Some(i) = found.filter(|i| !out.contains(i)) {
                    out.push(i);
                }
            }
            out.sort_unstable();
            out
        }
        // twenty sections with equal titles, quotes and a list under each
        let text: String = (0..20).map(|n| format!("## Same\ntext {n}\n> quote\n> more\n- a\n- b\n\n")).collect();
        let blocks = find_blocks(&text);
        assert!(blocks.len() > SCAN_KEYS + 8);
        assert_eq!(blocks.iter().filter(|b| b.kind == FoldKind::Heading).map(|b| b.nth).collect::<Vec<_>>(), (0..20).collect::<Vec<_>>());
        let mut folds = NoteFolds::default();
        folds.apply(&blocks, FoldCmd::FoldAll, 0);
        assert!(folds.keys.len() > SCAN_KEYS);
        assert_eq!(folds.folded(&blocks), by_scan(&folds.keys, &blocks));
        assert_eq!(folds.folded(&blocks).len(), blocks.len());
        // after edits: a header edited (the line is the fall-back), a section removed (its key matches nothing), two keys for one block
        for edited in [text.replacen("## Same", "## Other", 1), text.replacen("## Same\ntext 3\n> quote\n> more\n- a\n- b\n\n", "", 1), text.replace("- b", "- c")] {
            let now = find_blocks(&edited);
            assert_eq!(folds.folded(&now), by_scan(&folds.keys, &now), "{edited:?}");
        }
        let mut twice = folds.clone();
        twice.keys.extend(folds.keys.clone());
        assert_eq!(twice.folded(&blocks), by_scan(&twice.keys, &blocks));
    }
}
