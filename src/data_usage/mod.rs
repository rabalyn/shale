//! Data usage and caches (USE, raba/shale#51): what Shale keeps on disk ("Saved", read-only)
//! and the caches it may throw away, each with one clear and one prune routine, retention
//! settings and a daily cleanup.
//!
//! - [`scan`]: counts the vault and the person's data outside it (background job).
//! - [`files`]: helpers for caches that are plain files.
//! - [`cleanup`]: when each cache was last cleared or pruned, and when the next cleanup is due.
//! - [`dialog`]: the Data usage dialog and the retention fields of Settings.
//!
//! The clear and prune routines of a cache live in the module that owns it (`social::images`,
//! `social::post_index`, `mail_store`, `tagwords`); [`clear`] and [`prune`] only pick the one.

pub mod cleanup;
pub mod dialog;
pub mod files;
pub mod scan;

use crate::config::CacheRetention;
use crate::mail::store::MailStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// One cache (USE-010…USE-017), in the order the dialog lists them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CacheId {
    SocialPictures,
    PostIndex,
    MailIndex,
    MailBackups,
    MailBroken,
    TagWords,
    UndoHistories,
    DebugLog,
}

impl CacheId {
    pub const ALL: [CacheId; 8] = [CacheId::SocialPictures, CacheId::PostIndex, CacheId::MailIndex, CacheId::MailBackups, CacheId::MailBroken, CacheId::TagWords, CacheId::UndoHistories, CacheId::DebugLog];

    pub fn name(self) -> &'static str {
        match self {
            CacheId::SocialPictures => "Social pictures",
            CacheId::PostIndex => "Social post index",
            CacheId::MailIndex => "Mail index",
            CacheId::MailBackups => "Mail index backups",
            CacheId::MailBroken => "Damaged mail index copies",
            CacheId::TagWords => "Tag words",
            CacheId::UndoHistories => "Undo histories",
            CacheId::DebugLog => "Input debug log",
        }
    }

    /// The label of the button (the mail index is built again, not just emptied).
    pub fn clear_label(self) -> &'static str {
        match self {
            CacheId::MailIndex => "Rebuild",
            _ => "Clear",
        }
    }

    /// What Clear does, shown before a clear that is slow or cannot be undone (USE-008); None:
    /// the cache is cleared at once.
    pub fn confirm_text(self) -> Option<&'static str> {
        match self {
            CacheId::MailIndex => Some(
                "The mail index is built again from the message files. This can take a while for a big mailbox; meanwhile mail does not sync. No message file is touched. Read and flagged marks come back with the next sync; local sent copies you chose to keep here may be listed again.",
            ),
            CacheId::MailBroken => Some("The damaged copies of the mail index are deleted. They cannot be made again; Shale never needs them, they were kept in case you want to look into them."),
            CacheId::UndoHistories => Some("The undo history of every note of this vault is deleted, with the damaged copies of history files. u has nothing to undo afterwards, in the open notes too. They cannot be made again."),
            _ => None,
        }
    }

    /// The caches with no retention setting: they mirror current data (USE-020).
    pub fn kept_current(self) -> bool {
        matches!(self, CacheId::MailIndex | CacheId::TagWords)
    }
}

/// Size, number of entries and the oldest entry of a cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheStats {
    pub bytes: u64,
    pub entries: u64,
    /// seconds since 1970
    pub oldest: Option<i64>,
}

/// What a clear or prune took away. For the mail index rebuild `entries` is the number of
/// messages indexed again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freed {
    pub bytes: u64,
    pub entries: u64,
}

/// Where the caches and the saved data are. The real app uses the user's folders; an app built
/// for tests gets temporary ones, so a test never clears a real cache.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Places {
    pub pictures: PathBuf,
    /// None: the post index lives in memory (tests)
    pub post_index: Option<PathBuf>,
    pub mail_root: PathBuf,
    pub debug_log: PathBuf,
    pub calendars: PathBuf,
    pub calendar_trash: PathBuf,
    pub recovery: PathBuf,
    /// `<data folder>/shale/cache-cleanup.json` (USE-024)
    pub cleanup_file: PathBuf,
    /// the open vault
    pub vault: Option<PathBuf>,
}

