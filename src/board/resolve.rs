//! A board resolved here and now, without the window's search worker (BOARD-030, BOARD-031): the
//! lanes' queries are run one after the other on a snapshot of the vault, and their answers are
//! placed and sorted as the view does ([`super::arrange`]). The exports of a board, the built-in
//! engine's (`pdf/board.rs`) and the LaTeX one's, start from [`resolve`] and read the order of the
//! cards from [`Resolved::chapters`].

use super::card::Card;
use super::file::{self, Board, Props};
use super::lanes::Search;
use super::LaneView;
use crate::search_all::matcher::{Budget, Run, Stop};
use crate::search_all::query::Checked;
use crate::search_sources::Prefixes;
use crate::vault::Vault;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

/// A board as an export reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub rel: PathBuf,
    pub title: String,
    pub props: Props,
    /// the lanes in their order, each with its cards in the order the view shows them; a lane
    /// whose query fails the check has the reason in `error` and no cards
    pub lanes: Vec<LaneView>,
}

/// The lanes of the board note `rel` with their cards. An error says why `rel` is no board.
pub fn resolve(vault: &Vault, rel: &Path) -> Result<Resolved, String> {
    let text = vault.notes.get(rel).map(|n| n.text.as_str()).ok_or_else(|| format!("“{}” is not a note of the vault", rel.display()))?;
    if !file::is_board(text) {
        return Err(format!("“{}” is not a board (it has no type: board)", rel.display()));
    }
    let board = Board::parse(text);
    let props = file::props(text);
    let snapshot = vault.search_snapshot();
    let never = AtomicBool::new(false);
    let checked: Vec<Checked> = board.lanes.iter().map(|l| Checked::lane(l.query(), Prefixes::default())).collect();
    // what each lane matched, and whether the work budget ended its search early (QUERY-010)
    let answers: Vec<(Vec<PathBuf>, bool)> = checked
        .iter()
        .map(|c| match c.expr() {
            // (a blank query is a manual lane: it matches nothing, only its pins)
            None => (Vec::new(), false),
            Some(expr) => {
                let mut run = Run::new(&snapshot, &never, Budget::standard());
                let notes = run.match_notes(expr);
                (notes, run.stopped == Some(Stop::Budget))
            }
        })
        .collect();
    let searches: Vec<Search> = checked
        .iter()
        .zip(&answers)
        .map(|(c, (notes, partial))| match c.error() {
            Some(e) => Search::Invalid(e),
            None => Search::Found { notes, partial: *partial },
        })
        .collect();
    let title = vault.file(rel).map(|f| f.title()).unwrap_or_default();
    Ok(Resolved { rel: rel.to_path_buf(), title, lanes: super::arrange(vault, rel, &board, props.every_lane, &searches), props })
}

impl Resolved {
    /// What the notes of the export are, in order: each lane that has cards with its cards, a card
    /// of several lanes (`cards: every-lane`, or a pin) only in the first lane that has it.
    pub fn chapters(&self) -> Vec<(&LaneView, Vec<&Card>)> {
        let mut seen: HashSet<&Path> = HashSet::new();
        self.lanes
            .iter()
            .filter_map(|lane| {
                let cards: Vec<&Card> = lane.cards.iter().filter(|c| seen.insert(c.rel.as_path())).collect();
                (!cards.is_empty()).then_some((lane, cards))
            })
            .collect()
    }
}
