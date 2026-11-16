//! Versioned JSON settings files, and the per-vault files in `<vault>/.shale/`
//! (ADR-0005; CFG-004, CFG-006…CFG-008).
//!
//! - `settings.json`: vault settings that travel with the vault (people folder, git sync).
//! - `state.json`: session state that may be lost (open tabs, last import folder, recent tags).
//!
//! Every file carries `"version": N`. Loading runs the migration steps in order on the raw
//! JSON; fields this program does not know are kept and written back. A file that cannot be
//! parsed is kept as `<name>.broken-<date>`; a file from a newer Shale is used but never
//! overwritten; a file that cannot be read is never overwritten either.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// A JSON settings file with a format version and explicit migrations.
pub trait Versioned: Serialize + DeserializeOwned + Default {
    /// The version this program writes.
    const CURRENT: u64;
    /// What the file is, for messages ("vault settings").
    const WHAT: &'static str;
    /// Migrate the raw JSON one step, from version `from` to `from + 1`. Version 0 is a file
    /// without a `"version"` field.
    fn step(json: &mut Map<String, Value>, from: u64);
}

/// A loaded file and whether it may be written back.
#[derive(Debug)]
pub struct Loaded<T> {
    pub value: T,
    /// false for a file from a newer Shale or one that could not be read: never overwritten.
    pub writable: bool,
    /// The file was there (missing files start from defaults).
    pub existed: bool,
    /// Something the user should be told.
    pub problem: Option<String>,
}

/// Load a versioned file. Never fails: problems come back in `problem`.
pub fn load<T: Versioned>(path: &Path) -> Loaded<T> {
    let bytes = match crate::fsutil::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Loaded { value: T::default(), writable: true, existed: false, problem: None },
        Err(e) => return Loaded { value: T::default(), writable: false, existed: true, problem: Some(format!("The {} ({}) could not be read, so defaults are used and the file is left alone: {e}", T::WHAT, path.display())) },
    };
    match parse::<T>(&bytes) {
        Ok((value, false)) => Loaded { value, writable: true, existed: true, problem: None },
        Ok((value, true)) => {
            Loaded { value, writable: false, existed: true, problem: Some(format!("The {} ({}) were written by a newer Shale. They are used as far as this version understands them, and not changed.", T::WHAT, path.display())) }
        }
        Err(why) => {
            let kept = broken_name(path);
            match crate::fsutil::rename_noreplace(path, &kept) {
                Ok(()) => Loaded { value: T::default(), writable: true, existed: false, problem: Some(format!("The {} could not be loaded ({why}). The file was kept as {} and defaults are used.", T::WHAT, kept.display())) },
                Err(e) => Loaded { value: T::default(), writable: false, existed: true, problem: Some(format!("The {} could not be loaded ({why}) nor set aside ({e}); defaults are used and the file is left alone.", T::WHAT)) },
            }
        }
    }
}

/// Parse and migrate. Ok((value, newer)) where `newer` = written by a newer version.
pub fn parse<T: Versioned>(bytes: &[u8]) -> Result<(T, bool), String> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let Value::Object(mut map) = v else { return Err("not a JSON object".into()) };
    let version = match map.get("version") {
        None => 0,
        Some(v) => v.as_u64().ok_or("\"version\" is not a number")?,
    };
    if version > T::CURRENT {
        let value = serde_json::from_value(Value::Object(map)).map_err(|e| e.to_string())?;
        return Ok((value, true));
    }
    for from in version..T::CURRENT {
        T::step(&mut map, from);
    }
    map.insert("version".into(), Value::from(T::CURRENT));
    let value = serde_json::from_value(Value::Object(map)).map_err(|e| e.to_string())?;
    Ok((value, false))
}

/// The JSON a value is saved as.
pub fn to_bytes<T: Serialize>(value: &T) -> Vec<u8> {
    let mut s = serde_json::to_string_pretty(value).unwrap_or_default();
    s.push('\n');
    s.into_bytes()
}

