//! `ShaleApp::calendar_action` and the finished calendar jobs of a frame, on a local calendar
//! with a repeating event that has one changed date (CAL-005, CAL-007, CAL-011, CAL-021).

use super::tests::test_app;
use super::*;
use crate::calendar::ics::{parse_ics, to_ics, Event};
use crate::calendar::store::{CalendarSource, Job, Occ};
use crate::calendar::ui::CalAction;
use std::sync::{Arc, Mutex};

const START: i64 = 1_800_000_000;
const DAY: i64 = 86_400;
/// the changed date of the series (it starts an hour later than the series says)
const MOVED_FROM: i64 = START + DAY;
const MOVED_TO: i64 = MOVED_FROM + 3_600;
/// a date of the series nobody changed
const PLAIN: i64 = START + 2 * DAY;

struct Cal {
    app: ShaleApp,
    src: CalendarSource,
    dir: PathBuf,
}

/// A vault app with a local calendar holding "Standup": daily, four times, the second date
/// changed to "Standup (moved)".
fn with_series(tag: &str) -> Cal {
    let (mut app, vault) = test_app(&format!("calact-{tag}"));
    let dir = vault.join("calendars");
    std::fs::create_dir_all(&dir).unwrap();
    app.calendar.local_dir = dir.clone();
    app.calendar.trash_dir = dir.join(".trash");
    let src = app.calendar.new_local_source(&format!("Cal {tag}"), &[]);
    app.cfg.calendars = vec![src.clone()];
    app.calendar.set_sources(vec![src.clone()]);
    let master = Event { uid: "standup".into(), summary: "Standup".into(), start: START, end: START + 1800, rrule: Some("FREQ=DAILY;COUNT=4".into()), ..Default::default() };
    let moved = Event { summary: "Standup (moved)".into(), recurrence_id: Some(MOVED_FROM), ..master.override_at(MOVED_TO, MOVED_TO + 1800) };
    std::fs::write(crate::calendar::store::file_of(&src).unwrap(), to_ics(&src.name, &[master, moved])).unwrap();
    app.calendar.reload_local();
    Cal { app, src, dir: vault }
}

fn stored(c: &Cal) -> Vec<Event> {
    parse_ics(&std::fs::read_to_string(crate::calendar::store::file_of(&c.src).unwrap()).unwrap())
}

fn all_occurrences(c: &Cal) -> Vec<Occ> {
    let day = START.div_euclid(DAY);
    c.app.calendar.occurrences(day - 2, day + 8)
}

/// The occurrence of the series that starts at `start`.
fn occ(c: &Cal, start: i64) -> Occ {
    all_occurrences(c).into_iter().find(|o| o.start == start).unwrap_or_else(|| panic!("no occurrence at {start}"))
}

/// The event the open event dialog edits (its id, its changed date, its title).
fn editing(c: &Cal) -> (String, Option<i64>, String) {
    let Some(Modal::Event(d)) = &c.app.modal else { panic!("no event dialog: {:?}", c.app.toasts) };
    let (cal, ev) = d.original.clone().expect("it edits an event");
    assert_eq!(cal, c.src.id);
    assert_eq!(d.title, ev.summary);
    (ev.uid, ev.recurrence_id, ev.summary)
}

