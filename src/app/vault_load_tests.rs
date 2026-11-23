//! The vault is read off the UI thread (NET-019): the start screen with its spinner and Cancel,
//! the open that finishes when the vault has arrived, and every way to open a vault.

use super::vault_load::{After, VAULT_OPEN};
use super::{Options, ShaleApp, View};
use crate::ui_test_support::Headless;
use crate::vault::Vault;
use eframe::egui::PointerButton;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

struct W {
    app: ShaleApp,
    hl: Headless,
    base: PathBuf,
    /// where the opener runs, once it has started (the thread that reads the vault)
    thread: Arc<std::sync::Mutex<Option<std::thread::ThreadId>>>,
    /// the opener waits for this to be set
    gate: Arc<AtomicBool>,
    /// how many times the opener was called
    calls: Arc<AtomicUsize>,
}

impl W {
    /// A window with no vault, whose way to read a vault waits for `gate` and notes where it runs.
    fn new(tag: &str) -> W {
        let base = std::env::temp_dir().join(format!("shale-vault-load-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for name in ["one", "two"] {
            std::fs::create_dir_all(base.join(name)).unwrap();
            std::fs::write(base.join(name).join("Welcome.md"), format!("# {name}\n")).unwrap();
        }
        let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
        let (thread, gate, calls) = (Arc::new(std::sync::Mutex::new(None)), Arc::new(AtomicBool::new(true)), Arc::new(AtomicUsize::new(0)));
        let (t, g, c) = (thread.clone(), gate.clone(), calls.clone());
        app.vault_load.with_opener(move |p| {
            c.fetch_add(1, Ordering::SeqCst);
            *t.lock().unwrap() = Some(std::thread::current().id());
            let start = Instant::now();
            while !g.load(Ordering::SeqCst) && start.elapsed() < Duration::from_secs(20) {
                std::thread::sleep(Duration::from_millis(5));
            }
            Vault::open(p).map_err(|e| e.to_string())
        });
        let mut w = W { app, hl: Headless::new(), base, thread, gate, calls };
        w.hl.frames(&mut w.app, 3);
        w
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.base.join(name)
    }

    /// Make the next loads wait until `release`.
    fn hold(&self) {
        self.gate.store(false, Ordering::SeqCst);
    }

    fn release(&self) {
        self.gate.store(true, Ordering::SeqCst);
    }

    fn painted(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t == text)
    }

    fn root(&self) -> Option<PathBuf> {
        self.app.vault.as_ref().map(|v| v.root.clone())
    }

    fn settle(&mut self) {
        self.hl.settle(&mut self.app);
    }