/// Save atomically if `writable` and the content differs from the file (so an unchanged vault
/// gives git sync nothing to commit). Returns whether the file was written.
pub fn save<T: Serialize>(path: &Path, value: &T, writable: bool) -> std::io::Result<bool> {
    if !writable {
        return Ok(false);
    }
    let bytes = to_bytes(value);
    if crate::fsutil::read(path).map(|old| old == bytes).unwrap_or(false) {
        return Ok(false);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    crate::fsutil::write_atomic(path, &bytes)?;
    Ok(true)
}

/// `name.broken-YYYY-MM-DD-HHMMSS` next to the file (a number is added if that is taken).
pub fn broken_name(path: &Path) -> PathBuf {
    let secs = crate::calendar::time::now();
    let (days, sod) = crate::calendar::time::utc_to_local(secs);
    let (y, m, d) = crate::calendar::time::civil_from_days(days);
    let stamp = format!("{y:04}-{m:02}-{d:02}-{:02}{:02}{:02}", sod / 3600, sod % 3600 / 60, sod % 60);
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut out = path.with_file_name(format!("{name}.broken-{stamp}"));
    let mut n = 1;
    while out.exists() {
        out = path.with_file_name(format!("{name}.broken-{stamp}-{n}"));
        n += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The vault's own files

/// When the files stored in Git LFS are downloaded (`sync.lfs_download`, SYNC-020).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LfsDownload {
    /// A file is downloaded when it is played or shown and the person asks for it.
    #[default]
    OnDemand,
    /// `git lfs pull` after every successful sync.
    Always,
    /// A value this Shale does not know (written by a newer one): it acts as `OnDemand`, and is
    /// written back as it was until the person changes the setting (CFG-007).
    Other(String),
}

impl Serialize for LfsDownload {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            LfsDownload::OnDemand => "on_demand",
            LfsDownload::Always => "always",
            LfsDownload::Other(v) => v,
        })
    }
}

impl<'de> Deserialize<'de> for LfsDownload {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d)?.as_str() {
            "on_demand" => LfsDownload::OnDemand,
            "always" => LfsDownload::Always,
            other => LfsDownload::Other(other.to_string()),
        })
    }
}

/// Git sync settings of a vault (ADR-0008, SYNC-015).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SyncSettings {
    /// Sync automatically when the vault is a git repository.
    pub enabled: bool,
    /// Minutes between automatic syncs (at least 1).
    pub interval_minutes: u32,
    /// Put in front of every commit message, as written.
    pub commit_prefix: String,
    /// Put after every commit message, as written.
    pub commit_suffix: String,
    /// Download the files Git LFS holds on demand (default) or after every sync (SYNC-020).
    pub lfs_download: LfsDownload,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for SyncSettings {
    fn default() -> Self {
        SyncSettings { enabled: true, interval_minutes: 5, commit_prefix: String::new(), commit_suffix: String::new(), lfs_download: LfsDownload::default(), other: Map::new() }
    }
}

impl SyncSettings {
    pub fn interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.interval_minutes.max(1)) * 60)
    }
}

/// `<vault>/.shale/settings.json`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct VaultSettings {
    pub version: u64,
    /// Folder of the contact notes (for `[[p …` links and people found when importing).
    pub people_dir: String,
    pub sync: SyncSettings,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for VaultSettings {
    fn default() -> Self {
        VaultSettings { version: Self::CURRENT, people_dir: crate::import::people::DEFAULT_DIR.into(), sync: SyncSettings::default(), other: Map::new() }
    }
}

impl Versioned for VaultSettings {
    const CURRENT: u64 = 1;
    const WHAT: &'static str = "vault settings";
    fn step(_json: &mut Map<String, Value>, from: u64) {
        match from {
            // 0 → 1: the first versioned format; the fields are unchanged.
            0 => {}
            _ => unreachable!("no migration from version {from}"),
        }
    }
}

