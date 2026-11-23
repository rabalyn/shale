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
    app.tag_learner.forget();
    app.tag_learner.tick(app.vault.as_ref(), &app.busy);
    assert!(app.busy.is_running(&crate::import::tagwords::TAG_LEARNING) && app.busy.is_running(&super::rescan::VAULT_RESCAN), "the work of the old vault is listed");
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
    // the tasks of that work go with it (JOB-017): a new vault starts its own
    assert!(!app.busy.is_running(&super::rescan::VAULT_RESCAN) && !app.busy.is_running(&crate::import::tagwords::TAG_LEARNING), "no task of the old vault is listed");
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

// spec: VAULT-037, JOB-017
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

/// Two vaults, `a` open and `c` known, and a sync of `a` that runs for `ms`.
fn with_running_sync(tag: &str, ms: u64) -> (ShaleApp, PathBuf, PathBuf, PathBuf) {
    let b = base(tag);
    let (a, c) = (b.join("a"), b.join("c"));
    make_vault(&a, "a");
    make_vault(&c, "c");
    let mut app = app();
    app.open_vault(&c);
    app.open_vault(&a);
    app.sync_test_job(std::time::Duration::from_millis(ms), true);
    assert!(app.sync.running());
    (app, a, c, b)
}

/// Frames (the ticks of the window) until `done`.
fn frames_until(app: &mut ShaleApp, what: &str, done: impl Fn(&ShaleApp) -> bool) {
    let ctx = eframe::egui::Context::default();
    let t = std::time::Instant::now();
    while !done(app) {
        assert!(t.elapsed() < std::time::Duration::from_secs(20), "{what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.sync_tick(&ctx);
        app.vault_leave_tick();
        app.vault_load_tick(&ctx);
    }
}

// spec: VAULT-037
#[test]
fn vault_037_a_switch_during_a_sync_waits_without_blocking_and_completes_when_the_sync_ends() {
    let (mut app, a, c, b) = with_running_sync("sync-switch", 400);
    let t = std::time::Instant::now();
    app.open_vault_in_background(&c, super::vault_load::After::Nothing);
    assert!(t.elapsed() < std::time::Duration::from_millis(300), "the call does not wait for the sync");
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(a.clone()), "the vault stays open meanwhile");
    assert!(app.pending_leave.is_some() && !app.vault_load.is_loading());
    let task = app.busy.info(&super::vault_close::LEAVE_WAIT).expect("the wait is a task");
    assert_eq!((task.label.as_str(), task.cancellable), ("Waiting for the sync to finish…", true));
    // a second request while it waits changes nothing
    app.open_vault_in_background(&a, super::vault_load::After::Nothing);
    assert_eq!(app.busy.list().iter().filter(|t| t.label.starts_with("Waiting")).count(), 1);
    frames_until(&mut app, "the switch did not complete", |a| a.pending_leave.is_none() && !a.vault_load.is_loading() && a.vault.as_ref().is_some_and(|v| v.root == c));
    assert!(app.busy.info(&super::vault_close::LEAVE_WAIT).is_none(), "the task ended");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-036
#[test]
fn vault_036_closing_during_a_sync_waits_without_blocking_and_closes_when_the_sync_ends() {
    let (mut app, a, _c, b) = with_running_sync("sync-close", 300);
    let t = std::time::Instant::now();
    app.close_vault(&a, Remove::default());
    assert!(t.elapsed() < std::time::Duration::from_millis(200), "the call does not wait for the sync");
    assert!(app.vault.is_some() && app.pending_leave.is_some() && app.cfg.vaults.entries().len() == 2, "nothing happened yet");
    frames_until(&mut app, "the close did not complete", |a| a.vault.is_none());
    assert!(app.pending_leave.is_none());
    assert_eq!(app.cfg.vaults.entries().len(), 1, "forgotten");
    assert!(matches!(&app.modal, Some(super::Modal::Vaults(_))), "the list is shown");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-037
#[test]
fn vault_037_cancel_drops_the_wait_and_keeps_the_vault_open() {
    let (mut app, a, c, b) = with_running_sync("sync-cancel", 300);
    app.open_vault_in_background(&c, super::vault_load::After::Nothing);
    let id = app.busy.info(&super::vault_close::LEAVE_WAIT).unwrap().id;
    assert!(app.busy.cancel(id));
    app.vault_leave_tick();
    assert!(app.pending_leave.is_none(), "Cancel dropped it");
    assert!(app.busy.info(&super::vault_close::LEAVE_WAIT).is_none());
    frames_until(&mut app, "the sync did not end", |a| !a.sync.running());
    app.vault_leave_tick();
    assert!(!app.vault_load.is_loading());
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(a), "still the first vault");
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-037
#[test]
fn vault_037_a_sync_that_runs_longer_than_the_limit_ends_the_wait_with_a_message_and_keeps_the_vault() {
    let (mut app, a, c, b) = with_running_sync("sync-limit", 600);
    app.open_vault_in_background(&c, super::vault_load::After::Nothing);
    app.vault_leave_step(std::time::Instant::now() + std::time::Duration::from_secs(29));
    assert!(app.pending_leave.is_some(), "still within the limit");
    app.vault_leave_step(std::time::Instant::now() + std::time::Duration::from_secs(31));
    assert!(app.pending_leave.is_none());
    assert!(app.toasts.iter().any(|(m, _)| m.contains("sync is still running")), "{:?}", app.toasts);
    assert_eq!(app.vault.as_ref().map(|v| v.root.clone()), Some(a));
    assert!(app.busy.info(&super::vault_close::LEAVE_WAIT).is_none());
    // a sync that dies is an error of the sync, not a reason to stay
    frames_until(&mut app, "the sync did not end", |a| !a.sync.running());
    app.sync_test_job(std::time::Duration::from_millis(1), false);
    frames_until(&mut app, "the sync did not end", |a| !a.sync.running());
    assert!(matches!(&app.sync.last, Some((_, Err(_)))), "{:?}", app.sync.last);
    let _ = std::fs::remove_dir_all(&b);
}

// spec: VAULT-037
#[test]
fn vault_037_without_a_sync_the_switch_does_not_wait() {
    let (mut app, _a, c, b) = with_running_sync("sync-none", 1);
    frames_until(&mut app, "the sync did not end", |a| !a.sync.running());
    app.open_vault_in_background(&c, super::vault_load::After::Nothing);
    assert!(app.pending_leave.is_none() && app.vault_load.is_loading(), "the load starts at once");
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
