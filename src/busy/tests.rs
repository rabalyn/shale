use super::ui;
use super::*;
use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::sync::mpsc::channel;
use std::time::Duration;

const SYNC: Key = Key::new("test-sync");
const OTHER: Key = Key::new("test-other");
const MAIL_INDEX: Key = Key::new("test-mail-index");

const WAIT: Duration = Duration::from_secs(10);

/// Wait for a job's result.
fn result<T>(job: &Job<T>) -> Result<T, String> {
    let start = Instant::now();
    loop {
        match job.try_recv() {
            Ok(r) => return r,
            Err(TryRecvError::Empty) => assert!(start.elapsed() < WAIT, "the job ends"),
            Err(TryRecvError::Disconnected) => panic!("the job vanished"),
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// A job that runs until the returned sender is used or dropped.
fn gated(busy: &Busy, key: Key, label: &str) -> (Job<u32>, std::sync::mpsc::Sender<()>) {
    let (tx, rx) = channel::<()>();
    let job = busy
        .spawn(
            key,
            label,
            move || {
                let _ = rx.recv_timeout(WAIT);
                7
            },
            || {},
        )
        .expect("not running yet");
    (job, tx)
}

// spec: JOB-003
#[test]
fn job_003_a_task_is_listed_while_its_job_runs_and_is_gone_before_the_result_can_be_taken() {
    let busy = Busy::default();
    assert!(busy.list().is_empty() && !busy.is_running(&SYNC));
    let (job, release) = gated(&busy, SYNC, "Syncing with git…");
    let info = busy.info(&SYNC).expect("listed while it runs");
    assert_eq!((info.label.as_str(), info.cancellable, info.cancelling), ("Syncing with git…", false, false));
    assert!(busy.is_running(&SYNC) && !busy.is_running(&OTHER));
    assert_eq!(busy.list().len(), 1);
    assert!(matches!(job.try_recv(), Err(TryRecvError::Empty)));
    release.send(()).unwrap();
    // the moment the result can be taken, the action is idle again (a view that reacts to the result finds no stale task)
    assert_eq!(result(&job), Ok(7));
    assert!(busy.list().is_empty() && busy.info(&SYNC).is_none());
}

// spec: JOB-003
#[test]
fn job_003_a_panicking_job_ends_its_task_and_delivers_the_error() {
    let busy = Busy::default();
    let job = busy.spawn(SYNC, "Syncing with git…", || -> u32 { panic!("boom {}", 3) }, || {}).unwrap();
    let e = result(&job).unwrap_err();
    assert!(e.contains("boom 3"), "{e}");
    assert!(busy.list().is_empty(), "no task stays listed after a panic");
    assert!(busy.begin(SYNC, "again").is_some(), "and the action can start again");
}

// spec: JOB-003
#[test]
fn job_003_a_task_that_a_thread_of_its_own_holds_is_listed_until_it_is_dropped_and_notify_comes_after_the_result() {
    let busy = Busy::default();
    let task = busy.begin(SYNC, "Working…").unwrap();
    assert_eq!(busy.list().iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Working…"]);
    drop(task);
    assert!(busy.list().is_empty());
    let (tx, rx) = channel();
    let b2 = busy.clone();
    let job = busy.spawn(SYNC, "Working…", || 1, move || tx.send(b2.is_running(&SYNC)).unwrap()).unwrap();
    assert_eq!(rx.recv_timeout(WAIT), Ok(false), "the window is told when the action is already idle");
    assert_eq!(result(&job), Ok(1));
}

// spec: JOB-005
#[test]
fn job_005_the_same_action_does_not_start_twice_and_another_one_does() {
    let busy = Busy::default();
    let (job, release) = gated(&busy, SYNC, "Syncing with git…");
    assert!(busy.begin(SYNC, "Syncing with git…").is_none(), "a second task with the same key is refused");
    assert!(busy.spawn(SYNC, "x", || 1, || {}).is_none() && busy.spawn_cancellable(SYNC, "x", |_| 1, || {}).is_none());
    assert_eq!(busy.list().len(), 1, "the refused ones left nothing behind");
    let other = busy.begin(OTHER, "Exporting PDF…").expect("another action runs beside it");
    assert_eq!(busy.list().iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Syncing with git…", "Exporting PDF…"], "the oldest first");
    drop(other);
    release.send(()).unwrap();
    result(&job).unwrap();
    assert!(busy.begin(SYNC, "Syncing with git…").is_some(), "idle again: it starts");
}

/// One frame of a widget with a click on it; whether the click arrived.
struct Clicker {
    ctx: egui::Context,
    t: f64,
}

impl Clicker {
    fn new() -> Self {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        Clicker { ctx, t: 0.0 }
    }

    fn frame(&mut self, events: Vec<Event>, add: &mut dyn FnMut(&mut egui::Ui) -> Option<egui::Response>) -> Option<egui::Response> {
        self.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))), time: Some(self.t), events, ..Default::default() };
        let mut got = None;
        let mut out = self.ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| got = add(ui));
        });
        out.textures_delta.clear();
        got
    }

    /// Click the widget `add` draws; true if it reported a click.
    fn click(&mut self, add: &mut dyn FnMut(&mut egui::Ui) -> Option<egui::Response>) -> bool {
        let at = self.frame(vec![], add).unwrap().rect.center();
        let b = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Default::default() };
        self.frame(vec![Event::PointerMoved(at)], add);
        self.frame(vec![b(true)], add);
        self.frame(vec![b(false)], add).is_some_and(|r| r.clicked())
    }
}

