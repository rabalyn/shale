//! Tests of the watched folders (MAIL-230 to MAIL-235) against the pretend IMAP server, which can
//! `IDLE` and tells an idling client what a test changes on it, as another mail program would.

use crate::config::{MailAccount, MailSecurity};
use crate::mail::imap_client::{check_read_only, Client, ConnectSettings, IdleEnd, ImapError, PlainAuth};
use crate::mail::imap_fake_tests::{message, server_cert, FakeServer};
use crate::mail::imap_sync::{sync_account, Connector, SyncGuard, SyncOptions};
use crate::mail::store::MailStore;
use crate::mail::watch::{Timing, Wanted, Watch};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::channel;
use std::sync::Arc;
use std::time::{Duration, Instant};

const NOW: i64 = 1_794_657_600;
const DAY: i64 = 86_400;

struct TestConnector {
    settings: ConnectSettings,
    user: String,
    password: String,
    /// The first this many connections fail (a server that is not there yet).
    fail_first: AtomicUsize,
    attempts: AtomicUsize,
    /// Connections that got through the sign-in (the server has seen all of its commands by then).
    signed_in: AtomicUsize,
    /// Every connection is refused as a wrong password.
    refuse_sign_in: bool,
}

impl Connector for TestConnector {
    fn connect(&self) -> Result<Client, ImapError> {
        self.attempts.fetch_add(1, Ordering::SeqCst);
        if self.refuse_sign_in {
            return Err(ImapError::Auth("wrong password".into()));
        }
        if self.fail_first.load(Ordering::SeqCst) > 0 {
            self.fail_first.fetch_sub(1, Ordering::SeqCst);
            return Err(ImapError::Connection("no route to the pretend host".into()));
        }
        let mut a = PlainAuth::new(&self.user, &self.password);
        let client = Client::connect_test(&self.settings, server_cert(), &mut a)?;
        self.signed_in.fetch_add(1, Ordering::SeqCst);
        Ok(client)
    }
}

fn connector(server: &FakeServer) -> Arc<TestConnector> {
    let s = server.state();
    Arc::new(TestConnector { settings: server.settings(), user: s.user.clone(), password: s.password.clone(), fail_first: AtomicUsize::new(0), attempts: AtomicUsize::new(0), signed_in: AtomicUsize::new(0), refuse_sign_in: false })
}

fn root(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("shale-watch-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    p
}

fn opts() -> SyncOptions {
    SyncOptions { now: Some(NOW), batch_size: 2, ..SyncOptions::default() }
}

/// A server with an Inbox of three mails, one of them flagged, and a folder "Work".
fn server() -> FakeServer {
    let server = FakeServer::start(MailSecurity::ImplicitTls);
    {
        let mut s = server.state();
        s.add_folder("INBOX", &[]);
        s.add_folder("Work", &[]);
        for i in 1..=3 {
            s.add_message("INBOX", message(&format!("inbox {i}"), "Bob <bob@x.de>", "hello"), NOW - DAY);
        }
        s.add_message("Work", message("work 1", "Bob <bob@x.de>", "plan"), NOW - DAY);
        s.set_flags("INBOX", 2, &["\\Flagged"]);
    }
    server
}

fn account() -> MailAccount {
    MailAccount { id: "acct1".into(), name: "Test account".into(), host: "localhost".into(), ..MailAccount::default() }
}

fn wanted(folder: &str) -> Wanted {
    Wanted { account: account(), folder: folder.into(), session_password: false }
}

/// A store with the first sync of the account done, like the one the window has when it starts watching.
fn synced(tag: &str, server: &FakeServer) -> (std::path::PathBuf, Arc<MailStore>) {
    let dir = root(tag);
    let store = Arc::new(MailStore::open(&dir).unwrap());
    let (tx, _rx) = channel();
    let summary = sync_account(&store, "acct1", "Test account", connector(server).as_ref(), &opts(), &tx, &AtomicBool::new(false));
    assert!(summary.errors.is_empty(), "{summary:?}");
    (dir, store)
}

fn fast() -> Timing {
    Timing { idle_max: Duration::from_secs(30), poll: Duration::from_millis(150), backoff_first: Duration::from_millis(40), backoff_max: Duration::from_millis(160), busy_retry: Duration::from_millis(40) }
}

fn flags_of(store: &MailStore, folder: &str, uid: u32) -> String {
    let r = store.reader().unwrap();
    let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == folder).unwrap();
    r.list(f.id, 100, 0).unwrap().into_iter().find(|m| m.uid == uid).map(|m| m.flags).unwrap_or_default()
}

