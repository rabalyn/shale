//! The "Blocked by" and "Blocks" lines of the peek of a card and the field that finds a note to add
//! (BOARD-044). The lines list the notes a card waits for and the cards that wait for it, each with a
//! ✕ that takes the link away; "+ Add blocker" and "+ Add blocked card" open a text field under them
//! whose list of notes is made by the search worker, off the UI thread (QUERY-019): board cards
//! first, then the other notes whose path has what was typed. The lines say what to change as
//! [`Edit`]s; the window writes them (`app/boards.rs`).

use crate::search_all::query::Checked;
use crate::search_all::{Lease, Request, SearchWorker, Snapshot};
use crate::search_sources::Prefixes;
use crate::theme;
use crate::vault::Vault;
use eframe::egui::{self, Key, Modifiers, RichText, Ui};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// How many notes the list of the field shows.
const MAX: usize = 8;

/// A change of a link between two cards that the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Edit {
    /// `blocker` becomes one of the blockers of `waiting`
    Add { blocker: PathBuf, waiting: PathBuf },
    /// `blocker` is no blocker of `waiting` any more
    Remove { blocker: PathBuf, waiting: PathBuf },
}

/// What the lines show of the card and its board.
pub struct Lists<'a> {
    /// the card the peek belongs to
    pub card: &'a Path,
    /// the board note: it is no card, and no blocker
    pub board: &'a Path,
    /// the notes the card waits for, with their titles
    pub blockers: &'a [(PathBuf, String)],
    /// the notes that wait for the card
    pub dependants: &'a [(PathBuf, String)],
    /// the cards of the board in the order of its lanes: they are listed first
    pub cards: &'a [PathBuf],
}

/// The words of `typed` as a query of the notes whose path has all of them (QUERY-002 `path:`); none
/// for nothing typed. Quotes and brackets are not part of a name here: they would be query syntax.
pub fn query_of(typed: &str) -> Option<String> {
    let words: Vec<String> = typed.split_whitespace().map(|w| w.chars().filter(|c| !matches!(c, '"' | '(' | ')')).collect::<String>()).filter(|w| !w.is_empty()).map(|w| format!("path:{w}")).collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// The notes to offer for `typed`: the board's cards before the other notes, those whose name
/// starts with it before the rest, each in its order; `skip` leaves out the card itself, the board and the
/// links that are there. Nothing typed offers the cards. At most [`MAX`].
pub fn rank(typed: &str, found: &[PathBuf], cards: &[PathBuf], skip: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return cards.iter().filter(|c| !skip(c)).take(MAX).cloned().collect();
    }
    let on_board: HashSet<&PathBuf> = cards.iter().collect();
    let place: std::collections::HashMap<&PathBuf, usize> = cards.iter().enumerate().map(|(i, c)| (c, i)).collect();
    let name = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mut list: Vec<(bool, bool, usize, &PathBuf)> = found.iter().enumerate().filter(|(_, p)| !skip(p)).map(|(i, p)| (!on_board.contains(p), !name(p).starts_with(&typed), place.get(p).copied().unwrap_or(i), p)).collect();
    list.sort_by_key(|(off, plain, at, _)| (*off, *plain, *at));
    list.into_iter().take(MAX).map(|(_, _, _, p)| p.clone()).collect()
}

/// The field of "+ Add blocker" and "+ Add blocked card" with its list.
pub struct Picker {
    /// adding a card that waits for this one (else a blocker of it)
    pub blocked: bool,
    text: String,
    lease: Lease,
    /// the notes of the vault as the search reads them, for the state of the vault they were made at
    snapshot: Option<((u64, u64), Arc<Snapshot>)>,
    /// the search on its way: its number and the text it asks about
    asked: Option<(u64, String)>,
    /// the text the notes in `found` were found for
    shown_for: Option<String>,
    found: Vec<PathBuf>,
    sel: usize,
    /// the field takes the keyboard when it first shows
    fresh: bool,
}

impl Picker {
    pub fn new(worker: &SearchWorker, blocked: bool) -> Picker {
        Picker { blocked, text: String::new(), lease: worker.lease(), snapshot: None, asked: None, shown_for: None, found: Vec::new(), sel: 0, fresh: true }
    }