// spec: JOB-005
#[test]
fn job_005_a_button_is_off_while_its_action_runs_and_says_why() {
    let busy = Busy::default();
    let mut ui_ = Clicker::new();
    let mut button = |ui: &mut egui::Ui| Some(ui::button(ui, &busy, &[&SYNC, &OTHER], egui::Button::new("Sync now")));
    assert!(ui_.click(&mut button), "idle: a click arrives");
    let task = busy.begin(OTHER, "Syncing with git…").unwrap();
    assert!(!ui_.click(&mut button), "while one of its actions runs, a click does nothing");
    let r = ui_.frame(vec![], &mut button).unwrap();
    assert!(!r.enabled(), "and the button looks off");
    drop(task);
    assert!(ui_.click(&mut button), "it is back when the action has ended");
}

// spec: JOB-004
#[test]
fn job_004_the_status_of_a_running_action_shows_its_text_and_cancel_only_where_it_can_stop() {
    let busy = Busy::default();
    let mut c = Clicker::new();
    let painted = |c: &mut Clicker, busy: &Busy, key: &Key| {
        c.t += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 300.0))), time: Some(c.t), ..Default::default() };
        let mut running = false;
        let mut out = c.ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| running = ui::status(ui, busy, key));
        });
        out.textures_delta.clear();
        let mut texts = vec![];
        for s in &out.shapes {
            collect(&s.shape, &mut texts);
        }
        (running, texts)
    };
    let (running, texts) = painted(&mut c, &busy, &SYNC);
    assert!(!running && texts.is_empty(), "nothing runs: nothing is shown");
    let _plain = busy.begin(SYNC, "Syncing with git…").unwrap();
    let (running, texts) = painted(&mut c, &busy, &SYNC);
    assert!(running && texts == ["Syncing with git…"], "{texts:?}");
    let mut stoppable = busy.begin(OTHER, "Checking file names…").unwrap();
    stoppable.cancellable();
    let (running, texts) = painted(&mut c, &busy, &OTHER);
    assert!(running && texts == ["Checking file names…", "Cancel"], "{texts:?}");
}

fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.text().to_string()),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

// spec: JOB-006
#[test]
fn job_006_cancel_reaches_the_job_through_the_registry_and_the_task_stays_until_the_job_has_ended() {
    let busy = Busy::default();
    let (seen_tx, seen_rx) = channel::<()>();
    let job = busy
        .spawn_cancellable(
            SYNC,
            "Checking…",
            move |cancel| {
                let mut steps = 0;
                seen_tx.send(()).unwrap();
                while !cancel.is_cancelled() && steps < 10_000 {
                    steps += 1;
                    std::thread::sleep(Duration::from_millis(1));
                }
                (steps < 10_000, cancel.is_cancelled())
            },
            || {},
        )
        .unwrap();
    seen_rx.recv_timeout(WAIT).unwrap();
    let info = busy.info(&SYNC).unwrap();
    assert!(info.cancellable && !info.cancelling);
    assert!(busy.cancel(info.id), "a task that can stop says so");
    let after = busy.info(&SYNC);
    assert!(after.is_none() || after.as_ref().is_some_and(|t| t.cancelling), "it shows as stopping until the job ends: {after:?}");
    assert_eq!(result(&job), Ok((true, true)), "the job saw the request and ended early");
    assert!(busy.list().is_empty());
}

