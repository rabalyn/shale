//! The file list's sort menu and the dialogs that live in other modules (Calendars, Event,
//! Export, Import), driven with real pointer and key events through the app's `modals` and
//! `file_explorer` (the harness is `panels_ui_tests::P`). Every test ends with what a person
//! would see or what is stored: the order of the rows, a calendar in the settings, an event in
//! the calendar's file, a PDF on disk, a note in the vault.
//! (A child module of `panels`, next to `ui_tests`.)

use super::ui_tests::{opened, Show, P};
use super::*;
use crate::cal_dialog::EventDialog;
use eframe::egui::PointerButton;
use std::time::{Duration, Instant};

/// Run frames until `done` holds (jobs run in the background; no fixed waits).
fn until(p: &mut P, show: Show, what: &str, done: impl Fn(&P) -> bool) {
    let start = Instant::now();
    while !done(p) {
        p.frame(show, vec![]);
        assert!(start.elapsed() < Duration::from_secs(40), "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The rows of the file list, top to bottom, by name.
fn names(p: &P) -> Vec<String> {
    p.app.visible_tree_rows().into_iter().map(|(r, _)| crate::markdown::strip_md_ext(&r.file_name().unwrap().to_string_lossy()).to_string()).collect()
}

fn set_mtime(path: &Path, secs: u64) {
    let f = std::fs::File::open(path).unwrap();
    f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(secs)).unwrap();
}

/// Where a test's local calendars go (its own folder: tests run in parallel).
fn own_calendar_dir(p: &mut P, tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("shale-dialogs-cal-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    p.app.calendar.local_dir = d.clone();
    p.app.calendar.trash_dir = d.join(".trash");
    d
}

fn dialog_rect(p: &P, key: &str) -> Rect {
    let rects = match &p.app.modal {
        Some(Modal::Calendars(d)) => &d.rects,
        Some(Modal::Event(d)) => &d.rects,
        Some(Modal::Import(d)) => &d.rects,
        Some(Modal::Export(d)) => &d.rects,
        Some(Modal::MailAccounts(d)) => &d.rects,
        _ => panic!("no dialog with controls is open"),
    };
    rects.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("no control {key:?}: {:?}", rects.iter().map(|r| &r.0).collect::<Vec<_>>())).1
}

/// Click into the text box `key` of the open dialog and type `text` (replacing what is there).
fn fill(p: &mut P, key: &str, text: &str) {
    p.look(Show::Modals);
    let r = dialog_rect(p, key);
    p.click_rect(Show::Modals, r, PointerButton::Primary);
    p.select_all(Show::Modals);
    p.type_text(Show::Modals, text);
}

fn click_control(p: &mut P, key: &str) {
    p.look(Show::Modals);
    let r = dialog_rect(p, key);
    p.click_rect(Show::Modals, r, PointerButton::Primary);
}

// spec: VAULT-024
#[test]
fn dialogs_the_sort_menu_orders_the_rows_and_names_the_choice() {
    let mut p = P::with_files("sort", &[("Bravo.md", "b\n"), ("Alpha.md", "a\n"), ("Charlie.md", "c\n"), ("Mango/x.md", "x\n"), ("Apple/y.md", "y\n")]);
    // modified times the test sets itself (folders last: writing into them changes theirs)
    for (f, s) in [("Alpha.md", 3000), ("Bravo.md", 1000), ("Charlie.md", 2000), ("Mango/x.md", 100), ("Apple/y.md", 100), ("Mango", 900), ("Apple", 500)] {
        set_mtime(&p.dir.join(f), s);
    }
    p.app.vault.as_mut().unwrap().rescan();
    p.look(Show::Files);
    assert_eq!(p.app.cfg.file_sort, crate::config::FileSort::NameAsc);
    assert_eq!(names(&p), ["Apple", "Mango", "Alpha", "Bravo", "Charlie"]);
    let painted: Vec<String> = p.painted.iter().map(|(t, _)| t.clone()).filter(|t| ["Apple", "Mango", "Alpha", "Bravo", "Charlie"].contains(&t.as_str())).collect();
    assert_eq!(painted, ["Apple", "Mango", "Alpha", "Bravo", "Charlie"], "the screen shows the rows in that order");
    let choose = |p: &mut P, from: &str, to: &str| {
        p.click(Show::Files, &format!("Sort: {from}"));
        p.click(Show::Files, to);
        // the menu is closed again and the button names the new order
        p.find(Show::Files, None, &format!("Sort: {to}"));
        let radios = p.look(Show::Files).iter().filter(|(r, _, _)| *r == egui::accesskit::Role::RadioButton).count();
        assert_eq!(radios, 0, "the menu closes after a choice");
    };
    choose(&mut p, "File name (A to Z)", "File name (Z to A)");
    assert_eq!(names(&p), ["Mango", "Apple", "Charlie", "Bravo", "Alpha"]);
    choose(&mut p, "File name (Z to A)", "Modified (new to old)");
    assert_eq!(names(&p), ["Mango", "Apple", "Alpha", "Charlie", "Bravo"]);
    choose(&mut p, "Modified (new to old)", "Modified (old to new)");
    assert_eq!(names(&p), ["Apple", "Mango", "Bravo", "Charlie", "Alpha"]);
    assert_eq!(p.app.cfg.file_sort, crate::config::FileSort::ModifiedAsc);
    // the creation times are whatever the file system made: check against them
    let created = |p: &P, n: &str| p.app.vault.as_ref().unwrap().files.iter().find(|f| f.rel == Path::new(&format!("{n}.md"))).and_then(|f| f.created);
    for (to, asc) in [("Created (old to new)", true), ("Created (new to old)", false)] {
        let from = p.app.cfg.file_sort.label().to_string();
        choose(&mut p, &from, to);
        let files: Vec<String> = names(&p).into_iter().filter(|n| n.len() > 4 && n != "Apple" && n != "Mango").collect();
        let mut want = files.clone();
        want.sort_by(|a, b| {
            let (ca, cb) = (created(&p, a), created(&p, b));
            (if asc { ca.cmp(&cb) } else { cb.cmp(&ca) }).then(a.to_lowercase().cmp(&b.to_lowercase()))
        });
        assert_eq!(files, want, "{to}");
    }
    // the files are still all there: a sort changes the view, not the vault
    assert!(["Alpha.md", "Bravo.md", "Charlie.md"].iter().all(|f| p.dir.join(f).is_file()));
}

// spec: CAL-001
#[test]
fn dialogs_calendars_add_a_local_calendar_and_keep_it_with_done() {
    let mut p = P::new("cal-add", &[]);
    let dir = own_calendar_dir(&mut p, "add");
    assert!(p.app.cfg.calendars.is_empty());
    p.app.open_calendar_settings();
    p.look(Show::Modals);
    fill(&mut p, "local-name", "Garden club");
    p.click(Show::Modals, "Add");
    // listed in the dialog, not yet saved
    assert!(p.shows(Show::Modals, "Local file"));
    assert!(matches!(&p.app.modal, Some(Modal::Calendars(d)) if d.sources.len() == 1 && d.sources[0].name == "Garden club"));
    assert!(p.app.cfg.calendars.is_empty());
    // Monday and Done: kept
    let monday = p.app.cfg.week_starts_monday;
    click_control(&mut p, "week-monday");
    p.click(Show::Modals, "Done");
    assert!(p.app.modal.is_none());
    assert_eq!(p.app.cfg.week_starts_monday, !monday);
    assert_eq!(p.app.cfg.calendars.len(), 1);
    assert_eq!(p.app.cfg.calendars[0].name, "Garden club");
    assert_eq!(p.app.calendar.sources.len(), 1, "the calendar view got it");
    // Cancel throws changes away: remove it in the dialog, cancel, it is still there
    p.app.open_calendar_settings();
    p.click(Show::Modals, "Remove");
    assert!(matches!(&p.app.modal, Some(Modal::Calendars(d)) if d.sources.is_empty()));
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none());
    assert_eq!(p.app.cfg.calendars.len(), 1);
    // and Esc closes it like Cancel
    p.app.open_calendar_settings();
    p.look(Show::Modals);
    p.key(Show::Modals, Key::Escape);
    assert!(p.app.modal.is_none() && p.app.cfg.calendars.len() == 1);
    let _ = std::fs::remove_dir_all(dir);
}

// spec: CAL-001
#[test]
fn dialogs_calendars_sample_events_are_written_and_a_bad_address_is_refused() {
    let mut p = P::new("cal-sample", &[]);
    let dir = own_calendar_dir(&mut p, "sample");
    p.app.open_calendar_settings();
    fill(&mut p, "local-name", "Samples");
    p.click(Show::Modals, "Add with sample events");
    // saved right away, with events in the calendar's file
    assert_eq!(p.app.cfg.calendars.len(), 1);
    let src = p.app.cfg.calendars[0].clone();
    let file = crate::cal_store::file_of(&src).unwrap().to_path_buf();
    let in_file = crate::cal_ics::parse_ics(&std::fs::read_to_string(&file).unwrap());
    assert!(!in_file.is_empty());
    assert_eq!(p.app.calendar.events.get(&src.id).map(Vec::len), Some(in_file.len()));
    let said = format!("Added {} sample events around today.", in_file.len());
    assert!(p.shows(Show::Modals, &said), "{said}");
    // an address that is not https
    fill(&mut p, "dav-url", "http://calendar.example.com/dav/");
    click_control(&mut p, "dav-add-url");
    assert!(p.shows(Show::Modals, "Can't use this address"));
    assert!(matches!(&p.app.modal, Some(Modal::Calendars(d)) if d.sources.len() == 1));
    // one that is: listed, and kept with Done
    fill(&mut p, "dav-url", "https://127.0.0.1:9/dav/work/");
    click_control(&mut p, "dav-add-url");
    assert!(p.shows(Show::Modals, "Added. Press Done to save."));
    p.click(Show::Modals, "Done");
    assert!(p.app.modal.is_none());
    let dav: Vec<_> = p.app.cfg.calendars.iter().filter(|s| matches!(&s.kind, crate::cal_store::SourceKind::Caldav { url, .. } if url == "https://127.0.0.1:9/dav/work/")).collect();
    assert_eq!(dav.len(), 1);
    assert_eq!(dav[0].name, "work");
    let _ = std::fs::remove_dir_all(dir);
}

fn walk_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk_files(&p));
        } else {
            out.push(p);
        }
    }
    out
}