fn uids_of(store: &MailStore, folder: &str) -> Vec<u32> {
    let r = store.reader().unwrap();
    let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == folder).unwrap();
    let mut v: Vec<u32> = r.list(f.id, 100, 0).unwrap().into_iter().map(|m| m.uid).collect();
    v.sort_unstable();
    v
}

/// Look at `ok` until it is true; false if it is not within `secs`.
fn wait_for(secs: u64, mut ok: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if ok() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    ok()
}

struct Rig {
    watch: Watch,
    store: Arc<MailStore>,
    conn: Arc<TestConnector>,
}

impl Rig {
    fn new(store: &Arc<MailStore>, conn: Arc<TestConnector>) -> Rig {
        let mut watch = Watch::default();
        watch.timing = fast();
        Rig { watch, store: store.clone(), conn }
    }

    fn watch(&mut self, folders: &[&str]) {
        let w: Vec<Wanted> = folders.iter().map(|f| wanted(f)).collect();
        let conn = self.conn.clone();
        self.watch.update(&w, &self.store, &move |_| conn.clone(), &opts(), None);
    }
}

fn changes_seen(w: &mut Watch) -> bool {
    w.drain()
}

// spec: MAIL-230
#[test]
fn idle_ends_when_the_folder_changes_when_the_time_is_up_or_when_asked_to_stop_and_the_connection_stays_good() {
    assert!(check_read_only("IDLE").is_ok(), "IDLE changes nothing, so the read-only gate lets it pass");
    let (dir, server) = (root("client"), server());
    let _ = std::fs::remove_dir_all(&dir);
    let c = connector(&server);
    let mut client = c.connect().unwrap();
    assert!(client.has_idle());
    client.examine("INBOX").unwrap();
    // another program unflags a mail while the client waits
    let other = {
        let state = server.state.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            state.lock().unwrap().set_flags("INBOX", 2, &[]);
        })
    };
    let t0 = Instant::now();
    assert_eq!(client.idle(Duration::from_secs(20), &|| false).unwrap(), IdleEnd::Changed);
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    other.join().unwrap();
    // the same for new and for deleted mail
    for change in 0..2 {
        let state = server.state.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            let mut s = state.lock().unwrap();
            if change == 0 {
                s.add_message("INBOX", message("fresh", "n@x.de", "new"), NOW);
            } else {
                s.delete_message("INBOX", 1);
            }
        });
        assert_eq!(client.idle(Duration::from_secs(20), &|| false).unwrap(), IdleEnd::Changed, "change {change}");
        t.join().unwrap();
    }
    // nothing happens: the time is up
    assert_eq!(client.idle(Duration::from_millis(400), &|| false).unwrap(), IdleEnd::Elapsed);
    // asked to stop
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        s2.store(true, Ordering::SeqCst);
    });
    assert_eq!(client.idle(Duration::from_secs(20), &|| stop.load(Ordering::SeqCst)).unwrap(), IdleEnd::Stopped);
    t.join().unwrap();
    // out of IDLE again: ordinary commands work, and only reading ones were sent
    assert_eq!(client.uid_search(crate::mail::imap_client::SearchKey::All).unwrap().len(), 3);
    let verbs = server.state().verbs();
    let done = server.state().commands.iter().filter(|c| *c == "DONE").count();
    assert!(verbs.iter().filter(|v| *v == "IDLE").count() >= 5 && done >= 5, "{verbs:?}");
    assert!(verbs.iter().all(|v| !v.starts_with("UID STORE") && !v.starts_with("SELECT")), "{verbs:?}");
}

