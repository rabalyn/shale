//! The threads of imported items (ADR-0032, rabalyn/shale#575): which notes belong together, in
//! which order, and the chain of `previous` / `next` links.
//!
//! Nothing here knows what an item is. A note is described by its **ids** (a mail's `message-id`, a
//! post's `post-id`), the ids it replies to and its `thread-id`, all read from the front matter
//! ([`Meta`]); an item that is about to be imported is an [`Ident`]. So the posts of the socials
//! (SOC-070…SOC-072) only have to fill the same properties and make an `Ident` of a post; matching,
//! the plan of the chain and the thread overview work unchanged.

use crate::markdown;
use crate::vault::Vault;
use std::path::{Path, PathBuf};

/// An id without the angle brackets of a Message-ID (`<a@b>` → `a@b`), as ids are compared.
pub fn bare_id(s: &str) -> String {
    s.trim().trim_start_matches('<').trim_end_matches('>').trim().to_string()
}

/// The ids of a `References` header (or of a list property), bare, in order.
pub fn bare_ids(s: &str) -> Vec<String> {
    s.split_whitespace().map(bare_id).filter(|i| !i.is_empty()).collect()
}

/// `2026-11-20 14:30` for the date text of a note (`2026-11-20`, `2026-11-20T14:30:05` …); empty
/// when the text is no date. Texts of this form compare in time order.
pub fn date_key(stamp: &str) -> String {
    let s = stamp.trim();
    let Some(day) = s.get(..10).filter(|d| d.bytes().enumerate().all(|(i, b)| if i == 4 || i == 7 { b == b'-' } else { b.is_ascii_digit() })) else {
        return String::new();
    };
    let rest = s[10..].trim_start_matches(['T', ' ']);
    let time = rest.get(..5).filter(|t| t.bytes().enumerate().all(|(i, b)| if i == 2 { b == b':' } else { b.is_ascii_digit() })).unwrap_or("00:00");
    format!("{day} {time}")
}

/// What the front matter of a note says about its thread (IMP-067).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// the ids of the note: `message-id`, `post-id` and the `thread-ids` of a saved self-thread
    pub ids: Vec<String>,
    /// the id it replies to
    pub in_reply_to: String,
    /// the ids of `references`, in order
    pub references: Vec<String>,
    /// the root of its thread; empty when it has none
    pub thread_id: String,
    /// the network of a post (post ids are only unique per network); empty for a mail
    pub network: String,
    /// the date as [`date_key`]
    pub date_key: String,
    /// the sender (`from`) or the author
    pub who: String,
}

impl Meta {
    /// The thread data of a note's text; `None` when the note has none of the properties.
    pub fn parse(text: &str) -> Option<Meta> {
        if !text.starts_with("---") {
            return None;
        }
        let entries = markdown::front_matter_values(text);
        let values = |k: &str| entries.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_slice()).unwrap_or(&[]);
        let first = |k: &str| values(k).iter().map(|v| v.trim()).find(|v| !v.is_empty()).unwrap_or("").to_string();
        let mut ids: Vec<String> = Vec::new();
        for k in ["message-id", "post-id", "thread-ids"] {
            for v in values(k) {
                let id = bare_id(v);
                if !id.is_empty() && !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        let in_reply_to = bare_id(&first("in-reply-to"));
        let references: Vec<String> = values("references").iter().flat_map(|v| bare_ids(v)).collect();
        let thread_id = bare_id(&first("thread-id"));
        if ids.is_empty() && in_reply_to.is_empty() && references.is_empty() && thread_id.is_empty() {
            return None;
        }
        let who = Some(first("from")).filter(|f| !f.is_empty()).unwrap_or_else(|| first("author"));
        Some(Meta { ids, in_reply_to, references, thread_id, network: first("network"), date_key: date_key(&first("date")), who })
    }

    /// The id that orders notes of the same time.
    fn id(&self) -> &str {
        self.ids.first().map_or("", String::as_str)
    }
}

/// An item that is going to be imported: what the matching and the chain need of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ident {
    /// its own id (bare); empty when it has none
    pub id: String,
    pub in_reply_to: String,
    pub references: Vec<String>,
    pub network: String,
    /// the date as [`date_key`]
    pub date_key: String,
}

