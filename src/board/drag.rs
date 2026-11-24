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

use super::file::{pin_item, pin_target, Board};
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
    /// the new list of pinned cards of the lanes whose list changes, as links (`[[Name]]`)
    pub pins: Vec<(usize, Vec<String>)>,
}

/// The property a drop on a lane sets (BOARD-019): its `drop:` line, else the one its query says.
pub fn property(query: &str, drop: &str) -> Option<(String, String)> {
    let text = if drop.trim().is_empty() { lanes::drop_property(query)? } else { drop.trim().to_string() };
    let (key, value) = text.split_once(':')?;
    let (key, value) = (key.trim(), value.trim());
    (!key.is_empty() && !value.is_empty() && !key.contains(char::is_whitespace)).then(|| (key.to_string(), value.to_string()))
}

/// The link to `card` for a pin (BOARD-004): its name when that finds it from the board, else its
/// path; a name or path that `[[ ]]` cannot hold (`#`, `|`, `^`, brackets) is a markdown link with
/// the path, see [`pin_item`].
fn pin_text(vault: &Vault, board: &Path, card: &Path) -> String {
    let name = card.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let by_name = vault.resolve(markdown::strip_md_ext(&name), Some(board)).as_deref() == Some(card) || vault.resolve(&name, Some(board)).as_deref() == Some(card);
    // (a drawing keeps its extension in the link, a note does not: `strip_md_ext` leaves it)
    pin_item(&name, &rel_to_string(card), by_name)
}

/// The pins of a lane, each with the note it links to.
fn entries(vault: &Vault, board_rel: &Path, board: &Board, lane: usize) -> Vec<(String, Option<PathBuf>)> {
    board.lanes[lane].pins().into_iter().map(|p| (p.to_string(), vault.resolve(&pin_target(p), Some(board_rel)))).collect()
}

/// What dropping `d` changes, or none when it changes nothing. `lanes` is what the view showed,
/// `board` the file now.
pub fn plan(vault: &Vault, board_rel: &Path, board: &Board, lanes: &[LaneView], d: &Dropped) -> Option<Plan> {
    plan_card(vault, board_rel, board, lanes, &Card { rel: &d.card, from: Some(d.from), has_properties: true }, d.to, d.index)
}

/// The dropped card of [`plan_card`]: its note, the lane it was taken from (none: it is not on the
/// board) and whether it has front matter to write (a drawing has not).
struct Card<'a> {
    rel: &'a Path,
    from: Option<usize>,
    has_properties: bool,
}

fn plan_card(vault: &Vault, board_rel: &Path, board: &Board, lanes: &[LaneView], card: &Card, to: usize, index: usize) -> Option<Plan> {
    if board.lanes.len() != lanes.len() || card.from.is_some_and(|f| f >= lanes.len()) || to >= lanes.len() || index > lanes[to].cards.len() {
        return None;
    }
    let target = &lanes[to];
    let at = target.cards.iter().position(|c| c.rel == card.rel);
    let pinned_in = |lane: &LaneView| lane.cards.iter().position(|c| c.rel == card.rel).is_some_and(|p| p < lane.pinned);
    let same = card.from == Some(to);
    if same && at.is_some_and(|p| index == p || index == p + 1) {
        return None;
    }
    let lane = &board.lanes[to];
    let property = if same || !card.has_properties { None } else { property(lane.query(), lane.drop()) };
    let mut pins = Vec::new();
    if let Some(from) = card.from.filter(|f| *f != to && pinned_in(&lanes[*f])) {
        let mut list = entries(vault, board_rel, board, from);
        list.retain(|(_, r)| r.as_deref() != Some(card.rel));
        pins.push((from, list.into_iter().map(|(p, _)| p).collect::<Vec<_>>()));
    }
    let pin_here = same || lane.query().trim().is_empty() || property.is_none() || index < target.pinned;
    if pin_here {
        let mut list = entries(vault, board_rel, board, to);
        list.retain(|(_, r)| r.as_deref() != Some(card.rel));
        let before = target.cards[..index].iter().filter(|c| c.rel != card.rel).count();
        let pinned_rest = target.pinned - usize::from(pinned_in(target));
        let place = place_after(&list, before.min(pinned_rest));
        list.insert(place, (pin_text(vault, board_rel, card.rel), Some(card.rel.to_path_buf())));
        pins.push((to, list.into_iter().map(|(p, _)| p).collect()));
    }
    Some(Plan { property, pins })
}

