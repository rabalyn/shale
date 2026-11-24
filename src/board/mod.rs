//! Kanban boards (ADR-0034, BOARD-001…BOARD-029, BOARD-032): a board is a note, its lanes are
//! made from queries, its cards are notes.
//!
//! * [`file`] reads and writes the board file and keeps what it does not know.
//! * [`lanes`] says which cards a lane shows and asks the search worker for the lanes.
//! * [`deps`] says which cards are blocked and puts a lane in dependency order.
//! * [`card`] and [`sort`] are what a card knows of its note and the order of a lane.
//! * [`drag`] says what dropping a card on a lane changes.
//! * [`view`] draws the board; [`peek`] is the editor that opens on a card.
//! * `app/boards.rs` is what the window does for them: the tab, the rewrite of the file, the peek.
//!
//! The state of the view is one field of the window, [`Boards`].

pub mod card;
pub mod deps;
pub mod drag;
pub mod file;
pub mod lanes;
pub mod peek;
pub mod sort;
pub mod view;

#[cfg(test)]
mod ui_tests;

use crate::search_form::sort as form_sort;
use crate::vault::{FileKind, Vault};
use card::Card;
use deps::Graph;
use file::Board;
use lanes::{LaneIn, Search};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A board of the vault, for the strip at the top of the view (BOARD-007).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub rel: PathBuf,
    pub title: String,
}

/// Every `type: board` note of the vault, in the order of the files (BOARD-007).
pub fn list(vault: &Vault) -> Vec<Info> {
    vault.files.iter().filter(|f| f.kind == FileKind::Note && !vault.is_shadow_copy(&f.rel)).filter(|f| vault.notes.get(&f.rel).is_some_and(|n| file::is_board(&n.text))).map(|f| Info { rel: f.rel.clone(), title: f.title() }).collect()
}

/// What the view draws of one lane.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneView {
    pub name: String,
    pub query: String,
    pub sort: String,
    pub drop: String,
    pub done: bool,
    /// why the query is refused (QUERY-012), or the sort
    pub error: Option<String>,
    /// the search has no answer yet
    pub waiting: bool,
    /// the work budget ended the search early (QUERY-010)
    pub partial: bool,
    /// the cards in the order they are shown: the pinned ones first
    pub cards: Vec<Card>,
    /// how many of the cards are pinned
    pub pinned: usize,
}