/// A vault with one local calendar of this test's own, ready for the Event dialog.
fn with_calendar(tag: &str) -> (P, CalendarSource, PathBuf) {
    let mut p = P::new(tag, &[]);
    let dir = own_calendar_dir(&mut p, tag);
    let src = p.app.calendar.new_local_source(&format!("Calendar {tag}"), &[]);
    p.app.cfg.calendars = vec![src.clone()];
    p.app.calendar.set_sources(vec![src.clone()]);
    (p, src, dir)
}

use crate::cal_store::CalendarSource;

const START: i64 = 1_800_000_000;

fn stored(src: &CalendarSource) -> Vec<crate::cal_ics::Event> {
    std::fs::read_to_string(crate::cal_store::file_of(src).unwrap()).map(|t| crate::cal_ics::parse_ics(&t)).unwrap_or_default()
}

// spec: CAL-005
#[test]
fn dialogs_event_new_event_is_typed_saved_and_changed() {
    let (mut p, src, dir) = with_calendar("evt-new");
    p.app.calendar_action(crate::cal_ui::CalAction::NewEventAt(START));
    p.look(Show::Modals);
    assert!(matches!(&p.app.modal, Some(Modal::Event(d)) if d.calendar == src.id));
    // the title box has the keyboard from the start
    p.type_text(Show::Modals, "Dentist");
    fill(&mut p, "location", "Main street 4");
    p.click(Show::Modals, "Save");
    until(&mut p, Show::Modals, "the event is saved", |p| p.app.modal.is_none());
    let evs = stored(&src);
    assert_eq!(evs.len(), 1);
    let e = &evs[0];
    assert_eq!((e.summary.as_str(), e.location.as_str(), e.start, e.end), ("Dentist", "Main street 4", START, START + 1800));
    assert!(p.app.calendar.events.get(&src.id).is_some_and(|v| v.iter().any(|x| x.summary == "Dentist")));
    assert_eq!(p.app.last_event_calendar.as_deref(), Some(src.id.as_str()));
    assert!(!p.app.toasts.is_empty(), "the save says so");
    // change it: same event, new title
    let sid = src.id.clone();
    p.app.modal = Some(Modal::Event(Box::new(EventDialog::edit(sid, evs[0].clone()))));
    p.look(Show::Modals);
    assert!(p.shows(Show::Modals, "Save"));
    fill(&mut p, "title", "Dentist (moved)");
    p.click(Show::Modals, "Save");
    until(&mut p, Show::Modals, "the change is saved", |p| p.app.modal.is_none());
    let evs2 = stored(&src);
    assert_eq!(evs2.len(), 1, "changed, not added");
    assert_eq!((evs2[0].uid.as_str(), evs2[0].summary.as_str()), (evs[0].uid.as_str(), "Dentist (moved)"));
    let _ = std::fs::remove_dir_all(dir);
}

