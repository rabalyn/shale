//! The rescan on a worker thread (NET-013): the frame does not wait for it, one scan runs at a
//! time, and the result of a scan that has gone stale changes nothing.

use super::rescan::{scan, DrawingProbe, Outcome};
use super::*;
use crate::vault::ScanInput;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

fn app_in(tag: &str, files: &[(&str, &[u8])]) -> (ShaleApp, PathBuf) {
    let d = std::env::temp_dir().join(format!("shale-rescan-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for (name, bytes) in files {
        crate::fsutil::write_atomic(&d.join(name), bytes).unwrap();
    }
    let app = ShaleApp::build(Options { screenshot: None, vault: Some(d.clone()), view: None, keys: None }, false);
    (app, d)
}

/// Control over the scan of one test: it waits for `start`, scans, says so on `scanned`, and
/// hands its result over when `done` is sent. `calls` counts the scans begun.
struct Gates {
    start: Sender<()>,
    scanned: Receiver<()>,
    done: Sender<()>,
    calls: Arc<AtomicUsize>,
}

impl Gates {
    fn new(app: &mut ShaleApp) -> Gates {
        let (start, start_rx) = channel::<()>();
        let (scanned_tx, scanned) = channel::<()>();
        let (done, done_rx) = channel::<()>();
        let calls = Arc::new(AtomicUsize::new(0));
        let (start_rx, scanned_tx, done_rx, n) = (Mutex::new(start_rx), Mutex::new(scanned_tx), Mutex::new(done_rx), calls.clone());
        app.rescan.with_runner(move |input: ScanInput, probes: Vec<DrawingProbe>| {
            n.fetch_add(1, Ordering::SeqCst);
            start_rx.lock().unwrap().recv_timeout(Duration::from_secs(10)).expect("the test lets the scan start");
            let out: Outcome = scan(input, probes);
            let _ = scanned_tx.lock().unwrap().send(());
            let _ = done_rx.lock().unwrap().recv_timeout(Duration::from_secs(10));
            out
        });
        Gates { start, scanned, done, calls }
    }

    /// Let the scan run and wait until it has read the files.
    fn scan_now(&self) {
        self.start.send(()).unwrap();
        self.scanned.recv_timeout(Duration::from_secs(10)).expect("the scan ends");
    }

    fn deliver(&self) {
        self.done.send(()).unwrap();
    }
}

fn frame(app: &mut ShaleApp, ctx: &egui::Context) {
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| app.run_frame(ui));
    out.textures_delta.clear();
}

