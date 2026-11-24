//! The autocomplete of the value fields of the form (QUERY-029): the tag of a "tag is" row, and the
//! name and the value of a "property is" row. A list of what the vault knows opens under the field
//! while it has the keyboard; it narrows as the field is typed in. The list is part of the form (it
//! is laid out below the row, not in a window of its own), so it is inside the popover of a lane and
//! the window of the picker, and a click on it is a click in the form.

use super::key_ok;
use crate::link_assist::{down_key, up_key};
use crate::markdown;
use crate::vault::Vault;
use eframe::egui::{self, Id, Key, Modifiers, Rect, RichText, Ui};
use egui::text::{CCursor, CCursorRange};
use std::collections::{HashMap, HashSet};

/// The most the list shows before it scrolls.
const MAX_HEIGHT: f32 = 170.0;

/// The properties Shale stamps on a note: a day and a time is no value to filter for (equality).
const STAMPS: [&str; 2] = ["created", "modified"];

/// What the vault knows: the tags, the names of the properties and, for each, the values that stand
/// in the front matter of the notes. Every list is sorted without regard to case.
#[derive(Default)]
struct Vocab {
    tags: Vec<String>,
    keys: Vec<String>,
    values: HashMap<String, Vec<String>>,
}

/// Sort without regard to case (then by case, so that the order is the same every time) and drop
/// the entries that are the same word in another case.
fn tidy(set: HashSet<String>) -> Vec<String> {
    let mut v: Vec<String> = set.into_iter().collect();
    v.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b)));
    v.dedup_by(|a, b| a.to_lowercase() == b.to_lowercase());
    v
}

impl Vocab {
    /// Read the vault's tags (as indexed) and the front matter of its notes. A property's name is
    /// one the language reads (`key_ok`), and not `created` or `modified`; a value is one a query
    /// can say (no quote, not starting with `//`). A list value gives one entry for each item, as
    /// the query matcher reads it.
    fn of(vault: &Vault) -> Vocab {
        let mut tags = HashSet::new();
        let mut keys = HashSet::new();
        let mut values: HashMap<String, HashSet<String>> = HashMap::new();
        for note in vault.notes.iter().filter(|(p, _)| !vault.is_shadow_copy(p)).map(|(_, n)| n) {
            tags.extend(note.tags.iter().cloned());
            for (key, items) in markdown::front_matter_values(&note.text) {
                if !key_ok(&key) || STAMPS.contains(&key.to_ascii_lowercase().as_str()) {
                    continue;
                }
                let seen = values.entry(key.clone()).or_default();
                seen.extend(items.iter().map(|v| v.trim()).filter(|v| !v.is_empty() && !v.contains('"') && !v.starts_with("//")).map(str::to_string));
                keys.insert(key);
            }
        }
        Vocab { tags: tidy(tags), keys: tidy(keys), values: values.into_iter().map(|(k, v)| (k, tidy(v))).collect() }
    }

    fn entries(&self, slot: Slot, key: &str) -> &[String] {
        match slot {
            Slot::Tag => &self.tags,
            Slot::Key => &self.keys,
            Slot::Value => self.values.get(key).map_or(&[][..], Vec::as_slice),
        }
    }
}

/// Which field of a row has a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Tag,
    Key,
    Value,
}

/// The entries that fit what is typed: those that start with it first, then those that have it
/// inside, each in the order of `entries` (alphabetical). Without regard to case. Nothing typed:
/// all of them.
pub(super) fn narrow(typed: &str, entries: &[String]) -> Vec<String> {
    let typed = typed.trim().to_lowercase();
    if typed.is_empty() {
        return entries.to_vec();
    }
    let lower: Vec<String> = entries.iter().map(|e| e.to_lowercase()).collect();
    let starts = entries.iter().zip(&lower).filter(|(_, l)| l.starts_with(&typed)).map(|(e, _)| e);
    let inside = entries.iter().zip(&lower).filter(|(_, l)| !l.starts_with(&typed) && l.contains(&typed)).map(|(e, _)| e);
    starts.chain(inside).cloned().collect()
}

/// The frame number the list was last drawn in, for the windows that read Esc before the form does.
fn open_mark() -> Id {
    Id::unique("search-form-list-open")
}

/// Is a list of the form open, or was it in the frame before? Esc closes the list and nothing else
/// (QUERY-029): the host that reads Esc (the picker, the peek of a card) leaves it to the form.
pub fn list_open(ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<u64>(open_mark())).is_some_and(|f| f + 1 >= now)
}

/// Which field of which row a text box is, and what it shows.
pub(super) struct Spec<'a> {
    pub row: usize,
    pub slot: Slot,
    /// the name of the property, for the list of its values
    pub key: &'a str,
    pub hint: &'a str,
    pub width: f32,
}

/// The state of the lists of one form: at most one field has its list.
#[derive(Default)]
pub struct Complete {
    vocab: Option<((u64, u64), Vocab)>,
    /// the field the list belongs to: the id of its text box, the row, which of its fields
    field: Option<(Id, usize, Slot)>,
    /// Esc or a pick closed it; typing in the field or coming back to it opens it
    closed: bool,
    /// what is listed, and what it was made for (so it is made again when that changes)
    items: Vec<String>,
    made_for: Option<(Slot, String, String, (u64, u64))>,
    sel: Option<usize>,
    /// the list was drawn in the last frame (the keys of this frame are for it) / is drawn in this one
    was: bool,
    shown: bool,
    /// the selection moved: the list scrolls to it
    moved: bool,
    /// where the list was drawn
    rect: Option<Rect>,
}

/// Put the cursor of the text box `id` behind `chars` characters.
fn cursor_to(ctx: &egui::Context, id: Id, chars: usize) {
    let mut state = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(chars))));
    state.store(ctx, id);
}

