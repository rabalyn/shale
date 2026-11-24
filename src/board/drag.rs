//! Dropping a card on a lane (BOARD-019 to BOARD-022): what the drop writes into the card's note
//! and into the board file. [`plan`] decides; the window carries the plan out (`app/boards.rs`)
//! through the checked saves of both notes, so each is one undo step.
//!
//! * A lane has a drop property when it has a `drop:` line, or a simple query such as
//!   `status:doing` that says one ([`lanes::drop_property`]). A drop on it sets that property in
//!   the card's front matter and nothing else of the note.
//! * A card that is pinned in the lane it is taken from is unpinned there, otherwise BOARD-032
//!   would keep it in that lane.
//! * The drop pins the card in the target lane (at the place it was dropped at) when the lane is
//!   manual, when it has no drop property (the pin is all a drop can do), when it is dropped among
//!   the pinned cards on top, or when the drop is inside the lane the card came from.

use super::file::{pin_target, Board};
use super::{lanes, LaneView};
use crate::markdown;
use crate::vault::{rel_to_string, FileKind, Vault};
use std::path::{Path, PathBuf};

/// A card dropped: the card, the lane it was taken from and the one it was dropped on, and the place in the
/// cards shown there (0 is above the first card, the number of cards is below the last).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dropped {
    pub card: PathBuf,
    pub from: usize,
    pub to: usize,
    pub index: usize,
}

/// What a drop changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// the property to set in the card's note, `status` and `doing`
    pub property: Option<(String, String)>,
    /// the new list of pinned cards of the lanes whose list changes, as the text inside `[[ ]]`
    pub pins: Vec<(usize, Vec<String>)>,
}

/// The property a drop on a lane sets (BOARD-019): its `drop:` line, else the one its query says.
pub fn property(query: &str, drop: &str) -> Option<(String, String)> {
    let text = if drop.trim().is_empty() { lanes::drop_property(query)? } else { drop.trim().to_string() };
    let (key, value) = text.split_once(':')?;
    let (key, value) = (key.trim(), value.trim());
    (!key.is_empty() && !value.is_empty() && !key.contains(char::is_whitespace)).then(|| (key.to_string(), value.to_string()))
}

/// The text of a link to `card` for a pin: its name when that finds it from the board, else its path.
fn pin_text(vault: &Vault, board: &Path, card: &Path) -> String {
    let strip = |s: String| match vault.file(card).map(|f| f.kind) {
        Some(FileKind::Note | FileKind::Drawing) => markdown::strip_md_ext(&s).to_string(),
        _ => s,
    };
    let name = strip(card.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    if vault.resolve(&name, Some(board)).as_deref() == Some(card) {
        name
    } else {
        strip(rel_to_string(card))
    }
}

/// The pins of a lane, each with the note it links to.
fn entries(vault: &Vault, board_rel: &Path, board: &Board, lane: usize) -> Vec<(String, Option<PathBuf>)> {
    board.lanes[lane].pins().into_iter().map(|p| (p.to_string(), vault.resolve(pin_target(p), Some(board_rel)))).collect()
}

/// What dropping `d` changes, or none when it changes nothing. `lanes` is what the view showed,
/// `board` the file now.
pub fn plan(vault: &Vault, board_rel: &Path, board: &Board, lanes: &[LaneView], d: &Dropped) -> Option<Plan> {
    if board.lanes.len() != lanes.len() || d.from >= lanes.len() || d.to >= lanes.len() || d.index > lanes[d.to].cards.len() {
        return None;
    }
    let target = &lanes[d.to];
    let at = target.cards.iter().position(|c| c.rel == d.card);
    let pinned_in = |lane: &LaneView| lane.cards.iter().position(|c| c.rel == d.card).is_some_and(|p| p < lane.pinned);
    let same = d.from == d.to;
    if same && at.is_some_and(|p| d.index == p || d.index == p + 1) {
        return None;
    }
    let lane = &board.lanes[d.to];
    let property = if same { None } else { property(lane.query(), lane.drop()) };
    let mut pins = Vec::new();
    if !same && pinned_in(&lanes[d.from]) {
        let mut from = entries(vault, board_rel, board, d.from);
        from.retain(|(_, r)| r.as_deref() != Some(d.card.as_path()));
        pins.push((d.from, from.into_iter().map(|(p, _)| p).collect::<Vec<_>>()));
    }
    let pin_here = same || lane.query().trim().is_empty() || property.is_none() || d.index < target.pinned;
    if pin_here {
        let mut list = entries(vault, board_rel, board, d.to);
        list.retain(|(_, r)| r.as_deref() != Some(d.card.as_path()));
        let before = target.cards[..d.index].iter().filter(|c| c.rel != d.card).count();
        let pinned_rest = target.pinned - usize::from(pinned_in(target));
        let k = before.min(pinned_rest);
        // after the k-th pin that links to a note; the lines that link to none stay where they are
        let place = if k == 0 { 0 } else { list.iter().enumerate().filter(|(_, (_, r))| r.is_some()).nth(k - 1).map_or(list.len(), |(i, _)| i + 1) };
        list.insert(place, (pin_text(vault, board_rel, &d.card), Some(d.card.clone())));
        pins.push((d.to, list.into_iter().map(|(p, _)| p).collect()));
    }
    Some(Plan { property, pins })
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BOARD-019
    #[test]
    fn board_019_the_property_is_the_drop_line_or_the_one_a_simple_query_says() {
        let p = |q: &str, d: &str| property(q, d);
        assert_eq!(p("", "status: doing"), Some(("status".into(), "doing".into())));
        assert_eq!(p("status:todo", "status: doing"), Some(("status".into(), "doing".into())), "the drop line wins");
        assert_eq!(p("status:doing", ""), Some(("status".into(), "doing".into())));
        assert_eq!(p("#todo", ""), None);
        assert_eq!(p("status:doing priority:high", ""), None, "more than one term says no property");
        assert_eq!(p("", "nonsense"), None);
        assert_eq!(p("", "status:"), None);
        assert_eq!(p("", "two words: x"), None);
    }
}
