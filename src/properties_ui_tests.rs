//! The front matter in the reading view (EDIT-063, EDIT-064, EDIT-086, CFG-052).

use crate::config::PropertiesView;
use crate::reader::{self, Caches, Extras, ReaderAction, SceneCache};
use crate::vault::Vault;
use eframe::egui::{self, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use std::path::Path;

const MAIL: &str = "---\ntype: email\nsubject: \"Plan\"\nfrom: \"Anna Müller <anna@x.de>\"\nto: \"Bob <bob@x.de>\"\ndate: 2026-11-12 14:30\nmessage-id: \"<abc@x.de>\"\ntags: [work]\n---\n\nBody text\n";

fn vault(tag: &str) -> Vault {
    let d = std::env::temp_dir().join(format!("shale-props-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    crate::vault::write_sample_vault(&d).unwrap();
    std::fs::write(d.join("Anna Müller.md"), "Contact\n").unwrap();
    Vault::open(&d).unwrap()
}

fn walk(s: &egui::Shape, out: &mut Vec<(String, Rect)>) {
    match s {
        egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.visual_bounding_rect())),
        egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
        _ => {}
    }
}

/// Run frames of the reading view; `clicks` are texts to click (press and release) once they are
/// painted. Returns the last painted texts and the action taken.
fn run(v: &Vault, text: &str, view: PropertiesView, clicks: &[&str]) -> (Vec<(String, Rect)>, Option<ReaderAction>) {
    let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut frame = |events: Vec<Event>| {
        time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 500.0))), time: Some(time), events, ..Default::default() };
        let mut action = None;
        let mut painted: Vec<(String, Rect)> = vec![];
        let mut out = ctx.run_ui(raw, |ui| {
            action = reader::show(ui, text, v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras { properties: view, ..Default::default() });
        });
        out.textures_delta.clear();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut painted));
        (action, painted)
    };
    frame(vec![]);
    let (mut action, mut painted) = frame(vec![]);
    for c in clicks {
        let at = painted.iter().find(|(t, _)| t == c).unwrap_or_else(|| panic!("{c:?} is painted: {painted:?}")).1.center();
        let click = |pressed| vec![Event::PointerMoved(at), Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }];
        frame(click(true));
        let (a, _) = frame(click(false));
        action = a.or(action);
        (_, painted) = frame(vec![]);
    }
    (painted, action)
}

fn texts(p: &[(String, Rect)]) -> Vec<&str> {
    p.iter().map(|(t, _)| t.as_str()).collect()
}

// spec: EDIT-063, EDIT-061
#[test]
fn a_mail_card_shows_the_header_and_no_raw_yaml_or_ids() {
    let v = vault("mail");
    let (painted, _) = run(&v, MAIL, PropertiesView::Card, &[]);
    let t = texts(&painted);
    for want in ["Anna Müller", "to", "Bob", "2026-11-12 14:30", "#work", "Body text"] {
        assert!(t.contains(&want), "{want:?} in {t:?}");
    }
    assert!(!t.iter().any(|x| x.contains("message-id") || x.contains("abc@x.de") || x.contains("type: email")), "{t:?}");
    let date = painted.iter().find(|(x, _)| x == "2026-11-12 14:30").unwrap().1;
    let from = painted.iter().find(|(x, _)| x == "Anna Müller").unwrap().1;
    assert!(date.min.x > from.max.x + 100.0, "the date is right-aligned: {date:?} {from:?}");
}

// spec: EDIT-063, CFG-052
#[test]
fn the_toggle_shows_all_raw_properties_and_hides_them_again() {
    let v = vault("toggle");
    let (closed, _) = run(&v, MAIL, PropertiesView::Card, &[]);
    assert!(!texts(&closed).iter().any(|x| x.contains("message-id")));
    let (open, _) = run(&v, MAIL, PropertiesView::Card, &["▸"]);
    assert!(texts(&open).iter().any(|x| x.contains("message-id")), "{:?}", texts(&open));
    assert!(texts(&open).contains(&"▾"));
}

