//! What the calendar store does when writing fails or the world changes under it: local calendar
//! files that cannot be written or are gone, one date of a repeating event, and the refresh of a
//! CalDAV calendar after errors and changes on the server.
//!
//! The failures are made with the file system's own refusals that hold for root as well (a folder
//! where the file should be, a file where the folder should be), not with permission bits, which
//! root ignores (the CI container runs as root).

use crate::calendar::ics::{self, Event};
use crate::calendar::store::tests::wait;
use crate::calendar::store::{file_of, CalendarSource, CalendarStore, SourceKind, DEFAULT_SYNC_MINUTES};
use crate::calendar::time as t;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// A fresh folder of its own (tests run in parallel).
fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-calfail-{tag}-{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::SeqCst)));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn has_curl() -> bool {
    std::process::Command::new("curl").arg("--version").output().is_ok()
}

const DAILY: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:daily\r\nDTSTART:20261110T090000Z\r\nDTEND:20261110T093000Z\r\nRRULE:FREQ=DAILY;COUNT=5\r\nSUMMARY:Daily\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
const DAY: i64 = 86_400;

/// A local calendar holding `ics`, loaded into a store.
fn local_with(tag: &str, ics: &str) -> (CalendarStore, CalendarSource, PathBuf) {
    let mut store = CalendarStore::new(vec![], tmp(tag));
    let src = store.new_local_source("Home", &[]);
    let file = file_of(&src).unwrap().to_path_buf();
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, ics).unwrap();
    store.set_sources(vec![src.clone()]);
    (store, src, file)
}

fn event() -> Event {
    Event { uid: "e1".into(), summary: "Dentist".into(), start: 1_800_000_000, end: 1_800_003_600, ..Default::default() }
}

fn trash_count(dir: &Path) -> usize {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|d| std::fs::read_dir(d.path()).map(|r| r.count()).unwrap_or(0)).sum()
}

// spec: CAL-021
#[test]
fn a_local_save_that_cannot_write_says_so_and_changes_nothing() {
    // the calendar's file is a folder
    let (mut store, src, file) = local_with("save-dir", "");
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let err = wait(&store.save_event(&src.id, event(), None, None).unwrap()).unwrap_err();
    assert!(err.starts_with("Could not save to “Home”") && err.contains("could not read"), "{err}");
    store.reload_local();
    assert!(store.events.get(&src.id).is_none_or(|v| v.is_empty()) && file.is_dir(), "nothing was written over the folder");
    assert!(store.status[&src.id].contains("could not read"), "the calendar shows why it is empty: {:?}", store.status);
    // the folder of the file is a file
    let blocked = tmp("save-blocked").join("not-a-folder");
    std::fs::write(&blocked, "x").unwrap();
    let src2 = CalendarSource { kind: SourceKind::Local { file: blocked.join("cal.ics") }, ..src.clone() };
    store.set_sources(vec![src2]);
    let err = wait(&store.save_event(&src.id, event(), None, None).unwrap()).unwrap_err();
    assert!(err.starts_with("Could not save to “Home”"), "{err}");
    assert_eq!(std::fs::read_to_string(&blocked).unwrap(), "x");
    // the calendar was taken out of the settings meanwhile
    let err = store.save_event("gone", event(), None, None).err().unwrap();
    assert!(err.contains("no longer configured"), "{err}");
}

// spec: CAL-021
#[test]
fn a_local_save_after_the_file_vanished_creates_it_again() {
    let (store, src, file) = local_with("save-vanished", DAILY);
    std::fs::remove_file(&file).unwrap();
    wait(&store.save_event(&src.id, event(), None, None).unwrap()).unwrap();
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(text.contains("SUMMARY:Dentist") && !text.contains("Daily"), "only what Shale has now is written: {text}");
    // moving from a calendar that cannot be written: the new one has it, the answer says the old still has it
    let (mut store, src, _) = local_with("move-fail", "");
    let from = store.new_local_source("Old", std::slice::from_ref(&src));
    let from_file = file_of(&from).unwrap().to_path_buf();
    std::fs::create_dir_all(from_file.parent().unwrap()).unwrap();
    std::fs::write(&from_file, "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20270115T080000Z\r\nSUMMARY:Dentist\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n").unwrap();
    store.set_sources(vec![src.clone(), from.clone()]);
    let old = store.find(&from.id, "e1").unwrap();
    std::fs::remove_file(&from_file).unwrap();
    std::fs::create_dir(&from_file).unwrap();
    let msg = wait(&store.save_event(&src.id, old.clone(), Some((from.id.clone(), old)), None).unwrap()).unwrap();
    assert!(msg.starts_with('⚠') && msg.contains("could not remove it from “Old”") && msg.contains("in both calendars"), "{msg}");
    store.reload_local();
    assert!(store.find(&src.id, "e1").is_some(), "the new calendar has it");
}

