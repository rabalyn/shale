//! The host of the file picker (PICK-001 to PICK-003): the system's dialog first, with a pretend
//! dialog behind the `Native` trait, and Shale's own picker as the fallback.

use super::{Filter, Native, Outcome, Picked, Request};
use crate::app::{Options, ShaleApp};
use crate::ui_test_support::{poll_until, Headless};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A system dialog that answers what the test says and remembers what it was asked.
struct Fake {
    answer: Outcome,
    seen: Mutex<Vec<Request>>,
}

impl Native for Fake {
    fn pick(&self, req: &Request) -> Outcome {
        self.seen.lock().unwrap().push(req.clone());
        self.answer.clone()
    }
}

fn window(answer: Outcome) -> (ShaleApp, Headless, Arc<Fake>) {
    let fake = Arc::new(Fake { answer, seen: Mutex::new(vec![]) });
    let mut app = ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false);
    app.pick = super::Host::with_native(fake.clone());
    (app, Headless::new(), fake)
}

fn wait_for_answer(app: &mut ShaleApp, hl: &mut Headless, key: &str) -> Picked {
    let mut got = None;
    poll_until("the answer of the file dialog", || {
        hl.frame(app, vec![]);
        got = app.pick.take(key);
        got.is_some()
    });
    got.unwrap()
}

// spec: PICK-002
#[test]
fn pick_002_the_system_dialog_answers_several_files_and_is_given_the_kinds_of_file() {
    let files = vec![PathBuf::from("/tmp/a.eml"), PathBuf::from("/tmp/b.eml")];
    let (mut app, mut hl, fake) = window(Outcome::Picked(files.clone()));
    app.pick.ask("k", Request::files("Add files").filter(Filter::new("Mail", &["eml"])));
    let picked = wait_for_answer(&mut app, &mut hl, "k");
    assert_eq!(picked.paths, files);
    let seen = fake.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].mode, seen[0].filters.clone()), (super::Mode::Files, vec![Filter::new("Mail", &["eml"])]));
    assert!(app.pick.take("k").is_none(), "an answer is taken once");
}

// spec: PICK-002
#[test]
fn pick_002_cancel_in_the_system_dialog_answers_nothing_and_opens_nothing() {
    let (mut app, mut hl, fake) = window(Outcome::Cancelled);
    app.pick.ask("k", Request::folder("Choose"));
    poll_until("the dialog was asked and is done", || {
        hl.frame(&mut app, vec![]);
        !fake.seen.lock().unwrap().is_empty() && !app.pick.running()
    });
    hl.frames(&mut app, 3);
    assert!(app.pick.take("k").is_none());
    assert!(app.pick.own_picker().is_none(), "Cancel does not fall back to Shale's own picker");
}

// spec: PICK-003, CFG-084
#[test]
fn pick_003_without_a_system_dialog_shales_own_picker_opens() {
    let (mut app, mut hl, fake) = window(Outcome::Unavailable);
    app.pick.ask("k", Request::folder("Choose"));
    poll_until("Shale's own picker", || {
        hl.frame(&mut app, vec![]);
        app.pick.own_picker().is_some()
    });
    assert_eq!(fake.seen.lock().unwrap().len(), 1, "the system's dialog was tried first");
}

// spec: PICK-003, CFG-084
#[test]
fn pick_003_the_setting_off_goes_straight_to_shales_own_picker_and_so_does_a_refused_choice() {
    let (mut app, mut hl, fake) = window(Outcome::Picked(vec![PathBuf::from("/tmp/x")]));
    app.cfg.system_file_dialog = false;
    app.pick.ask("k", Request::folder("Choose"));
    poll_until("Shale's own picker", || {
        hl.frame(&mut app, vec![]);
        app.pick.own_picker().is_some()
    });
    assert!(fake.seen.lock().unwrap().is_empty(), "the system's dialog was not asked");
    // a refused choice shows its reason, which only Shale's own picker can
    let (mut app, mut hl, fake) = window(Outcome::Picked(vec![PathBuf::from("/tmp/x")]));
    app.pick.ask("k", Request::folder("Choose").refused("That folder does not exist."));
    poll_until("Shale's own picker", || {
        hl.frame(&mut app, vec![]);
        app.pick.own_picker().is_some()
    });
    assert!(fake.seen.lock().unwrap().is_empty());
    assert_eq!(app.pick.own_picker().unwrap().error.as_deref(), Some("That folder does not exist."));
}

// spec: PICK-001
#[test]
fn pick_001_the_context_hands_asks_to_the_host_and_answers_back() {
    let (mut app, mut hl, _) = window(Outcome::Picked(vec![PathBuf::from("/tmp/x")]));
    hl.frame(&mut app, vec![]);
    super::ask(hl.ctx(), "by-ctx", Request::folder("From a part with no app"));
    let mut got = None;
    poll_until("the answer by the context", || {
        hl.frame(&mut app, vec![]);
        got = super::take(hl.ctx(), "by-ctx");
        got.is_some()
    });
    assert_eq!(got.unwrap().paths, [PathBuf::from("/tmp/x")]);
}