// spec: EDIT-063, CFG-052, EDIT-060
#[test]
fn the_folded_view_is_one_line_and_the_raw_view_all_lines() {
    let v = vault("modes");
    let (folded, _) = run(&v, MAIL, PropertiesView::Folded, &[]);
    let t = texts(&folded);
    assert!(t.contains(&"email · Anna Müller · 2026-11-12 · #work"), "{t:?}");
    assert!(!t.iter().any(|x| x.contains("message-id")));
    let (raw, _) = run(&v, MAIL, PropertiesView::Raw, &[]);
    assert!(texts(&raw).iter().any(|x| x.starts_with("message-id:")), "{:?}", texts(&raw));
}

// spec: EDIT-063
#[test]
fn an_event_card_has_the_time_place_and_a_word_card_the_page_chip() {
    let v = vault("kinds");
    let event = "---\ntype: event\ntitle: Standup\nstart: 2026-11-12 14:30\nend: 2026-11-12 15:00\nlocation: Room 4\nrepeats: weekly\nevent-uid: \"u1\"\n---\n";
    let (painted, _) = run(&v, event, PropertiesView::Card, &[]);
    let t = texts(&painted);
    for want in ["2026-11-12 14:30 – 15:00", "repeats weekly", "Room 4"] {
        assert!(t.contains(&want), "{want:?} in {t:?}");
    }
    assert!(!t.iter().any(|x| x.contains("event-uid")));
    let (painted, _) = run(&v, "---\nprint-paper: A4\nprint-margins: 25 25 25 25\nprint-font-size: 11\n---\n", PropertiesView::Card, &[]);
    assert!(texts(&painted).contains(&"A4 · 2.5 cm margins · 11 pt"), "{:?}", texts(&painted));
    let (painted, _) = run(&v, "---\nstatus: open\n---\n", PropertiesView::Card, &[]);
    assert!(texts(&painted).contains(&"status:") && texts(&painted).contains(&"open"));
}

// spec: EDIT-064
#[test]
fn a_person_with_a_contact_note_is_a_link_and_one_without_is_text() {
    let v = vault("people");
    let (_, action) = run(&v, MAIL, PropertiesView::Card, &["Anna Müller"]);
    match action {
        Some(ReaderAction::OpenLink { target, .. }) => assert_eq!(target, "Anna Müller"),
        _ => panic!("the contact is a link"),
    }
    let (_, action) = run(&v, MAIL, PropertiesView::Card, &["Bob"]);
    assert!(action.is_none(), "Bob has no note, so no link");
}

// spec: EDIT-108
#[test]
fn the_date_of_an_email_and_an_event_card_is_a_link_that_asks_for_that_day() {
    let v = vault("dates");
    let day = crate::calendar::time::days_from_civil(2026, 11, 12);
    let (_, action) = run(&v, MAIL, PropertiesView::Card, &["2026-11-12 14:30"]);
    assert!(matches!(action, Some(ReaderAction::OpenDay(d)) if d == day), "the email date");
    let event = "---\ntype: event\ntitle: Standup\nstart: 2026-11-12 14:30\nend: 2026-11-12 15:00\n---\n";
    let (_, action) = run(&v, event, PropertiesView::Card, &["2026-11-12 14:30 – 15:00"]);
    assert!(matches!(action, Some(ReaderAction::OpenDay(d)) if d == day), "the event time");
    let odd = MAIL.replace("date: 2026-11-12 14:30", "date: yesterday");
    let (_, action) = run(&v, &odd, PropertiesView::Card, &[]);
    assert!(action.is_none(), "no click, no action");
}

