//! More tests of the data usage module (USE): how the last cleanup reads, the record file and
//! its schedule, and the Saved part of the dialog in a headless egui context.

use super::*;
use crate::data_usage::dialog::{self, Ask};
use crate::data_usage::scan::{self, Line, Saved};
use eframe::egui::{self, Event, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

fn temp(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!("shale-usage-gap-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn last(kind: Kind, at: i64, freed: Option<Freed>, error: Option<&str>) -> cleanup::Last {
    cleanup::Last { kind, at, freed, error: error.map(String::from) }
}

// spec: USE-006, USE-024
#[test]
fn the_last_cleanup_reads_well_for_every_kind_of_result() {
    let now = 1_000_000;
    let mut r = cleanup::Record::default();
    assert_eq!(r.text(CacheId::DebugLog, now), "never cleaned");
    let some = Freed { bytes: 1536, entries: 5 };
    r.caches.insert(CacheId::DebugLog, last(Kind::Clear, now - 30, Some(Freed { bytes: 0, entries: 0 }), None));
    assert_eq!(r.text(CacheId::DebugLog, now), "cleared just now: 0 entries, 0 bytes");
    r.caches.insert(CacheId::DebugLog, last(Kind::Prune, now - 5 * 60, Some(Freed { bytes: 1536, entries: 1 }), None));
    assert_eq!(r.text(CacheId::DebugLog, now), "cleaned up 5 minutes ago: 1 entry, 1.5 KB");
    r.caches.insert(CacheId::SocialPictures, last(Kind::Clear, now - 2 * 3_600, Some(some), None));
    assert_eq!(r.text(CacheId::SocialPictures, now), "cleared 2 hours ago: 5 entries, 1.5 KB");
    r.caches.insert(CacheId::TagWords, last(Kind::Prune, now - 3 * 86_400, Some(some), None));
    assert_eq!(r.text(CacheId::TagWords, now), "cleaned up 3 days ago: 5 entries, 1.5 KB");
    // the mail index is "rebuilt" and counts messages, not entries and bytes
    r.caches.insert(CacheId::MailIndex, last(Kind::Clear, now - 120, Some(some), None));
    assert_eq!(r.text(CacheId::MailIndex, now), "rebuilt 2 minutes ago: 5 message(s)");
    // a prune of it is an ordinary cleanup
    r.caches.insert(CacheId::MailIndex, last(Kind::Prune, now - 120, Some(some), None));
    assert_eq!(r.text(CacheId::MailIndex, now), "cleaned up 2 minutes ago: 5 entries, 1.5 KB");
    // a failure names its reason, whatever else is stored
    r.caches.insert(CacheId::MailIndex, last(Kind::Clear, now - 10, Some(some), Some("not now: a sync runs")));
    assert_eq!(r.text(CacheId::MailIndex, now), "rebuilt just now: failed: not now: a sync runs");
    // neither a result nor an error: only when
    r.caches.insert(CacheId::DebugLog, last(Kind::Clear, now - 10 * 60, None, None));
    assert_eq!(r.text(CacheId::DebugLog, now), "cleared 10 minutes ago");
}

// spec: USE-024
#[test]
fn the_record_is_written_and_read_back_and_one_from_a_newer_shale_is_not_written() {
    let d = temp("record-file");
    let path = d.join("shale").join("cache-cleanup.json");
    let missing = cleanup::load(&path);
    assert!(!missing.value.read_only && missing.value.caches.is_empty() && missing.value.last_run.is_none(), "a missing file counts as never");
    // a record that is written is read again as it was
    let mut r = missing.value;
    r.last_run = Some(1_700_000_000);
    r.note(&Finished { id: CacheId::SocialPictures, kind: Kind::Clear, result: Ok(Freed { bytes: 150, entries: 2 }) }, 1_700_000_100);
    r.note(&Finished { id: CacheId::MailIndex, kind: Kind::Prune, result: Err("disk full".into()) }, 1_700_000_200);
    r.save(&path).unwrap();
    let back = cleanup::load(&path).value;
    assert_eq!(back.last_run, Some(1_700_000_000));
    assert_eq!(back.caches, r.caches);
    assert_eq!(back.caches[&CacheId::SocialPictures], last(Kind::Clear, 1_700_000_100, Some(Freed { bytes: 150, entries: 2 }), None));
    assert_eq!(back.caches[&CacheId::MailIndex], last(Kind::Prune, 1_700_000_200, None, Some("disk full")));
    // a record from a newer Shale is used but never written
    let newer = br#"{"version": 99, "last_run": 5, "from_the_future": {"x": 1}}"#;
    fs::write(&path, newer).unwrap();
    let l = cleanup::load(&path);
    assert!(l.value.read_only && l.problem.is_some());
    assert_eq!(l.value.last_run, Some(5));
    let mut l = l.value;
    l.note(&Finished { id: CacheId::DebugLog, kind: Kind::Clear, result: Ok(Freed::default()) }, 10);
    l.save(&path).unwrap();
    assert_eq!(fs::read(&path).unwrap(), newer, "the file is as it was");
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-023
#[test]
fn the_record_says_the_cleanup_is_due_when_it_never_ran_ran_a_day_ago_or_the_clock_went_back() {
    let mut r = cleanup::Record::default();
    assert!(r.due(1_000_000), "never ran");
    r.last_run = Some(1_000_000);
    assert!(!r.due(1_000_000 + 86_399));
    assert!(r.due(1_000_000 + 86_400));
    assert!(r.due(999_000), "a last run in the future (the clock went back) counts as due");
}

/// The dialog's content in a headless egui context, frame by frame.
struct Frames {
    ctx: egui::Context,
    t: f64,
    text: Vec<(String, Pos2)>,
    asks: Vec<Ask>,
}

fn collect(shape: &egui::Shape, out: &mut Vec<(String, Pos2)>) {
    match shape {
        egui::Shape::Text(t) => out.push((t.galley.text().to_string(), t.pos)),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

impl Frames {
    fn new() -> Frames {
        let ctx = egui::Context::default();
        crate::theme::apply(&ctx);
        Frames { ctx, t: 0.0, text: vec![], asks: vec![] }
    }

    fn frame(&mut self, u: &mut DataUsage, events: Vec<Event>) {
        self.t += 0.1;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 1600.0))), time: Some(self.t), events, ..Default::default() };
        let mut r = CacheRetention::default();
        let asks = &mut self.asks;
        let mut out = self.ctx.run_ui(raw, |ui| asks.extend(dialog::show(ui, u, &mut r)));
        out.textures_delta.clear();
        self.text.clear();
        out.shapes.iter().for_each(|c| collect(&c.shape, &mut self.text));
    }

    fn shows(&self, text: &str) -> bool {
        self.text.iter().any(|(t, _)| t.contains(text))
    }

    /// A real click on the painted text.
    fn click(&mut self, u: &mut DataUsage, text: &str) {
        let at = self.text.iter().find(|(t, _)| t == text).unwrap_or_else(|| panic!("{text:?} is not on screen: {:?}", self.text)).1 + Vec2::new(3.0, 3.0);
        self.frame(u, vec![Event::PointerMoved(at)]);
        self.frame(u, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
        self.frame(u, vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
        for _ in 0..8 {
            self.frame(u, vec![]);
        }
    }
}

fn usage(tag: &str) -> (DataUsage, PathBuf) {
    let d = temp(tag);
    let mut u = DataUsage::new(Places::in_folder(&d));
    u.places.vault = Some(d.join("vault"));
    (u, d)
}

// spec: USE-005
#[test]
fn saved_shows_counting_until_the_numbers_are_in_and_says_when_it_could_not_count() {
    let (mut u, d) = usage("dlg-state");
    let mut f = Frames::new();
    assert!(u.saved.is_none());
    f.frame(&mut u, vec![]);
    assert!(f.shows("counting…"));
    assert!(!f.shows("In the vault") && !f.shows("Outside the vault"), "nothing is listed before the count is in");
    u.saved = Some(Err("permission denied".into()));
    f.frame(&mut u, vec![]);
    assert!(f.shows("Could not count: permission denied"));
    assert!(!f.shows("Outside the vault"));
    let saved = Saved { outside: vec![Line { label: "Config".into(), path: None, files: 1, bytes: 2048, problem: None }], ..Saved::default() };
    u.saved = Some(Ok(saved));
    f.frame(&mut u, vec![]);
    assert!(f.shows("Outside the vault") && f.shows("Config") && f.shows("2.0 KB"));
    assert!(!f.shows("Could not count"));
    let _ = fs::remove_dir_all(&d);
}

// spec: USE-003
#[test]
fn a_click_on_a_largest_file_opens_it_and_one_on_a_largest_folder_shows_it() {
    let (mut u, d) = usage("dlg-largest");
    u.saved = Some(Ok(Saved { largest_files: vec![(PathBuf::from("Notes/Big.md"), 5_000)], largest_folders: vec![(PathBuf::from("Projects/Deep"), 9_000)], ..Saved::default() }));
    let mut f = Frames::new();
    f.frame(&mut u, vec![]);
    f.click(&mut u, &format!("Largest {} files", scan::LARGEST));
    assert!(f.shows("4.9 KB"), "the list is open with the size of the file: {:?}", f.text);
    f.click(&mut u, "Notes/Big.md");
    assert_eq!(f.asks, [Ask::Open(PathBuf::from("Notes/Big.md"))]);
    f.asks.clear();
    f.click(&mut u, &format!("Largest {} folders", scan::LARGEST));
    f.click(&mut u, "Projects/Deep");
    assert_eq!(f.asks, [Ask::ShowFolder(PathBuf::from("Projects/Deep"))]);
    let _ = fs::remove_dir_all(&d);
}
