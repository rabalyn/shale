//! Tests of the data usage module (USE): every clear and prune routine on temporary folders,
//! the retention settings, the schedule, the record and the counting.

use super::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};

fn temp(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-usage-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn write(p: &Path, bytes: &[u8]) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, bytes).unwrap();
}

/// Give a file a modification time `days` days ago.
fn age(p: &Path, days: u64) {
    let t = std::time::SystemTime::now() - Duration::from_secs(days * 86_400);
    fs::File::options().write(true).open(p).unwrap().set_modified(t).unwrap();
}

/// A folder with a bit of every cache, and saved data next to them that must stay.
fn places_with_caches(tag: &str) -> (Places, PathBuf) {
    let d = temp(tag);
    let mut p = Places::in_folder(&d);
    let vault = d.join("vault");
    write(&vault.join("Note.md"), b"# hi");
    write(&crate::import::tagwords::file_in(&vault), b"{}");
    p.vault = Some(vault);
    write(&p.pictures.join("0123456789abcdef.png"), &[1; 100]);
    write(&p.pictures.join("fedcba9876543210.jpg"), &[1; 50]);
    write(&p.pictures.join("readme.txt"), b"not a picture of the cache");
    write(&p.mail_root.join("acct1/INBOX/7/1.eml"), b"Subject: keep\r\n\r\nbody");
    write(&p.mail_root.join("index.backup-2026-11-01.sqlite"), &[0; 10]);
    write(&p.mail_root.join("index.backup-2026-11-02.sqlite"), &[0; 10]);
    write(&p.mail_root.join("index.sqlite.broken-2026-10-01"), &[0; 20]);
    write(&p.debug_log, &[b'k'; 30]);
    (p, d)
}

