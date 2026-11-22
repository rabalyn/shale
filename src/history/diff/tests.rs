use super::*;

fn spans(rows: &[Span]) -> Vec<(Change, &str)> {
    rows.iter().map(|s| (s.change, s.text.as_str())).collect()
}

// spec: HIST-016
#[test]
fn hist_016_edits_find_the_common_parts_and_the_changed_middle() {
    let a = ["a", "b", "c", "d"];
    let b = ["a", "x", "c", "d", "e"];
    assert_eq!(edits(&a, &b), [Edit::Same(0, 0), Edit::Del(1), Edit::Ins(1), Edit::Same(2, 2), Edit::Same(3, 3), Edit::Ins(4)]);
    assert_eq!(edits::<u8>(&[], &[]), []);
    assert_eq!(edits(&[1], &[]), [Edit::Del(0)]);
    assert_eq!(edits(&[], &[1]), [Edit::Ins(0)]);
    // a moved element is a removal and an insertion
    assert_eq!(edits(&["a", "b", "c"], &["b", "c", "a"]).iter().filter(|e| matches!(e, Edit::Same(..))).count(), 2);
}

// spec: HIST-016
#[test]
fn hist_016_a_middle_part_that_is_too_big_is_removed_and_inserted_whole() {
    let a: Vec<u32> = (0..2500).collect();
    let b: Vec<u32> = (5000..7500).collect();
    let e = edits(&a, &b);
    assert_eq!(e.len(), 5000);
    assert!(e[..2500].iter().all(|x| matches!(x, Edit::Del(_))) && e[2500..].iter().all(|x| matches!(x, Edit::Ins(_))));
    // the common start and end are still found
    let mut c = a.clone();
    c.insert(0, 99_999);
    c.push(88_888);
    let mut d = b.clone();
    d.insert(0, 99_999);
    d.push(88_888);
    let e = edits(&c, &d);
    assert_eq!(e[0], Edit::Same(0, 0));
    assert_eq!(*e.last().unwrap(), Edit::Same(2501, 2501));
}

// spec: HIST-009
#[test]
fn hist_009_words_inserted_and_deleted_are_marked_and_the_rest_is_kept() {
    let s = word_spans("the quick brown fox", "the slow brown fox jumps");
    assert_eq!(spans(&s), [(Change::Same, "the "), (Change::Del, "quick"), (Change::Ins, "slow"), (Change::Same, " brown fox"), (Change::Ins, " jumps")]);
    let joined_old: String = s.iter().filter(|x| x.change != Change::Ins).map(|x| x.text.as_str()).collect();
    let joined_new: String = s.iter().filter(|x| x.change != Change::Del).map(|x| x.text.as_str()).collect();
    assert_eq!(joined_old, "the quick brown fox", "the old text is the same and deleted spans");
    assert_eq!(joined_new, "the slow brown fox jumps", "the new text is the same and inserted spans");
    assert!(s.iter().any(|x| x.change == Change::Del && x.text.contains("quick")));
    assert!(s.iter().any(|x| x.change == Change::Ins && x.text.contains("slow")));
    assert_eq!(spans(&word_spans("same", "same")), [(Change::Same, "same")]);
}

// spec: HIST-009
#[test]
fn hist_009_markdown_marks_are_taken_off_the_text() {
    assert_eq!(plain("a **bold** and *em* and `code` and ~~gone~~ and ==marked=="), "a bold and em and code and gone and marked");
    assert_eq!(plain("see [[Other note|the other]] and [[Plain]] and [site](https://x.y)"), "see the other and Plain and site");
    assert_eq!(plain("![[cat.png]] and ![alt](a.png)"), "\u{25a3} cat.png and \u{25a3} alt");
    assert_eq!(plain("2 * 3 = 6 and snake_case"), "2 * 3 = 6 and snake_case");
    assert_eq!(plain("[[unclosed"), "[[unclosed");
}