// spec: MAIL-230
#[test]
fn a_server_that_refuses_idle_says_no_and_the_connection_goes_on() {
    let server = server();
    server.state().idle_refused = true;
    let mut client = connector(&server).connect().unwrap();
    client.examine("INBOX").unwrap();
    assert!(matches!(client.idle(Duration::from_secs(5), &|| false), Err(ImapError::Server(_))));
}

// spec: MAIL-231, MAIL-230
#[test]
fn an_unflag_in_another_client_reaches_the_index_within_seconds_through_idle_and_the_watcher_only_reads() {
    let (dir, server) = (root("idle"), server());
    let (_, store) = synced("idle2", &server);
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(flags_of(&store, "INBOX", 2), "\\Flagged");
    let mut rig = Rig::new(&store, connector(&server));
    rig.watch(&["INBOX"]);
    // it is connected and idles
    assert!(wait_for(10, || server.state().verbs().iter().any(|v| v == "IDLE")), "no IDLE");
    let t0 = Instant::now();
    // another client unflags the mail, reads another and flags a third
    {
        let mut s = server.state();
        s.set_flags("INBOX", 2, &[]);
        s.set_flags("INBOX", 3, &["\\Seen", "\\Flagged"]);
    }
    assert!(wait_for(10, || flags_of(&store, "INBOX", 2).is_empty() && flags_of(&store, "INBOX", 3).contains("\\Flagged")), "the change did not arrive: {:?}", flags_of(&store, "INBOX", 2));
    assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
    assert!(wait_for(5, || changes_seen(&mut rig.watch)), "the window is not told to read the list again");
    // new mail and deleted mail too
    {
        let mut s = server.state();
        s.add_message("INBOX", message("arrived", "n@x.de", "new"), NOW);
        s.delete_message("INBOX", 1);
    }
    assert!(wait_for(10, || uids_of(&store, "INBOX").contains(&4)), "the new mail did not arrive");
    assert!(wait_for(10, || {
        let r = store.reader().unwrap();
        let f = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
        r.list(f.id, 100, 0).unwrap().iter().any(|m| m.uid == 1 && m.gone_on_server)
    }));
    // read-only: examined, never selected, nothing stored
    let verbs = server.state().verbs();
    assert!(verbs.iter().any(|v| v.starts_with("EXAMINE")), "{verbs:?}");
    assert!(verbs.iter().all(|v| !v.starts_with("SELECT") && !v.contains(" STORE") && !v.starts_with("UID STORE") && !v.starts_with("APPEND") && !v.contains("EXPUNGE")), "{verbs:?}");
    // and no polling happens while the server pushes: one check after connecting, one per change
    drop(rig);
    let _ = std::fs::remove_dir_all(root("idle2"));
}

// spec: MAIL-231
#[test]
fn without_idle_the_watched_folder_is_asked_again_and_again() {
    let server = server();
    server.state().idle = false;
    let (_, store) = synced("poll", &server);
    let mut rig = Rig::new(&store, connector(&server));
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || server.state().verbs().iter().filter(|v| v.starts_with("UID SEARCH")).count() >= 3), "no polling");
    server.state().set_flags("INBOX", 2, &[]);
    assert!(wait_for(10, || flags_of(&store, "INBOX", 2).is_empty()), "the change did not arrive by polling");
    assert!(server.state().verbs().iter().all(|v| v != "IDLE"), "it never asks for IDLE of a server without it");
    // a server that has IDLE but says no for this user is asked in the same way
    server.state().idle = true;
    server.state().idle_refused = true;
    rig.watch(&[]);
    rig.watch(&["INBOX"]);
    server.state().set_flags("INBOX", 3, &["\\Flagged"]);
    assert!(wait_for(10, || flags_of(&store, "INBOX", 3).contains("\\Flagged")), "no fallback after a refused IDLE");
    drop(rig);
    let _ = std::fs::remove_dir_all(root("poll"));
}

