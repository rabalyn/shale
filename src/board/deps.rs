//! Dependencies between cards (BOARD-023…BOARD-028): which cards are blocked, the arrows' ends,
//! and the order and step numbers of a lane that sorts by `dependencies`.
//!
//! The rules of what is blocked are those of the search (`search_all::deps`, QUERY-002); the board
//! only adds what it knows: the lanes that hold finished cards (BOARD-024). [`Graph`] is that
//! over the notes of the vault, [`order`] is the topological order of the cards of one lane.

use super::card::Card;
use super::sort::tie;
use crate::search_all::deps::Deps;
use crate::vault::Vault;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What a card knows of its dependencies (BOARD-023, BOARD-025, BOARD-027, BOARD-028).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dep {
    /// the notes it names in `blocked-by:` that exist, done or not
    pub blockers: Vec<PathBuf>,
    /// those that are not done: while there are some, the card is blocked
    pub waiting: Vec<PathBuf>,
    /// the notes that name it as a blocker
    pub dependants: Vec<PathBuf>,
    /// its place in the dependency order, in a lane that sorts by `dependencies`
    pub step: Option<Step>,
}

impl Dep {
    pub fn blocked(&self) -> bool {
        !self.waiting.is_empty()
    }
}

/// A card's place in the dependency order of its lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// step `n` (from 1); `part` is the place among the cards that share the step (`3a`, `3b`)
    At { n: usize, part: Option<usize> },
    /// in a cycle (BOARD-028)
    Circular,
    /// waits for a card of a cycle: it cannot be ordered either, and it is listed with them
    Behind,
}

impl Step {
    /// `1`, `3a`, `circular`; none for a card that only waits for a circle.
    pub fn label(self) -> Option<String> {
        match self {
            Step::At { n, part: None } => Some(n.to_string()),
            Step::At { n, part: Some(p) } => Some(format!("{n}{}", letters(p))),
            Step::Circular => Some("circular".to_string()),
            Step::Behind => None,
        }
    }
}

/// `a`, `b`, … `z`, `aa`, `ab`, … for the place `p` (from 0).
fn letters(mut p: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'a' + (p % 26) as u8);
        if p < 26 {
            break;
        }
        p = p / 26 - 1;
    }
    out.reverse();
    String::from_utf8(out).expect("ascii")
}

/// The dependencies of the notes of a vault as a board sees them: `blocked-by:` and `status:` as
/// the search does, and the notes in a done lane done (BOARD-024).
pub struct Graph {
    deps: Deps,
    rels: Vec<PathBuf>,
    index: HashMap<PathBuf, usize>,
    dependants: Vec<Vec<usize>>,
}

impl Graph {
    /// `in_done_lane`: the notes that are in a lane marked `done: true`.
    pub fn new(vault: &Vault, in_done_lane: &HashSet<PathBuf>) -> Graph {
        let files: Vec<_> = vault.files.iter().filter(|f| !vault.is_shadow_copy(&f.rel)).collect();
        let items: Vec<(String, Option<&str>)> = files.iter().map(|f| (f.display_name(), vault.notes.get(&f.rel).map(|n| n.text.as_str()))).collect();
        let done: Vec<bool> = files.iter().map(|f| in_done_lane.contains(&f.rel)).collect();
        let deps = Deps::build(&items, &done);
        let rels: Vec<PathBuf> = files.iter().map(|f| f.rel.clone()).collect();
        let index = rels.iter().enumerate().map(|(i, r)| (r.clone(), i)).collect();
        let mut dependants = vec![Vec::new(); rels.len()];
        for i in 0..rels.len() {
            for &b in deps.blockers(i) {
                dependants[b].push(i);
            }
        }
        Graph { deps, rels, index, dependants }
    }

