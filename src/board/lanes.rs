//! Which cards a lane shows (BOARD-009, BOARD-032, BOARD-011) and how the lanes get their
//! matches from the search worker (QUERY-012, QUERY-019).
//!
//! [`place`] is the rule of the cards: what a lane matches and what it pins, as lists of paths,
//! in; the cards of each lane out. [`Runs`] holds the query of every lane of the open board,
//! asks the worker for the lanes whose result can have changed, and keeps the last answer of
//! each lane while a newer one is on its way.

use crate::search_all::matcher::{Budget, Run};
use crate::search_all::query::{Checked, Expr, QueryError, TermKind};
use crate::search_all::{Request, SearchWorker, Slot, Snapshot};
use crate::search_sources::Prefixes;
use crate::vault::Vault;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

/// What one lane matches and pins, as paths of the vault.
pub struct LaneIn<'a> {
    /// the notes its query matches, in the order of the vault
    pub matches: &'a [PathBuf],
    /// the cards of its links, in the order of the links
    pub pins: &'a [PathBuf],
}

/// A card in a lane, and whether the lane pins it (BOARD-004).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub rel: PathBuf,
    pub pinned: bool,
}

/// The cards of each lane (BOARD-009, BOARD-032). A lane has the cards it pins first, in the order
/// of its links, then the notes its query matches. A card that any lane pins is only in the lanes
/// that pin it. Of the others a card is in the first lane that matches it, or with `every_lane` in
/// every lane that does. The board itself is never one of its cards.
pub fn place(board: &Path, lanes: &[LaneIn], every_lane: bool) -> Vec<Vec<Placed>> {
    let pinned: HashSet<&PathBuf> = lanes.iter().flat_map(|l| l.pins).collect();
    let mut taken: HashSet<&PathBuf> = HashSet::new();
    lanes
        .iter()
        .map(|lane| {
            let mut seen = HashSet::new();
            let mut out: Vec<Placed> = lane.pins.iter().filter(|p| p.as_path() != board && seen.insert(*p)).map(|p| Placed { rel: p.clone(), pinned: true }).collect();
            for m in lane.matches.iter().filter(|m| m.as_path() != board && !pinned.contains(m)) {
                if every_lane || taken.insert(m) {
                    out.push(Placed { rel: m.clone(), pinned: false });
                }
            }
            out
        })
        .collect()
}

/// The property a card dropped on a lane gets when the lane has no `drop:` line (BOARD-019,
/// QUERY-028): a query that is one `key:value` says it, `status:doing` is `status: doing`. The
/// text of the query is read as the lane reads it, so a query that fails the check, or has more
/// than one term, gives none.
pub fn drop_property(query: &str) -> Option<String> {
    let checked = Checked::lane(query, Prefixes::default());
    match checked.expr()? {
        Expr::Term(t) if matches!(t.kind, TermKind::Prop { .. }) => {
            let (key, value) = checked.source(t).split_once(':')?;
            Some(format!("{}: {}", key.trim(), value.trim().trim_matches('"')))
        }
        _ => None,
    }
}

/// A lane to start from, in the menu next to "+ Add lane" (BOARD-029).
pub struct Preset {
    pub name: &'static str,
    pub query: &'static str,
    pub sort: &'static str,
}

/// The lane presets: "My dependency chain" is every note that waits or is waited for, in
/// dependency order (BOARD-029, BOARD-026).
pub const PRESETS: [Preset; 1] = [Preset { name: "My dependency chain", query: "blocked OR blocking", sort: "dependencies" }];