// spec: MAIL-233
#[test]
fn switching_the_folder_moves_the_watching_and_leaving_ends_every_connection_with_a_logout() {
    let server = server();
    let (_, store) = synced("switch", &server);
    let mut rig = Rig::new(&store, connector(&server));
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || server.state().open_connections == 1 && server.state().verbs().iter().any(|v| v == "IDLE")));
    // the person opens Work: the Inbox stays watched (a second connection), Work is added
    rig.watch(&["INBOX", "Work"]);
    assert!(wait_for(10, || server.state().open_connections == 2));
    assert_eq!(rig.watch.watching(), vec![("acct1".to_string(), "INBOX".to_string()), ("acct1".to_string(), "Work".to_string())]);
    server.state().set_flags("Work", 1, &["\\Flagged"]);
    assert!(wait_for(10, || flags_of(&store, "Work", 1).contains("\\Flagged")), "a change in the other folder arrives");
    // Work is closed again: that connection ends, the Inbox one stays
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || server.state().open_connections == 1), "the connection of the closed folder is still open");
    server.state().set_flags("INBOX", 3, &["\\Flagged"]);
    assert!(wait_for(10, || flags_of(&store, "INBOX", 3).contains("\\Flagged")));
    // a change of the account's settings starts the connection again
    let before = server.state().connections;
    let changed = MailAccount { name: "Renamed".into(), ..account() };
    let conn = rig.conn.clone();
    rig.watch.update(&[Wanted { account: changed, folder: "INBOX".into(), session_password: false }], &rig.store, &move |_| conn.clone(), &opts(), None);
    assert!(wait_for(10, || server.state().connections > before));
    // leaving the mail view: everything ends, with a LOGOUT
    rig.watch.stop_all();
    assert!(wait_for(10, || server.state().open_connections == 0), "connections are still open");
    assert!(server.state().verbs().iter().any(|v| v == "LOGOUT"));
    assert!(rig.watch.watching().is_empty());
    // removing the account does the same
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || server.state().open_connections == 1));
    rig.watch.stop_account("acct1");
    assert!(wait_for(10, || server.state().open_connections == 0));
    drop(rig);
    let _ = std::fs::remove_dir_all(root("switch"));
}

// spec: MAIL-233
#[test]
fn dropping_the_watch_ends_the_connections_too() {
    let server = server();
    let (_, store) = synced("drop", &server);
    let mut rig = Rig::new(&store, connector(&server));
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || server.state().open_connections == 1));
    drop(rig);
    assert!(wait_for(10, || server.state().open_connections == 0));
    let _ = std::fs::remove_dir_all(root("drop"));
}

// spec: MAIL-232
#[test]
fn a_quick_check_never_runs_beside_a_full_sync_of_the_same_account_and_catches_up_afterwards() {
    let server = server();
    let (_, store) = synced("guard", &server);
    // a full sync of the account is going on
    let guard = SyncGuard::try_enter(&store, "acct1").expect("nobody else is syncing");
    assert!(SyncGuard::try_enter(&store, "acct1").is_none(), "two at once for one account");
    assert!(SyncGuard::try_enter(&store, "another").is_some(), "other accounts are independent");
    let conn = connector(&server);
    let mut rig = Rig::new(&store, conn.clone());
    rig.watch(&["INBOX"]);
    // count from the end of the watcher's sign-in: its TLS handshake and LOGIN can come late under
    // load, and the server counts a connection when it accepts it (raba/shale#260)
    assert!(wait_for(10, || conn.signed_in.load(Ordering::SeqCst) > 0));
    std::thread::sleep(Duration::from_millis(300));
    let sent = server.state().commands.len();
    server.state().set_flags("INBOX", 2, &[]);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(flags_of(&store, "INBOX", 2), "\\Flagged", "the quick check must wait for the full sync");
    assert_eq!(server.state().commands.len(), sent, "the watcher sent commands while a full sync held the account");
    drop(guard);
    assert!(wait_for(10, || flags_of(&store, "INBOX", 2).is_empty()), "the quick check did not catch up");
    // and the other way round: a full sync waits for a quick check that is running, and can be cancelled while it waits
    // the quick check stores the flags before it lets go of the account, so wait for that
    // (raba/shale#252: taking it at once failed under load)
    let mut held = None;
    assert!(wait_for(10, || {
        held = SyncGuard::try_enter(&store, "acct1");
        held.is_some()
    }));
    let held = held.expect("the quick check let go of the account");
    let cancel = Arc::new(AtomicBool::new(false));
    let (c2, s2, conn) = (cancel.clone(), store.clone(), connector(&server));
    let sync = std::thread::spawn(move || {
        let (tx, _rx) = channel();
        sync_account(&s2, "acct1", "Test account", conn.as_ref(), &opts(), &tx, &c2)
    });
    std::thread::sleep(Duration::from_millis(300));
    assert!(!sync.is_finished(), "the full sync went on beside the other one");
    cancel.store(true, Ordering::SeqCst);
    assert!(sync.join().unwrap().cancelled);
    drop(held);
    drop(rig);
    let _ = std::fs::remove_dir_all(root("guard"));
}