// spec: HIST-009
#[test]
fn hist_009_a_note_is_split_into_headings_paragraphs_items_rows_quotes_and_code() {
    let text = "---\ntags: a\n---\n# Title\n\nFirst line\nsecond line.\n\n- one\n  - nested **bold**\n1. numbered\n- [ ] todo\n- [x] done\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quoted\n\n```rust\nlet x = 1;\n```\n\n---\n## Two\n";
    let b = blocks(text);
    let got: Vec<(Kind, &str)> = b.iter().map(|x| (x.kind.clone(), x.text.as_str())).collect();
    assert_eq!(
        got,
        [
            (Kind::Property, "tags: a"),
            (Kind::Heading(1), "Title"),
            (Kind::Paragraph, "First line second line."),
            (Kind::Item(0, "\u{2022}".into()), "one"),
            (Kind::Item(1, "\u{2022}".into()), "nested bold"),
            (Kind::Item(0, "1.".into()), "numbered"),
            (Kind::Item(0, "\u{2022}".into()), "\u{2610} todo"),
            (Kind::Item(0, "\u{2022}".into()), "\u{2611} done"),
            (Kind::TableRow, "a  \u{2502}  b"),
            (Kind::TableRow, "1  \u{2502}  2"),
            (Kind::Quote, "quoted"),
            (Kind::Code, "let x = 1;"),
            (Kind::Rule, ""),
            (Kind::Heading(2), "Two"),
        ]
    );
    assert!(blocks("").is_empty());
    assert_eq!(blocks("#nospace").len(), 1, "a hash without a space is text");
}

fn marks(rows: &[Row]) -> Vec<Mark> {
    rows.iter().map(|r| r.mark).collect()
}

// spec: HIST-009, HIST-011
#[test]
fn hist_009_paragraphs_headings_lists_and_tables_are_compared_block_by_block() {
    let old = "# Plan\n\nWe go to the lake on Friday.\n\n- tent\n- boat\n\n| item | n |\n|---|---|\n| tent | 1 |\n\nBye.\n";
    let new = "# Plan B\n\nWe go to the lake on Saturday.\n\n- tent\n- boat\n- food\n\n| item | n |\n|---|---|\n| tent | 2 |\n\nBye.\n";
    let rows = compare(old, new);
    assert_eq!(marks(&rows), [Mark::Changed, Mark::Changed, Mark::Same, Mark::Same, Mark::Added, Mark::Same, Mark::Changed, Mark::Same]);
    assert_eq!(rows[0].kind, Kind::Heading(1));
    assert_eq!(spans(&rows[0].spans), [(Change::Same, "Plan"), (Change::Ins, " B")]);
    assert!(rows[1].spans.iter().any(|s| s.change == Change::Del && s.text.contains("Friday")));
    assert!(rows[1].spans.iter().any(|s| s.change == Change::Ins && s.text.contains("Saturday")));
    assert_eq!(rows[4].kind, Kind::Item(0, "\u{2022}".into()));
    assert_eq!(spans(&rows[4].spans), [(Change::Ins, "food")]);
    assert_eq!(rows[6].kind, Kind::TableRow);
}

// spec: HIST-009
#[test]
fn hist_009_a_deleted_paragraph_is_wholly_deleted_and_a_new_one_wholly_inserted() {
    let rows = compare("one\n\ntwo\n\nthree\n", "one\n\nthree\n\nfour is here\n");
    assert_eq!(marks(&rows), [Mark::Same, Mark::Removed, Mark::Same, Mark::Added]);
    assert_eq!(spans(&rows[1].spans), [(Change::Del, "two")]);
    assert_eq!(spans(&rows[3].spans), [(Change::Ins, "four is here")]);
    assert!(compare("", "").is_empty());
    assert_eq!(marks(&compare("", "new\n")), [Mark::Added]);
    assert_eq!(marks(&compare("old\n", "")), [Mark::Removed]);
}

