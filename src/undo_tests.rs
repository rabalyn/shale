//! Property tests of the undo history (EDIT-021, EDIT-022, DEV-042): whatever is typed, `undo`
//! brings back every checkpoint in reverse order and `redo` walks forward again. The example
//! tests are in `undo.rs`.
//!
//! Rules the properties state, read from `History::checkpoint`: a checkpoint of the text that is
//! already known is no step and drops nothing (the redo steps stay); a checkpoint of another text
//! is one step and drops the redo steps; a note keeps its last `MAX_STEPS` steps, the oldest
//! dropped first.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use proptest::prelude::*;

use crate::property_test_support::{config, config_with};
use crate::undo::{Histories, History, MAX_STEPS};

/// One edit of a person: at the char `pos % (len + 1)` delete up to `del` chars and insert `ins`;
/// `again` checkpoints the result a second time (a repeated, equal checkpoint).
#[derive(Clone, Debug)]
struct Op {
    pos: usize,
    del: usize,
    ins: String,
    again: bool,
}

fn piece() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "[a-c ]{0,3}",
        2 => "[éüß日😀]{0,2}",
        1 => Just("\n".to_string()),
        1 => Just("x\ny\n".to_string()),
        1 => Just(String::new()),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    (0usize..64, 0usize..6, piece(), any::<bool>()).prop_map(|(pos, del, ins, again)| Op { pos, del, ins, again })
}

fn start() -> impl Strategy<Value = String> {
    prop_oneof![Just(String::new()), "[a-c é日😀\n]{0,12}"]
}

/// The text after the edit, cut at char boundaries only.
fn apply(text: &str, op: &Op) -> String {
    let chars: Vec<char> = text.chars().collect();
    let at = op.pos % (chars.len() + 1);
    let end = (at + op.del).min(chars.len());
    let mut out: String = chars[..at].iter().collect();
    out.push_str(&op.ins);
    out.extend(&chars[end..]);
    out
}

/// A history that saw `first` and then every edit, one checkpoint each (two for `again`), and the
/// distinct consecutive texts it saw: what the checkpoints make steps of.
fn typed(first: &str, ops: &[Op]) -> (History, String, Vec<String>) {
    let mut h = History::default();
    let mut text = first.to_string();
    let mut seen = vec![text.clone()];
    h.checkpoint(&text);
    for op in ops {
        text = apply(&text, op);
        h.checkpoint(&text);
        if op.again {
            h.checkpoint(&text);
        }
        if seen.last() != Some(&text) {
            seen.push(text.clone());
        }
    }
    (h, text, seen)
}

/// A cursor position is a char index, and so on a char boundary; it must not be past the end.
fn inside(text: &str, at: usize) -> bool {
    at <= text.chars().count()
}

proptest! {
    #![proptest_config(config())]

    // spec: EDIT-021
    #[test]
    fn edit_021_undo_brings_back_every_checkpoint_in_reverse(first in start(), ops in prop::collection::vec(op(), 0..30)) {
        let (mut h, mut text, seen) = typed(&first, &ops);
        for want in seen.iter().rev().skip(1) {
            let at = h.undo(&mut text);
            prop_assert!(at.is_some(), "a step is left for {:?}", want);
            prop_assert_eq!(&text, want);
            prop_assert!(inside(&text, at.unwrap()), "{} in {:?}", at.unwrap(), text);
        }
        // nothing older: None, and the text stays as it is, however often it is asked
        for _ in 0..2 {
            let before = text.clone();
            prop_assert_eq!(h.undo(&mut text), None);
            prop_assert_eq!(&text, &before);
        }
        prop_assert_eq!(&text, &seen[0]);
    }

    // spec: EDIT-021
    #[test]
    fn edit_021_redo_gives_back_the_text_before_the_undo(first in start(), ops in prop::collection::vec(op(), 1..30), undos in 1usize..32) {
        let (mut h, mut text, seen) = typed(&first, &ops);
        // right after an undo
        if seen.len() > 1 {
            let last = text.clone();
            h.undo(&mut text);
            let at = h.redo(&mut text);
            prop_assert!(at.is_some_and(|a| inside(&text, a)));
            prop_assert_eq!(&text, &last);
            prop_assert_eq!(h.redo(&mut text), None, "nothing newer than the last text");
            prop_assert_eq!(&text, &last);
        }
        // n undos, then as many redos: every text again, in order, up to the last
        let n = undos.min(seen.len() - 1);
        for _ in 0..n {
            h.undo(&mut text);
        }
        for want in &seen[seen.len() - n..] {
            let at = h.redo(&mut text);
            prop_assert!(at.is_some_and(|a| inside(&text, a)));
            prop_assert_eq!(&text, want);
        }
        prop_assert_eq!(&text, seen.last().unwrap());
        prop_assert_eq!(h.redo(&mut text), None);
    }

    // spec: EDIT-021
    #[test]
    fn edit_021_a_new_change_after_undos_drops_the_redo_steps(first in start(), ops in prop::collection::vec(op(), 2..30), undos in 1usize..32, change in op()) {
        let (mut h, mut text, seen) = typed(&first, &ops);
        let n = undos.min(seen.len() - 1);
        for _ in 0..n {
            h.undo(&mut text);
        }
        let here = text.clone();
        // the same text again is no step: the redo steps stay
        h.checkpoint(&text);
        if n > 0 {
            let mut probe = text.clone();
            let mut h2 = typed(&first, &ops).0;
            for _ in 0..n {
                h2.undo(&mut probe);
            }
            prop_assert!(h2.redo(&mut probe).is_some(), "an equal checkpoint keeps the redo steps");
            h2.undo(&mut probe);
        }
        // another text is one step and drops them
        let changed = apply(&text, &change);
        if changed != here {
            h.checkpoint(&changed);
            let mut now = changed.clone();
            prop_assert_eq!(h.redo(&mut now), None, "the change dropped the redo steps");
            prop_assert_eq!(&now, &changed);
            let at = h.undo(&mut now);
            prop_assert!(at.is_some_and(|a| inside(&now, a)));
            prop_assert_eq!(&now, &here, "the new change is one step");
        }
    }
}