// spec: CAL-021
#[test]
fn a_local_delete_that_cannot_write_says_so() {
    let (mut store, src, file) = local_with("delete-dir", "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:e1\r\nDTSTART:20270115T080000Z\r\nSUMMARY:Dentist\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n");
    let e = store.find(&src.id, "e1").unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let err = wait(&store.delete_event(&src.id, e, None).unwrap()).unwrap_err();
    assert!(err.starts_with("Could not delete the event from “Home”"), "{err}");
    // read-only and unknown calendars are refused before a job starts
    store.read_only.insert(src.id.clone());
    assert!(store.delete_event(&src.id, event(), None).err().unwrap().contains("read-only"));
    assert!(store.delete_event("gone", event(), None).err().unwrap().contains("no longer configured"));
}

// spec: CAL-007, CAL-021
#[test]
fn deleting_one_date_of_a_repeating_event_leaves_the_others() {
    let (mut store, src, file) = local_with("one-of-five", DAILY);
    let master = store.find(&src.id, "daily").unwrap();
    let third = master.start + 2 * DAY;
    let msg = wait(&store.delete_occurrence(&src.id, &master, third, None).unwrap()).unwrap();
    assert!(msg.starts_with("This date was removed"), "{msg}");
    store.reload_local();
    let from = master.start.div_euclid(DAY) - 1;
    let days: Vec<i64> = store.occurrences(from, from + 10).iter().map(|o| o.start_day() - master.start.div_euclid(DAY)).collect();
    assert_eq!(days, vec![0, 1, 3, 4], "five days, the third one is gone");
    // the same date again does not write it twice
    let master = store.find(&src.id, "daily").unwrap();
    wait(&store.delete_occurrence(&src.id, &master, third, None).unwrap()).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap().matches("EXDATE").count(), 1);
    assert_eq!(trash_count(&store.trash_dir), 2, "each removal keeps a copy first");
    // the first date and the last one too
    wait(&store.delete_occurrence(&src.id, &master, master.start, None).unwrap()).unwrap();
    store.reload_local();
    let master = store.find(&src.id, "daily").unwrap();
    wait(&store.delete_occurrence(&src.id, &master, master.start + 4 * DAY, None).unwrap()).unwrap();
    store.reload_local();
    let days: Vec<i64> = store.occurrences(from, from + 10).iter().map(|o| o.start_day() - master.start.div_euclid(DAY)).collect();
    assert_eq!(days, vec![1, 3]);
}

// spec: CAL-007, CAL-021
#[test]
fn deleting_one_date_that_is_only_a_changed_copy_removes_the_copy() {
    // a file with the changed date but without its series (another program removed the series)
    let ics = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:daily\r\nRECURRENCE-ID:20261111T090000Z\r\nDTSTART:20261111T100000Z\r\nDTEND:20261111T103000Z\r\nSUMMARY:Daily (late)\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    let (mut store, src, file) = local_with("override-only", ics);
    let copy = store.find(&src.id, "daily").unwrap();
    assert!(copy.recurrence_id.is_some());
    wait(&store.delete_occurrence(&src.id, &copy, copy.start, None).unwrap()).unwrap();
    store.reload_local();
    assert!(store.events[&src.id].is_empty(), "the copy is gone");
    assert!(!std::fs::read_to_string(&file).unwrap().contains("Daily"));
    assert_eq!(trash_count(&store.trash_dir), 1);
}