// spec: HIST-009, HIST-011
#[test]
fn hist_011_a_paragraph_that_became_a_list_item_is_a_change_of_its_kind() {
    let rows = compare("buy milk\n", "- buy milk\n");
    assert_eq!(marks(&rows), [Mark::Changed]);
    assert_eq!(rows[0].kind, Kind::Item(0, "\u{2022}".into()));
}

fn paragraphs(n: usize, from: usize) -> String {
    (from..from + n).map(|i| format!("paragraph number {i}\n\n")).collect()
}

// spec: HIST-010
#[test]
fn hist_010_long_unchanged_runs_fold_and_keep_one_row_next_to_each_change() {
    let old = format!("start\n\n{}end\n", paragraphs(34, 0));
    let new = format!("start changed\n\n{}end changed\n", paragraphs(34, 0));
    let rows = compare(&old, &new);
    assert_eq!(rows.len(), 36);
    let shown = fold(&rows, &HashSet::new());
    assert_eq!(shown, [Shown::Row(0), Shown::Row(1), Shown::Fold { from: 2, to: 34 }, Shown::Row(34), Shown::Row(35)]);
    assert_eq!(Shown::fold_text(&rows, 2, 34), "\u{2026} 32 unchanged paragraphs");
    // opened: everything
    let open: HashSet<usize> = [2].into();
    assert_eq!(fold(&rows, &open).len(), 36);
    // a short run is not folded
    let short = compare("a\n\nb\n\nc\n\nd\n", "A\n\nb\n\nc\n\nD\n");
    assert_eq!(fold(&short, &HashSet::new()).len(), short.len());
    // a run at the start keeps one row before the change, none before it
    let tail = compare(&format!("{}x here\n", paragraphs(10, 0)), &format!("{}y here\n", paragraphs(10, 0)));
    assert_eq!(fold(&tail, &HashSet::new()), [Shown::Fold { from: 0, to: 9 }, Shown::Row(9), Shown::Row(10)]);
    assert_eq!(Shown::fold_text(&tail, 0, 9), "\u{2026} 9 unchanged paragraphs");
    // nothing changed: one fold of all, and singular
    let same = compare(&paragraphs(5, 0), &paragraphs(5, 0));
    assert_eq!(fold(&same, &HashSet::new()), [Shown::Fold { from: 0, to: 5 }]);
    let one = compare("# a\n\nx\n\ny\n\nz\n\nw\n", "# a\n\nx\n\ny\n\nz\n\nw\n\nnew\n");
    assert_eq!(Shown::fold_text(&one, 0, 1), "\u{2026} 1 unchanged heading");
    assert_eq!(Shown::fold_text(&one, 0, 3), "\u{2026} 3 unchanged parts");
}

// spec: HIST-014
#[test]
fn hist_014_the_changes_start_at_the_first_row_of_each_run() {
    let rows = compare("a\n\nb x\n\nc x\n\nd\n\ne x\n", "a\n\nB x\n\nC x\n\nd\n\nE x\n");
    assert_eq!(change_starts(&rows), [1, 4]);
    assert!(change_starts(&compare("a\n", "a\n")).is_empty());
}

// spec: HIST-012
#[test]
fn hist_012_the_source_is_side_by_side_with_line_numbers_and_word_highlights() {
    let rows = side_by_side("one\ntwo words here\nthree\n", "one\ntwo other here\nthree\nfour\n");
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].mark, Mark::Same);
    assert_eq!(rows[0].left.as_ref().unwrap().0, 1);
    let changed = &rows[1];
    assert_eq!(changed.mark, Mark::Changed);
    let (l, r) = (changed.left.as_ref().unwrap(), changed.right.as_ref().unwrap());
    assert_eq!((l.0, r.0), (2, 2));
    assert!(l.1.iter().any(|s| s.change == Change::Del && s.text.contains("words")));
    assert!(r.1.iter().any(|s| s.change == Change::Ins && s.text.contains("other")));
    assert!(!l.1.iter().any(|s| s.change == Change::Ins) && !r.1.iter().any(|s| s.change == Change::Del));
    assert_eq!(rows[3].mark, Mark::Added);
    assert!(rows[3].left.is_none());
    assert_eq!(rows[3].right.as_ref().unwrap().0, 4);
    let gone = side_by_side("a\nb\n", "a\n");
    assert_eq!(gone[1].mark, Mark::Removed);
    assert!(gone[1].right.is_none());
}

