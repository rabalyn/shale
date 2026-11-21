//! Drops from other programs and from the window system, in the headless whole window
//! (DEV-012): the events `wl_drop` would send (or the `drop:` step makes) are fed to
//! `handle_drops` without a compositor.

use super::{Modal, Options, ShaleApp};
use crate::import::wl_drop::DropEvent;
use crate::ui_test_support::Headless;
use eframe::egui::{Pos2, Rect};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) struct W {
    pub(super) app: ShaleApp,
    pub(super) hl: Headless,
    /// files to drop, outside the vault
    pub(super) out: PathBuf,
}

impl W {
    pub(super) fn new(tag: &str) -> W {
        Self::with_files(tag, &[])
    }

    /// The same vault plus these files (`Folder/Name.md`, text).
    pub(super) fn with_files(tag: &str, extra: &[(&str, &str)]) -> W {
        let base = std::env::temp_dir().join(format!("shale-drops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, out) = (base.join("vault"), base.join("outside"));
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(dir.join("Welcome.md"), "# Welcome\n\nHello\n").unwrap();
        std::fs::write(dir.join("Sub/Inner.md"), "# Inner\n").unwrap();
        std::fs::write(out.join("first drop.md"), "# First\n\nfrom outside\n").unwrap();
        std::fs::write(out.join("second.md"), "# Second\n").unwrap();
        for (name, text) in extra {
            let f = dir.join(name);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, text).unwrap();
        }
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(dir), view: None, keys: None }, false);
        let mut w = W { app, hl: Headless::new(), out };
        w.frames(6);
        w
    }

    pub(super) fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.hl.frame(&mut self.app, vec![]);
        }
    }

    pub(super) fn file(&self, name: &str) -> PathBuf {
        self.out.join(name)
    }

    /// Run frames until `f` holds (the drop is read in the background).
    pub(super) fn until(&mut self, what: &str, f: impl Fn(&ShaleApp) -> bool) {
        let start = Instant::now();
        loop {
            self.hl.frame(&mut self.app, vec![]);
            if f(&self.app) {
                return;
            }
            assert!(start.elapsed() < Duration::from_secs(40), "timed out waiting for: {what}");
            std::thread::sleep(Duration::from_millis(8));
        }
    }

    /// A place something can be dropped on, as the last frame drew it (the smallest for a
    /// row of the file list, the largest for the editor of the note).
    pub(super) fn zone(&self, folder: &str, note: Option<&str>, smallest: bool) -> Rect {
        let mut zs: Vec<Rect> = self.app.drag.drop_zones.iter().filter(|z| z.folder == Path::new(folder) && z.note.as_deref() == note.map(Path::new)).map(|z| z.rect).collect();
        zs.sort_by(|a, b| a.area().total_cmp(&b.area()));
        let z = if smallest { zs.first() } else { zs.last() };
        *z.unwrap_or_else(|| panic!("no drop zone for {folder:?} {note:?}"))
    }

    pub(super) fn dialog(&self) -> &crate::import::dialog::ImportDialog {
        match &self.app.modal {
            Some(Modal::Import(d)) => d,
            _ => panic!("the Import dialog is not open"),
        }
    }

    pub(super) fn dialog_open(&self) -> bool {
        matches!(self.app.modal, Some(Modal::Import(_)))
    }

    /// Another program drops `paths` at `at` (what `wl_drop` sends for a file list).
    pub(super) fn drop_files(&mut self, at: Pos2, paths: &[PathBuf]) {
        let list: String = paths.iter().map(|p| format!("{}\r\n", crate::import::file_uri(p))).collect();
        self.app.drag.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![("text/uri-list".into(), list.into_bytes())], offered: vec!["text/uri-list".into()], report: vec![] });
    }

    pub(super) fn drop_data(&mut self, at: Pos2, mime: &str, data: &[u8]) {
        self.app.drag.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![(mime.into(), data.to_vec())], offered: vec![mime.into()], report: vec![] });
        self.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    }

    pub(super) fn close_dialog(&mut self) {
        self.app.modal = None;
        self.frames(2);
    }

    pub(super) fn painted(&self, text: &str) -> bool {
        self.hl.text.iter().any(|(t, _)| t.contains(text))
    }
}