// spec: CAL-005
#[test]
fn dialogs_event_cancel_and_a_wrong_date_store_nothing() {
    let (mut p, src, dir) = with_calendar("evt-bad");
    p.app.calendar_action(crate::cal_ui::CalAction::NewEventAt(START));
    p.look(Show::Modals);
    p.type_text(Show::Modals, "Never saved");
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none());
    assert!(stored(&src).is_empty());
    // a date that does not exist: the dialog stays and says what is wrong
    p.app.calendar_action(crate::cal_ui::CalAction::NewEventAt(START));
    p.look(Show::Modals);
    p.type_text(Show::Modals, "Wrong day");
    fill(&mut p, "start-date", "2027-02-31");
    p.click(Show::Modals, "Save");
    p.look(Show::Modals);
    let Some(Modal::Event(d)) = &p.app.modal else { panic!("the dialog closed") };
    let error = d.error.clone().expect("an error is shown");
    assert!(p.shows(Show::Modals, &error), "{error}");
    assert!(stored(&src).is_empty());
    // fixed: it saves
    fill(&mut p, "start-date", "2027-02-20");
    // the end moved with the start (CAL-031); an end typed before the start is refused
    let Some(Modal::Event(d)) = &p.app.modal else { panic!("the dialog closed") };
    assert_eq!(d.end_date, "2027-02-20");
    fill(&mut p, "end-date", "2027-02-19");
    p.click(Show::Modals, "Save");
    assert!(p.shows(Show::Modals, "The event must end after it starts"));
    assert!(stored(&src).is_empty());
    fill(&mut p, "end-date", "2027-02-20");
    p.click(Show::Modals, "Save");
    until(&mut p, Show::Modals, "the event is saved", |p| p.app.modal.is_none());
    let evs = stored(&src);
    assert_eq!((evs.len(), evs[0].summary.as_str()), (1, "Wrong day"));
    // with no calendar at all, "new event" opens the calendar settings instead
    p.app.cfg.calendars.clear();
    p.app.calendar.set_sources(vec![]);
    p.app.calendar_action(crate::cal_ui::CalAction::NewEventAt(START));
    assert!(matches!(&p.app.modal, Some(Modal::Calendars(d)) if d.sources.is_empty()));
    let _ = std::fs::remove_dir_all(dir);
}