    /// What the note `rel` knows of its dependencies; nothing for a file that is not in the vault.
    pub fn dep(&self, rel: &Path) -> Dep {
        let Some(&i) = self.index.get(rel) else { return Dep::default() };
        let rels = |v: &mut dyn Iterator<Item = usize>| v.map(|j| self.rels[j].clone()).collect();
        Dep { blockers: rels(&mut self.deps.blockers(i).iter().copied()), waiting: rels(&mut self.deps.waiting(i)), dependants: rels(&mut self.dependants[i].iter().copied()), step: None }
    }

    /// Does `a` wait for `b`, through its blockers and theirs? (A new blocker `b` of `a` would then close a
    /// cycle, BOARD-043.)
    pub fn waits_for(&self, a: &Path, b: &Path) -> bool {
        let (Some(&a), Some(&b)) = (self.index.get(a), self.index.get(b)) else { return false };
        let mut seen = vec![false; self.rels.len()];
        let mut todo = vec![a];
        while let Some(i) = todo.pop() {
            for &j in self.deps.blockers(i) {
                if j == b {
                    return true;
                }
                if !std::mem::replace(&mut seen[j], true) {
                    todo.push(j);
                }
            }
        }
        false
    }

    /// For each card the places in `cards` of the cards it waits for, the blockers that are
    /// cards of the same list (the lane); a blocker that is done is one too: done or not, it
    /// comes first.
    pub fn edges(&self, cards: &[Card]) -> Vec<Vec<usize>> {
        let at: HashMap<&Path, usize> = cards.iter().enumerate().map(|(i, c)| (c.rel.as_path(), i)).collect();
        cards.iter().map(|c| self.index.get(&c.rel).map(|&i| self.deps.blockers(i).iter().filter_map(|&b| at.get(self.rels[b].as_path()).copied()).collect()).unwrap_or_default()).collect()
    }
}

/// The dependency order of `cards` (BOARD-026, BOARD-027, BOARD-028). `edges[i]` are the places of
/// the cards that card `i` waits for. Gives the places of the cards in order with their steps:
/// the cards that wait for nothing are step 1, the cards that wait only for those step 2, and
/// so on; the cards of a step come by earlier due day, then higher priority, then title
/// (`sort::tie`), and several in one step are `a`, `b`, … Cards in a cycle, and the cards that
/// wait for one, cannot be ordered: they come last, in that order, the cards of a cycle marked.
pub fn order(cards: &[Card], edges: &[Vec<usize>]) -> Vec<(usize, Step)> {
    let n = cards.len();
    let blockers: Vec<Vec<usize>> = edges
        .iter()
        .map(|e| {
            let mut e = e.clone();
            e.sort_unstable();
            e.dedup();
            e
        })
        .collect();
    let mut waits: Vec<usize> = blockers.iter().map(Vec::len).collect();
    let mut dependants = vec![Vec::new(); n];
    for (i, list) in blockers.iter().enumerate() {
        for &b in list {
            dependants[b].push(i);
        }
    }
    let (mut placed, mut out) = (vec![false; n], Vec::with_capacity(n));
    let mut layer: Vec<usize> = (0..n).filter(|&i| waits[i] == 0).collect();
    let mut step = 0;
    while !layer.is_empty() {
        step += 1;
        layer.sort_by(|&a, &b| tie(&cards[a], &cards[b]));
        let shared = layer.len() > 1;
        let mut next = Vec::new();
        for (k, &i) in layer.iter().enumerate() {
            placed[i] = true;
            out.push((i, Step::At { n: step, part: shared.then_some(k) }));
        }
        for &i in &layer {
            for &d in &dependants[i] {
                waits[d] -= 1;
                if waits[d] == 0 {
                    next.push(d);
                }
            }
        }
        layer = next;
    }
    let mut rest: Vec<usize> = (0..n).filter(|&i| !placed[i]).collect();
    rest.sort_by(|&a, &b| tie(&cards[a], &cards[b]));
    for &i in &rest {
        out.push((i, if in_cycle(i, &blockers, &placed) { Step::Circular } else { Step::Behind }));
    }
    out
}