// spec: IMP-002
#[test]
fn imp_002_a_drop_on_a_folder_a_note_the_editor_or_nowhere_picks_the_folder() {
    let mut w = W::new("zones");
    let first = w.file("first drop.md");
    // on a folder of the file list: into it, no links
    let at = w.zone("Sub", None, true).center();
    w.drop_files(at, std::slice::from_ref(&first));
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let d = w.dialog();
    assert_eq!((d.folder.as_str(), d.link_on, d.link_into.is_none()), ("Sub", false, true));
    assert_eq!(d.items.len(), 1, "{:?}", d.notes);
    w.close_dialog();
    // on a note of the file list: its folder, links added to that note
    let at = w.zone("", Some("Welcome.md"), true).center();
    w.drop_files(at, std::slice::from_ref(&first));
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let d = w.dialog();
    assert_eq!((d.folder.as_str(), d.link_on, d.link_into.as_deref()), ("", true, Some(Path::new("Welcome.md"))));
    w.close_dialog();
    // on the editor of the open note: the same for that note
    let at = w.zone("", Some("Welcome.md"), false).center();
    w.drop_files(at, std::slice::from_ref(&first));
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let d = w.dialog();
    assert_eq!((d.link_on, d.link_into.as_deref()), (true, Some(Path::new("Welcome.md"))));
    w.close_dialog();
    // nowhere in particular: the folder used last, nothing linked
    let nowhere = (0..90).flat_map(|y| (0..150).map(move |x| Pos2::new(x as f32 * 10.0 + 3.0, y as f32 * 10.0 + 3.0))).find(|p| !w.app.drag.drop_zones.iter().any(|z| z.rect.contains(*p))).expect("a place outside every zone");
    w.app.vdata.state.last_import_folder = "Sub".into();
    w.drop_files(nowhere, &[first]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let d = w.dialog();
    assert_eq!((d.folder.as_str(), d.link_on), ("Sub", false));
}

// spec: IMP-002
#[test]
fn imp_002_a_drop_into_an_open_import_dialog_adds_to_it() {
    let mut w = W::new("more");
    let at = w.zone("", Some("Welcome.md"), true).center();
    w.drop_files(at, &[w.file("first drop.md")]);
    w.until("the first file", |a| matches!(&a.modal, Some(Modal::Import(d)) if d.items.len() == 1));
    // the dialog is open: the second drop goes into it, and the folder it was dropped on wins
    let sub = w.zone("Sub", None, true).center();
    w.drop_files(sub, &[w.file("second.md")]);
    w.until("the second file", |a| matches!(&a.modal, Some(Modal::Import(d)) if d.items.len() == 2));
    assert_eq!(w.dialog().folder, "Sub");
}

// spec: IMP-015
#[test]
fn imp_015_hover_refusal_and_busy_show_and_go_away() {
    let mut w = W::new("hint");
    let at = w.zone("Sub", None, true).center();
    w.app.drag.fake_drops.push(DropEvent::Hover { x: at.x, y: at.y });
    w.frames(2);
    assert_eq!(w.app.drag.ext_hover, Some(at));
    assert!(w.painted("Drop to import") && w.painted("into “Sub”"), "{:?}", w.hl.text);
    w.app.drag.fake_drops.push(DropEvent::Leave);
    w.frames(2);
    assert!(w.app.drag.ext_hover.is_none() && !w.painted("Drop to import"));
    // a browser tab: said at the pointer, until it leaves
    let offered: Vec<String> = ["application/x-tab", "a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
    w.app.drag.fake_drops.push(DropEvent::Refused { x: at.x, y: at.y, offered });
    w.frames(2);
    assert!(w.app.drag.ext_hover.is_none() && w.app.drag.ext_refused.is_some());
    assert!(w.painted("Shale can't take this") && w.painted("application/x-tab, a, b, c, …"), "{:?}", w.hl.text);
    w.app.drag.fake_drops.push(DropEvent::Leave);
    w.frames(2);
    assert!(w.app.drag.ext_refused.is_none() && !w.painted("Shale can't take this"));
    // reading a drop: said until the items are there
    w.app.drag.fake_drops.push(DropEvent::Busy);
    w.frames(2);
    assert!(w.app.drag.drop_busy && w.painted("Reading the dropped message"));
    w.drop_files(at, &[w.file("second.md")]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert!(!w.app.drag.drop_busy && w.app.drag.pending_drops.is_empty());
}

// spec: IMP-016
#[test]
fn imp_016_files_dropped_through_the_window_system_open_the_import_dialog() {
    let mut w = W::new("x11");
    let p = w.file("first drop.md");
    w.hl.frame_with(&mut w.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(super::ScriptDrop(p))));
    assert!(w.dialog_open());
    let d = w.dialog();
    assert_eq!((d.items.len(), d.link_on), (1, false), "{:?}", d.notes);
    // the same through the script step `drop:/path`, and at a place (`drop:T:/path`)
    w.close_dialog();
    let steps = crate::script::parse(&format!("drop:{}", w.file("second.md").display())).unwrap();
    w.app.script.steps = steps.into();
    w.app.frames = 36;
    w.hl.frame_with(&mut w.app, vec![], |app, raw| app.feed_script(raw));
    w.until("the dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!(w.dialog().items.len(), 1);
    w.close_dialog();
    let place = w.zone("Sub", None, true).center();
    let steps = crate::script::parse(&format!("drop:{},{}:{}", place.x, place.y, w.file("second.md").display())).unwrap();
    w.app.script.steps = steps.into();
    w.app.frames = 42;
    w.hl.frame_with(&mut w.app, vec![], |app, raw| app.feed_script(raw));
    w.until("the dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!((w.dialog().folder.as_str(), w.dialog().items.len()), ("Sub", 1));
}

// spec: IMP-016
#[test]
fn imp_016_an_unusable_drop_says_what_was_offered() {
    let mut w = W::new("nothing");
    let at = w.zone("Sub", None, true).center();
    w.drop_data(at, "text/uri-list", b"https://example.org/a.md\r\nfile:///no/such/folder/gone.md\r\n");
    let d = w.dialog();
    assert!(d.items.is_empty());
    assert!(d.notes.iter().any(|n| n.contains("https://example.org/a.md") && n.contains("not a file on this computer")), "{:?}", d.notes);
    assert!(d.notes.iter().any(|n| n.contains("It offered: text/uri-list")), "{:?}", d.notes);
    w.close_dialog();
    // a program that sends the kind but no data
    w.drop_data(at, "text/uri-list", b"");
    assert!(w.dialog().items.is_empty() && w.dialog().notes.iter().any(|n| n.contains("It offered")));
    w.close_dialog();
    // a mail program on X11 offers kinds that give nothing: the way out is explained
    w.app.drag.fake_drops.push(DropEvent::Drop {
        x: at.x,
        y: at.y,
        parts: vec![("text/plain".into(), vec![])],
        offered: vec!["STRING".into(), "DELETE".into()],
        report: vec!["  text/plain: while dragging empty, after the drop empty".into()],
    });
    w.until("the dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let notes = w.dialog().notes.join("\n");
    assert!(notes.contains("XWayland") && notes.contains("text/plain: while dragging empty"), "{notes}");
}

// spec: IMP-016
#[test]
fn imp_016_a_drop_without_a_vault_says_so() {
    let mut w = W::new("novault");
    w.app.vault = None;
    let at = Pos2::new(300.0, 300.0);
    w.app.drag.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![("text/plain".into(), b"just text".to_vec())], offered: vec!["text/plain".into()], report: vec![] });
    w.until("the message", |a| a.toasts.iter().any(|(t, _)| t.contains("Open a vault first")));
    assert!(!w.dialog_open());
}

// spec: IMP-017
#[test]
fn imp_017_dragging_in_the_file_list_hints_and_moves_onto_a_folder() {
    use eframe::egui::Vec2;
    let mut w = W::new("tree");
    let note = w.zone("", Some("Welcome.md"), true).center();
    let sub = w.zone("Sub", None, true).center();
    let nowhere = Pos2::new(750.0, 450.0);
    let root = w.app.vault.as_ref().unwrap().root.clone();
    // over the folder: the hint names the move
    w.hl.press_and_move(&mut w.app, note, &[note + Vec2::new(0.0, 12.0), sub]);
    assert!(w.painted("Move “Welcome” into “Sub”"), "{:?}", w.hl.text);
    // letting go away from every folder moves nothing
    w.hl.release(&mut w.app, nowhere);
    w.frames(3);
    assert!(root.join("Welcome.md").is_file());
    // dropped on the folder: it moves
    w.hl.press_and_move(&mut w.app, note, &[note + Vec2::new(0.0, 12.0), sub]);
    w.hl.release(&mut w.app, sub);
    w.until("the note moved", |_| root.join("Sub/Welcome.md").is_file());
    assert!(!root.join("Welcome.md").exists());
}

/// An event as the calendar hands it to a drag: a repeating one, with a slash in the title.
fn dragged_event(summary: &str) -> crate::calendar::ics::Event {
    crate::calendar::ics::Event {
        uid: "drag-1".into(),
        summary: summary.into(),
        location: "Room 4".into(),
        start: 1_800_000_000,
        end: 1_800_003_600,
        rrule: Some("FREQ=DAILY".into()),
        recurrence_id: Some(1_800_000_000),
        ..Default::default()
    }
}

impl W {
    /// Start dragging a calendar event at `from` and move the pointer to `to`, button down.
    pub(super) fn drag_event(&mut self, ev: crate::calendar::ics::Event, from: Pos2, to: Pos2) {
        eframe::egui::DragAndDrop::set_payload(self.hl.ctx(), crate::calendar::ui::CalDrag { event: ev, calendar: "Work".into() });
        self.hl.frame(&mut self.app, vec![eframe::egui::Event::PointerMoved(from)]);
        self.hl.frame(&mut self.app, vec![eframe::egui::Event::PointerMoved(to)]);
        self.frames(2);
    }

    pub(super) fn dragging(&self) -> bool {
        eframe::egui::DragAndDrop::payload::<crate::calendar::ui::CalDrag>(self.hl.ctx()).is_some()
    }

    /// A point outside every drop zone.
    pub(super) fn nowhere(&self) -> Pos2 {
        (0..90).flat_map(|y| (0..150).map(move |x| Pos2::new(x as f32 * 10.0 + 3.0, y as f32 * 10.0 + 3.0))).find(|p| !self.app.drag.drop_zones.iter().any(|z| z.rect.contains(*p))).expect("a place outside every zone")
    }

    /// The imported event of the only item of the open Import dialog and its calendar file.
    pub(super) fn dialog_event(&self) -> (crate::calendar::ics::Event, String, Option<(String, Vec<u8>)>) {
        let d = self.dialog();
        assert_eq!(d.items.len(), 1);
        match &d.items[0].source {
            crate::import::Source::Event { event, calendar, original } => ((**event).clone(), calendar.clone(), original.clone()),
            other => panic!("not an event: {other:?}"),
        }
    }
}

// spec: IMP-002
#[test]
fn imp_002_an_event_dragged_onto_a_folder_or_a_note_opens_the_import_dialog_there() {
    let mut w = W::new("cal-zone");
    let nowhere = w.nowhere();
    let sub = w.zone("Sub", None, true).center();
    w.drag_event(dragged_event("Team/Plan"), nowhere, sub);
    // while over the folder: the hint names the event and the target
    assert!(w.painted("Event: Team/Plan") && w.painted("into “Sub”"), "{:?}", w.hl.text);
    assert!(!w.dialog_open());
    w.hl.release(&mut w.app, sub);
    assert!(!w.dragging() && w.dialog_open());
    let d = w.dialog();
    assert_eq!((d.folder.as_str(), d.link_on, d.link_into.is_none()), ("Sub", false, true));
    // the stored event: the one dragged, its calendar and the .ics kept with the note
    let (ev, calendar, original) = w.dialog_event();
    assert_eq!((ev.uid.as_str(), ev.summary.as_str(), ev.start, ev.end, ev.recurrence_id), ("drag-1", "Team/Plan", 1_800_000_000, 1_800_003_600, Some(1_800_000_000)));
    assert_eq!(calendar, "Work");
    let (name, data) = original.expect("the calendar file");
    assert_eq!(name, "Team-Plan.ics", "a slash in the title can't be in a file name");
    let parsed = crate::calendar::ics::parse_ics(&String::from_utf8(data).unwrap());
    assert_eq!((parsed.len(), parsed[0].uid.as_str(), parsed[0].summary.as_str(), parsed[0].location.as_str()), (1, "drag-1", "Team/Plan", "Room 4"));
    assert_eq!((parsed[0].start, parsed[0].end, parsed[0].rrule.as_deref()), (1_800_000_000, 1_800_003_600, Some("FREQ=DAILY")));
    w.close_dialog();
    assert!(!w.painted("Event: Team/Plan"), "the hint goes away with the drag");
    // on a note: its folder, and links added to that note
    let note = w.zone("", Some("Welcome.md"), true).center();
    w.drag_event(dragged_event("Standup"), nowhere, note);
    assert!(w.painted("Event: Standup") && w.painted("linked from “Welcome”"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, note);
    let d = w.dialog();
    assert_eq!((d.folder.as_str(), d.link_on, d.link_into.as_deref()), ("", true, Some(Path::new("Welcome.md"))));
    assert_eq!(w.dialog_event().2.unwrap().0, "Standup.ics");
}

// spec: IMP-002
#[test]
fn imp_002_an_event_dragged_onto_the_editor_or_nowhere_and_one_without_a_title() {
    let mut w = W::new("cal-editor");
    let nowhere = w.nowhere();
    // the editor of the open note links the new note from it
    let editor = w.zone("", Some("Welcome.md"), false).center();
    w.drag_event(dragged_event("Review"), nowhere, editor);
    w.hl.release(&mut w.app, editor);
    let d = w.dialog();
    assert_eq!((d.link_on, d.link_into.as_deref()), (true, Some(Path::new("Welcome.md"))));
    w.close_dialog();
    // nowhere in particular, no title: the folder used last, "(no title)" and event.ics
    w.app.vdata.state.last_import_folder = "Sub".into();
    w.drag_event(dragged_event(""), editor, nowhere);
    assert!(w.painted("Event: (no title)") && w.painted("into the folder used last"), "{:?}", w.hl.text);
    w.hl.release(&mut w.app, nowhere);
    let d = w.dialog();
    // the open note is offered for links, but not switched on
    assert_eq!((d.folder.as_str(), d.link_on, d.link_into.as_deref()), ("Sub", false, Some(Path::new("Welcome.md"))));
    let (ev, _, original) = w.dialog_event();
    assert_eq!(ev.summary, "");
    assert_eq!(original.unwrap().0, "event.ics");
    assert!(w.dialog().items[0].title.ends_with("Event"), "{}", w.dialog().items[0].title);
}

// spec: IMP-002
#[test]
fn imp_002_an_event_dropped_back_on_the_calendar_is_not_imported() {
    let mut w = W::new("cal-back");
    w.app.show_view(crate::app::View::Calendar, false);
    w.frames(4);
    let area = *w.app.drag.calendar_areas.first().expect("the calendar draws its area");
    let inside = area.center();
    let sub = w.zone("Sub", None, true).center();
    w.drag_event(dragged_event("Review"), sub, inside);
    assert!(w.painted("drag it onto a note, a folder or the editor to import it"), "{:?}", w.hl.text);
    assert!(!w.painted("into “Sub”"));
    w.hl.release(&mut w.app, inside);
    assert!(!w.dragging(), "the drag ends");
    assert!(!w.dialog_open(), "nothing is imported");
    assert!(w.app.toasts.is_empty(), "{:?}", w.app.toasts);
    assert!(!w.painted("drag it onto a note"));
}

// spec: IMP-002
#[test]
fn imp_002_a_second_event_goes_into_the_open_dialog_and_importing_makes_the_note() {
    let mut w = W::new("cal-more");
    let nowhere = w.nowhere();
    let sub = w.zone("Sub", None, true).center();
    w.drag_event(dragged_event("First"), nowhere, sub);
    w.hl.release(&mut w.app, sub);
    let mut second = dragged_event("Second");
    second.uid = "drag-2".into();
    let note = w.zone("", Some("Welcome.md"), true).center();
    w.drag_event(second, nowhere, note);
    w.hl.release(&mut w.app, note);
    // both are in the one dialog, and the place of the second one wins
    let d = w.dialog();
    assert_eq!(d.items.len(), 2);
    assert_eq!((d.folder.as_str(), d.link_on, d.link_into.as_deref()), ("", true, Some(Path::new("Welcome.md"))));
    // Import writes a note per event and links them from the note
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    let root = w.app.vault.as_ref().unwrap().root.clone();
    let mut notes: Vec<String> = std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with("First.md") || n.ends_with("Second.md")).collect();
    notes.sort();
    assert_eq!(notes.len(), 2, "{notes:?}");
    let first = std::fs::read_to_string(root.join(&notes[0])).unwrap();
    assert!(first.contains("drag-1") && first.contains("Room 4"), "{first}");
    // the note is open: the links are in its text (saved by the autosave later)
    let text = w.app.notes.get(Path::new("Welcome.md")).expect("the open note").text.clone();
    assert!(text.contains("First]]") && text.contains("Second]]"), "{text}");
}

// spec: IMP-002
#[test]
fn imp_002_an_event_drop_without_a_vault_says_so_and_without_a_pointer_does_nothing() {
    let mut w = W::new("cal-novault");
    // no pointer seen yet: the drag is not looked at
    eframe::egui::DragAndDrop::set_payload(w.hl.ctx(), crate::calendar::ui::CalDrag { event: dragged_event("Review"), calendar: "Work".into() });
    w.frames(2);
    assert!(w.dragging() && !w.dialog_open() && !w.painted("Event: Review"));
    eframe::egui::DragAndDrop::clear_payload(w.hl.ctx());
    // dropped with no vault open
    let nowhere = w.nowhere();
    w.app.vault = None;
    w.drag_event(dragged_event("Review"), nowhere, nowhere + eframe::egui::Vec2::new(5.0, 5.0));
    w.hl.release(&mut w.app, nowhere);
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("Open a vault first")), "{:?}", w.app.toasts);
    assert!(!w.dialog_open() && !w.dragging());
}

// spec: IMP-031
#[test]
fn imp_031_pictures_and_pdfs_dropped_on_a_note_are_saved_as_files_and_linked_from_it() {
    let mut w = W::new("media");
    std::fs::write(w.file("cat.png"), b"meow").unwrap();
    std::fs::write(w.file("plan.pdf"), b"%PDF-1.4").unwrap();
    let at = w.zone("", Some("Welcome.md"), true).center();
    w.drop_files(at, &[w.file("cat.png"), w.file("plan.pdf"), w.file("first drop.md")]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let d = w.dialog();
    assert_eq!(d.items.iter().map(|i| i.kind().to_string()).collect::<Vec<_>>(), vec!["Image", "PDF", "Markdown note"], "{:?}", d.notes);
    assert!(d.link_on);
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    d.items.truncate(2);
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    let root = w.app.vault.as_ref().unwrap().root.clone();
    let (y, m, day) = crate::media::local_date();
    let dir = root.join(format!("z_media/{y:04}/{m:02}"));
    let stamp = format!("{:02}{m:02}{day:02}", y.rem_euclid(100));
    assert_eq!(std::fs::read(dir.join(format!("{stamp}_cat.png"))).unwrap(), b"meow");
    assert!(dir.join(format!("{stamp}_plan.pdf")).exists());
    // no note was made, the open note got the links
    let text = w.app.notes.get(Path::new("Welcome.md")).expect("the open note").text.clone();
    assert!(text.contains(&format!("- ![[z_media/{y:04}/{m:02}/{stamp}_cat.png]]\n")) && text.contains(&format!("- [[z_media/{y:04}/{m:02}/{stamp}_plan.pdf]]\n")), "{text}");
    assert!(w.app.toasts.iter().any(|(t, _)| t == "Saved 2 files to the media folder"), "{:?}", w.app.toasts);
    assert!(!root.join("cat.md").exists() && std::fs::read_dir(&root).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().contains("Files")));
}

// spec: IMP-035, IMP-036
#[cfg(unix)]
#[test]
fn imp_035_a_word_file_is_converted_in_a_job_while_the_dialog_waits_and_then_becomes_a_note() {
    let mut w = W::new("docx");
    // pandoc ends when the test has looked at the waiting dialog (a sleep would race the frames)
    let gate = w.out.join("pandoc-may-end");
    let fake = crate::import::docx_tests::fake_pandoc(
        &w.out,
        "pandoc",
        &format!("while [ $# -gt 0 ]; do case \"$1\" in -o) out=\"$2\"; shift;; esac; shift; done; n=0; while [ ! -e '{}' ] && [ $n -lt 3000 ]; do sleep 0.01; n=$((n+1)); done; printf '# From Word\\n' > \"$out\"", gate.display()),
    );
    let docx = w.file("Quarterly Plan.docx");
    std::fs::write(&docx, b"PK").unwrap();
    // off: an attachment, as before
    let nowhere = w.nowhere();
    w.drop_files(nowhere, std::slice::from_ref(&docx));
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!(w.dialog().items.iter().map(|i| i.kind().to_string()).collect::<Vec<_>>(), vec!["Files"]);
    w.close_dialog();
    // on, with the pandoc of the settings
    w.app.cfg.docx_import = true;
    w.app.cfg.pandoc_path = fake.to_string_lossy().into_owned();
    w.drop_files(nowhere, std::slice::from_ref(&docx));
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!(w.dialog().items.iter().map(|i| i.kind().to_string()).collect::<Vec<_>>(), vec!["Word document"]);
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    d.folder = "Sub".into();
    assert!(!w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import), "the dialog stays while pandoc works");
    assert!(d.converting.is_some());
    w.app.modal = Some(Modal::Import(d));
    w.frames(2);
    assert!(w.dialog_open() && w.painted("Converting the Word files with pandoc"), "the window is not blocked: it says what is going on");
    std::fs::write(&gate, b"").unwrap();
    w.until("the note", |a| a.modal.is_none());
    let root = w.app.vault.as_ref().unwrap().root.clone();
    let note = std::fs::read_to_string(root.join("Sub/Quarterly Plan.md")).unwrap();
    let (y, m, _) = crate::media::local_date();
    assert_eq!(note, format!("---\nsource: \"[[z_media/{y:04}/{m:02}/import_docx_Quarterly-Plan/Quarterly Plan.docx]]\"\n---\n\n# From Word\n"));
    assert!(root.join(format!("z_media/{y:04}/{m:02}/import_docx_Quarterly-Plan/Quarterly Plan.docx")).is_file());
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("Imported 1 note into “Sub”")), "{:?}", w.app.toasts);
    // pandoc missing while the box is ticked: the item says so and the file is an attachment
    w.app.cfg.pandoc_path = w.out.join("gone").to_string_lossy().into_owned();
    w.drop_files(nowhere, std::slice::from_ref(&docx));
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!(w.dialog().items[0].warning().as_deref(), Some("pandoc was not found (Settings → Import). The file is saved as an attachment."));
    w.frames(2);
    assert!(w.painted("pandoc was not found (Settings → Import)"));
}

