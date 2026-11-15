//! Drops from other programs and from the window system, in the headless whole window
//! (DEV-012): the events `wl_drop` would send (or the `drop:` step makes) are fed to
//! `handle_drops` without a compositor.

use super::{Modal, Options, ShaleApp};
use crate::ui_test_support::Headless;
use crate::wl_drop::DropEvent;
use eframe::egui::{Pos2, Rect};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

struct W {
    app: ShaleApp,
    hl: Headless,
    /// files to drop, outside the vault
    out: PathBuf,
}

impl W {
    fn new(tag: &str) -> W {
        let base = std::env::temp_dir().join(format!("shale-drops-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (dir, out) = (base.join("vault"), base.join("outside"));
        std::fs::create_dir_all(dir.join("Sub")).unwrap();
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(dir.join("Welcome.md"), "# Welcome\n\nHello\n").unwrap();
        std::fs::write(dir.join("Sub/Inner.md"), "# Inner\n").unwrap();
        std::fs::write(out.join("first drop.md"), "# First\n\nfrom outside\n").unwrap();
        std::fs::write(out.join("second.md"), "# Second\n").unwrap();
        let app = ShaleApp::build(Options { screenshot: None, vault: Some(dir), view: None, keys: None }, false);
        let mut w = W { app, hl: Headless::new(), out };
        w.frames(6);
        w
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.hl.frame(&mut self.app, vec![]);
        }
    }

    fn file(&self, name: &str) -> PathBuf {
        self.out.join(name)
    }

    /// Run frames until `f` holds (the drop is read in the background).
    fn until(&mut self, what: &str, f: impl Fn(&ShaleApp) -> bool) {
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
    fn zone(&self, folder: &str, note: Option<&str>, smallest: bool) -> Rect {
        let mut zs: Vec<Rect> = self.app.drop_zones.iter().filter(|z| z.folder == Path::new(folder) && z.note.as_deref() == note.map(Path::new)).map(|z| z.rect).collect();
        zs.sort_by(|a, b| a.area().total_cmp(&b.area()));
        let z = if smallest { zs.first() } else { zs.last() };
        *z.unwrap_or_else(|| panic!("no drop zone for {folder:?} {note:?}"))
    }

    fn dialog(&self) -> &crate::import_dialog::ImportDialog {
        match &self.app.modal {
            Some(Modal::Import(d)) => d,
            _ => panic!("the Import dialog is not open"),
        }
    }

    fn dialog_open(&self) -> bool {
        matches!(self.app.modal, Some(Modal::Import(_)))
    }

    /// Another program drops `paths` at `at` (what `wl_drop` sends for a file list).
    fn drop_files(&mut self, at: Pos2, paths: &[PathBuf]) {
        let list: String = paths.iter().map(|p| format!("{}\r\n", crate::import::file_uri(p))).collect();
        self.app.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![("text/uri-list".into(), list.into_bytes())], offered: vec!["text/uri-list".into()], report: vec![] });
    }