/// What a lane shows of its search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Search<'a> {
    /// no answer yet
    Waiting,
    /// the query does not pass the check: it is shown in the header and the lane is empty (QUERY-012)
    Invalid(&'a QueryError),
    /// the notes it matched; `partial` when the work budget ended the search early (QUERY-010)
    Found { notes: &'a [PathBuf], partial: bool },
}

/// How many notes may change before every lane asks again, without looking at each.
const FEW: usize = 8;
/// What the check of one changed note against a lane may cost.
const CHECK_CHARS: u64 = 2_000_000;
const CHECK_TIME: Duration = Duration::from_millis(20);

/// The notes that differ between two snapshots: changed, added or removed.
struct Changed {
    rels: Vec<PathBuf>,
    /// more than [`FEW`]: every lane asks again
    many: bool,
}

fn changes(old: &Snapshot, new: &Snapshot) -> Changed {
    let before: HashMap<&PathBuf, &Option<Arc<crate::vault::NoteIndex>>> = old.notes.iter().map(|n| (&n.file.rel, &n.note)).collect();
    let mut rels: Vec<PathBuf> = new
        .notes
        .iter()
        .filter(|n| match before.get(&n.file.rel) {
            None => true,
            Some(Some(a)) => n.note.as_ref().is_none_or(|b| !Arc::ptr_eq(a, b)),
            Some(None) => n.note.is_some(),
        })
        .map(|n| n.file.rel.clone())
        .collect();
    let now: HashSet<&PathBuf> = new.notes.iter().map(|n| &n.file.rel).collect();
    rels.extend(old.notes.iter().filter(|n| !now.contains(&n.file.rel)).map(|n| n.file.rel.clone()));
    let many = rels.len() > FEW;
    Changed { rels, many }
}

/// Does a term of the query read other notes than the one it is asked about?
fn reads_other_notes(expr: &Expr) -> bool {
    match expr {
        Expr::And(v) | Expr::Or(v) => v.iter().any(reads_other_notes),
        Expr::Not(e) => reads_other_notes(e),
        Expr::Term(t) => matches!(t.kind, TermKind::Blocked | TermKind::Blocking),
    }
}

/// The last answer of a lane.
struct Answer {
    notes: Vec<PathBuf>,
    partial: bool,
}

/// One lane of the open board: its query text as checked and the search in flight or done.
struct LaneRun {
    query: String,
    checked: Checked,
    /// the number of the search on its way
    asked: Option<u64>,
    /// the last answer; it stays while a newer search is on its way
    answer: Option<Answer>,
}

impl LaneRun {
    fn new(query: &str) -> LaneRun {
        LaneRun { query: query.to_string(), checked: Checked::lane(query, Prefixes::default()), asked: None, answer: None }
    }
}

/// The searches of the lanes of the open board (QUERY-019).
#[derive(Default)]
pub struct Runs {
    board: Option<PathBuf>,
    /// the state of the vault the last snapshot was made for
    stamp: Option<(u64, u64)>,
    snapshot: Option<Arc<Snapshot>>,
    lanes: Vec<LaneRun>,
    /// counts every answer taken in, so that a view that caches what it made of them knows
    pub version: u64,
    /// how many searches were asked for (tests)
    #[cfg(test)]
    pub asked: usize,
}

impl Runs {
    /// Bring the searches in step with the board `board` whose lanes have the queries `queries`
    /// and with the vault as it is now; take in the answers that arrived. A lane asks the worker
    /// when its query is new, and when a note that changed can change its result: the note was
    /// in it, matches it now, or the lane reads other notes (`blocked`, `blocking`); the lanes
    /// no note concerns keep their answer and ask nothing.
    pub fn update(&mut self, worker: &mut SearchWorker, vault: &Vault, board: &Path, queries: &[&str]) {
        // (another board, or another vault: what the lanes knew is not about this one)
        if self.board.as_deref() != Some(board) || self.stamp.is_some_and(|(uid, _)| uid != vault.uid) {
            for i in 0..self.lanes.len() {
                worker.cancel(Slot::lane(i));
            }
            *self = Runs { board: Some(board.to_path_buf()), version: self.version + 1, ..Runs::default() };
        }
        for i in queries.len()..self.lanes.len() {
            worker.cancel(Slot::lane(i));
        }
        self.lanes.truncate(queries.len());
        let stamp = (vault.uid, vault.generation);
        let changed = if self.stamp != Some(stamp) {
            let new = Arc::new(Snapshot::of(vault));
            let changed = self.snapshot.as_deref().map(|old| changes(old, &new));
            self.snapshot = Some(new);
            self.stamp = Some(stamp);
            changed
        } else {
            None
        };
        let Some(snapshot) = self.snapshot.clone() else { return };
        let index: HashMap<&PathBuf, usize> = snapshot.notes.iter().enumerate().map(|(i, n)| (&n.file.rel, i)).collect();
        for (i, query) in queries.iter().enumerate() {
            if self.lanes.len() <= i {
                self.lanes.push(LaneRun::new(query));
                self.version += 1;
                self.ask(worker, &snapshot, i);
            } else if self.lanes[i].query != *query {
                self.lanes[i] = LaneRun::new(query);
                self.version += 1;
                self.ask(worker, &snapshot, i);
            } else if changed.as_ref().is_some_and(|c| self.affected(i, c, &snapshot, &index)) {
                self.ask(worker, &snapshot, i);
            }
            let lane = &mut self.lanes[i];
            if let Some(id) = lane.asked {
                if let Some(found) = worker.take(Slot::lane(i), id) {
                    lane.asked = None;
                    lane.answer = Some(Answer { notes: found.lane, partial: found.stopped_early });
                    self.version += 1;
                }
            }
        }
    }

    /// Ask the worker for lane `i`, unless its query is no query (a manual lane) or fails the
    /// check (QUERY-012): those have no search.
    fn ask(&mut self, worker: &mut SearchWorker, snapshot: &Arc<Snapshot>, i: usize) {
        let lane = &mut self.lanes[i];
        if lane.checked.expr().is_none() {
            worker.cancel(Slot::lane(i));
            lane.asked = None;
            lane.answer = Some(Answer { notes: Vec::new(), partial: false });
            return;
        }
        #[cfg(test)]
        {
            self.asked += 1;
        }
        let lane = &mut self.lanes[i];
        lane.asked = Some(worker.submit(Slot::lane(i), Request::lane(snapshot.clone(), lane.checked.clone())));
    }

    /// Can what changed change the result of lane `i`?
    fn affected(&self, i: usize, changed: &Changed, snapshot: &Snapshot, index: &HashMap<&PathBuf, usize>) -> bool {
        let lane = &self.lanes[i];
        let Some(expr) = lane.checked.expr() else { return false };
        let Some(answer) = &lane.answer else { return true };
        if changed.many || reads_other_notes(expr) {
            return true;
        }
        let never = AtomicBool::new(false);
        let mut run = Run::new(snapshot, &never, Budget::new(CHECK_CHARS, CHECK_TIME));
        changed.rels.iter().any(|rel| answer.notes.contains(rel) || index.get(rel).is_none_or(|n| run.note_matches(expr, *n).unwrap_or(true)))
    }

    /// What lane `i` shows of its search.
    pub fn search(&self, i: usize) -> Search<'_> {
        match self.lanes.get(i) {
            None => Search::Waiting,
            Some(l) => match (l.checked.error(), &l.answer) {
                (Some(e), _) => Search::Invalid(e),
                (None, Some(a)) => Search::Found { notes: &a.notes, partial: a.partial },
                (None, None) => Search::Waiting,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(|n| PathBuf::from(format!("{n}.md"))).collect()
    }

    fn rels(placed: &[Placed]) -> Vec<String> {
        placed.iter().map(|p| p.rel.to_string_lossy().trim_end_matches(".md").to_string() + if p.pinned { "*" } else { "" }).collect()
    }

    // spec: BOARD-009
    #[test]
    fn board_009_a_card_is_in_the_first_lane_that_matches_unless_every_lane_is_set() {
        let (a, b) = (paths(&["X", "Y", "Z"]), paths(&["Y", "Z", "W"]));
        let lanes = [LaneIn { matches: &a, pins: &[] }, LaneIn { matches: &b, pins: &[] }];
        let exclusive = place(Path::new("Board.md"), &lanes, false);
        assert_eq!(rels(&exclusive[0]), ["X", "Y", "Z"]);
        assert_eq!(rels(&exclusive[1]), ["W"]);
        let every = place(Path::new("Board.md"), &lanes, true);
        assert_eq!(rels(&every[0]), ["X", "Y", "Z"]);
        assert_eq!(rels(&every[1]), ["Y", "Z", "W"]);
    }

    // spec: BOARD-009
    #[test]
    fn board_009_the_board_is_not_a_card_of_its_own_lanes() {
        let a = paths(&["Board", "X"]);
        let pins = paths(&["Board"]);
        let lanes = [LaneIn { matches: &a, pins: &pins }];
        assert_eq!(rels(&place(Path::new("Board.md"), &lanes, false)[0]), ["X"]);
    }

    // spec: BOARD-032
    #[test]
    fn board_032_a_pinned_card_is_only_in_the_lane_that_pins_it_and_comes_first() {
        let (a, b, c) = (paths(&["X", "Y", "Z"]), paths(&["Y"]), paths(&["Z", "Y"]));
        let pins = paths(&["Y", "Q"]);
        for every in [false, true] {
            let lanes = [LaneIn { matches: &a, pins: &[] }, LaneIn { matches: &b, pins: &pins }, LaneIn { matches: &c, pins: &[] }];
            let out = place(Path::new("Board.md"), &lanes, every);
            assert_eq!(rels(&out[0]), ["X", "Z"], "Y is pinned in lane 2: not here, though the query matches (every {every})");
            assert_eq!(rels(&out[1]), ["Y*", "Q*"], "the links come in their order, a card that also matches shows once");
            assert_eq!(rels(&out[2]), if every { vec!["Z"] } else { vec![] }, "(every {every})");
        }
        // a card pinned in two lanes is in both
        let (p1, p2) = (paths(&["Y"]), paths(&["Y"]));
        let lanes = [LaneIn { matches: &[], pins: &p1 }, LaneIn { matches: &[], pins: &p2 }];
        let out = place(Path::new("Board.md"), &lanes, false);
        assert_eq!(rels(&out[0]), ["Y*"]);
        assert_eq!(rels(&out[1]), ["Y*"]);
        // the same link twice is one card
        let twice = paths(&["Y", "Y"]);
        assert_eq!(rels(&place(Path::new("Board.md"), &[LaneIn { matches: &[], pins: &twice }], false)[0]), ["Y*"]);
    }
}