/// Where in the pin list a new pin goes so that it comes after the `k`-th pin that links to a
/// note; the lines that link to none stay where they are.
fn place_after(list: &[(String, Option<PathBuf>)], k: usize) -> usize {
    if k == 0 {
        0
    } else {
        list.iter().enumerate().filter(|(_, (_, r))| r.is_some()).nth(k - 1).map_or(list.len(), |(i, _)| i + 1)
    }
}

/// Where a file or folder of the file list was dropped on the board (BOARD-035 to BOARD-037).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// a lane, at this place among its cards
    Lane { lane: usize, index: usize },
    /// the empty part of the board, or the "+ Add lane" button
    NewLane,
}

/// The file or folder `path` of the file list dropped on `target`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeDropped {
    pub path: PathBuf,
    pub target: Target,
}

/// What a drop from the file list does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Todo {
    /// the property of the note and the new pin lists
    pub plan: Plan,
    /// a lane to add at the end, as its name and its query
    pub new_lane: Option<(String, String)>,
    /// something to tell the user after the drop
    pub notice: Option<String>,
}

/// What the drop from the file list says while it is held over the board (`hint`) and what it
/// does when let go (`todo`, none: nothing).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tree {
    pub hint: String,
    pub todo: Option<Todo>,
}

fn refused(hint: impl Into<String>) -> Tree {
    Tree { hint: hint.into(), todo: None }
}

/// The query of a lane that follows the folder `folder` (BOARD-037): `path:Folder/`, in quotes
/// when the path has a space or a parenthesis. A quote in the name cannot be written.
pub fn folder_query(folder: &Path) -> Option<String> {
    let path = rel_to_string(folder);
    if path.contains(['"', '\n', '\r']) {
        return None;
    }
    let quote = path.contains(|c: char| c.is_whitespace() || c == '(' || c == ')') || path.contains("[[");
    Some(if quote { format!("path:\"{path}/\"") } else { format!("path:{path}/") })
}

/// The notes and drawings of `folder` and below, in path order, that no lane of the view shows
/// (the board's own note is none of them).
fn folder_cards(vault: &Vault, board_rel: &Path, lanes: &[LaneView], folder: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = vault
        .files
        .iter()
        .filter(|f| matches!(f.kind, FileKind::Note | FileKind::Drawing) && f.rel.starts_with(folder) && f.rel != board_rel && !vault.is_shadow_copy(&f.rel))
        .filter(|f| !lanes.iter().any(|l| l.cards.iter().any(|c| c.rel == f.rel)))
        .map(|f| f.rel.clone())
        .collect();
    found.sort();
    found
}

/// What dropping the file or folder `path` of the file list on `target` does (BOARD-035 to BOARD-037).
pub fn tree(vault: &Vault, board_rel: &Path, board: &Board, lanes: &[LaneView], path: &Path, target: &Target) -> Tree {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if vault.folders.iter().any(|f| f == path) {
        return folder(vault, board_rel, board, lanes, path, &name, target);
    }
    let Some(file) = vault.file(path) else { return refused(format!("“{name}” is not in the vault")) };
    if file.kind == FileKind::Other {
        return refused(format!("“{name}” is not a note or a drawing"));
    }
    let title = file.title();
    if path == board_rel {
        return refused(format!("“{title}” is this board: drop another note"));
    }
    let Target::Lane { lane: to, index } = *target else { return refused(format!("Drop “{title}” on a lane")) };
    let Some(lane) = lanes.get(to) else { return refused("") };
    let note = file.kind == FileKind::Note;
    let from = lanes.iter().position(|l| l.cards.iter().any(|c| c.rel == path));
    let Some(plan) = plan_card(vault, board_rel, board, lanes, &Card { rel: path, from, has_properties: note }, to, index) else {
        return refused(format!("“{title}” is already here"));
    };
    let lane_name = &lane.name;
    let (hint, notice) = match &plan.property {
        Some((k, v)) => (format!("Set {k}: {v} in “{title}”"), None),
        None if !note && property(board.lanes[to].query(), board.lanes[to].drop()).is_some() && from != Some(to) => {
            (format!("Pin “{title}” in {lane_name} (a drawing has no properties)"), Some(format!("“{title}” is a drawing and has no properties, so it is only pinned in {lane_name}")))
        }
        None => (format!("Pin “{title}” in {lane_name}"), None),
    };
    Tree { hint, todo: Some(Todo { plan, new_lane: None, notice }) }
}