// spec: EDIT-108
#[test]
fn the_open_day_action_selects_the_day_in_the_calendar_of_the_sidebar() {
    let (mut app, _) = crate::app::tests::test_app("open-day");
    let day = crate::calendar::time::days_from_civil(2031, 3, 9);
    let mut actions = vec![];
    crate::app::handle_reader_action(&mut actions, ReaderAction::OpenDay(day), Path::new("a.md"));
    assert!(matches!(actions.as_slice(), [crate::app::Action::OpenCalendarDay(d)] if *d == day));
    app.cfg.show_right = false;
    app.actions = actions;
    app.process_actions();
    assert_eq!((app.cal.side.anchor, app.cal.side.selected, app.cfg.show_right), (day, day, true));
}

// spec: EDIT-086
#[test]
fn the_reading_view_draws_a_code_block_as_a_box_with_a_badge_at_its_top_right() {
    let v = vault("code");
    let (painted, _) = run(&v, "```rust\nlet a = 1;\n```\n\n~~~\nplain\n~~~\n", PropertiesView::Raw, &[]);
    let at = |t: &str| painted.iter().find(|(p, _)| p == t).unwrap_or_else(|| panic!("{t:?} in {:?}", texts(&painted))).1;
    let (badge, code) = (at("rust"), at("let a = 1;"));
    assert!(badge.min.x > code.min.x + 100.0, "the badge is at the right, the code at the left: {badge:?} {code:?}");
    assert!(badge.max.y <= code.min.y + 1.0, "above the first line of code: {badge:?} {code:?}");
    assert!(texts(&painted).contains(&"code"), "a block without a language says code: {:?}", texts(&painted));
}

// spec: EDIT-093
#[test]
fn the_reading_view_has_a_copy_button_left_of_the_badge_that_copies_the_code() {
    let v = vault("copy");
    let text = "```rust\nlet a = 1;\nlet b = 2;\n```\n";
    let (mut scenes, media) = (SceneCache::default(), crate::media::MediaPlayer::default());
    let ctx = egui::Context::default();
    let mut time = 0.0;
    let mut frame = |events: Vec<Event>| {
        time += 0.05;
        let raw = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 500.0))), time: Some(time), events, ..Default::default() };
        let mut painted: Vec<(String, Rect)> = vec![];
        let mut out = ctx.run_ui(raw, |ui| {
            reader::show(ui, text, &v, Path::new("Welcome.md"), &mut Caches { scenes: &mut scenes, media: &media }, (15.0, true), &Extras::default());
        });
        out.textures_delta.clear();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut painted));
        let copied: Vec<String> = out.platform_output.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(s) = c { Some(s.clone()) } else { None }).collect();
        (painted, copied)
    };
    frame(vec![]);
    let (painted, _) = frame(vec![]);
    let badge = painted.iter().find(|(t, _)| t == "rust").unwrap().1;
    let at = Pos2::new(badge.min.x - 6.0 - 8.0, badge.center().y);
    frame(vec![Event::PointerMoved(at)]);
    frame(vec![Event::PointerMoved(at)]);
    let click = |pressed| vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE }];
    let mut copied = frame(click(true)).1;
    copied.extend(frame(click(false)).1);
    assert_eq!(copied, vec!["let a = 1;\nlet b = 2;".to_string()]);
    let (painted, _) = frame(vec![]);
    assert!(texts(&painted).contains(&"Copied"), "{:?}", texts(&painted));
}

// spec: CFG-052
#[test]
fn the_folded_view_has_the_toggle_for_the_raw_lines_and_the_raw_view_has_none() {
    let v = vault("modes-toggle");
    let (folded, _) = run(&v, MAIL, PropertiesView::Folded, &[]);
    assert!(texts(&folded).contains(&"▸"), "{:?}", texts(&folded));
    let (open, _) = run(&v, MAIL, PropertiesView::Folded, &["▸"]);
    assert!(texts(&open).iter().any(|x| x.contains("message-id")), "{:?}", texts(&open));
    assert!(texts(&open).contains(&"▾"));
    let (raw, _) = run(&v, MAIL, PropertiesView::Raw, &[]);
    assert!(!texts(&raw).contains(&"▸") && !texts(&raw).contains(&"▾"), "{:?}", texts(&raw));
}