impl Places {
    /// The places in one folder (tests, screenshots).
    pub fn in_folder(root: &std::path::Path) -> Places {
        Places {
            pictures: root.join("social"),
            post_index: Some(root.join("social-posts.db")),
            mail_root: root.join("mail"),
            debug_log: root.join("input-debug.log"),
            calendars: root.join("calendars"),
            calendar_trash: root.join("calendar-trash"),
            recovery: root.join("recovery"),
            cleanup_file: root.join("cache-cleanup.json"),
            vault: None,
        }
    }

    /// Where the cache lives, for the dialog.
    pub fn location(&self, id: CacheId) -> String {
        let p = match id {
            CacheId::SocialPictures => self.pictures.clone(),
            CacheId::PostIndex => self.post_index.clone().unwrap_or_else(|| PathBuf::from("(in memory)")),
            CacheId::MailIndex => self.mail_root.join("index.sqlite"),
            CacheId::MailBackups => self.mail_root.join("index.backup-<date>.sqlite"),
            CacheId::MailBroken => self.mail_root.join("index.sqlite.broken-<date>"),
            CacheId::TagWords => self.vault.as_ref().map(|v| crate::import::tagwords::file_in(v)).unwrap_or_else(|| PathBuf::from("(no vault open)")),
            CacheId::UndoHistories => self.vault.as_ref().map(|v| v.join(".shale").join("undo")).unwrap_or_else(|| PathBuf::from("(no vault open)")),
            CacheId::DebugLog => self.debug_log.clone(),
        };
        p.display().to_string()
    }
}

fn file_entry(path: &std::path::Path) -> Vec<files::FileEntry> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str())) else { return vec![] };
    files::entries(dir, |n| n == name)
}

/// Size, entries and age of a cache (a background job).
pub fn stats(id: CacheId, places: &Places) -> Result<CacheStats, String> {
    Ok(match id {
        CacheId::SocialPictures => files::stats(&crate::social::images::cache_files(&places.pictures)),
        CacheId::PostIndex => match &places.post_index {
            Some(p) => crate::social::post_index::stats_file(p)?,
            None => CacheStats::default(),
        },
        CacheId::MailIndex => {
            let base = places.mail_root.join("index.sqlite");
            let bytes = ["", "-wal", "-shm"].iter().filter_map(|e| std::fs::metadata(format!("{}{e}", base.display())).ok()).map(|m| m.len()).sum();
            let entries = if base.exists() { crate::mail::db::open_reader(&base).ok().and_then(|c| c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get::<_, i64>(0)).ok()).unwrap_or(0) as u64 } else { 0 };
            CacheStats { bytes, entries, oldest: None }
        }
        CacheId::MailBackups => files::stats(&crate::mail::store::backup_files(&places.mail_root)),
        CacheId::MailBroken => files::stats(&crate::mail::store::broken_files(&places.mail_root)),
        CacheId::TagWords => match &places.vault {
            Some(v) => files::stats(&file_entry(&crate::import::tagwords::file_in(v))),
            None => CacheStats::default(),
        },
        CacheId::UndoHistories => match &places.vault {
            Some(v) => crate::undo::stats_of_files(v),
            None => CacheStats::default(),
        },
        CacheId::DebugLog => files::stats(&file_entry(&places.debug_log)),
    })
}