impl Ident {
    /// The root of its thread (ADR-0032, rule 2): the first of `references`, else `in-reply-to`, else
    /// its own id (it starts a thread). Empty when it has no id at all.
    pub fn thread_id(&self) -> String {
        self.references.first().cloned().or_else(|| Some(self.in_reply_to.clone()).filter(|i| !i.is_empty())).unwrap_or_else(|| self.id.clone())
    }
}

/// A note that continues the item: found by the reply headers (IMP-068).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub rel: PathBuf,
    pub date_key: String,
}

/// Where the new note goes in the chain of its thread (IMP-070, IMP-071): the notes before and
/// after it by date. Appending to the end has no `next`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub previous: Option<PathBuf>,
    pub next: Option<PathBuf>,
}

impl Plan {
    /// The item is older than a note of the thread: the chain has to be changed in the middle, which
    /// the person is asked (IMP-071).
    pub fn is_older(&self) -> bool {
        self.next.is_some()
    }

    /// How many other notes the plan rewrites: the neighbours.
    pub fn changes(&self) -> usize {
        usize::from(self.previous.is_some()) + usize::from(self.next.is_some())
    }
}

/// The notes of a vault that carry thread data, built once per import and once per change of the
/// vault for the overview (it reads the [`Meta`] the vault's own index holds, not the texts).
#[derive(Clone, Debug, Default)]
pub struct Index {
    notes: Vec<(PathBuf, Meta)>,
}

impl Index {
    pub fn build(vault: &Vault) -> Index {
        let mut notes: Vec<(PathBuf, Meta)> = vault.notes.iter().filter(|(p, _)| !vault.is_shadow_copy(p)).filter_map(|(p, n)| n.thread.as_ref().map(|m| (p.clone(), (**m).clone()))).collect();
        notes.sort_by(|a, b| a.0.cmp(&b.0));
        Index { notes }
    }

    fn meta(&self, rel: &Path) -> Option<&Meta> {
        self.notes.iter().find(|(p, _)| p == rel).map(|(_, m)| m)
    }

