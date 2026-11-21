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