/// The event dialog for a new event at `START`, with the title typed.
fn new_event_dialog(p: &mut P) {
    p.app.calendar_action(crate::cal_ui::CalAction::NewEventAt(START));
    p.look(Show::Modals);
    p.type_text(Show::Modals, "Moves");
}

fn event_fields(p: &P) -> (String, String, String, String) {
    let Some(Modal::Event(d)) = &p.app.modal else { panic!("the dialog closed") };
    (d.start_date.clone(), d.start_time.clone(), d.end_date.clone(), d.end_time.clone())
}

// spec: CAL-031
#[test]
fn dialogs_event_the_end_follows_a_typed_start_date_and_time_and_is_saved_so() {
    let (mut p, src, dir) = with_calendar("evt-follow");
    new_event_dialog(&mut p);
    let (sd, st, ed, et) = event_fields(&p);
    assert_eq!(sd, ed);
    // a later start date moves the end date, not the time
    fill(&mut p, "start-date", "2030-05-09");
    assert_eq!(event_fields(&p), ("2030-05-09".into(), st.clone(), "2030-05-09".into(), et.clone()));
    // a changed start time moves the end time (30 minutes stay), across midnight too
    fill(&mut p, "start-time", "23:45");
    assert_eq!(event_fields(&p), ("2030-05-09".into(), "23:45".into(), "2030-05-10".into(), "00:15".into()));
    // an end typed by hand is the new length, and the next start change keeps it
    fill(&mut p, "end-time", "01:45");
    fill(&mut p, "start-date", "2030-06-01");
    assert_eq!(event_fields(&p), ("2030-06-01".into(), "23:45".into(), "2030-06-02".into(), "01:45".into()));
    // an end that does not read as a time is left alone
    fill(&mut p, "end-time", "9:");
    fill(&mut p, "start-time", "10:00");
    assert_eq!(event_fields(&p).3, "9:");
    fill(&mut p, "end-time", "11:00");
    p.click(Show::Modals, "Save");
    until(&mut p, Show::Modals, "the event is saved", |p| p.app.modal.is_none());
    let evs = stored(&src);
    assert_eq!(evs.len(), 1);
    let day = |y, m, d| crate::cal_time::days_from_civil(y, m, d);
    assert_eq!((evs[0].start, evs[0].end), (crate::cal_time::local_to_utc(day(2030, 6, 1), 36_000), crate::cal_time::local_to_utc(day(2030, 6, 2), 39_600)));
    let _ = std::fs::remove_dir_all(dir);
}