// spec: IMP-052, CFG-060
#[test]
fn imp_052_the_dialog_starts_with_the_setting_and_a_ticked_box_writes_the_list() {
    let mut w = W::new("header-list");
    std::fs::write(w.file("Plan.eml"), crate::mail::tests::sample_eml()).unwrap();
    let nowhere = w.nowhere();
    // off: the box is empty and the note has the properties only
    w.drop_files(nowhere, &[w.file("Plan.eml")]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert!(!w.dialog().header_list);
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    d.folder = "Sub".into();
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    let root = w.app.vault.as_ref().unwrap().root.clone();
    let name = "Sub/2026-11-13 Neubau Hörsaal B – Pläne Stand 3.md";
    let plain = std::fs::read_to_string(root.join(name)).unwrap();
    assert!(plain.contains("from: ") && !plain.contains("**From:**"), "{plain}");
    std::fs::remove_file(root.join(name)).unwrap();
    // the setting on: the box starts ticked, and the list is written
    w.app.cfg.import_header_list = true;
    w.drop_files(nowhere, &[w.file("Plan.eml")]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert!(w.dialog().header_list);
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    d.folder = "Sub".into();
    d.items.iter_mut().for_each(|i| i.include = true);
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    let listed = std::fs::read_to_string(root.join(name)).unwrap();
    assert!(listed.contains("\n- **From:** ") && listed.contains("\n- **Date:** 2026-11-13 "), "{listed}");
}

/// The titles of the tabs that are open, in order.
fn tab_names(w: &W) -> Vec<String> {
    w.app.tabbar.tabs.iter().map(|t| t.view.path().map(|p| p.file_stem().unwrap().to_string_lossy().into_owned()).unwrap_or_default()).collect()
}

/// Import the two Markdown files of the fixture and take the summary window.
fn import_two(w: &mut W) -> Vec<String> {
    let nowhere = w.nowhere();
    w.drop_files(nowhere, &[w.file("first drop.md"), w.file("second.md")]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    d.folder = "Sub".into();
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    w.frames(4);
    tab_names(w)
}

// spec: IMP-056
#[test]
fn imp_056_importing_two_items_shows_a_summary_with_open_buttons() {
    use eframe::egui::PointerButton;
    let mut w = W::new("summary");
    let before = import_two(&mut w);
    assert_eq!(before, vec!["Welcome"], "nothing is opened yet");
    let Some(Modal::ImportSummary(s)) = &w.app.modal else { panic!("no summary") };
    assert_eq!(s.title(), "Imported 2 notes");
    assert_eq!(s.notes.iter().map(|e| (e.name.as_str(), e.line.as_str())).collect::<Vec<_>>(), vec![("first drop", "from outside"), ("second", "")]);
    assert!(w.painted("Imported 2 notes") && w.painted("first drop") && w.painted("from outside") && w.painted("Open all in tabs"), "{:?}", w.hl.text);
    // Open: that note in a tab, the window stays
    let open = w.hl.find_label(&mut w.app, "Open");
    w.hl.click(&mut w.app, open.center(), PointerButton::Primary);
    assert_eq!(tab_names(&w), vec!["Welcome", "first drop"]);
    assert!(matches!(w.app.modal, Some(Modal::ImportSummary(_))), "still there");
    // Open all in tabs: each note in a tab of its own, the window closes
    let all = w.hl.find_label(&mut w.app, "Open all in tabs");
    w.hl.click(&mut w.app, all.center(), PointerButton::Primary);
    assert!(w.app.modal.is_none());
    let tabs = tab_names(&w);
    assert!(tabs.contains(&"first drop".to_string()) && tabs.contains(&"second".to_string()), "{tabs:?}");
    // Close just closes
    let mut w = W::new("summary-close");
    import_two(&mut w);
    let close = w.hl.find_label(&mut w.app, "Close");
    w.hl.click(&mut w.app, close.center(), PointerButton::Primary);
    assert!(w.app.modal.is_none());
    assert_eq!(tab_names(&w), vec!["Welcome"]);
}

// spec: IMP-056
#[test]
fn imp_056_one_result_opens_as_before_and_errors_of_several_are_in_the_summary() {
    let mut w = W::new("summary-errors");
    let nowhere = w.nowhere();
    // a file that is there when it is dropped and gone when the import runs
    let missing = w.file("gone.txt");
    std::fs::write(&missing, "x").unwrap();
    // one note and one failure: the note opens, the failure is a toast
    w.drop_files(nowhere, &[w.file("first drop.md"), missing.clone()]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    std::fs::remove_file(&missing).unwrap();
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    w.frames(4);
    assert!(w.app.modal.is_none());
    assert_eq!(tab_names(&w).last().map(String::as_str), Some("first drop"));
    assert!(w.app.toasts.iter().any(|(t, _)| t.contains("gone")), "{:?}", w.app.toasts);
    // two notes and one failure: the summary lists all three, the failure is no toast
    w.app.toasts.clear();
    std::fs::write(&missing, "x").unwrap();
    w.drop_files(nowhere, &[w.file("second.md"), w.file("first drop.md"), missing.clone()]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let Some(Modal::Import(mut d)) = w.app.modal.take() else { panic!("no dialog") };
    std::fs::remove_file(&missing).unwrap();
    d.items.iter_mut().for_each(|i| i.include = true);
    assert!(w.app.import_action(&mut d, crate::import::dialog::ImportAction::Import));
    w.frames(4);
    let Some(Modal::ImportSummary(s)) = &w.app.modal else { panic!("no summary") };
    assert_eq!(s.notes.len(), 2);
    assert_eq!(s.errors.len(), 1, "{:?}", s.errors);
    assert!(s.errors[0].contains("gone"), "{:?}", s.errors);
    assert!(!w.app.toasts.iter().any(|(t, _)| t.contains("gone")), "{:?}", w.app.toasts);
    assert!(w.painted("Went wrong") && w.painted(&s.errors[0]), "{:?}", w.hl.text);
}