    /// Ask the worker about what is typed when that is new, and take in its answer when it is there.
    /// The answer of an older text, or of an older state of the vault, is not used.
    fn poll(&mut self, worker: &mut SearchWorker, vault: &Vault) {
        let stamp = (vault.uid, vault.generation);
        if self.snapshot.as_ref().is_none_or(|(s, _)| *s != stamp) {
            self.snapshot = Some((stamp, Arc::new(Snapshot::of(vault))));
            self.asked = None;
            self.shown_for = None;
        }
        let queue = self.lease.slot;
        let Some(query) = query_of(&self.text) else {
            worker.cancel(queue);
            (self.asked, self.shown_for, self.found) = (None, Some(self.text.clone()), Vec::new());
            return;
        };
        if self.asked.as_ref().map(|a| &a.1) != Some(&self.text) && self.shown_for.as_ref() != Some(&self.text) {
            let checked = Checked::lane(&query, Prefixes::default());
            let snapshot = self.snapshot.as_ref().map(|s| s.1.clone()).expect("made above");
            self.asked = Some((worker.submit(queue, Request::lane(snapshot, checked)), self.text.clone()));
        }
        let Some((id, text)) = self.asked.clone() else { return };
        if let Some(found) = worker.take(queue, id) {
            (self.found, self.shown_for, self.asked) = (found.lane, Some(text), None);
        } else if worker.gave_up(queue, id) {
            (self.shown_for, self.asked) = (Some(text), None);
        }
    }

    /// The notes on offer now, from what the worker found for the text it was last asked about.
    pub fn offered(&self, lists: &Lists) -> Vec<PathBuf> {
        let there: HashSet<&Path> = if self.blocked { lists.dependants } else { lists.blockers }.iter().map(|(p, _)| p.as_path()).collect();
        rank(&self.text, &self.found, lists.cards, &|p| p == lists.card || p == lists.board || there.contains(p))
    }

    /// Draw the field and its list; the answer is the note the user chose, by Enter or by a click.
    fn show(&mut self, ui: &mut Ui, worker: &mut SearchWorker, vault: &Vault, lists: &Lists) -> Option<PathBuf> {
        self.poll(worker, vault);
        let offered = self.offered(lists);
        self.sel = self.sel.min(offered.len().saturating_sub(1));
        let id = ui.scope_id().with("board-blocker-field");
        // (the keys are the field's only while it has the keyboard)
        let focused = self.fresh || ui.memory(|m| m.has_focus(id));
        let (down, up, enter) = if focused { ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::Enter))) } else { (false, false, false) };
        if down && self.sel + 1 < offered.len() {
            self.sel += 1;
        }
        if up {
            self.sel = self.sel.saturating_sub(1);
        }
        let hint = if self.blocked { "Name of the card that waits for this one" } else { "Name of the note this card waits for" };
        let resp = ui.add(egui::TextEdit::singleline(&mut self.text).id(id).hint_text(hint).desired_width(f32::INFINITY));
        if std::mem::take(&mut self.fresh) {
            resp.request_focus();
        }
        if resp.changed() {
            self.sel = 0;
        }
        let mut chosen = enter.then(|| offered.get(self.sel).cloned()).flatten();
        for (i, p) in offered.iter().enumerate() {
            let title = vault.file(p).map(|f| f.title()).unwrap_or_default();
            let folder = p.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| format!("  {}", d.display())).unwrap_or_default();
            let label = RichText::new(format!("{title}{folder}"));
            if ui.add(egui::Button::selectable(i == self.sel, label)).clicked() {
                chosen = Some(p.clone());
            }
        }
        if offered.is_empty() {
            theme::hint(ui, if query_of(&self.text).is_none() || self.asked.is_none() { "No other note to offer" } else { "Searching…" });
        }
        chosen
    }
}