// spec: CAL-031
#[test]
fn dialogs_event_an_all_day_event_keeps_its_days_when_the_start_moves() {
    let (mut p, src, dir) = with_calendar("evt-follow-day");
    new_event_dialog(&mut p);
    click_control(&mut p, "all-day");
    fill(&mut p, "start-date", "2030-05-09");
    fill(&mut p, "end-date", "2030-05-11");
    fill(&mut p, "start-date", "2030-05-20");
    let (_, _, ed, _) = event_fields(&p);
    assert_eq!(ed, "2030-05-22", "three days stay three days");
    p.click(Show::Modals, "Save");
    until(&mut p, Show::Modals, "the event is saved", |p| p.app.modal.is_none());
    let evs = stored(&src);
    assert_eq!(evs.len(), 1);
    let day = crate::cal_time::days_from_civil(2030, 5, 20);
    assert!(evs[0].all_day);
    assert_eq!((evs[0].start, evs[0].end), (day * 86_400, (day + 3) * 86_400));
    let _ = std::fs::remove_dir_all(dir);
}

// spec: PDF-001
#[test]
fn dialogs_export_writes_the_pdf_where_asked_and_says_when_it_cannot() {
    let mut p = P::new("export", &[]);
    p.app.open_export_dialog(Path::new("Welcome.md"));
    p.look(Show::Modals);
    let target = p.dir.join("exports/Welcome.pdf");
    assert!(matches!(&p.app.modal, Some(Modal::Export(d)) if Path::new(&d.path) == target));
    // the choices are the dialog's: landscape and a page size go into the file
    p.click(Show::Modals, "Landscape");
    assert!(matches!(&p.app.modal, Some(Modal::Export(d)) if d.opts.landscape));
    p.click(Show::Modals, "Export");
    assert!(p.app.modal.is_none());
    let pdf = std::fs::read(&target).unwrap();
    assert!(pdf.starts_with(b"%PDF"), "a PDF file");
    assert!(p.app.toasts.iter().any(|(t, _)| t.starts_with("Exported to") && t.contains("Welcome.pdf")), "{:?}", p.app.toasts);
    // a place that can't be written: the dialog stays with the reason
    std::fs::write(p.dir.join("in-the-way"), "a file").unwrap();
    p.app.open_export_dialog(Path::new("Ideas.md"));
    p.look(Show::Modals);
    if let Some(Modal::Export(d)) = &mut p.app.modal {
        d.path = p.dir.join("in-the-way/out.pdf").to_string_lossy().into_owned();
    }
    p.click(Show::Modals, "Export");
    let Some(Modal::Export(d)) = &p.app.modal else { panic!("the dialog closed") };
    let error = d.error.clone().expect("an error");
    assert!(error.contains("Could not create the folder"), "{error}");
    assert!(p.shows(Show::Modals, &error));
    // no place at all
    if let Some(Modal::Export(d)) = &mut p.app.modal {
        d.path.clear();
    }
    p.click(Show::Modals, "Export");
    assert!(p.shows(Show::Modals, "Choose where to save the PDF"));
    // Cancel
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none() && !p.dir.join("in-the-way/out.pdf").exists());
}