/// Run frames until the jobs of the calendar are all done.
fn finish_jobs(c: &mut Cal) {
    let ctx = egui::Context::default();
    let start = Instant::now();
    while !c.app.cal.jobs.is_empty() {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| c.app.run_frame(ui));
        out.textures_delta.clear();
        assert!(start.elapsed() < Duration::from_secs(40), "the calendar job did not end");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// spec: CAL-007
#[test]
fn calendar_action_edit_opens_the_series_and_a_changed_date_asks_for_the_series_too() {
    let mut c = with_series("edit");
    assert_eq!(all_occurrences(&c).len(), 4, "three plain dates and the changed one");
    // a plain date: the series
    let o = occ(&c, PLAIN);
    c.app.calendar_action(CalAction::Edit(o));
    assert_eq!(editing(&c), ("standup".into(), None, "Standup".into()));
    let Some(Modal::Event(d)) = &c.app.modal else { unreachable!() };
    assert_eq!(d.calendar, c.src.id);
    assert!(matches!(d.repeat, crate::calendar::dialog::RepeatMode::Every(_)), "the repeat is in the dialog");
    // "all repeats" on the changed date: still the series, not the changed copy
    c.app.modal = None;
    let o = occ(&c, MOVED_TO);
    c.app.calendar_action(CalAction::Edit(o));
    assert_eq!(editing(&c), ("standup".into(), None, "Standup".into()));
    let _ = std::fs::remove_dir_all(&c.dir);
}

// spec: CAL-007
#[test]
fn calendar_action_edit_one_edits_a_changed_date_as_it_is_and_copies_the_series_for_a_plain_one() {
    let mut c = with_series("edit-one");
    // a date that was changed before: as it is
    let o = occ(&c, MOVED_TO);
    c.app.calendar_action(CalAction::EditOne(o));
    assert_eq!(editing(&c), ("standup".into(), Some(MOVED_FROM), "Standup (moved)".into()));
    let Some(Modal::Event(d)) = &c.app.modal else { unreachable!() };
    let (_, ev) = d.original.as_ref().unwrap();
    assert_eq!((ev.start, ev.end), (MOVED_TO, MOVED_TO + 1800));
    // a plain date: a copy of the series for that date, without the repeat
    c.app.modal = None;
    let o = occ(&c, PLAIN);
    c.app.calendar_action(CalAction::EditOne(o));
    assert_eq!(editing(&c), ("standup".into(), Some(PLAIN), "Standup".into()));
    let Some(Modal::Event(d)) = &c.app.modal else { unreachable!() };
    let (_, ev) = d.original.as_ref().unwrap();
    assert_eq!((ev.start, ev.end, ev.rrule.as_deref()), (PLAIN, PLAIN + 1800, None));
    assert!(matches!(d.repeat, crate::calendar::dialog::RepeatMode::Never));
    let _ = std::fs::remove_dir_all(&c.dir);
}

// spec: CAL-007
#[test]
fn calendar_action_on_an_occurrence_that_is_not_found_does_nothing() {
    let mut c = with_series("gone");
    let real = occ(&c, PLAIN);
    let wrong_event = Occ { event: 99, ..real.clone() };
    let wrong_source = Occ { source: 5, ..real };
    for o in [wrong_event, wrong_source] {
        for a in [CalAction::Edit(o.clone()), CalAction::EditOne(o.clone()), CalAction::DeleteOne(o.clone()), CalAction::Delete(o)] {
            c.app.calendar_action(a);
            assert!(c.app.modal.is_none());
            assert!(c.app.cal.jobs.is_empty());
            assert!(c.app.toasts.is_empty(), "{:?}", c.app.toasts);
        }
    }
    assert_eq!(stored(&c).len(), 2, "nothing was written");
    let _ = std::fs::remove_dir_all(&c.dir);
}

// spec: CAL-007, CAL-011
#[test]
fn calendar_action_delete_one_removes_just_that_date_and_a_changed_date_with_its_copy() {
    let mut c = with_series("delete-one");
    let o = occ(&c, PLAIN);
    c.app.calendar_action(CalAction::DeleteOne(o));
    assert_eq!(c.app.cal.jobs.len(), 1, "a job runs");
    assert_eq!(c.app.cal.jobs[0].1, c.src.id);
    finish_jobs(&mut c);
    let evs = stored(&c);
    let master = evs.iter().find(|e| e.recurrence_id.is_none()).unwrap();
    assert_eq!(master.exdates, vec![PLAIN]);
    assert!(evs.iter().any(|e| e.recurrence_id == Some(MOVED_FROM)), "the changed date stays");
    assert!(c.app.toasts.iter().any(|(t, _)| t.starts_with("This date was removed")), "{:?}", c.app.toasts);
    let starts: Vec<i64> = all_occurrences(&c).iter().map(|o| o.start).collect();
    assert!(!starts.contains(&PLAIN) && starts.contains(&START), "{starts:?}");
    // the changed date: its copy goes and the date the series had is left out
    let o = occ(&c, MOVED_TO);
    c.app.calendar_action(CalAction::DeleteOne(o));
    finish_jobs(&mut c);
    let evs = stored(&c);
    assert_eq!(evs.len(), 1, "only the series is left");
    let mut ex = evs[0].exdates.clone();
    ex.sort();
    assert_eq!(ex, vec![MOVED_FROM, PLAIN]);
    let _ = std::fs::remove_dir_all(&c.dir);
}

// spec: CAL-011
#[test]
fn calendar_action_delete_removes_the_event_with_all_its_dates_and_keeps_a_copy() {
    let mut c = with_series("delete");
    let o = occ(&c, PLAIN);
    c.app.calendar_action(CalAction::Delete(o));
    assert_eq!(c.app.cal.jobs.len(), 1);
    finish_jobs(&mut c);
    assert!(stored(&c).is_empty(), "the series and its changed date are gone");
    assert!(c.app.calendar.events[&c.src.id].is_empty(), "the calendar is loaded again");
    assert!(c.app.toasts.iter().any(|(t, _)| t.starts_with("Event deleted. A copy is kept in")), "{:?}", c.app.toasts);
    assert!(std::fs::read_dir(&c.app.calendar.trash_dir).unwrap().count() > 0, "a copy is in the trash");
    let _ = std::fs::remove_dir_all(&c.dir);
}

// spec: CAL-011, CAL-021
#[test]
fn calendar_action_delete_in_a_read_only_calendar_says_so_and_starts_no_job() {
    let mut c = with_series("read-only");
    c.app.calendar.read_only.insert(c.src.id.clone());
    let o = occ(&c, PLAIN);
    c.app.calendar_action(CalAction::DeleteOne(o.clone()));
    let name = c.src.name.clone();
    assert_eq!(c.app.toasts.last().unwrap().0, format!("Could not remove the date: “{name}” is read-only"));
    c.app.calendar_action(CalAction::Delete(o));
    assert_eq!(c.app.toasts.last().unwrap().0, format!("Could not delete the event: “{name}” is read-only"));
    assert!(c.app.cal.jobs.is_empty());
    assert_eq!(stored(&c).len(), 2, "nothing was written");
    let _ = std::fs::remove_dir_all(&c.dir);
}

// spec: CAL-021
#[test]
fn a_frame_keeps_a_running_calendar_job_and_ends_a_finished_one_with_a_reload_and_a_toast() {
    let mut c = with_series("jobs");
    let ctx = egui::Context::default();
    let frame = |c: &mut Cal| {
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| c.app.run_frame(ui));
        out.textures_delta.clear();
    };
    let running: Job = Arc::new(Mutex::new(None));
    let done: Job = Arc::new(Mutex::new(None));
    let failed: Job = Arc::new(Mutex::new(Some(Err("the calendar file is gone".to_string()))));
    c.app.cal.jobs.push((running.clone(), c.src.id.clone()));
    // the file changes behind the app; the end of a job loads it again
    std::fs::write(crate::calendar::store::file_of(&c.src).unwrap(), to_ics(&c.src.name, &[])).unwrap();
    frame(&mut c);
    assert_eq!(c.app.cal.jobs.len(), 1, "a job without a result stays");
    assert!(c.app.toasts.is_empty(), "{:?}", c.app.toasts);
    assert_eq!(c.app.calendar.events[&c.src.id].len(), 2, "not loaded again yet");
    // one finishes, one fails: both leave the list, each says its message
    *running.lock().unwrap() = Some(Ok("Event saved".to_string()));
    c.app.cal.jobs.push((failed, c.src.id.clone()));
    c.app.cal.jobs.push((done.clone(), c.src.id.clone()));
    frame(&mut c);
    assert_eq!(c.app.cal.jobs.len(), 1, "only the job without a result is left");
    assert!(Arc::ptr_eq(&c.app.cal.jobs[0].0, &done));
    let said: Vec<&str> = c.app.toasts.iter().map(|(t, _)| t.as_str()).collect();
    assert_eq!(said, ["Event saved", "the calendar file is gone"]);
    assert!(c.app.calendar.events[&c.src.id].is_empty(), "the calendar was loaded again");
    let _ = std::fs::remove_dir_all(&c.dir);
}
