//! When each cache was last cleared or pruned and with what result, kept in
//! `<data folder>/shale/cache-cleanup.json` so the daily cleanup counts across restarts (USE-024).

use super::{CacheId, Finished, Freed, Kind};
use crate::vault_state::{Loaded, Versioned};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

/// `<data folder>/shale/cache-cleanup.json`
pub fn default_path() -> std::path::PathBuf {
    crate::userdirs::data_dir().unwrap_or_else(std::env::temp_dir).join("shale").join("cache-cleanup.json")
}

/// The last clear or prune of one cache.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Last {
    pub kind: Kind,
    /// seconds since 1970
    pub at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freed: Option<Freed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub version: u64,
    /// when the scheduled cleanup last ran (seconds since 1970)
    pub last_run: Option<i64>,
    pub caches: BTreeMap<CacheId, Last>,
    /// fields of a newer Shale, written back unchanged
    #[serde(flatten)]
    pub other: Map<String, Value>,
    /// the file may not be written (from a newer Shale, or unreadable)
    #[serde(skip)]
    pub read_only: bool,
}

impl Versioned for Record {
    const CURRENT: u64 = 1;
    const WHAT: &'static str = "cache cleanup record";
    fn step(_json: &mut Map<String, Value>, from: u64) {
        match from {
            // 0 → 1: the first format
            0 => {}
            _ => unreachable!("no migration from version {from}"),
        }
    }
}

/// Load the record; a missing file counts as "never", a broken one is kept aside (CFG-004).
pub fn load(path: &Path) -> Loaded<Record> {
    let mut l = crate::vault_state::load::<Record>(path);
    l.value.version = Record::CURRENT.max(l.value.version);
    l.value.read_only = !l.writable;
    l
}

impl Record {
    /// Whether the scheduled cleanup is due: never ran, or more than 24 h ago (USE-023).
    pub fn due(&self, now: i64) -> bool {
        self.last_run.is_none_or(|t| now - t >= super::EVERY.as_secs() as i64 || t > now)
    }

    /// Remember what a clear or prune did.
    pub fn note(&mut self, f: &Finished, at: i64) {
        let (freed, error) = match &f.result {
            Ok(fr) => (Some(*fr), None),
            Err(e) => (None, Some(e.clone())),
        };
        self.caches.insert(f.id, Last { kind: f.kind, at, freed, error });
    }

    /// Write the record atomically (DATA-004); a record from a newer Shale is left alone.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        crate::vault_state::save(path, self, !self.read_only).map(|_| ())
    }

    /// "pruned 3 hours ago: 12 entries, 1.2 MB" / "never"
    pub fn text(&self, id: CacheId, now: i64) -> String {
        let Some(l) = self.caches.get(&id) else { return "never cleaned".into() };
        let what = match (l.kind, id) {
            (Kind::Clear, CacheId::MailIndex) => "rebuilt",
            (Kind::Clear, _) => "cleared",
            (Kind::Prune, _) => "cleaned up",
        };
        let when = match now - l.at {
            s if s < 60 => format!("{what} just now"),
            s => format!("{what} {} ago", super::human_age(s)),
        };
        match (&l.error, &l.freed) {
            (Some(e), _) => format!("{when}: failed: {e}"),
            (None, Some(fr)) if l.kind == Kind::Clear && id == CacheId::MailIndex => format!("{when}: {} message(s)", fr.entries),
            (None, Some(fr)) => format!("{when}: {} entr{}, {}", fr.entries, if fr.entries == 1 { "y" } else { "ies" }, super::human_bytes(fr.bytes)),
            (None, None) => when,
        }
    }
}