    fn drop_data(&mut self, at: Pos2, mime: &str, data: &[u8]) {
        self.app.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![(mime.into(), data.to_vec())], offered: vec![mime.into()], report: vec![] });
        self.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    }

    fn close_dialog(&mut self) {
        self.app.modal = None;
        self.frames(2);
    }

    fn painted(&self, text: &str) -> bool {
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
    let nowhere = (0..90).flat_map(|y| (0..150).map(move |x| Pos2::new(x as f32 * 10.0 + 3.0, y as f32 * 10.0 + 3.0))).find(|p| !w.app.drop_zones.iter().any(|z| z.rect.contains(*p))).expect("a place outside every zone");
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

// spec: IMP-014
#[test]
fn imp_014_hover_refusal_and_busy_show_and_go_away() {
    let mut w = W::new("hint");
    let at = w.zone("Sub", None, true).center();
    w.app.fake_drops.push(DropEvent::Hover { x: at.x, y: at.y });
    w.frames(2);
    assert_eq!(w.app.ext_hover, Some(at));
    assert!(w.painted("Drop to import") && w.painted("into “Sub”"), "{:?}", w.hl.text);
    w.app.fake_drops.push(DropEvent::Leave);
    w.frames(2);
    assert!(w.app.ext_hover.is_none() && !w.painted("Drop to import"));
    // a browser tab: said at the pointer, until it leaves
    let offered: Vec<String> = ["application/x-tab", "a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
    w.app.fake_drops.push(DropEvent::Refused { x: at.x, y: at.y, offered });
    w.frames(2);
    assert!(w.app.ext_hover.is_none() && w.app.ext_refused.is_some());
    assert!(w.painted("Shale can't take this") && w.painted("application/x-tab, a, b, c, …"), "{:?}", w.hl.text);
    w.app.fake_drops.push(DropEvent::Leave);
    w.frames(2);
    assert!(w.app.ext_refused.is_none() && !w.painted("Shale can't take this"));
    // reading a drop: said until the items are there
    w.app.fake_drops.push(DropEvent::Busy);
    w.frames(2);
    assert!(w.app.drop_busy && w.painted("Reading the dropped message"));
    w.drop_files(at, &[w.file("second.md")]);
    w.until("the Import dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert!(!w.app.drop_busy && w.app.pending_drops.is_empty());
}

// spec: IMP-015
#[test]
fn imp_015_files_dropped_through_the_window_system_open_the_import_dialog() {
    let mut w = W::new("x11");
    let p = w.file("first drop.md");
    w.hl.frame_with(&mut w.app, vec![], |_, raw| raw.dropped_files.push(std::sync::Arc::new(super::ScriptDrop(p))));
    assert!(w.dialog_open());
    let d = w.dialog();
    assert_eq!((d.items.len(), d.link_on), (1, false), "{:?}", d.notes);
    // the same through the script step `drop:/path`, and at a place (`drop:T:/path`)
    w.close_dialog();
    let steps = crate::script::parse(&format!("drop:{}", w.file("second.md").display())).unwrap();
    w.app.script = steps.into();
    w.app.frames = 36;
    w.hl.frame_with(&mut w.app, vec![], |app, raw| app.feed_script(raw));
    w.until("the dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!(w.dialog().items.len(), 1);
    w.close_dialog();
    let place = w.zone("Sub", None, true).center();
    let steps = crate::script::parse(&format!("drop:{},{}:{}", place.x, place.y, w.file("second.md").display())).unwrap();
    w.app.script = steps.into();
    w.app.frames = 42;
    w.hl.frame_with(&mut w.app, vec![], |app, raw| app.feed_script(raw));
    w.until("the dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    assert_eq!((w.dialog().folder.as_str(), w.dialog().items.len()), ("Sub", 1));
}

// spec: IMP-015
#[test]
fn imp_015_an_unusable_drop_says_what_was_offered() {
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
    w.app.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![("text/plain".into(), vec![])], offered: vec!["STRING".into(), "DELETE".into()], report: vec!["  text/plain: while dragging empty, after the drop empty".into()] });
    w.until("the dialog", |a| matches!(a.modal, Some(Modal::Import(_))));
    let notes = w.dialog().notes.join("\n");
    assert!(notes.contains("XWayland") && notes.contains("text/plain: while dragging empty"), "{notes}");
}

// spec: IMP-015
#[test]
fn imp_015_a_drop_without_a_vault_says_so() {
    let mut w = W::new("novault");
    w.app.vault = None;
    let at = Pos2::new(300.0, 300.0);
    w.app.fake_drops.push(DropEvent::Drop { x: at.x, y: at.y, parts: vec![("text/plain".into(), b"just text".to_vec())], offered: vec!["text/plain".into()], report: vec![] });
    w.until("the message", |a| a.toasts.iter().any(|(t, _)| t.contains("Open a vault first")));
    assert!(!w.dialog_open());
}

// spec: IMP-016
#[test]
fn imp_016_dragging_in_the_file_list_hints_and_moves_onto_a_folder() {
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
