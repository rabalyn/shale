//! What the vault knows for the lists of the form (QUERY-029): the tags, the names of the
//! properties and the values seen for each name. It is built from a [`Snapshot`] on the search
//! worker, never on the window's thread (QUERY-030).

use super::key_ok;
use crate::markdown;
use crate::search_all::Snapshot;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};

/// The properties Shale stamps on a note: a day and a time is no value to filter for (equality).
const STAMPS: [&str; 2] = ["created", "modified"];

/// What the vault knows: the tags, the names of the properties and, for each, the values that stand
/// in the front matter of the notes. Every list is sorted without regard to case.
#[derive(Default)]
pub struct Vocab {
    pub(super) tags: Vec<String>,
    pub(super) keys: Vec<String>,
    pub(super) values: HashMap<String, Vec<String>>,
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
    /// Read the tags (as indexed) and the front matter of the notes of `snapshot`. A property's
    /// name is one the language reads (`key_ok`), and not `created` or `modified`; a value is one
    /// a query can say (no quote, not starting with `//`). A list value gives one entry for each
    /// item, as the query matcher reads it. None when `cancel` was set (the request was given up).
    pub fn of(snapshot: &Snapshot, cancel: &AtomicBool) -> Option<Vocab> {
        let mut tags = HashSet::new();
        let mut keys = HashSet::new();
        let mut values: HashMap<String, HashSet<String>> = HashMap::new();
        for note in snapshot.notes.iter().filter_map(|n| n.note.as_ref()) {
            if cancel.load(Ordering::Relaxed) {
                return None;
            }
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
        Some(Vocab { tags: tidy(tags), keys: tidy(keys), values: values.into_iter().map(|(k, v)| (k, tidy(v))).collect() })
    }

    pub(super) fn entries(&self, slot: super::complete::Slot, key: &str) -> &[String] {
        use super::complete::Slot;
        match slot {
            Slot::Tag => &self.tags,
            Slot::Key => &self.keys,
            Slot::Value => self.values.get(key).map_or(&[][..], Vec::as_slice),
        }
    }
}
