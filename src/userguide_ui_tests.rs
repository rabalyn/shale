//! F1 in the whole window (headless) opens the user guide as the vault (DEV-020, KEY-013).

use crate::app::{Options, ShaleApp};
use crate::ui_test_support::Headless;
use crate::userguide::Places;
use eframe::egui::{Event, Key, Modifiers};
use std::path::PathBuf;

fn f1(hl: &mut Headless, app: &mut ShaleApp) {
    let ev = |pressed| Event::Key { key: Key::F1, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
    hl.frame(app, vec![ev(true)]);
    hl.frame(app, vec![ev(false)]);
    hl.frames(app, 2);
    // the guide is read in the background (NET-019)
    hl.settle(app);
}

fn window(tag: &str) -> (ShaleApp, Headless, PathBuf) {
    let dir = std::env::temp_dir().join(format!("shale-userguide-ui-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let vault = dir.join("vault");
    crate::vault::write_sample_vault(&vault).unwrap();
    let app = ShaleApp::build(Options { screenshot: None, vault: Some(vault), view: None, keys: None }, false);
    (app, Headless::new(), dir)
}

// spec: DEV-020, KEY-013
#[test]
fn dev_020_f1_copies_the_guide_once_and_opens_it_as_the_vault_at_its_start_note() {
    let (mut app, mut hl, dir) = window("f1");
    let root = dir.join("data");
    app.guide = Places { files: crate::userguide::tests::SAMPLE, root: Some(root.clone()) };
    hl.frames(&mut app, 3);
    f1(&mut hl, &mut app);
    let copy = root.join(format!("userguide-{}", env!("CARGO_PKG_VERSION")));
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(copy.clone()), "the copy is the open vault");
    assert!(copy.join("Other.md").is_file());
    let open: Vec<_> = app.tabbar.tabs.iter().filter_map(|t| t.view.path().map(|p| p.to_string_lossy().into_owned())).collect();
    assert_eq!(open, ["Start here.md"]);
    let _ = std::fs::remove_dir_all(&dir);
}

// spec: DEV-020
#[test]
fn dev_020_f1_without_a_data_folder_keeps_the_vault_and_says_why() {
    let (mut app, mut hl, dir) = window("none");
    app.guide = Places::none();
    hl.frames(&mut app, 3);
    let before = app.vault.as_ref().map(|v| v.root.clone());
    f1(&mut hl, &mut app);
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), before);
    assert!(app.toasts.iter().any(|(m, _)| m.contains("no data folder")), "{:?}", app.toasts);
    let _ = std::fs::remove_dir_all(&dir);
}