// spec: IMP-002
#[test]
fn dialogs_import_makes_the_notes_and_stays_open_on_a_wrong_folder() {
    let mut p = P::new("import", &[]);
    let outside = p.dir.with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("Meeting notes.md"), "# Meeting\n\nText from outside\n").unwrap();
    std::fs::write(outside.join("More.md"), "# More\n").unwrap();
    let (items, notes) = crate::import::items_from_paths(&[outside.join("Meeting notes.md")]);
    p.app.open_import(items, notes, None);
    p.look(Show::Modals);
    assert!(p.shows(Show::Modals, "Import 1 item"));
    // a folder that can't be used: the dialog stays and says so
    fill(&mut p, "folder", "../outside");
    p.click(Show::Modals, "Import 1 item");
    assert!(matches!(&p.app.modal, Some(Modal::Import(d)) if d.error.is_some()));
    let msg = match &p.app.modal {
        Some(Modal::Import(d)) => d.error.clone().unwrap(),
        _ => unreachable!(),
    };
    assert!(p.shows(Show::Modals, &msg), "{msg}");
    // another file added by its path
    fill(&mut p, "add-path", &outside.join("More.md").to_string_lossy());
    click_control(&mut p, "add");
    assert!(p.shows(Show::Modals, "Import 2 items"));
    // a new folder, a tag and Import: both notes are in the vault, the dialog is gone
    fill(&mut p, "folder", "Imported/Inbox");
    fill(&mut p, "tags", "from-outside");
    p.click(Show::Modals, "Import 2 items");
    assert!(p.app.modal.is_none());
    let a = p.dir.join("Imported/Inbox");
    let mut made: Vec<String> = std::fs::read_dir(&a).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    made.sort();
    assert_eq!(made.len(), 2, "{made:?}");
    let text = std::fs::read_to_string(a.join(&made[1])).unwrap() + &std::fs::read_to_string(a.join(&made[0])).unwrap();
    assert!(text.contains("tags: [from-outside]") && text.contains("[[z_media/") && text.contains("Meeting notes.md]]"), "{text}");
    // the files themselves were copied into the vault's media folder
    let copies: Vec<PathBuf> = walk_files(&p.dir.join("z_media")).into_iter().filter(|f| f.to_string_lossy().ends_with("Meeting notes.md")).collect();
    assert_eq!(copies.len(), 1);
    assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), "# Meeting\n\nText from outside\n");
    assert!(p.app.toasts.iter().any(|(t, _)| t.contains("Imported 2 notes into “Imported/Inbox”")), "{:?}", p.app.toasts);
    assert_eq!(p.app.vdata.state.last_import_folder, "Imported/Inbox");
    assert_eq!(p.app.vdata.state.recent_tags.first().map(String::as_str), Some("from-outside"));
    assert!(p.app.expanded.contains(Path::new("Imported/Inbox")) && p.app.expanded.contains(Path::new("Imported")));
    let acts = p.actions();
    assert!(acts.iter().any(|a| matches!(a, Action::Rescan)));
    assert_eq!(opened(&acts).len(), 1);
    // Cancel makes nothing
    let (items, notes) = crate::import::items_from_paths(&[outside.join("More.md")]);
    p.app.open_import(items, notes, None);
    p.click(Show::Modals, "Cancel");
    assert!(p.app.modal.is_none());
    assert_eq!(std::fs::read_dir(&a).unwrap().count(), 2);
    let _ = std::fs::remove_dir_all(&outside);
}