// spec: HIST-013
#[test]
fn hist_013_the_patch_is_unified_diff_text_with_three_lines_of_context() {
    let old: String = (1..=12).map(|i| format!("line {i}\n")).collect();
    let new = old.replace("line 6\n", "line six\n");
    let p = patch("Note.md (11:05)", "Note.md (now)", &old, &new);
    assert_eq!(p, "--- Note.md (11:05)\n+++ Note.md (now)\n@@ -3,7 +3,7 @@\n line 3\n line 4\n line 5\n-line 6\n+line six\n line 7\n line 8\n line 9\n");
    assert_eq!(patch("a", "b", "x\n", "x\n"), "", "no change, no patch");
    // two far changes make two hunks; a change at the start and an added line at the end
    let two = patch("a", "b", &old, &old.replace("line 1\n", "first\n").replace("line 12\n", "line 12\nline 13\n"));
    assert_eq!(two.matches("@@ -").count(), 2, "{two}");
    assert!(two.contains("@@ -1,4 +1,4 @@") && two.contains("+line 13"), "{two}");
    // from nothing: starts at 0
    assert_eq!(patch("a", "b", "", "new\n"), "--- a\n+++ b\n@@ -0,0 +1,1 @@\n+new\n");
    assert_eq!(patch("a", "b", "gone\n", ""), "--- a\n+++ b\n@@ -1,1 +0,0 @@\n-gone\n");
}

// spec: HIST-014
#[test]
fn hist_014_the_changes_of_the_source_begin_at_the_first_row_of_each_run() {
    let rows = side_by_side("a\nb\nc\nd\ne\n", "a\nB\nC\nd\nE\nf\n");
    assert_eq!(side_change_starts(&rows), [1, 4]);
}

// ---------------------------------------------------------------------------------------------
// Property tests (DEV-042): the rules that hold for every pair of inputs. The generators draw from
// a small alphabet, so that the two sides have many elements in common.

mod properties {
    use std::collections::HashSet;

    use proptest::prelude::*;

    use super::super::*;
    use crate::property_test_support::config;