/// Empty a cache (USE-010…USE-017). The mail index needs its open store (it is rebuilt by
/// the store's writer); the window resets what it holds in memory when the result arrives.
pub fn clear(id: CacheId, places: &Places, mail: Option<&MailStore>) -> Result<Freed, String> {
    let _flow = tracing::info_span!("flow.data_usage_clear", peer = "app").entered();
    let _step = tracing::info_span!("flow.data_usage_clear.remove_files", peer = "disk").entered();
    match id {
        CacheId::SocialPictures => files::remove(&crate::social::images::cache_files(&places.pictures)),
        CacheId::PostIndex => match &places.post_index {
            Some(p) => crate::social::post_index::clear_file(p),
            None => Ok(Freed::default()),
        },
        CacheId::MailIndex => {
            let store = mail.ok_or("the mail store is not open")?;
            let before = stats(id, places)?.bytes;
            let n = store.rebuild_index().map_err(|e| e.0)?;
            let after = stats(id, places)?.bytes;
            Ok(Freed { bytes: before.saturating_sub(after), entries: n as u64 })
        }
        CacheId::MailBackups => files::remove(&crate::mail::store::backup_files(&places.mail_root)),
        CacheId::MailBroken => files::remove(&crate::mail::store::broken_files(&places.mail_root)),
        CacheId::TagWords => match &places.vault {
            Some(v) => files::remove(&file_entry(&crate::import::tagwords::file_in(v))),
            None => Ok(Freed::default()),
        },
        CacheId::UndoHistories => match &places.vault {
            Some(v) => crate::undo::clear_files(v),
            None => Ok(Freed::default()),
        },
        CacheId::DebugLog => empty_log(&places.debug_log),
    }
}

/// Empty the debug log in place: a recording that is on goes on appending to it (USE-016).
fn empty_log(path: &std::path::Path) -> Result<Freed, String> {
    let Some(f) = file_entry(path).into_iter().next().filter(|f| f.bytes > 0) else { return Ok(Freed::default()) };
    crate::fsutil::open_private_log(path, false).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Freed { bytes: f.bytes, entries: 1 })
}

fn mb(n: Option<u32>) -> Option<u64> {
    n.map(|n| u64::from(n) * 1024 * 1024)
}

/// Remove what the retention settings no longer allow (USE-022).
pub fn prune(id: CacheId, places: &Places, r: &CacheRetention, now: i64) -> Result<Freed, String> {
    let _flow = tracing::info_span!("flow.data_usage_prune", peer = "app").entered();
    let _step = tracing::info_span!("flow.data_usage_prune.remove_files", peer = "disk").entered();
    match id {
        CacheId::SocialPictures => {
            let all = crate::social::images::cache_files(&places.pictures);
            files::remove(&files::over_limits(&all, now, r.picture_days, mb(r.picture_mb), None))
        }
        CacheId::PostIndex => match &places.post_index {
            Some(p) => crate::social::post_index::prune_file(p, r.post_days, r.posts_per_account, now),
            None => Ok(Freed::default()),
        },
        CacheId::MailBackups => {
            let all = crate::mail::store::backup_files(&places.mail_root);
            files::remove(&files::over_limits(&all, now, None, None, r.mail_backups.map(u64::from)))
        }
        CacheId::MailBroken => {
            let all = crate::mail::store::broken_files(&places.mail_root);
            files::remove(&files::over_limits(&all, now, r.broken_days, None, None))
        }
        CacheId::UndoHistories => match &places.vault {
            Some(v) => crate::undo::prune_files(v, now.max(0) as u64, r.undo_days),
            None => Ok(Freed::default()),
        },
        CacheId::DebugLog => match (file_entry(&places.debug_log).first(), mb(r.debug_log_mb)) {
            (Some(f), Some(limit)) if f.bytes > limit => empty_log(&places.debug_log),
            _ => Ok(Freed::default()),
        },
        CacheId::MailIndex | CacheId::TagWords => Ok(Freed::default()),
    }
}

/// Made-up cache files for `--screenshot … --view data-usage` (the places are temporary there).
pub fn seed_demo(p: &Places) -> std::io::Result<()> {
    std::fs::create_dir_all(&p.pictures)?;
    for i in 0..24u64 {
        crate::fsutil::write_atomic(&p.pictures.join(format!("{:016x}.png", i * 7919 + 1)), &vec![0; 40_000 + (i as usize) * 3_000])?;
    }
    std::fs::create_dir_all(&p.mail_root)?;
    for d in ["2026-11-13", "2026-11-14", "2026-11-15"] {
        crate::fsutil::write_atomic(&p.mail_root.join(format!("index.backup-{d}.sqlite")), &vec![0; 900_000])?;
    }
    if let Some(d) = p.debug_log.parent() {
        std::fs::create_dir_all(d)?;
    }
    crate::fsutil::write_atomic(&p.debug_log, &vec![b'k'; 120_000])
}