    /// Frames until the opener has started.
    fn until_started(&mut self) {
        let start = Instant::now();
        while self.thread.lock().unwrap().is_none() {
            self.hl.frames(&mut self.app, 1);
            assert!(start.elapsed() < Duration::from_secs(20), "the load never started");
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

impl Drop for W {
    fn drop(&mut self) {
        self.release();
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

// spec: NET-019
#[test]
fn net_019_the_vault_is_read_on_another_thread_and_the_window_keeps_drawing_frames() {
    let mut w = W::new("thread");
    w.hold();
    let one = w.dir("one");
    w.app.open_vault_in_background(&one, After::Nothing);
    assert!(w.app.vault_load.is_loading());
    w.until_started();
    assert_ne!(*w.thread.lock().unwrap(), Some(std::thread::current().id()), "not the UI thread");
    // the load is as slow as a share can be: the frames still come, with the start screen and its spinner
    let started = Instant::now();
    for _ in 0..30 {
        w.hl.frames(&mut w.app, 1);
    }
    assert!(started.elapsed() < Duration::from_secs(10), "thirty frames did not wait for the load");
    assert!(w.app.vault.is_none(), "the vault has not arrived");
    assert!(w.painted("Shale") && w.painted("Opening one…"), "the start screen says what it opens: {:?}", w.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.painted("Cancel"));
    assert_eq!(w.app.busy.info(&VAULT_OPEN).map(|t| t.label), Some("Opening one…".to_string()), "the load is listed with the other busy tasks");
    assert!(w.app.busy_jobs().contains(&"the loading of a vault"));
    w.release();
    w.settle();
    assert_eq!(w.root(), Some(one));
    assert!(!w.app.vault_load.is_loading());
    assert!(w.app.busy.info(&VAULT_OPEN).is_none(), "the task has ended");
    assert!(!w.painted("Opening one…"), "the main window replaced the start screen");
}

// spec: NET-019
#[test]
fn net_019_nothing_that_depends_on_the_vault_starts_before_it_has_arrived() {
    let mut w = W::new("nothing");
    w.hold();
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    w.until_started();
    w.app.rescan.due_now();
    w.hl.frames(&mut w.app, 10);
    assert!(w.app.vault.is_none() && w.app.tabbar.tabs.is_empty() && w.app.notes.is_empty(), "no tab and no note before the vault is there");
    assert!(!w.app.rescan.in_flight(), "no rescan of a vault that is not there");
    assert!(w.app.cfg.vaults.entries().is_empty() && w.app.cfg.vault.is_none(), "the vault is not remembered before it is open");
    assert!(w.app.sync.remote.is_none() && w.app.sync.last_start.is_none());
    w.release();
    w.settle();
    assert!(!w.app.tabbar.tabs.is_empty(), "the tabs come with the vault");
    assert_eq!(w.app.cfg.vault, Some(w.dir("one")), "the vault is remembered when it is open");
}

// spec: NET-019
#[test]
fn net_019_cancel_drops_the_load_and_the_start_screen_stays_usable() {
    let mut w = W::new("cancel");
    w.hold();
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    w.until_started();
    w.hl.frames(&mut w.app, 2);
    // the button of the start screen
    let at = w.hl.text.iter().find(|(t, _)| t == "Cancel").map(|(_, p)| *p).expect("a Cancel button");
    w.hl.click(&mut w.app, at + eframe::egui::vec2(10.0, 6.0), PointerButton::Primary);
    w.hl.frames(&mut w.app, 2);
    assert!(!w.app.vault_load.is_loading(), "Cancel ends the load at once");
    assert!(w.app.busy.info(&VAULT_OPEN).is_none(), "the task is gone, so another vault can be picked");
    assert!(w.painted("Open folder") && !w.painted("Cancel"), "the start screen offers the open box again");
    // the read finishes later: its result is ignored
    w.release();
    std::thread::sleep(Duration::from_millis(100));
    w.hl.frames(&mut w.app, 5);
    assert!(w.app.vault.is_none(), "a cancelled load does not open the vault");
    // another vault can be opened now
    w.app.open_vault_in_background(&w.dir("two"), After::Nothing);
    w.settle();
    assert_eq!(w.root(), Some(w.dir("two")));
    assert_eq!(w.calls.load(Ordering::SeqCst), 2);
}

// spec: NET-019
#[test]
fn net_019_the_indicators_cancel_stops_the_load_too() {
    let mut w = W::new("indicator-cancel");
    w.hold();
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    w.until_started();
    let id = w.app.busy.info(&VAULT_OPEN).expect("listed").id;
    assert!(w.app.busy.cancel(id), "the task can be stopped");
    w.hl.frames(&mut w.app, 2);
    assert!(!w.app.vault_load.is_loading());
    w.release();
    w.hl.frames(&mut w.app, 3);
    assert!(w.app.vault.is_none());
}

// spec: NET-019
#[test]
fn net_019_a_vault_that_cannot_be_read_shows_why_on_the_start_screen() {
    let mut w = W::new("error");
    w.app.vault_load.with_opener(|_| Err("the share is gone".into()));
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    w.settle();
    assert!(w.app.vault.is_none());
    assert!(!w.app.vault_load.is_loading());
    assert!(w.painted("Could not open vault: the share is gone"), "{:?}", w.hl.text.iter().map(|t| &t.0).collect::<Vec<_>>());
    assert!(w.painted("Open folder"), "the start screen is usable");
    // a new try clears the message
    w.app.vault_load.with_opener(|p| Vault::open(p).map_err(|e| e.to_string()));
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    assert!(w.app.vault_load.error.is_none());
    w.settle();
    assert_eq!(w.root(), Some(w.dir("one")));
}

// spec: NET-019
#[test]
fn net_019_a_second_open_while_one_loads_is_refused_until_cancel() {
    let mut w = W::new("one-at-a-time");
    w.hold();
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    w.app.open_vault_in_background(&w.dir("two"), After::Nothing);
    assert_eq!(w.app.vault_load.target(), Some(w.dir("one").as_path()));
    w.release();
    w.settle();
    assert_eq!(w.root(), Some(w.dir("one")));
}

// spec: NET-019
#[test]
fn net_019_the_start_dialog_opens_a_folder_in_the_background() {
    let mut w = W::new("dialog");
    w.hold();
    w.app.welcome_path = w.dir("two").to_string_lossy().into_owned();
    w.hl.frames(&mut w.app, 2);
    let at = w.hl.text.iter().find(|(t, _)| t == "Open folder").map(|(_, p)| *p).expect("the button");
    w.hl.click(&mut w.app, at + eframe::egui::vec2(10.0, 6.0), PointerButton::Primary);
    w.hl.frames(&mut w.app, 2);
    assert!(w.app.vault_load.is_loading(), "Open folder starts a load");
    assert!(w.painted("Opening two…"));
    w.release();
    w.settle();
    assert_eq!(w.root(), Some(w.dir("two")));
}

// spec: NET-019
#[test]
fn net_019_the_start_of_the_window_reads_the_vault_in_the_background_and_applies_the_view() {
    let w = W::new("startup");
    let ctx = eframe::egui::Context::default();
    let opts = Options { screenshot: None, vault: Some(w.dir("one")), view: Some("tasks".into()), keys: None };
    let mut app = ShaleApp::build_with(opts, false, Some(&ctx));
    assert!(app.vault.is_none(), "build returns before the vault has been read");
    assert!(app.vault_load.is_loading());
    // (this window reads for real: the opener of the test window does not apply to it)
    let mut hl = Headless::new();
    hl.settle(&mut app);
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(w.dir("one")));
    assert!(matches!(app.tabbar.tabs[app.tabbar.active].view, View::Tasks), "--view is applied when the vault has arrived");
}

// spec: NET-019
#[test]
fn net_019_a_folder_handed_over_by_a_second_start_is_read_in_the_background() {
    let mut w = W::new("handover");
    w.app.open_vault(&w.dir("one"));
    w.hold();
    w.app.open_handed_over(&w.dir("two"));
    assert!(w.app.vault_load.is_loading());
    assert_eq!(w.root(), Some(w.dir("one")), "the open vault stays until the other one has arrived");
    w.release();
    w.settle();
    assert_eq!(w.root(), Some(w.dir("two")));
}

// spec: NET-019
#[test]
fn net_019_the_user_guide_opens_in_the_background_on_its_start_note() {
    let mut w = W::new("guide");
    let root = w.dir("guide-data");
    w.app.guide = crate::userguide::Places { files: crate::userguide::tests::SAMPLE, root: Some(root.clone()) };
    w.hold();
    w.app.open_user_guide();
    assert!(w.app.vault_load.is_loading());
    w.release();
    w.settle();
    assert_eq!(w.root(), Some(root.join(format!("userguide-{}", env!("CARGO_PKG_VERSION")))));
    let open: Vec<_> = w.app.tabbar.tabs.iter().filter_map(|t| t.view.path().map(Path::to_path_buf)).collect();
    assert_eq!(open, [PathBuf::from("Start here.md")]);
}

// spec: NET-019
#[test]
fn net_019_a_scripted_run_waits_for_the_vault() {
    // the script feeds its first step in frame 30: with the vault not there, it holds
    let mut w = W::new("script");
    w.hold();
    w.app.script = crate::script::ScriptRun::new(crate::script::parse("wait:1").unwrap_or_default().into());
    w.app.open_vault_in_background(&w.dir("one"), After::Nothing);
    w.until_started();
    let steps = w.app.script.steps.len();
    w.hl.frames(&mut w.app, 80);
    assert_eq!(w.app.script.steps.len(), steps, "no step was fed while the vault loads");
    w.release();
    w.settle();
}

// spec: VAULT-040, VAULT-006
#[test]
fn vault_040_a_folder_from_the_command_line_is_added_after_it_has_loaded() {
    let w = W::new("cli-adds");
    let ctx = eframe::egui::Context::default();
    let mut app = ShaleApp::build_with(Options { screenshot: None, vault: Some(w.dir("one")), view: None, keys: None }, false, Some(&ctx));
    assert!(app.vault_load.is_loading() && app.cfg.vaults.entries().is_empty(), "not listed while it loads");
    Headless::new().settle(&mut app);
    let listed: Vec<_> = app.cfg.vaults.entries().iter().map(|e| (e.name.clone(), e.path.clone())).collect();
    assert_eq!(listed, [("one".to_string(), crate::known_vaults::canonical(&w.dir("one")))]);
}

// spec: VAULT-040
#[test]
fn vault_040_a_folder_from_the_command_line_that_fails_to_load_is_not_added() {
    let w = W::new("cli-fails");
    let file = w.dir("not-a-folder");
    std::fs::write(&file, "x").unwrap();
    let ctx = eframe::egui::Context::default();
    let mut app = ShaleApp::build_with(Options { screenshot: None, vault: Some(file), view: None, keys: None }, false, Some(&ctx));
    Headless::new().settle(&mut app);
    assert!(app.vault.is_none() && app.vault_load.error.is_some(), "the load failed");
    assert!(app.cfg.vaults.entries().is_empty() && app.cfg.vault.is_none());
}

// spec: VAULT-040
#[test]
fn vault_040_a_folder_handed_over_by_a_second_start_is_added_after_it_has_loaded_and_not_when_it_fails() {
    let mut w = W::new("handover-adds");
    w.app.open_vault(&w.dir("one"));
    w.app.vault_load.with_opener(|_| Err("the share is gone".into()));
    w.app.open_handed_over(&w.dir("two"));
    w.settle();
    assert_eq!(w.app.cfg.vaults.entries().len(), 1, "a failed load adds nothing");
    w.app.vault_load.with_opener(|p| Vault::open(p).map_err(|e| e.to_string()));
    w.hold();
    w.app.open_handed_over(&w.dir("two"));
    assert_eq!(w.app.cfg.vaults.entries().len(), 1, "nothing is added while it loads");
    w.release();
    w.settle();
    let paths: Vec<_> = w.app.cfg.vaults.entries().iter().map(|e| e.path.clone()).collect();
    assert_eq!(paths, [crate::known_vaults::canonical(&w.dir("two")), crate::known_vaults::canonical(&w.dir("one"))]);
}

// spec: VAULT-031
#[test]
fn vault_031_the_window_title_is_the_name_of_the_open_vault() {
    let mut w = W::new("title");
    let titles =
        |out: &eframe::egui::FullOutput| -> Vec<String> { out.viewport_output.values().flat_map(|v| v.commands.iter()).filter_map(|c| if let eframe::egui::ViewportCommand::Title(t) = c { Some(t.clone()) } else { None }).collect() };
    w.app.open_vault(&w.dir("one"));
    let out = w.hl.frame(&mut w.app, vec![]);
    assert_eq!(titles(&out), ["one – Shale"]);
    assert!(titles(&w.hl.frame(&mut w.app, vec![])).is_empty(), "the title is sent when it changes");
    std::fs::create_dir_all(w.dir("sub").join("one")).unwrap();
    w.app.cfg.vaults.add(&w.dir("sub").join("one"));
    let out = w.hl.frame(&mut w.app, vec![]);
    assert_eq!(titles(&out), [format!("one ({}) – Shale", crate::known_vaults::canonical(&w.base).display())], "two vaults with one name: the parent folder follows");
}