proptest! {
    // a case types more than 200 steps, so fewer of them
    #![proptest_config(config_with(48))]

    // spec: EDIT-021
    #[test]
    fn edit_021_only_the_last_200_steps_are_kept_oldest_dropped_first(first in start(), ops in prop::collection::vec(op(), MAX_STEPS + 1..MAX_STEPS + 40)) {
        let (mut h, mut text, seen) = typed(&first, &ops);
        let steps = seen.len() - 1;
        let kept = steps.min(MAX_STEPS);
        for k in 1..=kept {
            prop_assert!(h.undo(&mut text).is_some());
            prop_assert_eq!(&text, &seen[steps - k]);
        }
        prop_assert_eq!(h.undo(&mut text), None, "nothing older than the cap");
        prop_assert_eq!(&text, &seen[steps - kept]);
        // the redo steps are not capped below what was undone
        for k in 1..=kept {
            prop_assert!(h.redo(&mut text).is_some());
            prop_assert_eq!(&text, &seen[steps - kept + k]);
        }
    }
}

static CASE: AtomicUsize = AtomicUsize::new(0);

proptest! {
    // each case writes and reads a real folder, so fewer of them
    #![proptest_config(config_with(64))]

    // spec: EDIT-022
    #[test]
    fn edit_022_what_was_written_is_read_back_the_same(first in start(), ops in prop::collection::vec(op(), 0..20), undos in 0usize..21) {
        let root = std::env::temp_dir().join(format!("shale-undo-prop-{}-{}", std::process::id(), CASE.fetch_add(1, Ordering::Relaxed)));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let rel = Path::new("dir/n.md");
        let now = 1_800_000_000;

        let mut a = Histories::default();
        a.open_vault(&root, now);
        let mut text = first.clone();
        a.of(rel).checkpoint(&text);
        let mut seen = vec![text.clone()];
        for op in &ops {
            text = apply(&text, op);
            a.of(rel).checkpoint(&text);
            if seen.last() != Some(&text) {
                seen.push(text.clone());
            }
        }
        let n = undos.min(seen.len() - 1);
        for _ in 0..n {
            a.of(rel).undo(&mut text);
        }
        a.flush_all();

        // the app restarts: the undone steps redo, then the whole history undoes again
        let mut b = Histories::default();
        b.open_vault(&root, now + 60);
        let at = seen.len() - 1 - n;
        prop_assert_eq!(&text, &seen[at]);
        for want in &seen[at + 1..] {
            let pos = b.of(rel).redo(&mut text);
            prop_assert!(pos.is_some_and(|p| inside(&text, p)));
            prop_assert_eq!(&text, want);
        }
        prop_assert_eq!(b.of(rel).redo(&mut text), None);
        for want in seen[..seen.len() - 1].iter().rev() {
            prop_assert!(b.of(rel).undo(&mut text).is_some());
            prop_assert_eq!(&text, want);
        }
        prop_assert_eq!(b.of(rel).undo(&mut text), None);
        prop_assert!(b.take_problems().is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