/// The lanes of `board` as they are shown now: the matches of every lane placed (BOARD-009,
/// BOARD-032) and the cards of each sorted (BOARD-015, BOARD-018).
pub fn lane_views(vault: &Vault, rel: &Path, board: &Board, every_lane: bool, runs: &lanes::Runs) -> Vec<LaneView> {
    let pins: Vec<Vec<PathBuf>> = board.lanes.iter().map(|l| l.pins().into_iter().filter_map(|p| vault.resolve(file::pin_target(p), Some(rel))).collect()).collect();
    let none = Vec::new();
    let found: Vec<&[PathBuf]> = (0..board.lanes.len())
        .map(|i| match runs.search(i) {
            Search::Found { notes, .. } => notes,
            _ => none.as_slice(),
        })
        .collect();
    let input: Vec<LaneIn> = found.iter().zip(&pins).map(|(m, p)| LaneIn { matches: m, pins: p }).collect();
    let placed = lanes::place(rel, &input, every_lane);
    // the cards of every lane first: which notes are in a done lane decides which are blocked (BOARD-024)
    let mut cards: Vec<Vec<(Card, bool)>> = placed.into_iter().map(|lane| lane.into_iter().filter_map(|p| Card::of(vault, &p.rel).map(|c| (c, p.pinned))).collect()).collect();
    let in_done_lane: HashSet<PathBuf> = board.lanes.iter().zip(&cards).filter(|(l, _)| l.done()).flat_map(|(_, c)| c.iter().map(|(c, _)| c.rel.clone())).collect();
    let graph = Graph::new(vault, &in_done_lane);
    cards.iter_mut().flatten().for_each(|(c, _)| c.dep = graph.dep(&c.rel));
    board
        .lanes
        .iter()
        .enumerate()
        .zip(cards)
        .map(|((i, lane), cards)| {
            let sort_keys = form_sort::parse(lane.sort());
            let (keys, sort_error) = match sort_keys {
                Ok(k) => (k, None),
                Err(e) => (Vec::new(), Some(e)),
            };
            let search = runs.search(i);
            let (mut pinned, mut rest): (Vec<Card>, Vec<Card>) = (Vec::new(), Vec::new());
            for (c, is_pinned) in cards {
                if is_pinned { &mut pinned } else { &mut rest }.push(c);
            }
            let mut places = HashMap::new();
            if keys.iter().any(|k| k.field == form_sort::Field::Dependencies) {
                // (the order is of the cards the keys sort: the pinned ones keep their places)
                for (place, (c, step)) in deps::order(&rest, &graph.edges(&rest)).into_iter().enumerate() {
                    places.insert(rest[c].rel.clone(), place);
                    rest[c].dep.step = Some(step);
                }
            }
            sort::sort(&mut rest, &keys, &places);
            let count = pinned.len();
            pinned.extend(rest);
            LaneView {
                name: lane.name.clone(),
                query: lane.query().to_string(),
                sort: lane.sort().to_string(),
                drop: lane.drop().to_string(),
                done: lane.done(),
                error: match search {
                    Search::Invalid(e) => Some(e.reason.clone()),
                    _ => sort_error,
                },
                waiting: search == Search::Waiting,
                partial: matches!(search, Search::Found { partial: true, .. }),
                cards: pinned,
                pinned: count,
            }
        })
        .collect()
}

/// The boards of the window: which one is open, the card that is selected, and what the view and
/// the peek keep between frames.
#[derive(Default)]
pub struct Boards {
    /// the board the view shows (BOARD-007)
    pub current: Option<PathBuf>,
    /// the card that is selected (BOARD-012)
    pub selected: Option<PathBuf>,
    pub peek: Option<peek::Peek>,
    pub(crate) runs: lanes::Runs,
    pub(crate) view: view::State,
    /// the boards of the vault, made again when the vault changes
    listed: Option<((u64, u64), Vec<Info>)>,
    pub(crate) shown: Shown,
}

impl Boards {
    /// The boards of the vault; made when the vault has changed since the last call.
    pub fn listing(&mut self, vault: &Vault) -> &[Info] {
        let stamp = (vault.uid, vault.generation);
        if self.listed.as_ref().is_none_or(|(s, _)| *s != stamp) {
            self.listed = Some((stamp, list(vault)));
        }
        &self.listed.as_ref().expect("listed above").1
    }
}

/// What the lanes on show were made from.
#[derive(PartialEq, Eq)]
struct Made {
    board: PathBuf,
    stamp: (u64, u64),
    version: u64,
    text: u64,
}

/// The lanes as last made, with what they were made from: they are made again only when the
/// vault, the board file or an answer of the search changed.
#[derive(Default)]
pub struct Shown(Option<(Made, Vec<LaneView>)>);

impl Shown {
    /// The lanes as last made: what the user saw when they dropped a card on one.
    pub fn lanes(&self) -> &[LaneView] {
        self.0.as_ref().map_or(&[], |(_, l)| l)
    }

    /// The lanes of the board `rel` with the text `text`.
    pub fn get(&mut self, vault: &Vault, rel: &Path, text: &str, board: &Board, every_lane: bool, runs: &lanes::Runs) -> &[LaneView] {
        let made = Made { board: rel.to_path_buf(), stamp: (vault.uid, vault.generation), version: runs.version, text: hash(text) ^ u64::from(every_lane) };
        if self.0.as_ref().is_none_or(|(m, _)| *m != made) {
            self.0 = Some((made, lane_views(vault, rel, board, every_lane, runs)));
        }
        &self.0.as_ref().expect("made above").1
    }
}

fn hash(text: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}
