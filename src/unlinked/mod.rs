//! The Unlinked notes view (NAV-072…NAV-074, rabalyn/shale#725): the notes and drawings that no link
//! connects, one group at a time (`orphan`: no link in or out; `no backlinks`: nothing links to it),
//! sortable by name and by the date the body was last changed. The rule is the one of the search words
//! (QUERY-031, [`crate::linkage::Linkage`], asked through the search snapshot of the vault).
//!
//! * this file: the pure data, the order and the list kept per state of the vault (PERF-001);
//! * `view`: the widget of the tab.

mod view;

#[cfg(test)]
mod ui_tests;

use crate::vault::{FileKind, Vault};
use std::cmp::Ordering;
use std::path::PathBuf;
use std::sync::Arc;

/// Which unlinked files the view lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// no resolved link in or out
    #[default]
    Orphan,
    /// no other file links to it (it may link out)
    NoBacklinks,
}

impl Mode {
    pub const ALL: [Mode; 2] = [Mode::Orphan, Mode::NoBacklinks];

    /// The name saved with the tab.
    pub fn name(self) -> &'static str {
        match self {
            Mode::Orphan => "orphan",
            Mode::NoBacklinks => "no-backlinks",
        }
    }

    pub fn from_name(name: &str) -> Option<Mode> {
        Mode::ALL.into_iter().find(|m| m.name() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Mode::Orphan => "Orphans",
            Mode::NoBacklinks => "No backlinks",
        }
    }

    fn hint(self) -> &'static str {
        match self {
            Mode::Orphan => "Notes and drawings with no link in and none out.",
            Mode::NoBacklinks => "Notes and drawings that nothing links to; they may link out.",
        }
    }

    fn empty(self) -> &'static str {
        match self {
            Mode::Orphan => "No orphans: every note and drawing has a link.",
            Mode::NoBacklinks => "Every note and drawing is linked from another one.",
        }
    }
}

/// How the list is ordered.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Sort {
    /// the newest change first
    #[default]
    ChangedDesc,
    ChangedAsc,
    NameAsc,
    NameDesc,
}

impl Sort {
    pub const ALL: [Sort; 4] = [Sort::ChangedDesc, Sort::ChangedAsc, Sort::NameAsc, Sort::NameDesc];

    /// The name saved with the tab.
    pub fn name(self) -> &'static str {
        match self {
            Sort::ChangedDesc => "changed-desc",
            Sort::ChangedAsc => "changed-asc",
            Sort::NameAsc => "name-asc",
            Sort::NameDesc => "name-desc",
        }
    }

    pub fn from_name(name: &str) -> Option<Sort> {
        Sort::ALL.into_iter().find(|s| s.name() == name)
    }

    pub fn label(self) -> &'static str {
        match self {
            Sort::ChangedDesc => "Date changed (new to old)",
            Sort::ChangedAsc => "Date changed (old to new)",
            Sort::NameAsc => "Name (A to Z)",
            Sort::NameDesc => "Name (Z to A)",
        }
    }
}

/// One unlinked note or drawing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub rel: PathBuf,
    pub name: String,
    pub drawing: bool,
    /// no resolved link in or out (else only without backlinks)
    pub orphan: bool,
    /// when the body was last changed, in seconds: `modified:` (VAULT-046), else the file's time
    pub changed: Option<i64>,
}

fn secs(t: Option<std::time::SystemTime>) -> Option<i64> {
    t?.duration_since(std::time::SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// Every note and drawing nothing links to (the orphans among them are marked), unordered.
pub fn without_backlinks(vault: &Vault) -> Vec<Entry> {
    let snap = vault.search_snapshot();
    let linkage = snap.linkage();
    snap.notes
        .iter()
        .filter(|n| n.file.kind != FileKind::Other && linkage.has_no_backlinks(&n.file.rel))
        .map(|n| {
            let file_time = secs(n.file.modified);
            let changed = match &n.note {
                Some(index) => crate::note_props::modified(&index.text, file_time),
                None => file_time,
            };
            Entry { rel: n.file.rel.clone(), name: n.file.display_name(), drawing: n.file.kind == FileKind::Drawing, orphan: linkage.is_orphan(&n.file.rel), changed }
        })
        .collect()
}

/// The order of two entries: by the date (a file without one comes last, in both directions) or by
/// the name without regard to case; ties by name, then by path.
pub fn order(sort: Sort, a: &Entry, b: &Entry) -> Ordering {
    let by_name = a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.rel.cmp(&b.rel));
    let by_date = |newest_first: bool| match (a.changed, b.changed) {
        (Some(x), Some(y)) if newest_first => y.cmp(&x),
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    match sort {
        Sort::NameAsc => by_name,
        Sort::NameDesc => by_name.reverse(),
        Sort::ChangedDesc => by_date(true).then(by_name),
        Sort::ChangedAsc => by_date(false).then(by_name),
    }
}

/// One state of one vault (`uid`, scans merged, `generation`).
type VaultState = (u64, u64, u64);

/// The state of the view: the switch, the sort, and the lists made from the vault and kept for its state.
#[derive(Default)]
pub struct Unlinked {
    pub mode: Mode,
    pub sort: Sort,
    /// the heights of the rows of the list, which draws only those in view (PERF-001)
    pub rows: crate::virtual_list::RowHeights,
    /// the files nothing links to, for one state of the vault
    all: Option<(VaultState, Arc<Vec<Entry>>)>,
    /// the list shown, for one state of the vault, mode and sort
    shown: Option<(VaultState, Mode, Sort, Arc<Vec<Entry>>)>,
    /// how many times the vault was read (tests)
    #[cfg(test)]
    pub(crate) reads: usize,
    /// the controls drawn in the last frame and where (tests)
    pub rects: Vec<(String, eframe::egui::Rect)>,
}

impl Unlinked {
    /// The saved state of a tab (NAV-074), from its names; an empty or unknown name is the default.
    pub fn restored(mode: &str, sort: &str) -> Unlinked {
        Unlinked { mode: Mode::from_name(mode).unwrap_or_default(), sort: Sort::from_name(sort).unwrap_or_default(), ..Default::default() }
    }

    /// The entries to show, in order. Made again only when the vault, its state, the mode or the sort
    /// is another, never for the next frame of the same state (PERF-001).
    pub fn list(&mut self, v: &Vault) -> Arc<Vec<Entry>> {
        let state = v.tree_state();
        if let Some((s, m, o, list)) = &self.shown {
            if *s == state && *m == self.mode && *o == self.sort {
                return list.clone();
            }
        }
        let all = match &self.all {
            Some((s, all)) if *s == state => all.clone(),
            _ => {
                #[cfg(test)]
                {
                    self.reads += 1;
                }
                let all = Arc::new(without_backlinks(v));
                self.all = Some((state, all.clone()));
                all
            }
        };
        let mut list: Vec<Entry> = all.iter().filter(|e| self.mode == Mode::NoBacklinks || e.orphan).cloned().collect();
        list.sort_by(|a, b| order(self.sort, a, b));
        let list = Arc::new(list);
        self.shown = Some((state, self.mode, self.sort, list.clone()));
        list
    }
}