    /// The notes that continue `item`, the nearest first (IMP-068): the note the item replies to,
    /// the other notes of its `references`, the notes that name the item (it is older than they
    /// are), the other notes of its thread; within a kind the newest first. Only ids count, never a
    /// subject, a sender or a text.
    pub fn matches(&self, item: &Ident) -> Vec<Candidate> {
        let thread = item.thread_id();
        let mut found: Vec<(u8, &PathBuf, &Meta)> = Vec::new();
        for (rel, m) in &self.notes {
            if m.network != item.network || (!item.id.is_empty() && m.ids.contains(&item.id)) {
                continue;
            }
            let rank = if !item.in_reply_to.is_empty() && m.ids.contains(&item.in_reply_to) {
                0
            } else if item.references.iter().any(|r| m.ids.contains(r)) {
                1
            } else if !item.id.is_empty() && (m.in_reply_to == item.id || m.references.contains(&item.id)) {
                2
            } else if !thread.is_empty() && m.thread_id == thread {
                3
            } else {
                continue;
            };
            found.push((rank, rel, m));
        }
        found.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| b.2.date_key.cmp(&a.2.date_key)).then_with(|| a.1.cmp(b.1)));
        found.into_iter().map(|(_, rel, m)| Candidate { rel: rel.clone(), date_key: m.date_key.clone() }).collect()
    }

    /// The notes of the thread `thread_id` (and `network`), oldest first, ties by id.
    fn members(&self, thread_id: &str, network: &str) -> Vec<(&PathBuf, &Meta)> {
        let mut m: Vec<(&PathBuf, &Meta)> = self.notes.iter().filter(|(_, m)| !thread_id.is_empty() && m.thread_id == thread_id && m.network == network).map(|(p, m)| (p, m)).collect();
        m.sort_by(|a, b| a.1.date_key.cmp(&b.1.date_key).then_with(|| a.1.id().cmp(b.1.id())).then_with(|| a.0.cmp(b.0)));
        m
    }

    /// Where `item` goes in the chain of its thread, which includes the note `anchor` it continues
    /// (an old note without a `thread-id` takes part by that). The date decides, ties break by the
    /// id text (IMP-070).
    pub fn plan(&self, item: &Ident, anchor: &Path) -> Plan {
        let thread = item.thread_id();
        let mut members: Vec<(&PathBuf, &Meta)> = self.members(&thread, &item.network);
        if !members.iter().any(|(p, _)| p.as_path() == anchor) {
            if let Some((p, m)) = self.notes.iter().find(|(p, _)| p == anchor) {
                members.push((p, m));
                members.sort_by(|a, b| a.1.date_key.cmp(&b.1.date_key).then_with(|| a.1.id().cmp(b.1.id())).then_with(|| a.0.cmp(b.0)));
            }
        }
        // a copy of the same message is no neighbour
        members.retain(|(_, m)| item.id.is_empty() || !m.ids.contains(&item.id));
        let at = members.partition_point(|(_, m)| (m.date_key.as_str(), m.id()) < (item.date_key.as_str(), item.id.as_str()));
        Plan { previous: at.checked_sub(1).map(|i| members[i].0.clone()), next: members.get(at).map(|(p, _)| (*p).clone()) }
    }

    /// The thread of the note `rel` for the overview (NAV-070): its notes oldest first, ties by id.
    /// `None` when the note has no `thread-id`.
    pub fn thread_of(&self, rel: &Path) -> Option<Vec<Row>> {
        let m = self.meta(rel)?;
        if m.thread_id.is_empty() {
            return None;
        }
        Some(self.members(&m.thread_id, &m.network).into_iter().map(|(p, m)| Row { rel: p.clone(), date_key: m.date_key.clone(), who: m.who.clone() }).collect())
    }
}

/// One row of the overview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub rel: PathBuf,
    pub date_key: String,
    pub who: String,
}

/// The note before or after `rel` in `rows` (the overview's order), for the keys (KEY-050); `None`
/// at the ends or when `rel` is not in the list.
pub fn step(rows: &[Row], rel: &Path, delta: isize) -> Option<PathBuf> {
    let at = rows.iter().position(|r| r.rel == rel)?;
    rows.get(at.checked_add_signed(delta)?).map(|r| r.rel.clone())
}

/// `text` with the property `key` set to `value` (YAML text, quoted by the caller): the line is
/// replaced where it is, or added as the last property; nothing else changes (IMP-070, IMP-071).
/// `None` when the note has no front matter or the line is already so.
pub fn set_property(text: &str, key: &str, value: &str) -> Option<String> {
    let n = markdown::front_matter_lines(text);
    if n == 0 {
        return None;
    }
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let close = n - 1;
    let eol = if lines[close.saturating_sub(1)].ends_with("\r\n") { "\r\n" } else { "\n" };
    let new_line = format!("{key}: {value}{eol}");
    let at = (1..close).find(|&i| lines[i].split_once(':').is_some_and(|(k, _)| !k.starts_with(char::is_whitespace) && k.trim().eq_ignore_ascii_case(key)));
    let mut out = String::with_capacity(text.len() + new_line.len());
    match at {
        Some(i) => {
            // the old value may be a block: its lines go with it
            let mut end = i + 1;
            while end < close && (lines[end].starts_with(char::is_whitespace) || lines[end].trim_start().starts_with("- ")) {
                end += 1;
            }
            lines[..i].iter().for_each(|l| out.push_str(l));
            out.push_str(&new_line);
            lines[end..].iter().for_each(|l| out.push_str(l));
        }
        None => {
            lines[..close].iter().for_each(|l| out.push_str(l));
            out.push_str(&new_line);
            lines[close..].iter().for_each(|l| out.push_str(l));
        }
    }
    (out != text).then_some(out)
}