fn folder(vault: &Vault, board_rel: &Path, board: &Board, lanes: &[LaneView], path: &Path, name: &str, target: &Target) -> Tree {
    match *target {
        Target::NewLane => {
            let Some(query) = folder_query(path) else { return refused(format!("“{name}” has a quote in its name: a lane query cannot hold it")) };
            Tree { hint: format!("New lane “{name}” ({query})"), todo: Some(Todo { plan: Plan { property: None, pins: Vec::new() }, new_lane: Some((name.to_string(), query)), notice: None }) }
        }
        Target::Lane { lane: to, index } => {
            let Some(shown) = lanes.get(to).filter(|l| lanes.len() == board.lanes.len() && index <= l.cards.len()) else { return refused("") };
            let cards = folder_cards(vault, board_rel, lanes, path);
            if cards.is_empty() {
                return refused(format!("“{name}” has no note or drawing that is not on the board"));
            }
            let mut list = entries(vault, board_rel, board, to);
            let at = place_after(&list, index.min(shown.pinned));
            list.splice(at..at, cards.iter().map(|card| (pin_text(vault, board_rel, card), Some(card.clone()))));
            let count = if cards.len() == 1 { "1 note".to_string() } else { format!("{} notes", cards.len()) };
            let hint = format!("Pin {count} of “{name}” in {}", shown.name);
            let plan = Plan { property: None, pins: vec![(to, list.into_iter().map(|(p, _)| p).collect())] };
            Tree { hint, todo: Some(Todo { plan, new_lane: None, notice: None }) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec: BOARD-019, BOARD-024
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

    // spec: BOARD-037
    #[test]
    fn board_037_a_folder_query_is_quoted_when_the_name_needs_it() {
        let q = |p: &str| folder_query(Path::new(p));
        assert_eq!(q("Proj").as_deref(), Some("path:Proj/"));
        assert_eq!(q("A/B").as_deref(), Some("path:A/B/"));
        assert_eq!(q("My Folder (x)").as_deref(), Some("path:\"My Folder (x)/\""));
        assert_eq!(q("a (b)").as_deref(), Some("path:\"a (b)/\""));
        assert_eq!(q("Q\"uote"), None);
    }

    // spec: BOARD-004, BOARD-022
    #[test]
    fn board_004_a_pin_for_any_file_name_is_read_back_as_the_same_file() {
        let names = ["Plain", "With #hash 1", "a|b", "up^caret", "[brackets]", "100% (done) <x>", "Größe ünï 日本", "50% (a) <b> #1", "Dup"];
        let d = std::env::temp_dir().join(format!("shale-board-pin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("Sub")).unwrap();
        for n in names {
            std::fs::write(d.join(format!("{n}.md")), "x\n").unwrap();
        }
        std::fs::write(d.join("Sub/Dup.md"), "x\n").unwrap();
        std::fs::write(d.join("Sub/Dir #1.md"), "x\n").unwrap();
        std::fs::write(d.join("Board.md"), "---\ntype: board\n---\n## M\n").unwrap();
        let v = Vault::open(&d).unwrap();
        let mut cards: Vec<PathBuf> = names.iter().map(|n| PathBuf::from(format!("{n}.md"))).collect();
        cards.extend([PathBuf::from("Sub/Dup.md"), PathBuf::from("Sub/Dir #1.md")]);
        let mut pins = Vec::new();
        for c in &cards {
            let pin = pin_text(&v, Path::new("Board.md"), c);
            assert_eq!(v.resolve(&pin_target(&pin), Some(Path::new("Board.md"))).as_deref(), Some(c.as_path()), "{pin}");
            pins.push(pin);
        }
        // the ordinary names keep the `[[Name]]` form; the others are markdown links with the path
        assert_eq!(pins[0], "[[Plain]]");
        assert_eq!(pins[1], "[With #hash 1](<With %23hash 1.md>)");
        assert_eq!(pins[5], "[[100% (done) <x>]]");
        assert_eq!(pins[7], "[50% (a) <b> #1](<50%25 %28a%29 %3Cb%3E %231.md>)");
        assert_eq!(pins[6], "[[Größe ünï 日本]]");
        assert_eq!(pins[10], "[Dir #1](<Sub/Dir %231.md>)");
        // and a board file that holds them gives them back, one pin per line
        let mut b = Board::parse("## M\n");
        b.lanes[0].set_pins(&pins);
        let again = Board::parse(&b.write());
        assert_eq!(again.lanes[0].pins(), pins.iter().map(String::as_str).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(d);
    }
}