impl Complete {
    /// Before the rows: what was shown is what the keys of this frame are for.
    pub(super) fn begin(&mut self) {
        self.was = std::mem::take(&mut self.shown);
    }

    /// The keys of the open list, before its field is drawn; true when one took an entry into `value`.
    fn take_keys(&mut self, ui: &mut Ui, id: Id, value: &mut String) -> bool {
        if !self.was || self.items.is_empty() || self.field.map(|f| f.0) != Some(id) {
            return false;
        }
        let n = self.items.len();
        let (mut down, mut up, mut take, mut esc) = (false, false, false, false);
        ui.input_mut(|i| {
            down = down_key(i);
            up = up_key(i);
            // (Enter and Tab only take an entry that was chosen: Tab through an empty field goes on)
            if self.sel.is_some() {
                take = i.consume_key(Modifiers::NONE, Key::Enter) || i.consume_key(Modifiers::NONE, Key::Tab);
            }
            esc = i.consume_key(Modifiers::NONE, Key::Escape);
        });
        if down {
            self.sel = Some(self.sel.map_or(0, |s| (s + 1) % n));
            self.moved = true;
        }
        if up {
            self.sel = Some(self.sel.map_or(n - 1, |s| (s + n - 1) % n));
            self.moved = true;
        }
        if esc {
            self.closed = true;
            // (egui takes the keyboard from a field on Esc: the field keeps it)
            ui.ctx().memory_mut(|m| m.request_focus(id));
        }
        let Some(entry) = self.sel.filter(|_| take).and_then(|s| self.items.get(s)).cloned() else { return false };
        cursor_to(ui.ctx(), id, entry.chars().count());
        *value = entry;
        self.closed = true;
        self.sel = None;
        true
    }

    /// A text box of a row with the list of its slot. True when `value` changed.
    pub(super) fn field(&mut self, ui: &mut Ui, vault: &Vault, spec: &Spec, value: &mut String) -> bool {
        let id = ui.scope_id().with(("complete", spec.slot as u8));
        let mut changed = self.take_keys(ui, id, value);
        let r = ui.add(egui::TextEdit::singleline(value).id(id).hint_text(spec.hint).desired_width(spec.width));
        if r.changed() && value.contains('"') {
            value.retain(|c| c != '"');
        }
        changed |= r.changed();
        if r.changed() || r.gained_focus() || r.clicked() {
            self.field = Some((id, spec.row, spec.slot));
            self.closed = false;
            self.sel = (!value.trim().is_empty()).then_some(0);
            self.moved = true;
        }
        if self.field.map(|f| f.0) == Some(id) {
            let over = self.rect.zip(ui.input(|i| i.pointer.hover_pos())).is_some_and(|(r, p)| r.contains(p));
            if (r.has_focus() || (over && self.was)) && !self.closed {
                self.make(vault, spec, value);
                self.shown = !self.items.is_empty();
            } else if !r.has_focus() && !over {
                self.field = None;
            }
        }
        changed
    }

    /// List what fits `typed`, when that is not already what is listed.
    fn make(&mut self, vault: &Vault, spec: &Spec, typed: &str) {
        let stamp = (vault.uid, vault.generation);
        let made_for = (spec.slot, spec.key.to_string(), typed.to_string(), stamp);
        if self.made_for.as_ref() == Some(&made_for) {
            return;
        }
        if self.vocab.as_ref().map(|v| v.0) != Some(stamp) {
            self.vocab = Some((stamp, Vocab::of(vault)));
        }
        let all = self.vocab.as_ref().map_or(&[][..], |v| v.1.entries(spec.slot, spec.key));
        self.items = narrow(typed, all);
        // (the one entry that is what is typed has nothing to add)
        if self.items.len() == 1 && self.items[0] == typed {
            self.items.clear();
        }
        self.made_for = Some(made_for);
        if self.sel.is_some_and(|s| s >= self.items.len()) {
            self.sel = (!self.items.is_empty()).then_some(0);
        }
    }

    /// Draw the list under row `row`, when it is the row of the open one; the entry clicked, with the
    /// field it is for.
    pub(super) fn list(&mut self, ui: &mut Ui, row: usize) -> Option<(Slot, String)> {
        let (id, at, slot) = self.field.filter(|f| f.1 == row)?;
        if !self.shown {
            self.rect = None;
            return None;
        }
        let now = ui.ctx().cumulative_frame_nr();
        ui.ctx().data_mut(|d| d.insert_temp(open_mark(), now));
        let mut clicked = None;
        let frame = egui::Frame::popup(ui.style()).show(ui, |ui| {
            egui::ScrollArea::vertical().id_salt(("form-complete", at)).max_height(MAX_HEIGHT).auto_shrink([false, true]).show(ui, |ui| {
                for (i, entry) in self.items.iter().enumerate() {
                    let r = ui.selectable_label(self.sel == Some(i), RichText::new(entry));
                    if self.sel == Some(i) && std::mem::take(&mut self.moved) {
                        r.scroll_to_me(None);
                    }
                    if r.clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
        self.rect = Some(frame.response.rect);
        let picked = clicked.and_then(|i| self.items.get(i).cloned())?;
        // the click took the keyboard from the field: give it back, the cursor behind the entry
        ui.ctx().memory_mut(|m| m.request_focus(id));
        cursor_to(ui.ctx(), id, picked.chars().count());
        self.closed = true;
        self.shown = false;
        self.sel = None;
        Some((slot, picked))
    }

    /// The entries the list shows now (tests).
    #[cfg(test)]
    pub(super) fn shown_items(&self) -> Option<&[String]> {
        self.shown.then_some(self.items.as_slice())
    }
}