// spec: JOB-006
#[test]
fn job_006_a_task_that_cannot_stop_has_no_cancel_and_the_job_handle_can_cancel_one_that_can() {
    let busy = Busy::default();
    let (plain, release) = gated(&busy, SYNC, "Syncing with git…");
    let id = busy.info(&SYNC).unwrap().id;
    assert!(!busy.cancel(id), "nothing to cancel");
    plain.cancel();
    assert!(matches!(plain.try_recv(), Err(TryRecvError::Empty)), "cancel() on it changes nothing");
    release.send(()).unwrap();
    result(&plain).unwrap();
    assert!(!busy.cancel(id), "an ended task is not found");
    let job = busy
        .spawn_cancellable(
            OTHER,
            "Exporting PDF…",
            |c| loop {
                if c.is_cancelled() {
                    return "stopped";
                }
                std::thread::sleep(Duration::from_millis(1));
            },
            || {},
        )
        .unwrap();
    job.cancel();
    assert_eq!(result(&job), Ok("stopped"));
}

// spec: JOB-008
#[test]
fn job_008_work_started_before_its_view_exists_is_shown_by_the_view_through_the_key() {
    // the app starts the check of the mail index (#385) with a clone of the registry, long before a view asks
    let busy = Busy::default();
    let (job, release) = gated(&busy, MAIL_INDEX, "Checking the mail index…");
    // the view that opens later knows only the key
    let view = |busy: &Busy| busy.info(&MAIL_INDEX).map(|t| t.label);
    assert_eq!(view(&busy.clone()).as_deref(), Some("Checking the mail index…"));
    release.send(()).unwrap();
    result(&job).unwrap();
    assert_eq!(view(&busy), None, "the view finds the index ready");
}