// spec: CAL-021
#[test]
fn deleting_one_date_when_the_file_cannot_be_written_or_has_vanished() {
    // the file became a folder: an error, and the copy of the series is still kept
    let (store, src, file) = local_with("date-dir", DAILY);
    let master = store.find(&src.id, "daily").unwrap();
    std::fs::remove_file(&file).unwrap();
    std::fs::create_dir(&file).unwrap();
    let err = wait(&store.delete_occurrence(&src.id, &master, master.start + DAY, None).unwrap()).unwrap_err();
    assert!(err.starts_with("Could not remove the date from “Home”"), "{err}");
    assert_eq!(trash_count(&store.trash_dir), 1);
    // the folder of the trash is a file: nothing is written and the answer says so
    let (mut store, src, file) = local_with("date-trash", DAILY);
    let master = store.find(&src.id, "daily").unwrap();
    let blocked = tmp("date-trash-blocked").join("trash");
    std::fs::write(&blocked, "x").unwrap();
    store.trash_dir = blocked;
    let err = wait(&store.delete_occurrence(&src.id, &master, master.start + DAY, None).unwrap()).unwrap_err();
    assert!(err.contains("nothing was deleted"), "{err}");
    assert!(!std::fs::read_to_string(&file).unwrap().contains("EXDATE"), "the file was not changed");
    // the file vanished after loading: the series is written back without the date
    let (store, src, file) = local_with("date-vanished", DAILY);
    let master = store.find(&src.id, "daily").unwrap();
    std::fs::remove_file(&file).unwrap();
    wait(&store.delete_occurrence(&src.id, &master, master.start + DAY, None).unwrap()).unwrap();
    let evs = ics::parse_ics(&std::fs::read_to_string(&file).unwrap());
    assert_eq!((evs.len(), evs[0].exdates.clone()), (1, vec![master.start + DAY]));
    // refused before a job starts
    let (mut store, src, _) = local_with("date-ro", DAILY);
    let master = store.find(&src.id, "daily").unwrap();
    store.read_only.insert(src.id.clone());
    assert!(store.delete_occurrence(&src.id, &master, master.start, None).err().unwrap().contains("read-only"));
    assert!(store.delete_occurrence("gone", &master, master.start, None).err().unwrap().contains("no longer configured"));
}

fn dav_source(id: &str, url: String) -> CalendarSource {
    CalendarSource { id: id.into(), name: id.into(), color: "#1971c2".into(), enabled: true, sync_minutes: DEFAULT_SYNC_MINUTES, kind: SourceKind::Caldav { url, username: "ada".into(), password: "x".into() } }
}