// spec: DRAW-012
#[test]
fn dialogs_convert_again_makes_a_second_drawing_and_open_uses_the_first() {
    let mut p = P::with_files("convert-again", &[("Maps/Map.canvas", crate::app::tests::CANVAS), ("Ideas.md", "# Ideas\n")]);
    let canvas = Path::new("Maps/Map.canvas");
    let first = p.app.convert_canvas(canvas, false).expect("the first conversion");
    assert_eq!(first, Path::new("Maps/Map.excalidraw.md"));
    p.app.drawings.remove(canvas);
    p.app.open(Path::new("Ideas.md"), false);
    // the canvas is opened again: the question names the copy made before
    p.app.open(canvas, false);
    assert!(matches!(&p.app.modal, Some(Modal::ConvertCanvas { existing: Some(e), .. }) if *e == first));
    assert!(p.shows(Show::Modals, "It was converted before: “Maps/Map.excalidraw.md”."));
    // "Open the drawing" opens the copy, no second one
    p.click(Show::Modals, "Open the drawing");
    assert!(p.app.modal.is_none());
    let acts = p.actions();
    assert!(matches!(acts.as_slice(), [Action::Open { rel, .. }] if *rel == first), "{} actions", acts.len());
    // "Convert again" asks for another drawing from the canvas as it is now
    p.app.open(canvas, false);
    p.click(Show::Modals, "Convert again");
    assert!(p.app.modal.is_none());
    let acts = p.actions();
    let [Action::ConvertCanvas { rel, new_tab }] = acts.as_slice() else { panic!("{} actions", acts.len()) };
    assert_eq!(rel, canvas);
    let second = p.app.convert_canvas(rel, *new_tab).expect("the second conversion");
    assert_ne!(second, first);
    assert!(p.dir.join(&first).is_file() && p.dir.join(&second).is_file(), "both drawings exist");
    assert_eq!(std::fs::read_to_string(p.dir.join(canvas)).unwrap(), crate::app::tests::CANVAS, "the canvas is never changed");
}

// spec: MAIL-040
#[test]
fn dialogs_mail_accounts_wrong_entries_are_explained_and_a_good_one_is_saved() {
    let mut p = P::new("mail-accounts", &[]);
    p.app.open_mail_settings();
    p.look(Show::Modals);
    click_control(&mut p, "md:add");
    // nothing typed: the first thing missing is named, and nothing is saved
    click_control(&mut p, "md:done");
    assert!(p.shows(Show::Modals, "Every account needs a server (host name)."));
    assert!(p.app.cfg.mail_accounts.is_empty() && p.app.modal.is_some());
    fill(&mut p, "md:host", "127.0.0.1");
    click_control(&mut p, "md:done");
    assert!(p.shows(Show::Modals, "needs a user name."));
    fill(&mut p, "md:user", "ada");
    fill(&mut p, "md:port", "99999");
    click_control(&mut p, "md:done");
    assert!(p.shows(Show::Modals, "the port must be a number between 1 and 65535."));
    fill(&mut p, "md:port", "1993");
    fill(&mut p, "md:minutes", "often");
    click_control(&mut p, "md:done");
    assert!(p.shows(Show::Modals, "“Sync every” must be a whole number of minutes"));
    assert!(p.app.cfg.mail_accounts.is_empty());
    // the rest is right: saved
    fill(&mut p, "md:minutes", "7");
    if matches!(&p.app.modal, Some(Modal::MailAccounts(d)) if !d.mark_read) {
        click_control(&mut p, "md:mark-read");
    }
    fill(&mut p, "md:mark-read-secs", "5");
    fill(&mut p, "md:undo-secs", "0");
    click_control(&mut p, "md:done");
    assert!(p.app.modal.is_none(), "closed");
    assert_eq!(p.app.cfg.mail_accounts.len(), 1);
    let a = &p.app.cfg.mail_accounts[0];
    assert_eq!((a.host.as_str(), a.user.as_str(), a.port), ("127.0.0.1", "ada", 1993));
    assert_eq!((p.app.cfg.mail_sync_minutes, p.app.cfg.mail_mark_read, p.app.cfg.mail_mark_read_secs, p.app.cfg.mail_undo_secs), (7, true, 5, 0));
    // Cancel keeps the account, removing it and saying Done takes it away (the mail on disk stays)
    p.app.open_mail_settings();
    p.look(Show::Modals);
    click_control(&mut p, "md:remove");
    click_control(&mut p, "md:cancel");
    assert!(p.app.modal.is_none() && p.app.cfg.mail_accounts.len() == 1);
    p.app.open_mail_settings();
    p.look(Show::Modals);
    click_control(&mut p, "md:remove");
    click_control(&mut p, "md:done");
    assert!(p.app.modal.is_none() && p.app.cfg.mail_accounts.is_empty());
    assert!(p.app.toasts.iter().any(|(t, _)| t.starts_with("The account was removed.")), "{:?}", p.app.toasts);
}