// spec: PICK-010
#[test]
fn pick_010_the_save_request_starts_in_the_folder_of_the_typed_path_and_offers_its_name() {
    let dir = std::env::temp_dir().join(format!("shale-pick-save-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("exports")).unwrap();
    let typed = dir.join("exports").join("Plan.pdf");
    let r = super::save_request("Save the PDF as", &typed.to_string_lossy(), Filter::new("PDF files", &["pdf"]));
    assert_eq!((r.mode, r.file_name.as_str(), r.start.as_deref()), (super::Mode::Save, "Plan.pdf", Some(dir.join("exports").as_path())));
    assert_eq!(r.filters, [Filter::new("PDF files", &["pdf"])]);
    let r = super::save_request("Save", &dir.join("not-there").join("x.pdf").to_string_lossy(), Filter::new("PDF files", &["pdf"]));
    assert_eq!(r.start, None, "a folder that is not there is not a start");
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: PICK-001, PICK-002
#[test]
fn pick_002_asks_wait_their_turn_and_a_second_ask_with_the_same_key_replaces_the_first() {
    let (mut app, mut hl, fake) = window(Outcome::Picked(vec![PathBuf::from("/tmp/x")]));
    app.pick.ask("one", Request::folder("First"));
    app.pick.ask("two", Request::folder("Second"));
    app.pick.ask("two", Request::folder("Second again"));
    assert_eq!(wait_for_answer(&mut app, &mut hl, "one").paths.len(), 1);
    assert_eq!(wait_for_answer(&mut app, &mut hl, "two").paths.len(), 1);
    let titles: Vec<String> = fake.seen.lock().unwrap().iter().map(|r| r.title.clone()).collect();
    assert_eq!(titles, ["First", "Second again"]);
}

fn click_text(app: &mut ShaleApp, hl: &mut Headless, text: &str) {
    let p = hl.text.iter().rev().find(|(t, _)| t.trim() == text).map(|(_, p)| *p).unwrap_or_else(|| panic!("“{text}” is not on the screen: {:?}", hl.text.iter().map(|t| &t.0).collect::<Vec<_>>()));
    hl.click(app, p + eframe::egui::vec2(12.0, 8.0), eframe::egui::PointerButton::Primary);
}

fn wait_for_confirm(app: &mut ShaleApp, hl: &mut Headless) {
    poll_until("the step after the system's dialog", || {
        hl.frame(app, vec![]);
        app.pick.confirm_open()
    });
    hl.frames(app, 4);
}

// spec: PICK-011
#[test]
fn pick_011_a_request_with_a_box_asks_it_after_the_system_dialog_and_answers_with_the_choice() {
    let folder = PathBuf::from("/tmp/new-vault-folder");
    let (mut app, mut hl, _) = window(Outcome::Picked(vec![folder.clone()]));
    app.pick.ask("k", Request::folder("Create a vault").button("Create").option("Start with sample notes"));
    wait_for_confirm(&mut app, &mut hl);
    assert!(app.pick.take("k").is_none(), "nothing is answered before the box was asked");
    assert!(hl.text.iter().any(|(t, _)| t == "/tmp/new-vault-folder") && hl.text.iter().any(|(t, _)| t == "Start with sample notes"), "{:?}", hl.text);
    click_text(&mut app, &mut hl, "Start with sample notes");
    click_text(&mut app, &mut hl, "Create");
    hl.frames(&mut app, 2);
    assert_eq!(app.pick.take("k"), Some(Picked { paths: vec![folder.clone()], option: true, offered: false }));
    // unticked is a choice too
    app.pick.ask("k", Request::folder("Create a vault").button("Create").option("Start with sample notes"));
    wait_for_confirm(&mut app, &mut hl);
    click_text(&mut app, &mut hl, "Create");
    hl.frames(&mut app, 2);
    assert_eq!(app.pick.take("k").map(|p| p.option), Some(false));
}

// spec: PICK-011
#[test]
fn pick_011_cancel_and_escape_in_the_step_answer_nothing_and_a_request_without_a_box_has_no_step() {
    let (mut app, mut hl, _) = window(Outcome::Picked(vec![PathBuf::from("/tmp/x")]));
    app.pick.ask("k", Request::folder("Create a vault").option("Start with sample notes"));
    wait_for_confirm(&mut app, &mut hl);
    click_text(&mut app, &mut hl, "Cancel");
    hl.frames(&mut app, 2);
    assert!(app.pick.take("k").is_none() && !app.pick.confirm_open());
    app.pick.ask("k", Request::folder("Create a vault").option("Start with sample notes"));
    wait_for_confirm(&mut app, &mut hl);
    hl.frame(&mut app, vec![eframe::egui::Event::Key { key: eframe::egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: eframe::egui::Modifiers::NONE }]);
    hl.frames(&mut app, 2);
    assert!(app.pick.take("k").is_none() && !app.pick.confirm_open());
    app.pick.ask("k", Request::folder("Open"));
    assert_eq!(wait_for_answer(&mut app, &mut hl, "k").paths.len(), 1, "answered at once");
}

// spec: PICK-005
#[test]
fn pick_005_a_filter_matches_the_extension_in_any_case_and_no_extension_means_every_file() {
    let f = Filter::new("Pictures", &["png", "jpg"]);
    assert!(f.matches("a.PNG") && f.matches("b.jpg") && !f.matches("c.txt") && !f.matches("png"));
    assert!(Filter::new("All files", &[]).matches("anything"));
}