    /// A sequence over a small alphabet (many matches), up to 24 long.
    fn seq() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(0u8..4, 0..24)
    }

    /// Text made of a few words, runs of white space (also a newline and a non-ASCII word).
    fn word_text() -> impl Strategy<Value = String> {
        let piece = prop::sample::select(vec!["a", "b", "ab", "é", " ", "  ", "\t", "\n", "x y"]);
        prop::collection::vec(piece, 0..12).prop_map(|p| p.concat())
    }

    /// Text of lines, some of them with Markdown marks (headings, items, table rows, quotes, code
    /// fences, rules, blank lines), so that the block diff sees every kind.
    fn note_text() -> impl Strategy<Value = String> {
        let line = prop::sample::select(vec!["alpha beta", "alpha gamma", "beta", "# Head", "## Head two", "- item", "  - nested item", "1. first", "| a | b |", "|---|---|", "> quote", "```", "---", "", "", "**bold** word"]);
        (prop::collection::vec(line, 0..14), any::<bool>()).prop_map(|(l, nl)| l.join("\n") + if nl { "\n" } else { "" })
    }

    /// Plain source lines with words from a small set, so that changed lines pair up.
    fn line_text() -> impl Strategy<Value = String> {
        let line = prop::sample::select(vec!["a b c", "a b d", "c", "a  b", " a", "", "x", "y z", "a", "b"]);
        (prop::collection::vec(line, 0..16), any::<bool>()).prop_map(|(l, nl)| l.join("\n") + if nl { "\n" } else { "" })
    }

    /// The length of the longest common subsequence, the slow and plain way.
    fn naive_lcs(a: &[u8], b: &[u8]) -> usize {
        let mut t = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for i in 0..a.len() {
            for j in 0..b.len() {
                t[i + 1][j + 1] = if a[i] == b[j] { t[i][j] + 1 } else { t[i][j + 1].max(t[i + 1][j]) };
            }
        }
        t[a.len()][b.len()]
    }

    /// The text of the spans that are not of the `skip` kind.
    fn side_text(spans: &[Span], skip: Change) -> String {
        spans.iter().filter(|s| s.change != skip).map(|s| s.text.as_str()).collect()
    }

    fn rows_of(marks: &[Mark]) -> Vec<Row> {
        marks.iter().map(|&mark| Row { kind: Kind::Paragraph, mark, spans: vec![] }).collect()
    }

    fn mark_list() -> impl Strategy<Value = Vec<Mark>> {
        prop::collection::vec(prop_oneof![6 => Just(Mark::Same), 1 => Just(Mark::Added), 1 => Just(Mark::Removed), 1 => Just(Mark::Changed)], 0..40)
    }

    /// The first row of every run of changed rows, found the plain way.
    fn runs(changed: &[bool]) -> Vec<usize> {
        let mut out = vec![];
        let mut in_run = false;
        for (i, &c) in changed.iter().enumerate() {
            if c && !in_run {
                out.push(i);
            }
            in_run = c;
        }
        out
    }

    /// A small unified-diff applier: the header, hunks with counts that match their lines, context
    /// and removed lines that match `old`, hunks in order and apart; the rest of `old` is copied.
    fn apply_patch(old: &str, text: &str) -> Result<Vec<String>, String> {
        let a: Vec<&str> = old.lines().collect();
        let mut lines = text.lines();
        let (from, to) = (lines.next().ok_or("empty")?, lines.next().ok_or("no +++")?);
        if !from.starts_with("--- ") || !to.starts_with("+++ ") {
            return Err(format!("bad header {from:?} {to:?}"));
        }
        let mut out: Vec<String> = vec![];
        let mut at = 0; // the next line of `old`
        let mut rest = lines.peekable();
        while let Some(head) = rest.next() {
            let h = head.strip_prefix("@@ -").and_then(|h| h.strip_suffix(" @@")).ok_or(format!("bad hunk header {head:?}"))?;
            let (l, r) = h.split_once(" +").ok_or("no +")?;
            let range = |s: &str| -> Result<(usize, usize), String> {
                let (s, c) = s.split_once(',').ok_or("no count")?;
                Ok((s.parse().map_err(|_| "start")?, c.parse().map_err(|_| "count")?))
            };
            let ((sa, ca), (sb, cb)) = (range(l)?, range(r)?);
            let first = if ca == 0 { sa } else { sa - 1 };
            if first < at || first > a.len() {
                return Err(format!("hunk at {first} but at {at}"));
            }
            out.extend(a[at..first].iter().map(|s| s.to_string()));
            if sb != out.len() + usize::from(cb > 0) {
                return Err(format!("new start {sb} but {} lines are out", out.len()));
            }
            at = first;
            let (mut na, mut nb) = (0, 0);
            while rest.peek().is_some_and(|l| !l.starts_with("@@ ")) {
                let body = rest.next().unwrap_or_default();
                let (mark, line) = body.split_at(1);
                match mark {
                    " " | "-" => {
                        if a.get(at) != Some(&line) {
                            return Err(format!("line {} is {:?}, the patch says {line:?}", at + 1, a.get(at)));
                        }
                        at += 1;
                        na += 1;
                        if mark == " " {
                            out.push(line.to_string());
                            nb += 1;
                        }
                    }
                    "+" => {
                        out.push(line.to_string());
                        nb += 1;
                    }
                    _ => return Err(format!("bad line {body:?}")),
                }
            }
            if (na, nb) != (ca, cb) {
                return Err(format!("counts {ca},{cb} but {na},{nb} lines"));
            }
        }
        out.extend(a[at..].iter().map(|s| s.to_string()));
        Ok(out)
    }

    proptest! {
        #![proptest_config(config())]

        // spec: HIST-016
        #[test]
        fn hist_016_the_edit_script_turns_a_into_b_with_every_element_once_and_in_order(a in seq(), b in seq()) {
            let (mut next_a, mut next_b) = (0, 0);
            let mut built = vec![];
            for e in edits(&a, &b) {
                match e {
                    Edit::Same(i, j) => {
                        prop_assert_eq!((i, j), (next_a, next_b), "both indexes grow by one");
                        prop_assert_eq!(a[i], b[j], "a common element is equal on both sides");
                        built.push(a[i]);
                        next_a += 1;
                        next_b += 1;
                    }
                    Edit::Del(i) => {
                        prop_assert_eq!(i, next_a);
                        next_a += 1;
                    }
                    Edit::Ins(j) => {
                        prop_assert_eq!(j, next_b);
                        built.push(b[j]);
                        next_b += 1;
                    }
                }
            }
            // nothing is left over on either side, and applying the script gives `b`
            prop_assert_eq!((next_a, next_b), (a.len(), b.len()));
            prop_assert_eq!(built, b);
        }

        // spec: HIST-016
        #[test]
        fn hist_016_the_edit_script_keeps_as_many_elements_as_the_longest_common_subsequence(a in seq(), b in seq()) {
            let same = edits(&a, &b).iter().filter(|e| matches!(e, Edit::Same(..))).count();
            prop_assert_eq!(same, naive_lcs(&a, &b));
        }

        // spec: HIST-016
        #[test]
        fn hist_016_within_a_changed_part_removed_elements_come_before_inserted_ones(a in seq(), b in seq()) {
            let e = edits(&a, &b);
            prop_assert!(!e.windows(2).any(|w| matches!((w[0], w[1]), (Edit::Ins(_), Edit::Del(_)))), "{:?}", e);
        }

        // spec: HIST-016
        #[test]
        fn hist_016_equal_sequences_are_all_common(a in seq()) {
            let e = edits(&a, &a);
            prop_assert_eq!(e, (0..a.len()).map(|i| Edit::Same(i, i)).collect::<Vec<_>>());
        }

        // spec: HIST-009
        #[test]
        fn hist_009_the_words_of_both_texts_are_in_the_spans_once_and_in_order(old in word_text(), new in word_text()) {
            let s = word_spans(&old, &new);
            prop_assert_eq!(side_text(&s, Change::Ins), old.clone(), "the old text is the same and deleted spans");
            prop_assert_eq!(side_text(&s, Change::Del), new.clone(), "the new text is the same and inserted spans");
            prop_assert!(s.iter().all(|x| !x.text.is_empty()), "no empty span");
            prop_assert!(s.windows(2).all(|w| w[0].change != w[1].change), "neighbouring spans of one kind are joined");
            if old == new {
                prop_assert!(s.iter().all(|x| x.change == Change::Same));
            }
        }

        // spec: HIST-009
        #[test]
        fn hist_009_every_block_of_both_notes_is_in_the_comparison_once_and_in_order(old in note_text(), new in note_text()) {
            let (a, b) = (blocks(&old), blocks(&new));
            let rows = compare(&old, &new);
            // the new side: every row but a removed one, with the kind and text of the new block
            let new_rows: Vec<&Row> = rows.iter().filter(|r| r.mark != Mark::Removed).collect();
            prop_assert_eq!(new_rows.len(), b.len());
            for (r, blk) in new_rows.iter().zip(&b) {
                prop_assert_eq!(&r.kind, &blk.kind);
                prop_assert_eq!(side_text(&r.spans, Change::Del), blk.text.clone());
            }
            // the old side: every row but an added one; the kind too unless the row is a change
            // (it carries the new kind)
            let old_rows: Vec<&Row> = rows.iter().filter(|r| r.mark != Mark::Added).collect();
            prop_assert_eq!(old_rows.len(), a.len());
            for (r, blk) in old_rows.iter().zip(&a) {
                prop_assert_eq!(side_text(&r.spans, Change::Ins), blk.text.clone());
                if r.mark != Mark::Changed {
                    prop_assert_eq!(&r.kind, &blk.kind);
                }
            }
            // the mark says what the spans say
            for r in &rows {
                let want = match r.mark {
                    Mark::Same => Some(Change::Same),
                    Mark::Added => Some(Change::Ins),
                    Mark::Removed => Some(Change::Del),
                    Mark::Changed => None,
                };
                if let Some(c) = want {
                    prop_assert!(r.spans.iter().all(|s| s.change == c), "{:?}", r);
                }
            }
        }

        // spec: HIST-009
        #[test]
        fn hist_009_a_note_compared_with_itself_has_no_change(text in note_text()) {
            let rows = compare(&text, &text);
            prop_assert!(rows.iter().all(|r| r.mark == Mark::Same));
            prop_assert!(change_starts(&rows).is_empty());
        }

        // spec: HIST-012
        #[test]
        fn hist_012_every_line_of_both_texts_is_in_the_side_by_side_rows_once_and_in_order(old in line_text(), new in line_text()) {
            let (a, b): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
            let rows = side_by_side(&old, &new);
            let left: Vec<&(usize, Vec<Span>)> = rows.iter().filter_map(|r| r.left.as_ref()).collect();
            let right: Vec<&(usize, Vec<Span>)> = rows.iter().filter_map(|r| r.right.as_ref()).collect();
            prop_assert_eq!(left.iter().map(|l| l.0).collect::<Vec<_>>(), (1..=a.len()).collect::<Vec<_>>());
            prop_assert_eq!(right.iter().map(|r| r.0).collect::<Vec<_>>(), (1..=b.len()).collect::<Vec<_>>());
            for (n, spans) in left {
                prop_assert_eq!(side_text(spans, Change::Ins), a[n - 1]);
                prop_assert!(spans.iter().all(|s| s.change != Change::Ins));
            }
            for (n, spans) in right {
                prop_assert_eq!(side_text(spans, Change::Del), b[n - 1]);
                prop_assert!(spans.iter().all(|s| s.change != Change::Del));
            }
            // the mark says which sides are there, and a same row is the same line on both
            for r in &rows {
                prop_assert_eq!((r.left.is_some(), r.right.is_some()), match r.mark {
                    Mark::Added => (false, true),
                    Mark::Removed => (true, false),
                    Mark::Same | Mark::Changed => (true, true),
                });
                if r.mark == Mark::Same {
                    prop_assert_eq!(r.left.as_ref().map(|l| &l.1), r.right.as_ref().map(|l| &l.1));
                }
            }
        }

        // spec: HIST-013
        #[test]
        fn hist_013_applying_the_patch_to_the_old_text_gives_the_new_text(old in line_text(), new in line_text()) {
            let p = patch("old.md", "new.md", &old, &new);
            let want: Vec<String> = new.lines().map(str::to_string).collect();
            prop_assert_eq!(apply_patch(&old, &p), Ok(want), "{}", p);
            // no change, no patch; a change has the two labels
            prop_assert_eq!(p.is_empty(), old.lines().eq(new.lines()));
            if !p.is_empty() {
                prop_assert!(p.starts_with("--- old.md\n+++ new.md\n@@ -"));
            }
        }

        // spec: HIST-013
        #[test]
        fn hist_013_a_hunk_has_no_more_than_three_lines_of_context_at_each_end(old in line_text(), new in line_text()) {
            let p = patch("a", "b", &old, &new);
            for hunk in p.split("\n@@ ").map(|h| h.lines().skip_while(|l| l.starts_with("--- ") || l.starts_with("+++ ") || l.starts_with("@@")).collect::<Vec<_>>()) {
                if hunk.is_empty() {
                    continue;
                }
                prop_assert!(hunk.iter().take_while(|l| l.starts_with(' ')).count() <= 3, "{:?}", hunk);
                prop_assert!(hunk.iter().rev().take_while(|l| l.starts_with(' ')).count() <= 3, "{:?}", hunk);
                prop_assert!(hunk.iter().any(|l| !l.starts_with(' ')), "a hunk has a change: {:?}", hunk);
            }
        }

        // spec: HIST-010
        #[test]
        fn hist_010_folding_hides_only_unchanged_rows_and_shows_everything_else_in_order(marks in mark_list(), open in prop::collection::hash_set(0usize..40, 0..6)) {
            let rows = rows_of(&marks);
            let mut next = 0;
            for s in fold(&rows, &open) {
                match s {
                    Shown::Row(i) => {
                        prop_assert_eq!(i, next);
                        next += 1;
                    }
                    Shown::Fold { from, to } => {
                        prop_assert_eq!(from, next);
                        prop_assert!(from < to && to <= rows.len());
                        prop_assert!(rows[from..to].iter().all(|r| r.mark == Mark::Same), "only unchanged rows are folded");
                        prop_assert!(!open.contains(&from), "an open run is shown whole");
                        let prefix = format!("\u{2026} {} unchanged ", to - from);
                        prop_assert!(Shown::fold_text(&rows, from, to).starts_with(&prefix));
                        next = to;
                    }
                }
            }
            prop_assert_eq!(next, rows.len(), "every row is shown or folded");
        }

        // spec: HIST-010
        #[test]
        fn hist_010_a_run_of_more_than_three_unchanged_rows_folds_all_but_one_row_next_to_each_change(marks in mark_list()) {
            let rows = rows_of(&marks);
            let shown = fold(&rows, &HashSet::new());
            let mut want = vec![];
            let mut i = 0;
            while i < rows.len() {
                if marks[i] != Mark::Same {
                    want.push(Shown::Row(i));
                    i += 1;
                    continue;
                }
                let start = i;
                while i < rows.len() && marks[i] == Mark::Same {
                    i += 1;
                }
                if i - start > 3 {
                    let (from, to) = (start + usize::from(start > 0), i - usize::from(i < rows.len()));
                    want.extend((start..from).map(Shown::Row));
                    want.push(Shown::Fold { from, to });
                    want.extend((to..i).map(Shown::Row));
                } else {
                    want.extend((start..i).map(Shown::Row));
                }
            }
            prop_assert_eq!(shown, want);
            // with every run open nothing is folded
            let all: HashSet<usize> = (0..rows.len()).collect();
            prop_assert_eq!(fold(&rows, &all), (0..rows.len()).map(Shown::Row).collect::<Vec<_>>());
        }

        // spec: HIST-014
        #[test]
        fn hist_014_the_changes_start_at_the_first_row_of_each_run_of_changed_rows(marks in mark_list()) {
            let changed: Vec<bool> = marks.iter().map(|m| *m != Mark::Same).collect();
            let want = runs(&changed);
            prop_assert_eq!(change_starts(&rows_of(&marks)), want.clone());
            let side: Vec<Side> = marks.iter().map(|&mark| Side { left: None, right: None, mark }).collect();
            prop_assert_eq!(side_change_starts(&side), want.clone());
            // a start is a changed row, and the row before it is unchanged
            prop_assert!(want.iter().all(|&i| changed[i] && (i == 0 || !changed[i - 1])));
            prop_assert!(want.windows(2).all(|w| w[0] < w[1]));
        }

        // spec: HIST-014
        #[test]
        fn hist_014_a_comparison_has_a_change_start_exactly_when_the_texts_differ_in_blocks(old in note_text(), new in note_text()) {
            let rows = compare(&old, &new);
            prop_assert_eq!(change_starts(&rows).is_empty(), rows.iter().all(|r| r.mark == Mark::Same));
            prop_assert_eq!(rows.iter().all(|r| r.mark == Mark::Same), blocks(&old) == blocks(&new));
            let side = side_by_side(&old, &new);
            prop_assert_eq!(side_change_starts(&side).is_empty(), old.lines().eq(new.lines()));
        }
    }
}