/// Run frames until `ready` holds.
fn frames_until(app: &mut ShaleApp, what: &str, ready: impl Fn(&ShaleApp) -> bool) {
    let ctx = egui::Context::default();
    let start = Instant::now();
    while !ready(app) {
        frame(app, &ctx);
        assert!(start.elapsed() < Duration::from_secs(20), "{what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn type_text(app: &mut ShaleApp, rel: &str, text: &str) {
    let doc = app.notes.get_mut(Path::new(rel)).unwrap();
    doc.text = text.to_string();
    doc.dirty = true;
    doc.last_edit = Instant::now() - Duration::from_secs(5);
    app.vault.as_mut().unwrap().set_note_text(Path::new(rel), text);
}

fn write(d: &Path, name: &str, text: &str) {
    crate::fsutil::write_atomic(&d.join(name), text.as_bytes()).unwrap();
}

// spec: NET-013
#[test]
fn net_013_the_frame_does_not_wait_for_the_scan_and_the_change_arrives_when_it_is_done() {
    let (mut app, d) = app_in("frame", &[("N.md", b"one\n")]);
    app.open(Path::new("N.md"), false);
    let gates = Gates::new(&mut app);
    write(&d, "N.md", "two\n");
    app.rescan.due_now();
    let ctx = egui::Context::default();
    let began = Instant::now();
    frame(&mut app, &ctx);
    assert!(began.elapsed() < Duration::from_secs(5), "the frame waited for the scan");
    assert!(app.rescan.in_flight(), "the scan runs in the background");
    assert_eq!(app.notes[Path::new("N.md")].text, "one\n", "nothing is applied before the scan is done");
    gates.scan_now();
    gates.deliver();
    frames_until(&mut app, "the change did not arrive", |a| a.notes[Path::new("N.md")].text == "two\n");
    assert!(!app.notes[Path::new("N.md")].dirty);
    assert!(!app.rescan.in_flight());
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013, DATA-002
#[test]
fn net_013_an_outside_change_is_reloaded_by_the_frames_alone() {
    let (mut app, d) = app_in("settle", &[("N.md", b"one\n")]);
    app.open(Path::new("N.md"), false);
    write(&d, "N.md", "two\n");
    app.rescan.due_now();
    frames_until(&mut app, "the change did not arrive", |a| a.notes[Path::new("N.md")].text == "two\n");
    write(&d, "New.md", "fresh\n");
    app.rescan.due_now();
    frames_until(&mut app, "the new note did not arrive", |a| a.vault.as_ref().unwrap().notes.contains_key(Path::new("New.md")));
    let _ = std::fs::remove_dir_all(&d);
}

// spec: VAULT-005, NET-013
#[test]
fn vault_005_a_scan_starts_when_the_3_seconds_have_passed_and_not_before() {
    assert_eq!(crate::vault::RESCAN_EVERY, Duration::from_secs(3));
    let (mut app, d) = app_in("three-seconds", &[("N.md", b"one\n")]);
    let gates = Gates::new(&mut app);
    let ctx = egui::Context::default();
    // a scan has just begun: the frames that follow start none
    app.rescan.postpone();
    for _ in 0..3 {
        frame(&mut app, &ctx);
    }
    assert!(!app.rescan.in_flight(), "no scan before the interval is over");
    // the interval is over: the next frame starts the scan
    app.rescan.due_now();
    frame(&mut app, &ctx);
    assert!(app.rescan.in_flight(), "a scan after the interval");
    gates.scan_now();
    assert_eq!(gates.calls.load(Ordering::SeqCst), 1);
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    // and then it waits again
    app.rescan.postpone();
    frame(&mut app, &ctx);
    assert!(!app.rescan.in_flight());
    assert_eq!(gates.calls.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013
#[test]
fn net_013_only_one_scan_runs_at_a_time() {
    let (mut app, d) = app_in("single", &[("N.md", b"one\n")]);
    let gates = Gates::new(&mut app);
    let ctx = egui::Context::default();
    // the first scan has begun (its worker thread may be slow to get going)
    let began = Instant::now();
    while gates.calls.load(Ordering::SeqCst) == 0 {
        app.rescan.due_now();
        frame(&mut app, &ctx);
        assert!(began.elapsed() < Duration::from_secs(60), "the first scan did not begin");
        std::thread::sleep(Duration::from_millis(5));
    }
    // while it runs, more frames with a scan due must not begin another
    for _ in 0..10 {
        app.rescan.due_now();
        frame(&mut app, &ctx);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(gates.calls.load(Ordering::SeqCst), 1, "a second scan began while the first was running");
    app.rescan.postpone();
    gates.scan_now();
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    assert_eq!(gates.calls.load(Ordering::SeqCst), 1);
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013, DATA-002
#[test]
fn net_013_a_result_does_not_overwrite_an_unsaved_edit() {
    let (mut app, d) = app_in("unsaved", &[("N.md", b"one\n")]);
    app.open(Path::new("N.md"), false);
    let gates = Gates::new(&mut app);
    write(&d, "N.md", "two\n");
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    gates.scan_now();
    // typed while the scan was running, not saved yet (the frames below must not autosave it)
    type_text(&mut app, "N.md", "mine\n");
    app.notes.get_mut(Path::new("N.md")).unwrap().last_edit = Instant::now() + Duration::from_secs(60);
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    let doc = &app.notes[Path::new("N.md")];
    assert_eq!((doc.text.as_str(), doc.dirty), ("mine\n", true), "the unsaved edit stays");
    // and it is saved as a conflict copy, not over the outside change (DATA-003)
    app.notes.get_mut(Path::new("N.md")).unwrap().last_edit = Instant::now() - Duration::from_secs(5);
    app.autosave();
    assert_eq!(std::fs::read_to_string(d.join("N.md")).unwrap(), "two\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013
#[test]
fn net_013_a_result_does_not_overwrite_a_note_saved_while_the_scan_ran() {
    let (mut app, d) = app_in("saved", &[("N.md", b"one\n")]);
    app.open(Path::new("N.md"), false);
    let gates = Gates::new(&mut app);
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    // saved while the scan has not looked yet, and again after it read that save
    type_text(&mut app, "N.md", "two\n");
    app.autosave();
    gates.scan_now();
    type_text(&mut app, "N.md", "three\n");
    app.autosave();
    assert!(!app.notes[Path::new("N.md")].dirty);
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    assert_eq!(crate::note_props::unstamped(&app.notes[Path::new("N.md")].text), "three\n", "the older read does not replace the newer save");
    assert_eq!(crate::note_props::unstamped(&app.vault.as_ref().unwrap().notes[Path::new("N.md")].text), "three\n");
    assert_eq!(crate::note_props::unstamped(&std::fs::read_to_string(d.join("N.md")).unwrap()), "three\n");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013
#[test]
fn net_013_a_result_does_not_overwrite_a_drawing_saved_while_the_scan_ran() {
    let scene = |n: usize| {
        let mut s = crate::drawing::Scene::parse(crate::drawing::SAMPLE_SCENE).unwrap();
        s.elements.truncate(n);
        s.to_json()
    };
    let (mut app, d) = app_in("drawing", &[("D.excalidraw", scene(3).as_bytes())]);
    let rel = PathBuf::from("D.excalidraw");
    app.open(&rel, false);
    let gates = Gates::new(&mut app);
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    gates.scan_now();
    // the drawing is edited and saved after the scan read the file
    let doc = app.drawings.get_mut(&rel).unwrap();
    doc.scene.elements.truncate(1);
    doc.dirty = true;
    app.save_drawing(&rel);
    assert!(!app.drawings[&rel].dirty);
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    assert_eq!(app.drawings[&rel].scene.elements.len(), 1, "the older read does not replace the saved drawing");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013
#[test]
fn net_013_a_result_for_another_vault_is_dropped_and_the_new_vault_is_scanned() {
    let (mut app, d) = app_in("switch", &[("Old.md", b"old\n")]);
    let other = std::env::temp_dir().join(format!("shale-rescan-switch2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&other);
    std::fs::create_dir_all(&other).unwrap();
    write(&other, "New.md", "new\n");
    let gates = Gates::new(&mut app);
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    gates.scan_now();
    app.vault = Some(Vault::open(&other).unwrap());
    gates.deliver();
    frames_until(&mut app, "the old scan was not dropped", |a| !a.rescan.in_flight());
    let v = app.vault.as_ref().unwrap();
    assert!(v.notes.contains_key(Path::new("New.md")) && !v.notes.contains_key(Path::new("Old.md")), "the new vault is untouched");
    // closed: nothing to apply to
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    app.vault = None;
    gates.scan_now();
    gates.deliver();
    frames_until(&mut app, "the scan of the closed vault did not end", |a| !a.rescan.in_flight());
    assert!(app.vault.is_none());
    let _ = std::fs::remove_dir_all(&d);
    let _ = std::fs::remove_dir_all(&other);
}

// spec: NET-013
#[test]
fn net_013_a_result_from_before_a_rename_is_dropped_and_the_next_scan_shows_the_new_state() {
    let (mut app, d) = app_in("rename", &[("A.md", b"a\n")]);
    let gates = Gates::new(&mut app);
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    gates.scan_now();
    app.rename(Path::new("A.md"), "B").unwrap();
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    let v = app.vault.as_ref().unwrap();
    assert!(v.notes.contains_key(Path::new("B.md")) && !v.notes.contains_key(Path::new("A.md")), "the old file list did not come back");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013, JOB-022
#[test]
fn net_013_a_scan_is_a_task_that_is_listed_only_when_it_takes_two_seconds() {
    use super::rescan::VAULT_RESCAN;
    let (mut app, d) = app_in("listed", &[("N.md", b"one\n")]);
    let gates = Gates::new(&mut app);
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    assert!(app.rescan.in_flight());
    let info = app.busy.info(&VAULT_RESCAN).expect("the scan is a task from its start");
    assert_eq!((info.label.as_str(), info.cancellable), ("Reading the vault folder…", false));
    // the clock is given: the scan is held by the gate, nothing waits for two seconds
    assert!(app.busy.list_at(info.started).is_empty(), "a scan that has just begun is nothing to show");
    assert!(app.busy.list_at(info.started + Duration::from_millis(1999)).is_empty());
    let after = app.busy.list_at(info.started + Duration::from_secs(2));
    assert_eq!(after.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Reading the vault folder…"], "listed once it takes two seconds");
    gates.scan_now();
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    assert!(app.busy.info(&VAULT_RESCAN).is_none() && app.busy.list_at(info.started + Duration::from_secs(60)).is_empty(), "gone with the scan");
    let _ = std::fs::remove_dir_all(&d);
}

// spec: NET-013, JOB-022, VAULT-037
#[test]
fn net_013_the_task_of_a_scan_ends_when_the_vault_is_dropped_and_the_new_vault_can_be_scanned() {
    use super::rescan::VAULT_RESCAN;
    let (mut app, d) = app_in("task-switch", &[("N.md", b"one\n")]);
    let gates = Gates::new(&mut app);
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    assert!(app.busy.is_running(&VAULT_RESCAN));
    app.drop_vault_state();
    assert!(!app.busy.is_running(&VAULT_RESCAN), "the old vault's scan is no task of the window any more");
    // the new vault's scan begins though the old worker still holds on
    app.rescan.due_now();
    frame(&mut app, &egui::Context::default());
    assert!(app.busy.is_running(&VAULT_RESCAN) && app.rescan.in_flight());
    gates.scan_now();
    gates.scan_now();
    gates.deliver();
    gates.deliver();
    frames_until(&mut app, "the scan did not end", |a| !a.rescan.in_flight());
    let _ = std::fs::remove_dir_all(&d);
}