/// Can card `start` be reached again by following blockers, over the cards not placed?
fn in_cycle(start: usize, blockers: &[Vec<usize>], placed: &[bool]) -> bool {
    let mut seen = vec![false; blockers.len()];
    let mut stack: Vec<usize> = blockers[start].iter().copied().filter(|&b| !placed[b]).collect();
    while let Some(i) = stack.pop() {
        if i == start {
            return true;
        }
        if !std::mem::replace(&mut seen[i], true) {
            stack.extend(blockers[i].iter().copied().filter(|&b| !placed[b]));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::note_props::Priority;
    use crate::property_test_support::config;
    use proptest::prelude::*;

    fn card(title: &str, due: Option<i64>, priority: Option<Priority>) -> Card {
        Card { rel: PathBuf::from(format!("{title}.md")), title: title.into(), drawing: false, due, status: None, priority, tags: vec![], created: None, modified: None, start: None, dep: Dep::default() }
    }

    fn plain(titles: &[&str]) -> Vec<Card> {
        titles.iter().map(|t| card(t, None, None)).collect()
    }

    /// The titles in order with their labels: `A1 B2 C3a`.
    fn shown(cards: &[Card], out: &[(usize, Step)]) -> Vec<String> {
        out.iter().map(|(i, s)| format!("{}{}", cards[*i].title, s.label().unwrap_or_default())).collect()
    }

    // spec: BOARD-027
    #[test]
    fn board_027_the_labels_are_numbers_and_letters_for_the_cards_that_share_a_step() {
        assert_eq!(Step::At { n: 2, part: None }.label().as_deref(), Some("2"));
        assert_eq!(Step::At { n: 3, part: Some(0) }.label().as_deref(), Some("3a"));
        assert_eq!(Step::At { n: 3, part: Some(1) }.label().as_deref(), Some("3b"));
        assert_eq!(Step::At { n: 3, part: Some(26) }.label().as_deref(), Some("3aa"));
        assert_eq!(Step::At { n: 3, part: Some(27) }.label().as_deref(), Some("3ab"));
        assert_eq!(Step::Circular.label().as_deref(), Some("circular"));
        assert_eq!(Step::Behind.label(), None);
    }

    // spec: BOARD-026, BOARD-027
    #[test]
    fn board_026_blockers_come_first_and_cards_that_run_in_parallel_share_a_step() {
        // D waits for B and C, which wait for A; E waits for nothing
        let cards = plain(&["D", "B", "E", "A", "C"]);
        let edges = vec![vec![1, 4], vec![3], vec![], vec![], vec![3]];
        let out = order(&cards, &edges);
        assert_eq!(shown(&cards, &out), ["A1a", "E1b", "B2a", "C2b", "D3"]);
    }

    // spec: BOARD-026
    #[test]
    fn board_026_cards_of_a_step_come_by_due_then_priority_then_title() {
        let cards =
            vec![card("Z", Some(5), Some(Priority::Low)), card("Y", Some(5), Some(Priority::Highest)), card("X", Some(9), Some(Priority::Highest)), card("W", None, Some(Priority::Highest)), card("V", Some(5), Some(Priority::Highest))];
        let out = order(&cards, &vec![vec![]; 5]);
        assert_eq!(shown(&cards, &out), ["V1a", "Y1b", "Z1c", "X1d", "W1e"], "due 5 first (high priority before low, then title), then 9, then no due");
    }

    // spec: BOARD-028
    #[test]
    fn board_028_a_cycle_is_marked_and_listed_after_the_ordered_cards() {
        // B and C wait for each other; D waits for B; E waits for A; A is free
        let cards = plain(&["A", "B", "C", "D", "E"]);
        let edges = vec![vec![], vec![2], vec![1], vec![1], vec![0]];
        let out = order(&cards, &edges);
        assert_eq!(shown(&cards, &out), ["A1", "E2", "Bcircular", "Ccircular", "D"]);
        // a card that waits for itself is a cycle too
        let one = plain(&["S"]);
        assert_eq!(shown(&one, &order(&one, &[vec![0]])), ["Scircular"]);
    }

    /// Cards with titles from a random shuffle, the edges between them random.
    fn graph() -> impl Strategy<Value = (Vec<Card>, Vec<Vec<usize>>)> {
        (1usize..9).prop_flat_map(|n| {
            let cards = prop::collection::vec((prop::option::of(0i64..4), prop::option::of(0usize..5)), n);
            let edges = prop::collection::vec(prop::collection::vec(0..n, 0..4), n);
            (cards, edges).prop_map(|(cards, edges)| {
                let cards = cards.into_iter().enumerate().map(|(i, (due, p))| card(&format!("C{i}"), due, p.map(|p| Priority::ALL[p]))).collect();
                (cards, edges)
            })
        })
    }

    /// Which cards are in a cycle, by the transitive closure.
    fn cyclic(edges: &[Vec<usize>]) -> Vec<bool> {
        let n = edges.len();
        let mut reach = vec![vec![false; n]; n];
        for (i, e) in edges.iter().enumerate() {
            for &b in e {
                reach[i][b] = true;
            }
        }
        for k in 0..n {
            for i in 0..n {
                for j in 0..n {
                    reach[i][j] |= reach[i][k] && reach[k][j];
                }
            }
        }
        (0..n).map(|i| reach[i][i]).collect()
    }

    proptest! {
        #![proptest_config(config())]

        // spec: BOARD-026, BOARD-028 (every card after its blockers, the cycles all marked and last, the order the same for any input order)
        #[test]
        fn board_026_the_order_puts_every_card_after_its_blockers_and_marks_every_cycle((cards, edges) in graph()) {
            let out = order(&cards, &edges);
            let n = cards.len();
            let mut at = vec![usize::MAX; n];
            for (place, (i, _)) in out.iter().enumerate() {
                at[*i] = place;
            }
            prop_assert!(at.iter().all(|a| *a != usize::MAX) && out.len() == n, "every card once");
            let cyc = cyclic(&edges);
            let steps: HashMap<usize, Step> = out.iter().copied().collect();
            for i in 0..n {
                prop_assert_eq!(steps[&i] == Step::Circular, cyc[i], "a card is circular exactly when it is in a cycle");
                if let Step::At { .. } = steps[&i] {
                    for &b in &edges[i] {
                        prop_assert!(at[b] < at[i], "card {i} comes after its blocker {b}");
                        let (Step::At { n: a, .. }, Step::At { n: c, .. }) = (steps[&b], steps[&i]) else { return Err(TestCaseError::fail("a blocker of an ordered card is ordered")) };
                        prop_assert!(a < c, "and in an earlier step");
                    }
                }
            }
            let first_unordered = out.iter().position(|(_, s)| !matches!(s, Step::At { .. })).unwrap_or(n);
            prop_assert!(out[first_unordered..].iter().all(|(_, s)| !matches!(s, Step::At { .. })), "the cards that cannot be ordered are last");
            // the same cards in another input order come in the same order (by title)
            let reversed: Vec<Card> = cards.iter().rev().cloned().collect();
            let back = |i: usize| n - 1 - i;
            let redges: Vec<Vec<usize>> = (0..n).map(|i| edges[back(i)].iter().map(|&b| back(b)).collect()).collect();
            let again: Vec<(String, Step)> = order(&reversed, &redges).iter().map(|(i, s)| (reversed[*i].title.clone(), *s)).collect();
            let first: Vec<(String, Step)> = out.iter().map(|(i, s)| (cards[*i].title.clone(), *s)).collect();
            prop_assert_eq!(first, again);
        }
    }
}