/// A history file with one step per entry of `times` (unix seconds), in the undo folder of the
/// vault of `p`; `version` is that of the file.
fn undo_file(p: &Places, name: &str, version: u32, times: &[i64]) -> PathBuf {
    let steps: Vec<String> = times.iter().map(|t| format!(r#"{{"at": 0, "removed": "", "inserted": "x", "time": {t}}}"#)).collect();
    let text = format!(r#"{{"version": {version}, "path": "{name}.md", "undo": [{}], "redo": [], "known": "x"}}"#, steps.join(","));
    let f = p.vault.as_ref().unwrap().join(".shale/undo").join(format!("{name}.json"));
    write(&f, text.as_bytes());
    f
}

fn undo_names(p: &Places) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(p.vault.as_ref().unwrap().join(".shale/undo")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

// spec: USE-017, USE-006
#[test]
fn the_undo_histories_are_counted_with_the_age_of_the_oldest_step() {
    let (p, d) = places_with_caches("undo-stats");
    assert_eq!(stats(CacheId::UndoHistories, &p), Ok(CacheStats::default()), "no folder, no entries");
    undo_file(&p, "a", 1, &[5_000, 9_000]);
    undo_file(&p, "b", 1, &[7_000]);
    write(&p.vault.as_ref().unwrap().join(".shale/undo/c.json.broken-2026-10-01-000000"), &[0; 7]);
    write(&p.vault.as_ref().unwrap().join(".shale/undo/other.txt"), b"not ours");
    let s = stats(CacheId::UndoHistories, &p).unwrap();
    assert_eq!((s.entries, s.oldest), (3, Some(5_000)), "the files of the cache, the time of the oldest step");
    assert!(s.bytes > 7);
    assert!(std::path::Path::new(&p.location(CacheId::UndoHistories)).ends_with(std::path::Path::new(".shale").join("undo")), "a folder, shown with the separators of the system");
    let mut none = p.clone();
    none.vault = None;
    assert_eq!(stats(CacheId::UndoHistories, &none), Ok(CacheStats::default()));
    assert_eq!(none.location(CacheId::UndoHistories), "(no vault open)");
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-017, USE-022
#[test]
fn the_undo_prune_drops_old_steps_empty_files_and_every_damaged_copy_and_spares_newer_files() {
    let (p, d) = places_with_caches("undo-prune");
    let now = crate::calendar::time::now();
    let day = 86_400;
    let (fresh, old) = (now - day, now - 40 * day);
    undo_file(&p, "mixed", 1, &[old, fresh]);
    undo_file(&p, "fresh", 1, &[fresh]);
    undo_file(&p, "stale", 1, &[old]);
    let newer = undo_file(&p, "newer", 99, &[old]);
    let dir = p.vault.as_ref().unwrap().join(".shale/undo");
    write(&dir.join("x.json.broken-2026-11-15-120000"), b"{");
    write(&dir.join("y.json.broken-2020-01-01-000000-2"), b"{");
    write(&dir.join("other.txt"), b"not ours");
    let r = CacheRetention::default();
    let freed = prune(CacheId::UndoHistories, &p, &r, now).unwrap();
    assert_eq!(freed.entries, 4, "one rewritten, one empty file and two damaged copies");
    assert_eq!(undo_names(&p), ["fresh.json", "mixed.json", "newer.json", "other.txt"]);
    let mixed = fs::read_to_string(dir.join("mixed.json")).unwrap();
    assert!(mixed.contains(&fresh.to_string()) && !mixed.contains(&old.to_string()), "only the old step went: {mixed}");
    assert!(newer.exists(), "a file from a newer Shale is left alone");
    // forever: no step goes, but the damaged copies still do
    write(&dir.join("z.json.broken-2026-11-16-000000"), b"{");
    undo_file(&p, "stale", 1, &[old]);
    let forever = CacheRetention { undo_days: None, ..Default::default() };
    assert_eq!(prune(CacheId::UndoHistories, &p, &forever, now).map(|f| f.entries), Ok(1));
    assert!(undo_names(&p).contains(&"stale.json".to_string()));
    let mut none = p.clone();
    none.vault = None;
    assert_eq!(prune(CacheId::UndoHistories, &none, &r, now), Ok(Freed::default()));
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-017, USE-007
#[test]
fn the_undo_clear_deletes_histories_and_damaged_copies_only_and_the_live_ones_forget() {
    let (p, d) = places_with_caches("undo-clear");
    let now = crate::calendar::time::now();
    let dir = p.vault.as_ref().unwrap().join(".shale/undo");
    undo_file(&p, "a", 1, &[now]);
    undo_file(&p, "b", 1, &[now]);
    let newer = undo_file(&p, "newer", 99, &[now]);
    write(&dir.join("c.json.broken-2026-11-15-120000"), b"{");
    write(&dir.join("d.json"), b"{ garbage");
    write(&dir.join("other.txt"), b"not ours");
    assert_eq!(clear(CacheId::UndoHistories, &p, None).map(|f| f.entries), Ok(4));
    assert_eq!(undo_names(&p), ["newer.json", "other.txt"]);
    assert!(newer.exists() && p.vault.as_ref().unwrap().join("Note.md").exists() && p.vault.as_ref().unwrap().join(".shale/tag-words.json").exists());
    // the histories in memory are emptied by the window (app.rs); here: the helper
    let mut live = crate::undo::Histories::default();
    live.open_vault(p.vault.as_ref().unwrap(), now as u64);
    live.of(Path::new("n.md")).checkpoint("one");
    live.of(Path::new("n.md")).checkpoint("one two");
    live.clear_loaded();
    let mut t = "one two".to_string();
    assert_eq!(live.of(Path::new("n.md")).undo(&mut t), None, "u has nothing to undo");
    let mut none = p.clone();
    none.vault = None;
    assert_eq!(clear(CacheId::UndoHistories, &none, None), Ok(Freed::default()));
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-007, USE-010, USE-013, USE-014, USE-015, USE-016
#[test]
fn each_clear_removes_only_its_own_files_and_never_saved_data() {
    let (p, d) = places_with_caches("clear");
    assert_eq!(clear(CacheId::SocialPictures, &p, None), Ok(Freed { bytes: 150, entries: 2 }));
    assert!(p.pictures.join("readme.txt").exists(), "a file that is not the cache's own stays");
    assert_eq!(clear(CacheId::MailBackups, &p, None), Ok(Freed { bytes: 20, entries: 2 }));
    assert_eq!(clear(CacheId::MailBroken, &p, None), Ok(Freed { bytes: 20, entries: 1 }));
    assert_eq!(clear(CacheId::TagWords, &p, None), Ok(Freed { bytes: 2, entries: 1 }));
    assert_eq!(clear(CacheId::DebugLog, &p, None), Ok(Freed { bytes: 30, entries: 1 }));
    assert_eq!(fs::metadata(&p.debug_log).unwrap().len(), 0, "the log is emptied, not removed (a recording goes on)");
    assert!(clear(CacheId::MailIndex, &p, None).unwrap_err().contains("not open"), "the rebuild needs the open store");
    assert!(p.mail_root.join("acct1/INBOX/7/1.eml").exists() && p.vault.as_ref().unwrap().join("Note.md").exists(), "saved data stays");
    for id in CacheId::ALL.into_iter().filter(|id| *id != CacheId::MailIndex) {
        assert_eq!(clear(id, &p, None).map(|f| f.entries), Ok(0), "{id:?}: clearing an empty cache is fine");
    }
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-012
#[test]
fn the_mail_index_is_rebuilt_through_the_open_store() {
    let (p, d) = places_with_caches("rebuild");
    let store = crate::mail::store::MailStore::open(&p.mail_root).unwrap();
    assert_eq!(store.reader().unwrap().count_messages().unwrap(), 1, "the file was indexed at the start");
    let before = stats(CacheId::MailIndex, &p).unwrap();
    assert!(before.bytes > 0 && before.entries == 1, "{before:?}");
    assert_eq!(clear(CacheId::MailIndex, &p, Some(&store)).map(|f| f.entries), Ok(1));
    drop(store);
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-022
#[test]
fn each_prune_follows_its_retention_settings() {
    let (p, d) = places_with_caches("prune");
    let now = crate::calendar::time::now();
    age(&p.pictures.join("0123456789abcdef.png"), 40);
    let r = CacheRetention::default();
    assert_eq!(prune(CacheId::SocialPictures, &p, &r, now), Ok(Freed { bytes: 100, entries: 1 }), "older than 30 days");
    let tiny = CacheRetention { picture_mb: Some(0), debug_log_mb: Some(0), mail_backups: Some(1), broken_days: Some(30), ..Default::default() };
    assert_eq!(prune(CacheId::SocialPictures, &p, &tiny, now).map(|f| f.entries), Ok(1), "over the size: the oldest go");
    assert_eq!(prune(CacheId::MailBackups, &p, &tiny, now).map(|f| f.entries), Ok(1));
    assert!(p.mail_root.join("index.backup-2026-11-02.sqlite").exists(), "the newest stays");
    let late = crate::calendar::time::days_from_civil(2026, 11, 15) * 86_400;
    assert_eq!(prune(CacheId::MailBroken, &p, &r, late).map(|f| f.entries), Ok(0), "damaged copies are kept forever by default");
    assert_eq!(prune(CacheId::MailBroken, &p, &tiny, late).map(|f| f.entries), Ok(1), "older than 30 days by the date in the name");
    assert_eq!(prune(CacheId::DebugLog, &p, &r, now).map(|f| f.entries), Ok(0), "under 10 MB");
    assert_eq!(prune(CacheId::DebugLog, &p, &tiny, now).map(|f| f.entries), Ok(1), "over the limit: emptied");
    for id in [CacheId::MailIndex, CacheId::TagWords] {
        assert_eq!(prune(id, &p, &tiny, now), Ok(Freed::default()), "{id:?} is kept current");
    }
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-020, USE-017
#[test]
fn the_retention_settings_have_defaults_and_take_forever() {
    let c: crate::config::Config = serde_json::from_str("{}").unwrap();
    assert_eq!(c.caches, CacheRetention::default());
    assert_eq!((c.caches.picture_days, c.caches.picture_mb, c.caches.post_days, c.caches.posts_per_account), (Some(30), Some(500), Some(365), Some(10_000)));
    assert_eq!((c.caches.mail_backups, c.caches.broken_days, c.caches.debug_log_mb, c.caches.undo_days), (Some(3), None, Some(10), Some(30)));
    let c: crate::config::Config = serde_json::from_str(r#"{"caches": {"undo_days": null}}"#).unwrap();
    assert_eq!(c.caches.undo_days, None, "forever");
    let c: crate::config::Config = serde_json::from_str(r#"{"caches": {"picture_days": null, "mail_backups": 5}}"#).unwrap();
    assert_eq!(c.caches.undo_days, Some(30), "a config file from before the setting gets the default");
    let c: crate::config::Config = serde_json::from_str(r#"{"caches": {"picture_days": null, "mail_backups": 5}}"#).unwrap();
    assert_eq!((c.caches.picture_days, c.caches.mail_backups, c.caches.picture_mb), (None, Some(5), Some(500)), "null is forever, missing is the default");
    let back: crate::config::Config = serde_json::from_str(&serde_json::to_string(&c).unwrap()).unwrap();
    assert_eq!(back.caches, c.caches);
    let c: crate::config::Config = serde_json::from_str(r#"{"caches": "oops"}"#).unwrap();
    assert_eq!(c.caches, CacheRetention::default(), "a value of the wrong shape loads as the default");
    for id in CacheId::ALL {
        assert_eq!(dialog::fields(id).is_empty(), id.kept_current(), "{id:?}");
    }
}

// spec: USE-021
#[test]
fn values_out_of_range_are_brought_into_it() {
    let mut r = CacheRetention { picture_days: Some(0), picture_mb: Some(u32::MAX), mail_backups: Some(0), broken_days: None, ..Default::default() };
    dialog::sanitize(&mut r);
    assert_eq!((r.picture_days, r.picture_mb, r.mail_backups, r.broken_days), (Some(1), Some(100_000), Some(1), None));
}

// spec: USE-008, USE-017
#[test]
fn clear_asks_first_only_for_the_mail_index_the_damaged_copies_and_the_undo_histories() {
    let d = temp("confirm");
    let mut u = DataUsage::new(Places::in_folder(&d));
    for id in CacheId::ALL {
        u.confirm = None;
        let asked = dialog::clear_clicked(&mut u, id);
        let slow = matches!(id, CacheId::MailIndex | CacheId::MailBroken | CacheId::UndoHistories);
        assert_eq!(asked.is_none(), slow, "{id:?}");
        assert_eq!(u.confirm == Some(id), slow);
        assert_eq!(id.confirm_text().is_some(), slow);
    }
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-023
#[test]
fn the_cleanup_runs_a_minute_after_the_start_then_daily_and_after_a_setting_settles() {
    let d = temp("schedule");
    let mut u = DataUsage::new(Places::in_folder(&d));
    let t0 = u.started;
    let wall = 1_000_000_000;
    assert!(u.due(t0 + Duration::from_secs(30), wall).is_empty(), "not right at the start");
    let all: Vec<CacheId> = CacheId::ALL.into_iter().filter(|id| !id.kept_current()).collect();
    assert_eq!(u.due(t0 + START_DELAY, wall), all);
    assert_eq!(u.record.last_run, Some(wall));
    assert!(u.due(t0 + START_DELAY + Duration::from_secs(5), wall).is_empty(), "once");
    assert_eq!(u.due(t0 + START_DELAY + EVERY, wall + 86_400), all, "and again a day later");
    u.setting_changed(CacheId::DebugLog);
    let now = Instant::now();
    assert!(u.due(now, wall).is_empty(), "not while the value is still being changed");
    assert_eq!(u.due(now + SETTLE, wall), [CacheId::DebugLog]);
    // a restart within the 24 hours: no cleanup at the start
    let mut again = DataUsage::new(Places::in_folder(&d));
    again.record.last_run = Some(wall);
    assert!(again.due(again.started + START_DELAY, wall + 3_600).is_empty());
    assert!(!again.due(again.started + START_DELAY, wall + 86_400).is_empty());
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-007, USE-024
#[test]
fn a_clear_runs_in_the_background_once_and_is_recorded_across_restarts() {
    let (p, d) = places_with_caches("record");
    let mut u = DataUsage::new(p.clone());
    assert!(u.start(CacheId::SocialPictures, Kind::Clear, &CacheRetention::default(), None, || {}));
    assert!(!u.start(CacheId::SocialPictures, Kind::Clear, &CacheRetention::default(), None, || {}), "never twice at once");
    assert!(u.running(CacheId::SocialPictures));
    let mut done = vec![];
    for _ in 0..200 {
        done.extend(u.poll(|| {}));
        if !done.is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(done, [Finished { id: CacheId::SocialPictures, kind: Kind::Clear, result: Ok(Freed { bytes: 150, entries: 2 }) }]);
    assert!(finished_text(&done[0]).contains("150 bytes freed"), "{}", finished_text(&done[0]));
    assert!(!u.running(CacheId::SocialPictures));
    let again = DataUsage::new(p.clone());
    let last = &again.record.caches[&CacheId::SocialPictures];
    assert_eq!((last.kind, last.freed), (Kind::Clear, Some(Freed { bytes: 150, entries: 2 })));
    assert!(again.record.text(CacheId::SocialPictures, last.at + 7_200).starts_with("cleared 2 hours ago: 2 entries"), "{}", again.record.text(CacheId::SocialPictures, last.at + 7_200));
    assert_eq!(again.record.text(CacheId::DebugLog, 0), "never cleaned");
    // a broken record is kept aside and counts as "never"
    fs::write(&p.cleanup_file, b"{ nope").unwrap();
    let broken = DataUsage::new(p.clone());
    assert!(broken.record.caches.is_empty() && broken.record.due(0));
    assert!(broken.problem.is_some());
    assert!(fs::read_dir(&d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with("cache-cleanup.json.broken-")));
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-002, USE-003, USE-004, USE-005
#[test]
fn saved_counts_the_vault_by_kind_its_largest_parts_and_the_data_outside() {
    let (mut p, d) = places_with_caches("scan");
    let v = p.vault.clone().unwrap();
    write(&v.join("Projects/Plan.excalidraw.md"), &[b'x'; 300]);
    write(&v.join("Projects/pic.PNG"), &[0; 2_000]);
    write(&v.join("paper.pdf"), &[0; 1_000]);
    write(&v.join("data.csv"), b"a,b");
    write(&v.join(".trash/old.md"), b"gone");
    write(&v.join(".git/HEAD"), b"ref");
    write(&v.join(".obsidian/app.json"), b"{}");
    write(&p.mail_root.join("drafts/acct1/x.json"), b"{}");
    write(&p.recovery.join("2026-11-15/Note.md"), b"saved");
    let s = scan::saved(&p);
    let line = |label: &str| s.vault.iter().find(|l| l.label.starts_with(label)).map(|l| (l.files, l.bytes));
    assert_eq!(line("Notes"), Some((1, 4)));
    assert_eq!(line("Drawings"), Some((1, 300)));
    assert_eq!(line("Pictures"), Some((1, 2_000)));
    assert_eq!(line("PDFs"), Some((1, 1_000)));
    assert_eq!(line("Other attachments"), Some((1, 3)));
    assert_eq!(line(".shale/"), Some((1, 2)));
    assert_eq!((line(".trash/"), line(".git/"), line("Other hidden")), (Some((1, 4)), Some((1, 3)), Some((1, 2))));
    assert_eq!(s.largest_files[0], (PathBuf::from("Projects/pic.PNG"), 2_000));
    assert_eq!(s.largest_folders[0], (PathBuf::from("Projects"), 2_300));
    let out = |label: &str| s.outside.iter().find(|l| l.label == label).map(|l| l.files);
    assert_eq!((out("Mail of acct1"), out("Mail drafts"), out("Recovered notes"), out("Mail outbox")), (Some(1), Some(1), Some(1), Some(0)));
    assert!(s.outside.iter().all(|l| l.problem.is_none()), "a folder that is not there yet is no problem");
    p.vault = None;
    assert!(scan::saved(&p).vault.is_empty());
    // the count runs in the background and arrives later
    let mut u = DataUsage::new(p);
    u.refresh(|| {});
    assert!(u.saved.is_none() && u.any_running());
    for _ in 0..200 {
        u.poll(|| {});
        if u.saved.is_some() && u.stats.len() == CacheId::ALL.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(u.saved.as_ref().is_some_and(|s| s.is_ok()));
    assert_eq!(u.stats[&CacheId::SocialPictures], Ok(CacheStats { bytes: 150, entries: 2, oldest: u.stats[&CacheId::SocialPictures].as_ref().unwrap().oldest }));
    assert!(u.total().unwrap() >= 150 + 20 + 20 + 30);
    let _ = fs::remove_dir_all(&d);
}

#[test]
fn sizes_and_ages_read_well() {
    assert_eq!(human_bytes(0), "0 bytes");
    assert_eq!(human_bytes(1536), "1.5 KB");
    assert_eq!(human_bytes(5 * 1024 * 1024 * 1024), "5.0 GB");
    assert_eq!(human_age(10), "just now");
    assert_eq!(human_age(3_600), "1 hour");
    assert_eq!(human_age(3 * 86_400), "3 days");
}

// spec: DEV-031
#[test]
fn dataflow_data_usage_scan() {
    let (p, d) = places_with_caches("dataflow-scan");
    write(&p.vault.clone().unwrap().join("Projects/Plan.md"), b"# plan");
    write(&p.recovery.join("2026-11-15/Note.md"), b"saved");
    let flow = crate::dataflow::record(|| {
        assert!(!scan::saved(&p).vault.is_empty());
    });
    crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/data-usage.md"), "data_usage_scan", &crate::dataflow::render_mermaid(&flow));
    let _ = fs::remove_dir_all(&d);
}

// spec: DEV-031
#[test]
fn dataflow_data_usage_clear() {
    let (p, d) = places_with_caches("dataflow-clear");
    let flow = crate::dataflow::record(|| {
        assert_eq!(clear(CacheId::SocialPictures, &p, None).map(|f| f.entries), Ok(2));
    });
    crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/data-usage.md"), "data_usage_clear", &crate::dataflow::render_mermaid(&flow));
    let _ = fs::remove_dir_all(&d);
}

// spec: DEV-031
#[test]
fn dataflow_data_usage_prune() {
    let (p, d) = places_with_caches("dataflow-prune");
    let r = CacheRetention { picture_days: Some(30), ..CacheRetention::default() };
    age(&p.pictures.join("0123456789abcdef.png"), 90);
    let now = crate::calendar::time::now();
    let flow = crate::dataflow::record(|| {
        assert_eq!(prune(CacheId::SocialPictures, &p, &r, now).map(|f| f.entries), Ok(1));
    });
    crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow/data-usage.md"), "data_usage_prune", &crate::dataflow::render_mermaid(&flow));
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-025
#[test]
fn saved_lists_the_file_history_of_a_vault_without_git_and_not_of_one_with_git() {
    let (p, d) = places_with_caches("history-line");
    let v = p.vault.clone().unwrap();
    let line = |p: &Places| scan::saved(p).outside.into_iter().find(|l| l.label == "File history of this vault");
    // no history yet: an empty line
    let empty = line(&p).expect("a line for a vault without .git");
    assert_eq!((empty.files, empty.bytes, empty.problem), (0, 0, None));
    // a history: the git directory of the vault's ID, counted
    let id = "0123456789abcdef0123456789abcdef";
    write(&v.join(".shale/history-id"), format!("{id}\n").as_bytes());
    write(&p.history.join(id).join("objects/aa/bb"), &[0; 700]);
    write(&p.history.join(id).join("HEAD"), b"ref: refs/heads/main\n");
    write(&p.history.join("fedcba9876543210fedcba9876543210/HEAD"), b"another vault's history");
    let l = line(&p).unwrap();
    assert_eq!((l.files, l.bytes), (2, 700 + 21));
    assert_eq!(l.path.as_deref(), Some(p.history.join(id).as_path()));
    // a vault with .git: its history is in .git, which the vault's own list counts (USE-002)
    write(&v.join(".git/HEAD"), b"ref");
    assert!(line(&p).is_none());
    let _ = fs::remove_dir_all(&d);
}