/// `<vault>/.shale/state.json`
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct VaultState {
    pub version: u64,
    /// The tabs open when the app was closed, in tab order, each with its pin (NAV-026).
    pub tabs: Vec<SavedTab>,
    /// The folder used for the last import (vault-relative).
    pub last_import_folder: String,
    /// The folder used for the last saved post (vault-relative); empty: "Social".
    pub last_social_folder: String,
    /// Tags used for imports, most recent first.
    pub recent_tags: Vec<String>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Default for VaultState {
    fn default() -> Self {
        VaultState { version: Self::CURRENT, tabs: vec![], last_import_folder: String::new(), last_social_folder: String::new(), recent_tags: vec![], other: Map::new() }
    }
}

/// One saved tab: a note or drawing (`file`, vault-relative, '/' separated) or a Feeds tab
/// (`feed`), and whether it is pinned.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct SavedTab {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub feed: Option<crate::social::FeedRef>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

impl Versioned for VaultState {
    const CURRENT: u64 = 2;
    const WHAT: &'static str = "vault session state";
    fn step(json: &mut Map<String, Value>, from: u64) {
        match from {
            // 0 → 1: the first versioned format; the fields are unchanged.
            0 => {}
            // 1 → 2 (NAV-026): `open_files`, `pinned_files` and `open_feeds` become one ordered
            // list `tabs`: the notes in their order (pinned ones first), then the Feeds tabs.
            1 => {
                let take = |json: &mut Map<String, Value>, k: &str| match json.remove(k) {
                    Some(Value::Array(a)) => a,
                    _ => vec![],
                };
                let files = take(json, "open_files");
                let pinned = take(json, "pinned_files");
                let feeds = take(json, "open_feeds");
                let mut tabs: Vec<(bool, Value)> = vec![];
                for f in files.into_iter().filter(|f| f.is_string()) {
                    let pin = pinned.contains(&f);
                    tabs.push((pin, serde_json::json!({"file": f, "pinned": pin})));
                }
                tabs.extend(feeds.into_iter().map(|f| (false, serde_json::json!({"feed": f}))));
                tabs.sort_by_key(|(pin, _)| !pin);
                json.insert("tabs".into(), Value::Array(tabs.into_iter().map(|(_, t)| t).collect()));
            }
            _ => unreachable!("no migration from version {from}"),
        }
    }
}

/// Per-vault fields that older versions kept in the global config (moved by CFG-008).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LegacyVaultFields {
    pub people_dir: Option<String>,
    pub open_files: Option<Vec<String>>,
    pub last_import_folder: Option<String>,
    pub recent_tags: Option<Vec<String>>,
}

impl LegacyVaultFields {
    pub fn is_empty(&self) -> bool {
        *self == LegacyVaultFields::default()
    }
}

pub const SETTINGS_FILE: &str = "settings.json";
pub const STATE_FILE: &str = "state.json";

/// A vault's settings and session state, loaded from and saved to its `.shale/` folder.
#[derive(Debug, Default)]
pub struct VaultData {
    /// The vault folder; empty = no vault (nothing is saved).
    pub root: PathBuf,
    pub settings: VaultSettings,
    pub state: VaultState,
    settings_writable: bool,
    state_writable: bool,
    settings_existed: bool,
    state_existed: bool,
}

impl VaultData {
    pub fn settings_path(root: &Path) -> PathBuf {
        root.join(".shale").join(SETTINGS_FILE)
    }

    pub fn state_path(root: &Path) -> PathBuf {
        root.join(".shale").join(STATE_FILE)
    }

    /// Load both files of the vault at `root`; problems are returned for the user.
    pub fn load(root: &Path) -> (VaultData, Vec<String>) {
        let s = load::<VaultSettings>(&Self::settings_path(root));
        let t = load::<VaultState>(&Self::state_path(root));
        let problems = s.problem.iter().chain(t.problem.iter()).cloned().collect();
        let d = VaultData { root: root.to_path_buf(), settings: s.value, state: t.value, settings_writable: s.writable, state_writable: t.writable, settings_existed: s.existed, state_existed: t.existed };
        (d, problems)
    }

    /// Read the settings again (after git sync brought a new version).
    pub fn reload_settings(&mut self) -> Option<String> {
        if self.root.as_os_str().is_empty() {
            return None;
        }
        let s = load::<VaultSettings>(&Self::settings_path(&self.root));
        self.settings = s.value;
        self.settings_writable = s.writable;
        self.settings_existed = s.existed;
        s.problem
    }

    /// Both files may be written.
    pub fn writable(&self) -> bool {
        self.settings_writable && self.state_writable
    }

    /// Whether the settings may be changed (false for a file from a newer Shale).
    pub fn settings_writable(&self) -> bool {
        self.settings_writable
    }

    /// Take over the per-vault fields of an old global config, but only into files this vault
    /// did not have yet (CFG-008). Returns whether anything was taken over.
    pub fn adopt_legacy(&mut self, old: &LegacyVaultFields) -> bool {
        let mut any = false;
        if !self.settings_existed && self.settings_writable {
            if let Some(p) = &old.people_dir {
                self.settings.people_dir = p.clone();
                any = true;
            }
        }
        if !self.state_existed && self.state_writable {
            if let Some(f) = &old.open_files {
                self.state.tabs = f.iter().map(|f| SavedTab { file: Some(f.clone()), ..SavedTab::default() }).collect();
                any = true;
            }
            if let Some(f) = &old.last_import_folder {
                self.state.last_import_folder = f.clone();
                any = true;
            }
            if let Some(t) = &old.recent_tags {
                self.state.recent_tags = t.clone();
                any = true;
            }
        }
        any
    }