// spec: JOB-015
#[test]
fn job_015_the_mail_view_shows_the_check_of_the_index_with_a_spinner_while_its_task_runs() {
    use crate::mail::ui::{StoreState, MAIL_INDEX as CHECK};
    let (mut app, mut h, dir) = window("mail-index");
    app.show_view(crate::app::View::Mail, false);
    app.mail.store = StoreState::Opening;
    let task = app.busy.begin(CHECK, "Checking the mail index…").expect("the registry of the window is the mail view's");
    h.frames(&mut app, 3);
    assert!(painted_at(&h, "Checking the mail index…").is_some(), "the view says what runs");
    assert!(painted_at(&h, "Opening the local mail copy…").is_none());
    drop(task);
    h.frames(&mut app, 3);
    assert!(painted_at(&h, "Checking the mail index…").is_none());
    assert!(painted_at(&h, "Opening the local mail copy…").is_some(), "no task: the plain line");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: JOB-015
#[test]
fn job_015_the_store_opens_at_start_when_there_is_an_account_though_the_mail_view_is_not_open() {
    let dir = std::env::temp_dir().join(format!("shale-busy-mail-start-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::vault::write_sample_vault(&dir).unwrap();
    let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
    app.cfg.mail_accounts = vec![crate::config::MailAccount { id: "a".into(), ..Default::default() }];
    assert!(!app.tabbar.tabs.iter().any(|t| t.view == crate::app::View::Mail));
    let mut h = Headless::new();
    h.frames(&mut app, 1);
    assert!(!matches!(app.mail.store, crate::mail::ui::StoreState::Closed), "the first frame starts the open");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: JOB-016
#[test]
fn job_016_the_mail_view_shows_the_rebuild_of_the_index_with_a_spinner_and_cancel_while_its_task_runs() {
    use crate::mail::ui::{StoreState, MAIL_REBUILD};
    let (mut app, mut h, dir) = window("mail-rebuild");
    app.show_view(crate::app::View::Mail, false);
    let store = std::sync::Arc::new(crate::mail::store::MailStore::open(&dir.join("mail-test")).unwrap());
    app.mail.store = StoreState::Rebuilding(store);
    let mut task = app.busy.begin(MAIL_REBUILD, "Rebuilding the mail index…").expect("the registry of the window is the mail view's");
    let cancel = task.cancellable();
    h.frames(&mut app, 3);
    assert!(painted_at(&h, "Rebuilding the mail index…").is_some(), "the view says what runs");
    assert!(painted_at(&h, "Rebuilding the mail index from the message files…").is_none());
    let button = h.find_label(&mut app, "Cancel");
    h.click(&mut app, button.center(), PointerButton::Primary);
    assert!(cancel.is_cancelled(), "Cancel in the view reaches the job");
    drop(task);
    h.frames(&mut app, 3);
    assert!(painted_at(&h, "Rebuilding the mail index…").is_none());
    assert!(painted_at(&h, "Rebuilding the mail index from the message files…").is_some(), "no task: the plain line");
    let _ = std::fs::remove_dir_all(&dir);
}

fn window(tag: &str) -> (ShaleApp, Headless, std::path::PathBuf) {
    let dir = std::env::temp_dir().join(format!("shale-busy-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::vault::write_sample_vault(&dir).unwrap();
    let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
    let mut h = Headless::new();
    h.frames(&mut app, 4);
    (app, h, dir)
}

fn painted_at(h: &Headless, text: &str) -> Option<Pos2> {
    h.text.iter().find(|(t, _)| t == text).map(|(_, p)| *p)
}

// spec: JOB-007
#[test]
fn job_007_the_indicator_sits_top_right_of_the_editor_pane_lists_the_tasks_and_cancels_from_the_list() {
    let (mut app, mut h, dir) = window("indicator");
    assert!(painted_at(&h, "Syncing with git…").is_none() && !h.text.iter().any(|(t, _)| t.contains("running")), "nothing runs: no indicator");
    let _sync = app.busy.begin(SYNC, "Syncing with git…").unwrap();
    h.frames(&mut app, 3);
    let at = painted_at(&h, "Syncing with git…").expect("one task: its text");
    let screen = Vec2::new(1500.0, 900.0);
    assert!(at.x > screen.x * 0.6 && at.x < screen.x && at.y < 150.0, "top right of the window's editor area: {at:?}");
    let mut export = app.busy.begin(OTHER, "Exporting PDF…").unwrap();
    let cancel = export.cancellable();
    h.frames(&mut app, 3);
    let pill = painted_at(&h, "2 tasks running").expect("several tasks: how many");
    assert!(painted_at(&h, "Syncing with git…").is_none());
    // hovering lists them with Cancel where the work can stop
    h.frame(&mut app, vec![Event::PointerMoved(pill + Vec2::new(10.0, 4.0))]);
    h.frames(&mut app, 3);
    assert!(painted_at(&h, "Syncing with git…").is_some() && painted_at(&h, "Exporting PDF…").is_some(), "{:?}", h.text);
    assert_eq!(h.text.iter().filter(|(t, _)| t == "Cancel").count(), 1, "only the PDF export can be stopped");
    let button = h.find_label(&mut app, "Cancel");
    h.click(&mut app, button.center(), PointerButton::Primary);
    assert!(cancel.is_cancelled(), "Cancel in the list reaches the task");
    assert!(app.busy.info(&OTHER).is_some_and(|t| t.cancelling), "it stays listed until its job has ended");
    drop(export);
    drop(_sync);
    h.frames(&mut app, 4);
    assert!(painted_at(&h, "Exporting PDF…").is_none() && !h.text.iter().any(|(t, _)| t.contains("running") || t == "Syncing with git…"), "the indicator is gone with the work: {:?}", h.text);
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: JOB-007
#[test]
fn job_007_a_click_pins_the_list_open_and_the_indicator_follows_the_rightmost_pane() {
    let (mut app, mut h, dir) = window("pinned");
    let _t = app.busy.begin(SYNC, "Syncing with git…").unwrap();
    app.open_in_split_right(crate::app::View::Note("Ideas.md".into()));
    h.frames(&mut app, 4);
    let right = *app.panes.rects.last().unwrap();
    let at = painted_at(&h, "Syncing with git…").expect("the text of the task");
    assert!(right.contains(at) && at.x > right.center().x, "in the rightmost pane: {at:?} {right:?}");
    // a click on it pins the list; moving away keeps it
    h.click(&mut app, at + Vec2::new(10.0, 4.0), PointerButton::Primary);
    h.frame(&mut app, vec![Event::PointerMoved(Pos2::new(5.0, 5.0))]);
    h.frames(&mut app, 3);
    assert!(h.text.iter().any(|(t, _)| t.ends_with(" s")), "the list with the running time stays: {:?}", h.text);
    h.click(&mut app, painted_at(&h, "Syncing with git…").unwrap() + Vec2::new(10.0, 4.0), PointerButton::Primary);
    h.frame(&mut app, vec![Event::PointerMoved(Pos2::new(5.0, 5.0))]);
    h.frames(&mut app, 3);
    assert!(!h.text.iter().any(|(t, _)| t.ends_with(" s")), "a second click closes it");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: JOB-007
#[test]
fn job_007_the_summary_is_the_text_of_one_task_or_the_number_of_several_and_time_is_short() {
    let info = |l: &str| TaskInfo { id: 0, label: l.into(), started: Instant::now(), cancellable: false, cancelling: false };
    assert_eq!(ui::summary(&[info("Syncing with git…")]), "Syncing with git…");
    assert_eq!(ui::summary(&[info("a"), info("b"), info("c")]), "3 tasks running");
    assert_eq!((ui::elapsed(Duration::from_secs(5)), ui::elapsed(Duration::from_secs(59)), ui::elapsed(Duration::from_secs(125))), ("5 s".into(), "59 s".into(), "2 min".into()));
}

// spec: JOB-022
#[test]
fn job_022_a_task_that_waits_to_be_listed_is_running_at_once_and_listed_after_its_time() {
    let busy = Busy::default();
    let after = Duration::from_secs(2);
    let task = busy.begin_listed_after(SYNC, "Reading the vault folder…", after).expect("begins");
    let started = busy.info(&SYNC).expect("running at once").started;
    assert!(busy.is_running(&SYNC) && busy.begin(SYNC, "again").is_none(), "the key is taken from the start, a second one is refused (JOB-005)");
    // the clock is given: nothing sleeps
    assert!(busy.list_at(started).is_empty(), "not listed at the start");
    assert!(busy.list_at(started + Duration::from_millis(1999)).is_empty(), "not listed just before its time");
    let listed = busy.list_at(started + after);
    assert_eq!(listed.iter().map(|t| t.label.as_str()).collect::<Vec<_>>(), ["Reading the vault folder…"], "listed when its time has come");
    assert_eq!(busy.next_listing(started), Some(after), "the indicator wakes when the time has come");
    assert_eq!(busy.next_listing(started + after), None, "nothing waits any more");
    // an ordinary task has no wait
    let other = busy.begin(OTHER, "Working…").expect("begins");
    assert_eq!(busy.list_at(started + Duration::from_millis(1)).len(), 1, "an ordinary task is listed from its start");
    drop((task, other));
    assert!(busy.list_at(started + after).is_empty() && busy.next_listing(started).is_none(), "a task that ended leaves nothing");
}

// spec: JOB-022
#[test]
fn job_022_runs_of_one_action_with_their_own_number_go_side_by_side_and_the_same_number_is_refused() {
    let busy = Busy::default();
    let one = busy.begin(Key::numbered("test-open", 1), "Opening a.mp4…").expect("begins");
    let two = busy.begin(Key::numbered("test-open", 2), "Opening b.mp4…").expect("another file runs next to it");
    assert!(busy.begin(Key::numbered("test-open", 1), "Opening a.mp4…").is_none(), "the same file twice is refused");
    assert_eq!(busy.list().len(), 2);
    drop(one);
    assert!(busy.begin(Key::numbered("test-open", 1), "Opening a.mp4…").is_some(), "free again when it ended");
    drop(two);
}

// spec: JOB-022
#[test]
fn job_022_the_indicator_draws_nothing_for_a_task_that_waits_and_asks_to_be_drawn_again_for_its_time() {
    let ctx = egui::Context::default();
    let busy = Busy::default();
    let _task = busy.begin_listed_after(SYNC, "Learning the words of the tags…", Duration::from_secs(3600)).expect("begins");
    let mut out = ctx.run_ui(RawInput::default(), |ui| ui::indicator(ui.ctx(), &busy, Pos2::new(600.0, 10.0)));
    out.textures_delta.clear();
    let drawn = out.shapes.iter().any(|s| matches!(&s.shape, egui::epaint::Shape::Text(_)));
    assert!(!drawn, "nothing is drawn while the task waits");
    let delay = out.viewport_output.values().map(|v| v.repaint_delay).min().expect("a viewport");
    assert!(delay <= Duration::from_secs(3600), "the window comes back when the time has come: {delay:?}");
}