// spec: MAIL-234
#[test]
fn a_broken_connection_is_opened_again_after_a_growing_pause_without_telling_anyone_and_a_refused_sign_in_is_not_tried_again() {
    let server = server();
    let (_, store) = synced("backoff", &server);
    let flaky = {
        let c = connector(&server);
        c.fail_first.store(3, Ordering::SeqCst);
        c
    };
    let mut rig = Rig::new(&store, flaky.clone());
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || server.state().verbs().iter().any(|v| v == "IDLE")), "it never got through");
    assert_eq!(flaky.attempts.load(Ordering::SeqCst), 4, "three failures, then a connection");
    rig.watch.drain();
    server.state().set_flags("INBOX", 2, &[]);
    assert!(wait_for(10, || flags_of(&store, "INBOX", 2).is_empty()), "after the failures it works");
    assert!(
        wait_for(5, || {
            rig.watch.drain();
            rig.watch.errors.is_empty()
        }),
        "the error goes away with the first good check"
    );
    drop(rig);
    // a wrong password: one try, then it rests
    let refused = Arc::new(TestConnector { refuse_sign_in: true, ..Arc::try_unwrap(connector(&server)).ok().unwrap() });
    let mut rig = Rig::new(&store, refused.clone());
    rig.watch(&["INBOX"]);
    assert!(wait_for(10, || {
        rig.watch.drain();
        !rig.watch.errors.is_empty()
    }));
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(refused.attempts.load(Ordering::SeqCst), 1, "a wrong password was tried again");
    drop(rig);
    let _ = std::fs::remove_dir_all(root("backoff"));
}

// spec: MAIL-108
#[test]
fn a_change_on_the_server_to_a_mail_with_a_waiting_action_is_not_lost_when_the_action_is_dropped() {
    use crate::mail::imap_client::MailFlag;
    use crate::mail::ops::{enqueue, Action};
    let server = server();
    let (_, store) = synced("held", &server);
    let r = store.reader().unwrap();
    let inbox = r.folders(Some("acct1")).unwrap().into_iter().find(|f| f.name == "INBOX").unwrap();
    let msg = r.list(inbox.id, 100, 0).unwrap().into_iter().find(|m| m.uid == 3).unwrap();
    // the person flags mail 3 (the action waits: the server is not reached), another client reads it
    enqueue(&store, &[msg], &Action::SetFlags { add: vec![MailFlag::Flagged], remove: vec![] }, NOW).unwrap();
    server.state().set_flags("INBOX", 3, &["\\Seen"]);
    let (tx, _rx) = channel();
    let c = connector(&server);
    let sync = || sync_account(&store, "acct1", "Test account", c.as_ref(), &opts(), &tx, &AtomicBool::new(false));
    assert!(sync().errors.is_empty());
    assert!(flags_of(&store, "INBOX", 3).contains("\\Flagged"), "the person's change shows while it waits");
    // the action fails for good: the local copy goes back, and the next sync must see the other client's change
    let pending = r.pending_ops(Some("acct1")).unwrap();
    store.commit(pending.iter().map(|p| crate::mail::db::Op::PendingRollback { id: p.id }).collect()).unwrap();
    assert!(sync().errors.is_empty());
    assert!(flags_of(&store, "INBOX", 3).contains("\\Seen"), "the other client's change was lost: {:?}", flags_of(&store, "INBOX", 3));
    let _ = std::fs::remove_dir_all(root("held"));
}