    /// Write both files (atomically, only when changed). Files that may not be written are
    /// skipped silently; their problem was reported when they were loaded.
    pub fn save(&mut self) -> Result<(), String> {
        if self.root.as_os_str().is_empty() {
            return Ok(());
        }
        let mut errors = vec![];
        match save(&Self::settings_path(&self.root), &self.settings, self.settings_writable) {
            Ok(w) => self.settings_existed |= w,
            Err(e) => errors.push(format!("Could not save the vault settings: {e}")),
        }
        match save(&Self::state_path(&self.root), &self.state, self.state_writable) {
            Ok(w) => self.state_existed |= w,
            Err(e) => errors.push(format!("Could not save the vault state: {e}")),
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("shale-vstate-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    // spec: CFG-006
    #[test]
    fn settings_migration_0_to_1_keeps_the_fields() {
        let (s, newer) = parse::<VaultSettings>(br#"{"people_dir": "Adressbuch", "sync": {"interval_minutes": 9}}"#).unwrap();
        assert!(!newer);
        assert_eq!(s.version, 1);
        assert_eq!(s.people_dir, "Adressbuch");
        assert_eq!(s.sync.interval_minutes, 9);
        assert!(s.sync.enabled);
    }

    // spec: SYNC-020, CFG-006
    #[test]
    fn the_lfs_download_setting_defaults_to_on_demand_round_trips_and_needs_no_version_bump() {
        // a file from before the setting: on demand, still version 1
        let (old, newer) = parse::<VaultSettings>(br#"{"version": 1, "sync": {"interval_minutes": 9}}"#).unwrap();
        assert!(!newer);
        assert_eq!((old.version, old.sync.lfs_download), (1, LfsDownload::OnDemand));
        assert_eq!(VaultSettings::CURRENT, 1, "adding a field with a default is no new file version");
        // written as text, read back
        let mut s = VaultSettings::default();
        assert_eq!(serde_json::to_value(&s).unwrap()["sync"]["lfs_download"], "on_demand");
        s.sync.lfs_download = LfsDownload::Always;
        let json = serde_json::to_vec(&s).unwrap();
        assert_eq!(serde_json::to_value(&s).unwrap()["sync"]["lfs_download"], "always");
        assert_eq!(parse::<VaultSettings>(&json).unwrap().0.sync.lfs_download, LfsDownload::Always);
        // a value this Shale does not know acts as the default, keeps the file readable and is written back as it was
        let (odd, _) = parse::<VaultSettings>(br#"{"version": 1, "sync": {"lfs_download": "when_on_wifi", "interval_minutes": 9}}"#).unwrap();
        assert_eq!((&odd.sync.lfs_download, odd.sync.interval_minutes), (&LfsDownload::Other("when_on_wifi".into()), 9));
        assert!(!crate::gitsync::lfs::pull_after_sync(&odd.sync.lfs_download, std::path::Path::new("/nonexistent")));
        let back: Value = serde_json::from_slice(&serde_json::to_vec(&odd).unwrap()).unwrap();
        assert_eq!(back["sync"]["lfs_download"], "when_on_wifi");
        // ... also through a save of the vault's files
        let root = tmp("lfs-unknown");
        let p = VaultData::settings_path(&root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, br#"{"version": 1, "sync": {"lfs_download": "when_on_wifi"}}"#).unwrap();
        let (mut d, _) = VaultData::load(&root);
        d.settings.people_dir = "Q".into();
        d.save().unwrap();
        let v: Value = serde_json::from_slice(&crate::fsutil::read(&p).unwrap()).unwrap();
        assert_eq!(v["sync"]["lfs_download"], "when_on_wifi");
        // the person changing the setting replaces it
        d.settings.sync.lfs_download = LfsDownload::OnDemand;
        d.save().unwrap();
        let v: Value = serde_json::from_slice(&crate::fsutil::read(&p).unwrap()).unwrap();
        assert_eq!(v["sync"]["lfs_download"], "on_demand");
        let _ = std::fs::remove_dir_all(&root);
    }

    fn file_tab(f: &str, pinned: bool) -> SavedTab {
        SavedTab { file: Some(f.into()), pinned, ..SavedTab::default() }
    }

    fn feed_tab(account: &str, feed: &str) -> SavedTab {
        SavedTab { feed: Some(crate::social::FeedRef { account: account.into(), feed: feed.into() }), ..SavedTab::default() }
    }

    // spec: CFG-006
    #[test]
    fn state_migration_0_to_1_to_2_makes_one_tab_list() {
        let (s, newer) = parse::<VaultState>(br#"{"open_files": ["a.md"], "recent_tags": ["x"]}"#).unwrap();
        assert!(!newer);
        assert_eq!(s.version, 2);
        assert_eq!(s.tabs, vec![file_tab("a.md", false)]);
        assert_eq!(s.recent_tags, vec!["x".to_string()]);
    }

    // spec: NAV-026
    #[test]
    fn an_old_state_file_with_two_lists_loads_as_one_ordered_tab_list() {
        let old = br#"{"version": 1, "open_files": ["a.md", "b.md", "c.md"], "pinned_files": ["b.md"],
            "open_feeds": [{"account": "x", "feed": "home"}], "future": 7}"#;
        let (s, _) = parse::<VaultState>(old).unwrap();
        assert_eq!(s.tabs, vec![file_tab("b.md", true), file_tab("a.md", false), file_tab("c.md", false), feed_tab("x", "home")]);
        let text = String::from_utf8(to_bytes(&s)).unwrap();
        assert!(text.contains("\"future\""), "unknown fields are kept");
        assert!(!text.contains("open_files") && !text.contains("pinned_files") && !text.contains("open_feeds"));
        // the saved list round-trips, with pins and Feeds tabs in place
        let mixed = VaultState { tabs: vec![file_tab("d.canvas", true), feed_tab("x", "home"), file_tab("a.md", false)], ..VaultState::default() };
        let (back, _) = parse::<VaultState>(&to_bytes(&mixed)).unwrap();
        assert_eq!(back.tabs, mixed.tabs);
    }

    // spec: CFG-007
    #[test]
    fn unknown_fields_in_a_saved_tab_are_kept() {
        let (s, _) = parse::<VaultState>(br#"{"version": 2, "tabs": [{"file": "a.md", "scroll": 4}]}"#).unwrap();
        assert!(String::from_utf8(to_bytes(&s)).unwrap().contains("\"scroll\""));
    }

    // spec: CFG-006
    #[test]
    fn vault_files_round_trip_and_unchanged_files_are_not_rewritten() {
        let root = tmp("roundtrip");
        let (mut d, problems) = VaultData::load(&root);
        assert!(problems.is_empty());
        d.settings.people_dir = "Leute".into();
        d.settings.sync.commit_prefix = "[notes] ".into();
        d.state.tabs = vec![file_tab("Welcome.md", false)];
        d.save().unwrap();
        let text = crate::fsutil::read_to_string(VaultData::settings_path(&root)).unwrap();
        assert!(text.contains("\"version\": 1"), "{text}");
        let mtime = std::fs::metadata(VaultData::settings_path(&root)).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let (mut again, _) = VaultData::load(&root);
        assert_eq!(again.settings, d.settings);
        assert_eq!(again.state, d.state);
        again.save().unwrap();
        assert_eq!(std::fs::metadata(VaultData::settings_path(&root)).unwrap().modified().unwrap(), mtime);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-007
    #[test]
    fn unknown_fields_are_written_back_unchanged() {
        let root = tmp("unknown");
        let p = VaultData::settings_path(&root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, br#"{"version": 1, "people_dir": "P", "future": {"a": [1, 2]}, "sync": {"enabled": false, "later": true}}"#).unwrap();
        let (mut d, _) = VaultData::load(&root);
        assert!(!d.settings.sync.enabled);
        d.settings.people_dir = "Q".into();
        d.save().unwrap();
        let v: Value = serde_json::from_slice(&crate::fsutil::read(&p).unwrap()).unwrap();
        assert_eq!(v["future"], serde_json::json!({"a": [1, 2]}));
        assert_eq!(v["sync"]["later"], Value::Bool(true));
        assert_eq!(v["people_dir"], "Q");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-007
    #[test]
    fn a_file_from_a_newer_shale_is_used_but_never_overwritten() {
        let root = tmp("newer");
        let p = VaultData::settings_path(&root);
        let original = br#"{"version": 99, "people_dir": "Kontakte", "sync": {"enabled": false}}"#;
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, original).unwrap();
        let (mut d, problems) = VaultData::load(&root);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].contains("newer"));
        assert_eq!(d.settings.people_dir, "Kontakte");
        assert!(!d.settings_writable());
        d.settings.people_dir = "x".into();
        d.save().unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), original);
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-004
    #[test]
    fn a_broken_file_is_kept_aside_and_defaults_are_used() {
        let root = tmp("broken");
        let p = VaultData::state_path(&root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, b"{ not json").unwrap();
        let (mut d, problems) = VaultData::load(&root);
        assert_eq!(problems.len(), 1);
        assert_eq!(d.state, VaultState::default());
        let kept: Vec<_> = std::fs::read_dir(root.join(".shale")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("state.json.broken-")).collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(std::fs::read(root.join(".shale").join(&kept[0])).unwrap(), b"{ not json");
        assert!(problems[0].contains(&kept[0]));
        // a wrong type is broken too
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, br#"{"version": 2, "tabs": 5}"#).unwrap();
        let (_, problems) = VaultData::load(&root);
        assert_eq!(problems.len(), 1);
        d.save().unwrap();
        assert!(p.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-004
    #[test]
    fn a_broken_settings_file_is_kept_aside_too_and_never_overwritten() {
        let root = tmp("broken-settings");
        let p = VaultData::settings_path(&root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, b"{ \"sync\": [oops").unwrap();
        let (mut d, problems) = VaultData::load(&root);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(d.settings, VaultSettings::default());
        let kept: Vec<_> = std::fs::read_dir(root.join(".shale")).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("settings.json.broken-")).collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(std::fs::read(root.join(".shale").join(&kept[0])).unwrap(), b"{ \"sync\": [oops");
        // the next save writes the defaults to the path and leaves the kept file alone
        d.settings.people_dir = "x".into();
        d.save().unwrap();
        assert_eq!(std::fs::read(root.join(".shale").join(&kept[0])).unwrap(), b"{ \"sync\": [oops");
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-006
    #[test]
    fn reload_settings_reads_what_git_sync_brought_and_reports_a_broken_file() {
        let root = tmp("reload");
        let (mut d, _) = VaultData::load(&root);
        assert!(!d.settings_existed);
        let p = VaultData::settings_path(&root);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&p, br#"{"version": 1, "people_dir": "Kontakte"}"#).unwrap();
        assert!(d.reload_settings().is_none());
        assert_eq!(d.settings.people_dir, "Kontakte");
        assert!(d.settings_existed && d.settings_writable());
        // a file of a newer Shale is used, not writable, and reported
        crate::fsutil::write_atomic(&p, br#"{"version": 99, "people_dir": "Neu"}"#).unwrap();
        assert!(d.reload_settings().unwrap().contains("newer"));
        assert_eq!(d.settings.people_dir, "Neu");
        assert!(!d.settings_writable());
        // a broken one is kept aside and the defaults are used
        crate::fsutil::write_atomic(&p, b"{ nope").unwrap();
        assert!(d.reload_settings().unwrap().contains("settings.json.broken-"));
        assert_eq!(d.settings, VaultSettings::default());
        // no vault open: nothing to read
        let mut none = VaultData::default();
        assert!(none.reload_settings().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    // spec: CFG-008
    #[test]
    fn legacy_fields_fill_only_files_the_vault_did_not_have() {
        let old = LegacyVaultFields { people_dir: Some("Adressbuch".into()), open_files: Some(vec!["a.md".into()]), last_import_folder: Some("In".into()), recent_tags: Some(vec!["t".into()]) };
        let fresh = tmp("legacy-fresh");
        let (mut d, _) = VaultData::load(&fresh);
        assert!(d.adopt_legacy(&old));
        assert_eq!(d.settings.people_dir, "Adressbuch");
        assert_eq!(d.state.tabs, vec![file_tab("a.md", false)]);
        assert_eq!(d.state.last_import_folder, "In");
        let had = tmp("legacy-had");
        std::fs::create_dir_all(VaultData::settings_path(&had).parent().unwrap()).unwrap();
        crate::fsutil::write_atomic(&VaultData::settings_path(&had), br#"{"version": 1, "people_dir": "Mine"}"#).unwrap();
        let (mut d, _) = VaultData::load(&had);
        d.adopt_legacy(&old);
        assert_eq!(d.settings.people_dir, "Mine");
        assert_eq!(d.state.recent_tags, vec!["t".to_string()]);
        let _ = std::fs::remove_dir_all(&fresh);
        let _ = std::fs::remove_dir_all(&had);
    }
}