/// A clear or a prune.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Clear,
    Prune,
}

/// A clear or prune that ended; the window applies what it means for what it holds in memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finished {
    pub id: CacheId,
    pub kind: Kind,
    pub result: Result<Freed, String>,
}

/// "1.4 MB"
pub fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u < UNITS.len() - 1 {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{n} bytes")
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// "3 days", "5 hours", "just now"
pub fn human_age(secs: i64) -> String {
    let (n, unit) = match secs.max(0) {
        s if s < 60 => return "just now".into(),
        s if s < 3_600 => (s / 60, "minute"),
        s if s < 86_400 => (s / 3_600, "hour"),
        s => (s / 86_400, "day"),
    };
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

/// The cleanup waits this long after the start (USE-023).
pub const START_DELAY: Duration = Duration::from_secs(60);
/// ... and runs this often.
pub const EVERY: Duration = Duration::from_secs(24 * 3_600);
/// A changed retention setting prunes its cache this long after the last change.
pub const SETTLE: Duration = Duration::from_secs(2);

/// What a cache-statistics job sends back: every cache's numbers, or why the job died.
type StatsResult = Result<Vec<(CacheId, Result<CacheStats, String>)>, String>;
/// A running clear or prune: what it does and where its result arrives.
type ClearJob = (Kind, Receiver<Result<Result<Freed, String>, String>>);

/// The sub-state of the window: the dialog's numbers, the running jobs, the schedule.
pub struct DataUsage {
    pub places: Places,
    /// what the last count found (None: still counting)
    pub saved: Option<Result<scan::Saved, String>>,
    saved_job: Option<Receiver<Result<scan::Saved, String>>>,
    pub stats: HashMap<CacheId, Result<CacheStats, String>>,
    stats_job: Option<Receiver<StatsResult>>,
    jobs: HashMap<CacheId, ClearJob>,
    pub record: cleanup::Record,
    /// a cache asked about before it is cleared (USE-008)
    pub confirm: Option<CacheId>,
    /// changed retention settings: prune that cache after [`SETTLE`]
    changed: HashMap<CacheId, Instant>,
    started: Instant,
    /// when the scheduled cleanup ran in this run of Shale
    last_cleanup: Option<Instant>,
    /// a problem loading the record file, to tell once
    pub problem: Option<String>,
}

impl DataUsage {
    pub fn new(places: Places) -> DataUsage {
        let loaded = cleanup::load(&places.cleanup_file);
        DataUsage {
            places,
            saved: None,
            saved_job: None,
            stats: HashMap::new(),
            stats_job: None,
            jobs: HashMap::new(),
            record: loaded.value,
            confirm: None,
            changed: HashMap::new(),
            started: Instant::now(),
            last_cleanup: None,
            problem: loaded.problem,
        }
    }

    /// Count again: the vault and the data outside it, and the caches (USE-005).
    pub fn refresh(&mut self, notify: impl Fn() + Send + Clone + 'static) {
        let places = self.places.clone();
        self.saved = None;
        self.saved_job = Some(crate::jobs::spawn(move || scan::saved(&places), notify.clone()));
        self.refresh_stats(notify);
    }

    fn refresh_stats(&mut self, notify: impl Fn() + Send + 'static) {
        let places = self.places.clone();
        self.stats_job = Some(crate::jobs::spawn(move || CacheId::ALL.iter().map(|id| (*id, stats(*id, &places))).collect(), notify));
    }

    /// Whether a clear or prune of this cache runs (its button is disabled, USE-007).
    pub fn running(&self, id: CacheId) -> bool {
        self.jobs.contains_key(&id)
    }

    pub fn any_running(&self) -> bool {
        !self.jobs.is_empty() || self.saved_job.is_some() || self.stats_job.is_some()
    }

    /// Start a clear or prune of one cache in the background; false if one already runs.
    pub fn start(&mut self, id: CacheId, kind: Kind, retention: &CacheRetention, mail: Option<Arc<MailStore>>, notify: impl FnOnce() + Send + 'static) -> bool {
        if self.running(id) {
            return false;
        }
        let places = self.places.clone();
        let r = retention.clone();
        let now = crate::calendar::time::now();
        let rx = crate::jobs::spawn(
            move || match kind {
                Kind::Clear => clear(id, &places, mail.as_deref()),
                Kind::Prune => prune(id, &places, &r, now),
            },
            notify,
        );
        self.jobs.insert(id, (kind, rx));
        true
    }

    /// A retention setting of this cache changed: prune it once the typing settles (USE-023).
    pub fn setting_changed(&mut self, id: CacheId) {
        self.changed.insert(id, Instant::now());
    }

    /// The caches whose scheduled cleanup or settled setting change is due now.
    pub fn due(&mut self, now: Instant, wall: i64) -> Vec<CacheId> {
        let mut out: Vec<CacheId> = self.changed.iter().filter(|(_, t)| now.duration_since(**t) >= SETTLE).map(|(id, _)| *id).collect();
        self.changed.retain(|id, _| !out.contains(id));
        let scheduled = match self.last_cleanup {
            Some(t) => now.duration_since(t) >= EVERY,
            None => now.duration_since(self.started) >= START_DELAY && self.record.due(wall),
        };
        if scheduled {
            self.last_cleanup = Some(now);
            self.record.last_run = Some(wall);
            out.extend(CacheId::ALL.iter().filter(|id| !id.kept_current()));
        }
        out.sort();
        out.dedup();
        out
    }

    /// Collect what ended: the counts, and the clears and prunes (recorded, USE-024). A clear
    /// or prune that ended counts the caches again.
    pub fn poll(&mut self, notify: impl Fn() + Send + 'static) -> Vec<Finished> {
        if let Some(rx) = &self.saved_job {
            if let Ok(r) = rx.try_recv() {
                self.saved = Some(r);
                self.saved_job = None;
            }
        }
        if let Some(rx) = &self.stats_job {
            if let Ok(r) = rx.try_recv() {
                match r {
                    Ok(v) => self.stats.extend(v),
                    Err(e) => self.stats = CacheId::ALL.iter().map(|id| (*id, Err(e.clone()))).collect(),
                }
                self.stats_job = None;
            }
        }
        let mut done = vec![];
        self.jobs.retain(|id, (kind, rx)| match rx.try_recv() {
            Ok(r) => {
                done.push(Finished { id: *id, kind: *kind, result: r.and_then(|r| r) });
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        if !done.is_empty() {
            let wall = crate::calendar::time::now();
            for f in &done {
                self.record.note(f, wall);
            }
            if let Err(e) = self.record.save(&self.places.cleanup_file) {
                self.problem = Some(format!("Could not save when the caches were cleaned ({}): {e}", self.places.cleanup_file.display()));
            }
            self.refresh_stats(notify);
        }
        done
    }

    /// The total size of the caches counted last (for the Settings section).
    pub fn total(&self) -> Option<u64> {
        if self.stats.is_empty() {
            return None;
        }
        Some(self.stats.values().filter_map(|s| s.as_ref().ok()).map(|s| s.bytes).sum())
    }
}

/// The message for a clear or prune that ended (shown once, JOB-002).
pub fn finished_text(f: &Finished) -> String {
    let name = f.id.name();
    match (&f.result, f.kind, f.id) {
        (Err(e), Kind::Clear, _) => format!("{name}: could not {}: {e}", f.id.clear_label().to_lowercase()),
        (Err(e), Kind::Prune, _) => format!("{name}: the cleanup failed: {e}"),
        (Ok(fr), Kind::Clear, CacheId::MailIndex) => format!("Mail index rebuilt from the message files: {} message(s).", fr.entries),
        (Ok(fr), Kind::Clear, _) => format!("{name} cleared: {} freed.", human_bytes(fr.bytes)),
        (Ok(fr), Kind::Prune, _) => format!("{name}: cleanup removed {} entr{} ({}).", fr.entries, if fr.entries == 1 { "y" } else { "ies" }, human_bytes(fr.bytes)),
    }
}

#[cfg(test)]
mod usage_gap_tests;
#[cfg(test)]
mod usage_tests;