/// Poll until no load is running; true when `poll` said so at least once.
fn settle(store: &mut CalendarStore) -> bool {
    let mut changed = false;
    for _ in 0..500 {
        changed |= store.poll();
        if !store.is_loading() {
            return changed;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the load did not finish: {:?}", store.status)
}

fn report_with(summary: &str) -> String {
    let today = t::today();
    let (y, m, d) = t::civil_from_days(today);
    format!(
        r#"<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav"><d:response><d:href>/w/1.ics</d:href><d:propstat><d:prop><c:calendar-data>BEGIN:VCALENDAR
BEGIN:VEVENT
UID:r1
DTSTART;VALUE=DATE:{y:04}{m:02}{d:02}
SUMMARY:{summary}
END:VEVENT
END:VCALENDAR</c:calendar-data></d:prop></d:propstat></d:response></d:multistatus>"#
    )
}

// spec: CAL-022
#[test]
fn a_refresh_that_fails_shows_the_reason_and_keeps_what_was_loaded() {
    if !has_curl() {
        return;
    }
    // the server answers the first load and then fails in different ways
    let mode = Arc::new(Mutex::new(0usize));
    let m2 = mode.clone();
    let (base, _h) = crate::calendar::dav::tests::mock_http_with(60, move |method, _, _, _| {
        let mode = *m2.lock().unwrap();
        match (method, mode) {
            ("PROPFIND", _) => (404, String::new(), String::new()),
            (_, 0) => (207, String::new(), report_with("Before")),
            (_, 1) => (401, String::new(), String::new()),
            (_, 2) => (503, String::new(), "down for maintenance".into()),
            (_, 3) => (200, String::new(), "this is not xml".into()),
            _ => (207, String::new(), report_with("After")),
        }
    });
    let mut store = CalendarStore::new(vec![], tmp("poll-fail"));
    assert!(!store.poll(), "nothing to do: nothing changed");
    store.set_sources(vec![dav_source("w", format!("{base}/w/"))]);
    assert!(settle(&mut store));
    let summaries = |s: &CalendarStore| s.events["w"].iter().map(|e| e.summary.clone()).collect::<Vec<_>>();
    assert_eq!(summaries(&store), vec!["Before"], "{:?}", store.status);
    assert!(store.status["w"].contains("1 events") && store.status["w"].contains("synced"), "{:?}", store.status);
    let g = store.generation;
    for (mode_now, words) in [(1, "refused the user name or password"), (2, "status 503"), (3, "could not be read")] {
        *mode.lock().unwrap() = mode_now;
        store.sync_now("w");
        assert!(settle(&mut store), "{words}");
        assert!(store.status["w"].contains(words), "{words}: {:?}", store.status);
        assert_eq!(summaries(&store), vec!["Before"], "{words}: the calendar keeps its events");
    }
    assert!(store.generation > g);
    // and when the server is back, the next load shows the news
    *mode.lock().unwrap() = 4;
    store.sync_now("w");
    settle(&mut store);
    assert_eq!(summaries(&store), vec!["After"]);
    assert!(store.status["w"].contains("synced"));
}

// spec: CAL-022
#[test]
fn a_server_that_cannot_be_reached_is_shown_and_tried_again_on_the_next_timer() {
    let down = crate::calendar::dav::tests::closed_port();
    let mut store = CalendarStore::new(vec![], tmp("poll-down"));
    store.set_sources(vec![dav_source("w", format!("http://127.0.0.1:{}/w/", down.port))]);
    assert!(store.is_loading());
    assert!(settle(&mut store));
    assert!(!store.status["w"].is_empty() && !store.status["w"].contains("synced"), "{:?}", store.status);
    assert!(store.events.get("w").is_none_or(|v| v.is_empty()));
    // not due before the interval, due after it
    assert!(store.due_sources(std::time::Instant::now()).is_empty());
    assert_eq!(store.due_sources(std::time::Instant::now() + Duration::from_secs(16 * 60)), vec!["w".to_string()]);
}

// spec: CAL-022, CAL-002
#[test]
fn a_changed_version_tag_downloads_again_and_a_changed_permission_is_taken_over() {
    if !has_curl() {
        return;
    }
    let state = Arc::new(Mutex::new(("1".to_string(), false)));
    let reports = Arc::new(AtomicUsize::new(0));
    let (s2, r2) = (state.clone(), reports.clone());
    let (base, _h) = crate::calendar::dav::tests::mock_http_with(60, move |method, _, _, _| {
        if method == "PROPFIND" {
            let (tag, writable) = s2.lock().unwrap().clone();
            let privilege = if writable { "<d:write/>" } else { "<d:read/>" };
            return (
                207,
                String::new(),
                format!(
                    r#"<d:multistatus xmlns:d="DAV:" xmlns:cs="http://calendarserver.org/ns/"><d:response><d:href>/w/</d:href><d:propstat><d:prop><cs:getctag>{tag}</cs:getctag><d:current-user-privilege-set><d:privilege>{privilege}</d:privilege></d:current-user-privilege-set></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response></d:multistatus>"#
                ),
            );
        }
        let n = r2.fetch_add(1, Ordering::SeqCst) + 1;
        (207, String::new(), report_with(&format!("Version {n}")))
    });
    let mut store = CalendarStore::new(vec![], tmp("poll-tag"));
    store.set_sources(vec![dav_source("w", format!("{base}/w/"))]);
    settle(&mut store);
    assert_eq!((reports.load(Ordering::SeqCst), store.events["w"][0].summary.as_str()), (1, "Version 1"));
    assert!(store.read_only.contains("w") && store.status["w"].contains("read-only"), "{:?}", store.status);
    // nobody changed anything: no download, and the status still says it is synced
    store.refresh_source("w");
    settle(&mut store);
    assert_eq!(reports.load(Ordering::SeqCst), 1);
    assert!(store.status["w"].contains("synced"), "{:?}", store.status);
    // somebody changed the calendar on the server, and the account may write now
    *state.lock().unwrap() = ("2".into(), true);
    store.refresh_source("w");
    settle(&mut store);
    assert_eq!((reports.load(Ordering::SeqCst), store.events["w"][0].summary.as_str()), (2, "Version 2"));
    assert!(!store.read_only.contains("w") && !store.status["w"].contains("read-only"), "{:?}", store.status);
}

// spec: CAL-022
#[test]
fn without_a_version_tag_every_refresh_downloads() {
    if !has_curl() {
        return;
    }
    let reports = Arc::new(AtomicUsize::new(0));
    let r2 = reports.clone();
    let (base, _h) = crate::calendar::dav::tests::mock_http_with(60, move |method, _, _, _| {
        if method == "PROPFIND" {
            return (207, String::new(), r#"<d:multistatus xmlns:d="DAV:"/>"#.into());
        }
        r2.fetch_add(1, Ordering::SeqCst);
        (207, String::new(), report_with("Same"))
    });
    let mut store = CalendarStore::new(vec![], tmp("poll-no-tag"));
    store.set_sources(vec![dav_source("w", format!("{base}/w/"))]);
    settle(&mut store);
    store.refresh_source("w");
    settle(&mut store);
    assert_eq!(reports.load(Ordering::SeqCst), 2, "no tag to compare: loaded both times");
    // a disabled calendar is not loaded, a local one is read straight away
    let mut off = dav_source("off", format!("{base}/off/"));
    off.enabled = false;
    store.set_sources(vec![off]);
    store.refresh_source("off");
    assert!(!store.is_loading() && !store.poll());
    assert_eq!(reports.load(Ordering::SeqCst), 2);
}
