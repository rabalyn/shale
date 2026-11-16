//! The markdown preview toggle in the header of the editor, driven in the whole window.

use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use eframe::egui::{self, Event};
use std::path::Path;

// spec: EDIT-016
#[test]
fn the_header_icon_flips_the_preview_and_typing_still_edits() {
    let dir = std::env::temp_dir().join(format!("shale-preview-ui-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::vault::write_sample_vault(&dir).unwrap();
    let mut app = ShaleApp::build(Options { screenshot: None, vault: Some(dir.clone()), view: None, keys: None }, false);
    app.open(Path::new("Welcome.md"), false);
    let mut hl = Headless::new();
    hl.frames(&mut app, 4);
    assert!(!app.cfg.live_preview, "off by default");

    let r = hl.find_label(&mut app, "Markdown preview");
    hl.click(&mut app, r.center(), egui::PointerButton::Primary);
    assert!(app.cfg.live_preview, "the icon turns the preview on");
    assert!(app.tabbar.tabs[app.tabbar.active].editor.live_preview, "and the editor of the tab follows");
    hl.find_label(&mut app, "Raw markdown");

    // the note stays editable
    app.tabbar.tabs[app.tabbar.active].editor.request_focus = true;
    hl.frames(&mut app, 3);
    let before = app.notes.get(Path::new("Welcome.md")).unwrap().text.clone();
    hl.frame(&mut app, vec![Event::Text("XYZ".into())]);
    hl.frames(&mut app, 2);
    let after = &app.notes.get(Path::new("Welcome.md")).unwrap().text;
    assert_eq!(after.len(), before.len() + 3, "typing in the preview edits the text");
    assert!(after.contains("XYZ"));

    let r = hl.find_label(&mut app, "Raw markdown");
    hl.click(&mut app, r.center(), egui::PointerButton::Primary);
    assert!(!app.cfg.live_preview, "the icon turns it off again");
    let _ = std::fs::remove_dir_all(&dir);
}
