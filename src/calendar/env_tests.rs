//! Tests against the real CalDAV server of the local container environment
//! (`tests/caldav-env/`, CALENV-001): Radicale behind TLS with a seeded user and calendars. They
//! are ignored unless asked for, because they need the containers:
//!
//! ```text
//! tests/caldav-env/caldav-env.sh up
//! cargo test -- --ignored caldav_env
//! ```
//!
//! Trust: the server's certificate is made by `setup-certs.sh`; here it is trusted as an extra
//! authority by test code only (`dav::TEST_CA`, handed to curl as `--cacert`). The production
//! client has no such switch (CALENV-002).
//!
//! The tests share one server, so they take turns (`TURN`). Every event they make has a unique
//! UID starting with `caldav-env-`; each test removes what it made, and the next test first
//! removes what a failed earlier run left. The seeded events are only read.

use crate::calendar::dav::{self, Account, TEST_CA};
use crate::calendar::ics::Event;
use crate::calendar::store::{CalendarSource, CalendarStore, SourceKind, DEFAULT_SYNC_MINUTES};
use crate::calendar::time as t;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

const SERVER: &str = "https://localhost:15232/";
const USER: &str = "shale";
const PASSWORD: &str = "shale-test-pw";
const SCRATCH_PREFIX: &str = "caldav-env-";
const CALENDARS: [&str; 3] = ["work", "private", "moves"];

static TURN: Mutex<()> = Mutex::new(());

fn cal(name: &str) -> String {
    format!("{SERVER}{USER}/{name}/")
}

fn acct_for(url: &str, password: &str) -> Account {
    Account { url: url.into(), username: USER.into(), password: password.into() }
}

fn acct(name: &str) -> Account {
    acct_for(&cal(name), PASSWORD)
}

fn ca_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("caldav-env").join("certs").join("ca.crt")
}

/// Everything in a calendar, whatever its date.
fn all(name: &str) -> Vec<Event> {
    dav::fetch_events(&cal(name), &acct(name), 0, 4_000_000_000).unwrap_or_else(|e| panic!("could not load {name}: {e}. Start the environment first: tests/caldav-env/caldav-env.sh up"))
}

fn scratch_in(name: &str) -> Vec<Event> {
    all(name).into_iter().filter(|e| e.uid.starts_with(SCRATCH_PREFIX)).collect()
}

fn remove_scratch() {
    for name in CALENDARS {
        for e in scratch_in(name) {
            if let Some(h) = &e.href {
                let _ = dav::request_with("DELETE", h, &[], "", &acct(name));
            }
        }
    }
}

/// One test at a time; trusts the test CA; removes what an earlier run left, and what this test
/// made when it ends (also when it fails).
struct Env {
    _turn: MutexGuard<'static, ()>,
}

impl Env {
    fn start() -> Env {
        let turn = TURN.lock().unwrap_or_else(|e| e.into_inner());
        let ca = ca_path();
        assert!(ca.exists(), "no {}. Start the environment first: tests/caldav-env/caldav-env.sh up", ca.display());
        *TEST_CA.lock().unwrap() = Some(ca);
        remove_scratch();
        Env { _turn: turn }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        *TEST_CA.lock().unwrap() = Some(ca_path());
        remove_scratch();
        *TEST_CA.lock().unwrap() = None;
    }
}

fn unique_uid(tag: &str) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    format!("{SCRATCH_PREFIX}{tag}-{}-{nanos}@shale.test", std::process::id())
}

/// A new one-hour event three days from now.
fn new_event(tag: &str, summary: &str) -> Event {
    let start = (t::today() + 3) * 86_400 + 10 * 3600;
    Event { uid: unique_uid(tag), summary: summary.into(), start, end: start + 3600, ..Default::default() }
}

fn find(name: &str, uid: &str) -> Vec<Event> {
    all(name).into_iter().filter(|e| e.uid == uid).collect()
}