/// One line, "Blocked by" or "Blocks", of `items` with a ✕ each: gives the item whose ✕ was clicked.
fn line(ui: &mut Ui, caption: &str, items: &[(PathBuf, String)], add: &str, adding: bool, open: &mut Option<bool>, blocked: bool) -> Option<PathBuf> {
    let mut removed = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(caption).strong().color(theme::palette().text_strong));
        for (rel, title) in items {
            ui.label(title).on_hover_text(rel.to_string_lossy());
            if ui.small_button("✕").on_hover_text(format!("Take {title} out of “{}”", caption.to_lowercase())).clicked() {
                removed = Some(rel.clone());
            }
            ui.add_space(4.0);
        }
        if !adding && ui.small_button(add).clicked() {
            *open = Some(blocked);
        }
    });
    removed
}

/// The two lines of the peek and the field that is open under them. `picker` is the field (none while
/// closed); the answer is what the user asked to change.
pub fn show(ui: &mut Ui, picker: &mut Option<Picker>, worker: &mut SearchWorker, vault: &Vault, lists: &Lists) -> Option<Edit> {
    let mut edit = None;
    let mut open = None;
    let adding = picker.as_ref().map(|p| p.blocked);
    if let Some(blocker) = line(ui, "Blocked by", lists.blockers, "+ Add blocker", adding == Some(false), &mut open, false) {
        edit = Some(Edit::Remove { blocker, waiting: lists.card.to_path_buf() });
    }
    if adding == Some(false) {
        field(ui, picker, worker, vault, lists, &mut edit);
    }
    if let Some(waiting) = line(ui, "Blocks", lists.dependants, "+ Add blocked card", adding == Some(true), &mut open, true) {
        edit = Some(Edit::Remove { blocker: lists.card.to_path_buf(), waiting });
    }
    if adding == Some(true) {
        field(ui, picker, worker, vault, lists, &mut edit);
    }
    if let Some(blocked) = open {
        *picker = Some(Picker::new(worker, blocked));
    }
    edit
}

/// The field under its line, and what it chose.
fn field(ui: &mut Ui, picker: &mut Option<Picker>, worker: &mut SearchWorker, vault: &Vault, lists: &Lists, edit: &mut Option<Edit>) {
    let Some(p) = picker.as_mut() else { return };
    let blocked = p.blocked;
    let chosen = ui.horizontal(|ui| ui.vertical(|ui| p.show(ui, worker, vault, lists)).inner).inner;
    if let Some(other) = chosen {
        let me = lists.card.to_path_buf();
        *edit = Some(if blocked { Edit::Add { blocker: me, waiting: other } } else { Edit::Add { blocker: other, waiting: me } });
        *picker = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    // spec: BOARD-044
    #[test]
    fn board_044_the_typed_words_make_a_path_query() {
        assert_eq!(query_of("  "), None);
        assert_eq!(query_of("Roof tiles").as_deref(), Some("path:Roof path:tiles"));
        assert_eq!(query_of("a \"b\" (c)").as_deref(), Some("path:a path:b path:c"), "no quote or bracket of the query language gets in");
        assert!(Checked::lane(&query_of("100% #1 x:y").unwrap(), Prefixes::default()).error().is_none());
    }

    // spec: BOARD-044
    #[test]
    fn board_044_cards_of_the_board_come_first_then_names_that_start_with_the_text() {
        let found = paths(&["Archive/Wall plan.md", "Walls.md", "Awall.md", "Wallpaper.md", "Me.md"]);
        let cards = paths(&["Wallpaper.md", "Walls.md", "Me.md"]);
        let me = Path::new("Me.md");
        let out = rank("wall", &found, &cards, &|p| p == me);
        assert_eq!(out, paths(&["Wallpaper.md", "Walls.md", "Archive/Wall plan.md", "Awall.md"]), "cards in their board order, then the others, names that start with it first");
        assert_eq!(rank("", &found, &cards, &|p| p == me), paths(&["Wallpaper.md", "Walls.md"]), "nothing typed offers the cards, not the card itself");
        let many: Vec<PathBuf> = (0..20).map(|i| PathBuf::from(format!("N{i}.md"))).collect();
        assert_eq!(rank("n", &many, &[], &|_| false).len(), MAX);
    }
}
