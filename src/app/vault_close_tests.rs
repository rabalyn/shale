//! Closing a vault and switching to another (VAULT-035, VAULT-036, VAULT-037): what is dropped, what
//! is saved first, and which files Close removes. The dropdown that starts them is in
//! `vault_menu_tests.rs`.

use super::vault_close::{remove_shale_files, Remove};
use super::{Options, ShaleApp};
use std::path::{Path, PathBuf};

fn base(tag: &str) -> PathBuf {
    let b = crate::known_vaults::canonical(&std::env::temp_dir()).join(format!("shale-vault-close-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&b);
    b
}

fn app() -> ShaleApp {
    ShaleApp::build(Options { screenshot: None, vault: None, view: None, keys: None }, false)
}

/// A vault folder with two linked notes.
fn make_vault(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("Welcome.md"), format!("# {name}\n\n- [ ] task of {name}\n\nSee [[Other]].\n")).unwrap();
    std::fs::write(dir.join("Other.md"), format!("# Other of {name}\n\nBack to [[Welcome]].\n")).unwrap();
}

/// Put something of every kind of per-vault state into the window.
fn fill(app: &mut ShaleApp) {
    let rel = PathBuf::from("Other.md");
    app.open(&rel, true);
    app.add_pane_right();
    app.open(Path::new("Welcome.md"), false);
    assert!(app.panes.len() == 2 && app.tabbar.tabs.len() == 1 && app.notes.len() == 2, "the window shows two panes");
    app.search.query = "needle".into();
    app.tasks.query = "task".into();
    app.tasks.show_done = true;
    app.tree.expanded.insert(PathBuf::from("Folder"));
    app.tree.sel = Some(rel.clone());
    let v = app.vault.as_ref().unwrap();
    assert!(!app.vault_cache.backlinks(v, Path::new("Welcome.md")).is_empty());
    app.thread.index(v);
    app.status.counts(&rel, "some text");
    app.undo.of(&rel);
    app.trashed.push(PathBuf::from(".trash/x.md"));
    app.history.sched.saved(&rel, 1);
    app.sync.last = Some(("10:00".into(), Ok("done".into())));
    app.sync.url = "https://example.org/repo.git".into();
    app.sync.remote = Some("https://example.org/repo.git".into());
    app.sync.last_start = Some(std::time::Instant::now());
    app.sync.repo_ask = Some(super::git_ask::RepoAsk { root: PathBuf::from("/old"), later: false });
    app.usage.places.vault = Some(PathBuf::from("/old"));
    app.graphs.global.show_missing = false;
    let ctx = eframe::egui::Context::default();
    app.rescan.due_now();
    app.rescan_tick(&ctx);
    assert!(app.rescan.in_flight(), "a scan of the old vault runs");
}

/// Nothing of the old vault is left: what a window that has just been built shows, too.
fn assert_nothing_of_the_old_vault(app: &mut ShaleApp) {
    assert!(app.tabbar.tabs.is_empty() || app.tabbar.tabs.iter().all(|t| !matches!(&t.view, super::View::Note(p) if p == Path::new("Other.md"))), "no tab of the old vault");
    assert_eq!(app.panes.len(), 1, "one pane");
    assert!(app.notes.is_empty() && app.drawings.is_empty(), "no buffer");
    assert_eq!((app.search.query.as_str(), app.search.listed().len(), app.search.sel), ("", 0, None), "no search");
    assert_eq!((app.tasks.query.as_str(), app.tasks.show_done), ("", false), "no task filter");
    assert!(app.tree.expanded.is_empty() && app.tree.sel.is_none(), "no folders open in the file list");
    assert!(!app.vault_cache.holds_anything(), "no backlinks, mentions or tasks");
    assert!(!app.thread.has_index(), "no thread index");
    assert_eq!(app.undo.loaded(), 0, "no undo");
    assert!(app.trashed.is_empty(), "nothing to restore from the old trash");
    assert!(app.history.sched.take_all().is_empty(), "no snapshot waiting");
    assert!(app.sync.last.is_none() && app.sync.url.is_empty() && app.sync.remote.is_none() && app.sync.last_start.is_none() && app.sync.repo_ask.is_none(), "no sync state");
    assert!(!app.sync.running() && !app.sync.lfs_job_running(), "no sync job");
    assert!(app.graphs.global.show_missing, "the graph is as new");
    assert!(app.usage.places.vault.is_none(), "the data usage does not look at the old vault");
    assert!(!app.rescan.in_flight(), "the scan of the old vault is dropped");
}

// spec: VAULT-037
#[test]
fn vault_037_switching_to_another_vault_shows_nothing_of_the_first() {
    let b = base("switch");
    let (a, c) = (b.join("a"), b.join("c"));
    make_vault(&a, "a");
    make_vault(&c, "c");
    let mut app = app();
    app.open_vault(&a);
    fill(&mut app);
    // a dirty buffer is saved by the switch, not dropped
    app.notes.get_mut(Path::new("Welcome.md")).unwrap().text = "# a\n\nedited\n".into();
    app.notes.get_mut(Path::new("Welcome.md")).unwrap().dirty = true;
    app.open_vault(&c);
    assert_eq!(std::fs::read_to_string(a.join("Welcome.md")).unwrap(), "# a\n\nedited\n", "the edit of the old vault was saved first");
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(c.clone()));
    // the new vault shows its own first note and nothing else
    assert_eq!(app.tabbar.tabs.len(), 1);
    assert_eq!(app.notes.keys().cloned().collect::<Vec<_>>(), [PathBuf::from("Welcome.md")]);
    assert!(app.notes[Path::new("Welcome.md")].text.contains("# c"));
    assert_eq!(app.panes.len(), 1);
    assert_eq!((app.search.query.as_str(), app.tasks.query.as_str()), ("", ""));
    assert!(app.tree.expanded.is_empty() && app.trashed.is_empty());
    assert!(!app.vault_cache.holds_anything() && !app.thread.has_index());
    assert_eq!(app.undo.loaded(), 0);
    assert!(app.sync.last.is_none() && app.sync.repo_ask.is_none() && app.sync.url.is_empty());
    assert!(!app.rescan.in_flight());
    assert!(app.graphs.global.show_missing);
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-037
#[test]
fn vault_037_dropping_the_state_of_a_vault_is_one_function() {
    let b = base("drop");
    make_vault(&b.join("a"), "a");
    let mut app = app();
    app.open_vault(&b.join("a"));
    fill(&mut app);
    app.modal = Some(super::Modal::TexSetup);
    app.drop_vault_state();
    assert_nothing_of_the_old_vault(&mut app);
    assert!(app.modal.is_none(), "no dialog of the old vault");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-037
#[test]
fn vault_037_a_switch_waits_for_a_running_sync_and_takes_in_its_result() {
    let b = base("sync-wait");
    make_vault(&b.join("a"), "a");
    let mut app = app();
    app.open_vault(&b.join("a"));
    // no sync: nothing to wait for
    assert!(app.wait_for_sync(std::time::Duration::from_millis(1)));
    app.sync_test_job(std::time::Duration::from_millis(150), true);
    assert!(app.sync.running());
    assert!(app.prepare_vault_switch(), "the switch goes on when the sync has ended");
    assert!(!app.sync.running(), "the sync ended before the vault was left");
    assert!(matches!(&app.sync.last, Some((_, Ok(_)))), "its result was taken in: {:?}", app.sync.last);
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-037
#[test]
fn vault_037_a_sync_that_does_not_end_keeps_the_vault_open() {
    let b = base("sync-stuck");
    make_vault(&b.join("a"), "a");
    let mut app = app();
    app.open_vault(&b.join("a"));
    app.sync_test_job(std::time::Duration::from_millis(600), true);
    assert!(!app.wait_for_sync(std::time::Duration::from_millis(40)), "gives up after the limit");
    assert!(app.toasts.iter().any(|(m, _)| m.contains("sync is still running")));
    assert!(app.vault.is_some(), "nothing was dropped");
    assert!(app.sync.running());
    // it ends in the end, and its result is taken in
    assert!(app.wait_for_sync(std::time::Duration::from_secs(5)));
    assert!(!app.sync.running() && matches!(&app.sync.last, Some((_, Ok(_)))));
    // a sync that dies counts as an error of the sync, not as a reason to stay
    app.sync_test_job(std::time::Duration::from_millis(1), false);
    assert!(app.wait_for_sync(std::time::Duration::from_secs(5)));
    assert!(matches!(&app.sync.last, Some((_, Err(_)))), "{:?}", app.sync.last);
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-036
#[test]
fn vault_036_closing_the_open_vault_saves_the_buffers_forgets_it_and_leaves_no_vault() {
    let b = base("close-open");
    let (a, c) = (b.join("a"), b.join("c"));
    make_vault(&a, "a");
    make_vault(&c, "c");
    std::fs::create_dir_all(a.join(".trash")).unwrap();
    std::fs::write(a.join(".trash/Old.md"), "old").unwrap();
    let mut app = app();
    app.open_vault(&c);
    app.open_vault(&a);
    fill(&mut app);
    app.notes.get_mut(Path::new("Other.md")).unwrap().text = "# changed\n".into();
    app.notes.get_mut(Path::new("Other.md")).unwrap().dirty = true;
    app.close_vault(&a, Remove::default());
    assert_eq!(std::fs::read_to_string(a.join("Other.md")).unwrap(), "# changed\n", "the buffer was saved first");
    assert!(app.vault.is_none(), "no vault is open");
    assert_nothing_of_the_old_vault(&mut app);
    assert_eq!(app.cfg.vaults.entries().iter().map(|e| e.path.clone()).collect::<Vec<_>>(), [c], "the closed vault is forgotten, the other stays");
    assert!(app.cfg.vault.is_none(), "it is not the vault to open at the next start");
    assert!(matches!(&app.modal, Some(super::Modal::Vaults(m)) if matches!(&**m, crate::vault_menu::VaultModal::Menu { .. })), "the list is open on the start screen");
    assert!(a.join("Welcome.md").is_file() && a.join("Other.md").is_file(), "the notes stay");
    assert!(a.join(".trash/Old.md").is_file(), "nothing is removed by default");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-036
#[cfg(unix)]
#[test]
fn vault_036_a_buffer_that_cannot_be_saved_keeps_the_vault_open() {
    let b = base("close-unsaved");
    let a = b.join("a");
    make_vault(&a, "a");
    let mut app = app();
    app.open_vault(&a);
    app.notes.get_mut(Path::new("Welcome.md")).unwrap().text = "# lost\n".into();
    app.notes.get_mut(Path::new("Welcome.md")).unwrap().dirty = true;
    // the folder takes no new file: the save fails
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o555)).unwrap();
    app.close_vault(&a, Remove::default());
    std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(app.vault.is_some(), "the vault stays open, the edit is not lost");
    assert!(app.toasts.iter().any(|(m, _)| m.contains("could not be saved")), "{:?}", app.toasts);
    assert_eq!(app.cfg.vaults.entries().len(), 1, "it is still known");
    assert_eq!(app.notes[Path::new("Welcome.md")].text, "# lost\n", "the edit is still in the buffer");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-035
#[test]
fn vault_035_closing_a_vault_that_is_not_open_keeps_the_open_one_and_forgets_the_other() {
    let b = base("close-other");
    let (a, c) = (b.join("a"), b.join("c"));
    make_vault(&a, "a");
    make_vault(&c, "c");
    let mut app = app();
    app.open_vault(&c);
    app.open_vault(&a);
    app.close_vault(&c, Remove::default());
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(a.clone()), "the open vault stays open");
    assert_eq!(app.cfg.vaults.entries().iter().map(|e| e.path.clone()).collect::<Vec<_>>(), std::slice::from_ref(&a));
    assert_eq!(app.cfg.vault, Some(a.clone()));
    assert!(app.modal.is_none(), "no list: the window stays as it is");
    assert!(c.join("Welcome.md").is_file());
    let _ = std::fs::remove_dir_all(&b);
}

fn shale_folders(dir: &Path) {
    for (d, f) in [(".shale", "state.json"), (".trash", "2026-11-23/Old.md"), (".git", "HEAD")] {
        let p = dir.join(d).join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, "x").unwrap();
    }
    std::fs::create_dir_all(dir.join("Attachments")).unwrap();
    std::fs::write(dir.join("Attachments/pic.png"), "x").unwrap();
}

// spec: VAULT-035
#[test]
fn vault_035_nothing_is_removed_unless_asked_and_each_folder_is_asked_for_alone() {
    let b = base("remove");
    make_vault(&b, "v");
    shale_folders(&b);
    assert!(remove_shale_files(&b, Remove::default()).is_empty());
    assert!(b.join(".shale").is_dir() && b.join(".trash").is_dir(), "both are off by default: nothing is removed");
    assert!(remove_shale_files(&b, Remove { shale: true, trash: false }).is_empty());
    assert!(!b.join(".shale").exists() && b.join(".trash/2026-11-23/Old.md").is_file(), "only .shale/ went");
    shale_folders(&b);
    assert!(remove_shale_files(&b, Remove { shale: false, trash: true }).is_empty());
    assert!(b.join(".shale").is_dir() && !b.join(".trash").exists(), "only .trash/ went");
    shale_folders(&b);
    assert!(remove_shale_files(&b, Remove { shale: true, trash: true }).is_empty());
    assert!(!b.join(".shale").exists() && !b.join(".trash").exists());
    // notes, attachments and the repository are never touched
    for kept in ["Welcome.md", "Other.md", "Attachments/pic.png", ".git/HEAD"] {
        assert!(b.join(kept).is_file(), "{kept} stays");
    }
    // a folder that has none of them is not an error
    assert!(remove_shale_files(&b, Remove { shale: true, trash: true }).is_empty());
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-035
#[cfg(unix)]
#[test]
fn vault_035_a_link_named_like_a_shale_folder_is_removed_as_a_link_and_not_followed() {
    let b = base("remove-link");
    make_vault(&b.join("v"), "v");
    let outside = b.join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("precious.md"), "keep").unwrap();
    std::os::unix::fs::symlink(&outside, b.join("v/.shale")).unwrap();
    std::fs::write(b.join("v/.trash"), "a file by that name").unwrap();
    assert!(remove_shale_files(&b.join("v"), Remove { shale: true, trash: true }).is_empty());
    assert!(std::fs::symlink_metadata(b.join("v/.shale")).is_err() && !b.join("v/.trash").exists(), "both are gone");
    assert!(outside.join("precious.md").is_file(), "what the link pointed to is untouched");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-035
#[cfg(unix)]
#[test]
fn vault_035_a_folder_that_cannot_be_removed_is_reported() {
    let b = base("remove-fail");
    make_vault(&b, "v");
    // a folder that is not there is fine; one the process cannot reach is reported
    let missing = b.join("not-a-folder-at-all/v");
    assert!(remove_shale_files(&missing, Remove { shale: true, trash: true }).is_empty());
    let file_as_root = b.join("Welcome.md");
    let problems = remove_shale_files(&file_as_root, Remove { shale: true, trash: false });
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains(".shale/"), "{problems:?}");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-035
#[test]
fn vault_035_close_removes_the_chosen_folders_after_the_state_was_saved() {
    let b = base("close-remove");
    let a = b.join("a");
    make_vault(&a, "a");
    shale_folders(&a);
    let mut app = app();
    app.open_vault(&a);
    app.close_vault(&a, Remove { shale: true, trash: true });
    assert!(app.vault.is_none());
    assert!(!a.join(".shale").exists() && !a.join(".trash").exists(), "the folders are gone and were not written again");
    assert!(a.join(".git/HEAD").is_file() && a.join("Welcome.md").is_file());
    let _ = std::fs::remove_dir_all(&b);
}