fn find_one(name: &str, uid: &str) -> Event {
    let mut v = find(name, uid);
    assert_eq!(v.len(), 1, "{name} should hold exactly one part of {uid}: {v:?}");
    v.remove(0)
}

fn dav_source(id: &str, name: &str) -> CalendarSource {
    CalendarSource { id: id.into(), name: name.into(), color: "#1971c2".into(), enabled: true, sync_minutes: DEFAULT_SYNC_MINUTES, kind: SourceKind::Caldav { url: cal(id), username: USER.into(), password: PASSWORD.into() } }
}

fn store(tag: &str) -> (PathBuf, CalendarStore) {
    let dir = std::env::temp_dir().join(format!("shale-calenv-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = CalendarStore::new(vec![dav_source("work", "Work"), dav_source("private", "Private"), dav_source("moves", "Moves")], dir.clone());
    (dir, store)
}

/// Load every calendar of the store from the server and wait until it is done.
fn load(store: &mut CalendarStore) {
    store.refresh_remote();
    for _ in 0..500 {
        store.poll();
        if !store.is_loading() {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("the calendars did not load: {:?}", store.status);
}

fn finish(store: &mut CalendarStore, job: &crate::calendar::store::Job, ids: &[&str]) -> Result<String, String> {
    let r = crate::calendar::store::tests::wait(job);
    let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
    store.after_write(&ids);
    load(store);
    r
}

/// Check the diagram of one flow against its block in `docs/dataflow/<doc>` (DEV-026, DEV-028): the
/// spans named like the flow's root are found in a recording wherever they lie, and only the steps
/// named after the flow itself stay (what other flows did meanwhile is their own diagram).
fn verify_flow(forest: &[crate::dataflow::Node], root: &str, flow: &str, doc: &str) {
    use crate::dataflow::Node;
    fn own(nodes: &[Node], prefix: &str) -> Vec<Node> {
        nodes.iter().filter(|n| n.name.starts_with(prefix)).map(|n| Node { children: own(&n.children, prefix), ..n.clone() }).collect()
    }
    fn find(nodes: &[Node], root: &str, out: &mut Vec<Node>) {
        for n in nodes {
            if n.name == root {
                out.push(Node { children: own(&n.children, &format!("{root}.")), ..n.clone() });
            } else {
                find(&n.children, root, out);
            }
        }
    }
    let mut mine = Vec::new();
    find(forest, root, &mut mine);
    assert!(!mine.is_empty(), "the run recorded no {root}");
    crate::dataflow::verify(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("docs/dataflow").join(doc), flow, &crate::dataflow::render_mermaid(&mine));
}

// spec: CALENV-002
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_discovers_and_loads_the_seeded_calendars() {
    let _env = Env::start();
    let forest = crate::dataflow::record(|| {
        // discovery from the bare server address (it goes through /.well-known/caldav and its redirect)
        let mut found = dav::discover(&acct_for(SERVER, PASSWORD)).expect("discovery");
        found.sort_by(|a, b| a.name.cmp(&b.name));
        let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Moves", "Private", "Work"], "{found:?}");
        assert!(found.iter().all(|c| !c.read_only && c.url.starts_with(SERVER)), "{found:?}");
        assert_eq!(found.iter().find(|c| c.name == "Work").unwrap().color.as_deref(), Some("#1971c2"));

        let work = all("work");
        let seed = |uid: &str| -> Vec<&Event> { work.iter().filter(|e| e.uid == format!("seed-{uid}@shale.test")).collect() };
        assert_eq!(work.iter().filter(|e| e.uid.starts_with("seed-")).map(|e| e.uid.as_str()).collect::<std::collections::BTreeSet<_>>().len(), 6, "{:?}", work.iter().map(|e| &e.summary).collect::<Vec<_>>());
        // single events, with their address and version tag
        let standup = seed("standup")[0];
        assert_eq!((standup.summary.as_str(), standup.location.as_str(), standup.end - standup.start), ("Standup", "Room 1", 1800));
        assert!(standup.href.as_deref().is_some_and(|h| h.starts_with(&cal("work"))) && standup.etag.is_some());
        assert_eq!(seed("umlaut")[0].summary, "Übergabe: Café & Größe");
        // an event with a reminder
        assert_eq!(seed("reminder")[0].alarms.iter().map(|a| a.before).collect::<Vec<_>>(), [Some(900)]);
        // an all-day event
        let allday = seed("allday")[0];
        assert!(allday.all_day && allday.end - allday.start == 86_400, "{allday:?}");
        // a repeating event with a skipped date and a changed one: both parts are in one file
        let weekly = seed("weekly");
        assert_eq!(weekly.len(), 2, "{weekly:?}");
        let master = weekly.iter().find(|e| e.recurrence_id.is_none()).expect("master");
        let moved = weekly.iter().find(|e| e.recurrence_id.is_some()).expect("changed date");
        assert!(master.rrule.as_deref().is_some_and(|r| r.contains("WEEKLY")) && master.exdates.len() == 1, "{master:?}");
        assert_eq!((moved.summary.as_str(), moved.start - moved.recurrence_id.unwrap()), ("Weekly sync (moved)", 2 * 3600));
        assert_eq!(master.href, moved.href);

        let private: Vec<String> = all("private").into_iter().map(|e| e.summary).collect();
        assert!(private.contains(&"Doctor".to_string()) && private.contains(&"Birthday".to_string()), "{private:?}");
        assert!(all("moves").iter().all(|e| !e.uid.starts_with("seed-")), "the calendar for moves starts empty");

        // the version tag and write permission
        let st = dav::calendar_state(&cal("work"), &acct("work")).unwrap();
        assert!(st.tag.is_some() && st.read_only == Some(false), "{st:?}");

        // the same through the calendar store: the repeat is expanded with its skipped and changed date
        let (dir, mut store) = store("load");
        load(&mut store);
        let today = t::today();
        let occ = store.occurrences(today - 30, today + 60);
        let weekly: Vec<_> = occ.iter().filter(|o| store.event(o).unwrap().uid == "seed-weekly@shale.test").collect();
        assert_eq!(weekly.len(), 5, "6 weekly dates, one skipped, one of them changed but still there");
        assert_eq!(weekly.iter().filter(|o| store.event(o).unwrap().summary == "Weekly sync (moved)").count(), 1);
        assert!(store.status["work"].contains("events"), "{:?}", store.status);
        let _ = std::fs::remove_dir_all(dir);
    });
    verify_flow(&forest, "flow.caldav_discover", "caldav_discover", "caldav-load.md");
    verify_flow(&forest, "flow.caldav_load", "caldav_load", "caldav-load.md");
}

// spec: CALENV-002
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_creates_edits_and_deletes_an_event() {
    let _env = Env::start();
    let forest = crate::dataflow::record(|| {
        let (a, name) = (acct("private"), "private");
        let mut e = new_event("crud", "Created by the test");
        e.description = "first".into();
        e.alarms = vec![crate::calendar::ics::Alarm::before_start(600)];
        let (href, etag) = dav::put_event(&cal(name), &a, std::slice::from_ref(&e), None, &[]).expect("create");
        assert!(href.starts_with(&cal(name)) && etag.is_some(), "{href} {etag:?}");

        let on_server = find_one(name, &e.uid);
        assert_eq!((on_server.summary.as_str(), on_server.description.as_str(), on_server.start, on_server.end), ("Created by the test", "first", e.start, e.end));
        assert_eq!(on_server.alarms.iter().map(|x| x.before).collect::<Vec<_>>(), [Some(600)]);
        assert_eq!((on_server.href.as_deref(), on_server.etag.as_deref()), (Some(href.as_str()), etag.as_deref()));

        // the same UID can't be created twice
        let err = dav::put_event(&cal(name), &a, std::slice::from_ref(&e), None, &[]).unwrap_err();
        assert!(err.contains("already exists"), "{err}");

        // edit in place: same UID, same address, a new version tag
        let mut edited = on_server.clone();
        edited.summary = "Edited by the test".into();
        edited.description = "second".into();
        let (href2, etag2) = dav::put_event(&cal(name), &a, std::slice::from_ref(&edited), Some((&href, etag.as_deref())), std::slice::from_ref(&on_server)).expect("edit");
        assert_eq!(href2, href);
        assert!(etag2.is_some() && etag2 != etag, "{etag:?} {etag2:?}");
        let after = find_one(name, &e.uid);
        assert_eq!((after.summary.as_str(), after.description.as_str(), after.etag.as_deref()), ("Edited by the test", "second", etag2.as_deref()));
        assert_eq!(after.alarms.len(), 1, "the reminder was kept");

        // delete with the current version tag
        dav::delete_event(&href, &a, etag2.as_deref(), std::slice::from_ref(&after)).expect("delete");
        assert!(find(name, &e.uid).is_empty());
        // an event that is already gone counts as removed
        dav::delete_event(&href, &a, None, std::slice::from_ref(&after)).expect("delete again");
    });
    verify_flow(&forest, "flow.caldav_edit", "caldav_edit", "caldav-edit.md");
}

// spec: CALENV-002
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_edits_and_removes_one_date_of_a_repeating_event() {
    let _env = Env::start();
    let (a, name) = (acct("work"), "work");
    let mut series = new_event("series", "Daily test");
    series.rrule = Some("FREQ=DAILY;COUNT=5".into());
    dav::put_event(&cal(name), &a, std::slice::from_ref(&series), None, &[]).expect("create the series");
    let master = find_one(name, &series.uid);

    // change the third date only: stored as its own copy beside the series, in the same file
    let third = master.start + 2 * 86_400;
    let mut changed = master.override_at(third + 1800, third + 1800 + 3600);
    changed.recurrence_id = Some(third);
    changed.summary = "Daily test (third, later)".into();
    dav::put_event(&cal(name), &a, &[changed, master.clone()], Some((master.href.as_deref().unwrap(), master.etag.as_deref())), std::slice::from_ref(&master)).expect("edit one date");
    let parts = find(name, &series.uid);
    assert_eq!(parts.len(), 2, "{parts:?}");
    let over = parts.iter().find(|p| p.recurrence_id.is_some()).unwrap();
    assert_eq!((over.summary.as_str(), over.recurrence_id, over.start), ("Daily test (third, later)", Some(third), third + 1800));
    let master_now = parts.iter().find(|p| p.recurrence_id.is_none()).unwrap();
    assert_eq!((master_now.summary.as_str(), master_now.rrule.as_deref()), ("Daily test", Some("FREQ=DAILY;COUNT=5")));

    // remove the first date through the calendar store: the series gets an excluded date, the changed date stays
    let (dir, mut store) = store("dates");
    load(&mut store);
    let ev = store.find("work", &series.uid).expect("the series is loaded");
    assert!(ev.recurrence_id.is_none());
    let job = store.delete_occurrence("work", &ev, ev.start, None).expect("job");
    let msg = finish(&mut store, &job, &["work"]).expect("remove one date");
    assert!(msg.contains("This date was removed"), "{msg}");
    let parts = find(name, &series.uid);
    assert_eq!(parts.len(), 2, "{parts:?}");
    let master_now = parts.iter().find(|p| p.recurrence_id.is_none()).unwrap();
    assert_eq!(master_now.exdates, [series.start]);
    assert!(parts.iter().any(|p| p.recurrence_id == Some(third)));
    let _ = std::fs::remove_dir_all(dir);
}

// spec: CALENV-002
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_moves_an_event_to_another_calendar_and_deletes_it() {
    let _env = Env::start();
    let forest = crate::dataflow::record(|| {
        let e = new_event("move", "Moving test");
        dav::put_event(&cal("work"), &acct("work"), std::slice::from_ref(&e), None, &[]).expect("create");
        let (dir, mut store) = store("move");
        load(&mut store);
        let original = store.find("work", &e.uid).expect("loaded");

        // moved: written to the new calendar first, then removed from the old one; a copy is kept (CAL-017)
        let mut moved = original.clone();
        moved.summary = "Moving test (moved)".into();
        let job = store.save_event("moves", moved, Some(("work".into(), original)), None).expect("job");
        let msg = finish(&mut store, &job, &["work", "moves"]).expect("move");
        assert!(msg.starts_with("Moved to"), "{msg}");
        assert!(find("work", &e.uid).is_empty(), "gone from the old calendar");
        let there = find_one("moves", &e.uid);
        assert_eq!(there.summary, "Moving test (moved)");
        assert!(store.find("moves", &e.uid).is_some() && store.find("work", &e.uid).is_none());
        let kept = std::fs::read_dir(store.trash_dir.join(crate::mail::store::today_string())).expect("the trash copy").count();
        assert!(kept >= 1);

        // delete through the store
        let job = store.delete_event("moves", store.find("moves", &e.uid).unwrap(), None).expect("job");
        let msg = finish(&mut store, &job, &["moves"]).expect("delete");
        assert!(msg.starts_with("Event deleted"), "{msg}");
        assert!(find("moves", &e.uid).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    });
    verify_flow(&forest, "flow.caldav_move", "caldav_move", "caldav-edit.md");
}

// spec: CALENV-002
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_a_change_on_the_server_meanwhile_is_not_overwritten() {
    let _env = Env::start();
    let (a, name) = (acct("work"), "work");
    let e = new_event("conflict", "Original");
    let (href, _) = dav::put_event(&cal(name), &a, std::slice::from_ref(&e), None, &[]).expect("create");
    let read = find_one(name, &e.uid);

    // somebody else changes the event after it was read
    let mut theirs = read.clone();
    theirs.summary = "Changed by somebody else".into();
    let (_, their_tag) = dav::put_event(&cal(name), &a, std::slice::from_ref(&theirs), Some((&href, read.etag.as_deref())), std::slice::from_ref(&read)).expect("their edit");
    assert_ne!(their_tag, read.etag);

    // our save with the old version tag is refused (HTTP 412) and the server keeps their version
    let mut mine = read.clone();
    mine.summary = "Changed by me".into();
    let err = dav::put_event(&cal(name), &a, std::slice::from_ref(&mine), Some((&href, read.etag.as_deref())), std::slice::from_ref(&read)).unwrap_err();
    assert!(err.contains("changed on the server meanwhile"), "{err}");
    assert_eq!(find_one(name, &e.uid).summary, "Changed by somebody else");
    // so is a delete with the old tag
    let err = dav::delete_event(&href, &a, read.etag.as_deref(), std::slice::from_ref(&read)).unwrap_err();
    assert!(err.contains("changed on the server meanwhile"), "{err}");
    assert_eq!(find_one(name, &e.uid).summary, "Changed by somebody else");

    // without any version tag (a server that sends none): the event is compared with what was read (CAL-016)
    let err = dav::put_event(&cal(name), &a, std::slice::from_ref(&mine), Some((&href, None)), std::slice::from_ref(&read)).unwrap_err();
    assert!(err.contains("changed on the server meanwhile"), "{err}");
    let err = dav::delete_event(&href, &a, None, std::slice::from_ref(&read)).unwrap_err();
    assert!(err.contains("changed on the server meanwhile"), "{err}");
    assert_eq!(find_one(name, &e.uid).summary, "Changed by somebody else");
    // …and goes through when the server still has what was read
    let current = find_one(name, &e.uid);
    let mut mine = current.clone();
    mine.summary = "Changed by me, knowing theirs".into();
    dav::put_event(&cal(name), &a, std::slice::from_ref(&mine), Some((&href, None)), std::slice::from_ref(&current)).expect("save without a tag");
    assert_eq!(find_one(name, &e.uid).summary, "Changed by me, knowing theirs");
    let current = find_one(name, &e.uid);
    dav::delete_event(&href, &a, None, std::slice::from_ref(&current)).expect("delete without a tag");
    assert!(find(name, &e.uid).is_empty());

    // an event somebody deleted meanwhile is not brought back by a save
    let (href, _) = dav::put_event(&cal(name), &a, std::slice::from_ref(&e), None, &[]).expect("create again");
    let read = find_one(name, &e.uid);
    dav::delete_event(&href, &a, read.etag.as_deref(), std::slice::from_ref(&read)).expect("their delete");
    let err = dav::put_event(&cal(name), &a, std::slice::from_ref(&read), Some((&href, read.etag.as_deref())), std::slice::from_ref(&read)).unwrap_err();
    assert!(!err.is_empty());
    assert!(find(name, &e.uid).is_empty());
}

// spec: CALENV-002
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_a_wrong_password_is_an_error_and_changes_nothing() {
    let _env = Env::start();
    let bad = acct_for(&cal("work"), "not-the-password");
    let refused = "the server refused the user name or password";
    assert_eq!(dav::discover(&acct_for(SERVER, "not-the-password")).unwrap_err(), refused);
    assert_eq!(dav::calendar_state(&cal("work"), &bad).unwrap_err(), refused);
    assert_eq!(dav::fetch_events(&cal("work"), &bad, 0, 4_000_000_000).unwrap_err(), refused);
    let e = new_event("badpw", "Must not appear");
    assert_eq!(dav::put_event(&cal("work"), &bad, std::slice::from_ref(&e), None, &[]).unwrap_err(), refused);
    assert!(find("work", &e.uid).is_empty());
    let seeded = all("work").into_iter().find(|x| x.uid == "seed-standup@shale.test").unwrap();
    assert!(dav::delete_event(seeded.href.as_deref().unwrap(), &bad, seeded.etag.as_deref(), std::slice::from_ref(&seeded)).is_err());
    assert!(find("work", "seed-standup@shale.test").len() == 1, "the seeded event is still there");
    // another user name with the right password is refused as well
    let other = Account { username: "somebody".into(), ..acct("work") };
    assert_eq!(dav::calendar_state(&cal("work"), &other).unwrap_err(), refused);
}

// spec: CALENV-002, SEC-001
#[test]
#[ignore = "needs the containers: tests/caldav-env/caldav-env.sh up"]
fn caldav_env_certificate_and_address_rules() {
    let _env = Env::start();
    let a = acct("work");
    // with the test CA trusted (as the other tests do) the server answers
    assert!(dav::calendar_state(&cal("work"), &a).is_ok());
    // a redirect on the same server is followed (and the answer says where it ended)
    let r = dav::request_with("GET", &format!("{SERVER}.well-known/caldav"), &[], "", &a).expect("redirect");
    assert!(r.url.starts_with(SERVER) && !r.url.contains("well-known"), "the redirect of /.well-known/caldav was followed: {}", r.url);

    // production trust: without the test CA the certificate is refused and nothing is sent
    *TEST_CA.lock().unwrap() = None;
    let err = dav::calendar_state(&cal("work"), &a).unwrap_err();
    assert!(err.to_lowercase().contains("certificate"), "{err}");
    let err = dav::discover(&acct_for(SERVER, PASSWORD)).unwrap_err();
    assert!(err.to_lowercase().contains("certificate"), "{err}");
    *TEST_CA.lock().unwrap() = Some(ca_path());

    // plain http to this computer is allowed by the address rules, but this server speaks TLS only: an error, not an answer
    let err = dav::request_with("GET", "http://localhost:15232/shale/work/", &[], "", &a).map(|r| r.status);
    assert!(!matches!(err, Ok(200..=299)), "{err:?}");
    // http to another server is refused before anything is sent
    let err = dav::request_with("GET", "http://dav.example.com/", &[], "", &a).unwrap_err();
    assert!(err.contains("https"), "{err}");
    // no user name or password inside an address
    assert!(dav::request_with("GET", &format!("https://{USER}:{PASSWORD}@localhost:15232/"), &[], "", &Account { username: String::new(), ..a.clone() }).is_err());
}
